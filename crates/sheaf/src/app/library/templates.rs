//! Note templates, `<notes>/.sheaf/templates/*.md` (plan 03 §9). Filling one in is
//! `sheaf_core::templates`, which the CLI's `sheaf new --template` shares.

use std::path::PathBuf;

use gpui::{App, AppContext as _, Context, Task};
use sheaf_core::{CreateOptions, NoteMeta, Result, frontmatter, templates};

use super::{Effect, Library};

impl Library {
    /// The names of the templates, sorted.
    pub fn templates(&self, cx: &App) -> Task<Result<Vec<String>>> {
        let root = self.store.root().to_owned();
        cx.background_spawn(async move { templates::list(&root) })
    }

    /// Creates a note from the template `name`, with `{{date}}` and `{{time}}` taken
    /// from the Library's clock and an empty `{{title}}` for the user to type.
    pub fn create_from_template(
        &mut self,
        name: &str,
        options: CreateOptions,
        cx: &mut Context<Self>,
    ) -> Task<Result<NoteMeta>> {
        let name = name.to_owned();
        let now = self.now();
        self.write(None, Effect::Keep, cx, move |store| {
            let text = templates::note_text(store.root(), &name, "", &now)?;
            store.create(&text, &options)
        })
    }

    /// The text of the template `name` filled in for the note titled `title`, without
    /// its frontmatter, to insert into that note (`/vorlage`).
    pub fn template_text(&self, name: &str, title: &str, cx: &App) -> Task<Result<String>> {
        let root = self.store.root().to_owned();
        let (name, title) = (name.to_owned(), title.to_owned());
        let now = self.now();
        cx.background_spawn(async move {
            let text = templates::render(&root, &name, &title, &now)?;
            Ok(frontmatter::split(&text).body.to_owned())
        })
    }

    /// The templates folder, created if it is missing, for "Edit Templates…".
    pub fn templates_dir(&self, cx: &App) -> Task<Result<PathBuf>> {
        let root = self.store.root().to_owned();
        cx.background_spawn(async move { templates::ensure_dir(&root) })
    }
}
