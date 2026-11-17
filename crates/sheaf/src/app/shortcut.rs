//! The global Quick Note shortcut (plan 03 §8): Ctrl+Shift+N bound through the
//! GlobalShortcuts portal while the app runs, as `docs/verified.md` §1 found it
//! possible on GNOME 48 and later.
//!
//! [`listen`] runs on the portal thread of [`portal`](super::portal), after the call to
//! the host Registry that gives the portal the app id. It binds the shortcut once per
//! run; GNOME asks the user once and remembers the answer under Settings, Apps. A
//! press arrives with the activation token the Quick Note window takes the focus with.
//!
//! Where the portal is missing or the user declines, the fallback is a GNOME custom
//! shortcut running `sheaf --quick` (Settings, Keyboard, Custom Shortcuts).

use std::collections::HashMap;

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use async_channel::Sender;
use futures_util::StreamExt as _;
use sheaf_core::i18n::tr;
use zbus::{Connection, OwnedVariant};

/// The id of the shortcut, which GNOME stores the user's choice of keys under.
const QUICK_NOTE: &str = "quick-note";
/// What the app proposes; the user may pick other keys.
const TRIGGER: &str = "CTRL+SHIFT+n";

/// Binds the Quick Note shortcut on `connection` and sends the activation token, if
/// any, of each press to `presses`. Returns when the portal is missing, the shortcut
/// was not bound, or nobody listens any more. Blocks for as long as the user takes to
/// answer GNOME's dialog, so it runs on a thread of its own.
pub async fn listen(connection: Connection, presses: Sender<Option<String>>) -> ashpd::Result<()> {
    let portal = GlobalShortcuts::with_connection(connection).await?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let mut activated = portal.receive_activated().await?;
    let shortcut = NewShortcut::new(QUICK_NOTE, tr("Quick Note")).preferred_trigger(TRIGGER);
    let bound = portal
        .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
        .await?
        .response()?;
    match bound
        .shortcuts()
        .iter()
        .find(|bound| bound.id() == QUICK_NOTE)
    {
        Some(bound) => log::info!("Quick Note is on {}", bound.trigger_description()),
        None => {
            log::info!("the Quick Note shortcut was not bound; `sheaf --quick` remains");
            return Ok(());
        }
    }
    // The session lives as long as this loop: the shortcut works only while it does.
    while let Some(press) = activated.next().await {
        if press.shortcut_id() != QUICK_NOTE {
            continue;
        }
        if presses
            .send(activation_token(press.options()))
            .await
            .is_err()
        {
            break;
        }
    }
    Ok(())
}

/// The token in the options of an `Activated` signal that lets the window take focus.
fn activation_token(options: &HashMap<String, OwnedVariant>) -> Option<String> {
    let token = options.get("activation_token")?.clone();
    String::try_from(token)
        .ok()
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use zbus::Variant;

    use super::*;

    fn options(entries: &[(&str, Variant<'static>)]) -> HashMap<String, OwnedVariant> {
        entries
            .iter()
            .map(|(key, value)| {
                let value = OwnedVariant::try_from(value.clone()).expect("no file descriptors");
                ((*key).to_owned(), value)
            })
            .collect()
    }

    #[test]
    fn the_token_comes_from_the_options() {
        let token = options(&[("activation_token", Variant::from("abc"))]);
        assert_eq!(activation_token(&token).as_deref(), Some("abc"));
        assert_eq!(activation_token(&options(&[])), None);
        let empty = options(&[("activation_token", Variant::from(""))]);
        assert_eq!(activation_token(&empty), None);
        let wrong = options(&[("activation_token", Variant::from(3_u32))]);
        assert_eq!(activation_token(&wrong), None);
    }
}
