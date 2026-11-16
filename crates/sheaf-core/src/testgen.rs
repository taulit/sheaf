//! Generator of synthetic notes folders for benchmarks and budget checks.
//!
//! The notes are made up, but shaped like a real folder: German and English mixed,
//! most notes a few paragraphs long and a few very long, with headings, checklists,
//! tables, images, attachments, tags, wiki links and nested folders. Word frequencies
//! are skewed and compounds, names and numbers give the vocabulary a long tail, since
//! the size of the vocabulary is what prefix and typo search cost.
//!
//! The same seed always gives the same notes, on every platform.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

use jiff::{SignedDuration, Timestamp};

use crate::model::{NoteDoc, NoteId, NoteMeta};

/// The seed [`notes`] uses.
pub const DEFAULT_SEED: u64 = 0x5EAF;

/// The moment the generated notes were "last touched" before: pass it as `now` to
/// searches, so that `modified:` filters find the same notes on every run.
pub fn now() -> Timestamp {
    // 2026-11-16T12:00:00Z
    Timestamp::UNIX_EPOCH + SignedDuration::from_secs(1_794_830_400)
}

/// `count` notes from the default seed.
pub fn notes(count: usize) -> Vec<NoteDoc> {
    notes_with_seed(count, DEFAULT_SEED)
}

/// `count` notes from `seed`.
pub fn notes_with_seed(count: usize, seed: u64) -> Vec<NoteDoc> {
    let mut generator = Generator {
        rng: Rng(seed),
        paths: HashSet::new(),
        titles: Vec::new(),
    };
    (0..count).map(|_| generator.note()).collect()
}

/// Writes `notes` as Markdown files below `dir`, the way Sheaf would have: the file
/// name follows the title, notes with a real id get frontmatter, and the file's
/// modification time is the note's.
pub fn write_notes(dir: &Path, notes: &[NoteDoc]) -> io::Result<()> {
    for note in notes {
        let meta = &note.meta;
        let path = dir.join(&meta.rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = String::with_capacity(note.body.len() + 128);
        if !meta.id.is_provisional() || meta.pinned {
            text.push_str("---\n");
            if !meta.id.is_provisional() {
                let _ = writeln!(text, "id: {}\ncreated: {}", meta.id, meta.created);
            }
            if meta.pinned {
                text.push_str("pinned: true\n");
            }
            text.push_str("---\n");
        }
        text.push_str(&note.body);
        fs::write(&path, text)?;
        let modified = SystemTime::try_from(meta.modified).map_err(io::Error::other)?;
        fs::File::options()
            .write(true)
            .open(&path)?
            .set_modified(modified)?;
    }
    Ok(())
}

/// The queries the benchmark and the budget check run, by class. They are what a user
/// of the generated folder would type, one keystroke state each: words, the beginning
/// of a word, typos, umlaut variants, phrases, filters, and filters with words.
pub const QUERIES: &[(&str, &[&str])] = &[
    (
        "one word",
        &[
            "protokoll",
            "rechnung",
            "meeting",
            "garten",
            "budget",
            "urlaub",
        ],
    ),
    (
        "two words",
        &[
            "projekt planung",
            "meeting budget",
            "rezept kuchen",
            "angebot kunde",
            "server update",
            "reise hotel",
        ],
    ),
    (
        "prefix",
        &[
            "p",
            "ei",
            "aus",
            "aussch",
            "ausschr",
            "proj",
            "einkau",
            "project pl",
            "termin ar",
        ],
    ),
    (
        "typo",
        &["ausschriebung", "protokol", "meetign", "rechnugn", "budgte"],
    ),
    (
        "umlaut",
        &["einkäufe", "einkaeufe", "bücher", "buro", "grosse"],
    ),
    (
        "phrase",
        &[
            "\"nächste woche\"",
            "\"offene punkte\"",
            "\"next week\"",
            "\"action items\" budget",
        ],
    ),
    (
        "filter",
        &[
            "#arbeit",
            "folder:privat",
            "has:checklist",
            "open:",
            "modified:week",
        ],
    ),
    (
        "filter and words",
        &[
            "#projekt planung",
            "folder:arbeit protokoll",
            "hat:checkliste einkauf",
            "offen: meeting",
            "geändert:monat rechnung",
            "has:table budget",
        ],
    ),
];

/// SplitMix64: tiny, fast and identical everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    /// A number in `low..=high`.
    fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high - low + 1)
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    /// A number in `0.0..1.0`.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn pick<'a, T: ?Sized>(&mut self, items: &[&'a T]) -> &'a T {
        items[self.below(items.len())]
    }

    /// Like [`pick`](Rng::pick), but the items at the front come up far more often, as
    /// words do in a language.
    fn pick_skewed<'a, T: ?Sized>(&mut self, items: &[&'a T]) -> &'a T {
        let index = (self.unit().powi(3) * items.len() as f64) as usize;
        items[index.min(items.len() - 1)]
    }
}

