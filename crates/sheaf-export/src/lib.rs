//! PDF of a note (plan 03 §9, `docs/verified.md` §2): the Markdown is read into
//! blocks, set into lines and pages by Sheaf itself and written with krilla.
//!
//! The pages follow the editor's look: the date above the title, headings, paragraphs
//! with bold, italic, code and links, lists and checklists, quotes, code blocks,
//! tables, images, rules and page breaks (`\newpage`), page numbers at the bottom. The
//! fonts are embedded as subsets; text stays selectable and searchable, headings form
//! the outline.
//!
//! [`render`] is pure and blocks: call it off the UI thread.

mod blocks;
mod fonts;
mod layout;
mod pdf;
mod text;

use std::path::Path;

pub use fonts::{FontFile, Fonts};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the {0} font cannot be read")]
    Font(&'static str),
    #[error("the page is too small")]
    PageSize,
    #[error("the PDF cannot be written: {0}")]
    Pdf(String),
}

/// Paper size and margins, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
}

const POINTS_PER_MM: f32 = 72. / 25.4;

impl PageSetup {
    /// A4 with 20 mm margins.
    pub const A4: PageSetup = PageSetup {
        width: 210.,
        height: 297.,
        margin_top: 20.,
        margin_right: 20.,
        margin_bottom: 20.,
        margin_left: 20.,
    };

    fn size_points(&self) -> (f32, f32) {
        (self.width * POINTS_PER_MM, self.height * POINTS_PER_MM)
    }

    /// Top, right, bottom, left.
    fn margins_points(&self) -> [f32; 4] {
        [
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
        ]
        .map(|mm| mm.max(0.) * POINTS_PER_MM)
    }
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup::A4
    }
}

/// What goes with the text of a note.
#[derive(Clone, Debug, Default)]
pub struct Options<'a> {
    pub page: PageSetup,
    /// The folder relative image paths start from: the note's folder.
    pub base_dir: Option<&'a Path>,
    /// Shown small above the title, as in the editor.
    pub date: Option<String>,
}

/// A rendered PDF.
#[derive(Clone, Debug)]
pub struct Pdf {
    pub bytes: Vec<u8>,
    pub pages: usize,
}

/// Renders the Markdown `body` of a note (without frontmatter) as a PDF.
pub fn render(body: &str, fonts: &Fonts, options: &Options<'_>) -> Result<Pdf, Error> {
    let faces = text::Faces::load(fonts)?;
    let blocks = blocks::parse(body);
    let laid = layout::lay_out(
        &blocks,
        &faces,
        &options.page,
        options.base_dir,
        options.date.as_deref(),
    );
    let title = laid
        .headings
        .first()
        .map(|heading| heading.text.trim().to_owned())
        .unwrap_or_default();
    let pages = laid.pages.len();
    let bytes = pdf::write(&laid, &faces, options.page.size_points(), &title)?;
    Ok(Pdf { bytes, pages })
}
