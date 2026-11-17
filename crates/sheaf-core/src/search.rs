//! Search: which notes match a query, in which order, and where.
//!
//! Every word of a query must be found in a note, exactly, as the beginning of a word
//! or with a typo. A word earns the weight of the field it is found in times the
//! quality of the match, counting only its best field, and a note's score is the sum
//! over the query's words. Scores are deliberately coarse, so that among notes that
//! match equally well the one edited last comes first.

use std::ops::Range;
use std::sync::Arc;

use jiff::ZonedDateTime;
use serde::Serialize;
use tantivy::postings::TermInfo;
use tantivy::query::{
    AllQuery, BooleanQuery, ConstScoreQuery, DfaWrapper, DisjunctionMaxQuery, EnableScoring,
    Explanation, FuzzyTermQuery, Occur, PhraseQuery, Query, Scorer, TermSetQuery, Weight,
};
use tantivy::schema::Field;
use tantivy::{
    DocAddress, DocId, DocSet, InvertedIndexReader, Score, Searcher, SegmentReader, TERMINATED,
    TantivyDocument, TantivyError, Term,
};

use crate::analysis::{self, Folded, MAX_GRAM, MIN_GRAM};
use crate::error::Result;
use crate::index::{
    Fields, MODIFIED, Reader, Sort, for_each_doc, for_each_term_with_prefix, term_query, text,
};
use crate::model::NoteMeta;
use crate::query::{ItemKind, ParsedQuery};
use crate::snippet::{self, Snippet};

/// Weights of title, tags, headings and body.
const FIELD_WEIGHTS: [Score; 4] = [4.0, 3.0, 2.0, 1.0];

/// How well a word of a note matches a word of the query. The numeric value indexes
/// [`MATCH_FACTORS`].
const FUZZY: u8 = 1;
const PREFIX: u8 = 2;
const EXACT: u8 = 3;

/// The share of a field's weight a match earns: none, fuzzy, prefix, exact. A typo
/// match in the title must not outrank the exact word in the body, so fuzzy is worth
/// less than a quarter.
const MATCH_FACTORS: [Score; 4] = [0.0, 0.1875, 0.5, 1.0];

/// Added when the title is the query, so that this note comes before the notes that
/// merely have the query's words in their title.
const TITLE_IS_QUERY_BONUS: Score = 2.0;

/// A note found by a search.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub meta: NoteMeta,
    /// Higher is better; `0.0` for queries without words, which are ordered by time.
    pub score: f32,
    /// Byte ranges of `meta.title` that match the query.
    pub title_highlights: Vec<Range<usize>>,
    pub snippet: Snippet,
}

/// The best hits of a search and the number of notes that matched in all.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SearchResults {
    pub hits: Vec<Hit>,
    pub total: usize,
}

impl Reader {
    /// Runs `query` and returns its `limit` best hits with highlights and snippets.
    ///
    /// A query without words lists the notes that pass its filters, most recently
    /// modified first; an empty query is [`Reader::recent`]. `now` is what
    /// `modified:` filters are measured against.
    pub fn search(
        &self,
        query: &ParsedQuery,
        limit: usize,
        now: &ZonedDateTime,
    ) -> Result<SearchResults> {
        let searcher = self.inner.searcher();
        let matcher = Matcher::new(query);
        let filters = self.filter_query(query.filters(), now);
        let (ranked, total) = if matcher.is_empty() {
            let query = filters.unwrap_or_else(|| Box::new(AllQuery));
            let (addresses, total) =
                self.sorted(&searcher, query.as_ref(), Sort::Modified, Some(limit))?;
            let unscored = addresses.into_iter().map(|address| (0.0, address));
            (unscored.collect(), total)
        } else {
            let query = self.text_query(&matcher, filters);
            ranked(&searcher, &query, limit)?
        };
        let hits = ranked
            .into_iter()
            .map(|(score, address)| {
                let doc: TantivyDocument = searcher.doc(address)?;
                let meta = self.meta(&doc)?;
                Ok(Hit {
                    score,
                    title_highlights: matcher.highlights(&meta.title),
                    snippet: snippet::build(text(&doc, self.fields.markdown), &matcher),
                    meta,
                })
            })
            .collect::<Result<_>>()?;
        Ok(SearchResults { hits, total })
    }

