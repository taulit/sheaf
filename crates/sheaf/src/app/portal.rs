//! The settings portal: reads what the desktop asks for and follows its changes.
//!
//! All D-Bus work happens on a thread of its own; the settings it decodes reach the app
//! through a channel and end up in [`theme::set_appearance`].

use ashpd::desktop::settings::Settings;
use futures_util::StreamExt as _;
use gpui::App;
use sheaf_searchd::api::APP_NAME;
use zbus::{Connection, OwnedVariant};

use super::theme::{self, Appearance};

const APPEARANCE: &str = "org.freedesktop.appearance";
const INTERFACE: &str = "org.gnome.desktop.interface";

/// One setting the theme depends on, decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    /// 1 prefers dark, 2 prefers light, anything else has no preference.
    ColorScheme(u32),
    /// sRGB components in 0..=1; out of range means the desktop has none.
    Accent(f64, f64, f64),
    Contrast(u32),
    ReducedMotion(u32),
    FontName(String),
    TextScaling(f64),
    EnableAnimations(bool),
}

impl Setting {
    /// Decodes the setting `key` of `namespace`. `None` for settings the theme does not
    /// use and for values of an unexpected type.
    pub fn decode(namespace: &str, key: &str, value: &OwnedVariant) -> Option<Setting> {
        let value = value.clone();
        Some(match (namespace, key) {
            (APPEARANCE, "color-scheme") => Setting::ColorScheme(value.try_into().ok()?),
            (APPEARANCE, "accent-color") => {
                let (r, g, b): (f64, f64, f64) = value.try_into().ok()?;
                Setting::Accent(r, g, b)
            }
            (APPEARANCE, "contrast") => Setting::Contrast(value.try_into().ok()?),
            (APPEARANCE, "reduced-motion") => Setting::ReducedMotion(value.try_into().ok()?),
            (INTERFACE, "font-name") => Setting::FontName(value.try_into().ok()?),
            (INTERFACE, "text-scaling-factor") => Setting::TextScaling(value.try_into().ok()?),
            (INTERFACE, "enable-animations") => Setting::EnableAnimations(value.try_into().ok()?),
            _ => return None,
        })
    }

    pub fn apply(self, appearance: &mut Appearance) {
        match self {
            // GNOME's default without a preference is light.
            Setting::ColorScheme(scheme) => appearance.dark = scheme == 1,
            Setting::Accent(r, g, b) => {
                let channels = [r, g, b];
                if channels.iter().all(|channel| (0.0..=1.0).contains(channel)) {
                    appearance.accent = channels.iter().fold(0, |rgb, channel| {
                        (rgb << 8) | (channel * 255.0).round() as u32
                    });
                } else {
                    appearance.accent = Appearance::default().accent;
                }
            }
            Setting::Contrast(contrast) => appearance.high_contrast = contrast == 1,
            Setting::ReducedMotion(reduced) => appearance.reduced_motion = reduced == 1,
            Setting::FontName(name) => appearance.set_font_name(&name),
            Setting::TextScaling(factor) if factor > 0.0 => {
                appearance.text_scaling = factor as f32;
            }
            Setting::TextScaling(_) => {}
            Setting::EnableAnimations(enabled) => appearance.enable_animations = enabled,
        }
    }
}

/// Starts following the settings portal over `connection`. Until the portal answers,
/// and if it never does, the appearance stays what it is.
pub fn follow(connection: Connection, cx: &mut App) {
    let (sender, settings) = async_channel::unbounded::<Vec<Setting>>();
    let spawned = std::thread::Builder::new()
        .name("sheaf-portal".to_owned())
        .spawn(move || {
            if let Err(err) = zbus::block_on(read_and_watch(connection, sender)) {
                log::info!("no settings portal, keeping the built-in appearance: {err}");
            }
        });
    if let Err(err) = spawned {
        log::warn!("cannot start the portal thread: {err}");
        return;
    }
    cx.spawn(async move |cx| {
        while let Ok(batch) = settings.recv().await {
            cx.update(|cx| {
                let mut appearance = theme::appearance(cx).clone();
                for setting in batch {
                    setting.apply(&mut appearance);
                }
                theme::set_appearance(appearance, cx);
            });
        }
    })
    .detach();
}

