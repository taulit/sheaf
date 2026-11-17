//! Blocks in, pages of positioned items out. The measures follow the editor (plan 04
//! "Editor"), scaled to print: points instead of pixels, a smaller body size.

use std::path::Path;

use krilla::image::Image;
use krilla::text::KrillaGlyph;

use crate::PageSetup;
use crate::blocks::{Align, Block, Item as ListItem, Span, Style};
use crate::text::{Base, FaceId, Faces, Line, break_lines, measure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// The colours of 04 on paper: the light theme over white.
pub const TEXT: Rgb = Rgb(29, 29, 31);
pub const MUTED: Rgb = Rgb(115, 115, 116);
pub const FAINT: Rgb = Rgb(165, 165, 166);
pub const ACCENT: Rgb = Rgb(53, 132, 228);
const HAIR: Rgb = Rgb(232, 232, 233);
const SURFACE: Rgb = Rgb(242, 242, 243);

const BODY: f32 = 10.5;
const LEADING: f32 = 1.55;
const TITLE: f32 = 22.;
const PARAGRAPH_GAP: f32 = 6.;
const INDENT: f32 = 18.;
const QUOTE_INDENT: f32 = 12.;
const CODE: f32 = 9.;
const CODE_PADDING: f32 = 7.;
const CELL: f32 = 9.5;
const CELL_PADDING: (f32, f32) = (4., 6.);
/// Points per pixel at 96 dpi, for the size of images.
const POINTS_PER_PIXEL: f32 = 0.75;

#[derive(Clone)]
pub enum Item {
    Text {
        x: f32,
        baseline: f32,
        face: FaceId,
        size: f32,
        color: Rgb,
        /// Slanted: Inter comes without italics.
        italic: bool,
        text: String,
        glyphs: Vec<KrillaGlyph>,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Rgb,
    },
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
        color: Rgb,
    },
    /// A checklist box: a ring, or ticked a filled circle with a tick.
    Check {
        x: f32,
        y: f32,
        radius: f32,
        ticked: bool,
    },
    /// The bullet of a list item.
    Dot { x: f32, y: f32, radius: f32 },
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        image: Image,
    },
}

pub struct Link {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub uri: String,
}

#[derive(Default)]
pub struct Page {
    pub items: Vec<Item>,
    pub links: Vec<Link>,
}

/// A heading, for the outline of the document.
pub struct Heading {
    pub text: String,
    pub page: usize,
    pub y: f32,
}

pub struct Laid {
    pub pages: Vec<Page>,
    pub headings: Vec<Heading>,
}

/// Lays out `blocks`. The first paragraph or heading is the title; `date` stands
/// above it. Images are read from `base_dir`.
pub fn lay_out(
    blocks: &[Block],
    faces: &Faces,
    setup: &PageSetup,
    base_dir: Option<&Path>,
    date: Option<&str>,
) -> Laid {
    let (width, height) = setup.size_points();
    let [top, right, bottom, left] = setup.margins_points();
    let mut flow = Flow {
        faces,
        base_dir,
        top,
        bottom: height - bottom,
        y: top,
        pages: vec![Page::default()],
        decor: Vec::new(),
        marker: None,
        headings: Vec::new(),
    };
    let x = left;
    let content = (width - left - right).max(40.);
    if let Some(date) = date {
        let base = Base {
            size: 8.5,
            face: faces.regular,
            leading: 1.4,
        };
        let spans = [plain(date)];
        for line in break_lines(faces, &base, &spans, content) {
            let offset = (content - line.width) / 2.;
            flow.line(&line, x + offset, &spans, FAINT);
        }
        flow.gap(8.);
    }
    let mut rest = blocks;
    if let Some((first, after)) = blocks.split_first()
        && let Block::Paragraph(spans) | Block::Heading { spans, .. } = first
    {
        let base = Base {
            size: TITLE,
            face: faces.extra_bold,
            leading: 1.25,
        };
        flow.heading(spans, &base, x, content);
        flow.gap(10.);
        rest = after;
    }
    flow.blocks(rest, x, content, TEXT, PARAGRAPH_GAP);
    let mut pages = flow.pages;
    if pages.len() > 1 && pages.last().is_some_and(|page| page.items.is_empty()) {
        pages.pop();
    }
    number(&mut pages, faces, width, height, bottom);
    Laid {
        pages,
        headings: flow.headings,
    }
}