    fn text_query(&self, matcher: &Matcher, filters: Option<Box<dyn Query>>) -> BooleanQuery {
        let fields = &self.fields;
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        if !matcher.words.is_empty() {
            let words = WordsQuery {
                words: Arc::clone(&matcher.words),
                fields: Arc::clone(fields),
            };
            clauses.push((Occur::Must, Box::new(words)));
        }
        for phrase in &matcher.phrases {
            clauses.push((Occur::Must, phrase_query(fields, phrase)));
        }
        if let Some(filters) = filters {
            clauses.push((Occur::Must, Box::new(ConstScoreQuery::new(filters, 0.0))));
        }
        let title_keys = matcher
            .title_key
            .forms()
            .map(|key| Term::from_field_text(fields.title_key, key));
        let title_is_query = ConstScoreQuery::new(
            Box::new(TermSetQuery::new(title_keys)),
            TITLE_IS_QUERY_BONUS,
        );
        clauses.push((Occur::Should, Box::new(title_is_query)));
        BooleanQuery::new(clauses)
    }
}

/// The text fields a word is looked up in, in the order of [`FIELD_WEIGHTS`], each
/// with its n-gram companion if it has one.
fn text_fields(fields: &Fields) -> [(Field, Option<Field>); 4] {
    [
        (fields.title, Some(fields.title_prefix)),
        (fields.tags, None),
        (fields.headings, Some(fields.headings_prefix)),
        (fields.body, None),
    ]
}

/// The live notes matching `query`, best first, cut to `limit`, and their number
/// before the cut. Equal scores are ordered by modification time, newest first.
fn ranked(
    searcher: &Searcher,
    query: &dyn Query,
    limit: usize,
) -> Result<(Vec<(Score, DocAddress)>, usize)> {
    let weight = query.weight(EnableScoring::enabled_from_searcher(searcher))?;
    let mut rows: Vec<(Score, i64, DocAddress)> = Vec::new();
    for (ordinal, segment) in searcher.segment_readers().iter().enumerate() {
        let modified = segment.fast_fields().i64(MODIFIED)?;
        weight.for_each(segment, &mut |doc, score| {
            if !segment.is_deleted(doc) {
                let modified = modified.first(doc).unwrap_or(i64::MIN);
                rows.push((score, modified, DocAddress::new(ordinal as u32, doc)));
            }
        })?;
    }
    let total = rows.len();
    rows.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    rows.truncate(limit);
    let ranked = rows.into_iter().map(|(score, _, address)| (score, address));
    Ok((ranked.collect(), total))
}

/// A phrase must occur in one of the text fields; like a word found exactly, it earns
/// the weight of the best field it occurs in.
fn phrase_query(fields: &Fields, phrase: &[Folded]) -> Box<dyn Query> {
    let in_field = |field: Field, form: fn(&Folded) -> &str| -> Box<dyn Query> {
        let mut terms: Vec<Term> = phrase
            .iter()
            .map(|word| Term::from_field_text(field, form(word)))
            .collect();
        match terms.len() {
            1 => term_query(terms.remove(0)),
            _ => Box::new(PhraseQuery::new(terms)),
        }
    };
    let has_german_form = phrase.iter().any(|word| word.german.is_some());
    let per_field = text_fields(fields).into_iter().zip(FIELD_WEIGHTS).map(
        |((field, _), weight)| -> Box<dyn Query> {
            let mut forms = vec![in_field(field, |word| &word.primary)];
            if has_german_form {
                forms.push(in_field(field, |word| {
                    word.german.as_deref().unwrap_or(&word.primary)
                }));
            }
            Box::new(ConstScoreQuery::new(
                Box::new(BooleanQuery::union(forms)),
                weight,
            ))
        },
    );
    Box::new(DisjunctionMaxQuery::new(per_field.collect()))
}

/// A word of the query in the forms it is looked up in.
#[derive(Clone, Debug)]
struct WordTerm {
    /// The folded word and, if different, its German transliteration.
    forms: Vec<String>,
    /// Edits a word of a note may be away from this one: 1 from 4 characters, 2 from 8.
    distance: u8,
    /// Whether words that merely start with this one match. False only for the first
    /// parts of a word the analysis splits, such as the `e` of `e-mail`.
    prefix: bool,
}

impl WordTerm {
    fn new(folded: &Folded, prefix: bool) -> Self {
        WordTerm {
            forms: folded.forms().map(str::to_owned).collect(),
            distance: match folded.primary.chars().count() {
                0..4 => 0,
                4..8 => 1,
                _ => 2,
            },
            prefix,
        }
    }