async fn read_and_watch(
    connection: Connection,
    sender: async_channel::Sender<Vec<Setting>>,
) -> ashpd::Result<()> {
    // Portals take the app id of a host app from this call, which must be the first
    // one on the connection (`docs/verified.md` §1). Sandboxed, it does nothing.
    if let Err(err) = register_host_app(connection.clone()).await {
        log::info!("the portal registry did not take the app id: {err}");
    }
    let portal = Settings::with_connection(connection).await?;
    let mut changes = portal.receive_setting_changed().await?;

    let all = portal.read_all(&[APPEARANCE, INTERFACE]).await?;
    let initial: Vec<Setting> = all
        .iter()
        .flat_map(|(namespace, values)| {
            values
                .iter()
                .filter_map(|(key, value)| Setting::decode(namespace, key, value))
        })
        .collect();
    if sender.send(initial).await.is_err() {
        return Ok(());
    }

    while let Some(change) = changes.next().await {
        let Some(setting) = Setting::decode(change.namespace(), change.key(), change.value())
        else {
            continue;
        };
        if sender.send(vec![setting]).await.is_err() {
            break;
        }
    }
    Ok(())
}

async fn register_host_app(connection: Connection) -> ashpd::Result<()> {
    let app_id = APP_NAME
        .parse::<ashpd::AppID>()
        .map_err(|err| ashpd::Error::from(zbus::Error::Failure(err.to_string())))?;
    ashpd::register_host_app_with_connection(connection, app_id).await
}

#[cfg(test)]
mod tests {
    use zbus::Variant;

    use super::*;

    fn owned(value: Variant<'static>) -> OwnedVariant {
        OwnedVariant::try_from(value).expect("no file descriptors")
    }

    fn decoded(namespace: &str, key: &str, value: Variant<'static>) -> Option<Setting> {
        Setting::decode(namespace, key, &owned(value))
    }

    #[test]
    fn settings_decode_by_namespace_and_key() {
        assert_eq!(
            decoded(APPEARANCE, "color-scheme", Variant::from(1_u32)),
            Some(Setting::ColorScheme(1))
        );
        assert_eq!(
            decoded(APPEARANCE, "accent-color", Variant::from((0.5, 0.25, 1.0))),
            Some(Setting::Accent(0.5, 0.25, 1.0))
        );
        assert_eq!(
            decoded(INTERFACE, "font-name", Variant::from("Arial 11")),
            Some(Setting::FontName("Arial 11".to_owned()))
        );
        assert_eq!(
            decoded(INTERFACE, "text-scaling-factor", Variant::from(1.25)),
            Some(Setting::TextScaling(1.25))
        );
        assert_eq!(
            decoded(INTERFACE, "enable-animations", Variant::from(false)),
            Some(Setting::EnableAnimations(false))
        );
        // Another namespace, another key, another type.
        assert_eq!(
            decoded("org.gnome.desktop.wm", "color-scheme", Variant::from(1_u32)),
            None
        );
        assert_eq!(
            decoded(INTERFACE, "cursor-size", Variant::from(24_i32)),
            None
        );
        assert_eq!(
            decoded(APPEARANCE, "color-scheme", Variant::from("dark")),
            None
        );
    }

    #[test]
    fn settings_change_the_appearance() {
        let mut appearance = Appearance::default();
        Setting::ColorScheme(2).apply(&mut appearance);
        assert!(!appearance.dark);
        Setting::ColorScheme(1).apply(&mut appearance);
        assert!(appearance.dark);
        Setting::ColorScheme(0).apply(&mut appearance);
        assert!(!appearance.dark);

        Setting::Accent(0.0862745, 0.627451, 0.521569).apply(&mut appearance);
        assert_eq!(appearance.accent, 0x16a085);
        Setting::Accent(-1.0, 0.0, 0.0).apply(&mut appearance);
        assert_eq!(appearance.accent, 0x3584e4);

        Setting::Contrast(1).apply(&mut appearance);
        Setting::ReducedMotion(1).apply(&mut appearance);
        assert!(appearance.high_contrast && !appearance.animations());
        Setting::ReducedMotion(0).apply(&mut appearance);
        assert!(appearance.animations());
        Setting::EnableAnimations(false).apply(&mut appearance);
        assert!(!appearance.animations());

        Setting::FontName("Cantarell 12".to_owned()).apply(&mut appearance);
        Setting::TextScaling(1.5).apply(&mut appearance);
        Setting::TextScaling(0.0).apply(&mut appearance);
        assert_eq!(appearance.font_family, "Cantarell");
        assert_eq!(appearance.text_scaling, 1.5);
    }
}
