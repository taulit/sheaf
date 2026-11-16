//! A private bus, the real `sheaf-searchd` on a temporary notes folder, and a fake app.

use std::collections::HashMap;
use std::fs;
use std::future::poll_fn;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_core::Stream;
use sheaf_searchd::api::{APP_NAME, APP_PATH, SERVICE_NAME, Sheaf1Proxy};
use tempfile::TempDir;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::{Connection, OwnedVariant};
use zruntime::SharedRuntime;

pub const TIMEOUT: Duration = Duration::from_secs(10);

/// No service directories, so nothing on this bus can be started by D-Bus activation.
const BUS_CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A private `dbus-daemon`, killed on drop.
struct Bus {
    daemon: Child,
    address: String,
}

impl Bus {
    fn start(dir: &Path) -> Option<Bus> {
        let config = dir.join("bus.conf");
        fs::write(&config, BUS_CONFIG).unwrap();
        let spawned = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut daemon = match spawned {
            Ok(daemon) => daemon,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("skipping: dbus-daemon is not installed");
                return None;
            }
            Err(err) => panic!("cannot start dbus-daemon: {err}"),
        };
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Some(Bus {
            daemon,
            address: address.trim().to_owned(),
        })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

/// A temporary notes folder, cache and configuration, a bus, and the service on it.
pub struct Fixture {
    services: Vec<Child>,
    bus: Bus,
    dir: TempDir,
}

impl Fixture {
    /// `None` (and a message) if `dbus-daemon` is missing.
    pub fn new() -> Option<Fixture> {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();
        let bus = Bus::start(dir.path())?;
        Some(Fixture {
            services: Vec::new(),
            bus,
            dir,
        })
    }

    pub fn notes(&self) -> PathBuf {
        self.dir.path().join("notes")
    }

    pub fn cache(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    pub fn write(&self, rel_path: &str, text: &str) {
        let path = self.notes().join(rel_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// The service binary with this fixture's environment and nothing else.
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sheaf-searchd"));
        command
            .env_clear()
            .env("HOME", self.dir.path())
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus.address)
            .env("SHEAF_NOTES_DIR", self.notes())
            .env("XDG_CACHE_HOME", self.cache())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("SHEAF_LOG", "info")
            .stdin(Stdio::null());
        command
    }

    /// Starts a service and returns once it owns its bus name.
    pub async fn start(&mut self, idle_secs: u64) {
        let log = fs::File::create(self.dir.path().join("searchd.log")).unwrap();
        let child = self
            .command()
            .env("SHEAF_SEARCHD_IDLE_SECS", idle_secs.to_string())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.services.push(child);
        let conn = self.connect().await;
        let dbus = DBusProxy::new(&conn).await.unwrap();
        let started = Instant::now();
        while !dbus
            .name_has_owner(BusName::try_from(SERVICE_NAME).unwrap())
            .await
            .unwrap()
        {
            if let Some(status) = self.service().try_wait().unwrap() {
                panic!("sheaf-searchd exited with {status}:\n{}", self.log());
            }
            assert!(started.elapsed() < TIMEOUT, "sheaf-searchd did not start");
            sleep(Duration::from_millis(20)).await;
        }
    }

    pub fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("searchd.log")).unwrap_or_default()
    }

    /// The service started last.
    pub fn service(&mut self) -> &mut Child {
        self.services.last_mut().expect("a started service")
    }

    /// Sends SIGTERM to the service started last and waits for it to exit.
    pub async fn stop(&mut self) -> ExitStatus {
        let pid = self.service().id().to_string();
        let killed = Command::new("kill").args(["-TERM", &pid]).status().unwrap();
        assert!(killed.success());
        self.wait_exit(TIMEOUT)
            .await
            .expect("no exit after SIGTERM")
    }

