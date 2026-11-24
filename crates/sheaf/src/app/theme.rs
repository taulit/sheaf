//! The look of Sheaf: colours, sizes and motion of `docs/design/plan/04-design.md`, and
//! how they follow the desktop.
//!
//! [`Appearance`] is what the desktop asks for (portal settings, or the fallback),
//! [`Tokens`] is what the views draw with. [`set_appearance`] derives the tokens, maps
//! them onto gpui-component's `Theme` so its components match, and refreshes every
//! window. Views read the tokens with [`ActiveTokens::tokens`]:
//!
//! ```ignore
//! let tokens = cx.tokens();
//! div().bg(tokens.card).text_color(tokens.muted).rounded(size::CARD_RADIUS)
//! ```

use std::time::Duration;

use std::borrow::Cow;

use gpui::{
    Animation, App, Font, FontWeight, Global, Hsla, Pixels, Rgba, SharedString, black, font, px,
    white,
};
use gpui_component::{Theme, ThemeMode};
use sheaf_core::config::ColorScheme;

use super::{assets, settings};

/// Sizes and radii in logical pixels, from "Sheaf: Maße".
pub mod size {
    use gpui::{Pixels, px};

    pub const WINDOW_WIDTH: Pixels = px(1100.);
    pub const WINDOW_HEIGHT: Pixels = px(720.);
    pub const WINDOW_MIN_WIDTH: Pixels = px(360.);
    pub const WINDOW_MIN_HEIGHT: Pixels = px(480.);
    pub const WINDOW_RADIUS: Pixels = px(15.);

    pub const HEADER_HEIGHT: Pixels = px(47.);
    pub const FLAT_BUTTON: Pixels = px(34.);
    pub const FLAT_BUTTON_RADIUS: Pixels = px(8.);
    pub const CLOSE_BUTTON: Pixels = px(24.);
    /// Symbolic icons in flat buttons.
    pub const ICON: Pixels = px(17.);

    pub const SEARCH_HEIGHT: Pixels = px(34.);
    pub const SEARCH_RADIUS: Pixels = px(8.);
    pub const CHIP_HEIGHT: Pixels = px(28.);

    pub const GALLERY_GAP: Pixels = px(16.);
    pub const CARD_MIN_WIDTH: Pixels = px(200.);
    pub const CARD_HEIGHT: Pixels = px(200.);
    pub const CARD_RADIUS: Pixels = px(14.);
    pub const CARD_PADDING: Pixels = px(14.);

    pub const EDITOR_MAX_WIDTH: Pixels = px(700.);
    pub const POPOVER_RADIUS: Pixels = px(14.);
}

/// Durations and curves, from "Bewegung". Take animations from [`Tokens`], which
/// shortens them to nothing when the desktop turns animations off.
pub mod motion {
    use std::time::Duration;

    /// The note view sliding in and out.
    pub const SLIDE: Duration = Duration::from_millis(350);
    /// A card lifting under the pointer.
    pub const CARD_HOVER: Duration = Duration::from_millis(350);
    pub const POPOVER: Duration = Duration::from_millis(250);
    pub const CLOSE: Duration = Duration::from_millis(400);
    pub const QUICK_NOTE: Duration = Duration::from_millis(450);

    /// The damped spring of the prototypes (stiffness ≈ 300, damping ≈ 26) as its CSS
    /// `linear()` samples: position, at a fraction of the duration.
    const SPRING: [(f32, f32); 14] = [
        (0.0, 0.0),
        (0.014, 0.006),
        (0.028, 0.025),
        (0.061, 0.101),
        (0.189, 0.539),
        (0.253, 0.721),
        (0.315, 0.849),
        (0.381, 0.937),
        (0.418, 0.968),
        (0.457, 0.991),
        (0.501, 1.006),
        (0.55, 1.015),
        (0.639, 1.017),
        (1.0, 1.001),
    ];

