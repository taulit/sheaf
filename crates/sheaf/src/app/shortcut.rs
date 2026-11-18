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
//!
//! With Visor and its Notes module on, Visor supplies the shortcut (its own keys, then
//! "note") and the portal is left alone (`docs/verified.md` §1, decision 6). GNOME
//! Shell tells which extensions are on through `org.gnome.Shell.Extensions`.

use std::collections::HashMap;

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use async_channel::Sender;
use futures_util::StreamExt as _;
use gpui::Global;
use sheaf_core::i18n::tr;
use zbus::{Connection, OwnedVariant};

/// The id of the shortcut, which GNOME stores the user's choice of keys under.
const QUICK_NOTE: &str = "quick-note";
/// What the app proposes; the user may pick other keys.
const TRIGGER: &str = "CTRL+SHIFT+n";

/// The extensions that make Visor supply the shortcut: Visor and its Notes module.
const VISOR: [&str; 2] = ["visor@taulit.github.io", "sheaf-visor@taulit.github.io"];
/// `ExtensionState.ACTIVE` of GNOME Shell (`ENABLED` before GNOME 45).
const ACTIVE: f64 = 1.;

/// Set once Visor turned out to supply the shortcut; the settings say so then.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FromVisor;

impl Global for FromVisor {}

/// What the shortcut thread tells the app.
#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    /// Visor supplies the shortcut, so it was not bound.
    FromVisor,
    /// It was pressed, with the activation token if any.
    Pressed(Option<String>),
}

/// Whether Visor and its Notes module are on in the GNOME Shell of `connection`. No
/// shell, or an answer that does not say, is `false`.
pub async fn visor_supplies_it(connection: &Connection) -> bool {
    for uuid in VISOR {
        let reply = connection
            .call_method(
                Some("org.gnome.Shell"),
                "/org/gnome/Shell",
                Some("org.gnome.Shell.Extensions"),
                "GetExtensionInfo",
                &(uuid,),
            )
            .await;
        let info =
            reply.and_then(|reply| reply.body().deserialize::<HashMap<String, OwnedVariant>>());
        match info {
            Ok(info) if is_active(&info) => {}
            Ok(_) => return false,
            Err(err) => {
                log::debug!("cannot ask GNOME Shell about {uuid}: {err}");
                return false;
            }
        }
    }
    true
}

/// Whether the app runs inside Flatpak. There the sandbox keeps GNOME Shell out of
/// reach, so Visor is never detected and the portal always binds the shortcut.
pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

/// What the settings say about the shortcut when Visor does not supply it.
pub fn fallback_hint(in_flatpak: bool) -> String {
    if in_flatpak {
        tr(
            "While Notes runs, through GNOME’s shortcut portal, also with Visor installed. Otherwise add a custom shortcut for “flatpak run io.github.taulit.Sheaf --quick” in GNOME Settings, Keyboard",
        )
    } else {
        tr(
            "While Notes runs. Otherwise add a custom shortcut for “sheaf --quick” in GNOME Settings, Keyboard",
        )
    }
}

/// Whether the `GetExtensionInfo` answer of an extension says it is on; an unknown
/// extension's answer is empty.
fn is_active(info: &HashMap<String, OwnedVariant>) -> bool {
    info.get("state")
        .and_then(|state| f64::try_from(state).ok())
        .is_some_and(|state| state == ACTIVE)
}

/// Binds the Quick Note shortcut on `connection`, unless Visor supplies it, and sends
/// what happens to `messages`. Returns when Visor supplies it, the portal is missing,
/// the shortcut was not bound, or nobody listens any more. Blocks for as long as the
/// user takes to answer GNOME's dialog, so it runs on a thread of its own.
pub async fn listen(connection: Connection, messages: Sender<Message>) -> ashpd::Result<()> {
    if visor_supplies_it(&connection).await {
        log::info!("Visor supplies the Quick Note shortcut");
        let _ = messages.send(Message::FromVisor).await;
        return Ok(());
    }
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
        if messages
            .send(Message::Pressed(activation_token(press.options())))
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

    #[test]
    fn inside_flatpak_the_settings_say_the_portal_binds_the_shortcut() {
        assert!(fallback_hint(true).contains("portal"));
        assert!(fallback_hint(true).contains("flatpak run io.github.taulit.Sheaf --quick"));
        assert!(fallback_hint(false).contains("“sheaf --quick”"));
    }

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
    fn an_extension_is_on_in_state_one() {
        let info = |state: Variant<'static>| {
            let value = OwnedVariant::try_from(state).expect("no file descriptors");
            HashMap::from([("state".to_owned(), value)])
        };
        assert!(is_active(&info(Variant::from(1.0_f64))));
        assert!(!is_active(&info(Variant::from(2.0_f64))));
        assert!(!is_active(&info(Variant::from("1"))));
        assert!(!is_active(&HashMap::new()));
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
