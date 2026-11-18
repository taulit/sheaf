//! The Markdown of a note as blocks of styled text, the input of the layout. Parsed
//! with pulldown-cmark and the extensions `sheaf-core` reads notes with.

use pulldown_cmark::{Alignment, Event, LinkType, Options, Parser, Tag, TagEnd};

/// How a piece of text is set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    /// The target of a link that opens in a browser or viewer.
    pub link: Option<String>,
    /// A wiki link: shown like a link, but it points to a note, not to a file.
    pub wiki: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Paragraph(Vec<Span>),
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Quote(Vec<Block>),
    Code(String),
    Table {
        align: Vec<Align>,
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Image {
        src: String,
        alt: String,
    },
    Rule,
    /// A paragraph that holds only `\newpage` or `\pagebreak`, as in Pandoc.
    PageBreak,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    /// `Some` for a checklist item: whether it is ticked.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// The deepest nesting of quotes and lists that is kept: deeper ones go on at this
/// level. Keeps the layout's recursion short and leaves the text room on the page.
pub const MAX_DEPTH: usize = 16;

pub fn parse(markdown: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_WIKILINKS);
    let mut builder = Builder::default();
    for event in Parser::new_ext(markdown, options) {
        builder.event(event);
    }
    builder.finish()
}

/// A block that is still being read.
enum Open {
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Item>,
    },
    Item(Item),
    Table {
        align: Vec<Align>,
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
        row: Vec<Vec<Span>>,
    },
}

/// Where the text being read goes when it ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Inline {
    Paragraph,
    Heading(u8),
    Cell,
}

#[derive(Default)]
struct Builder {
    root: Vec<Block>,
    open: Vec<Open>,
    inline: Option<Inline>,
    spans: Vec<Span>,
    code: Option<String>,
    bold: usize,
    italic: usize,
    strike: usize,
    links: Vec<(Option<String>, bool)>,
    /// Alt text of the image being read, with its source.
    image: Option<(String, String)>,
    /// Quotes and lists open in `open`.
    depth: usize,
    /// Quotes, lists and items open beyond [`MAX_DEPTH`], which are not in `open`:
    /// their content goes to the block open at that depth.
    flattened: usize,
}

