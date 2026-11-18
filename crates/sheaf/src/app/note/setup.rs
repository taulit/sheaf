//! How the note view configures the editor: its options, the lookups behind wiki
//! link and tag completion, the resolver of relative image paths, and its style.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use gpui::{App, Edges, FontWeight, Pixels, px};
use gpui_markdown_editor::extensions::{Highlight, Tag, WikiLink};
use gpui_markdown_editor::{
    Completion, CompletionKind, EditorOptions, EditorStyle, SlashItem, SrcResolver, UiParts,
};
use sheaf_core::config::LineWidth;

use crate::app::settings;
use crate::app::theme::ActiveTokens as _;

/// The space beside the text in the column, 04 "Editor".
const SIDE: Pixels = px(48.);

/// The space beside the text that makes the column of the chosen line width, for an
/// editor `width` wide. The editor's column is [`settings::WIDEST_COLUMN`] at most.
pub fn side_padding(width: Pixels, line_width: LineWidth) -> Pixels {
    let room = width.min(settings::WIDEST_COLUMN);
    let extra = ((room - settings::column(line_width)) / 2.).max(Pixels::ZERO);
    SIDE + extra
}

/// More suggestions than a completion list can usefully show.
const COMPLETIONS: usize = 12;

/// What the editor's callbacks look things up in. They run inside the editor without
/// access to the app, so the view keeps this up to date from the [`Library`].
///
/// [`Library`]: crate::app::library::Library
#[derive(Debug, Default)]
pub struct Lookup {
    /// The folder of the note on disk, which relative image paths start from.
    pub folder: PathBuf,
    /// Every note as title and folder, for `[[`.
    pub titles: Vec<(String, String)>,
    /// Every tag with the number of notes that carry it, for `#`.
    pub tags: Vec<(String, usize)>,
}

pub type SharedLookup = Arc<Mutex<Lookup>>;

pub fn options(lookup: &SharedLookup, slash_items: Vec<SlashItem>, ui: UiParts) -> EditorOptions {
    let resolver: SrcResolver = {
        let lookup = lookup.clone();
        Arc::new(move |src| resolve(&lookup, src))
    };
    let completions = {
        let lookup = lookup.clone();
        Arc::new(move |kind, typed: &str| complete(&lookup, kind, typed))
    };
    EditorOptions {
        first_line_is_title: true,
        // Narrower line widths are padding inside it, see `side_padding`.
        max_width: Some(settings::WIDEST_COLUMN),
        extensions: vec![Box::new(WikiLink), Box::new(Tag), Box::new(Highlight)],
        resolve_src: Some(resolver),
        completions: Some(completions),
        slash_items,
        ui,
        ..Default::default()
    }
}

/// An image source as written in the note, as a file. Relative paths start at the
/// note's folder; web addresses are left to the editor.
fn resolve(lookup: &SharedLookup, src: &str) -> Option<PathBuf> {
    if src.contains("://") {
        return None;
    }
    let path = PathBuf::from(src);
    if path.is_absolute() {
        return Some(path);
    }
    let lookup = lookup.lock().unwrap_or_else(PoisonError::into_inner);
    Some(lookup.folder.join(path))
}

/// Notes whose title, and tags whose name, start with what was typed, ignoring case.
fn complete(lookup: &SharedLookup, kind: CompletionKind, typed: &str) -> Vec<Completion> {
    let typed = typed.to_lowercase();
    let lookup = lookup.lock().unwrap_or_else(PoisonError::into_inner);
    match kind {
        CompletionKind::Tag => lookup
            .tags
            .iter()
            .filter(|(tag, _)| tag.to_lowercase().starts_with(&typed))
            .take(COMPLETIONS)
            .map(|(tag, count)| Completion {
                detail: Some(count.to_string().into()),
                ..Completion::new(tag.clone())
            })
            .collect(),
        _ => lookup
            .titles
            .iter()
            .filter(|(title, _)| !title.is_empty() && title.to_lowercase().starts_with(&typed))
            .take(COMPLETIONS)
            .map(|(title, folder)| Completion {
                detail: (!folder.is_empty()).then(|| folder.clone().into()),
                ..Completion::new(title.clone())
            })
            .collect(),
    }
}

