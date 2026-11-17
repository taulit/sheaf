//! The cover images of cards and list rows. A cover is decoded off the UI thread and
//! scaled down to what a card draws at most, then kept in a small cache of the most
//! recently drawn ones, by path: twice as many as the last frame drew, a few screens
//! at least, and never one the last frame drew. A cover that is drawn is looked at
//! again on disk every few seconds: a replaced file shows its new image.
//!
//! [`Covers`] is the cache itself; the gallery drives it (`GalleryView::cover`) and
//! drops the images that fall out of it from the GPU.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::RenderImage;
use image::imageops::FilterType;
use image::{Frame, ImageReader};

/// How many covers are kept at least: a few screens of cards.
pub const MIN_CAPACITY: usize = 48;
/// How often a cover that is drawn is looked at again on disk.
pub const RECHECK: Duration = Duration::from_secs(2);
/// The size a cover is decoded at, at most, in device pixels: twice what a card draws,
/// for a screen of scale 2. A card is narrower than two of its minimum width and a gap
/// (416), its cover is 70 high.
pub const MAX_SIZE: (u32, u32) = (832, 140);

/// What the gallery has to do for a cover it asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wanted {
    /// Decode the file.
    Load,
    /// Look at its modification time; see [`Covers::checked`].
    Check,
    Nothing,
}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    /// The modification time of the file the image was decoded from.
    modified: Option<SystemTime>,
    /// `None` while it is decoded, or if the file is no image.
    image: Option<Arc<RenderImage>>,
    busy: bool,
    checked: Instant,
    /// The frame that last drew it.
    drawn: u64,
}

/// The decoded covers, the least recently drawn first.
#[derive(Debug, Default)]
pub struct Covers {
    entries: Vec<Entry>,
    /// Counts the frames; see [`next_frame`](Self::next_frame).
    frame: u64,
}

impl Covers {
    /// A new frame starts drawing: the covers [`get`](Self::get) asks for from now on
    /// are the ones it draws.
    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    /// The cover at `path` as last decoded, and what to do for it. Counts as drawn.
    pub fn get(&mut self, path: &Path, now: Instant) -> (Option<Arc<RenderImage>>, Wanted) {
        let frame = self.frame;
        let Some(index) = self.entries.iter().position(|entry| entry.path == path) else {
            self.entries.push(Entry {
                path: path.to_owned(),
                modified: None,
                image: None,
                busy: true,
                checked: now,
                drawn: frame,
            });
            return (None, Wanted::Load);
        };
        let mut entry = self.entries.remove(index);
        entry.drawn = frame;
        let wanted = if !entry.busy && now.duration_since(entry.checked) >= RECHECK {
            entry.busy = true;
            entry.checked = now;
            Wanted::Check
        } else {
            Wanted::Nothing
        };
        let image = entry.image.clone();
        self.entries.push(entry);
        (image, wanted)
    }

    /// The file at `path` was decoded: `image` (`None` if it is no image) from the file
    /// as it was at `modified`. Returns the images that fell out of the cache.
    pub fn loaded(
        &mut self,
        path: &Path,
        modified: Option<SystemTime>,
        image: Option<Arc<RenderImage>>,
    ) -> Vec<Arc<RenderImage>> {
        let mut dropped = Vec::new();
        match self.entries.iter_mut().find(|entry| entry.path == path) {
            Some(entry) => {
                dropped.extend(std::mem::replace(&mut entry.image, image));
                entry.modified = modified;
                entry.busy = false;
            }
            None => dropped.extend(image),
        }
        let frame = self.frame;
        let drawn = self
            .entries
            .iter()
            .filter(|entry| entry.drawn == frame)
            .count();
        let capacity = (2 * drawn).max(MIN_CAPACITY);
        while self.entries.len() > capacity {
            let Some(index) = self
                .entries
                .iter()
                .position(|entry| !entry.busy && entry.drawn != frame)
            else {
                break;
            };
            dropped.extend(self.entries.remove(index).image);
        }
        dropped
    }

    /// The file at `path` was looked at and has the modification time `modified`.
    /// `true` if that is not the one its image was decoded from: decode it again.
    pub fn checked(&mut self, path: &Path, modified: Option<SystemTime>) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.path == path) else {
            return false;
        };
        let changed = entry.modified != modified;
        entry.busy = changed;
        changed
    }
}

