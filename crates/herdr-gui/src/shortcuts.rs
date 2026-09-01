//! Shortcut Registry: stable command IDs, scope metadata, and user override resolution.
//!
//! [INPUT]: Application's existing action types (from actions!(shardlane, ...)),
//!          user config (ShortcutConfig from settings.rs),
//!          GPUI focus context identifiers
//! [OUTPUT]: ShortcutRegistry (stable IDs → platform default chord + scope + label),
//!           resolved_bindings() for GPUI cx.bind_keys
//! [POS]: Central registry bridging the existing action keybindings with configurable overrides.

use std::collections::{BTreeMap, BTreeSet};

/// Stable identifier for a registerable command (e.g. "app.quit", "terminal.paste").
pub(crate) type ShortcutId = &'static str;

/// Scope determines when a keybinding is active.
/// Maps to GPUI context strings passed to `KeyBinding::new(..., Some(context))`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ShortcutScope {
    /// Active everywhere (GPUI context = None).
    Global,
    /// Active only when ShardlaneApp root focus_handle owns focus.
    App,
    /// Active when the Ctrl-Tab Agent Switcher overlay is visible (audit E07).
    AgentSwitcher,
}

impl ShortcutScope {
    /// Returns the GPUI context string for `KeyBinding::new(chord, action, context)`.
    pub(crate) fn gpui_context(self) -> Option<&'static str> {
        match self {
            Self::Global => None,
            Self::App => Some("ShardlaneApp"),
            Self::AgentSwitcher => Some("AgentSwitcher"),
        }
    }
}

/// One entry in the shortcut registry.
#[derive(Clone, Debug)]
pub(crate) struct ShortcutEntry {
    pub id: ShortcutId,
    pub label: &'static str,
    pub default_chord: &'static str,
    pub scope: ShortcutScope,
    pub category: ShortcutCategory,
}

/// Grouping for Settings UI display.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ShortcutCategory {
    App,
    Sidebar,
    Terminal,
    Pane,
    Tab,
    Project,
}

impl ShortcutCategory {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::App => "Application",
            Self::Sidebar => "Sidebar",
            Self::Terminal => "Terminal",
            Self::Pane => "Panes",
            Self::Tab => "Tabs",
            Self::Project => "Projects",
        }
    }
}

