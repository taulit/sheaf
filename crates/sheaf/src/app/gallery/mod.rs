//! The start view: the notes of the library as cards or rows, in groups, with the
//! search field, the filter chips and the folder popover of plan 03 §6.
//!
//! # Interface
//!
//! - The window creates one [`GalleryView`] and keeps it alive for the whole session,
//!   also while a note covers it, so it keeps its scroll position and selection.
//! - It reads and changes notes only through the [`Library`] and reloads on every
//!   [`LibraryEvent`].
//! - It asks the window to open a note by emitting [`GalleryEvent::Open`], reports
//!   a deletion with [`GalleryEvent::Trashed`] for the window's Undo toast and a change
//!   that failed with [`GalleryEvent::Failed`]; creating a
//!   note is the window's `NewNote` action, in [`current_folder`](GalleryView::current_folder).
//! - The window calls [`set_query`](GalleryView::set_query) for a search that came
//!   from outside (GNOME search, `sheaf` on the command line) and
//!   [`toggle_pin`](GalleryView::toggle_pin) for Ctrl+P.
//! - Keys reach it through the [`GALLERY`](super::actions::GALLERY) key context.
//!
//! # Modules
//!
//! | Module | Owns |
//! |---|---|
//! | [`chips`] | the search field as chips and text |
//! | [`dates`] | groups and the dates on cards |
//! | [`layout`] | groups and rows of cards, arrow-key moves |
//! | [`preview`] | the miniature of a note's text |
//! | `render` | header, search field, chip row, popovers, empty states |
//! | `cards` | cards, list rows and their context menu |
//! | [`trash`] | "Recently Deleted": restore, delete for good, the read-only note |
//! | [`smart`] | smart folders: saved searches |
//! | `templates` | the `+ ▾` menu: new notes from templates |
//!
//! Only metadata from the index is needed to show the gallery. The text of a note is
//! read, in the background, when its card is first drawn, and its miniature is kept
//! until the note changes.

mod cards;
pub mod chips;
mod covers;
pub mod dates;
mod folders;
pub mod layout;
pub mod preview;
mod render;
pub mod smart;
mod tags;
mod templates;
pub mod trash;

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    ListAlignment, ListOffset, ListState, Modifiers, RenderImage, Subscription, Task, Window, px,
};
use gpui_component::input::{InputEvent, InputState};
use jiff::Timestamp;
use sheaf_core::config::{FolderView, Sort as SortOrder, View};
use sheaf_core::i18n::tr;
use sheaf_core::query::{self, ItemKind};
use sheaf_core::{Filter, FolderConfig, NoteId, NoteMeta, Snippet, Sort};

use super::actions::{
    ClearSearch, OpenSelected, Search, SelectLeft, SelectNext, SelectPrevious, SelectRight, Trash,
};
use super::dialogs::dialogs;
use super::library::{Folder, Library, LibraryEvent};
use super::note::export::{self, Format, Source};
use super::overlays::Toast;
use super::trace;
use covers::{Covers, Wanted};
use layout::{Layout, Row, Section};
use preview::Preview;

/// More hits than anyone scrolls through; ranked, so the best come first.
const SEARCH_LIMIT: usize = 1000;
/// How long typing may pause before the search runs. Each keystroke restarts it, and
/// a search that is overtaken is dropped.
const SEARCH_DELAY: Duration = Duration::from_millis(30);
/// What a row of cards is about to measure, for the list to estimate its length.
const ROW_HEIGHT_HINT: f32 = 270.;

#[derive(Clone, Debug)]
pub enum GalleryEvent {
    /// Open this note; `query` is the search it was found with, or empty.
    Open { id: NoteId, query: String },
    /// These notes went to the trash; the window offers to undo it.
    Trashed(Vec<NoteId>),
    /// A change failed; the message says why, for a toast.
    Failed(String),
    /// A change worth telling, such as notes restored, for a toast.
    Notice(String),
    /// Create a note from this template and open it.
    NewFromTemplate(String),
    /// Something to tell with an action, such as "Exported" with "Open".
    Toast(Toast),
}

/// Which notes the gallery shows: chosen in the chip row or the folder popover.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    /// A folder with its subfolders, relative to the notes folder.
    Folder(String),
    Tag(String),
    /// "Recently Deleted": the notes in the trash.
    Trash,
    /// A smart folder, by name: what its saved search finds.
    Smart(String),
}

impl Scope {
    fn filter(&self) -> Option<Filter> {
        match self {
            Scope::All => None,
            // Anchored, so that `Privat` does not also find `Arbeit/Privat`.
            Scope::Folder(path) => Some(Filter::Folder(format!("/{path}"))),
            Scope::Tag(tag) => Some(Filter::Tag(tag.clone())),
            Scope::Trash | Scope::Smart(_) => None,
        }
    }

