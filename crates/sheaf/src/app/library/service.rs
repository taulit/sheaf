//! The app's link to `sheaf-searchd`, the only writer of the on-disk index.
//!
//! Every function of [`ServiceLink`] blocks on D-Bus and belongs on a background
//! thread; [`Syncer`] asks the service to look at the folder without blocking.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_channel::Sender;
use async_io::Timer;
use futures_util::StreamExt as _;
use futures_util::future::{self, Either};
use sheaf_searchd::api::Sheaf1Proxy;
use zbus::Connection;
use zbus::proxy::CacheProperties;

use crate::searchd;

/// How long a service started by hand may take to appear on the bus.
const START_TIMEOUT: Duration = Duration::from_secs(5);
const START_POLL: Duration = Duration::from_millis(50);
/// How long the service may take to confirm that it looked at the folder after a
/// write; it sees the change through its watcher anyway.
const SYNC_TIMEOUT: Duration = Duration::from_secs(2);

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

    /// Returns once the index reflects the notes folder, or after [`SYNC_TIMEOUT`].
    /// `false` if the service is gone or did not answer in time.
    pub fn sync(&self) -> bool {
        zbus::block_on(async {
            let synced = Box::pin(async {
                match searchd::service_for(&self.connection, &self.notes_dir).await {
                    Ok(Some(service)) => service.sync().await.is_ok(),
                    _ => false,
                }
            });
            match future::select(synced, Timer::after(SYNC_TIMEOUT)).await {
                Either::Left((synced, _)) => synced,
                Either::Right(_) => false,
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

    /// Hands this link to a [`Syncer`] that reports on `confirmed`.
    pub fn into_syncer(self, confirmed: Sender<u64>) -> Syncer {
        Syncer::new(move || self.sync(), confirmed)
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

/// Runs `sync` on a thread of its own whenever the app wrote a note: a write never
/// waits for the service, and requests made while one waits become one. Writes are
/// numbered; after each sync the service confirmed, the number of the last write it
/// covers goes to `confirmed`.
#[derive(Debug)]
pub(super) struct Syncer {
    requests: Sender<()>,
    /// The number of the last write a sync was asked for.
    latest: Arc<AtomicU64>,
}

impl Syncer {
    pub fn new(sync: impl Fn() -> bool + Send + 'static, confirmed: Sender<u64>) -> Syncer {
        // Room for one request: one waiting while a sync runs covers all writes since.
        let (requests, pending) = async_channel::bounded(1);
        let latest = Arc::new(AtomicU64::new(0));
        let spawned = std::thread::Builder::new()
            .name("sheaf-searchd-sync".to_owned())
            .spawn({
                let latest = latest.clone();
                move || {
                    while pending.recv_blocking().is_ok() {
                        // Every write up to here is on disk before the sync starts.
                        let upto = latest.load(Ordering::SeqCst);
                        if sync() {
                            let _ = confirmed.send_blocking(upto);
                        } else {
                            log::warn!("sheaf-searchd did not confirm the change");
                        }
                    }
                }
            });
        if let Err(err) = spawned {
            log::warn!("cannot ask sheaf-searchd to follow changes: {err}");
        }
        Syncer { requests, latest }
    }

    /// Asks for a sync that covers write number `write`, unless one is waiting.
    pub fn request(&self, write: u64) {
        self.latest.fetch_max(write, Ordering::SeqCst);
        let _ = self.requests.try_send(());
    }
}
