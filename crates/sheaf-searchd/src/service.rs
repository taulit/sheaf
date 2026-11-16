//! The D-Bus face of the service: both interfaces, the hand-over to the app, and the
//! loop that ends the process when it is idle or told to stop.

use std::collections::HashMap;
use std::future::{Future, poll_fn};
use std::path::Path;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use futures_core::Stream;
use sheaf_core::Change;
use sheaf_searchd::api::{
    ACTION_OPEN_NOTE, ACTION_QUICK_NOTE, ACTION_SEARCH, APP_NAME, ApplicationProxy, ERROR_PREFIX,
    NoteRow, SERVICE_NAME, SERVICE_PATH,
};
use zbus::fdo::{DBusProxy, RequestNameFlags};
use zbus::message::{Header, Message};
use zbus::names::{BusName, ErrorName};
use zbus::object_server::SignalEmitter;
use zbus::proxy::CacheProperties;
use zbus::{Connection, DBusError, OwnedVariant, Str, Variant};
use zruntime::lock::Mutex;
use zruntime::signal::{Signal, Signals};
use zruntime::{Event, SharedRuntime, mpmc, unblock};

use crate::library::Library;

const GICON: &str = "io.github.taulit.Sheaf";
const PROVIDER_RESULTS: u32 = 20;

/// State shared by both interfaces and the background tasks.
struct Shared {
    library: Arc<Mutex<Library>>,
    notes_dir: String,
    /// Notified at the start and end of every request, for the idle timer.
    activity: Event,
}

impl Shared {
    /// Runs `f` on the library from a worker thread, after the calls before it.
    async fn locked<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Library) -> T + Send + 'static,
    ) -> T {
        let mut library = self.library.lock_arc().await;
        unblock(move || f(&mut library)).await
    }

    /// [`Shared::locked`] on behalf of a D-Bus caller.
    async fn request<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Library) -> sheaf_core::Result<T> + Send + 'static,
    ) -> Result<T, Failure> {
        let _busy = self.busy();
        self.locked(f).await.map_err(Failure::from)
    }

    fn busy(&self) -> Busy<'_> {
        self.activity.notify(usize::MAX);
        Busy(&self.activity)
    }
}

struct Busy<'a>(&'a Event);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.notify(usize::MAX);
    }
}

/// A D-Bus error named `<ERROR_PREFIX>.<Code>`.
#[derive(Debug)]
struct Failure {
    name: String,
    message: String,
}

impl Failure {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Failure {
            name: format!("{ERROR_PREFIX}.{code}"),
            message: message.into(),
        }
    }

    fn app_unavailable(err: &zbus::Error) -> Self {
        Failure::new("AppUnavailable", format!("cannot reach {APP_NAME}: {err}"))
    }
}

impl From<sheaf_core::Error> for Failure {
    fn from(err: sheaf_core::Error) -> Self {
        Failure::new(&camel_case(err.code()), err.to_string())
    }
}

impl DBusError for Failure {
    fn create_reply(&self, call: &Header<'_>) -> zbus::Result<Message> {
        Message::error(call, self.name()).build(&(self.message.as_str(),))
    }

    fn name(&self) -> ErrorName<'_> {
        // Built from ERROR_PREFIX and a code of letters only.
        ErrorName::from_str_unchecked(&self.name)
    }

    fn description(&self) -> Option<&str> {
        Some(&self.message)
    }
}

/// `not_found` → `NotFound`.
fn camel_case(code: &str) -> String {
    code.split('_')
        .flat_map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase())
                .into_iter()
                .chain(chars)
        })
        .collect()
}

/// Calls `ActivateAction` on the app, which D-Bus starts if it is not running.
async fn activate_action(conn: &Connection, action: &str, params: &[&str]) -> Result<(), Failure> {
    let app = ApplicationProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(|err| Failure::app_unavailable(&err))?;
    let params: Vec<Variant<'_>> = params.iter().map(|param| Variant::from(*param)).collect();
    app.activate_action(action, &params, HashMap::new())
        .await
        .map_err(|err| Failure::app_unavailable(&err))
}

/// `io.github.taulit.Sheaf1`.
struct Notes(Arc<Shared>);

#[zbus::interface(name = "io.github.taulit.Sheaf1")]
impl Notes {
    async fn search(&self, query: String, limit: u32) -> Result<Vec<NoteRow>, Failure> {
        self.0
            .request(move |library| library.search(&query, limit))
            .await
    }