/// The modification time of the file at `path`, `None` if it cannot be read.
pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Reads the image at `path`, scaled down to cover [`MAX_SIZE`] at most, with the
/// modification time of the file it read. Blocks.
pub fn decode(path: &Path) -> (Option<SystemTime>, Option<Arc<RenderImage>>) {
    let modified = modified(path);
    let decoded = ImageReader::open(path)
        .map_err(image::ImageError::IoError)
        .and_then(|reader| {
            reader
                .with_guessed_format()
                .map_err(image::ImageError::IoError)
        })
        .and_then(|reader| reader.decode());
    let image = match decoded {
        Ok(image) => image,
        Err(err) => {
            log::debug!("no cover from {}: {err}", path.display());
            return (modified, None);
        }
    };
    let (width, height) = (image.width().max(1), image.height().max(1));
    let scale = (MAX_SIZE.0 as f32 / width as f32)
        .max(MAX_SIZE.1 as f32 / height as f32)
        .min(1.0);
    let image = if scale < 1.0 {
        let scaled = |side: u32| ((side as f32 * scale).round() as u32).max(1);
        image.resize_exact(scaled(width), scaled(height), FilterType::Triangle)
    } else {
        image
    };
    // GPUI takes BGRA.
    let mut pixels = image.into_rgba8();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    (
        modified,
        Some(Arc::new(RenderImage::new(vec![Frame::new(pixels)]))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(1, 1))]))
    }

    #[test]
    fn the_cache_keeps_the_most_recently_drawn() {
        let mut covers = Covers::default();
        let now = Instant::now();
        let path = |index: usize| PathBuf::from(format!("/covers/{index}.png"));
        for index in 0..MIN_CAPACITY + 10 {
            // A frame of its own for each card, as while scrolling.
            covers.next_frame();
            assert_eq!(covers.get(&path(index), now), (None, Wanted::Load));
            // Drawn again while it loads: no second load.
            assert_eq!(covers.get(&path(index), now).1, Wanted::Nothing);
            // The first one stays drawn, so it stays.
            covers.get(&path(0), now);
            let dropped = covers.loaded(&path(index), None, Some(image()));
            assert_eq!(dropped.len(), usize::from(index >= MIN_CAPACITY));
            assert!(covers.entries.len() <= MIN_CAPACITY);
            assert!(covers.get(&path(0), now).0.is_some());
        }
        assert_eq!(covers.entries.len(), MIN_CAPACITY);
        assert!(covers.get(&path(1), now).0.is_none(), "fell out");
    }

    #[test]
    fn more_covers_on_screen_than_the_minimum_settle() {
        let mut covers = Covers::default();
        let now = Instant::now();
        let path = |index: usize| PathBuf::from(format!("/covers/{index}.png"));
        let on_screen = MIN_CAPACITY + 8;
        for frame in 0..5 {
            covers.next_frame();
            let wanted: Vec<usize> = (0..on_screen)
                .filter(|index| covers.get(&path(*index), now).1 == Wanted::Load)
                .collect();
            assert_eq!(wanted.is_empty(), frame > 0, "frame {frame}: {wanted:?}");
            for index in wanted {
                assert!(
                    covers.loaded(&path(index), None, Some(image())).is_empty(),
                    "a cover on screen is never dropped"
                );
            }
        }
        assert!((0..on_screen).all(|index| covers.get(&path(index), now).0.is_some()));
    }

    #[test]
    fn a_changed_file_is_decoded_again() {
        let mut covers = Covers::default();
        let path = Path::new("/covers/a.png");
        let then = Instant::now();
        let first = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        covers.get(path, then);
        assert!(covers.loaded(path, Some(first), Some(image())).is_empty());

        assert_eq!(covers.get(path, then + RECHECK / 2).1, Wanted::Nothing);
        let later = then + RECHECK;
        assert_eq!(covers.get(path, later).1, Wanted::Check);
        assert_eq!(
            covers.get(path, later).1,
            Wanted::Nothing,
            "one look at a time"
        );
        assert!(!covers.checked(path, Some(first)));
        assert_eq!(covers.get(path, later + RECHECK).1, Wanted::Check);
        assert!(covers.checked(path, Some(first + Duration::from_secs(1))));
        let replaced = covers.loaded(path, Some(first + Duration::from_secs(1)), Some(image()));
        assert_eq!(replaced.len(), 1, "the old image leaves the GPU");
    }

    #[test]
    fn covers_are_decoded_small() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("big.png");
        let mut big = RgbaImage::new(2000, 1500);
        big.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        big.save(&path).expect("a PNG");

        let (modified, image) = decode(&path);
        assert!(modified.is_some());
        let size = image.expect("an image").size(0);
        assert_eq!((size.width.0, size.height.0), (MAX_SIZE.0 as i32, 624));

        std::fs::write(&path, "no image").expect("a file");
        assert!(decode(&path).1.is_none());
    }
}
