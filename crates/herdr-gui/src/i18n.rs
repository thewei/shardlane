//! Shardlane i18n seam: the `t()` text lookup and the global language switch.
//!
//! The backend is `rust-i18n` — deliberately the same crate and version
//! gpui-component 0.5.1 uses for its own component strings, so one process
//! global locale covers both the shell and the component layer. The `i18n!`
//! backend invocation lives at the crate root (`main.rs`) because
//! `rust_i18n::t!` expands to `crate::_rust_i18n_t!`; this module only exposes
//! the typed helpers. Locale data sits in `crates/herdr-gui/locales/*.yml`
//! (rust-i18n format 1: nested keys flatten to dot keys; `en` is the fallback).
//!
//! Migration policy: strings move to `t()` surface by surface (Settings →
//! Appearance is the first slice). Unmigrated surfaces stay hardcoded English
//! until their batch lands; new UI strings must use `t()` so adding a language
//! only requires a sibling locale file. Herdr runtime/TUI strings are Herdr's
//! own surface and are never translated here.
//!
//! [INPUT]: Depends on rust_i18n (global locale + compile-time-embedded catalog) and settings::Language
//! [OUTPUT]: Exposes `t(key) -> SharedString` (zero-alloc for static translations, debug-asserts missing keys)
//!           and `apply_language(Language)` (process-global switch shared with gpui-component)
//! [POS]: Presentation-layer text lookup; consumed by view modules, startup config load, and config reload
//! [PROTOCOL]: Update this header when making changes, then check CLAUDE.md

use gpui::SharedString;
use rust_i18n::t;
use std::borrow::Cow;

/// Translate `key` under the current global locale (see [`apply_language`]).
///
/// Returns the English fallback until a locale provides a translation. Debug
/// builds assert that the key exists in the catalog, so a typo surfaces as a
/// debug/test failure instead of a raw `en.settings.…` string in the UI.
pub fn t(key: &'static str) -> SharedString {
    #[cfg(debug_assertions)]
    {
        let locale = rust_i18n::locale();
        debug_assert!(
            crate::_rust_i18n_try_translate(&locale, key).is_some(),
            "missing i18n catalog entry: locale={} key={key}",
            &*locale
        );
    }
    match t!(key) {
        Cow::Borrowed(text) => SharedString::from(text),
        Cow::Owned(text) => SharedString::from(text),
    }
}

/// Apply `language` to the process-global locale. gpui-component's own strings
/// read the same global, so one call reskins every surface at the next repaint
/// (the caller triggers it via config load or a full `cx.notify()`).
pub fn apply_language(language: crate::settings::Language) {
    rust_i18n::set_locale(language.code());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Language;

    #[test]
    fn english_catalog_serves_the_migrated_settings_slice() {
        // The section titles feed Settings navigation; the subtitle is the
        // Appearance card copy. Both pin the YAML folding against accidental
        // whitespace drift.
        assert_eq!(
            t("settings.section.appearance"),
            SharedString::from("Theme")
        );
        assert_eq!(
            t("settings.section.terminal"),
            SharedString::from("Terminal")
        );
        assert_eq!(
            t("settings.appearance.subtitle"),
            SharedString::from(
                "Pick an appearance first, then a Herdr built-in theme that the App chrome and the terminal surface both follow."
            )
        );
        assert_eq!(
            t("settings.appearance.notice_auto_both"),
            SharedString::from(
                "Both selections are used: the light theme in light appearance, the dark theme in dark appearance."
            )
        );
    }

    #[test]
    fn language_row_copy_is_in_the_catalog() {
        assert_eq!(
            t("settings.appearance.language_title"),
            SharedString::from("Language")
        );
        assert_eq!(
            t("settings.appearance.language_subtitle"),
            SharedString::from(
                "Interface language for Shardlane's native shell. Additional languages will arrive in future releases."
            )
        );
    }

    #[test]
    #[should_panic(expected = "missing i18n catalog entry")]
    fn missing_catalog_keys_fail_loudly_in_debug() {
        let _ = t("settings.does_not_exist");
    }

    #[test]
    fn apply_language_switches_the_global_locale() {
        apply_language(Language::En);
        assert_eq!(&*rust_i18n::locale(), "en");
    }
}