    /// Where view and sort order are remembered; tags share those of all notes.
    fn config_key(&self) -> &str {
        match self {
            Scope::Folder(path) => path,
            Scope::All | Scope::Tag(_) | Scope::Trash | Scope::Smart(_) => "",
        }
    }

    /// The title of the header bar.
    pub fn title(&self) -> String {
        match self {
            Scope::All => tr("All Notes"),
            Scope::Folder(path) => path.rsplit('/').next().unwrap_or(path).to_owned(),
            Scope::Tag(tag) => format!("#{tag}"),
            Scope::Trash => tr("Recently Deleted"),
            Scope::Smart(name) => name.clone(),
        }
    }
}

/// A popover of the header bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Folders,
    Sort,
    /// The `+ ▾` menu of new notes and templates.
    Templates,
}

/// Where the query matched a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marks {
    /// Byte ranges of the title.
    pub title: Vec<Range<usize>>,
    pub snippet: Snippet,
}

/// What a reload found.
struct Listing {
    notes: Vec<NoteMeta>,
    sections: Vec<Section>,
    marks: HashMap<NoteId, Marks>,
}

/// The cards of the frame being drawn, by path, and how many lack their miniature.
#[derive(Debug, Default)]
struct Drawn {
    notes: Vec<String>,
    unready: usize,
}

/// The miniature of a note at one modification time; `None` while it is read.
struct CachedPreview {
    modified: Timestamp,
    preview: Option<Arc<Preview>>,
}

/// The card under the pointer and the one it just left, which still sinks back.
#[derive(Default)]
struct Hover {
    on: Option<NoteId>,
    left: Option<NoteId>,
    epoch: usize,
}

pub struct GalleryView {
    library: Entity<Library>,
    scope: Scope,
    /// The settings of the notes folder; `None` until they are read. Nothing is
    /// changed in them before.
    config: Option<FolderConfig>,
    folders: Vec<Folder>,
    search: Entity<InputState>,
    /// Completed filters, shown as chips in front of the text.
    chips: Vec<Filter>,
    /// The text of the search field, without the chips.
    text: String,
    /// Text for the field that arrived without a window, set on the next render.
    pending_text: Option<String>,
    search_focused: bool,
    notes: Vec<NoteMeta>,
    sections: Vec<Section>,
    marks: HashMap<NoteId, Marks>,
    /// Whether the first listing has arrived, so that no empty state flashes.
    loaded: bool,
    layout: Layout,
    columns: usize,
    /// The width the list had when it was last drawn.
    list_width: Option<gpui::Pixels>,
    list_state: ListState,
    scroll_to_top: bool,
    /// The cursor of the keyboard.
    selected: Option<usize>,
    /// The note a Shift+click extends from: by id, as its index changes with a reload.
    anchor: Option<NoteId>,
    /// Notes picked with Ctrl or Shift; when empty, the selection is the cursor.
    marked: BTreeSet<NoteId>,
    previews: HashMap<NoteId, CachedPreview>,
    /// What the frame being drawn shows, while the startup trace waits for it.
    drawn: Drawn,
    covers: Covers,
    hover: Hover,
    popover: Option<Popover>,
    /// The name field of "New Folder" in the folder popover, while it is open.
    new_folder: Option<Entity<InputState>>,
    /// The name field of a folder being renamed, and its path.
    renaming: Option<(Entity<InputState>, String)>,
    trash: trash::TrashState,
    smart: smart::SmartState,
    tags: tags::TagsState,
    templates: templates::TemplatesState,
    generation: u64,
    focus_handle: FocusHandle,
    /// The pause before a search, while typing.
    debounce: Task<()>,
    loading: Task<()>,
    loading_config: Task<()>,
    saving: Task<()>,
    _subscriptions: [Subscription; 2],
}

impl std::fmt::Debug for GalleryView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GalleryView")
            .field("scope", &self.scope)
            .field("notes", &self.notes.len())
            .field("selected", &self.selected)
            .field("query", &self.query())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<GalleryEvent> for GalleryView {}

