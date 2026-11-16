//! Text analysis shared by indexing, querying and highlighting.
//!
//! A text is cut into words, and every word is folded: lower-cased, Unicode-normalised
//! and reduced to ASCII where a letter has an obvious base form (`ä→a`, `ß→ss`, `é→e`).
//! Words with German umlauts get a second form with the transliteration (`ä→ae`), which
//! the index stores at the same position. "Einkäufe", "einkaufe" and "einkaeufe" then
//! all find each other.

use std::ops::Range;

use tantivy::tokenizer::{TextAnalyzer, Token, TokenStream, Tokenizer, TokenizerManager};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::{decompose_compatible, is_combining_mark};

/// Name of the word analyser in the tantivy schema.
pub(crate) const WORDS: &str = "sheaf_words";
/// Name of the edge n-gram analyser in the tantivy schema.
pub(crate) const EDGE_NGRAMS: &str = "sheaf_edge_ngrams";

/// Shortest prefix, in characters, that the n-gram fields answer.
pub const MIN_GRAM: usize = 2;
/// Longest prefix, in characters, that the n-gram fields answer.
pub const MAX_GRAM: usize = 12;

/// Words longer than this are not indexed: nobody types them, and they are usually
/// encoded data rather than language.
const MAX_WORD_BYTES: usize = 96;

/// The folded forms of a word.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folded {
    /// Lower-cased and reduced to base letters (`Einkäufe` → `einkaufe`).
    pub primary: String,
    /// The German transliteration (`einkaeufe`), when it differs from `primary`.
    pub german: Option<String>,
}

impl Folded {
    /// Both forms, `primary` first.
    pub fn forms(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.primary.as_str()).chain(self.german.as_deref())
    }
}

/// A word of an analysed text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    /// Byte range of the word in the text it was cut from.
    pub range: Range<usize>,
    pub folded: Folded,
}

/// Folds `text`, which may be a single word or a longer string; characters that are
/// not letters are kept as they are.
pub fn fold(text: &str) -> Folded {
    let mut primary = String::with_capacity(text.len());
    let mut german = String::new();
    let differs = fold_into(text, &mut primary, &mut german);
    Folded {
        primary,
        german: differs.then_some(german),
    }
}

/// Appends the folded forms of `text` to `primary` and `german`, and tells whether the
/// two differ. `german` is only meaningful if they do.
fn fold_into(text: &str, primary: &mut String, german: &mut String) -> bool {
    if text.is_ascii() {
        primary.extend(text.chars().map(|c| c.to_ascii_lowercase()));
        return false;
    }
    let start = primary.len();
    german.clear();
    let mut differs = false;
    // Composing first turns `a` + U+0308 into `ä`, so both spellings fold alike.
    for c in text.nfc().flat_map(char::to_lowercase) {
        let umlaut = match c {
            'ä' => Some(('a', "ae")),
            'ö' => Some(('o', "oe")),
            'ü' => Some(('u', "ue")),
            _ => None,
        };
        if let Some((base, transliteration)) = umlaut {
            if !differs {
                german.push_str(&primary[start..]);
                differs = true;
            }
            primary.push(base);
            german.push_str(transliteration);
            continue;
        }
        let before = primary.len();
        decompose_compatible(c, |part| {
            if !is_combining_mark(part) {
                push_base(part, primary);
            }
        });
        if differs {
            german.push_str(&primary[before..]);
        }
    }
    differs
}

/// Letters without a Unicode decomposition that still have an ASCII spelling.
fn push_base(c: char, out: &mut String) {
    match c {
        'ß' => out.push_str("ss"),
        'æ' => out.push_str("ae"),
        'œ' => out.push_str("oe"),
        'ø' => out.push('o'),
        'đ' | 'ð' => out.push('d'),
        'ł' => out.push('l'),
        'þ' => out.push_str("th"),
        'ħ' => out.push('h'),
        'ı' => out.push('i'),
        _ => out.push(c),
    }
}

