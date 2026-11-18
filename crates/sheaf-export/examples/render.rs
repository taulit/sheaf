//! Renders a Markdown file as PDF, to look at the layout:
//! `cargo run -p sheaf-export --example render -- note.md note.pdf`.
//! Images are looked up beside the Markdown file.

use std::path::PathBuf;

use sheaf_export::{FontFile, Fonts, Options, render};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        return Err("usage: render <note.md> <note.pdf>".into());
    };
    let font = |bytes: &'static [u8]| FontFile::new(bytes, 0);
    let fonts = Fonts::new(
        font(include_bytes!("../../sheaf/assets/fonts/Inter-Regular.ttf")),
        font(include_bytes!(
            "../../sheaf/assets/fonts/Inter-SemiBold.ttf"
        )),
        font(include_bytes!("../../sheaf/assets/fonts/Inter-Bold.ttf")),
        font(include_bytes!(
            "../../sheaf/assets/fonts/Inter-ExtraBold.ttf"
        )),
    )
    .with_system_fonts();
    let body = std::fs::read_to_string(&input)?;
    let options = Options {
        base_dir: input.parent(),
        date: Some("17 November 2026 at 10:12".into()),
        ..Options::default()
    };
    let pdf = render(&body, &fonts, &options)?;
    std::fs::write(&output, &pdf.bytes)?;
    println!("{} pages, {} bytes", pdf.pages, pdf.bytes.len());
    Ok(())
}