    /// Whether a word of a note, given in one of its folded forms, matches.
    fn matches(&self, word: &str) -> bool {
        let distance = usize::from(self.distance);
        self.forms.iter().any(|form| {
            word == form
                || (self.prefix && word.starts_with(form.as_str()))
                || (distance > 0 && within_distance(word, form, distance))
        })
    }
}

/// A match of a query term in a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    pub range: Range<usize>,
    /// Which word or phrase of the query matched; equal for equal terms.
    pub term: usize,
}

/// The words and phrases of a query, analysed. Decides what the index is asked for
/// and, with the same rules, which words of a hit are highlighted.
#[derive(Debug)]
pub(crate) struct Matcher {
    words: Arc<[WordTerm]>,
    phrases: Vec<Vec<Folded>>,
    /// The title a note would need to have for the query to name it.
    title_key: Folded,
}

impl Matcher {
    pub(crate) fn new(query: &ParsedQuery) -> Self {
        let mut words = Vec::new();
        let mut phrases = Vec::new();
        let mut text = String::new();
        for item in &query.items {
            match &item.kind {
                ItemKind::Word(word) => {
                    let parts: Vec<Folded> =
                        analysis::words(word).map(|part| part.folded).collect();
                    for (index, part) in parts.iter().enumerate() {
                        words.push(WordTerm::new(part, index + 1 == parts.len()));
                    }
                    text.push_str(word);
                }
                ItemKind::Phrase(phrase) => {
                    let parts: Vec<Folded> =
                        analysis::words(phrase).map(|part| part.folded).collect();
                    if !parts.is_empty() {
                        phrases.push(parts);
                    }
                    text.push_str(phrase);
                }
                ItemKind::Filter(_) | ItemKind::Pending(_) => continue,
            }
            text.push(' ');
        }
        Matcher {
            words: words.into(),
            phrases,
            title_key: analysis::key(&text),
        }
    }

    /// Whether the query has nothing to search for, only filters at most.
    pub(crate) fn is_empty(&self) -> bool {
        self.words.is_empty() && self.phrases.is_empty()
    }

    /// The number of words and phrases, which is the most different terms a text can
    /// have [`marks`](Matcher::marks) for.
    pub(crate) fn num_terms(&self) -> usize {
        self.words.len() + self.phrases.len()
    }

    /// The words of `text` the query matches, in text order.
    pub(crate) fn marks(&self, text: &str) -> Vec<Mark> {
        let mut marks = Vec::new();
        // With phrases every word is kept until the phrases are found, with one bit
        // per word of a phrase that it equals.
        let mut candidates: Vec<(Range<usize>, Option<usize>, u64)> = Vec::new();
        let phrase_words = || self.phrases.iter().flatten().take(u64::BITS as usize);
        analysis::for_each_word(text, |range, primary, german| {
            let forms = [Some(primary), german];
            let matches = |word: &WordTerm| forms.iter().flatten().any(|form| word.matches(form));
            let term = self.words.iter().position(matches);
            if self.phrases.is_empty() {
                marks.extend(term.map(|term| Mark { range, term }));
                return;
            }
            let equal = |wanted: &Folded| {
                wanted
                    .forms()
                    .any(|wanted| forms.iter().flatten().any(|form| *form == wanted))
            };
            let equals = phrase_words()
                .enumerate()
                .filter(|(_, wanted)| equal(wanted))
                .fold(0, |bits, (index, _)| bits | 1 << index);
            candidates.push((range, term, equals));
        });

        let mut first_bit = 0;
        for (index, phrase) in self.phrases.iter().enumerate() {
            let bits = first_bit..(first_bit + phrase.len()).min(u64::BITS as usize);
            first_bit = bits.end;
            for start in 0..(candidates.len() + 1).saturating_sub(bits.len().max(1)) {
                let occurs = !bits.is_empty()
                    && bits
                        .clone()
                        .zip(&candidates[start..])
                        .all(|(bit, (_, _, equals))| equals & (1 << bit) != 0);
                if occurs {
                    for (_, term, _) in &mut candidates[start..start + bits.len()] {
                        term.get_or_insert(self.words.len() + index);
                    }
                }
            }
        }
        marks.extend(
            candidates
                .into_iter()
                .filter_map(|(range, term, _)| Some(Mark { range, term: term? })),
        );
        marks
    }