/// "3 / 12" centred in the bottom margin of every page.
fn number(pages: &mut [Page], faces: &Faces, width: f32, height: f32, margin: f32) {
    let total = pages.len();
    let base = Base {
        size: 8.,
        face: faces.regular,
        leading: 1.,
    };
    for (index, page) in pages.iter_mut().enumerate() {
        let spans = [plain(&format!("{} / {total}", index + 1))];
        let lines = break_lines(faces, &base, &spans, width);
        let Some(line) = lines.first() else { continue };
        let x = (width - line.width) / 2.;
        let baseline = height - margin / 2.;
        for frag in &line.frags {
            page.items.push(Item::Text {
                x: x + frag.x,
                baseline,
                face: frag.face,
                size: frag.size,
                color: FAINT,
                italic: false,
                text: frag.text.clone(),
                glyphs: frag.glyphs.clone(),
            });
        }
    }
}

fn plain(text: &str) -> Span {
    Span {
        text: text.to_owned(),
        style: Style::default(),
    }
}

/// What every line placed while it is active also gets: the bar of a quote, the
/// background of a code block.
#[derive(Clone, Copy)]
enum Decor {
    Bar { x: f32 },
    Fill { x: f32, width: f32 },
}

/// What goes in front of the first line of a list item.
#[derive(Clone, Copy)]
enum Marker {
    Bullet { x: f32 },
    Number { x: f32, number: u64 },
    Check { x: f32, ticked: bool },
}

struct Flow<'a> {
    faces: &'a Faces,
    base_dir: Option<&'a Path>,
    top: f32,
    bottom: f32,
    y: f32,
    pages: Vec<Page>,
    decor: Vec<Decor>,
    marker: Option<Marker>,
    headings: Vec<Heading>,
}

