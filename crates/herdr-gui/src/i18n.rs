//! Shardlane i18n seam: the `t()` text lookup and the global language switch.
//!
//! The backend is `rust-i18n` — deliberately the same crate and version
//! gpui-component 0.5.1 uses for its own component strings, so one process
//! global locale covers both the shell and the component layer. The `i18n!`
//! backend invocation lives at the crate root (`main.rs`) because
//! `rust_i18n::t!` expands to `crate::_rust_i18n_t!`; this module only exposes
//! the typed helpers. Locale data sits in `crates/herdr-gui/locales/*.yml`
//! (rust-i18n format 1: nested keys flatten to dot keys; `en` is the default
//! and the fallback; en/zh-CN/zh-TW/ja/ko ship in lockstep).
//!
//! Migration policy: strings move to `t()` surface by surface (the whole
//! Settings surface — every section page — is migrated; remaining surfaces
//! stay hardcoded English until their batch lands). Every catalog key lands
//! in ALL sibling locale files in the same change (enforced by
//! `locale_files_share_one_key_set`); new UI strings must use `t()` so a
//! language never needs a new file. Herdr runtime/TUI strings are Herdr's own
//! surface and are never translated here.
//!
//! [INPUT]: Depends on rust_i18n (global locale + compile-time-embedded catalog) and settings::Language
//! [OUTPUT]: Exposes `t(key) -> SharedString` (zero-alloc for static translations, debug-asserts missing keys)
//!           and `t_with(key, args)` (%{name} placeholder interpolation)
//!           and `t_dyn(key)` (runtime-composed keys, e.g. shortcut registry entries; same debug assert)
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

/// Translate `key` with `%{name}` placeholder interpolation (catalog-side
/// interpolation stays a plain replace so the single-key lookup path and its
/// missing-key debug assert keep working unchanged).
pub fn t_with(key: &'static str, args: &[(&'static str, String)]) -> SharedString {
    let mut text = t(key).to_string();
    for (name, value) in args {
        text = text.replace(&format!("%{{{name}}}"), value);
    }
    SharedString::from(text)
}