/// The complete registry of known shortcuts with their defaults.
pub(crate) static REGISTRY: &[ShortcutEntry] = &[
    // --- App ---
    ShortcutEntry {
        id: "app.quit",
        label: "Quit Shardlane",
        default_chord: "cmd-q",
        scope: ShortcutScope::Global,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.settings",
        label: "Settings",
        default_chord: "cmd-,",
        scope: ShortcutScope::Global,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.search",
        label: "Search",
        default_chord: "cmd-k",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.find",
        label: "Find in Conversation",
        default_chord: "cmd-f",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.new-window",
        label: "New Window",
        default_chord: "cmd-n",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.new-task",
        label: "New Task",
        default_chord: "cmd-shift-n",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.refresh",
        label: "Reconnect",
        default_chord: "cmd-r",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.help",
        label: "Help",
        default_chord: "f1",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "app.reload-config",
        label: "Reload Config",
        default_chord: "cmd-shift-r",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "lazygit.open",
        label: "Open Lazygit",
        // Deliberately unbound: users can opt into a chord in Settings without
        // taking over a conventional application shortcut.
        default_chord: "",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    // --- Agent Switcher (audit E07: previously internal-only bindings — not configurable,
    // absent from help, and invisible to chord_is_bound's terminal swallow decision) ---
    ShortcutEntry {
        id: "agent.switcher-next",
        label: "Next Agent (Switcher)",
        default_chord: "ctrl-tab",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "agent.switcher-prev",
        label: "Previous Agent (Switcher)",
        default_chord: "ctrl-shift-tab",
        scope: ShortcutScope::App,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "agent.switcher-confirm",
        label: "Confirm Agent Switch",
        default_chord: "enter",
        scope: ShortcutScope::AgentSwitcher,
        category: ShortcutCategory::App,
    },
    ShortcutEntry {
        id: "agent.switcher-cancel",
        label: "Cancel Agent Switch",
        default_chord: "escape",
        scope: ShortcutScope::AgentSwitcher,
        category: ShortcutCategory::App,
    },
    // --- Sidebar ---
    ShortcutEntry {
        id: "sidebar.toggle",
        label: "Toggle Sidebar",
        default_chord: "cmd-b",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Sidebar,
    },
    ShortcutEntry {
        id: "sidebar.agents",
        label: "Toggle Agents",
        default_chord: "cmd-shift-a",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Sidebar,
    },
    // --- Terminal ---
    ShortcutEntry {
        id: "terminal.paste",
        label: "Paste",
        default_chord: "cmd-v",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Terminal,
    },
    ShortcutEntry {
        id: "terminal.copy",
        label: "Copy",
        default_chord: "cmd-c",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Terminal,
    },
    ShortcutEntry {
        id: "terminal.select-all",
        label: "Select All",
        default_chord: "cmd-a",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Terminal,
    },
    ShortcutEntry {
        id: "terminal.font-increase",
        label: "Increase Font Size",
        default_chord: "cmd-=",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Terminal,
    },
    ShortcutEntry {
        id: "terminal.font-decrease",
        label: "Decrease Font Size",
        default_chord: "cmd--",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Terminal,
    },
    ShortcutEntry {
        id: "terminal.font-reset",
        label: "Reset Font Size",
        default_chord: "cmd-0",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Terminal,
    },
    // --- Panes ---
    ShortcutEntry {
        id: "pane.split-right",
        label: "Split Right",
        default_chord: "cmd-]",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.split-down",
        label: "Split Down",
        default_chord: "cmd-shift-]",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.zoom",
        label: "Toggle Zoom",
        default_chord: "cmd-shift-enter",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.focus-left",
        label: "Focus Left",
        default_chord: "cmd-alt-left",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.focus-right",
        label: "Focus Right",
        default_chord: "cmd-alt-right",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.focus-up",
        label: "Focus Up",
        default_chord: "cmd-alt-up",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.focus-down",
        label: "Focus Down",
        default_chord: "cmd-alt-down",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.resize-left",
        label: "Resize Left",
        default_chord: "cmd-alt-shift-left",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.resize-right",
        label: "Resize Right",
        default_chord: "cmd-alt-shift-right",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.resize-up",
        label: "Resize Up",
        default_chord: "cmd-alt-shift-up",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.resize-down",
        label: "Resize Down",
        default_chord: "cmd-alt-shift-down",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    ShortcutEntry {
        id: "pane.close",
        label: "Close Pane",
        default_chord: "cmd-shift-w",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Pane,
    },
    // --- Tabs ---
    ShortcutEntry {
        id: "tab.new",
        label: "New Tab",
        default_chord: "cmd-t",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Tab,
    },
    ShortcutEntry {
        id: "tab.close",
        label: "Close Tab",
        default_chord: "cmd-w",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Tab,
    },
    ShortcutEntry {
        id: "tab.previous",
        label: "Previous Tab",
        default_chord: "cmd-left",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Tab,
    },
    ShortcutEntry {
        id: "tab.next",
        label: "Next Tab",
        default_chord: "cmd-right",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Tab,
    },
    // --- Projects ---
    ShortcutEntry {
        id: "project.previous",
        label: "Previous Project",
        default_chord: "cmd-shift-left",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Project,
    },
    ShortcutEntry {
        id: "project.next",
        label: "Next Project",
        default_chord: "cmd-shift-right",
        scope: ShortcutScope::App,
        category: ShortcutCategory::Project,
    },
];

/// Resolve a registry default through the platform profile.
///
/// Action IDs and persisted overrides are platform-neutral. The registry keeps
/// the macOS chord as its canonical source while Linux/Windows use the same
/// action with Ctrl as the primary modifier. Returning static strings keeps the
/// existing `effective_chord` borrow contract and avoids allocating in render or
/// keymap construction paths.
pub(crate) fn platform_default_chord(default: &'static str) -> &'static str {
    #[cfg(target_os = "macos")]
    {
        default
    }

    #[cfg(not(target_os = "macos"))]
    {
        match default {
            "cmd-q" => "ctrl-q",
            "cmd-," => "ctrl-,",
            "cmd-k" => "ctrl-k",
            "cmd-f" => "ctrl-f",
            "cmd-n" => "ctrl-n",
            "cmd-r" => "ctrl-r",
            "cmd-shift-r" => "ctrl-shift-r",
            "cmd-b" => "ctrl-b",
            "cmd-shift-a" => "ctrl-shift-a",
            "cmd-v" => "ctrl-v",
            "cmd-c" => "ctrl-c",
            "cmd-a" => "ctrl-a",
            "cmd-=" => "ctrl-=",
            "cmd--" => "ctrl--",
            "cmd-0" => "ctrl-0",
            "cmd-]" => "ctrl-]",
            "cmd-shift-]" => "ctrl-shift-]",
            "cmd-shift-enter" => "ctrl-shift-enter",
            "cmd-alt-left" => "ctrl-alt-left",
            "cmd-alt-right" => "ctrl-alt-right",
            "cmd-alt-up" => "ctrl-alt-up",
            "cmd-alt-down" => "ctrl-alt-down",
            "cmd-alt-shift-left" => "ctrl-alt-shift-left",
            "cmd-alt-shift-right" => "ctrl-alt-shift-right",
            "cmd-alt-shift-up" => "ctrl-alt-shift-up",
            "cmd-alt-shift-down" => "ctrl-alt-shift-down",
            "cmd-shift-w" => "ctrl-shift-w",
            "cmd-t" => "ctrl-t",
            "cmd-w" => "ctrl-w",
            "cmd-left" => "ctrl-left",
            "cmd-right" => "ctrl-right",
            "cmd-shift-left" => "ctrl-shift-left",
            "cmd-shift-right" => "ctrl-shift-right",
            other => other,
        }
    }
}

/// User-configurable shortcut overrides persisted in ApplicationConfig.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct ShortcutConfig {
    /// Mapping from ShortcutId to overridden chord strings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Vec<String>>,
    /// Set of ShortcutIds explicitly disabled by the user.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled: BTreeSet<String>,
}

impl ShortcutConfig {
    pub(crate) fn is_empty(&self) -> bool {
        self.overrides.is_empty() && self.disabled.is_empty()
    }
}

/// A conflict between two shortcuts sharing the same chord in overlapping scopes.
#[derive(Clone, Debug)]
pub(crate) struct ShortcutConflict {
    pub chord: String,
    pub ids: Vec<ShortcutId>,
}

/// Resolve the effective chord for a given shortcut ID, applying user overrides.
pub(crate) fn effective_chord(id: ShortcutId, config: &ShortcutConfig) -> Option<&str> {
    if config.disabled.contains(id) {
        return None;
    }
    if let Some(chords) = config.overrides.get(id) {
        chords
            .first()
            .map(|s| s.as_str())
            .filter(|chord| !chord.trim().is_empty())
    } else {
        REGISTRY
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| platform_default_chord(entry.default_chord))
            .filter(|chord| !chord.trim().is_empty())
    }
}

/// Find the registry entry by stable ID.
pub(crate) fn find_entry(id: ShortcutId) -> Option<&'static ShortcutEntry> {
    REGISTRY.iter().find(|entry| entry.id == id)
}