impl GalleryView {
    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Search")));
        let subscriptions = [
            cx.subscribe(&library, |this, _, _: &LibraryEvent, cx| this.reload(cx)),
            cx.subscribe_in(&search, window, Self::on_search_event),
        ];
        let mut view = GalleryView {
            library,
            scope: Scope::All,
            config: None,
            folders: Vec::new(),
            search,
            chips: Vec::new(),
            text: String::new(),
            pending_text: None,
            search_focused: false,
            notes: Vec::new(),
            sections: Vec::new(),
            marks: HashMap::new(),
            loaded: false,
            layout: Layout::default(),
            columns: 1,
            list_width: None,
            list_state: ListState::new(0, ListAlignment::Top, px(400.)),
            scroll_to_top: false,
            selected: None,
            anchor: None,
            marked: BTreeSet::new(),
            previews: HashMap::new(),
            drawn: Drawn::default(),
            covers: Covers::default(),
            hover: Hover::default(),
            popover: None,
            new_folder: None,
            renaming: None,
            trash: trash::TrashState::default(),
            smart: smart::SmartState::default(),
            tags: tags::TagsState::default(),
            templates: templates::TemplatesState::default(),
            generation: 0,
            focus_handle: cx.focus_handle(),
            debounce: Task::ready(()),
            loading: Task::ready(()),
            loading_config: Task::ready(()),
            saving: Task::ready(()),
            _subscriptions: subscriptions,
        };
        view.load_config(cx);
        view
    }

    // ---- What is shown -------------------------------------------------------------

    /// The notes shown, in the order shown.
    pub fn notes(&self) -> &[NoteMeta] {
        &self.notes
    }

    /// The groups of [`notes`](Self::notes), in order; search results have no group.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The rows the list draws.
    pub fn rows(&self) -> &[Row] {
        self.layout.rows()
    }

    /// Where the search matched a note, while a search with words is shown.
    pub fn marks(&self, id: &NoteId) -> Option<&Marks> {
        self.marks.get(id)
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// The folder new notes go to: the one shown, or the top of the notes folder.
    pub fn current_folder(&self) -> String {
        match &self.scope {
            Scope::Folder(path) => path.clone(),
            Scope::All | Scope::Tag(_) | Scope::Trash | Scope::Smart(_) => String::new(),
        }
    }

    pub fn folders(&self) -> &[Folder] {
        &self.folders
    }

    pub fn view(&self) -> View {
        self.folder_view().view
    }

    pub fn sort(&self) -> SortOrder {
        self.folder_view().sort
    }

    fn folder_view(&self) -> FolderView {
        self.config
            .as_ref()
            .map(|config| config.view(self.scope.config_key()))
            .unwrap_or_default()
    }

    pub fn popover(&self) -> Option<Popover> {
        self.popover
    }

    /// The scroll position, for the window to prove it survives a note.
    pub fn list_state(&self) -> &ListState {
        &self.list_state
    }

    /// How many miniatures were asked for: one per card that was ever drawn.
    pub fn previews_requested(&self) -> usize {
        self.previews.len()
    }

    // ---- Search --------------------------------------------------------------------

    /// The search as typed: chips and text in the search language.
    pub fn query(&self) -> String {
        chips::compose(&self.chips, &self.text)
    }

    pub fn chips(&self) -> &[Filter] {
        &self.chips
    }

    /// The text of the search field after the chips.
    pub fn search_text(&self) -> &str {
        &self.text
    }

    /// Whether the search field has the keyboard.
    pub fn search_focused(&self) -> bool {
        self.search_focused
    }

    /// Shows the hits of `query` instead of all notes; an empty query shows all. Its
    /// filters become chips.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let (filters, text) = chips::take(query.trim(), true);
        self.chips.clear();
        self.add_chips(filters);
        self.pending_text = Some(text.clone());
        self.text = text;
        self.search_changed(cx);
        self.reload(cx);
    }

    /// Removes the chip at `index`, as its × does.
    pub fn remove_chip(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.chips.len() {
            self.chips.remove(index);
            self.search_changed(cx);
            self.reload(cx);
        }
    }

    /// Drops the chips and the scope and searches the text alone.
    pub fn search_without_filters(&mut self, cx: &mut Context<Self>) {
        self.chips.clear();
        self.scope = Scope::All;
        self.search_changed(cx);
        self.reload(cx);
    }

    fn add_chips(&mut self, filters: Vec<Filter>) {
        for filter in filters {
            if !self.chips.contains(&filter) {
                self.chips.push(filter);
            }
        }
    }

    /// A new search starts at the top with nothing selected.
    fn search_changed(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.anchor = None;
        self.marked.clear();
        self.scroll_to_top = true;
        cx.notify();
    }

    fn on_search_event(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => self.text_changed(state, window, cx),
            InputEvent::PressEnter { .. } => self.open_selected_or_first(cx),
            InputEvent::Focus => {
                self.search_focused = true;
                cx.notify();
            }
            InputEvent::Blur => {
                self.search_focused = false;
                cx.notify();
            }
        }
    }

    /// Turns completed filters into chips and searches after a short pause.
    fn text_changed(
        &mut self,
        state: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = state.read(cx).value().to_string();
        let (filters, rest) = chips::take(&value, false);
        let chipped = !filters.is_empty();
        if chipped {
            self.add_chips(filters);
            let text = rest.clone();
            state.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        if !chipped && rest == self.text {
            return;
        }
        self.text = rest;
        self.search_changed(cx);
        let generation = self.generation;
        self.debounce = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.reload(cx);
                }
            });
        });
    }

    /// Backspace at the start of the field: the last chip goes.
    fn remove_last_chip(&mut self, cx: &mut Context<Self>) {
        if let Some(last) = self.chips.len().checked_sub(1) {
            self.remove_chip(last, cx);
        }
    }

    /// Esc in the field: clears it, and leaves it when it is empty.
    fn escape_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chips.is_empty() && self.text.is_empty() {
            self.focus_handle.focus(window, cx);
            self.search_focused = false;
            cx.notify();
        } else {
            self.clear_search(window, cx);
        }
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chips.is_empty() && self.text.is_empty() {
            return;
        }
        self.chips.clear();
        self.text.clear();
        self.search
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.search_changed(cx);
        self.reload(cx);
    }

    /// Puts the cursor into the search field, as Ctrl+F does.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |state, cx| state.focus(window, cx));
        // The field reports its focus with the next frame; the state is true now.
        self.search_focused = true;
        cx.notify();
    }

    /// A printable key while the gallery itself has the keyboard starts a search.
    fn type_to_search(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }
        let Some(typed) = keystroke.key_char.clone() else {
            return;
        };
        if typed.is_empty() || typed.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return;
        }
        cx.stop_propagation();
        let search = self.search.clone();
        search.update(cx, |state, cx| {
            state.focus(window, cx);
            state.insert(typed, window, cx);
        });
        self.search_focused = true;
        self.text_changed(&search, window, cx);
    }

    // ---- Loading -------------------------------------------------------------------

    /// Reads the folder settings, with the default order of the settings, and lists
    /// again; the scope, search and selection stay.
    pub fn load_config(&mut self, cx: &mut Context<Self>) {
        let config = self.library.read(cx).folder_config(cx);
        self.loading_config = cx.spawn(async move |this, cx| {
            let config = config.await;
            let _ = this.update(cx, |this, cx| {
                match config {
                    Ok(config) => this.config = Some(config),
                    Err(err) => {
                        log::warn!("cannot read the folder settings: {err}");
                        let message = tr("Could not read the folder settings: {}")
                            .replace("{}", &err.to_string());
                        cx.emit(GalleryEvent::Failed(message));
                    }
                }
                this.reload(cx);
            });
        });
    }

    /// Changes the folder settings shown and writes the keys `change` changes, after
    /// the changes before it. Does nothing before the settings were read. If the
    /// write fails, the gallery says so and shows the settings as they are on disk.
    fn change_config(
        &mut self,
        change: impl Fn(&mut FolderConfig) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(config) = &mut self.config else {
            return;
        };
        let before = config.clone();
        change(config);
        let saved = self.library.read(cx).update_folder_config(change, cx);
        let previous = std::mem::replace(&mut self.saving, Task::ready(()));
        self.saving = cx.spawn(async move |this, cx| {
            previous.await;
            if let Err(err) = saved.await {
                log::warn!("cannot save the folder settings: {err}");
                let message =
                    tr("Could not save the folder settings: {}").replace("{}", &err.to_string());
                let _ = this.update(cx, |this, cx| {
                    cx.emit(GalleryEvent::Failed(message));
                    this.config = Some(before);
                    this.relayout();
                    this.load_config(cx);
                });
            }
        });
    }

    /// Lists or searches again with what is set now. A reload that is overtaken by
    /// the next one is dropped.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        if self.scope == Scope::Trash {
            self.reload_trash(generation, cx);
            return;
        }
        let library = self.library.read(cx);
        let mut filters: Vec<Filter> = self.scope.filter().into_iter().collect();
        filters.extend(self.chips.iter().cloned());
        let (smart_filters, smart_words) = self.smart_query();
        filters.extend(smart_filters);
        let text = searchable_text(&format!("{smart_words} {}", self.text));
        let listing: Task<sheaf_core::Result<Listing>> = if text.is_empty() {
            let sort = self.sort();
            let now = library.now();
            let notes = library.notes(filters, index_sort(sort), cx);
            cx.background_spawn(async move {
                let (notes, sections) = layout::group(notes.await?, index_sort(sort), &now);
                Ok(Listing {
                    notes,
                    sections,
                    marks: HashMap::new(),
                })
            })
        } else {
            let found = library.search(&chips::compose(&filters, &text), SEARCH_LIMIT, cx);
            cx.background_spawn(async move {
                let mut marks = HashMap::new();
                let notes: Vec<NoteMeta> = found
                    .await?
                    .hits
                    .into_iter()
                    .map(|hit| {
                        let mark = Marks {
                            title: hit.title_highlights,
                            snippet: hit.snippet,
                        };
                        marks.insert(hit.meta.id.clone(), mark);
                        hit.meta
                    })
                    .collect();
                let sections = vec![Section {
                    group: None,
                    len: notes.len(),
                }];
                Ok(Listing {
                    notes,
                    sections,
                    marks,
                })
            })
        };
        let folders = library.folders(cx);
        self.loading = cx.spawn(async move |this, cx| {
            let (listing, folders) = (listing.await, folders.await);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match folders {
                    Ok(folders) => this.folders = folders,
                    Err(err) => log::warn!("cannot list the folders: {err}"),
                }
                match listing {
                    Ok(listing) => this.apply(listing, cx),
                    Err(err) => log::error!("cannot list the notes: {err}"),
                }
            });
        });
    }

    fn apply(&mut self, listing: Listing, cx: &mut Context<Self>) {
        // The selection and the scroll position follow their notes, not their rows.
        let cursor = self.selected().map(|note| note.id.clone());
        let anchor = self.scroll_anchor();
        let Listing {
            notes,
            sections,
            marks,
        } = listing;
        self.anchor = self
            .anchor
            .take()
            .filter(|id| notes.iter().any(|note| &note.id == id));
        self.selected = cursor
            .and_then(|id| notes.iter().position(|note| note.id == id))
            .or(self.selected.filter(|_| !notes.is_empty()))
            .map(|index| index.min(notes.len().saturating_sub(1)));
        self.marked
            .retain(|id| notes.iter().any(|note| &note.id == id));
        self.notes = notes;
        self.sections = sections;
        self.marks = marks;
        self.loaded = true;
        self.relayout_from(anchor);
        cx.notify();
    }

    /// Cuts the notes into rows for the current width and view, keeping the first
    /// visible note in view.
    fn relayout(&mut self) {
        let anchor = self.scroll_anchor();
        self.relayout_from(anchor);
    }

    /// Where the list is scrolled to, the note at the top and whether the heading
    /// above it is in view: what a new layout scrolls back to.
    fn scroll_anchor(&self) -> Option<(ListOffset, NoteId, bool)> {
        let top = self.list_state.logical_scroll_top();
        let rows = self.layout.rows();
        let note = rows.get(top.item_ix..)?.iter().find_map(|row| match row {
            Row::Notes(range) => self.notes.get(range.start),
            Row::Header(_) => None,
        })?;
        let at_header = matches!(rows.get(top.item_ix), Some(Row::Header(_)));
        Some((top, note.id.clone(), at_header))
    }

    fn relayout_from(&mut self, anchor: Option<(ListOffset, NoteId, bool)>) {
        let columns = match self.view() {
            View::Gallery => self.columns,
            View::List => 1,
        };
        self.layout = Layout::new(&self.sections, columns);
        let count = self.layout.rows().len();
        self.list_state
            .reset_with_uniform_height(count, px(ROW_HEIGHT_HINT));
        let anchor = anchor.filter(|_| !std::mem::take(&mut self.scroll_to_top));
        let offset = anchor
            .and_then(|(top, id, at_header)| {
                let note = self.notes.iter().position(|note| note.id == id)?;
                let row = self.layout.anchor_row(note, at_header)?;
                Some(if row == top.item_ix {
                    top
                } else {
                    ListOffset {
                        item_ix: row,
                        offset_in_item: px(0.),
                    }
                })
            })
            .unwrap_or_default();
        self.list_state.scroll_to(offset);
    }

    /// The cover image at `path`, or `None` while it is decoded in the background.
    fn cover(&mut self, path: &Path, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        let now = cx.background_executor().now();
        let (image, wanted) = self.covers.get(path, now);
        match wanted {
            Wanted::Nothing => {}
            Wanted::Load => self.load_cover(path.to_owned(), cx),
            Wanted::Check => {
                let path = path.to_owned();
                let modified = cx.background_spawn({
                    let path = path.clone();
                    async move { covers::modified(&path) }
                });
                cx.spawn(async move |this, cx| {
                    let modified = modified.await;
                    let _ = this.update(cx, |this, cx| {
                        if this.covers.checked(&path, modified) {
                            this.load_cover(path, cx);
                        }
                    });
                })
                .detach();
            }
        }
        image
    }

    fn load_cover(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let decoded = cx.background_spawn({
            let path = path.clone();
            async move { covers::decode(&path) }
        });
        cx.spawn(async move |this, cx| {
            let (modified, image) = decoded.await;
            let _ = this.update(cx, |this, cx| {
                for dropped in this.covers.loaded(&path, modified, image) {
                    cx.drop_image(dropped, None);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Notes a card of the frame being drawn for the startup trace; `ready` if it shows
    /// its miniature.
    fn count_drawn(&mut self, note: &NoteMeta, ready: bool) {
        if trace::pending(trace::Event::Miniatures) {
            self.drawn.notes.push(note.rel_path.clone());
            self.drawn.unready += usize::from(!ready);
        }
    }

    /// The miniature of `note`, or `None` while it is read in the background.
    fn preview(&mut self, note: &NoteMeta, cx: &mut Context<Self>) -> Option<Arc<Preview>> {
        if let Some(cached) = self.previews.get(&note.id)
            && cached.modified == note.modified
        {
            return cached.preview.clone();
        }
        let (id, modified) = (note.id.clone(), note.modified);
        self.previews.insert(
            id.clone(),
            CachedPreview {
                modified,
                preview: None,
            },
        );
        let body = if self.scope == Scope::Trash {
            self.library.read(cx).trashed_body(&note.rel_path, cx)
        } else {
            self.library.read(cx).body(&id, cx)
        };
        let built = cx.background_spawn(async move {
            let body = body.await.ok().flatten().unwrap_or_default();
            Arc::new(Preview::build(&body))
        });
        cx.spawn(async move |this, cx| {
            let preview = built.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(cached) = this.previews.get_mut(&id)
                    && cached.modified == modified
                {
                    cached.preview = Some(preview);
                    cx.notify();
                }
            });
        })
        .detach();
        None
    }

    // ---- Scope, view, sort ---------------------------------------------------------

    pub fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.popover = None;
        if self.scope == scope {
            cx.notify();
            return;
        }
        self.scope = scope;
        self.search_changed(cx);
        self.reload(cx);
    }

    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.config.is_none() || self.view() == view {
            return;
        }
        let key = self.scope.config_key().to_owned();
        self.change_config(move |config| folder_view(config, &key).view = view, cx);
        self.relayout();
        if let Some(row) = self.selected.and_then(|index| self.layout.row_of(index)) {
            self.list_state.scroll_to_reveal_item(row);
        }
        cx.notify();
    }

    /// Cards become the list and the list becomes cards.
    pub fn toggle_view(&mut self, cx: &mut Context<Self>) {
        let other = match self.view() {
            View::Gallery => View::List,
            View::List => View::Gallery,
        };
        self.set_view(other, cx);
    }

    pub fn set_sort(&mut self, sort: SortOrder, cx: &mut Context<Self>) {
        self.popover = None;
        if self.config.is_some() && self.sort() != sort {
            let key = self.scope.config_key().to_owned();
            self.change_config(move |config| folder_view(config, &key).sort = sort, cx);
            self.scroll_to_top = true;
            self.reload(cx);
        }
        cx.notify();
    }

    /// Opens or closes a popover of the header bar.
    pub fn set_popover(&mut self, popover: Option<Popover>, cx: &mut Context<Self>) {
        if popover == Some(Popover::Folders) {
            self.count_trash(cx);
            self.load_tags(cx);
        }
        if popover == Some(Popover::Templates) {
            self.load_templates(cx);
        }
        self.popover = popover;
        self.new_folder = None;
        self.renaming = None;
        self.stop_smart_edit();
        cx.notify();
    }

    fn create_folder(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.trim().trim_matches('/').to_owned();
        if name.is_empty() {
            return;
        }
        let created = self
            .library
            .update(cx, |library, cx| library.create_folder(&name, cx));
        cx.spawn_in(window, async move |this, cx| match created.await {
            Ok(()) => {
                let _ = this.update(cx, |this, cx| this.set_scope(Scope::Folder(name), cx));
            }
            Err(err) => log::error!("cannot create the folder: {err}"),
        })
        .detach();
        self.new_folder = None;
        cx.notify();
    }

    // ---- Selection -----------------------------------------------------------------

    /// The note under the keyboard cursor.
    pub fn selected(&self) -> Option<&NoteMeta> {
        self.notes.get(self.selected?)
    }

    /// The notes actions apply to, in screen order.
    pub fn selection(&self) -> Vec<&NoteMeta> {
        if self.marked.is_empty() {
            self.selected().into_iter().collect()
        } else {
            self.notes
                .iter()
                .filter(|note| self.marked.contains(&note.id))
                .collect()
        }
    }

    fn is_selected(&self, index: usize) -> bool {
        if self.marked.is_empty() {
            self.selected == Some(index)
        } else {
            self.notes
                .get(index)
                .is_some_and(|note| self.marked.contains(&note.id))
        }
    }

    /// A click on a card: opens it, or with Ctrl toggles it in the selection, with
    /// Shift selects everything from the last clicked one.
    pub fn click(&mut self, index: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        if index >= self.notes.len() {
            return;
        }
        if modifiers.secondary(cx) {
            if self.marked.is_empty()
                && let Some(cursor) = self.selected()
            {
                self.marked.insert(cursor.id.clone());
            }
            let id = self.notes[index].id.clone();
            if !self.marked.remove(&id) {
                self.marked.insert(id);
            }
            self.selected = Some(index);
            self.anchor = Some(self.notes[index].id.clone());
        } else if modifiers.shift {
            let anchor = self
                .anchor
                .as_ref()
                .and_then(|id| self.position(id))
                .or(self.selected)
                .filter(|anchor| *anchor < self.notes.len())
                .unwrap_or(index);
            let range = anchor.min(index)..=anchor.max(index);
            self.marked = self.notes[range]
                .iter()
                .map(|note| note.id.clone())
                .collect();
            self.selected = Some(index);
        } else {
            self.open(index, cx);
        }
        cx.notify();
    }

    /// A click on the card of `id`, which may have moved since the card was drawn.
    fn click_note(&mut self, id: &NoteId, modifiers: Modifiers, cx: &mut Context<Self>) {
        if let Some(index) = self.position(id) {
            self.click(index, modifiers, cx);
        }
    }

    fn position(&self, id: &NoteId) -> Option<usize> {
        self.notes.iter().position(|note| &note.id == id)
    }

    fn select_only(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(note) = self.notes.get(index) else {
            return;
        };
        self.anchor = Some(note.id.clone());
        self.selected = Some(index);
        self.marked.clear();
        cx.notify();
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.scope == Scope::Trash {
            self.open_trashed(index, cx);
            return;
        }
        let Some(note) = self.notes.get(index) else {
            return;
        };
        let id = note.id.clone();
        self.select_only(index, cx);
        cx.emit(GalleryEvent::Open {
            id,
            query: self.query(),
        });
    }

    fn open_selected_or_first(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected.or((!self.notes.is_empty()).then_some(0)) {
            self.open(index, cx);
        }
    }

    fn open_selected(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.confirming().is_some() {
            // Deleting for good takes a click on its button, never Enter.
            self.cancel_confirm(cx);
            return;
        }
        if let Some(index) = self.selected {
            self.open(index, cx);
        }
    }

    fn move_cursor(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let Some(last) = self.notes.len().checked_sub(1) else {
            return;
        };
        let index = match (self.selected, direction) {
            (None, Direction::Down | Direction::Right) => 0,
            (None, Direction::Up | Direction::Left) => last,
            (Some(index), Direction::Right) => (index + 1).min(last),
            (Some(index), Direction::Left) => index.saturating_sub(1),
            (Some(index), Direction::Down) => self.layout.vertical(index, true).unwrap_or(index),
            (Some(index), Direction::Up) => self.layout.vertical(index, false).unwrap_or(index),
        };
        self.select_only(index, cx);
        if let Some(row) = self.layout.row_of(index) {
            self.list_state.scroll_to_reveal_item(row);
        }
    }

    // ---- Actions on the selection --------------------------------------------------

    /// Pins the selected notes, or unpins them if all of them are pinned.
    pub fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        if self.scope == Scope::Trash {
            return;
        }
        let notes: Vec<(NoteId, bool)> = self
            .selection()
            .iter()
            .map(|note| (note.id.clone(), note.pinned))
            .collect();
        let pin = !notes.iter().all(|(_, pinned)| *pinned);
        let changes = notes
            .iter()
            .map(|(id, _)| {
                self.library
                    .update(cx, |library, cx| library.set_pinned(id, pin, cx))
            })
            .collect();
        let message = if pin {
            tr("Could not pin the note: {}")
        } else {
            tr("Could not unpin the note: {}")
        };
        Self::report(changes, message, cx);
    }

    pub fn move_selection_to(&mut self, folder: &str, cx: &mut Context<Self>) {
        let changes = self
            .selection_ids()
            .iter()
            .map(|id| {
                self.library
                    .update(cx, |library, cx| library.move_to(id, folder, cx))
            })
            .collect();
        Self::report(changes, tr("Could not move the note: {}"), cx);
    }

    pub fn duplicate_selection(&mut self, cx: &mut Context<Self>) {
        let changes = self
            .selection_ids()
            .iter()
            .map(|id| {
                self.library
                    .update(cx, |library, cx| library.duplicate(id, cx))
            })
            .collect();
        Self::report(changes, tr("Could not duplicate the note: {}"), cx);
    }

    /// "Export": the selected notes as their files are now, one into a file, several
    /// into a folder, each chosen in the portal's dialog.
    pub fn export_selection(&mut self, format: Format, cx: &mut Context<Self>) {
        let library = self.library.read(cx);
        let loads: Vec<_> = self
            .selection_ids()
            .iter()
            .map(|id| library.load(id, cx))
            .collect();
        if loads.is_empty() {
            return;
        }
        let notes_dir = library.notes_dir().to_owned();
        let zone = library.now().time_zone().clone();
        let dialogs = dialogs(cx);
        let exported = cx.background_spawn(async move {
            let mut sources = Vec::with_capacity(loads.len());
            for load in loads {
                let note = load.await.map_err(|err| err.to_string())?;
                sources.push(Source::of(&note, &notes_dir, &zone));
            }
            export::run(&sources, format, &dialogs)
        });
        cx.spawn(async move |this, cx| {
            if let Some(toast) = export::toast(exported.await) {
                let _ = this.update(cx, |_, cx| cx.emit(GalleryEvent::Toast(toast)));
            }
        })
        .detach();
    }

    /// Waits for `changes` and, if one failed, says why in a toast: `message` with
    /// the reason for `{}`. The Library has looked at the note's file again by then,
    /// so the gallery shows it as it is.
    fn report(
        changes: Vec<Task<sheaf_core::Result<NoteMeta>>>,
        message: String,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let mut failed = None;
            for change in changes {
                if let Err(err) = change.await {
                    log::error!("cannot change a note: {err}");
                    failed.get_or_insert(err.to_string());
                }
            }
            if let Some(reason) = failed {
                let message = message.replace("{}", &reason);
                let _ = this.update(cx, |_, cx| cx.emit(GalleryEvent::Failed(message)));
            }
        })
        .detach();
    }

    /// Moves the selected notes to the trash; the window offers to undo it. If one
    /// could not go, a toast says why instead: an Undo would hide that.
    pub fn trash_selection(&mut self, cx: &mut Context<Self>) {
        let trashing: Vec<_> = self
            .selection_ids()
            .iter()
            .map(|id| self.library.update(cx, |library, cx| library.trash(id, cx)))
            .collect();
        if trashing.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let mut trashed = Vec::new();
            let mut failed = None;
            for task in trashing {
                match task.await {
                    Ok(meta) => trashed.push(meta.id),
                    Err(err) => {
                        log::error!("cannot move a note to the trash: {err}");
                        failed.get_or_insert(err.to_string());
                    }
                }
            }
            let event = match failed {
                Some(reason) => {
                    GalleryEvent::Failed(tr("Could not move to trash: {}").replace("{}", &reason))
                }
                None if trashed.is_empty() => return,
                None => GalleryEvent::Trashed(trashed),
            };
            let _ = this.update(cx, |_, cx| cx.emit(event));
        })
        .detach();
    }

    fn selection_ids(&self) -> Vec<NoteId> {
        self.selection()
            .iter()
            .map(|note| note.id.clone())
            .collect()
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The words and phrases of what is typed, without the filters still being typed:
/// `#re` must not empty the results before it becomes `#rezepte`.
/// How `folder` is shown in `config`, stored for it from now on.
fn folder_view<'a>(config: &'a mut FolderConfig, folder: &str) -> &'a mut FolderView {
    let current = config.view(folder);
    config.folders.entry(folder.to_owned()).or_insert(current)
}