impl Flow<'_> {
    fn page(&mut self) -> &mut Page {
        if self.pages.is_empty() {
            self.pages.push(Page::default());
        }
        let last = self.pages.len() - 1;
        &mut self.pages[last]
    }

    fn at_top(&self) -> bool {
        self.y <= self.top + 0.01
    }

    fn new_page(&mut self) {
        self.pages.push(Page::default());
        self.y = self.top;
    }

    /// Starts a new page unless `height` fits on this one.
    fn need(&mut self, height: f32) {
        if self.y + height > self.bottom && !self.at_top() {
            self.new_page();
        }
    }

    /// Takes `height` from the page, on a new one if it does not fit, and returns its
    /// top. A block taller than a page is cut at the bottom edge.
    fn take(&mut self, height: f32) -> f32 {
        self.need(height);
        let y = self.y;
        self.decorate(y, height);
        self.y += height;
        y
    }

    /// Space between blocks; none at the top of a page.
    fn gap(&mut self, height: f32) {
        if self.at_top() {
            return;
        }
        let height = height.min((self.bottom - self.y).max(0.));
        let y = self.y;
        self.decorate(y, height);
        self.y += height;
    }

    fn decorate(&mut self, y: f32, height: f32) {
        let decor = self.decor.clone();
        let page = self.page();
        for decor in decor {
            page.items.push(match decor {
                Decor::Bar { x } => Item::Rect {
                    x,
                    y,
                    width: 2.5,
                    height,
                    color: ACCENT,
                },
                Decor::Fill { x, width } => Item::Rect {
                    x,
                    y,
                    width,
                    height,
                    color: SURFACE,
                },
            });
        }
    }

    fn blocks(&mut self, blocks: &[Block], x: f32, width: f32, color: Rgb, gap: f32) {
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 {
                self.gap(gap);
            }
            self.block(block, x, width, color, gap);
        }
    }

    fn block(&mut self, block: &Block, x: f32, width: f32, color: Rgb, gap: f32) {
        let faces = self.faces;
        match block {
            Block::Paragraph(spans) => {
                let base = Base {
                    size: BODY,
                    face: faces.regular,
                    leading: LEADING,
                };
                for line in break_lines(faces, &base, spans, width) {
                    self.line(&line, x, spans, color);
                }
            }
            Block::Heading { level, spans } => {
                let (size, face) = match level {
                    1 => (18., faces.bold),
                    2 => (14., faces.bold),
                    3 => (12., faces.semibold),
                    _ => (BODY, faces.semibold),
                };
                self.gap(size * 0.6);
                let base = Base {
                    size,
                    face,
                    leading: 1.3,
                };
                self.heading(spans, &base, x, width);
            }
            Block::List { start, items } => self.list(*start, items, x, width, color),
            Block::Quote(blocks) => {
                self.decor.push(Decor::Bar { x });
                self.blocks(blocks, x + QUOTE_INDENT, width - QUOTE_INDENT, color, gap);
                self.decor.pop();
            }
            Block::Code(code) => self.code(code, x, width),
            Block::Table {
                align,
                header,
                rows,
            } => self.table(align, header, rows, x, width),
            Block::Image { src, alt } => self.image(src, alt, x, width),
            Block::Rule => {
                let y = self.take(14.) + 7.;
                self.page().items.push(Item::Line {
                    from: (x, y),
                    to: (x + width, y),
                    width: 0.75,
                    color: HAIR,
                });
            }
            Block::PageBreak => {
                if !self.at_top() {
                    self.new_page();
                }
            }
        }
    }

    /// A heading or the title: kept together with the line after it, and noted for
    /// the outline.
    fn heading(&mut self, spans: &[Span], base: &Base, x: f32, width: f32) {
        let lines = break_lines(self.faces, base, spans, width);
        let height: f32 = lines.iter().map(|line| line.height).sum();
        self.need(height + BODY * LEADING);
        self.headings.push(Heading {
            text: spans.iter().map(|span| span.text.as_str()).collect(),
            page: self.pages.len() - 1,
            y: self.y,
        });
        for line in &lines {
            self.line(line, x, spans, TEXT);
        }
    }

    /// Places a line of text at `x` below what is there.
    fn line(&mut self, line: &Line, x: f32, spans: &[Span], color: Rgb) {
        let y = self.take(line.height);
        let baseline = y + line.baseline;
        if let Some(marker) = self.marker.take() {
            let size = line.frags.first().map_or(BODY, |frag| frag.size);
            self.draw_marker(marker, baseline, size);
        }
        self.put(line, x, y, spans, color);
    }

    /// Draws a line of text whose top is at `y`.
    fn put(&mut self, line: &Line, x: f32, y: f32, spans: &[Span], color: Rgb) {
        let baseline = y + line.baseline;
        let page = self.page();
        for frag in &line.frags {
            let style = &spans[frag.span].style;
            let left = x + frag.x;
            if style.code {
                page.items.push(Item::Rect {
                    x: left - 1.,
                    y: baseline - frag.size * 0.95,
                    width: frag.width + 2.,
                    height: frag.size * 1.3,
                    color: SURFACE,
                });
            }
            let color = if style.link.is_some() || style.wiki {
                ACCENT
            } else {
                color
            };
            page.items.push(Item::Text {
                x: left,
                baseline,
                face: frag.face,
                size: frag.size,
                color,
                italic: style.italic,
                text: frag.text.clone(),
                glyphs: frag.glyphs.clone(),
            });
            if style.strike {
                let y = baseline - frag.size * 0.3;
                page.items.push(Item::Line {
                    from: (left, y),
                    to: (left + frag.width, y),
                    width: frag.size * 0.07,
                    color,
                });
            }
            if let Some(uri) = &style.link {
                page.links.push(Link {
                    x: left,
                    y,
                    width: frag.width,
                    height: line.height,
                    uri: uri.clone(),
                });
            }
        }
    }

    fn draw_marker(&mut self, marker: Marker, baseline: f32, size: f32) {
        let middle = baseline - size * 0.36;
        match marker {
            Marker::Bullet { x } => self.page().items.push(Item::Dot {
                x: x - INDENT / 2.,
                y: middle,
                radius: size * 0.17,
            }),
            Marker::Number { x, number } => {
                let base = Base {
                    size,
                    face: self.faces.regular,
                    leading: 1.,
                };
                let spans = [plain(&format!("{number}."))];
                let lines = break_lines(self.faces, &base, &spans, INDENT * 4.);
                if let Some(line) = lines.first() {
                    let left = x - 4. - line.width;
                    for frag in &line.frags {
                        self.page().items.push(Item::Text {
                            x: left + frag.x,
                            baseline,
                            face: frag.face,
                            size: frag.size,
                            color: TEXT,
                            italic: false,
                            text: frag.text.clone(),
                            glyphs: frag.glyphs.clone(),
                        });
                    }
                }
            }
            Marker::Check { x, ticked } => self.page().items.push(Item::Check {
                x: x - INDENT / 2.,
                y: middle,
                radius: size * 0.48,
                ticked,
            }),
        }
    }

    fn list(&mut self, start: Option<u64>, items: &[ListItem], x: f32, width: f32, color: Rgb) {
        let inner = x + INDENT;
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.gap(2.);
            }
            self.marker = Some(match (item.task, start) {
                (Some(ticked), _) => Marker::Check { x: inner, ticked },
                (None, Some(first)) => Marker::Number {
                    x: inner,
                    number: first + index as u64,
                },
                (None, None) => Marker::Bullet { x: inner },
            });
            let color = if item.task == Some(true) {
                MUTED
            } else {
                color
            };
            self.blocks(&item.blocks, inner, width - INDENT, color, 2.);
            if let Some(marker) = self.marker.take() {
                // An empty item still shows its marker.
                let y = self.take(BODY * LEADING);
                self.draw_marker(marker, y + BODY * 1.1, BODY);
            }
        }
    }

    fn code(&mut self, code: &str, x: f32, width: f32) {
        let base = Base {
            size: CODE,
            face: self.faces.mono,
            leading: 1.45,
        };
        let spans = [plain(code)];
        let lines = break_lines(self.faces, &base, &spans, width - 2. * CODE_PADDING);
        let first = lines.first().map_or(0., |line| line.height);
        self.need(CODE_PADDING + first);
        self.decor.push(Decor::Fill { x, width });
        self.take(CODE_PADDING);
        for line in &lines {
            self.line(line, x + CODE_PADDING, &spans, TEXT);
        }
        self.take(CODE_PADDING);
        self.decor.pop();
    }

    fn table(
        &mut self,
        align: &[Align],
        header: &[Vec<Span>],
        rows: &[Vec<Vec<Span>>],
        x: f32,
        width: f32,
    ) {
        let faces = self.faces;
        let body = Base {
            size: CELL,
            face: faces.regular,
            leading: 1.35,
        };
        let head = Base {
            face: faces.bold,
            ..body
        };
        let all: Vec<(&[Vec<Span>], &Base)> = std::iter::once((header, &head))
            .filter(|(row, _)| !row.is_empty())
            .chain(rows.iter().map(|row| (row.as_slice(), &body)))
            .collect();
        let columns = all.iter().map(|(row, _)| row.len()).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let padding = 2. * CELL_PADDING.1;
        let mut natural = vec![padding; columns];
        let mut least = vec![padding; columns];
        for (row, base) in &all {
            for (column, cell) in row.iter().enumerate() {
                let (line, word) = measure(faces, base, cell);
                natural[column] = natural[column].max(line + padding);
                least[column] = least[column].max(word.min(width / 3.) + padding);
            }
        }
        let widths = column_widths(&natural, &least, width);
        for (number, (row, base)) in all.iter().enumerate() {
            let cells: Vec<Vec<Line>> = (0..columns)
                .map(|column| {
                    let cell = row.get(column).map_or(&[][..], Vec::as_slice);
                    break_lines(faces, base, cell, widths[column] - padding)
                })
                .collect();
            let height = cells
                .iter()
                .map(|lines| lines.iter().map(|line| line.height).sum::<f32>())
                .fold(0., f32::max)
                + 2. * CELL_PADDING.0;
            let y = self.take(height);
            let total: f32 = widths.iter().sum();
            if number == 0 && !header.is_empty() {
                self.page().items.push(Item::Rect {
                    x,
                    y,
                    width: total,
                    height,
                    color: SURFACE,
                });
            }
            let mut left = x;
            for (column, lines) in cells.iter().enumerate() {
                let spans = row.get(column).map_or(&[][..], Vec::as_slice);
                let mut top = y + CELL_PADDING.0;
                for line in lines {
                    let room = widths[column] - padding - line.width;
                    let offset = match align.get(column) {
                        Some(Align::Right) => room,
                        Some(Align::Center) => room / 2.,
                        _ => 0.,
                    };
                    self.put(line, left + CELL_PADDING.1 + offset, top, spans, TEXT);
                    top += line.height;
                }
                left += widths[column];
            }
            let page = self.page();
            let hair = |from, to| Item::Line {
                from,
                to,
                width: 0.75,
                color: HAIR,
            };
            page.items.push(hair((x, y), (x + total, y)));
            page.items
                .push(hair((x, y + height), (x + total, y + height)));
            let mut left = x;
            for column in 0..=columns {
                page.items.push(hair((left, y), (left, y + height)));
                left += widths.get(column).copied().unwrap_or(0.);
            }
        }
    }

    fn image(&mut self, src: &str, alt: &str, x: f32, width: f32) {
        let Some(image) = self.load_image(src) else {
            let text = if alt.is_empty() { src } else { alt };
            let spans = [Span {
                text: text.to_owned(),
                style: Style {
                    italic: true,
                    ..Style::default()
                },
            }];
            let base = Base {
                size: BODY,
                face: self.faces.regular,
                leading: LEADING,
            };
            for line in break_lines(self.faces, &base, &spans, width) {
                self.line(&line, x, &spans, MUTED);
            }
            return;
        };
        let (pixels_wide, pixels_high) = image.size();
        let mut shown = (
            pixels_wide as f32 * POINTS_PER_PIXEL,
            pixels_high as f32 * POINTS_PER_PIXEL,
        );
        let room = (width, (self.bottom - self.top) * 0.9);
        let scale = (room.0 / shown.0).min(room.1 / shown.1).min(1.);
        shown = (shown.0 * scale, shown.1 * scale);
        let y = self.take(shown.1);
        self.marker = None;
        self.page().items.push(Item::Image {
            x,
            y,
            width: shown.0,
            height: shown.1,
            image,
        });
    }

    /// The image at `src`, relative to the note's folder; `None` for addresses on
    /// the web and files that cannot be read as PNG, JPEG, GIF or WebP.
    fn load_image(&self, src: &str) -> Option<Image> {
        if src.contains("://") {
            return None;
        }
        let path = Path::new(&percent_decode(src)).to_owned();
        let path = match self.base_dir {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path,
        };
        let data = std::fs::read(path).ok()?;
        let kind = data.get(..12)?.to_vec();
        let data: krilla::Data = data.into();
        match kind.as_slice() {
            [0x89, b'P', b'N', b'G', ..] => Image::from_png(data, true).ok(),
            [0xFF, 0xD8, ..] => Image::from_jpeg(data, true).ok(),
            [b'G', b'I', b'F', b'8', ..] => Image::from_gif(data, true).ok(),
            [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P'] => {
                Image::from_webp(data, true).ok()
            }
            _ => None,
        }
        .filter(|image| image.size().0 > 0 && image.size().1 > 0)
    }
}