/// Detect conflicts: two entries with the same effective chord in overlapping scopes.
pub(crate) fn detect_conflicts(config: &ShortcutConfig) -> Vec<ShortcutConflict> {
    let mut chord_map: BTreeMap<(String, ShortcutScope), Vec<ShortcutId>> = BTreeMap::new();
    for entry in REGISTRY {
        if let Some(chord) = effective_chord(entry.id, config) {
            chord_map
                .entry((chord.to_string(), entry.scope))
                .or_default()
                .push(entry.id);
        }
    }
    // Global overlaps with every scope
    let globals: BTreeMap<String, Vec<ShortcutId>> = chord_map
        .iter()
        .filter(|((_, scope), _)| *scope == ShortcutScope::Global)
        .map(|((chord, _), ids)| (chord.clone(), ids.clone()))
        .collect();

    let mut conflicts = Vec::new();
    for ((chord, scope), ids) in &chord_map {
        if ids.len() > 1 {
            conflicts.push(ShortcutConflict {
                chord: chord.clone(),
                ids: ids.clone(),
            });
        }
        // Check overlap with globals
        if *scope != ShortcutScope::Global {
            if let Some(global_ids) = globals.get(chord) {
                let mut combined: Vec<ShortcutId> = global_ids.clone();
                combined.extend(ids.iter().copied());
                if combined.len() > 1 {
                    let already_reported = conflicts
                        .iter()
                        .any(|c| c.chord == *chord && c.ids.len() >= combined.len());
                    if !already_reported {
                        conflicts.push(ShortcutConflict {
                            chord: chord.clone(),
                            ids: combined,
                        });
                    }
                }
            }
        }
    }
    conflicts
}

