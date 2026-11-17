//! The command palette (Ctrl+K): every command of [`actions::commands`] with its keys,
//! and for a typed query the notes the search finds. Enter runs the highlighted entry,
//! Esc clears the query and then closes the palette.
//!
//! The palette does not run anything itself: it tells the window with a
//! [`PaletteEvent`], which closes it, gives the keyboard back to the view that had it
//! and dispatches the action there, so the view's own handlers hear it.

use gpui::{
    Action, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Task, Window, div, px,
};
use gpui_component::IndexPath;
use gpui_component::command::{Command, CommandGroup, CommandItem, CommandState};
use sheaf_core::NoteMeta;
use sheaf_core::i18n::tr;

use crate::app::actions::{self, CommandInfo};
use crate::app::library::Library;
use crate::app::theme::ActiveTokens as _;

/// How many notes the palette lists for a query.
const NOTES: usize = 8;
const WIDTH: f32 = 560.;

#[derive(Debug)]
pub enum PaletteEvent {
    /// Run this action where the keyboard was before the palette opened.
    Run(Box<dyn Action>),
    /// Open this note, which the search found for `query`.
    Open { note: NoteMeta, query: String },
    Dismiss,
}

pub struct CommandPalette {
    state: Entity<CommandState>,
    library: Entity<Library>,
    commands: Vec<CommandInfo>,
    /// The keys of each command, as shown.
    keys: Vec<Option<String>>,
    notes: Vec<NoteMeta>,
    query: String,
    _searching: Task<()>,
}

impl std::fmt::Debug for CommandPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandPalette")
            .field("query", &self.query)
            .field("notes", &self.notes.len())
            .finish_non_exhaustive()
    }
}

impl EventEmitter<PaletteEvent> for CommandPalette {}

impl CommandPalette {
    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        let commands: Vec<CommandInfo> = actions::commands()
            .into_iter()
            .filter(|command| command.in_palette)
            .collect();
        let keys = commands
            .iter()
            .map(|command| {
                let bindings = super::bindings_of(command.action.as_ref(), cx);
                bindings.into_iter().next().map(|(_, keys)| keys)
            })
            .collect();
        CommandPalette {
            state,
            library,
            commands,
            keys,
            notes: Vec::new(),
            query: String::new(),
            _searching: Task::ready(()),
        }
    }

    /// The labels of the entries the palette offers now, commands first.
    pub fn entries(&self) -> Vec<String> {
        let commands = self.commands.iter().map(|command| command.label.clone());
        let notes = self.notes.iter().map(|note| open_label(note));
        commands.chain(notes).collect()
    }

    fn search(&mut self, query: &str, cx: &mut Context<Self>) {
        self.query = query.to_owned();
        if query.trim().is_empty() {
            self.notes.clear();
            self._searching = Task::ready(());
            cx.notify();
            return;
        }
        let found = self.library.read(cx).search(query, NOTES, cx);
        self._searching = cx.spawn(async move |this, cx| {
            let notes = match found.await {
                Ok(results) => results.hits.into_iter().map(|hit| hit.meta).collect(),
                Err(err) => {
                    log::warn!("the palette cannot search: {err}");
                    Vec::new()
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.notes = notes;
                cx.notify();
            });
        });
    }

    fn confirm(&mut self, index: IndexPath, cx: &mut Context<Self>) {
        let event = match index.section {
            0 => self
                .commands
                .get(index.row)
                .map(|command| PaletteEvent::Run(command.action.boxed_clone())),
            _ => self.notes.get(index.row).cloned().map(|note| PaletteEvent::Open {
                note,
                query: self.query.clone(),
            }),
        };
        if let Some(event) = event {
            cx.emit(event);
        }
    }

    fn row(label: SharedString, keys: Option<String>, cx: &gpui::App) -> gpui::Div {
        let tokens = cx.tokens();
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap(px(12.))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .children(keys.map(|keys| {
                div()
                    .flex_none()
                    .text_size(tokens.text(12.5))
                    .text_color(tokens.muted)
                    .child(keys)
            }))
    }
}

fn open_label(note: &NoteMeta) -> String {
    let title = if note.title.is_empty() {
        tr("Untitled")
    } else {
        note.title.clone()
    };
    // TRANSLATORS: an entry of the command palette that opens the note with this title.
    tr("Open note: {title}").replace("{title}", &title)
}

impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.state.focus_handle(cx)
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = cx.tokens().clone();
        let commands = self.commands.iter().zip(&self.keys).map(|(command, keys)| {
            let label = SharedString::from(command.label.clone());
            let keys = keys.clone();
            CommandItem::new()
                .label(label.clone())
                .child(move |_, cx| Self::row(label.clone(), keys.clone(), cx))
        });
        // The search found these for the query; the query itself lets them pass the
        // palette's own filter, which only knows the labels.
        let query = self.query.clone();
        let notes = self.notes.iter().map(|note| {
            let label = SharedString::from(open_label(note));
            CommandItem::new()
                .label(label.clone())
                .keywords([query.clone()])
                .child(move |_, cx| Self::row(label.clone(), None, cx))
        });
        let this = cx.entity().downgrade();
        let confirmed = this.clone();
        let cancelled = this.clone();
        let palette = Command::new(&self.state)
            .group(
                CommandGroup::new()
                    .label(tr("Commands"))
                    .items(commands),
            )
            .group(CommandGroup::new().label(tr("Notes")).items(notes))
            .placeholder(tr("Search commands and notes"))
            .bordered(false)
            .max_h(px(380.))
            .on_query(move |query, _, cx| {
                let query = query.to_owned();
                let _ = this.update(cx, |this, cx| this.search(&query, cx));
            })
            .on_confirm(move |index, _, cx| {
                let _ = confirmed.update(cx, |this, cx| this.confirm(index, cx));
            })
            .on_cancel(move |_, cx| {
                let _ = cancelled.update(cx, |_, cx| cx.emit(PaletteEvent::Dismiss));
            });

        div()
            .id("palette-scrim")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(gpui::black().opacity(0.18))
            .flex()
            .justify_center()
            .pt(px(72.))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)),
            )
            .child(
                super::popover("palette", cx)
                    .w(px(WIDTH))
                    .max_w(gpui::relative(0.9))
                    .h_auto()
                    .text_size(tokens.text(14.))
                    // A click inside must not count as one on the scrim.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(palette),
            )
    }
}