    /// The spring as an easing function. It overshoots slightly past 1 on the way.
    pub fn spring(t: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        let after = SPRING
            .partition_point(|&(at, _)| at <= t)
            .min(SPRING.len() - 1);
        let (from_t, from) = SPRING[after - 1];
        let (to_t, to) = SPRING[after];
        from + (to - from) * (t - from_t) / (to_t - from_t)
    }

    /// `cubic-bezier(.22, 1, .36, 1)`, for closing and for popovers.
    pub fn ease_out(t: f32) -> f32 {
        gpui::ease_out_quint()(t.clamp(0.0, 1.0))
    }
}

const DEFAULT_ACCENT: u32 = 0x3584e4;
const DEFAULT_FONT: &str = "Adwaita Sans";
/// The size of GNOME's default interface font, which the sizes of 04 are drawn for.
const DEFAULT_FONT_POINTS: f32 = 11.0;
/// Tried in this order when the font of the desktop cannot be used. The embedded
/// Inter always can.
const FALLBACK_FONTS: [&str; 2] = [DEFAULT_FONT, assets::FONT_FAMILY];

/// What the desktop asks for. The default is the fallback without a settings portal.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    pub dark: bool,
    /// The accent colour as `0xRRGGBB`.
    pub accent: u32,
    pub high_contrast: bool,
    /// `org.gnome.desktop.interface enable-animations`.
    pub enable_animations: bool,
    /// `org.freedesktop.appearance reduced-motion`.
    pub reduced_motion: bool,
    pub font_family: String,
    /// The size of the interface font in points.
    pub font_points: f32,
    /// `org.gnome.desktop.interface text-scaling-factor`.
    pub text_scaling: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            dark: true,
            accent: DEFAULT_ACCENT,
            high_contrast: false,
            enable_animations: true,
            reduced_motion: false,
            font_family: DEFAULT_FONT.to_owned(),
            font_points: DEFAULT_FONT_POINTS,
            text_scaling: 1.0,
        }
    }
}

impl Appearance {
    pub fn animations(&self) -> bool {
        self.enable_animations && !self.reduced_motion
    }

    /// How much larger than drawn text is: the font size of the desktop relative to
    /// GNOME's default, times its text scaling.
    pub fn text_scale(&self) -> f32 {
        (self.font_points / DEFAULT_FONT_POINTS) * self.text_scaling
    }

    /// Sets family and size from a GTK font description such as `Adwaita Sans 11`.
    pub fn set_font_name(&mut self, description: &str) {
        let description = description.trim();
        let (family, points) = match description.rsplit_once(' ') {
            Some((family, size)) => match size.parse::<f32>() {
                Ok(points) if points > 0.0 => (family.trim(), Some(points)),
                _ => (description, None),
            },
            None => (description, None),
        };
        if !family.is_empty() {
            self.font_family = family.to_owned();
        }
        self.font_points = points.unwrap_or(DEFAULT_FONT_POINTS);
    }
}

/// Fixed values that win over the desktop, for tests and screenshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    pub dark: Option<bool>,
    pub accent: Option<u32>,
}

impl Overrides {
    fn apply(self, mut appearance: Appearance) -> Appearance {
        appearance.dark = self.dark.unwrap_or(appearance.dark);
        appearance.accent = self.accent.unwrap_or(appearance.accent);
        appearance
    }
}

/// What the views draw with. Colours are the Sheaf column of 04, "Farben".
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// The window and the gallery.
    pub window: Hsla,
    /// The surface text is written on: the note view.
    pub view: Hsla,
    pub headerbar: Hsla,
    pub popover: Hsla,
    pub card: Hsla,
    pub fg: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    /// Translucent, to lay over any surface.
    pub hover: Hsla,
    pub selected: Hsla,
    pub hair: Hsla,
    pub accent: Hsla,
    /// Text and symbols on the accent colour.
    pub on_accent: Hsla,
    pub font_family: SharedString,
    /// Multiply text sizes by this; see [`Tokens::text`].
    pub text_scale: f32,
    /// Whether anything may move.
    pub animations: bool,
}