    /// The byte ranges of `text` the query matches.
    pub(crate) fn highlights(&self, text: &str) -> Vec<Range<usize>> {
        self.marks(text)
            .into_iter()
            .map(|mark| mark.range)
            .collect()
    }
}

/// Whether `a` can be turned into `b` with at most `max` insertions, deletions,
/// substitutions and swaps of neighbours, which is what the index's typo tolerance
/// accepts.
fn within_distance(a: &str, b: &str, max: usize) -> bool {
    if a.len().abs_diff(b.len()) > max * 4 {
        return false;
    }
    if a.is_ascii() && b.is_ascii() {
        // An edit adds at most one kind of letter and removes at most one, so words
        // whose letters differ more than that are told apart without comparing them
        // position by position. This rejects nearly every word of a text.
        let (letters_a, letters_b) = (letters(a), letters(b));
        (letters_a & !letters_b).count_ones() as usize <= max
            && (letters_b & !letters_a).count_ones() as usize <= max
            && edits_within(a.as_bytes(), b.as_bytes(), max)
    } else {
        let a: Vec<char> = a.chars().collect();
        let b: Vec<char> = b.chars().collect();
        edits_within(&a, &b, max)
    }
}

/// The set of bytes that occur in `word`, in 32 buckets.
fn letters(word: &str) -> u32 {
    word.bytes().fold(0, |set, byte| set | 1 << (byte % 32))
}