fn searchable_text(text: &str) -> String {
    query::parse(text)
        .items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Word(_) | ItemKind::Phrase(_)))
        .map(|item| &text[item.span.clone()])
        .collect::<Vec<_>>()
        .join(" ")
}

fn index_sort(sort: SortOrder) -> Sort {
    match sort {
        SortOrder::Modified => Sort::Modified,
        SortOrder::Created => Sort::Created,
        SortOrder::Title => Sort::Title,
    }
}

impl GalleryView {
    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(Direction::Down, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(Direction::Up, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(Direction::Left, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(Direction::Right, cx);
    }

    fn trash(&mut self, _: &Trash, _: &mut Window, cx: &mut Context<Self>) {
        if self.scope == Scope::Trash {
            self.ask_purge_selection(cx);
        } else {
            self.trash_selection(cx);
        }
    }

    fn search_action(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_search(window, cx);
    }

    fn clear(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_search(window, cx);
    }
}

impl Focusable for GalleryView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_being_typed_are_not_searched() {
        assert_eq!(searchable_text("feta #re"), "feta");
        assert_eq!(searchable_text("\"rote paprika\" has:"), "\"rote paprika\"");
        assert_eq!(searchable_text("#"), "");
    }

    #[test]
    fn scopes_filter_and_name_themselves() {
        assert_eq!(Scope::All.filter(), None);
        assert_eq!(
            Scope::Folder("Privat/Rezepte".to_owned()).filter(),
            Some(Filter::Folder("/Privat/Rezepte".to_owned()))
        );
        assert_eq!(
            Scope::Folder("Privat/Rezepte".to_owned()).title(),
            "Rezepte"
        );
        assert_eq!(Scope::Tag("rezepte".to_owned()).title(), "#rezepte");
        assert_eq!(Scope::Tag("rezepte".to_owned()).config_key(), "");
    }
}
