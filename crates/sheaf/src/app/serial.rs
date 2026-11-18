//! [`Serial`]: jobs that run one after another, in the order they were given, on the
//! background executor. For the writes of a settings file, where a later change of a
//! key must not be overtaken by an earlier one, and where a write must happen even if
//! the caller drops its task.

use std::fmt;

use async_channel::Sender;
use gpui::{App, AppContext as _, Task};

type Job = Box<dyn FnOnce() + Send>;

#[derive(Clone)]
pub struct Serial {
    jobs: Sender<Job>,
}

impl fmt::Debug for Serial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Serial").finish_non_exhaustive()
    }
}

impl Serial {
    pub fn new(cx: &App) -> Self {
        let (jobs, queue) = async_channel::unbounded::<Job>();
        cx.background_spawn(async move {
            while let Ok(job) = queue.recv().await {
                job();
            }
        })
        .detach();
        Serial { jobs }
    }

    /// Runs `job` after the jobs given before it. Resolves to `None` if it could not
    /// run because the app is closing.
    pub fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> T + Send + 'static,
        cx: &App,
    ) -> Task<Option<T>> {
        let (done, result) = async_channel::bounded(1);
        let _ = self.jobs.try_send(Box::new(move || {
            let _ = done.try_send(job());
        }));
        cx.background_spawn(async move { result.recv().await.ok() })
    }
}