impl Builder {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text, false),
            Event::Code(text) => self.text(&text, true),
            Event::Html(text) | Event::InlineHtml(text) => self.text(&text, false),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.text(&text, true),
            Event::FootnoteReference(text) => self.text(&text, false),
            Event::SoftBreak => self.text(" ", false),
            Event::HardBreak => self.text("\n", false),
            Event::Rule => {
                self.close_inline();
                self.push(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if self.flattened == 0
                    && let Some(Open::Item(item)) = self.open.last_mut()
                {
                    item.task = Some(checked);
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        if matches!(tag, Tag::BlockQuote(_) | Tag::List(_) | Tag::Item)
            && (self.flattened > 0 || (self.depth >= MAX_DEPTH && !matches!(tag, Tag::Item)))
        {
            self.close_inline();
            self.flattened += 1;
            return;
        }
        if matches!(tag, Tag::BlockQuote(_) | Tag::List(_)) {
            self.depth += 1;
        }
        match tag {
            Tag::Paragraph => self.begin(Inline::Paragraph),
            Tag::Heading { level, .. } => self.begin(Inline::Heading(level as u8)),
            Tag::BlockQuote(_) => {
                self.close_inline();
                self.open.push(Open::Quote(Vec::new()));
            }
            Tag::CodeBlock(_) => {
                self.close_inline();
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.close_inline();
                self.open.push(Open::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.close_inline();
                self.open.push(Open::Item(Item::default()));
            }
            Tag::Table(align) => {
                self.close_inline();
                let align = align
                    .into_iter()
                    .map(|align| match align {
                        Alignment::Center => Align::Center,
                        Alignment::Right => Align::Right,
                        Alignment::Left | Alignment::None => Align::Left,
                    })
                    .collect();
                self.open.push(Open::Table {
                    align,
                    header: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                });
            }
            Tag::TableCell => self.begin(Inline::Cell),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let wiki = matches!(link_type, LinkType::WikiLink { .. });
                let target = (!wiki && !dest_url.is_empty()).then(|| dest_url.into_string());
                self.links.push((target, wiki));
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.into_string(), String::new()));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        let container = matches!(tag, TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item);
        if container && self.flattened > 0 {
            self.close_inline();
            self.flattened -= 1;
            return;
        }
        if matches!(tag, TagEnd::BlockQuote(_) | TagEnd::List(_)) {
            self.depth = self.depth.saturating_sub(1);
        }
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.close_inline(),
            TagEnd::CodeBlock => {
                if let Some(mut code) = self.code.take() {
                    if code.ends_with('\n') {
                        code.pop();
                    }
                    self.push(Block::Code(code));
                }
            }
            TagEnd::BlockQuote(_) => {
                self.close_inline();
                if let Some(Open::Quote(blocks)) = self.open.pop() {
                    self.push(Block::Quote(blocks));
                }
            }
            TagEnd::List(_) => {
                if let Some(Open::List { start, items }) = self.open.pop() {
                    self.push(Block::List { start, items });
                }
            }
            TagEnd::Item => {
                self.close_inline();
                if let Some(Open::Item(item)) = self.open.pop()
                    && let Some(Open::List { items, .. }) = self.open.last_mut()
                {
                    items.push(item);
                }
            }
            TagEnd::TableCell => {
                let spans = std::mem::take(&mut self.spans);
                self.inline = None;
                if let Some(Open::Table { row, .. }) = self.open.last_mut() {
                    row.push(spans);
                }
            }
            TagEnd::TableHead => {
                if let Some(Open::Table { header, row, .. }) = self.open.last_mut() {
                    *header = std::mem::take(row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Open::Table { rows, row, .. }) = self.open.last_mut() {
                    rows.push(std::mem::take(row));
                }
            }
            TagEnd::Table => {
                if let Some(Open::Table {
                    align,
                    header,
                    rows,
                    ..
                }) = self.open.pop()
                {
                    self.push(Block::Table {
                        align,
                        header,
                        rows,
                    });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                if let Some((src, alt)) = self.image.take() {
                    // An image stands on its own: the text around it goes on in a
                    // paragraph of its own.
                    let inline = self.inline;
                    self.close_inline();
                    self.push(Block::Image { src, alt });
                    if let Some(inline @ (Inline::Paragraph | Inline::Heading(_))) = inline {
                        self.inline = Some(inline);
                    }
                }
            }
            _ => {}
        }
    }

    fn begin(&mut self, inline: Inline) {
        self.close_inline();
        self.inline = Some(inline);
    }

    fn text(&mut self, text: &str, code: bool) {
        if let Some(block) = &mut self.code {
            block.push_str(text);
            return;
        }
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if self.inline.is_none() {
            // Text right inside a tight list item, without a paragraph.
            self.inline = Some(Inline::Paragraph);
        }
        let (link, wiki) = self.links.last().cloned().unwrap_or((None, false));
        let style = Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
            link,
            wiki,
        };
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => self.spans.push(Span {
                text: text.to_owned(),
                style,
            }),
        }
    }

    /// Ends the paragraph or heading being read.
    fn close_inline(&mut self) {
        let Some(inline) = self.inline else { return };
        if inline == Inline::Cell {
            return;
        }
        self.inline = None;
        let spans = std::mem::take(&mut self.spans);
        if spans.iter().all(|span| span.text.trim().is_empty()) {
            return;
        }
        let block = match inline {
            Inline::Heading(level) => Block::Heading { level, spans },
            _ if is_page_break(&spans) => Block::PageBreak,
            _ => Block::Paragraph(spans),
        };
        self.push(block);
    }

    fn push(&mut self, block: Block) {
        match self.open.last_mut() {
            Some(Open::Quote(blocks)) => blocks.push(block),
            Some(Open::Item(item)) => item.blocks.push(block),
            // A block right inside a list or table cannot happen in CommonMark.
            Some(Open::List { .. } | Open::Table { .. }) | None => self.root.push(block),
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.close_inline();
        while let Some(open) = self.open.pop() {
            let block = match open {
                Open::Quote(blocks) => Block::Quote(blocks),
                Open::List { start, items } => Block::List { start, items },
                Open::Item(item) => Block::List {
                    start: None,
                    items: vec![item],
                },
                Open::Table {
                    align,
                    header,
                    rows,
                    ..
                } => Block::Table {
                    align,
                    header,
                    rows,
                },
            };
            self.push(block);
        }
        self.root
    }
}

fn is_page_break(spans: &[Span]) -> bool {
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    matches!(text.trim(), "\\newpage" | "\\pagebreak")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn reads_every_kind_of_block() {
        let blocks = parse(
            "Titel\n\n## Teil\n\nEin **fetter** und *schräger* Satz mit `code` und [Link](https://example.org).\n\n\
             - eins\n- zwei\n\n1. erstens\n\n- [ ] offen\n- [x] erledigt\n\n> Zitat\n\n```rust\nfn main() {}\n```\n\n\
             | A | B |\n|---|--:|\n| 1 | 2 |\n\n![Bild](_attachments/x/a.png)\n\n---\n\n\\newpage\n\n[[Andere Notiz]]\n",
        );
        assert!(matches!(&blocks[0], Block::Paragraph(spans) if plain(spans) == "Titel"));
        assert!(matches!(&blocks[1], Block::Heading { level: 2, .. }));
        let Block::Paragraph(spans) = &blocks[2] else {
            panic!("{:?}", blocks[2]);
        };
        assert!(
            spans
                .iter()
                .any(|span| span.style.bold && span.text == "fetter")
        );
        assert!(spans.iter().any(|span| span.style.italic));
        assert!(
            spans
                .iter()
                .any(|span| span.style.code && span.text == "code")
        );
        assert!(
            spans
                .iter()
                .any(|span| span.style.link.as_deref() == Some("https://example.org"))
        );
        assert!(matches!(&blocks[3], Block::List { start: None, items } if items.len() == 2));
        assert!(matches!(&blocks[4], Block::List { start: Some(1), .. }));
        let Block::List { items, .. } = &blocks[5] else {
            panic!("{:?}", blocks[5]);
        };
        assert_eq!(items[0].task, Some(false));
        assert_eq!(items[1].task, Some(true));
        assert!(matches!(&blocks[6], Block::Quote(inner) if inner.len() == 1));
        assert_eq!(blocks[7], Block::Code("fn main() {}".into()));
        let Block::Table {
            align,
            header,
            rows,
        } = &blocks[8]
        else {
            panic!("{:?}", blocks[8]);
        };
        assert_eq!(align, &[Align::Left, Align::Right]);
        assert_eq!(plain(&header[1]), "B");
        assert_eq!(plain(&rows[0][0]), "1");
        assert_eq!(
            blocks[9],
            Block::Image {
                src: "_attachments/x/a.png".into(),
                alt: "Bild".into()
            }
        );
        assert_eq!(blocks[10], Block::Rule);
        assert_eq!(blocks[11], Block::PageBreak);
        let Block::Paragraph(spans) = &blocks[12] else {
            panic!("{:?}", blocks[12]);
        };
        assert!(spans[0].style.wiki && spans[0].style.link.is_none());
        assert_eq!(plain(spans), "Andere Notiz");
    }

    #[test]
    fn an_image_in_a_paragraph_splits_it() {
        let blocks = parse("vorher ![a](b.png) nachher\n");
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert!(matches!(&blocks[1], Block::Image { .. }));
    }

    #[test]
    fn nested_lists_and_quotes_keep_their_children() {
        let blocks = parse("- a\n  - b\n    > c\n");
        let Block::List { items, .. } = &blocks[0] else {
            panic!("{blocks:?}");
        };
        let Block::List { items: inner, .. } = &items[0].blocks[1] else {
            panic!("{items:?}");
        };
        assert!(matches!(inner[0].blocks[1], Block::Quote(_)));
    }

    fn depth(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match block {
                Block::Quote(blocks) => 1 + depth(blocks),
                Block::List { items, .. } => {
                    1 + items
                        .iter()
                        .map(|item| depth(&item.blocks))
                        .max()
                        .unwrap_or(0)
                }
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    fn words(blocks: &[Block]) -> Vec<String> {
        blocks
            .iter()
            .flat_map(|block| match block {
                Block::Paragraph(spans) => vec![plain(spans)],
                Block::Quote(blocks) => words(blocks),
                Block::List { items, .. } => {
                    items.iter().flat_map(|item| words(&item.blocks)).collect()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn deep_nesting_goes_on_at_the_deepest_kept_level() {
        let quotes = format!(
            "{} tief\n\n{}\n\nwieder oben\n",
            ">".repeat(5000),
            "- ".repeat(5000) + "x"
        );
        let blocks = parse(&quotes);
        assert!(depth(&blocks) <= MAX_DEPTH, "{}", depth(&blocks));
        assert_eq!(words(&blocks), ["tief", "x", "wieder oben"]);

        let tasks = format!("{}[x] erledigt\n\n> c\n", "- ".repeat(40));
        let blocks = parse(&tasks);
        assert_eq!(words(&blocks), ["erledigt", "c"]);
        assert!(matches!(blocks.last(), Some(Block::Quote(_))));
    }
}