    /// The exit status of the service started last, if it exits within `timeout`.
    pub async fn wait_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.service().try_wait().unwrap() {
                return Some(status);
            }
            if started.elapsed() >= timeout {
                return None;
            }
            sleep(Duration::from_millis(20)).await;
        }
    }

    pub async fn connect(&self) -> Connection {
        zbus::connection::Builder::address(self.bus.address.as_str())
            .build()
            .await
            .unwrap()
    }

    /// A fake app owning the app's bus name and recording the actions it is asked for.
    pub async fn fake_app(&self) -> (Connection, Calls) {
        let calls = Calls::default();
        let conn = zbus::connection::Builder::address(self.bus.address.as_str())
            .name(APP_NAME)
            .serve_at(APP_PATH, FakeApp(calls.clone()))
            .build()
            .await
            .unwrap();
        (conn, calls)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for service in &mut self.services {
            let _ = service.kill();
            let _ = service.wait();
        }
    }
}

#[zbus::proxy(
    interface = "org.gnome.Shell.SearchProvider2",
    default_service = "io.github.taulit.Sheaf.SearchProvider",
    default_path = "/io/github/taulit/Sheaf/SearchProvider"
)]
pub trait SearchProvider2 {
    fn get_initial_result_set(&self, terms: &[&str]) -> zbus::Result<Vec<String>>;

    fn get_subsearch_result_set(
        &self,
        previous_results: &[&str],
        terms: &[&str],
    ) -> zbus::Result<Vec<String>>;

    fn get_result_metas(
        &self,
        identifiers: &[&str],
    ) -> zbus::Result<Vec<HashMap<String, OwnedVariant>>>;

    fn activate_result(&self, identifier: &str, terms: &[&str], timestamp: u32)
    -> zbus::Result<()>;

    fn launch_search(&self, terms: &[&str], timestamp: u32) -> zbus::Result<()>;
}

/// An action the fake app was asked for, with its string parameters.
pub type Call = (String, Vec<String>);

/// The actions the fake app was asked for.
#[derive(Clone, Debug, Default)]
pub struct Calls(Arc<Mutex<Vec<Call>>>);

impl Calls {
    pub fn take(&self) -> Vec<Call> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct FakeApp(Calls);

#[zbus::interface(name = "org.freedesktop.Application")]
impl FakeApp {
    fn activate(&self, _platform_data: HashMap<String, OwnedVariant>) {}

    fn open(&self, _uris: Vec<String>, _platform_data: HashMap<String, OwnedVariant>) {}

    fn activate_action(
        &self,
        action_name: String,
        parameter: Vec<OwnedVariant>,
        _platform_data: HashMap<String, OwnedVariant>,
    ) {
        let params = parameter
            .into_iter()
            .map(|param| String::try_from(param).unwrap_or_else(|_| "<not a string>".to_owned()))
            .collect();
        self.0.0.lock().unwrap().push((action_name, params));
    }
}

pub async fn notes_proxy(conn: &Connection) -> Sheaf1Proxy<'static> {
    Sheaf1Proxy::new(conn).await.unwrap()
}

pub async fn provider_proxy(conn: &Connection) -> SearchProvider2Proxy<'static> {
    SearchProvider2Proxy::new(conn).await.unwrap()
}

pub async fn sleep(duration: Duration) {
    SharedRuntime::current().unwrap().sleep(duration).await;
}

/// The next item of `stream`, unless `timeout` passes first.
pub async fn next<S: Stream + Unpin>(stream: &mut S, timeout: Duration) -> Option<S::Item> {
    let runtime = SharedRuntime::current().unwrap();
    let item = poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx));
    runtime.timeout(timeout, item).await.ok().flatten()
}

/// Checks `condition` until it gives a value or [`TIMEOUT`] passes.
pub async fn eventually<T>(mut condition: impl AsyncFnMut() -> Option<T>) -> Option<T> {
    let started = Instant::now();
    loop {
        if let Some(value) = condition().await {
            return Some(value);
        }
        if started.elapsed() >= TIMEOUT {
            return None;
        }
        sleep(Duration::from_millis(50)).await;
    }
}

/// The D-Bus name of a method error.
pub fn error_name(err: &zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(name, ..) => name.to_string(),
        other => panic!("not a method error: {other:?}"),
    }
}