impl Tokens {
    pub fn new(appearance: &Appearance) -> Self {
        let accent: Hsla = gpui::rgb(appearance.accent).into();
        let contrast = appearance.high_contrast;
        let rgb = |hex: u32| -> Hsla { gpui::rgb(hex).into() };
        let common = |tokens: Tokens| Tokens {
            accent,
            on_accent: white(),
            font_family: appearance.font_family.clone().into(),
            text_scale: appearance.text_scale(),
            animations: appearance.animations(),
            ..tokens
        };
        let unset = Hsla::default();
        if appearance.dark {
            let fg = rgb(0xf5f5f7);
            common(Tokens {
                dark: true,
                window: rgb(0x222226),
                view: rgb(0x1d1d20),
                headerbar: rgb(0x2e2e32),
                popover: rgb(0x38383d),
                card: rgb(0x2a2a2e),
                fg,
                muted: fg.opacity(if contrast { 0.8 } else { 0.62 }),
                faint: fg.opacity(if contrast { 0.6 } else { 0.38 }),
                hover: white().opacity(0.07),
                selected: white().opacity(0.11),
                hair: white().opacity(if contrast { 0.3 } else { 0.08 }),
                accent: unset,
                on_accent: unset,
                font_family: SharedString::default(),
                text_scale: 1.0,
                animations: true,
            })
        } else {
            let fg = rgb(0x1d1d1f);
            common(Tokens {
                dark: false,
                window: rgb(0xfafafb),
                view: rgb(0xffffff),
                headerbar: rgb(0xffffff),
                popover: rgb(0xffffff),
                card: rgb(0xf4f4f6),
                fg,
                muted: fg.opacity(if contrast { 0.8 } else { 0.62 }),
                faint: fg.opacity(if contrast { 0.6 } else { 0.4 }),
                hover: black().opacity(0.05),
                selected: black().opacity(0.09),
                hair: black().opacity(if contrast { 0.3 } else { 0.09 }),
                accent: unset,
                on_accent: unset,
                font_family: SharedString::default(),
                text_scale: 1.0,
                animations: true,
            })
        }
    }

    /// A text size of 04 at the text scale of the desktop.
    pub fn text(&self, size: f32) -> Pixels {
        px(size * self.text_scale)
    }

    /// `duration`, or nothing when animations are off.
    pub fn duration(&self, duration: Duration) -> Duration {
        if self.animations {
            duration
        } else {
            Duration::ZERO
        }
    }

    /// An animation on the spring curve: sliding in, lifting, switching.
    pub fn spring(&self, duration: Duration) -> Animation {
        Animation::new(self.duration(duration)).with_easing(motion::spring)
    }

    /// An animation that eases out: closing, popovers, content fading in.
    pub fn ease_out(&self, duration: Duration) -> Animation {
        Animation::new(self.duration(duration)).with_easing(motion::ease_out)
    }
}

struct ThemeState {
    appearance: Appearance,
    overrides: Overrides,
    tokens: Tokens,
}

impl Global for ThemeState {}

/// Access to the [`Tokens`] from any context that derefs to [`App`].
pub trait ActiveTokens {
    fn tokens(&self) -> &Tokens;
}

impl ActiveTokens for App {
    fn tokens(&self) -> &Tokens {
        &self.global::<ThemeState>().tokens
    }
}

/// Installs the fallback appearance with `overrides`. Call it once, after
/// `gpui_component::init`, before the first view is created.
pub fn init(overrides: Overrides, cx: &mut App) {
    register_fonts(cx);
    install(Appearance::default(), overrides, cx);
}

/// Adds the embedded Inter to the text system unless an Inter with a bold face of its
/// own is there already (an earlier app on the same text system, or static Inter
/// installed on the system).
fn register_fonts(cx: &App) {
    if has_bold_face(assets::FONT_FAMILY, cx) {
        return;
    }
    let fonts = assets::FONTS
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::warn!("cannot load the embedded fonts: {err:#}");
    }
}

