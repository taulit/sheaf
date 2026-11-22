//! Translations through gettext, text domain `sheaf`. English is the source language.

use std::path::{Path, PathBuf};

use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, textdomain};

pub const DOMAIN: &str = "sheaf";

/// Where the catalogs live when the executable is not under a `<prefix>/bin`.
const SYSTEM_LOCALE_DIR: &str = "/usr/share/locale";

/// Binds the text domain and adopts the locale of the environment.
///
/// Call it once at program start, before any other thread exists. Without it every
/// string stays English.
pub fn init() {
    // SAFETY: `setlocale` reads the environment and replaces the process locale without
    // synchronisation. The caller contract above (start of `main`, single thread) is
    // what makes that sound; gettext-rs offers no safe way to select the locale.
    #[allow(unsafe_code)]
    let locale = unsafe { gettextrs::setlocale(LocaleCategory::LcAll, "") };
    if locale.is_none() {
        log::warn!("the locale of the environment is not installed, using English");
    }

    let dir = std::env::var_os("SHEAF_LOCALEDIR").map_or_else(installed_locale_dir, PathBuf::from);
    let bound = bindtextdomain(DOMAIN, dir)
        .and_then(|_| bind_textdomain_codeset(DOMAIN, "UTF-8"))
        .and_then(|_| textdomain(DOMAIN));
    if let Err(err) = bound {
        log::warn!("cannot bind the text domain {DOMAIN}: {err}");
    }
}

/// The catalogs of this installation: `<prefix>/share/locale` next to `<prefix>/bin`, so
/// `/usr/local`, `/usr` and Flatpak's `/app` all find theirs; the system directory otherwise.
fn installed_locale_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| locale_dir_for(&exe))
        .unwrap_or_else(|| PathBuf::from(SYSTEM_LOCALE_DIR))
}

/// `<prefix>/share/locale` for an executable at `<prefix>/bin/<name>`, if that directory exists.
fn locale_dir_for(exe: &Path) -> Option<PathBuf> {
    let bin = exe.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let dir = bin.parent()?.join("share/locale");
    dir.is_dir().then_some(dir)
}

/// The translation of `msgid`, or `msgid` itself when there is none.
pub fn tr(msgid: &str) -> String {
    gettextrs::dgettext(DOMAIN, msgid)
}

/// The translation of a counted string, choosing the plural form for `n`.
pub fn ntr(singular: &str, plural: &str, n: u32) -> String {
    gettextrs::dngettext(DOMAIN, singular, plural, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untranslated_strings_come_back_unchanged() {
        assert_eq!(tr("Untitled"), "Untitled");
        assert_eq!(ntr("{} note", "{} notes", 1), "{} note");
        assert_eq!(ntr("{} note", "{} notes", 3), "{} notes");
    }

    #[test]
    fn the_locale_dir_sits_next_to_the_bin_dir() {
        let prefix = tempfile::tempdir().unwrap();
        let locale = prefix.path().join("share/locale");
        std::fs::create_dir_all(&locale).unwrap();
        assert_eq!(
            locale_dir_for(&prefix.path().join("bin/sheaf")),
            Some(locale)
        );
        assert_eq!(locale_dir_for(&prefix.path().join("libexec/sheaf")), None);
        let bare = tempfile::tempdir().unwrap();
        assert_eq!(locale_dir_for(&bare.path().join("bin/sheaf")), None);
    }
}
