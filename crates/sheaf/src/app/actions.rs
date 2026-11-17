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

use gpui::{App, KeyBinding, actions};

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
        OpenCommandPalette,
        OpenSettings,
        ShowShortcuts,
        CloseWindow,
    ]
);

/// Key context of the main window; every other context is nested in it.
pub const WINDOW: &str = "Sheaf";
/// Key context of the gallery.
pub const GALLERY: &str = "Gallery";
/// Key context of the note view, around the editor.
pub const NOTE: &str = "Note";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewNote, Some(WINDOW)),
        KeyBinding::new("ctrl-shift-n", QuickNote, Some(WINDOW)),
        KeyBinding::new("ctrl-k", OpenCommandPalette, Some(WINDOW)),
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
    ]);
}