/// Whether `family` is installed with a face for bold that is not its regular face.
/// A variable font is one face, and GPUI 0.3.12 draws every weight of it as regular.
fn has_bold_face(family: &str, cx: &App) -> bool {
    let text_system = cx.text_system();
    if !text_system
        .all_font_names()
        .iter()
        .any(|name| name == family)
    {
        return false;
    }
    let regular = font(family.to_owned());
    let bold = Font {
        weight: FontWeight::BOLD,
        ..regular.clone()
    };
    text_system.resolve_font(&regular) != text_system.resolve_font(&bold)
}

/// The appearance last set, without the overrides.
pub fn appearance(cx: &App) -> &Appearance {
    &cx.global::<ThemeState>().appearance
}

/// Follows a change of the desktop: new tokens, a matching gpui-component theme, and a
/// redraw of every window.
pub fn set_appearance(appearance: Appearance, cx: &mut App) {
    let overrides = cx.global::<ThemeState>().overrides;
    install(appearance, overrides, cx);
}

/// Draws again with the appearance last set, after the colour scheme setting changed.
/// Does nothing before [`init`].
pub fn refresh(cx: &mut App) {
    if let Some(state) = cx.try_global::<ThemeState>() {
        let (appearance, overrides) = (state.appearance.clone(), state.overrides);
        install(appearance, overrides, cx);
    }
}

fn install(appearance: Appearance, overrides: Overrides, cx: &mut App) {
    let mut chosen = appearance.clone();
    match settings::config(cx).color_scheme {
        ColorScheme::System => {}
        ColorScheme::Light => chosen.dark = false,
        ColorScheme::Dark => chosen.dark = true,
    }
    let effective = overrides.apply(chosen);
    let mut tokens = Tokens::new(&effective);
    if let Some(family) = installed_font(&tokens.font_family, cx) {
        tokens.font_family = family;
    }
    cx.set_global(ThemeState {
        appearance,
        overrides,
        tokens: tokens.clone(),
    });
    cx.set_reduce_motion(!tokens.animations);
    apply_to_component_theme(&tokens, cx);
}

/// `family` if it is installed with a real bold face, else the first fallback that is.
/// `None` leaves the choice to gpui-component, which then uses the system font.
fn installed_font(family: &SharedString, cx: &App) -> Option<SharedString> {
    std::iter::once(family.as_ref())
        .chain(FALLBACK_FONTS)
        .find(|candidate| has_bold_face(candidate, cx))
        .map(|family| SharedString::from(family.to_owned()))
}

/// Shifts the lightness of `color` by `amount` towards white (positive) or black.
fn shade(color: Hsla, amount: f32) -> Hsla {
    Hsla {
        l: (color.l + amount).clamp(0.0, 1.0),
        ..color
    }
}

/// `overlay` painted over the opaque `base`, as one opaque colour. For places where
/// gpui-component needs a solid colour and 04 names a translucent one.
fn flatten(base: Hsla, overlay: Hsla) -> Hsla {
    let base = Rgba::from(base);
    let over = Rgba::from(overlay);
    let mix = |below: f32, above: f32| below + (above - below) * over.a;
    Hsla::from(Rgba {
        r: mix(base.r, over.r),
        g: mix(base.g, over.g),
        b: mix(base.b, over.b),
        a: 1.0,
    })
}

