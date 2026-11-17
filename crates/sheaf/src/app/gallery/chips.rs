//! The search field as a model: filters that are typed out become chips in front of
//! the text, the rest stays text. Pure; the spans come from [`sheaf_core::query`].

use sheaf_core::i18n::tr;
use sheaf_core::query::{self, ItemKind};
use sheaf_core::{Feature, Filter, Period};

/// Takes the filters out of `text` that are complete, meaning a blank follows them,
/// so that `#re` stays text while it is typed and `#rezepte ` becomes a chip. With
/// `all`, every filter is taken: for a search that arrives whole. Returns the filters
/// and what remains of the text.
pub fn take(text: &str, all: bool) -> (Vec<Filter>, String) {
    let mut filters = Vec::new();
    let mut rest = String::with_capacity(text.len());
    let mut position = 0;
    for item in query::parse(text).items {
        let ItemKind::Filter(filter) = item.kind else {
            continue;
        };
        let after = &text[item.span.end..];
        if !all && !after.starts_with(char::is_whitespace) {
            continue;
        }
        rest.push_str(&text[position..item.span.start]);
        filters.push(filter);
        position = item.span.end;
        // The blank that completed the filter goes with it.
        if let Some(blank) = after.chars().next().filter(|c| c.is_whitespace()) {
            position += blank.len_utf8();
        }
    }
    rest.push_str(&text[position..]);
    if all {
        rest.truncate(rest.trim_end().len());
    }
    (filters, rest)
}

/// The query the chips and the text stand for, in the search language.
pub fn compose(chips: &[Filter], text: &str) -> String {
    let mut parts: Vec<String> = chips.iter().map(Filter::to_string).collect();
    let text = text.trim();
    if !text.is_empty() {
        parts.push(text.to_owned());
    }
    parts.join(" ")
}

/// What a chip says: the tag, the folder's name or the filter in words.
pub fn label(filter: &Filter) -> String {
    match filter {
        Filter::Tag(tag) => format!("#{tag}"),
        Filter::Folder(folder) => folder
            .trim_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned(),
        Filter::Has(Feature::Checklist) => tr("Checklist"),
        Filter::Has(Feature::Image) => tr("Image"),
        Filter::Has(Feature::Table) => tr("Table"),
        Filter::Has(Feature::Attachment) => tr("Attachment"),
        Filter::Open => tr("Open Tasks"),
        Filter::Modified(Period::Today) => tr("Today"),
        Filter::Modified(Period::Week) => tr("This Week"),
        Filter::Modified(Period::Month) => tr("This Month"),
        Filter::Pinned => tr("Pinned"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str) -> Filter {
        Filter::Tag(name.to_owned())
    }

    #[test]
    fn a_filter_becomes_a_chip_once_a_blank_follows_it() {
        assert_eq!(take("#re", false), (vec![], "#re".to_owned()));
        assert_eq!(
            take("#rezepte ", false),
            (vec![tag("rezepte")], String::new())
        );
        assert_eq!(
            take("feta #rezepte ", false),
            (vec![tag("rezepte")], "feta ".to_owned())
        );
        assert_eq!(
            take("hat:bild ordner:Arbeit feta", false),
            (
                vec![
                    Filter::Has(Feature::Image),
                    Filter::Folder("Arbeit".to_owned())
                ],
                "feta".to_owned()
            )
        );
        // Filters still being typed and plain words stay.
        assert_eq!(
            take("has:ch folder: x", false),
            (vec![], "has:ch folder: x".to_owned())
        );
        assert_eq!(
            take("geändert:heute  a", false),
            (vec![Filter::Modified(Period::Today)], " a".to_owned())
        );
    }

    #[test]
    fn a_whole_search_gives_up_every_filter() {
        assert_eq!(
            take("#rezepte", true),
            (vec![tag("rezepte")], String::new())
        );
        assert_eq!(
            take("feta angeheftet: \"rote Paprika\" offen:", true),
            (
                vec![Filter::Pinned, Filter::Open],
                "feta \"rote Paprika\"".to_owned()
            )
        );
    }

    #[test]
    fn chips_and_text_compose_back_into_a_query() {
        let chips = [tag("rezepte"), Filter::Folder("Privat/Rezepte".to_owned())];
        assert_eq!(
            compose(&chips, " feta "),
            "#rezepte folder:Privat/Rezepte feta"
        );
        assert_eq!(compose(&[], "feta"), "feta");
        assert_eq!(compose(&[], "  "), "");
        let (filters, text) = take(&compose(&chips, "feta"), true);
        assert_eq!(
            (filters.as_slice(), text.as_str()),
            (chips.as_slice(), "feta")
        );
    }

    #[test]
    fn chips_speak_words() {
        assert_eq!(label(&tag("rezepte")), "#rezepte");
        assert_eq!(
            label(&Filter::Folder("/Privat/Rezepte/".to_owned())),
            "Rezepte"
        );
        assert_eq!(label(&Filter::Has(Feature::Checklist)), "Checklist");
        assert_eq!(label(&Filter::Modified(Period::Week)), "This Week");
        assert_eq!(label(&Filter::Open), "Open Tasks");
    }
}
