use std::process::{Command, Stdio};

use sheaf_core::i18n::tr;

use super::resolve;
use crate::cli::context::Ctx;
use crate::cli::dbus;
use crate::cli::error::{CliError, Result};
use crate::cli::output::Done;

pub fn open(ctx: &Ctx, reference: &str) -> Result {
    let meta = resolve(ctx, reference)?;
    dbus::open_note(meta.id.as_str())?;
    ctx.emit_note(&meta)
}

pub fn quick(ctx: &Ctx) -> Result {
    dbus::quick_note()?;
    ctx.emit(&Done { ok: true }, |_| String::new())
}

pub fn reindex(ctx: &Ctx) -> Result {
    if !dbus::reindex_service(&ctx.notes_dir) {
        let searchd = crate::searchd::binary().ok_or_else(|| {
            CliError::ServiceUnavailable(tr("sheaf-searchd cannot be reached or found"))
        })?;
        // Its output must not end up in ours: with --json stdout is one document.
        let status = Command::new(&searchd)
            .arg("--reindex")
            .env("SHEAF_NOTES_DIR", &ctx.notes_dir)
            .stdout(Stdio::null())
            .status()
            .map_err(|err| CliError::ServiceUnavailable(format!("{}: {err}", searchd.display())))?;
        if !status.success() {
            return Err(CliError::ServiceUnavailable(format!(
                "{} --reindex exited with {status}",
                searchd.display()
            )));
        }
    }
    ctx.emit(&Done { ok: true }, |_| String::new())
}