/// Whether the character at byte `at` is part of a word, and its length in bytes.
fn word_char(text: &str, at: usize) -> (bool, usize) {
    let byte = text.as_bytes()[at];
    if byte.is_ascii() {
        return (byte.is_ascii_alphanumeric(), 1);
    }
    text[at..].chars().next().map_or((false, 1), |c| {
        // Combining marks belong to the letter before them (decomposed umlauts).
        (c.is_alphanumeric() || is_combining_mark(c), c.len_utf8())
    })
}

/// Byte ranges of the words in `text`, in order.
pub fn word_ranges(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut position = 0;
    std::iter::from_fn(move || {
        loop {
            let mut start = position;
            let mut end = loop {
                if start == text.len() {
                    position = start;
                    return None;
                }
                let (in_word, len) = word_char(text, start);
                if in_word {
                    break start + len;
                }
                start += len;
            };
            while end < text.len() {
                let (in_word, len) = word_char(text, end);
                if !in_word {
                    break;
                }
                end += len;
            }
            position = end;
            if end - start <= MAX_WORD_BYTES {
                return Some(start..end);
            }
        }
    })
}

/// The folded words of `text`, in order.
pub fn words(text: &str) -> impl Iterator<Item = Word> + '_ {
    word_ranges(text).map(|range| Word {
        folded: fold(&text[range.clone()]),
        range,
    })
}

/// Calls `visit` with the byte range, the primary form and the German form, if it has
/// one, of every word of `text`. Unlike [`words`] it allocates nothing per word.
pub fn for_each_word(text: &str, mut visit: impl FnMut(Range<usize>, &str, Option<&str>)) {
    let mut primary = String::new();
    let mut german = String::new();
    for range in word_ranges(text) {
        primary.clear();
        let differs = fold_into(&text[range.clone()], &mut primary, &mut german);
        visit(range, &primary, differs.then_some(german.as_str()));
    }
}

/// The folded words of `text` joined by single spaces: a form of a title that ignores
/// case, accents and punctuation. Used to compare, complete and sort titles.
pub fn key(text: &str) -> Folded {
    let mut primary = String::with_capacity(text.len());
    let mut german = String::with_capacity(text.len());
    let mut differs = false;
    for word in words(text) {
        if !primary.is_empty() {
            primary.push(' ');
            german.push(' ');
        }
        german.push_str(
            word.folded
                .german
                .as_deref()
                .unwrap_or(&word.folded.primary),
        );
        differs |= word.folded.german.is_some();
        primary.push_str(&word.folded.primary);
    }
    Folded {
        primary,
        german: differs.then_some(german),
    }
}

/// Registers the analysers the schema refers to.
pub(crate) fn register(tokenizers: &TokenizerManager) {
    tokenizers.register(WORDS, TextAnalyzer::from(WordTokenizer::default()));
    tokenizers.register(
        EDGE_NGRAMS,
        TextAnalyzer::from(EdgeNgramTokenizer::default()),
    );
}

/// Emits every word in its primary form and, at the same position, in its German form
/// if it has one.
#[derive(Clone, Default)]
struct WordTokenizer {
    token: Token,
    german: String,
}

struct WordTokenStream<'a> {
    text: &'a str,
    ranges: Box<dyn Iterator<Item = Range<usize>> + 'a>,
    token: &'a mut Token,
    german: &'a mut String,
    german_pending: bool,
}

impl Tokenizer for WordTokenizer {
    type TokenStream<'a> = WordTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> WordTokenStream<'a> {
        self.token.reset();
        WordTokenStream {
            text,
            ranges: Box::new(word_ranges(text)),
            token: &mut self.token,
            german: &mut self.german,
            german_pending: false,
        }
    }
}

impl TokenStream for WordTokenStream<'_> {
    fn advance(&mut self) -> bool {
        if self.german_pending {
            self.german_pending = false;
            self.token.text.clear();
            self.token.text.push_str(self.german);
            return true;
        }
        let Some(range) = self.ranges.next() else {
            return false;
        };
        self.token.text.clear();
        self.german_pending =
            fold_into(&self.text[range.clone()], &mut self.token.text, self.german);
        self.token.offset_from = range.start;
        self.token.offset_to = range.end;
        self.token.position = self.token.position.wrapping_add(1);
        true
    }

    fn token(&self) -> &Token {
        self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        self.token
    }
}