const DE_FUNCTION: &str = "die der und in den von zu das mit sich des auf für ist im dem nicht \
    ein eine als auch es an werden aus er hat dass sie nach wird bei einer um am sind noch wie \
    einem über einen so zum war haben nur oder aber vor zur bis mehr durch man sein wurde sehr \
    wenn kann schon gegen vom unter wir soll ich etwas alle wieder diese muss zwischen immer \
    heute morgen dann danach außerdem deshalb später früher während ohne nächste letzte";

const DE_NOUNS: &str = "Projekt Termin Haus Garten Rechnung Angebot Kunde Planung Protokoll \
    Einkauf Liste Rezept Kuchen Urlaub Reise Hotel Büro Arbeit Woche Monat Jahr Zeit Idee \
    Notiz Aufgabe Besprechung Ausschreibung Vertrag Budget Kosten Preis Bestellung Lieferung \
    Wohnung Miete Versicherung Steuer Erklärung Konto Bank Überweisung Arzt Zahnarzt \
    Apotheke Schule Kind Geburtstag Geschenk Feier Einladung Familie Freund Nachbar Auto \
    Werkstatt Fahrrad Bahn Ticket Flug Koffer Wetter Wanderung Berg See Strand Stadt Museum \
    Konzert Buch Bücher Film Serie Musik Kurs Vorlesung Prüfung Klausur Seminar Hausarbeit \
    Thema Frage Antwort Lösung Problem Fehler Server Rechner Programm Update Passwort \
    Sicherung Netzwerk Drucker Telefon Nummer Adresse Straße Tür Fenster Küche Bad Keller \
    Dach Heizung Strom Wasser Müll Pflanze Tomate Apfel Äpfel Brot Brötchen Milch Käse \
    Butter Eier Mehl Zucker Salz Kaffee Tee Wein Bier Suppe Salat Nudeln Reis Gemüse Obst \
    Größe Länge Höhe Gewicht Menge Stück Einkäufe Vorschlag Entwurf Ergebnis Bericht \
    Übersicht Zusammenfassung Ziel Schritt Phase Frist Abgabe Team Leitung Abteilung Firma \
    Kollege Chef Gespräch Entscheidung Änderung Prüfung Freigabe Zahlung Mahnung Quittung";

const DE_VERBS: &str = "machen kaufen planen schreiben lesen prüfen klären fragen anrufen \
    bestellen bezahlen abholen bringen holen suchen finden ändern löschen speichern senden \
    vergleichen besprechen vorbereiten erledigen überweisen kündigen verlängern reparieren \
    putzen kochen backen schneiden gießen packen buchen reservieren absagen bestätigen \
    testen installieren aktualisieren aufräumen sortieren notieren erinnern vergessen";

const DE_ADJECTIVES: &str = "neu alt gut schlecht groß klein wichtig dringend offen fertig \
    schnell langsam teuer günstig einfach schwierig möglich nötig richtig falsch früh spät \
    kurz lang schön grün rot blau weiß schwarz frisch kalt warm süß nächsten letzten ersten \
    zweiten großen kleinen grosse";

const EN_FUNCTION: &str = "the of and to a in is that for it as was with be by on not he this \
    are or his from at which but have an had they you were their one all we can her has \
    there been if more when will would who so no out up into than them only its about then \
    some could these two may first new other time after should also before must next last \
    still between each every because during without today tomorrow later soon";

