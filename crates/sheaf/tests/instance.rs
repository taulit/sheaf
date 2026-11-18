//! Single instance on a private bus: a second launch of the real binary hands its
//! request to the process that owns the app's name, and exits.
//!
//! The first instance here is this test process, through the same `instance::start`
//! the app uses, so no window and no GPUI are involved. The session bus of the user
//! is never touched: the bus is a `dbus-daemon` of its own in a temporary directory.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use async_channel::Receiver;
use sheaf::app::instance::{self, Activation, Bus, Instance, Request};
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(10);

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
struct PrivateBus {
    daemon: Child,
    address: String,
    dir: TempDir,
}

impl PrivateBus {
    /// `None` (and a message) if `dbus-daemon` is missing.
    fn start() -> Option<PrivateBus> {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("bus.conf");
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
        Some(PrivateBus {
            daemon,
            address: address.trim().to_owned(),
            dir,
        })
    }

    /// Runs the real binary on this bus with nothing else of the user's environment.
    fn launch(&self, args: &[&str], token: Option<(&str, &str)>) -> Output {
        let home: &Path = self.dir.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_sheaf"));
        command
            .args(args)
            .env_clear()
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("SHEAF_NOTES_DIR", home.join("notes"))
            .env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .stdin(Stdio::null());
        if let Some((name, value)) = token {
            command.env(name, value);
        }
        command.output().unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn next(requests: &Receiver<Activation>) -> Activation {
    let started = Instant::now();
    loop {
        if let Ok(activation) = requests.try_recv() {
            return activation;
        }
        assert!(started.elapsed() < TIMEOUT, "no request arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_forwarded(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{:?}: {stderr}", output.status);
    assert_eq!(stderr, "");
}

#[test]
fn a_second_launch_reaches_the_first_instance_and_exits() {
    let Some(bus) = PrivateBus::start() else {
        return;
    };
    let first = Activation {
        request: Request::Activate,
        token: None,
    };
    let started = instance::start(Bus::Address(&bus.address), &first).unwrap();
    let Instance::Primary {
        connection,
        requests,
    } = started
    else {
        panic!("the first launch on an empty bus is the app");
    };
    assert!(connection.is_some());
    assert!(
        requests.is_empty(),
        "the first launch is not sent to itself"
    );

    // Plain `sheaf`: Activate, with the Wayland token of the launcher.
    assert_forwarded(&bus.launch(&[], Some(("XDG_ACTIVATION_TOKEN", "wayland-token"))));
    assert_eq!(
        next(&requests),
        Activation {
            request: Request::Activate,
            token: Some("wayland-token".to_owned()),
        }
    );

    // `--new` and `--quick` become actions; an X11 startup id serves as the token.
    assert_forwarded(&bus.launch(&["--new"], Some(("DESKTOP_STARTUP_ID", "x11-startup-id"))));
    assert_eq!(
        next(&requests),
        Activation {
            request: Request::NewNote,
            token: Some("x11-startup-id".to_owned()),
        }
    );
    assert_forwarded(&bus.launch(&["--quick"], None));
    assert_eq!(
        next(&requests),
        Activation {
            request: Request::QuickNote,
            token: None,
        }
    );
    // The service flag changes nothing when the app already runs.
    assert_forwarded(&bus.launch(&["--dbus-service"], None));
    assert_eq!(next(&requests).request, Request::Activate);

    // What `sheaf-searchd` and `sheaf open` send: an action with parameters.
    let open = Activation {
        request: Request::OpenNote {
            id: "01JD3K8Z6Q4X7V9B2M5N8P0R3T".to_owned(),
            query: "milch kaufen".to_owned(),
        },
        token: Some("search-token".to_owned()),
    };
    let second = instance::start(Bus::Address(&bus.address), &open).unwrap();
    assert!(matches!(second, Instance::Forwarded));
    assert_eq!(next(&requests), open);

    let uris = Activation {
        request: Request::OpenUris(vec!["file:///home/me/Notes/a%20b.md".to_owned()]),
        token: None,
    };
    let third = instance::start(Bus::Address(&bus.address), &uris).unwrap();
    assert!(matches!(third, Instance::Forwarded));
    assert_eq!(next(&requests), uris);
    assert!(requests.is_empty());

    // Nothing of the launches ended up in a notes folder: they never got that far.
    assert!(!bus.dir.path().join("notes").exists());
}

#[test]
fn without_a_bus_the_launch_is_the_only_instance() {
    let dir = tempfile::tempdir().unwrap();
    let address = format!("unix:path={}", dir.path().join("no-such-bus").display());
    let launch = Activation {
        request: Request::NewNote,
        token: None,
    };
    let started = instance::start(Bus::Address(&address), &launch).unwrap();
    let Instance::Primary {
        connection,
        requests,
    } = started
    else {
        panic!("without a bus there is nobody to forward to");
    };
    assert!(connection.is_none());
    assert!(requests.is_empty());
}

/// GNOME Shell's list of extensions, with these ones on.
struct FakeShell {
    on: Vec<&'static str>,
}

#[zbus::interface(name = "org.gnome.Shell.Extensions")]
impl FakeShell {
    fn get_extension_info(
        &self,
        uuid: &str,
    ) -> std::collections::HashMap<String, zbus::OwnedVariant> {
        if !uuid.contains('@') {
            return std::collections::HashMap::new();
        }
        let state = if self.on.contains(&uuid) { 1.0 } else { 2.0 };
        let value = zbus::OwnedVariant::try_from(zbus::Variant::from(state)).unwrap();
        std::collections::HashMap::from([("state".to_owned(), value)])
    }
}

#[test]
fn the_shortcut_is_left_to_visor_only_with_visor_and_its_module_on() {
    let Some(bus) = PrivateBus::start() else {
        return;
    };
    let ask = |on: Option<Vec<&'static str>>| {
        zbus::block_on(async {
            let app = zbus::connection::Builder::address(bus.address.as_str())
                .build()
                .await
                .unwrap();
            let _shell = match on {
                Some(on) => Some(
                    zbus::connection::Builder::address(bus.address.as_str())
                        .name("org.gnome.Shell")
                        .serve_at("/org/gnome/Shell", FakeShell { on })
                        .build()
                        .await
                        .unwrap(),
                ),
                None => None,
            };
            sheaf::app::shortcut::visor_supplies_it(&app).await
        })
    };
    assert!(!ask(None), "no GNOME Shell");
    assert!(!ask(Some(vec![])));
    assert!(!ask(Some(vec!["visor@taulit.github.io"])));
    assert!(ask(Some(vec![
        "visor@taulit.github.io",
        "sheaf-visor@taulit.github.io"
    ])));
}