/// Emits the prefixes of [`MIN_GRAM`] to [`MAX_GRAM`] characters of every form of every
/// word. A field analysed this way answers "which notes have a word starting with …"
/// with a single term lookup.
#[derive(Clone, Default)]
struct EdgeNgramTokenizer {
    token: Token,
    forms: [String; 2],
}

struct EdgeNgramTokenStream<'a> {
    text: &'a str,
    ranges: Box<dyn Iterator<Item = Range<usize>> + 'a>,
    token: &'a mut Token,
    forms: &'a mut [String; 2],
    /// Number of valid entries in `forms` for the current word.
    num_forms: usize,
    form: usize,
    /// Characters of the last emitted prefix of the current form.
    gram_chars: usize,
    /// Bytes of the last emitted prefix of the current form.
    gram_bytes: usize,
}

impl Tokenizer for EdgeNgramTokenizer {
    type TokenStream<'a> = EdgeNgramTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> EdgeNgramTokenStream<'a> {
        self.token.reset();
        EdgeNgramTokenStream {
            text,
            ranges: Box::new(word_ranges(text)),
            token: &mut self.token,
            forms: &mut self.forms,
            num_forms: 0,
            form: 0,
            gram_chars: 0,
            gram_bytes: 0,
        }
    }
}

impl EdgeNgramTokenStream<'_> {
    /// Extends the current prefix by one character, if the current form has one left.
    fn next_gram(&mut self) -> bool {
        if self.form >= self.num_forms || self.gram_chars >= MAX_GRAM {
            return false;
        }
        let form = &self.forms[self.form];
        let target = (self.gram_chars + 1).max(MIN_GRAM);
        for c in form[self.gram_bytes..]
            .chars()
            .take(target - self.gram_chars)
        {
            self.gram_bytes += c.len_utf8();
            self.gram_chars += 1;
        }
        if self.gram_chars < target {
            return false;
        }
        self.token.text.clear();
        self.token.text.push_str(&form[..self.gram_bytes]);
        true
    }
}

