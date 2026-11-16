//! Translations through gettext, text domain `sheaf`. English is the source language.

use std::path::PathBuf;

use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, textdomain};

pub const DOMAIN: &str = "sheaf";

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

    let dir = std::env::var_os("SHEAF_LOCALEDIR")
        .map_or_else(|| PathBuf::from(SYSTEM_LOCALE_DIR), PathBuf::from);
    let bound = bindtextdomain(DOMAIN, dir)
        .and_then(|_| bind_textdomain_codeset(DOMAIN, "UTF-8"))
        .and_then(|_| textdomain(DOMAIN));
    if let Err(err) = bound {
        log::warn!("cannot bind the text domain {DOMAIN}: {err}");
    }
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
}