/// Format a chord for display (e.g. "cmd-k" → "⌘K").
pub(crate) fn format_chord_display(chord: &str) -> String {
    let parts: Vec<&str> = chord.split('-').collect();
    let mut display = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i < parts.len() - 1 {
            match *part {
                "cmd" => display.push('⌘'),
                "alt" => display.push('⌥'),
                "shift" => display.push('⇧'),
                "ctrl" => display.push('⌃'),
                _ => display.push_str(part),
            }
        } else {
            let key = match *part {
                "enter" => "↩",
                "up" => "↑",
                "down" => "↓",
                "left" => "←",
                "right" => "→",
                "escape" => "⎋",
                "tab" => "⇥",
                "backspace" => "⌫",
                "=" => "+",
                "-" => "−",
                "]" => "]",
                "[" => "[",
                other => other,
            };
            display.push_str(&key.to_uppercase());
        }
    }
    display
}

/// Resolve the display string for a shortcut by its stable ID.
pub(crate) fn display_chord_for_id(id: ShortcutId, config: &ShortcutConfig) -> Option<String> {
    effective_chord(id, config).map(format_chord_display)
}

// ---------------------------------------------------------------------------
// SCT-01: runtime binding resolution (single source of truth)
// ---------------------------------------------------------------------------

/// A resolved runtime binding: id + effective chord + scope.
/// main.rs maps the id to the concrete GPUI action type and hands it to `cx.bind_keys`.
#[derive(Clone, Debug)]
pub(crate) struct ResolvedBinding {
    pub id: ShortcutId,
    pub chord: String,
    pub scope: ShortcutScope,
}

/// Resolve all currently effective bindings (overrides / disabled applied).
/// Shared by startup bind_keys and runtime rebinding — one source of truth for both.
pub(crate) fn resolved_bindings(config: &ShortcutConfig) -> Vec<ResolvedBinding> {
    REGISTRY
        .iter()
        .filter_map(|entry| {
            let chord = effective_chord(entry.id, config)?.to_string();
            Some(ResolvedBinding {
                id: entry.id,
                chord,
                scope: entry.scope,
            })
        })
        .collect()
}

/// SCT-02: whether a chord hits any currently effective binding (including new chords after override).
/// The terminal key-swallowing decision derives from this function and is never hand-synced with the binding table.
/// A disabled chord no longer matches (the key passes through and triggers no action).
pub(crate) fn chord_is_bound(chord: &str, config: &ShortcutConfig) -> bool {
    REGISTRY.iter().any(|entry| {
        !config.disabled.contains(entry.id) && effective_chord(entry.id, config) == Some(chord)
    })
}