const EN_NOUNS: &str = "project meeting budget plan report update server client customer \
    invoice offer contract deadline task issue release version design review feedback \
    question answer idea note list item schedule calendar call email team manager office \
    house garden kitchen recipe cake dinner lunch breakfast coffee shopping groceries \
    trip flight hotel ticket train car bike weather weekend holiday birthday present party \
    book books movie music course lecture exam paper topic problem solution error bug \
    feature test backup password network printer phone number address street door window \
    price cost payment account bank insurance tax doctor dentist school family friend \
    neighbour summary overview goal step phase milestone roadmap proposal draft result \
    decision change approval action items notes week month year morning evening";

const EN_VERBS: &str = "make buy plan write read check ask call order pay collect bring find \
    change delete save send compare discuss prepare finish cancel renew repair clean cook \
    bake pack book reserve confirm test install update tidy sort remember forget ship fix \
    review merge deploy measure";

const EN_ADJECTIVES: &str = "new old good bad big small important urgent open done quick \
    slow expensive cheap simple hard possible necessary right wrong early late short long \
    nice green red blue white black fresh cold warm sweet next last first second";

const FIRST_NAMES: &str = "Anna Jonas Lena Paul Mia Felix Emma Lukas Laura Max Sophie Tim \
    Julia Jan Marie Tom Clara Ben Nina David Sarah Erik Hannah Moritz Lea Simon Eva Jakob \
    Ida Oskar Jürgen Günter Bärbel Sören";

const LAST_NAMES: &str = "Müller Schmidt Schneider Fischer Weber Meyer Wagner Becker Schulz \
    Hoffmann Schäfer Koch Bauer Richter Klein Wolf Schröder Neumann Schwarz Zimmermann Braun \
    Krüger Hofmann Hartmann Lange Schmitt Werner Krause Meier Lehmann Köhler Böhm Groß Voß";

const CITIES: &str = "Berlin Hamburg München Köln Zürich Wien Leipzig Dresden Bremen \
    Stuttgart Düsseldorf Lübeck Graz Basel Paris London Rome Lisbon Oslo Prague";

const SYLLABLES: &str = "ka lo mi ra tu ven dor sil qua nex ori pal zen tro bix fy gur hal \
    jet kor lum mar nov pix";

const FOLDERS: &[&str] = &[
    "",
    "Arbeit",
    "Privat",
    "Arbeit/Projekte",
    "Schnellnotizen",
    "Arbeit/Protokolle",
    "Privat/Rezepte",
    "Ideen",
    "Privat/Reisen",
    "Uni",
    "Work",
    "Uni/Vorlesungen",
    "Journal/2026",
    "Work/Meetings",
    "Privat/Haus und Garten",
    "Journal/2025",
    "Arbeit/Projekte/Archiv",
    "Bücher",
];

const TAGS: &[&str] = &[
    "arbeit",
    "privat",
    "projekt",
    "idee",
    "todo",
    "einkauf",
    "arbeit/projekte",
    "rezepte",
    "projekt/planung",
    "reise",
    "meeting",
    "wichtig",
    "rezepte/backen",
    "uni",
    "haus",
    "garten",
    "bücher",
    "finanzen",
    "arbeit/protokolle",
    "gesundheit",
    "work",
    "reading-list",
];

/// Collocations that occur as written, so that phrase queries have something to find.
const DE_PHRASES: &[&str] = &[
    "nächste Woche",
    "offene Punkte",
    "bis Ende des Monats",
    "wie besprochen",
    "noch zu klären",
];

const EN_PHRASES: &[&str] = &[
    "next week",
    "action items",
    "as discussed",
    "by the end of the month",
    "to be confirmed",
];

struct Language {
    function: Vec<&'static str>,
    nouns: Vec<&'static str>,
    verbs: Vec<&'static str>,
    adjectives: Vec<&'static str>,
    phrases: &'static [&'static str],
    german: bool,
}

struct Words {
    german: Language,
    english: Language,
    first_names: Vec<&'static str>,
    last_names: Vec<&'static str>,
    cities: Vec<&'static str>,
    syllables: Vec<&'static str>,
}