/// Widths of the columns: as wide as their content if that fits, else shrunk towards
/// their widest word, and below that in proportion.
fn column_widths(natural: &[f32], least: &[f32], width: f32) -> Vec<f32> {
    let wanted: f32 = natural.iter().sum();
    if wanted <= width {
        return natural.to_vec();
    }
    let floor: f32 = least.iter().sum();
    if floor >= width {
        return least.iter().map(|least| least * width / floor).collect();
    }
    let share = (width - floor) / (wanted - floor);
    natural
        .iter()
        .zip(least)
        .map(|(natural, least)| least + (natural - least) * share)
        .collect()
}

/// `%20` and friends in a link target, as files on disk do not have them.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                index += 3;
            }
            None => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_shrink_towards_their_widest_word() {
        assert_eq!(column_widths(&[50., 50.], &[10., 10.], 200.), [50., 50.]);
        let widths = column_widths(&[300., 100.], &[50., 50.], 200.);
        assert!(
            (widths.iter().sum::<f32>() - 200.).abs() < 0.01,
            "{widths:?}"
        );
        assert!(widths[0] > widths[1]);
        let widths = column_widths(&[300., 300.], &[150., 250.], 200.);
        assert!(
            (widths.iter().sum::<f32>() - 200.).abs() < 0.01,
            "{widths:?}"
        );
    }

    #[test]
    fn link_targets_lose_their_escapes() {
        assert_eq!(percent_decode("a%20b%C3%BC.png"), "a bü.png");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
