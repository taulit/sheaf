//! The app's link to `sheaf-searchd`, the only writer of the on-disk index.
//!
//! Every function here blocks on D-Bus and belongs on a background thread.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use async_channel::Sender;
use futures_util::StreamExt as _;
use sheaf_searchd::api::Sheaf1Proxy;
use zbus::Connection;
use zbus::proxy::CacheProperties;

use crate::searchd;

/// How long a service started by hand may take to appear on the bus.
const START_TIMEOUT: Duration = Duration::from_secs(5);
const START_POLL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub(super) struct ServiceLink {
    connection: Connection,
    notes_dir: PathBuf,
}

impl ServiceLink {
    /// Makes sure the service runs for `notes_dir` and has indexed it, then reports
    /// every `Changed` signal on `changes`. `None` if no service can be reached or it
    /// serves another folder.
    pub fn connect(
        connection: Connection,
        notes_dir: &Path,
        changes: Sender<Vec<String>>,
    ) -> Option<ServiceLink> {
        let link = ServiceLink {
            connection,
            notes_dir: notes_dir.to_owned(),
        };
        let ready = zbus::block_on(async {
            let service = link.running_service().await?;
            service.sync().await.ok()
        });
        ready?;
        link.watch(changes);
        Some(link)
    }

    /// Returns once the index reflects the notes folder. `false` if the service is gone.
    pub fn sync(&self) -> bool {
        zbus::block_on(async {
            match searchd::service_for(&self.connection, &self.notes_dir).await {
                Ok(Some(service)) => service.sync().await.is_ok(),
                _ => false,
            }
        })
    }

    /// The service, started through D-Bus activation where its service file is
    /// installed, else from the binary next to this one.
    async fn running_service(&self) -> Option<Sheaf1Proxy<'_>> {
        match searchd::service_for(&self.connection, &self.notes_dir).await {
            Ok(Some(service)) => return Some(service),
            Ok(None) => {
                log::info!("sheaf-searchd serves another notes folder");
                return None;
            }
            Err(err) => log::info!("sheaf-searchd is not activatable: {err}"),
        }
        let binary = searchd::binary()?;
        let child = Command::new(&binary)
            .env("SHEAF_NOTES_DIR", &self.notes_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(err) => {
                log::warn!("cannot start {}: {err}", binary.display());
                return None;
            }
        };
        // Reaps the service when it exits before the app does.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let attempts = START_TIMEOUT.as_millis() / START_POLL.as_millis();
        for _ in 0..attempts {
            std::thread::sleep(START_POLL);
            if let Ok(Some(service)) = searchd::service_for(&self.connection, &self.notes_dir).await
            {
                return Some(service);
            }
        }
        log::warn!("{} did not appear on the bus", binary.display());
        None
    }

    fn watch(&self, changes: Sender<Vec<String>>) {
        let connection = self.connection.clone();
        let watching = std::thread::Builder::new()
            .name("sheaf-searchd-signals".to_owned())
            .spawn(move || {
                zbus::block_on(async move {
                    let proxy = Sheaf1Proxy::builder(&connection)
                        .cache_properties(CacheProperties::No)
                        .build()
                        .await;
                    let Ok(proxy) = proxy else { return };
                    let Ok(mut signals) = proxy.receive_changed().await else {
                        return;
                    };
                    while let Some(signal) = signals.next().await {
                        let Ok(args) = signal.args() else { continue };
                        if changes.send(args.ids.clone()).await.is_err() {
                            break;
                        }
                    }
                });
            });
        if let Err(err) = watching {
            log::warn!("cannot follow sheaf-searchd: {err}");
        }
    }
}