impl Words {
    fn get() -> &'static Words {
        static WORDS: LazyLock<Words> = LazyLock::new(Words::new);
        &WORDS
    }

    fn new() -> Self {
        let list = |words: &'static str| words.split_whitespace().collect::<Vec<_>>();
        Words {
            german: Language {
                function: list(DE_FUNCTION),
                nouns: list(DE_NOUNS),
                verbs: list(DE_VERBS),
                adjectives: list(DE_ADJECTIVES),
                phrases: DE_PHRASES,
                german: true,
            },
            english: Language {
                function: list(EN_FUNCTION),
                nouns: list(EN_NOUNS),
                verbs: list(EN_VERBS),
                adjectives: list(EN_ADJECTIVES),
                phrases: EN_PHRASES,
                german: false,
            },
            first_names: list(FIRST_NAMES),
            last_names: list(LAST_NAMES),
            cities: list(CITIES),
            syllables: list(SYLLABLES),
        }
    }
}

struct Generator {
    rng: Rng,
    paths: HashSet<String>,
    /// Titles of the notes so far, the targets of wiki links.
    titles: Vec<String>,
}

/// The text of a note while it is written, with what the index needs to know about it.
#[derive(Default)]
struct Draft {
    body: String,
    headings: Vec<String>,
    tags: Vec<String>,
    links: Vec<String>,
    tasks_open: u32,
    tasks_total: u32,
    has_image: bool,
    has_table: bool,
    has_attachment: bool,
}

impl Generator {
    fn note(&mut self) -> NoteDoc {
        let german = self.rng.chance(65);
        let mut draft = Draft::default();
        let mut title = self.title(german);
        let folder = self.rng.pick_skewed(FOLDERS);
        let conflict = self.rng.chance(1);
        let rel_path = loop {
            let name = if conflict {
                format!("{title}.sync-conflict-20261101-120000-ABCDEFG.md")
            } else {
                format!("{title}.md")
            };
            let path = if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            };
            if self.paths.insert(path.clone()) {
                break path;
            }
            let _ = write!(title, " {}", self.rng.between(2, 99));
        };

        if self.rng.chance(60) {
            draft.body.push_str("# ");
        }
        draft.body.push_str(&title);
        draft.body.push_str("\n\n");

        // Most notes have 50 to 400 words; one in twenty is a long document.
        let target_words = if self.rng.chance(5) {
            self.rng.between(1_500, 6_000)
        } else {
            self.rng.between(50, 400)
        };
        let mut words = 0;
        while words < target_words {
            let german = if self.rng.chance(10) { !german } else { german };
            words += self.section(&mut draft, german, target_words - words);
        }
        if self.rng.chance(60) {
            for _ in 0..self.rng.between(1, 3) {
                let tag = self.rng.pick_skewed(TAGS);
                if !draft.tags.iter().any(|seen| seen == tag) {
                    draft.tags.push(tag.to_owned());
                    let _ = write!(draft.body, "#{tag} ");
                }
            }
            draft.body.push('\n');
        }

        // Recent notes are touched far more often than old ones.
        let age_seconds = (self.rng.unit().powi(3) * 730.0 * 86_400.0) as i64;
        let modified = now() - SignedDuration::from_secs(age_seconds);
        // A file Sheaf never wrote has no frontmatter: no id, and no creation time
        // other than the file's own.
        let written_by_sheaf = self.rng.chance(40);
        let (id, created) = if written_by_sheaf {
            let since_creation = self.rng.below(200 * 86_400) as i64;
            (
                self.ulid(),
                modified - SignedDuration::from_secs(since_creation),
            )
        } else {
            (NoteId::provisional(&rel_path), modified)
        };