/// Translate a runtime-composed key (e.g. `shortcuts.action.terminal_copy`
/// derived from a shortcut registry id). The debug build asserts catalog
/// presence exactly like [`t`]; release builds fall back to the raw key, so
/// callers derive keys by the documented convention only.
pub fn t_dyn(key: &str) -> SharedString {
    #[cfg(debug_assertions)]
    {
        let locale = rust_i18n::locale();
        debug_assert!(
            crate::_rust_i18n_try_translate(&locale, key).is_some(),
            "missing i18n catalog entry: locale={} key={key}",
            &*locale
        );
    }
    match crate::_rust_i18n_try_translate(&rust_i18n::locale(), key) {
        Some(Cow::Borrowed(text)) => SharedString::from(text),
        Some(Cow::Owned(text)) => SharedString::from(text),
        None => SharedString::from(key.to_owned()),
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
    use std::sync::Mutex;

    /// `rust_i18n::set_locale` mutates process-global state; tests that
    /// switch the locale and tests that assert English catalog text share
    /// this lock so parallel test threads never observe a mid-switch locale.
    static LOCALE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn t_with_interpolates_placeholders() {
        let _guard = LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let label = t_with(
            "conversation.tool_group_summary",
            &[("count", "4".to_string())],
        );
        assert_eq!(label, "4 tool calls");
    }

    #[test]
    fn english_catalog_serves_the_migrated_settings_slice() {
        let _guard = LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            t("settings.appearance.language_title"),
            SharedString::from("Language")
        );
        assert_eq!(
            t("settings.appearance.language_subtitle"),
            SharedString::from(
                "Interface language for Shardlane's native shell. English is the fallback when a translation is missing."
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
        let _guard = LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        apply_language(Language::En);
        assert_eq!(&*rust_i18n::locale(), "en");
    }

    /// Every catalog key must exist in every sibling locale file. This is the
    /// guard behind `t`'s debug assert: switching to any language must serve
    /// every migrated string, never a fallback mid-surface.
    #[test]
    fn locale_files_share_one_key_set() {
        fn flatten(
            prefix: &str,
            value: &serde_yaml::Value,
            out: &mut std::collections::BTreeSet<String>,
        ) {
            match value {
                serde_yaml::Value::Mapping(map) => {
                    for (key, child) in map {
                        let Some(key) = key.as_str() else {
                            panic!("catalog keys are strings");
                        };
                        let path = if prefix.is_empty() {
                            key.to_string()
                        } else {
                            format!("{prefix}.{key}")
                        };
                        flatten(&path, child, out);
                    }
                }
                _ => {
                    out.insert(prefix.to_string());
                }
            }
        }
        let locales_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/herdr-gui/locales");
        let mut expected: Option<std::collections::BTreeSet<String>> = None;
        for locale in ["en", "zh-CN", "zh-TW", "ja", "ko"] {
            let path = locales_dir.join(format!("{locale}.yml"));
            let raw = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let value: serde_yaml::Value = serde_yaml::from_str(&raw)
                .unwrap_or_else(|error| panic!("parse {locale}.yml: {error}"));
            let mut keys = std::collections::BTreeSet::new();
            flatten("", &value, &mut keys);
            assert!(!keys.is_empty(), "{locale}.yml has no keys");
            match &expected {
                None => expected = Some(keys),
                Some(base) => {
                    let missing: Vec<_> = base.difference(&keys).collect();
                    let extra: Vec<_> = keys.difference(base).collect();
                    assert!(
                        missing.is_empty() && extra.is_empty(),
                        "{locale}.yml key drift; missing: {missing:?} extra: {extra:?}"
                    );
                }
            }
        }
    }

    /// Interpolation placeholders must survive every translation, or
    /// `t_with` silently renders the raw `%{name}` marker.
    #[test]
    fn placeholders_survive_every_locale() {
        for locale in Language::ALL {
            let translated =
                crate::_rust_i18n_try_translate(locale.code(), "conversation.tool_group_summary")
                    .unwrap_or_else(|| {
                        panic!(
                            "conversation.tool_group_summary missing in {}",
                            locale.code()
                        )
                    });
            assert!(
                translated.contains("%{count}"),
                "locale {} lost the %{{count}} placeholder",
                locale.code()
            );
        }
    }

    #[test]
    fn apply_language_round_trips_every_variant() {
        let _guard = LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for language in Language::ALL {
            apply_language(language);
            assert_eq!(&*rust_i18n::locale(), language.code());
        }
        apply_language(Language::En);
    }

    #[test]
    fn from_config_value_accepts_common_aliases() {
        assert_eq!(Language::from_config_value("en"), Language::En);
        assert_eq!(Language::from_config_value("zh"), Language::ZhCn);
        assert_eq!(Language::from_config_value("zh-CN"), Language::ZhCn);
        assert_eq!(Language::from_config_value("zh_cn"), Language::ZhCn);
        assert_eq!(Language::from_config_value("zh-Hans"), Language::ZhCn);
        assert_eq!(Language::from_config_value("zh-TW"), Language::ZhTw);
        assert_eq!(Language::from_config_value("zh-hant"), Language::ZhTw);
        assert_eq!(Language::from_config_value("ja"), Language::Ja);
        assert_eq!(Language::from_config_value("ko"), Language::Ko);
        assert_eq!(Language::from_config_value(" fr "), Language::En);
        assert_eq!(Language::from_config_value("nonsense"), Language::En);
    }

    /// Shortcut registry labels resolve through `t_dyn` by key convention;
    /// this catches registry-id drift that would break that convention.
    #[test]
    fn shortcut_registry_labels_resolve_in_every_locale() {
        for entry in crate::shortcuts::REGISTRY.iter() {
            let key = crate::shortcuts::label_key(entry.id);
            for language in Language::ALL {
                assert!(
                    crate::_rust_i18n_try_translate(language.code(), &key).is_some(),
                    "missing {} in {}",
                    key,
                    language.code()
                );
            }
        }
    }
}