fn edits_within<T: PartialEq>(a: &[T], b: &[T], max: usize) -> bool {
    /// Longest word compared; anything longer is not a typo of a word in a note.
    const MAX_LEN: usize = 127;
    if a.len().abs_diff(b.len()) > max || a.len() > MAX_LEN || b.len() > MAX_LEN {
        return false;
    }
    // Three rows of the edit-distance table, reused in turn: row `i % 3` holds the
    // distances between the first `i` elements of `a` and every prefix of `b`.
    let mut rows = [[0u8; MAX_LEN + 1]; 3];
    for (column, cell) in rows[0][..=b.len()].iter_mut().enumerate() {
        *cell = column as u8;
    }
    for i in 1..=a.len() {
        let (current, previous, before_previous) = (i % 3, (i + 2) % 3, (i + 1) % 3);
        rows[current][0] = i as u8;
        let mut smallest = rows[current][0];
        for j in 1..=b.len() {
            let substitution = rows[previous][j - 1] + u8::from(a[i - 1] != b[j - 1]);
            let mut edits = substitution
                .min(rows[previous][j] + 1)
                .min(rows[current][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                edits = edits.min(rows[before_previous][j - 2] + 1);
            }
            rows[current][j] = edits;
            smallest = smallest.min(edits);
        }
        if usize::from(smallest) > max {
            return false;
        }
    }
    usize::from(rows[a.len() % 3][b.len()]) <= max
}

/// Matches the notes that contain every word, and scores them.
///
/// Tantivy's own queries cannot tell an exact from a prefix from a fuzzy match once
/// they are combined, and they score by term statistics. This query walks the term
/// dictionaries itself: per word and field it notes for every document the best kind
/// of match, which is all the ranking needs.
#[derive(Clone, Debug)]
struct WordsQuery {
    words: Arc<[WordTerm]>,
    fields: Arc<Fields>,
}

impl Query for WordsQuery {
    fn weight(&self, _: EnableScoring<'_>) -> tantivy::Result<Box<dyn Weight>> {
        let mut automata = Vec::with_capacity(self.words.len());
        for word in self.words.iter() {
            let mut forms = Vec::with_capacity(word.forms.len());
            for form in &word.forms {
                let fuzzy = if word.distance > 0 {
                    let term = Term::from_field_text(self.fields.body, form);
                    Some(FuzzyTermQuery::new(term, word.distance, true).automaton()?)
                } else {
                    None
                };
                forms.push(fuzzy);
            }
            automata.push(forms);
        }
        Ok(Box::new(WordsWeight {
            words: Arc::clone(&self.words),
            automata,
            fields: Arc::clone(&self.fields),
        }))
    }
}

struct WordsWeight {
    words: Arc<[WordTerm]>,
    /// Per word and form, the automaton accepting its typos, if it tolerates any.
    automata: Vec<Vec<Option<DfaWrapper>>>,
    fields: Arc<Fields>,
}

impl WordsWeight {
    /// Notes in `kinds`, for every document of the segment, the best kind of match of
    /// one form of a word in one field.
    fn mark_matches(
        &self,
        segment: &SegmentReader,
        (field, grams): (Field, Option<Field>),
        word: &WordTerm,
        form: &str,
        fuzzy: Option<&DfaWrapper>,
        mut mark: impl FnMut(DocId, u8),
    ) -> tantivy::Result<()> {
        let inverted = segment.inverted_index(field)?;
        let terms = inverted.terms();
        let mut mark_term = |inverted: &InvertedIndexReader, term: &TermInfo, kind: u8| {
            for_each_doc(inverted, term, |doc| mark(doc, kind))
        };

        if let Some(term) = terms.get(form)? {
            mark_term(&inverted, &term, EXACT)?;
        }
        if word.prefix {
            let num_chars = form.chars().count();
            match grams {
                Some(grams) if (MIN_GRAM..=MAX_GRAM).contains(&num_chars) => {
                    let grams = segment.inverted_index(grams)?;
                    if let Some(term) = grams.terms().get(form)? {
                        mark_term(&grams, &term, PREFIX)?;
                    }
                }
                _ => for_each_term_with_prefix(terms, form.as_bytes(), |_, term| {
                    mark_term(&inverted, term, PREFIX)?;
                    Ok(true)
                })?,
            }
        }
        if let Some(fuzzy) = fuzzy {
            let mut similar = terms.search(fuzzy).into_stream()?;
            while similar.advance() {
                // Already found as a prefix; no need to read its documents again.
                if word.prefix && similar.key().starts_with(form.as_bytes()) {
                    continue;
                }
                mark_term(&inverted, similar.value(), FUZZY)?;
            }
        }
        Ok(())
    }
}

impl Weight for WordsWeight {
    fn scorer(&self, segment: &SegmentReader, boost: Score) -> tantivy::Result<Box<dyn Scorer>> {
        let num_docs = segment.max_doc() as usize;
        // The running sum per document; negative infinity once a word is missing.
        let mut scores = vec![0.0; num_docs];
        let mut kinds = vec![[0u8; 4]; num_docs];
        for (word, automata) in self.words.iter().zip(&self.automata) {
            kinds.fill([0; 4]);
            for (slot, target) in text_fields(&self.fields).into_iter().enumerate() {
                for (form, fuzzy) in word.forms.iter().zip(automata) {
                    self.mark_matches(segment, target, word, form, fuzzy.as_ref(), |doc, kind| {
                        let best = &mut kinds[doc as usize][slot];
                        *best = (*best).max(kind);
                    })?;
                }
            }
            for (score, kinds) in scores.iter_mut().zip(&kinds) {
                let best = kinds
                    .iter()
                    .zip(FIELD_WEIGHTS)
                    .map(|(kind, weight)| weight * MATCH_FACTORS[usize::from(*kind)])
                    .fold(Score::NEG_INFINITY, Score::max);
                *score += if best > 0.0 {
                    best
                } else {
                    Score::NEG_INFINITY
                };
            }
        }
        let (docs, scores) = scores
            .into_iter()
            .enumerate()
            .filter(|(_, score)| *score > 0.0)
            .map(|(doc, score)| (doc as DocId, score * boost))
            .unzip();
        Ok(Box::new(ScoredDocs {
            docs,
            scores,
            cursor: 0,
        }))
    }

    fn explain(&self, segment: &SegmentReader, doc: DocId) -> tantivy::Result<Explanation> {
        let mut scorer = self.scorer(segment, 1.0)?;
        if scorer.seek(doc) != doc {
            return Err(TantivyError::InvalidArgument(format!(
                "document {doc} does not contain every word"
            )));
        }
        Ok(Explanation::new(
            "sum over words of the best field weight × match quality",
            scorer.score(),
        ))
    }
}

/// The matching documents of a segment, ascending, with their scores.
struct ScoredDocs {
    docs: Vec<DocId>,
    scores: Vec<Score>,
    cursor: usize,
}

impl DocSet for ScoredDocs {
    fn advance(&mut self) -> DocId {
        self.cursor = (self.cursor + 1).min(self.docs.len());
        self.doc()
    }

    fn seek(&mut self, target: DocId) -> DocId {
        self.cursor += self.docs[self.cursor..].partition_point(|doc| *doc < target);
        self.doc()
    }

    fn doc(&self) -> DocId {
        self.docs.get(self.cursor).copied().unwrap_or(TERMINATED)
    }

    fn size_hint(&self) -> u32 {
        self.docs.len() as u32
    }
}

impl Scorer for ScoredDocs {
    fn score(&mut self) -> Score {
        self.scores.get(self.cursor).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::parse;

    #[test]
    fn edit_distance_counts_swaps_as_one_edit() {
        let cases = [
            ("haus", "haus", 0, true),
            ("haus", "hasu", 1, true),
            ("haus", "maus", 1, true),
            ("haus", "hau", 1, true),
            ("haus", "hauss", 1, true),
            ("haus", "mais", 1, false),
            ("haus", "mais", 2, true),
            ("ausschreibung", "ausschriebung", 1, true),
            ("ausschreibung", "auschriebung", 2, true),
            ("ausschreibung", "auschriebnug", 2, false),
            ("größe", "grüße", 1, true),
            ("", "ab", 2, true),
            ("", "abc", 2, false),
        ];
        for (a, b, max, expected) in cases {
            assert_eq!(
                within_distance(a, b, max),
                expected,
                "{a} ~ {b} within {max}"
            );
            assert_eq!(
                within_distance(b, a, max),
                expected,
                "{b} ~ {a} within {max}"
            );
        }
    }

    #[test]
    fn typo_tolerance_grows_with_the_word() {
        let distance = |word: &str| Matcher::new(&parse(word)).words[0].distance;
        assert_eq!(distance("a"), 0);
        assert_eq!(distance("tee"), 0);
        assert_eq!(distance("haus"), 1);
        assert_eq!(distance("flasche"), 1);
        assert_eq!(distance("flaschen"), 2);
        // Measured on the folded word, the form both sides are compared in.
        assert_eq!(distance("Fuß"), 1);
    }

    #[test]
    fn split_words_only_complete_their_last_part() {
        let matcher = Matcher::new(&parse("e-mail"));
        let prefixes: Vec<(&str, bool)> = matcher
            .words
            .iter()
            .map(|word| (word.forms[0].as_str(), word.prefix))
            .collect();
        assert_eq!(prefixes, [("e", false), ("mail", true)]);
    }

    #[test]
    fn filters_and_pending_filters_are_not_searched_for() {
        assert!(Matcher::new(&parse("#tag folder:arbeit has: #")).is_empty());
        assert!(Matcher::new(&parse("")).is_empty());
        assert!(Matcher::new(&parse("& - \"…\"")).is_empty());
        assert!(!Matcher::new(&parse("#tag milch")).is_empty());
    }

    fn marked<'a>(query: &str, text: &'a str) -> Vec<&'a str> {
        Matcher::new(&parse(query))
            .highlights(text)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn marks_cover_exact_prefix_fuzzy_and_umlaut_matches() {
        let text = "Die Ausschreibung für Einkäufe, Einkaeufe und den Einkauf läuft.";
        assert_eq!(marked("ausschreibung", text), ["Ausschreibung"]);
        assert_eq!(marked("ausschr", text), ["Ausschreibung"]);
        assert_eq!(marked("ausschriebung", text), ["Ausschreibung"]);
        assert_eq!(
            marked("einkäufe", text),
            ["Einkäufe", "Einkaeufe", "Einkauf"]
        );
        assert_eq!(
            marked("einkaeufe", text),
            ["Einkäufe", "Einkaeufe", "Einkauf"]
        );
        assert_eq!(marked("einkauf", text), ["Einkäufe", "Einkauf"]);
        assert_eq!(marked("lauft die", text), ["Die", "läuft"]);
        assert_eq!(marked("xyz", text), Vec::<&str>::new());
    }

    #[test]
    fn marks_cover_phrases_only_where_they_occur_in_order() {
        let text = "den Einkauf machen, nicht machen den Einkauf";
        assert_eq!(marked("\"einkauf machen\"", text), ["Einkauf", "machen"]);
        assert_eq!(
            marked("\"Einkäufe machen\"", "Einkaeufe machen"),
            ["Einkaeufe", "machen"]
        );
        assert_eq!(marked("\"machen\"", text), ["machen", "machen"]);
        assert_eq!(marked("\"mach\"", text), Vec::<&str>::new());
    }

    #[test]
    fn marks_tell_query_terms_apart() {
        let terms: Vec<usize> = Matcher::new(&parse("brot milch \"frische milch\""))
            .marks("Milch und Brot, frische Milch")
            .into_iter()
            .map(|mark| mark.term)
            .collect();
        assert_eq!(terms, [1, 0, 2, 1]);
    }
}