/// Shadowed chords (when overridden, the old default chord needs a NoAction shadow).
pub(crate) fn shadowed_default_chords(
    config: &ShortcutConfig,
) -> Vec<(ShortcutId, String, ShortcutScope)> {
    REGISTRY
        .iter()
        .filter(|entry| {
            let default_chord = platform_default_chord(entry.default_chord);
            if default_chord.trim().is_empty() {
                return false;
            }
            let effective = effective_chord(entry.id, config);
            // Overridden and new chord != default: the default chord needs shadowing.
            // Disabled: the default chord also needs shadowing.
            effective.is_none() || (effective != Some(default_chord))
        })
        .filter_map(|entry| {
            let default_chord = platform_default_chord(entry.default_chord);
            // Shadow the default chord only when no other entry currently uses it (avoid collateral damage).
            let default_in_use_elsewhere = REGISTRY.iter().any(|other| {
                other.id != entry.id
                    && !config.disabled.contains(other.id)
                    && effective_chord(other.id, config) == Some(default_chord)
            });
            if default_in_use_elsewhere {
                None
            } else {
                Some((entry.id, default_chord.to_string(), entry.scope))
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique() {
        let mut seen = BTreeSet::new();
        for entry in REGISTRY {
            assert!(seen.insert(entry.id), "duplicate ShortcutId: {}", entry.id);
        }
    }

    #[test]
    fn effective_chord_returns_default_when_no_override() {
        let config = ShortcutConfig::default();
        assert_eq!(
            effective_chord("app.quit", &config),
            Some(platform_default_chord("cmd-q"))
        );
    }

    #[test]
    fn effective_chord_returns_override_when_set() {
        let mut config = ShortcutConfig::default();
        config
            .overrides
            .insert("app.quit".to_string(), vec!["cmd-shift-q".to_string()]);
        assert_eq!(effective_chord("app.quit", &config), Some("cmd-shift-q"));
    }

    #[test]
    fn effective_chord_returns_none_when_disabled() {
        let mut config = ShortcutConfig::default();
        config.disabled.insert("app.quit".to_string());
        assert_eq!(effective_chord("app.quit", &config), None);
    }

    #[test]
    fn no_conflicts_with_default_config() {
        let config = ShortcutConfig::default();
        let conflicts = detect_conflicts(&config);
        // Default registry should be conflict-free within same scope
        let same_scope_conflicts: Vec<_> = conflicts
            .iter()
            .filter(|c| {
                let entries: Vec<_> = c.ids.iter().filter_map(|id| find_entry(id)).collect();
                entries.windows(2).all(|w| w[0].scope == w[1].scope)
            })
            .collect();
        assert!(
            same_scope_conflicts.is_empty(),
            "default registry has same-scope conflicts: {same_scope_conflicts:?}"
        );
    }

    #[test]
    fn conflict_detected_for_duplicate_chord_in_same_scope() {
        let mut config = ShortcutConfig::default();
        let primary = if cfg!(target_os = "macos") {
            "cmd-q"
        } else {
            "ctrl-q"
        };
        config
            .overrides
            .insert("app.search".to_string(), vec![primary.to_string()]);
        let conflicts = detect_conflicts(&config);
        let has_primary_conflict = conflicts.iter().any(|c| c.chord == primary);
        assert!(has_primary_conflict, "expected conflict on {primary}");
    }

    #[test]
    fn format_chord_display_renders_mac_symbols() {
        assert_eq!(format_chord_display("cmd-q"), "⌘Q");
        assert_eq!(format_chord_display("cmd-shift-a"), "⌘⇧A");
        assert_eq!(format_chord_display("cmd-alt-left"), "⌘⌥←");
        assert_eq!(format_chord_display("ctrl-1"), "⌃1");
        assert_eq!(format_chord_display("f1"), "F1");
    }

    #[test]
    fn display_chord_for_id_works() {
        let config = ShortcutConfig::default();
        assert_eq!(
            display_chord_for_id("app.quit", &config),
            Some(format_chord_display(platform_default_chord("cmd-q")))
        );
        assert_eq!(
            display_chord_for_id("sidebar.toggle", &config),
            Some(format_chord_display(platform_default_chord("cmd-b")))
        );
    }

    #[test]
    fn platform_profile_keeps_action_ids_stable() {
        assert_eq!(platform_default_chord("f1"), "f1");
        assert_eq!(platform_default_chord("ctrl-1"), "ctrl-1");

        #[cfg(target_os = "macos")]
        assert_eq!(platform_default_chord("cmd-k"), "cmd-k");

        #[cfg(not(target_os = "macos"))]
        assert_eq!(platform_default_chord("cmd-k"), "ctrl-k");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn every_non_macos_cmd_default_uses_ctrl_profile() {
        for entry in REGISTRY {
            if entry.default_chord.starts_with("cmd-") {
                assert!(
                    platform_default_chord(entry.default_chord).starts_with("ctrl-"),
                    "unmapped non-macOS default for {}: {}",
                    entry.id,
                    entry.default_chord
                );
            }
        }
    }

    #[test]
    fn find_entry_returns_correct_entry() {
        let entry = find_entry("terminal.paste").unwrap();
        assert_eq!(entry.label, "Paste");
        assert_eq!(entry.scope, ShortcutScope::App);
    }

    #[test]
    fn unknown_id_returns_none() {
        assert!(find_entry("nonexistent.command").is_none());
        let config = ShortcutConfig::default();
        assert_eq!(effective_chord("nonexistent.command", &config), None);
    }
}