impl TokenStream for EdgeNgramTokenStream<'_> {
    fn advance(&mut self) -> bool {
        loop {
            if self.next_gram() {
                return true;
            }
            if self.form + 1 < self.num_forms {
                self.form += 1;
            } else {
                let Some(range) = self.ranges.next() else {
                    return false;
                };
                let [primary, german] = &mut *self.forms;
                primary.clear();
                let differs = fold_into(&self.text[range.clone()], primary, german);
                self.num_forms = if differs { 2 } else { 1 };
                self.form = 0;
                self.token.offset_from = range.start;
                self.token.offset_to = range.end;
                self.token.position = self.token.position.wrapping_add(1);
            }
            self.gram_chars = 0;
            self.gram_bytes = 0;
        }
    }

    fn token(&self) -> &Token {
        self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folded(word: &str) -> (String, Option<String>) {
        let folded = fold(word);
        (folded.primary, folded.german)
    }

    #[test]
    fn folding_reduces_to_base_letters() {
        let cases = [
            ("Haus", "haus", None),
            ("Einkäufe", "einkaufe", Some("einkaeufe")),
            ("EINKÄUFE", "einkaufe", Some("einkaeufe")),
            ("Straße", "strasse", None),
            ("GROẞ", "gross", None),
            ("Über", "uber", Some("ueber")),
            ("schön", "schon", Some("schoen")),
            ("Café", "cafe", None),
            ("Señor", "senor", None),
            ("Żółć", "zolc", None),
            ("Œuvre", "oeuvre", None),
            ("Smørrebrød", "smorrebrod", None),
            ("ﬁn", "fin", None),
            ("Привет", "привет", None),
            ("2026", "2026", None),
        ];
        for (input, primary, german) in cases {
            assert_eq!(
                folded(input),
                (primary.to_owned(), german.map(str::to_owned)),
                "{input}"
            );
        }
    }

    #[test]
    fn decomposed_umlauts_fold_like_composed_ones() {
        assert_eq!(fold("Einka\u{308}ufe"), fold("Einkäufe"));
        assert_eq!(fold("u\u{308}ber"), fold("über"));
    }

    #[test]
    fn german_form_keeps_the_letters_around_the_umlaut() {
        assert_eq!(
            fold("Müßiggänger").german.as_deref(),
            Some("muessiggaenger")
        );
        assert_eq!(fold("Müßiggänger").primary, "mussigganger");
    }

    #[test]
    fn words_are_cut_at_non_letters() {
        let text = "E-Mail an Jürgen: foo_bar, don't (3,5 kg)!";
        let cut: Vec<&str> = word_ranges(text).map(|range| &text[range]).collect();
        assert_eq!(
            cut,
            [
                "E", "Mail", "an", "Jürgen", "foo", "bar", "don", "t", "3", "5", "kg"
            ]
        );
    }

    #[test]
    fn a_decomposed_umlaut_does_not_split_a_word() {
        let text = "Einka\u{308}ufe machen";
        let cut: Vec<&str> = word_ranges(text).map(|range| &text[range]).collect();
        assert_eq!(cut, ["Einka\u{308}ufe", "machen"]);
    }

    #[test]
    fn overlong_words_are_skipped() {
        let text = format!("vor {} nach", "x".repeat(MAX_WORD_BYTES + 1));
        let cut: Vec<&str> = word_ranges(&text).map(|range| &text[range]).collect();
        assert_eq!(cut, ["vor", "nach"]);
    }

    #[test]
    fn key_joins_folded_words() {
        assert_eq!(
            key("  Einkäufe — für die *Party*! ").primary,
            "einkaufe fur die party"
        );
        assert_eq!(
            key("Einkäufe für die Party").german.as_deref(),
            Some("einkaeufe fuer die party")
        );
        assert_eq!(key("Shopping list").german, None);
        assert_eq!(key("…").primary, "");
    }

    fn tokens(analyzer: &str, text: &str) -> Vec<(String, usize)> {
        let manager = TokenizerManager::default();
        register(&manager);
        let mut analyzer = manager.get(analyzer).unwrap();
        let mut stream = analyzer.token_stream(text);
        let mut out = Vec::new();
        while stream.advance() {
            out.push((stream.token().text.clone(), stream.token().position));
        }
        out
    }

    #[test]
    fn word_tokenizer_emits_both_forms_at_one_position() {
        let expected = [
            ("grosse", 0),
            ("einkaufe", 1),
            ("einkaeufe", 1),
            ("heute", 2),
        ];
        assert_eq!(
            tokens(WORDS, "Große Einkäufe heute"),
            expected.map(|(text, position)| (text.to_owned(), position))
        );
    }

    #[test]
    fn word_tokenizer_reports_offsets_of_the_original_word() {
        let manager = TokenizerManager::default();
        register(&manager);
        let mut analyzer = manager.get(WORDS).unwrap();
        let text = "Die Tür";
        let mut stream = analyzer.token_stream(text);
        let mut offsets = Vec::new();
        while stream.advance() {
            offsets.push(&text[stream.token().offset_from..stream.token().offset_to]);
        }
        assert_eq!(offsets, ["Die", "Tür", "Tür"]);
    }

    #[test]
    fn edge_ngrams_cover_two_to_twelve_characters() {
        let grams: Vec<String> = tokens(EDGE_NGRAMS, "Ausschreibungsunterlagen a ab")
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        let mut expected: Vec<String> = (MIN_GRAM..=MAX_GRAM)
            .map(|len| "ausschreibungsunterlagen"[..len].to_owned())
            .collect();
        expected.push("ab".to_owned());
        assert_eq!(grams, expected);
    }

    #[test]
    fn edge_ngrams_cover_both_forms() {
        let grams: Vec<String> = tokens(EDGE_NGRAMS, "Tür")
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(grams, ["tu", "tur", "tu", "tue", "tuer"]);
    }
}
