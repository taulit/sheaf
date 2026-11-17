//! What the keyboard can ask for: the actions and their keys (plan 03 §5).
//!
//! # Adding an action
//!
//! 1. Name it in the `actions!` list below.
//! 2. Give it a key in [`bind_keys`], in the context of the view that should hear it.
//! 3. Handle it where it belongs with `.on_action(cx.listener(Self::handler))` on an
//!    element inside that key context. Actions of the whole window are handled in
//!    `window.rs`, those of one view in the view.
//!
//! Buttons trigger the same handlers with `window.dispatch_action(Box::new(Action), cx)`.
//!
//! 4. Name it in [`commands`], so that the command palette offers it and the list of
//!    shortcuts shows its keys. A test fails for a key whose action is missing there.

use gpui::{Action, App, KeyBinding, actions};
use sheaf_core::i18n::tr;

actions!(
    sheaf,
    [
        /// Create a note and open it.
        NewNote,
        /// Open the Quick Note window.
        QuickNote,
        /// Leave the note for the gallery.
        Back,
        /// Put the cursor into the search field of the gallery.
        Search,
        /// Pin or unpin the open or selected note.
        TogglePin,
        /// Move the selected note to the trash.
        Trash,
        /// Open the selected note.
        OpenSelected,
        /// Move the selection down, or to the next note in a list.
        SelectNext,
        SelectPrevious,
        /// Move the selection to the card on the left or right.
        SelectLeft,
        SelectRight,
        /// Empty the search field of the gallery.
        ClearSearch,
        /// Switch the gallery between cards and the compact list.
        ToggleView,
        /// Open the sort popover of the gallery.
        ShowSortOptions,
        /// Open the folder popover of the gallery.
        ShowFolders,
        OpenCommandPalette,
        OpenSettings,
        ShowShortcuts,
        CloseWindow,
        /// Store the quick note and close its window.
        QuickNoteDone,
        /// Store the quick note, close its window and show the note in the main window.
        QuickNoteOpen,
        /// Save the gallery's search as a smart folder.
        SaveSearchAsSmartFolder,
        /// Open the `+ ▾` menu of the gallery: new notes from templates.
        ShowTemplates,
    ]
);

/// Key context of the main window; every other context is nested in it.
pub const WINDOW: &str = "Sheaf";
/// Key context of the gallery.
pub const GALLERY: &str = "Gallery";
/// Key context of the note view, around the editor.
pub const NOTE: &str = "Note";
/// Key context of the editor itself, inside [`NOTE`].
const EDITOR: &str = "MarkdownEditor";
/// Key context of the Quick Note window.
pub const QUICK_NOTE: &str = "QuickNote";
/// The editor inside the Quick Note window, where Ctrl+Enter is not the editor's.
const QUICK_NOTE_EDITOR: &str = "QuickNote > MarkdownEditor";

pub fn bind_keys(cx: &mut App) {
    // The editor's keys first, so that ours win where both bind the same key.
    gpui_markdown_editor::init(cx);
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewNote, Some(WINDOW)),
        KeyBinding::new("ctrl-shift-n", QuickNote, Some(WINDOW)),
        KeyBinding::new("ctrl-k", OpenCommandPalette, Some(WINDOW)),
        // The editor binds Ctrl+K to links; in Sheaf it is the palette (plan 03 §5).
        KeyBinding::new("ctrl-k", OpenCommandPalette, Some(EDITOR)),
        KeyBinding::new("ctrl-,", OpenSettings, Some(WINDOW)),
        KeyBinding::new("ctrl-?", ShowShortcuts, Some(WINDOW)),
        KeyBinding::new("ctrl-p", TogglePin, Some(WINDOW)),
        KeyBinding::new("ctrl-q", CloseWindow, Some(WINDOW)),
        // In a note Ctrl+F belongs to the editor's own find bar.
        KeyBinding::new("ctrl-f", Search, Some(GALLERY)),
        KeyBinding::new("delete", Trash, Some(GALLERY)),
        KeyBinding::new("enter", OpenSelected, Some(GALLERY)),
        KeyBinding::new("down", SelectNext, Some(GALLERY)),
        KeyBinding::new("up", SelectPrevious, Some(GALLERY)),
        KeyBinding::new("left", SelectLeft, Some(GALLERY)),
        KeyBinding::new("right", SelectRight, Some(GALLERY)),
        KeyBinding::new("escape", ClearSearch, Some(GALLERY)),
        KeyBinding::new("ctrl-w", Back, Some(NOTE)),
        KeyBinding::new("escape", Back, Some(NOTE)),
        KeyBinding::new("escape", QuickNoteDone, Some(QUICK_NOTE)),
        KeyBinding::new("ctrl-w", QuickNoteDone, Some(QUICK_NOTE)),
        KeyBinding::new("ctrl-enter", QuickNoteOpen, Some(QUICK_NOTE)),
        KeyBinding::new("ctrl-enter", QuickNoteOpen, Some(QUICK_NOTE_EDITOR)),
    ]);
}

/// An action as people see it: in the command palette and the list of shortcuts.
#[derive(Debug)]
pub struct CommandInfo {
    pub action: Box<dyn Action>,
    pub label: String,
    /// Whether the command palette offers it; keys that only move a selection do
    /// not belong there.
    pub in_palette: bool,
}

/// Every action with its name, in the order the palette offers them. The keys come
/// from the keymap, so that what is shown cannot drift from what is bound.
pub fn commands() -> Vec<CommandInfo> {
    let command = |action: Box<dyn Action>, label: String| CommandInfo {
        action,
        label,
        in_palette: true,
    };
    let hidden = |action: Box<dyn Action>, label: String| CommandInfo {
        action,
        label,
        in_palette: false,
    };
    vec![
        command(Box::new(NewNote), tr("New Note")),
        command(Box::new(QuickNote), tr("Quick Note")),
        command(Box::new(ShowTemplates), tr("New Note from Template…")),
        command(Box::new(Search), tr("Search")),
        command(Box::new(TogglePin), tr("Pin or Unpin")),
        command(Box::new(Trash), tr("Move to Trash")),
        command(Box::new(ToggleView), tr("Toggle View")),
        command(Box::new(ShowSortOptions), tr("Sort Notes")),
        command(Box::new(ShowFolders), tr("Show Folders")),
        command(
            Box::new(SaveSearchAsSmartFolder),
            tr("Save Search as Smart Folder"),
        ),
        command(Box::new(Back), tr("Back to Notes")),
        command(Box::new(ShowShortcuts), tr("Keyboard Shortcuts")),
        command(Box::new(OpenSettings), tr("Settings")),
        command(Box::new(CloseWindow), tr("Close Window")),
        hidden(Box::new(OpenCommandPalette), tr("Command Palette")),
        hidden(Box::new(OpenSelected), tr("Open Selected Note")),
        hidden(Box::new(SelectNext), tr("Select Next Note")),
        hidden(Box::new(SelectPrevious), tr("Select Previous Note")),
        hidden(Box::new(SelectLeft), tr("Select Note on the Left")),
        hidden(Box::new(SelectRight), tr("Select Note on the Right")),
        hidden(Box::new(ClearSearch), tr("Clear Search")),
        hidden(Box::new(QuickNoteDone), tr("Close Quick Note")),
        hidden(Box::new(QuickNoteOpen), tr("Open Quick Note in Notes")),
    ]
}
