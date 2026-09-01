//! Host system monospace font discovery: the data source for the Settings Font family menu.
//!
//! [INPUT]: Depends on core-text (CTFontCollection system-wide font enumeration +
//! bit filtering on kCTFontMonoSpaceTrait, the authoritative SDK CTFontTraits.h value 1<<10); zero app state coupling
//! [OUTPUT]: Exposes `monospace_font_families` (sorted, deduplicated monospace family names
//! with an in-process OnceLock cache; the first call pays the enumeration cost)
//! [POS]: herdr-gui's host discovery layer, in the same family as agent_cli.rs (Agent CLI discovery);
//! consumed by settings_view.rs's Font family ControlMenu; never touches the Ghostty/render path

use std::sync::OnceLock;

/// Data source for the Settings Font family menu: system monospace font families
/// (sorted, deduplicated, cached in-process). Non-monospace fonts break column
/// alignment in a grid terminal, so only monospace families are exposed.
pub(crate) fn monospace_font_families() -> &'static [String] {
    static CACHE: OnceLock<Vec<String>> = OnceLock::new();
    CACHE.get_or_init(list_font_families)
}

#[cfg(target_os = "macos")]
fn list_font_families() -> Vec<String> {
    use core_text::font_collection::create_for_all_families;
    use core_text::font_descriptor::{SymbolicTraitAccessors as _, TraitAccessors as _};
    use std::collections::BTreeSet;

    let mut families = BTreeSet::new();
    if let Some(descriptors) = create_for_all_families().get_descriptors() {
        for descriptor in descriptors.iter() {
            if descriptor.traits().symbolic_traits().is_monospace() {
                let name = descriptor.family_name();
                if !name.is_empty() {
                    families.insert(name);
                }
            }
        }
    }
    if families.is_empty() {
        return fallback_families();
    }
    sorted_case_insensitive(families.into_iter().collect())
}

/// Exact dedup via BTreeSet, then case-insensitive sorting (the ASCII order that puts
/// "Andale Mono" before "menlo" is user-unfriendly).
fn sorted_case_insensitive(mut families: Vec<String>) -> Vec<String> {
    families.sort_by_key(|family| family.to_lowercase());
    families
}

#[cfg(not(target_os = "macos"))]
fn list_font_families() -> Vec<String> {
    fallback_families()
}

/// Fallback for enumeration failure / non-macOS: three built-in system monospace fonts.
fn fallback_families() -> Vec<String> {
    ["Menlo", "Monaco", "Courier New"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sorted_case_insensitive;

    #[test]
    fn families_sort_case_insensitively() {
        let families = vec!["Menlo".into(), "Andale Mono".into(), "monaco".into()];
        assert_eq!(
            sorted_case_insensitive(families),
            vec!["Andale Mono", "Menlo", "monaco"]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_enumeration_returns_monospace_families_without_panicking() {
        let families = super::monospace_font_families();
        assert!(
            families.iter().any(|family| family == "Menlo"),
            "system enumeration should include Menlo, got: {families:?}"
        );
    }
}