        self.titles.push(title.clone());
        NoteDoc {
            meta: NoteMeta {
                id,
                rel_path,
                folder: folder.to_owned(),
                title,
                created,
                modified,
                pinned: self.rng.chance(2),
                tags: draft.tags,
                tasks_open: draft.tasks_open,
                tasks_total: draft.tasks_total,
                has_image: draft.has_image,
                has_table: draft.has_table,
                has_attachment: draft.has_attachment,
                conflict,
            },
            headings: draft.headings,
            body: draft.body,
            links: draft.links,
        }
    }

    fn ulid(&mut self) -> NoteId {
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let mut id = String::from("01");
        for _ in 0..24 {
            id.push(ALPHABET[self.rng.below(32)] as char);
        }
        id.parse().unwrap_or_else(|_| NoteId::provisional(&id))
    }

    fn language(german: bool) -> &'static Language {
        let words = Words::get();
        if german {
            &words.german
        } else {
            &words.english
        }
    }

    fn title(&mut self, german: bool) -> String {
        let noun = self.noun(german);
        let other = self.noun(german);
        let name = self.rng.pick(&Words::get().last_names);
        let city = self.rng.pick(&Words::get().cities);
        let date = self.date();
        if german {
            match self.rng.below(12) {
                0 => format!("Protokoll {date}"),
                1 => format!("Besprechung {noun} mit {name}"),
                2 => format!("Ausschreibung {noun}"),
                3 => format!("Einkaufsliste {date}"),
                4 => format!("Ideen für {noun} und {other}"),
                5 => format!("Rezept: {noun}{}", other.to_lowercase()),
                6 => format!("Reise nach {city}"),
                7 => format!("Notizen zu {noun}"),
                8 => format!("{noun} {other}"),
                9 => format!("Angebot {name} {noun}"),
                10 => format!("Planung {noun} {}", self.rng.between(2024, 2027)),
                _ => noun.to_owned(),
            }
        } else {
            match self.rng.below(10) {
                0 => format!("Meeting notes {date}"),
                1 => format!("{noun} plan"),
                2 => format!("Ideas for the {noun}"),
                3 => format!("Todo: {noun} and {other}"),
                4 => format!("Trip to {city}"),
                5 => format!("Reading list {}", self.rng.between(2024, 2027)),
                6 => format!("{noun} {other} review"),
                7 => format!("Call with {name} about the {noun}"),
                8 => format!("Weekly update {date}"),
                _ => format!("Notes on {noun}"),
            }
        }
    }

    fn date(&mut self) -> String {
        format!(
            "{}-{:02}-{:02}",
            self.rng.between(2024, 2026),
            self.rng.between(1, 12),
            self.rng.between(1, 28)
        )
    }

    fn noun(&mut self, german: bool) -> &'static str {
        let nouns = &Self::language(german).nouns;
        let index = (self.rng.unit().powi(2) * nouns.len() as f64) as usize;
        nouns[index.min(nouns.len() - 1)]
    }

    /// Writes a heading, some paragraphs and perhaps a list, a table or a link, and
    /// returns the number of words written. `budget` is what the note still needs.
    fn section(&mut self, draft: &mut Draft, german: bool, budget: usize) -> usize {
        let mut words = 0;
        if self.rng.chance(50) {
            let heading = if self.rng.chance(50) {
                self.noun(german).to_owned()
            } else {
                format!("{} {}", self.noun(german), self.noun(german))
            };
            let level = if self.rng.chance(75) { "##" } else { "###" };
            let _ = writeln!(draft.body, "{level} {heading}\n");
            draft.headings.push(heading);
        }
        for _ in 0..self.rng.between(1, 3) {
            let length = self
                .rng
                .between(20, 90)
                .min(budget.saturating_sub(words).max(12));
            words += self.paragraph(draft, german, length);
            if words >= budget {
                break;
            }
        }
        match self.rng.below(100) {
            0..12 => words += self.checklist(draft, german),
            12..17 => words += self.table(draft, german),
            17..23 => {
                let noun = self.noun(german);
                let file = self.pseudo_word();
                let _ = writeln!(draft.body, "![{noun}](_attachments/{file}/{file}.jpg)\n");
                draft.has_image = true;
            }
            23..26 => {
                let noun = self.noun(german);
                let file = self.pseudo_word();
                let _ = writeln!(draft.body, "[{noun}](_attachments/{file}/{file}.pdf)\n");
                draft.has_attachment = true;
            }
            26..32 if !self.titles.is_empty() => {
                let target = self.titles[self.rng.below(self.titles.len())].clone();
                let _ = writeln!(draft.body, "→ [[{target}]]\n");
                draft.links.push(target);
            }
            _ => {}
        }
        words
    }

    fn paragraph(&mut self, draft: &mut Draft, german: bool, length: usize) -> usize {
        let mut words = 0;
        while words < length {
            let sentence_length = self.rng.between(5, 16);
            for position in 0..sentence_length {
                let mut word = self.word(german);
                if position == 0 {
                    word = capitalised(&word);
                }
                if position > 0 {
                    draft.body.push(if self.rng.chance(6) { ',' } else { ' ' });
                    if draft.body.ends_with(',') {
                        draft.body.push(' ');
                    }
                }
                words += word.split(' ').count();
                draft.body.push_str(&word);
            }
            draft
                .body
                .push_str(if self.rng.chance(8) { "? " } else { ". " });
        }
        draft.body.truncate(draft.body.trim_end().len());
        draft.body.push_str("\n\n");
        words
    }

    fn checklist(&mut self, draft: &mut Draft, german: bool) -> usize {
        let items = self.rng.between(2, 7);
        for _ in 0..items {
            let done = self.rng.chance(45);
            let verb = self.rng.pick(&Self::language(german).verbs);
            let noun = self.noun(german);
            let mark = if done { 'x' } else { ' ' };
            let _ = if german {
                writeln!(draft.body, "- [{mark}] {noun} {verb}")
            } else {
                writeln!(draft.body, "- [{mark}] {verb} the {noun}")
            };
            draft.tasks_total += 1;
            draft.tasks_open += u32::from(!done);
        }
        draft.body.push('\n');
        items * 2
    }

    fn table(&mut self, draft: &mut Draft, german: bool) -> usize {
        let header = if german {
            "| Posten | Menge | Preis |"
        } else {
            "| Item | Amount | Price |"
        };
        let _ = writeln!(draft.body, "{header}\n|---|---:|---:|");
        let rows = self.rng.between(2, 6);
        for _ in 0..rows {
            let noun = self.noun(german);
            let amount = self.rng.between(1, 40);
            let price = self.rng.between(1, 900);
            let cents = self.rng.below(100);
            let _ = writeln!(draft.body, "| {noun} | {amount} | {price},{cents:02} € |");
        }
        draft.body.push('\n');
        draft.has_table = true;
        rows * 3
    }

    /// One word of running text, or occasionally a few that belong together.
    fn word(&mut self, german: bool) -> String {
        let language = Self::language(german);
        let german = language.german;
        match self.rng.below(100) {
            0..42 => self.rng.pick_skewed(&language.function).to_owned(),
            42..68 => self.noun(german).to_owned(),
            68..79 => self.rng.pick_skewed(&language.verbs).to_owned(),
            79..88 => self.rng.pick_skewed(&language.adjectives).to_owned(),
            88..92 if german => {
                // Compounds are where German gets its endless vocabulary from.
                let head = self.noun(true);
                let tail = self.rng.pick(&language.nouns).to_lowercase();
                format!("{head}{tail}")
            }
            88..92 => format!("{}-{}", self.noun(false), self.noun(false)),
            92..94 => self.rng.pick(language.phrases).to_owned(),
            94..96 => {
                let first = self.rng.pick(&Words::get().first_names);
                let last = self.rng.pick(&Words::get().last_names);
                format!("{first} {last}")
            }
            96..98 => match self.rng.below(3) {
                0 => self.date(),
                1 => format!(
                    "{},{:02} €",
                    self.rng.between(1, 4_000),
                    self.rng.below(100)
                ),
                _ => self.rng.between(1, 9_999).to_string(),
            },
            98 => self.rng.pick(&Words::get().cities).to_owned(),
            _ => self.pseudo_word(),
        }
    }

    /// A made-up word, like a product name or an abbreviation nobody else uses.
    fn pseudo_word(&mut self) -> String {
        (0..self.rng.between(2, 4))
            .map(|_| self.rng.pick(&Words::get().syllables))
            .collect()
    }
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis;

    #[test]
    fn the_same_seed_gives_the_same_notes() {
        assert_eq!(notes(200), notes(200));
        assert_eq!(notes(50), notes(200)[..50]);
        assert_ne!(notes_with_seed(50, 1), notes_with_seed(50, 2));
    }

    #[test]
    fn notes_look_like_a_real_folder() {
        let notes = notes(2_000);
        let share = |count: usize| count * 100 / notes.len();
        let count =
            |predicate: fn(&NoteDoc) -> bool| notes.iter().filter(|note| predicate(note)).count();

        let paths: HashSet<&str> = notes
            .iter()
            .map(|note| note.meta.rel_path.as_str())
            .collect();
        assert_eq!(paths.len(), notes.len(), "paths are unique");
        let ids: HashSet<&NoteId> = notes.iter().map(|note| &note.meta.id).collect();
        assert_eq!(ids.len(), notes.len(), "ids are unique");

        let lengths: Vec<usize> = notes
            .iter()
            .map(|note| analysis::word_ranges(&note.body).count())
            .collect();
        let typical = lengths
            .iter()
            .filter(|length| (40..=500).contains(*length))
            .count();
        let long = lengths.iter().filter(|length| **length > 1_500).count();
        assert!(share(typical) >= 85, "{}% typical", share(typical));
        assert!((2..=8).contains(&share(long)), "{}% long", share(long));

        assert!((10..=45).contains(&share(count(|note| note.meta.tasks_total > 0))));
        assert!(count(|note| note.meta.tasks_open > 0) > 0);
        assert!((5..=30).contains(&share(count(|note| note.meta.has_table))));
        assert!((5..=35).contains(&share(count(|note| note.meta.has_image))));
        assert!((2..=25).contains(&share(count(|note| note.meta.has_attachment))));
        assert!((40..=75).contains(&share(count(|note| !note.meta.tags.is_empty()))));
        assert!((25..=55).contains(&share(count(|note| !note.meta.id.is_provisional()))));
        assert!(count(|note| note.meta.pinned) > 0);
        assert!(count(|note| note.meta.conflict) > 0);
        assert!(count(|note| !note.links.is_empty()) > 0);
        assert!(count(|note| note.meta.folder.is_empty()) > 0);
        assert!(count(|note| note.meta.folder.contains('/')) > 0);
        assert!(count(|note| note.body.contains('ä')) > notes.len() / 2);
        assert!(
            notes
                .iter()
                .all(|note| note.meta.created <= note.meta.modified)
        );
        assert!(notes.iter().all(|note| note.meta.modified <= now()));
    }

    #[test]
    fn the_meta_data_describes_the_text() {
        for note in notes(300) {
            let meta = &note.meta;
            let body = &note.body;
            let first_line = body.lines().next().unwrap();
            assert_eq!(first_line.trim_start_matches("# "), meta.title);
            assert!(meta.rel_path.ends_with(".md"));
            assert!(meta.rel_path.starts_with(&meta.folder));
            assert_eq!(body.matches("- [ ] ").count() as u32, meta.tasks_open);
            assert_eq!(
                (body.matches("- [ ] ").count() + body.matches("- [x] ").count()) as u32,
                meta.tasks_total
            );
            assert_eq!(body.contains("!["), meta.has_image);
            assert_eq!(body.contains("|---|"), meta.has_table);
            assert_eq!(body.contains(".pdf)"), meta.has_attachment);
            assert_eq!(
                body.matches("\n## ").count() + body.matches("\n### ").count(),
                note.headings.len()
            );
            assert_eq!(body.matches("[[").count(), note.links.len());
            for tag in &meta.tags {
                assert!(body.contains(&format!("#{tag} ")), "{tag}");
            }
        }
    }

    #[test]
    fn notes_are_written_as_files() {
        let dir = tempfile::tempdir().unwrap();
        let notes = notes(60);
        write_notes(dir.path(), &notes).unwrap();
        for note in &notes {
            let path = dir.path().join(&note.meta.rel_path);
            let text = fs::read_to_string(&path).unwrap();
            assert!(text.ends_with(&note.body));
            let frontmatter = &text[..text.len() - note.body.len()];
            assert_eq!(
                frontmatter.contains(&format!("id: {}\n", note.meta.id)),
                !note.meta.id.is_provisional()
            );
            assert_eq!(frontmatter.contains("pinned: true\n"), note.meta.pinned);
            assert_eq!(
                frontmatter.is_empty(),
                note.meta.id.is_provisional() && !note.meta.pinned
            );
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            assert_eq!(Timestamp::try_from(modified).unwrap(), note.meta.modified);
        }
    }

    #[test]
    fn the_query_mix_has_about_forty_queries() {
        let queries: Vec<&str> = QUERIES
            .iter()
            .flat_map(|(_, queries)| queries.iter().copied())
            .collect();
        assert!((35..=50).contains(&queries.len()), "{}", queries.len());
        let unique: HashSet<&str> = queries.iter().copied().collect();
        assert_eq!(unique.len(), queries.len());
    }
}
