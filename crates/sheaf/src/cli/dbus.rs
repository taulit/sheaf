//! Talking to `sheaf-searchd` and the app. Every call is best effort: a missing bus or
//! service is an answer, not a reason to stall.

use std::path::Path;
use std::time::Duration;

use sheaf_core::i18n::tr;
use sheaf_searchd::api::{ACTION_OPEN_NOTE, ACTION_QUICK_NOTE, ApplicationProxy, action_parameter};
use zbus::connection::Builder;
use zbus::{Connection, Variant};

use super::error::{CliError, Result};
use crate::searchd::service_for;

/// Long enough for the service to start and index a large folder, short enough that a
/// service that hangs does not stall the command.
const TIMEOUT: Duration = Duration::from_secs(5);

async fn connect() -> zbus::Result<Connection> {
    Builder::session().method_timeout(TIMEOUT).build().await
}

/// Waits until the service's index reflects the notes folder. `false` if there is no
/// service for this folder, in which case the caller reads the folder itself.
pub fn sync_service(notes_dir: &Path) -> bool {
    let synced = async {
        let conn = connect().await?;
        match service_for(&conn, notes_dir).await? {
            Some(service) => service.sync().await.map(|()| true),
            None => Ok(false),
        }
    };
    zbus::block_on(synced).unwrap_or(false)
}

/// Asks the service for this folder to rebuild its index. `false` if there is none.
pub fn reindex_service(notes_dir: &Path) -> bool {
    let reindexed = async {
        let conn = connect().await?;
        match service_for(&conn, notes_dir).await? {
            Some(service) => service.reindex().await.map(|()| true),
            None => Ok(false),
        }
    };
    zbus::block_on(reindexed).unwrap_or(false)
}

pub fn open_note(id: &str) -> Result {
    activate(ACTION_OPEN_NOTE, &action_parameter(&[id, ""]))
}

pub fn quick_note() -> Result {
    activate(ACTION_QUICK_NOTE, &[])
}

fn activate(action: &str, parameter: &[Variant<'_>]) -> Result {
    let call = async {
        let conn = connect().await?;
        ApplicationProxy::new(&conn)
            .await?
            .activate_action(action, parameter, Default::default())
            .await
    };
    zbus::block_on(call).map_err(|err| {
        CliError::AppUnavailable(tr("the app does not answer: {}").replace("{}", &err.to_string()))
    })
}