/// The editor's style in the colours and at the text scale of the theme, with the
/// measures of 04 "Editor".
pub fn style(cx: &App) -> EditorStyle {
    let tokens = cx.tokens();
    let mut style = EditorStyle::from_theme(cx);
    style.font_family = settings::font_family(settings::config(cx).editor.font, cx);
    style.text_color = tokens.fg;
    style.muted_color = tokens.muted;
    style.faint_color = tokens.faint;
    // The date line above the editor takes the top margin of 04.
    style.padding = Edges {
        top: px(0.),
        right: SIDE,
        bottom: px(120.),
        left: SIDE,
    };
    style.text_size = tokens.text(15.5);
    style.line_height = 1.6;
    style.title_size = tokens.text(28.);
    style.title_weight = FontWeight::EXTRA_BOLD;
    style.h2_size = tokens.text(19.);
    style.h2_weight = FontWeight::BOLD;
    style.h3_size = tokens.text(16.);
    style.heading_line_height = 1.2;
    style.code_size = tokens.text(14.);
    style.code_background = tokens.hover;
    style.quote_bar_color = tokens.accent;
    style.checkbox_size = px(21.);
    style.checkbox_border_color = tokens.faint;
    style.checkbox_checked_color = tokens.accent;
    style.checkbox_check_color = tokens.on_accent;
    style.table_text_size = tokens.text(14.5);
    // The prototype's cells have the browser's "normal" line height.
    style.table_line_height = 1.2;
    style.table_border_color = tokens.hair;
    style.table_header_background = tokens.hover;
    style.link_color = tokens.accent;
    style.tag_color = tokens.accent;
    style.highlight_color = tokens.accent.opacity(0.25);
    style.selection_color = tokens.accent.opacity(0.4);
    style.caret_color = tokens.accent;
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup() -> SharedLookup {
        Arc::new(Mutex::new(Lookup {
            folder: PathBuf::from("/notes/Privat"),
            titles: vec![
                ("Shakshuka".into(), "Privat/Rezepte".into()),
                ("Sprint Review".into(), "Arbeit".into()),
                ("Einkauf".into(), String::new()),
            ],
            tags: vec![("rezepte".into(), 1), ("reise".into(), 3)],
        }))
    }

    #[test]
    fn completes_titles_and_tags_by_prefix() {
        let lookup = lookup();
        let titles = complete(&lookup, CompletionKind::WikiLink, "s");
        let labels: Vec<&str> = titles.iter().map(|c| c.label.as_ref()).collect();
        assert_eq!(labels, ["Shakshuka", "Sprint Review"]);
        assert_eq!(titles[0].detail.as_deref(), Some("Privat/Rezepte"));
        assert_eq!(titles[0].insert.as_ref(), "Shakshuka");
        assert!(complete(&lookup, CompletionKind::WikiLink, "Review").is_empty());

        let tags = complete(&lookup, CompletionKind::Tag, "RE");
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[1].detail.as_deref(), Some("3"));
    }

    #[test]
    fn narrower_lines_are_padding_in_the_widest_column() {
        let text = |width: f32, line| {
            f32::from(px(width).min(settings::WIDEST_COLUMN) - side_padding(px(width), line) * 2.)
        };
        assert_eq!(text(1200., LineWidth::Medium), 700. - 96.);
        assert_eq!(text(1200., LineWidth::Narrow), 600. - 96.);
        assert_eq!(text(1200., LineWidth::Wide), 860. - 96.);
        assert_eq!(text(760., LineWidth::Medium), 700. - 96.);
        assert_eq!(
            text(500., LineWidth::Medium),
            500. - 96.,
            "narrow windows keep 48 px"
        );
    }

    #[test]
    fn resolves_relative_sources_against_the_folder() {
        let lookup = lookup();
        assert_eq!(
            resolve(&lookup, "_attachments/x/a.png"),
            Some(PathBuf::from("/notes/Privat/_attachments/x/a.png"))
        );
        assert_eq!(
            resolve(&lookup, "/tmp/b.png"),
            Some(PathBuf::from("/tmp/b.png"))
        );
        assert_eq!(resolve(&lookup, "https://example.org/c.png"), None);
    }
}