    async fn recent(&self, limit: u32) -> Result<Vec<NoteRow>, Failure> {
        self.0
            .request(move |library| library.search("", limit))
            .await
    }

    async fn create(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        text: String,
        folder: String,
    ) -> Result<String, Failure> {
        let id = self
            .0
            .request(move |library| library.create(&text, &folder))
            .await?;
        emit_changed(&emitter, std::slice::from_ref(&id)).await;
        Ok(id)
    }

    async fn append(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: String,
        text: String,
    ) -> Result<(), Failure> {
        let ids = self
            .0
            .request(move |library| library.append(&id, &text))
            .await?;
        emit_changed(&emitter, &ids).await;
        Ok(())
    }

    async fn open(&self, #[zbus(connection)] conn: &Connection, id: &str) -> Result<(), Failure> {
        let _busy = self.0.busy();
        activate_action(conn, ACTION_OPEN_NOTE, &[id, ""]).await
    }

    async fn quick_note(&self, #[zbus(connection)] conn: &Connection) -> Result<(), Failure> {
        let _busy = self.0.busy();
        activate_action(conn, ACTION_QUICK_NOTE, &[]).await
    }

    /// Changes made a moment ago may still wait in the watcher; comparing the folder
    /// with the index catches them without waiting for it.
    async fn sync(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Failure> {
        let ids = self.0.request(Library::reconcile).await?;
        if !ids.is_empty() {
            emit_changed(&emitter, &ids).await;
        }
        Ok(())
    }

    async fn reindex(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Failure> {
        self.0.request(Library::reindex).await?;
        emit_changed(&emitter, &[]).await;
        Ok(())
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn notes_dir(&self) -> String {
        let _busy = self.0.busy();
        self.0.notes_dir.clone()
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>, ids: &[String]) -> zbus::Result<()>;
}

async fn emit_changed(emitter: &SignalEmitter<'_>, ids: &[String]) {
    if let Err(err) = Notes::changed(emitter, ids).await {
        log::warn!("cannot emit Changed: {err}");
    }
}

/// `org.gnome.Shell.SearchProvider2`.
struct Provider(Arc<Shared>);

impl Provider {
    async fn search(&self, terms: Vec<String>) -> Result<Vec<String>, Failure> {
        let rows = self
            .0
            .request(move |library| library.search(&terms.join(" "), PROVIDER_RESULTS))
            .await?;
        Ok(rows.into_iter().map(|(id, ..)| id).collect())
    }
}

#[zbus::interface(name = "org.gnome.Shell.SearchProvider2")]
impl Provider {
    async fn get_initial_result_set(&self, terms: Vec<String>) -> Result<Vec<String>, Failure> {
        self.search(terms).await
    }

    async fn get_subsearch_result_set(
        &self,
        _previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Result<Vec<String>, Failure> {
        self.search(terms).await
    }

    async fn get_result_metas(
        &self,
        identifiers: Vec<String>,
    ) -> Result<Vec<HashMap<&'static str, OwnedVariant>>, Failure> {
        let metas = self
            .0
            .request(move |library| library.result_metas(&identifiers))
            .await?;
        let text = |value: String| OwnedVariant::from(Str::from(value));
        Ok(metas
            .into_iter()
            .map(|meta| {
                HashMap::from([
                    ("id", text(meta.id)),
                    ("name", text(meta.name)),
                    ("description", text(meta.description)),
                    ("gicon", text(GICON.to_owned())),
                ])
            })
            .collect())
    }

    async fn activate_result(
        &self,
        #[zbus(connection)] conn: &Connection,
        identifier: &str,
        terms: Vec<String>,
        _timestamp: u32,
    ) {
        let _busy = self.0.busy();
        let query = terms.join(" ");
        if let Err(err) = activate_action(conn, ACTION_OPEN_NOTE, &[identifier, &query]).await {
            log::warn!("{}", err.message);
        }
    }

    async fn launch_search(
        &self,
        #[zbus(connection)] conn: &Connection,
        terms: Vec<String>,
        _timestamp: u32,
    ) {
        let _busy = self.0.busy();
        let query = terms.join(" ");
        if let Err(err) = activate_action(conn, ACTION_SEARCH, &[&query]).await {
            log::warn!("{}", err.message);
        }
    }
}

/// Serves the notes folder on the session bus until the service is idle for
/// `idle_timeout` or receives SIGTERM. Returns early, without error, if another
/// instance owns the bus name.
pub async fn serve(
    notes_dir: &Path,
    index_dir: &Path,
    idle_timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = zbus::connection::Builder::session().build().await?;
    let library = Library::open(notes_dir, index_dir)?;
    let shared = Arc::new(Shared {
        library: Arc::new(Mutex::new(library)),
        notes_dir: notes_dir.display().to_string(),
        activity: Event::new(),
    });
    // Taken before the name is claimed, so that requests wait for the first build.
    let startup = shared.library.lock_arc().await;

    let server = conn.object_server();
    server.at(SERVICE_PATH, Notes(shared.clone())).await?;
    server.at(SERVICE_PATH, Provider(shared.clone())).await?;
    match conn
        .request_name_with_flags(SERVICE_NAME, RequestNameFlags::DO_NOT_QUEUE)
        .await
    {
        Ok(_) => {}
        Err(zbus::Error::NameTaken) => {
            log::info!("{SERVICE_NAME} is already owned, exiting");
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    }

    let (changes, batches) = mpmc::unbounded::<Vec<Change>>();
    // Started before the first build, so that nothing written during it is missed.
    let _watch = sheaf_core::watch(notes_dir, move |batch| {
        let _ = changes.try_send(batch);
    })?;
    let emitter = SignalEmitter::new(&conn, SERVICE_PATH)?.into_owned();
    zruntime::spawn({
        let emitter = emitter.clone();
        async move {
            let mut library = startup;
            match unblock(move || library.start()).await {
                Ok(ids) if !ids.is_empty() => emit_changed(&emitter, &ids).await,
                Ok(_) => {}
                Err(err) => log::error!("cannot bring the index up to date: {err}"),
            }
        }
    })
    .detach();
    zruntime::spawn({
        let shared = shared.clone();
        async move {
            while let Ok(mut batch) = batches.recv().await {
                while let Ok(more) = batches.try_recv() {
                    batch.extend(more);
                }
                match shared.locked(move |library| library.apply(batch)).await {
                    Ok(ids) => emit_changed(&emitter, &ids).await,
                    Err(err) => log::error!("cannot index changed notes: {err}"),
                }
            }
        }
    })
    .detach();

    wait_for_exit(&conn, &shared, idle_timeout).await?;
    // Lets a write in progress finish before the process ends.
    drop(shared.library.lock().await);
    Ok(())
}

enum Wake {
    Stop,
    Idle,
    Request,
    App(bool),
}

/// Returns on SIGTERM or SIGINT, when the bus goes away, or once `idle_timeout` passed
/// without a request while the app is not running. Sleeps in between.
async fn wait_for_exit(
    conn: &Connection,
    shared: &Shared,
    idle_timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = SharedRuntime::current()?;
    let mut stop = Signals::new(&runtime, [Signal::Terminate, Signal::Interrupt])?;
    let dbus = DBusProxy::new(conn).await?;
    let mut app_changes = dbus
        .receive_name_owner_changed_with_args(&[(0, APP_NAME)])
        .await?;
    let mut app_running = dbus.name_has_owner(BusName::try_from(APP_NAME)?).await?;
    loop {
        let mut activity = pin!(shared.activity.listen());
        let mut timer = pin!(runtime.sleep(idle_timeout));
        let wake = poll_fn(|cx| {
            if stop.poll_recv(cx).is_ready() {
                return Poll::Ready(Wake::Stop);
            }
            if activity.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Wake::Request);
            }
            if let Poll::Ready(change) = Pin::new(&mut app_changes).poll_next(cx) {
                return Poll::Ready(match change {
                    Some(change) => {
                        Wake::App(change.args().is_ok_and(|args| args.new_owner().is_some()))
                    }
                    None => Wake::Stop,
                });
            }
            if !app_running && timer.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Wake::Idle);
            }
            Poll::Pending
        })
        .await;
        match wake {
            Wake::Stop => return Ok(()),
            Wake::Idle => {
                log::info!("idle for {} s, exiting", idle_timeout.as_secs());
                return Ok(());
            }
            Wake::Request => {}
            Wake::App(running) => app_running = running,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_become_camel_case() {
        assert_eq!(camel_case("not_found"), "NotFound");
        assert_eq!(camel_case("io"), "Io");
        let failure = Failure::from(sheaf_core::Error::NotFound("x".to_owned()));
        assert_eq!(failure.name, "io.github.taulit.Sheaf1.Error.NotFound");
    }
}
