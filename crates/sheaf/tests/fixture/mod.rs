//! What the app tests and the screenshots share: a seeded notes folder in a temporary
//! directory, a fixed "now", and the app set up the way `app::run` does it, without a
//! bus, a display or the user's files.

// Each test binary uses its own part of these helpers.
#![allow(dead_code, clippy::unwrap_used)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui::{App, Entity};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};
use sheaf::app::actions;
use sheaf::app::library::Library;
use sheaf::app::theme::{self, Appearance, Overrides};
use tempfile::TempDir;

/// The moment the fixture pretends it is: Monday, 16 November 2026, 13:24 in Berlin.
pub const NOW: &str = "2026-11-16T13:24:00+01:00";

pub fn now() -> ZonedDateTime {
    let time: Timestamp = NOW.parse().unwrap();
    time.to_zoned_datetime(TimeZone::fixed(jiff::tz::offset(1)))
}

/// One seeded note: path, last modification relative to [`NOW`] in minutes, text.
struct Seed {
    path: &'static str,
    minutes_ago: u64,
    text: &'static str,
}

const HOUR: u64 = 60;
const DAY: u64 = 24 * HOUR;

/// A notes folder as someone might have it after a few weeks: German and English,
/// pinned notes, checklists, a table, tags, wiki links, an image, nested folders, and
/// one file another editor wrote (no frontmatter).
const SEEDS: &[Seed] = &[
    Seed {
        path: "Privat/Einkauf.md",
        minutes_ago: 2 * HOUR + 22,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R01\ncreated: 2026-11-10T08:00:00+01:00\npinned: true\n---\n\
Einkauf\n\n- [ ] Milch\n- [ ] Geschenk für Jukek\n- [x] Kaffeebohnen\n- [ ] Backpapier\n",
    },
    Seed {
        path: "Arbeit/Ausschreibung – offene Fragen.md",
        minutes_ago: 3 * HOUR + 44,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R02\ncreated: 2026-11-09T09:40:00+01:00\npinned: true\n---\n\
Ausschreibung – offene Fragen\n\nAbgabe am Freitag. Stand nach dem Call mit Frau K.:\n\n## Offen\n\n\
- [ ] Kostenschätzung finalisieren\n- [ ] Kapitel 4 gegenlesen lassen\n- [x] Referenzen anfragen\n\n## Kosten\n\n\
| Posten | Betrag |\n|---|---:|\n| Konzept | 4.800 € |\n| Umsetzung | 18.200 € |\n| Betrieb (12 Mon.) | 3.600 € |\n\n\
#ausschreibung\n",
    },
    Seed {
        path: "Arbeit/Notch-Ideen.md",
        minutes_ago: DAY + 3 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R03\ncreated: 2026-11-08T17:05:00+01:00\n---\n\
Notch-Ideen\n\nWas die Notch können soll, ohne zu überladen:\n\n- Hover deutet die Notch an\n\
- Nur zeigen, was gerade läuft\n- Module: Jetzt, Ausblick oder Dock\n\n\
Notizen als eigene App, siehe [[Notch-Konzept]].\n",
    },
    Seed {
        path: "Privat/Geschenkideen Jukek.md",
        minutes_ago: 3 * DAY + 2 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R04\ncreated: 2026-11-05T19:30:00+01:00\n---\n\
Geschenkideen Jukek\n\n![Skizze](_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R04/skizze.png)\n\n\
- Konzertkarten\n- Gutes Buch über Architektur\n- Siebträger-Kurs\n",
    },
    Seed {
        path: "Privat/Rezepte/Shakshuka.md",
        minutes_ago: 4 * DAY + HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R05\ncreated: 2026-10-28T18:10:00+01:00\n---\n\
Shakshuka\n\nFür 2 Personen, 25 Minuten.\n\n- [ ] 2 Paprika, 1 Zwiebel\n- [ ] 1 Dose Tomaten\n\
- [ ] 4 Eier, Feta, Kreuzkümmel\n\n#rezepte\n",
    },
    Seed {
        path: "Arbeit/Sprint Review 12.11..md",
        minutes_ago: 4 * DAY + 5 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R06\ncreated: 2026-11-12T15:00:00+01:00\n---\n\
Sprint Review 12.11.\n\nDemo lief gut. Feedback:\n\n- Ladezeiten im Dashboard\n- Export als CSV gewünscht\n\n\
Nächster Termin Do 15:00.\n",
    },
    Seed {
        path: "Privat/Lesestoff.md",
        minutes_ago: 8 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R07\ncreated: 2026-11-01T21:15:00+01:00\n---\n\
Lesestoff\n\n[GNOME Human Interface Guidelines](https://developer.gnome.org/hig/)\n\n\
[libadwaita: Adaptive Layouts](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/adaptive-layouts.html)\n\n\
„The Design of Everyday Things“, Kapitel 3\n",
    },
    Seed {
        path: "Work/Project plan.md",
        minutes_ago: 9 * DAY + 4 * HOUR,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R08\ncreated: 2026-10-30T10:00:00+01:00\n---\n\
# Project plan\n\nKickoff next week. Owner: ==Maya==. #project\n\n## Milestones\n\n\
1. Write the brief\n2. Book the venue\n3. Send invitations\n\n- [x] Budget approved\n- [ ] Agenda drafted\n",
    },
    Seed {
        path: "Work/Meeting notes 2026-11-03.md",
        minutes_ago: 13 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R09\ncreated: 2026-11-03T14:00:00+01:00\n---\n\
Meeting notes 2026-11-03\n\n> Ship the smallest thing that is useful.\n\n\
Decisions: weekly demo on Thursdays, see [[Project plan]].\n\n```sh\ncargo test --workspace\n```\n",
    },
    Seed {
        path: "Reading list.md",
        minutes_ago: 15 * DAY,
        text: "Reading list\n\nThe Overstory and Braiding Sweetgrass. #books\n",
    },
    Seed {
        path: "Ideen für den Garten.md",
        minutes_ago: 20 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R11\ncreated: 2026-10-20T11:00:00+02:00\ntags: [garten]\n---\n\
Ideen für den Garten\n\nTomaten und Kräuter pflanzen. Hochbeet an die Südseite.\n",
    },
    Seed {
        path: "Privat/Reise nach Lissabon.md",
        minutes_ago: 27 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R12\ncreated: 2026-10-12T12:00:00+02:00\n---\n\
Reise nach Lissabon\n\n| Tag | Plan |\n|---|---|\n| Fr | Ankunft, Alfama |\n| Sa | Belém, Pastéis |\n| So | Sintra |\n\n\
- [ ] Flüge buchen\n- [ ] Unterkunft\n\n#urlaub\n",
    },
    Seed {
        path: "Schnellnotizen/Idee Hover-Vorschau.md",
        minutes_ago: 40 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R13\ncreated: 2026-10-07T08:45:00+02:00\n---\n\
Idee: Hover-Vorschau zeigt auch die letzte Notiz?\n",
    },
    Seed {
        path: "Archive/Tax return 2025.md",
        minutes_ago: 120 * DAY,
        text: "---\nid: 01JD3K8Z6Q4X7V9B2M5N8P0R14\ncreated: 2026-05-02T09:00:00+02:00\n---\n\
Tax return 2025\n\nSubmitted on 2 May. Reference number in the letter from the tax office.\n\n\
- [x] Collect receipts\n- [x] Submit\n",
    },
];

/// A 1×1 PNG, so that the image link of the fixture points at a real picture.
const PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A temporary notes folder, removed when dropped.
pub struct Notes {
    root: TempDir,
    pub dir: PathBuf,
}

impl Notes {
    pub fn empty() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("notes");
        fs::create_dir(&dir).unwrap();
        Notes { root, dir }
    }

    /// The folder with the notes of [`SEEDS`], last modified at fixed times.
    pub fn seeded() -> Self {
        let notes = Self::empty();
        let now: Timestamp = NOW.parse().unwrap();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(now.as_second().unsigned_abs());
        for seed in SEEDS {
            notes.write(seed.path, seed.text);
            let modified = now - Duration::from_secs(seed.minutes_ago * 60);
            let file = File::options()
                .write(true)
                .open(notes.dir.join(seed.path))
                .unwrap();
            file.set_modified(modified).unwrap();
        }
        let attachment = "Privat/_attachments/01JD3K8Z6Q4X7V9B2M5N8P0R04/skizze.png";
        fs::create_dir_all(notes.dir.join(attachment).parent().unwrap()).unwrap();
        fs::write(notes.dir.join(attachment), PIXEL_PNG).unwrap();
        notes
    }

    pub fn write(&self, rel_path: &str, text: &str) {
        let path = self.dir.join(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    pub fn read(&self, rel_path: &str) -> String {
        fs::read_to_string(self.dir.join(rel_path)).unwrap()
    }

    /// Paths of all note files, relative to the folder, sorted.
    pub fn files(&self) -> Vec<String> {
        let mut found = Vec::new();
        collect(&self.dir, &self.dir, &mut found);
        found.sort();
        found
    }

    /// A place for files that are not notes, such as the state file.
    pub fn scratch(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
}

fn collect(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with(['.', '_']) {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, found);
        } else if name.ends_with(".md") {
            found.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

/// How a test wants the app to look.
#[derive(Clone, Copy)]
pub struct Look {
    pub dark: bool,
    pub accent: u32,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            dark: true,
            accent: 0x3584e4,
        }
    }
}

/// Sets the app up as `app::run` does, with a fixed look and nothing moving, so that
/// tests need no clock and screenshots show the end of every animation.
pub fn init_app(look: Look, cx: &mut App) {
    gpui_component::init(cx);
    let overrides = Overrides {
        dark: Some(look.dark),
        accent: Some(look.accent),
    };
    theme::init(overrides, cx);
    theme::set_appearance(
        Appearance {
            enable_animations: false,
            ..Appearance::default()
        },
        cx,
    );
    actions::bind_keys(cx);
}

/// A Library over `notes` that never looks for the service and lives at [`NOW`].
pub fn open_library(notes: &Notes, cx: &mut App) -> Entity<Library> {
    let library = Library::local(&notes.dir, cx).unwrap();
    library.update(cx, |library, _| library.set_clock(Arc::new(now)));
    library
}