fn apply_to_component_theme(tokens: &Tokens, cx: &mut App) {
    let mode = if tokens.dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    // Loads gpui-component's own palette for the mode; ours goes on top of it.
    Theme::change(mode, None, cx);

    let tokens = tokens.clone();
    let step = if tokens.dark { 0.05 } else { -0.05 };
    Theme::update(cx, move |theme| {
        theme.background = tokens.window;
        theme.foreground = tokens.fg;
        theme.muted = tokens.hover;
        theme.muted_foreground = tokens.muted;
        theme.border = tokens.hair;
        theme.input = tokens.hair;
        theme.ring = tokens.accent.opacity(0.7);

        theme.primary = tokens.accent;
        theme.primary_hover = shade(tokens.accent, step);
        theme.primary_active = shade(tokens.accent, -step);
        theme.primary_foreground = tokens.on_accent;
        theme.secondary = tokens.selected;
        theme.secondary_hover = tokens.selected.opacity(1.3);
        theme.secondary_active = tokens.selected.opacity(1.6);
        theme.secondary_foreground = tokens.fg;
        theme.accent = flatten(tokens.popover, tokens.hover);
        theme.accent_foreground = tokens.fg;

        theme.popover = tokens.popover;
        theme.popover_foreground = tokens.fg;
        theme.colors.list = tokens.window;
        theme.list_hover = tokens.hover;
        theme.list_active = tokens.selected;
        theme.title_bar = tokens.headerbar;
        theme.title_bar_border = tokens.hair;
        theme.sidebar = tokens.headerbar;
        theme.sidebar_foreground = tokens.fg;
        theme.sidebar_border = tokens.hair;

        theme.link = tokens.accent;
        theme.link_hover = shade(tokens.accent, step);
        theme.link_active = shade(tokens.accent, -step);
        theme.caret = tokens.accent;
        theme.selection = tokens.accent.opacity(0.4);
        theme.scrollbar = gpui::transparent_black();
        theme.scrollbar_thumb = tokens.selected;
        theme.scrollbar_thumb_hover = tokens.faint;
        // The prototype rings the window with a hairline; a brighter one on dark.
        theme.window_border = if tokens.dark {
            white().opacity(0.07)
        } else {
            black().opacity(0.16)
        };

        theme.radius = size::FLAT_BUTTON_RADIUS;
        theme.radius_lg = size::POPOVER_RADIUS;
        theme.window_radius = size::WINDOW_RADIUS;
        theme.font_family = tokens.font_family.clone();
        theme.font_size = tokens.text(16.);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spring_starts_at_rest_overshoots_and_settles() {
        assert_eq!(motion::spring(0.0), 0.0);
        assert_eq!(motion::spring(-1.0), 0.0);
        assert_eq!(motion::spring(1.0), 1.0);
        assert_eq!(motion::spring(2.0), 1.0);
        assert!((motion::spring(0.189) - 0.539).abs() < 1e-4);
        // Half way between two samples.
        assert!((motion::spring(0.221) - 0.63).abs() < 1e-3);
        let peak = (0..=100)
            .map(|step| motion::spring(step as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.0 && peak < 1.02, "{peak}");
        let mut last = 0.0;
        for step in 0..=60 {
            let value = motion::spring(step as f32 / 100.0);
            assert!(value >= last, "rises until the overshoot");
            last = value;
        }
    }

    #[test]
    fn easing_out_is_fast_first() {
        assert_eq!(motion::ease_out(0.0), 0.0);
        assert_eq!(motion::ease_out(1.0), 1.0);
        assert!(motion::ease_out(0.3) > 0.8);
    }

    #[test]
    fn tokens_follow_scheme_accent_and_contrast() {
        let dark = Tokens::new(&Appearance::default());
        assert!(dark.dark);
        assert_eq!(dark.window, Hsla::from(gpui::rgb(0x222226)));
        assert_eq!(dark.view, Hsla::from(gpui::rgb(0x1d1d20)));
        assert_eq!(dark.card, Hsla::from(gpui::rgb(0x2a2a2e)));
        assert_eq!(dark.accent, Hsla::from(gpui::rgb(0x3584e4)));
        assert!((dark.muted.a - 0.62).abs() < 1e-6);
        assert!((dark.faint.a - 0.38).abs() < 1e-6);

        let light = Tokens::new(&Appearance {
            dark: false,
            accent: 0xe62d42,
            ..Appearance::default()
        });
        assert!(!light.dark);
        assert_eq!(light.window, Hsla::from(gpui::rgb(0xfafafb)));
        assert_eq!(light.card, Hsla::from(gpui::rgb(0xf4f4f6)));
        assert_eq!(light.accent, Hsla::from(gpui::rgb(0xe62d42)));
        assert!((light.faint.a - 0.4).abs() < 1e-6);
        assert!((light.hair.a - 0.09).abs() < 1e-6);

        let contrast = Tokens::new(&Appearance {
            high_contrast: true,
            ..Appearance::default()
        });
        assert!(contrast.hair.a > dark.hair.a);
        assert!(contrast.muted.a > dark.muted.a);
    }

    #[test]
    fn animations_take_no_time_when_the_desktop_turns_them_off() {
        let on = Tokens::new(&Appearance::default());
        assert_eq!(on.duration(motion::SLIDE), Duration::from_millis(350));
        assert_eq!(on.spring(motion::SLIDE).duration, motion::SLIDE);

        for appearance in [
            Appearance {
                enable_animations: false,
                ..Appearance::default()
            },
            Appearance {
                reduced_motion: true,
                ..Appearance::default()
            },
        ] {
            let off = Tokens::new(&appearance);
            assert!(!off.animations);
            assert_eq!(off.duration(motion::SLIDE), Duration::ZERO);
            assert_eq!(off.spring(motion::SLIDE).duration, Duration::ZERO);
            assert_eq!(off.ease_out(motion::POPOVER).duration, Duration::ZERO);
        }
    }

    #[test]
    fn font_descriptions_split_into_family_and_size() {
        let mut appearance = Appearance::default();
        appearance.set_font_name("Arial 11");
        assert_eq!(appearance.font_family, "Arial");
        assert_eq!(appearance.text_scale(), 1.0);

        appearance.set_font_name("Noto Sans Display 13.2");
        assert_eq!(appearance.font_family, "Noto Sans Display");
        assert!((appearance.text_scale() - 1.2).abs() < 1e-6);

        appearance.text_scaling = 1.25;
        assert!((appearance.text_scale() - 1.5).abs() < 1e-6);
        assert!((f32::from(Tokens::new(&appearance).text(14.)) - 21.0).abs() < 1e-4);

        appearance.set_font_name("Cantarell");
        assert_eq!(appearance.font_family, "Cantarell");
        assert_eq!(appearance.font_points, 11.0);
        appearance.set_font_name("  ");
        assert_eq!(appearance.font_family, "Cantarell");
    }

    #[test]
    fn overrides_win() {
        let overrides = Overrides {
            dark: Some(false),
            accent: Some(0xe62d42),
        };
        let applied = overrides.apply(Appearance::default());
        assert!(!applied.dark);
        assert_eq!(applied.accent, 0xe62d42);
        assert_eq!(applied.font_family, "Adwaita Sans");
        assert_eq!(
            Overrides::default().apply(Appearance::default()),
            Appearance::default()
        );
    }

    #[gpui::test]
    fn the_colour_scheme_setting_wins_over_the_desktop(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            init(Overrides::default(), cx);
            let desktop = Appearance {
                dark: false,
                ..Appearance::default()
            };
            set_appearance(desktop, cx);
            assert!(!cx.tokens().dark, "System follows the desktop");

            settings::update(cx, |config| config.color_scheme = ColorScheme::Dark).detach();
            assert!(cx.tokens().dark);
            assert!(!appearance(cx).dark, "the desktop's choice is kept");

            let light_desktop = appearance(cx).clone();
            let dark_desktop = Appearance {
                dark: true,
                ..light_desktop
            };
            settings::update(cx, |config| config.color_scheme = ColorScheme::Light).detach();
            set_appearance(dark_desktop, cx);
            assert!(
                !cx.tokens().dark,
                "a change of the desktop does not undo it"
            );

            settings::update(cx, |config| config.color_scheme = ColorScheme::System).detach();
            assert!(cx.tokens().dark);
        });
    }

    #[test]
    fn translucent_colours_flatten_onto_their_surface() {
        let flat = flatten(gpui::rgb(0x000000).into(), white().opacity(0.5));
        let rgba = Rgba::from(flat);
        assert!((rgba.r - 0.5).abs() < 0.01 && (rgba.a - 1.0).abs() < 1e-6);
    }
}
