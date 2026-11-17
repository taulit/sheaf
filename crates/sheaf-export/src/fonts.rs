//! The fonts a PDF is set in. All of them are embedded, as subsets, so the file looks
//! the same everywhere.

use std::fmt;

use krilla::Data;

/// One font file, or one face of a collection.
#[derive(Clone)]
pub struct FontFile {
    data: Data,
    index: u32,
}

impl FontFile {
    /// A face of a font file in memory; `index` picks the face of a collection.
    pub fn new(data: impl Into<Data>, index: u32) -> Self {
        FontFile {
            data: data.into(),
            index,
        }
    }

    pub(crate) fn data(&self) -> &Data {
        &self.data
    }

    pub(crate) fn index(&self) -> u32 {
        self.index
    }
}

impl fmt::Debug for FontFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontFile")
            .field("bytes", &self.data.as_ref().len())
            .field("index", &self.index)
            .finish()
    }
}

/// The text faces, a monospaced face for code and fallbacks for characters the text
/// faces lack, such as emoji.
#[derive(Clone, Debug)]
pub struct Fonts {
    pub regular: FontFile,
    pub semibold: FontFile,
    pub bold: FontFile,
    /// The title.
    pub extra_bold: FontFile,
    /// Code; without it code is set in the regular face.
    pub mono: Option<FontFile>,
    /// Tried in order for characters the face of the text does not have.
    pub fallbacks: Vec<FontFile>,
}

impl Fonts {
    /// The four weights of the text face, without a code face or fallbacks.
    pub fn new(
        regular: FontFile,
        semibold: FontFile,
        bold: FontFile,
        extra_bold: FontFile,
    ) -> Self {
        Fonts {
            regular,
            semibold,
            bold,
            extra_bold,
            mono: None,
            fallbacks: Vec::new(),
        }
    }

    /// Adds a monospaced face and fallbacks from the fonts installed on this system:
    /// the first of the usual monospaced families, then emoji and wide coverage faces.
    /// Blocks while it scans the font folders.
    pub fn with_system_fonts(mut self) -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let load = |names: &[&str]| -> Option<FontFile> {
            let families: Vec<fontdb::Family<'_>> = names
                .iter()
                .map(|name| fontdb::Family::Name(name))
                .collect();
            let id = db.query(&fontdb::Query {
                families: &families,
                ..fontdb::Query::default()
            })?;
            db.with_face_data(id, |data, index| FontFile::new(data.to_vec(), index))
        };
        if self.mono.is_none() {
            self.mono = load(&[
                "Adwaita Mono",
                "Source Code Pro",
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Liberation Mono",
            ]);
        }
        for names in [
            &["Noto Color Emoji", "Twemoji", "JoyPixels"][..],
            &["Noto Sans CJK SC", "Noto Sans CJK JP", "Source Han Sans"][..],
            &["Noto Sans", "DejaVu Sans"][..],
        ] {
            self.fallbacks.extend(load(names));
        }
        self
    }
}
