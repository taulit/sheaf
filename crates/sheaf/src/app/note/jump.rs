//! The matches of the search a note was opened from, to mark them and scroll to the
//! first one.

use std::collections::HashMap;

use gpui_markdown_editor::{Block, BlockId, BlockKind, MarkdownEditor, TextRange};
use sheaf_core::query::{self, ItemKind};
use sheaf_core::Filter;

/// The words, phrases and tags of `query`, as the editor can find them in the text.
fn terms(query: &str) -> Vec<String> {
    query::parse(query)
        .items
        .into_iter()
        .filter_map(|item| match item.kind {
            ItemKind::Word(word) | ItemKind::Phrase(word) => Some(word),
            ItemKind::Filter(Filter::Tag(tag)) => Some(tag),
            _ => None,
        })
        .filter(|term| !term.trim().is_empty())
        .collect()
}

/// Every match of the terms of `query` in `editor`, in document order.
pub fn matches(editor: &MarkdownEditor, query: &str) -> Vec<TextRange> {
    let mut found: Vec<TextRange> = terms(query)
        .iter()
        .flat_map(|term| editor.find(term))
        .collect();
    let mut order = HashMap::new();
    number(editor.document().blocks(), &mut order);
    let position = |range: &TextRange| {
        let block = order.get(&range.start.block).copied().unwrap_or(usize::MAX);
        (block, range.start.offset)
    };
    found.sort_by_key(position);
    found.dedup_by_key(|range| position(range));
    found
}

/// Numbers the blocks and table cells in the order the text reads.
fn number(blocks: &[Block], order: &mut HashMap<BlockId, usize>) {
    for block in blocks {
        let next = order.len();
        order.insert(block.id(), next);
        if let BlockKind::Table(table) = block.kind() {
            for cell in table.rows.iter().flatten() {
                let next = order.len();
                order.insert(cell.id(), next);
            }
        }
        number(block.children(), order);
    }
}

#[cfg(test)]
mod tests {
    use super::terms;

    #[test]
    fn takes_words_phrases_and_tags_but_not_filters() {
        assert_eq!(
            terms(r#"feta "zwei eier" #rezepte folder:Privat has:image"#),
            ["feta", "zwei eier", "rezepte"]
        );
        assert!(terms("").is_empty());
    }
}
