//! "Recently Deleted": what is in the trash, read-only, and deleting it for good.
//! Restoring is [`Library::restore`].

use gpui::{App, AppContext as _, Context, Task};
use sheaf_core::{Note, NoteId, Result, TRASH_MAX_AGE, TrashedNote};

use super::Library;

impl Library {
    /// The notes in the trash, most recently deleted first. Those that have been there
    /// longer than the plan's 30 days are deleted for good first, as `sheaf-searchd`
    /// does when it starts, so that the list never shows them.
    pub fn trashed(&self, cx: &App) -> Task<Result<Vec<TrashedNote>>> {
        let store = self.store.clone();
        let now = self.now().timestamp();
        cx.background_spawn(async move {
            match store.purge_expired(now, TRASH_MAX_AGE) {
                Ok(purged) if !purged.is_empty() => {
                    log::info!("removed {} old notes from the trash", purged.len());
                }
                Ok(_) => {}
                Err(err) => log::warn!("cannot remove old notes from the trash: {err}"),
            }
            store.list_trash()
        })
    }

    /// A note in the trash with its text, to show it without editing.
    pub fn trashed_note(&self, id: &NoteId, cx: &App) -> Task<Result<Note>> {
        let store = self.store.clone();
        let id = id.clone();
        cx.background_spawn(async move {
            let trashed = store
                .list_trash()?
                .into_iter()
                .find(|trashed| trashed.meta.id == id)
                .ok_or_else(|| sheaf_core::Error::NotFound(id.to_string()))?;
            store.load(&trashed.meta.rel_path)
        })
    }

    /// Deletes a note in the trash for good.
    pub fn purge(&mut self, id: &NoteId, cx: &mut Context<Self>) -> Task<Result<()>> {
        let id = id.clone();
        self.run(cx, move |store, touched| {
            store.purge(&id)?;
            touched.tell(id);
            Ok(())
        })
    }

    /// Deletes every note in the trash for good and resolves to how many there were.
    /// One that cannot be deleted stops it and is the error.
    pub fn empty_trash(&mut self, cx: &mut Context<Self>) -> Task<Result<usize>> {
        self.run(cx, move |store, touched| {
            let mut purged = 0;
            for trashed in store.list_trash()? {
                store.purge(&trashed.meta.id)?;
                touched.tell(trashed.meta.id);
                purged += 1;
            }
            Ok(purged)
        })
    }
}
