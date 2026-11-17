//! The PDF renderer from Markdown to bytes. Text is checked with `pdftotext` where it
//! is installed (poppler); without it those checks are skipped.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use sheaf_export::{FontFile, Fonts, Options, PageSetup, render};

fn fonts() -> Fonts {
    let font = |bytes: &'static [u8]| FontFile::new(bytes, 0);
    Fonts::new(
        font(include_bytes!("../../sheaf/assets/fonts/Inter-Regular.ttf")),
        font(include_bytes!(
            "../../sheaf/assets/fonts/Inter-SemiBold.ttf"
        )),
        font(include_bytes!("../../sheaf/assets/fonts/Inter-Bold.ttf")),
        font(include_bytes!(
            "../../sheaf/assets/fonts/Inter-ExtraBold.ttf"
        )),
    )
}

const ALL_BLOCKS: &str = "Größe der Wohnung

## Räume

Ein **fetter** und *schräger* Satz mit `code`, ~~alt~~ und einem [Link](https://example.org) sowie [[Andere Notiz]].

- Punkt eins
- Punkt zwei
  - eingerückt

1. Erstens
2. Zweitens

- [ ] offene Aufgabe
- [x] erledigte Aufgabe

> Ein Zitat über Zimmer

```
fn main() {
    println!(\"hallo\");
}
```

| Raum | Fläche |
|------|-------:|
| Küche | 12 m² |
| Bad | 6 m² |

![Grünes Bild](green.png)

![fehlt](nicht-da.png)

---

\\newpage

Nach dem Umbruch
";

/// The text of `pdf`, or `None` without `pdftotext`.
fn text_of(pdf: &[u8]) -> Option<String> {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("note.pdf");
    std::fs::write(&path, pdf).expect("written");
    let output = Command::new("pdftotext")
        .arg("-layout")
        .arg(&path)
        .arg("-")
        .output()
        .ok()?;
    assert!(output.status.success(), "pdftotext failed");
    Some(String::from_utf8(output.stdout).expect("UTF-8"))
}

fn fixture_dir() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sheaf/tests/fixture/covers"
    ))
}

#[test]
fn a_short_note_is_one_page() {
    let pdf = render("Einkauf\n\nMilch, Brot\n", &fonts(), &Options::default()).expect("rendered");
    assert_eq!(pdf.pages, 1);
    assert!(pdf.bytes.starts_with(b"%PDF-"));
}

#[test]
fn every_block_kind_shows_up() {
    let options = Options {
        base_dir: Some(fixture_dir()),
        date: Some("17. November 2026, 10:12".into()),
        ..Options::default()
    };
    let pdf = render(ALL_BLOCKS, &fonts(), &options).expect("rendered");
    assert_eq!(pdf.pages, 2, "the page break starts a second page");
    let Some(text) = text_of(&pdf.bytes) else {
        eprintln!("pdftotext is not installed; text not checked");
        return;
    };
    for words in [
        "17. November 2026",
        "Größe der Wohnung",
        "Räume",
        "fetter",
        "schräger",
        "code",
        "Link",
        "Andere Notiz",
        "Punkt eins",
        "eingerückt",
        "1.",
        "Zweitens",
        "offene Aufgabe",
        "erledigte Aufgabe",
        "Ein Zitat über Zimmer",
        "println!(\"hallo\");",
        "Küche",
        "12 m²",
        "fehlt",
        "Nach dem Umbruch",
    ] {
        assert!(text.contains(words), "{words:?} missing in:\n{text}");
    }
    let squeezed = text.replace(' ', "");
    assert!(
        squeezed.contains("1/2") && squeezed.contains("2/2"),
        "{text}"
    );
    let second = text.split('\u{c}').nth(1).unwrap_or_default();
    assert!(second.contains("Nach dem Umbruch"), "{text}");
}

#[test]
fn long_notes_flow_over_pages() {
    let paragraph = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. ";
    let body = |paragraphs: usize| {
        let mut body = String::from("Lang\n\n");
        for number in 0..paragraphs {
            body.push_str(&format!("{number}: {}\n\n", paragraph.repeat(3)));
        }
        body
    };
    let fonts = fonts();
    let ten = render(&body(10), &fonts, &Options::default())
        .expect("rendered")
        .pages;
    let hundred = render(&body(100), &fonts, &Options::default())
        .expect("rendered")
        .pages;
    assert!(ten >= 2, "{ten}");
    assert!(hundred >= 4 * ten && hundred <= 10 * ten, "{ten} {hundred}");
}

#[test]
fn a_smaller_page_takes_more_pages() {
    let body = "Viel\n\n".to_owned() + &"Text in Absätzen. ".repeat(2000);
    let fonts = fonts();
    let a4 = render(&body, &fonts, &Options::default()).expect("rendered");
    let a5 = Options {
        page: PageSetup {
            width: 148.,
            height: 210.,
            ..PageSetup::A4
        },
        ..Options::default()
    };
    let a5 = render(&body, &fonts, &a5).expect("rendered");
    assert!(a5.pages > a4.pages, "{} {}", a5.pages, a4.pages);
}

#[test]
fn umlauts_emoji_and_odd_text_do_not_panic() {
    let body = "Ümlaute äöüß ÄÖÜ\n\nEmoji 🎉👩‍👩‍👧 ❤️ und 漢字 und עברית\n\n\
                Einwortohneleerzeichendasvielzulangistfüreinezeileundweiterundweiter"
        .to_owned()
        + &"x".repeat(2000)
        + "\n\n\t\tTabs\n\n|a|\n|-|\n||\n\n- \n\n> \n\n```\n```\n\n****\n";
    let pdf = render(&body, &fonts(), &Options::default()).expect("rendered");
    assert!(pdf.pages >= 1);
    if let Some(text) = text_of(&pdf.bytes) {
        assert!(text.contains("Ümlaute äöüß ÄÖÜ"), "{text}");
    }
    let empty = render("", &fonts(), &Options::default()).expect("rendered");
    assert_eq!(empty.pages, 1);
}

#[test]
fn two_hundred_pages_render_in_reasonable_time() {
    let mut body = String::from("Buch\n\n");
    let mut number = 0;
    while body.len() < 450_000 {
        number += 1;
        body.push_str(&format!(
            "## Kapitel {number}\n\nEin Absatz mit **fettem** Text, der sich über mehrere Zeilen zieht und eine Seite füllt. {}\n\n- eins\n- [x] zwei\n\n",
            "Noch mehr Worte für die Länge. ".repeat(12)
        ));
    }
    let started = Instant::now();
    let pdf = render(&body, &fonts(), &Options::default()).expect("rendered");
    let took = started.elapsed();
    assert!(pdf.pages >= 200, "{} pages", pdf.pages);
    // Unoptimised test builds are several times slower than the app.
    assert!(
        took < Duration::from_secs(60),
        "{took:?} for {} pages",
        pdf.pages
    );
    eprintln!("{} pages in {took:?}", pdf.pages);
}

#[test]
fn an_image_that_is_a_device_shows_its_placeholder() {
    let started = Instant::now();
    let pdf = render(
        "Null\n\n![Endlos](/dev/zero)\n\n![Auch](../../../../../../dev/zero)\n",
        &fonts(),
        &Options {
            base_dir: Some(fixture_dir()),
            ..Options::default()
        },
    )
    .expect("rendered");
    assert!(started.elapsed() < Duration::from_secs(10));
    let Some(text) = text_of(&pdf.bytes) else {
        return;
    };
    assert!(text.contains("Endlos") && text.contains("Auch"), "{text}");
}
