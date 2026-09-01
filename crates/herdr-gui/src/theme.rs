use gpui::{px, rgb, App, Pixels, SharedString};

/// Shardlane application typography tokens. Terminal text remains user-configurable and
/// intentionally does not use these fixed UI sizes.
/// Headline size for sentence-style titles (New Agent "What should we do in …?") — finalized at 28px.
pub const FONT_HEADLINE: Pixels = px(28.0);
pub const FONT_APP_TITLE: Pixels = px(18.0);
pub const FONT_SECTION_TITLE: Pixels = px(14.0);
pub const FONT_LIST_TITLE: Pixels = px(13.0);
pub const FONT_BODY: Pixels = px(12.0);
pub const FONT_DESCRIPTION: Pixels = px(12.0);
pub const FONT_META: Pixels = px(11.0);
pub const FONT_DECORATIVE: Pixels = px(10.0);

pub const WASH_HOVER: f32 = 0.06;
pub const WASH_ACTIVE: f32 = 0.09;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemePresetCategory {
    Light,
    Dark,
}

impl ThemePresetCategory {
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    pub fn label(self) -> SharedString {
        match self {
            Self::Light => crate::i18n::t("settings.theme_category.light"),
            Self::Dark => crate::i18n::t("settings.theme_category.dark"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemePreset {
    pub id: &'static str,
    pub label: &'static str,
    pub category: ThemePresetCategory,
    pub appearance: &'static str,
    /// Exact Herdr built-in theme written to Shardlane's derived Herdr config.
    pub herdr_theme: &'static str,
    /// Shardlane chrome family paired with the Herdr theme above.
    pub scheme: &'static str,
}

pub const THEME_PRESETS: [ThemePreset; 17] = [
    ThemePreset {
        id: "catppuccin-latte",
        label: "Catppuccin Latte",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "catppuccin-latte",
        scheme: "catppuccin",
    },
    ThemePreset {
        id: "tokyo-day",
        label: "Tokyo Day",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "tokyo-night-day",
        scheme: "tokyo-night",
    },
    ThemePreset {
        id: "gruvbox-light",
        label: "Gruvbox Light",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "gruvbox-light",
        scheme: "gruvbox",
    },
    ThemePreset {
        id: "one-light",
        label: "One Light",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "one-light",
        scheme: "one-dark",
    },
    ThemePreset {
        id: "solarized-light",
        label: "Solarized Light",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "solarized-light",
        scheme: "solarized",
    },
    ThemePreset {
        id: "kanagawa-lotus",
        label: "Kanagawa Lotus",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "kanagawa-lotus",
        scheme: "kanagawa",
    },
    ThemePreset {
        id: "rose-pine-dawn",
        label: "Rosé Pine Dawn",
        category: ThemePresetCategory::Light,
        appearance: "light",
        herdr_theme: "rose-pine-dawn",
        scheme: "rose-pine",
    },
    ThemePreset {
        id: "catppuccin-mocha",
        label: "Catppuccin Mocha",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "catppuccin",
        scheme: "catppuccin",
    },
    ThemePreset {
        id: "tokyo-night",
        label: "Tokyo Night",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "tokyo-night",
        scheme: "tokyo-night",
    },
    ThemePreset {
        id: "gruvbox-dark",
        label: "Gruvbox Dark",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "gruvbox",
        scheme: "gruvbox",
    },
    ThemePreset {
        id: "one-dark",
        label: "One Dark",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "one-dark",
        scheme: "one-dark",
    },
    ThemePreset {
        id: "solarized-dark",
        label: "Solarized Dark",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "solarized",
        scheme: "solarized",
    },
    ThemePreset {
        id: "kanagawa-wave",
        label: "Kanagawa Wave",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "kanagawa",
        scheme: "kanagawa",
    },
    ThemePreset {
        id: "rose-pine",
        label: "Rosé Pine",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "rose-pine",
        scheme: "rose-pine",
    },
    ThemePreset {
        id: "dracula",
        label: "Dracula",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "dracula",
        scheme: "dracula",
    },
    ThemePreset {
        id: "nord",
        label: "Nord",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "nord",
        scheme: "nord",
    },
    ThemePreset {
        id: "vesper",
        label: "Vesper",
        category: ThemePresetCategory::Dark,
        appearance: "dark",
        herdr_theme: "vesper",
        scheme: "vesper",
    },
];

pub fn preset_for_herdr_theme(name: &str) -> Option<ThemePreset> {
    THEME_PRESETS
        .iter()
        .copied()
        .find(|preset| preset.herdr_theme == name)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UiTheme {
    pub bg: u32,
    pub panel: u32,
    pub terminal: u32,
    pub text: u32,
    pub label: u32,
    pub muted: u32,
    pub hover: u32,
    pub active: u32,
    pub border: u32,
}

const DEFAULT_THEMES_JSON: &str = r#"{
  "catppuccin": ["1e1e2e", "181825", "1e1e2e", "cdd6f4", "a6adc8", "313244", "45475a"],
  "catppuccin-latte": ["f5f5f5", "eff1f5", "ffffff", "4c4f69", "6c6f85", "e6e9ef", "ccd0da"],
  "tokyo-night": ["1a1b26", "1a1b26", "11121a", "c0caf5", "a9b1d6", "24283b", "414868"],
  "tokyo-night-day": ["e1e2e7", "e1e2e7", "f8f8fb", "3760bf", "6172b0", "d2d3da", "c4c8da"],
  "dracula": ["282a36", "282a36", "15161c", "f8f8f2", "d2d2dc", "44475a", "6272a4"],
  "nord": ["2e3440", "2e3440", "20242d", "eceff4", "d8dee9", "3b4252", "434c5e"],
  "gruvbox": ["282828", "282828", "1d2021", "ebdbb2", "d5c4a1", "3c3836", "504945"],
  "gruvbox-light": ["fbf1c7", "fbf1c7", "fffff0", "3c3836", "504945", "f2e5bc", "ebdbb2"],
  "one-dark": ["282c34", "282c34", "1f2329", "abb2bf", "969ca8", "2c313a", "3e4451"],
  "one-light": ["fafafa", "fafafa", "ffffff", "383a42", "686b77", "f5f5f6", "e5e5e6"],
  "solarized": ["002b36", "002b36", "001f27", "93a1a1", "839496", "073642", "586e75"],
  "solarized-light": ["fdf6e3", "fdf6e3", "fffff4", "657b83", "839496", "eee8d5", "93a1a1"],
  "kanagawa": ["1f1f28", "1f1f28", "16161d", "dcd7ba", "c8c3aa", "2a2a37", "363646"],
  "kanagawa-lotus": ["f2ecbc", "f2ecbc", "fffae0", "545464", "43436c", "d5cea3", "dcd5ac"],
  "rose-pine": ["191724", "191724", "111019", "e0def4", "c8c5dc", "1f1d2e", "26233a"],
  "rose-pine-dawn": ["faf4ed", "faf4ed", "fffbf5", "464261", "797593", "f2e9e1", "fffaf3"],
  "vesper": ["1a1a1a", "1a1a1a", "101010", "ffffff", "a0a0a0", "232323", "282828"]
}"#;

static THEMES: OnceLock<HashMap<String, UiTheme>> = OnceLock::new();

pub fn shardlane_theme(name: &str) -> UiTheme {
    THEMES
        .get_or_init(load_themes)
        .get(name)
        .copied()
        .unwrap_or_else(default_theme)
}

pub fn theme_for_scheme(name: &str, dark: bool) -> UiTheme {
    if name == "shardlane-native" || name.trim().is_empty() {
        return shardlane_native_theme(dark);
    }

    let paired = match name {
        "catppuccin" | "catppuccin-latte" => {
            if dark {
                "catppuccin"
            } else {
                "catppuccin-latte"
            }
        }
        "tokyo-night" | "tokyo-night-day" => {
            if dark {
                "tokyo-night"
            } else {
                "tokyo-night-day"
            }
        }
        "gruvbox" | "gruvbox-light" => {
            if dark {
                "gruvbox"
            } else {
                "gruvbox-light"
            }
        }
        "one-dark" | "one-light" => {
            if dark {
                "one-dark"
            } else {
                "one-light"
            }
        }
        "solarized" | "solarized-light" => {
            if dark {
                "solarized"
            } else {
                "solarized-light"
            }
        }
        "kanagawa" | "kanagawa-lotus" => {
            if dark {
                "kanagawa"
            } else {
                "kanagawa-lotus"
            }
        }
        "rose-pine" | "rose-pine-dawn" => {
            if dark {
                "rose-pine"
            } else {
                "rose-pine-dawn"
            }
        }
        other if dark => other,
        _ => return shardlane_native_theme(false),
    };
    shardlane_theme(paired)
}

pub fn shardlane_native_theme(dark: bool) -> UiTheme {
    if dark {
        UiTheme {
            bg: 0x1c1c1e,
            panel: 0x252527,
            terminal: 0x161618,
            text: 0xf5f5f7,
            label: 0xf5f5f7,
            muted: 0x98989f,
            hover: 0x303034,
            active: 0x3a3a40,
            border: 0x3a3a3c,
        }
    } else {
        UiTheme {
            bg: 0xf7f7f8,
            panel: 0xeeeef0,
            terminal: 0xffffff,
            text: 0x1d1d1f,
            label: 0x1d1d1f,
            muted: 0x707078,
            hover: 0xe4e4e7,
            active: 0xd8d8dc,
            border: 0xd1d1d6,
        }
    }
}

fn load_themes() -> HashMap<String, UiTheme> {
    let path = themes_path();
    if !path.exists() {
        write_default_themes(&path);
    }

    // Built-ins are product defaults; themes.json is an override layer rather than a
    // replacement catalog. This lets Shardlane add/fix presets without invalidating an older
    // user file, while still allowing the user to override any named scheme intentionally.
    let mut themes = parse_themes(DEFAULT_THEMES_JSON).unwrap_or_else(|_| {
        let mut fallback = HashMap::new();
        fallback.insert("oled".to_string(), default_theme());
        fallback
    });
    if let Ok(json) = std::fs::read_to_string(&path) {
        if let Ok(overrides) = parse_themes(&json) {
            themes.extend(overrides);
        }
    }
    themes
}

fn parse_themes(json: &str) -> Result<HashMap<String, UiTheme>, String> {
    let raw: HashMap<String, Vec<String>> =
        serde_json::from_str(json).map_err(|err| err.to_string())?;
    let mut themes = HashMap::new();
    for (name, colors) in raw {
        if colors.len() != 7 {
            continue;
        }
        let parse = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
        let text = parse(&colors[3]);
        let active = parse(&colors[6]);
        themes.insert(
            name,
            UiTheme {
                bg: parse(&colors[0]),
                panel: parse(&colors[1]),
                terminal: parse(&colors[2]),
                text,
                label: text,
                muted: parse(&colors[4]),
                hover: parse(&colors[5]),
                active,
                border: active,
            },
        );
    }
    Ok(themes)
}

fn write_default_themes(path: &std::path::Path) {
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(path, DEFAULT_THEMES_JSON);
}

fn themes_path() -> PathBuf {
    crate::settings::app_data_dir().join("themes.json")
}

fn default_theme() -> UiTheme {
    UiTheme {
        bg: 0x000000,
        panel: 0x000000,
        terminal: 0x000000,
        text: 0xffffff,
        label: 0xffffff,
        // UX (2026-08-27 review): 0x88 hint text on a black background (e.g. the History
        // empty state) is nearly unreadable; brighten two steps to above the WCAG boundary.
        muted: 0x9a9aa2,
        hover: 0x1a1a1a,
        active: 0x2a2a2a,
        border: 0x2a2a2a,
    }
}

/// Sync the gpui-component global `Theme` to match the current `UiTheme`.
///
/// Maps the 9-field UiTheme onto the gpui-component ThemeColor semantic slots
/// so that gpui-component widgets (Button, Sidebar, Tabs, etc.) render with
/// colors consistent with the active Shardlane theme.
#[allow(dead_code)]
pub fn sync_gpui_component_theme(ui: UiTheme, cx: &mut App) {
    let is_dark = ui.bg < 0x808080;
    let mode = if is_dark {
        gpui_component::theme::ThemeMode::Dark
    } else {
        gpui_component::theme::ThemeMode::Light
    };

    let theme = gpui_component::theme::Theme::global_mut(cx);
    theme.mode = mode;

    let c = &mut theme.colors;
    let bg: gpui::Hsla = rgb(ui.bg).into();
    let panel: gpui::Hsla = rgb(ui.panel).into();
    let text: gpui::Hsla = rgb(ui.text).into();
    let muted: gpui::Hsla = rgb(ui.muted).into();
    let hover: gpui::Hsla = rgb(ui.hover).into();
    let active: gpui::Hsla = rgb(ui.active).into();
    let border: gpui::Hsla = rgb(ui.border).into();

    c.background = bg;
    c.foreground = text;
    c.border = border;
    c.muted = hover;
    c.muted_foreground = muted;

    c.primary = active;
    c.primary_foreground = text;
    c.primary_hover = hover;
    c.primary_active = active;

    c.secondary = panel;
    c.secondary_foreground = text;
    c.secondary_hover = hover;
    c.secondary_active = active;

    c.accent = hover;
    c.accent_foreground = text;

    c.sidebar = panel;
    c.sidebar_foreground = text;
    c.sidebar_border = border;
    c.sidebar_accent = hover;
    c.sidebar_accent_foreground = text;
    c.sidebar_primary = active;
    c.sidebar_primary_foreground = text;

    c.tab_bar = panel;
    c.tab_bar_segmented = hover;
    c.tab = panel;
    c.tab_active = active;
    c.tab_foreground = muted;
    c.tab_active_foreground = text;

    c.popover = panel;
    c.popover_foreground = text;

    c.list = panel;
    c.list_hover = hover;
    c.list_active = active;
    c.list_active_border = border;

    c.title_bar = panel;
    // Keep the native titlebar visually continuous with the sidebar rather than
    // drawing a persistent divider under the traffic-light region.
    c.title_bar_border = panel;

    c.scrollbar = panel;
    c.scrollbar_thumb = active;
    c.scrollbar_thumb_hover = muted;

    c.input = border;
    c.caret = text;
    c.ring = active;
    c.selection = if is_dark {
        rgb(0x264f78).into()
    } else {
        rgb(0xadd6ff).into()
    };
}

#[cfg(test)]
mod tests {
    use super::{theme_for_scheme, ThemePresetCategory, THEME_PRESETS};

    #[test]
    fn native_and_paired_app_schemes_follow_global_appearance() {
        assert!(theme_for_scheme("shardlane-native", false).bg > 0x808080);
        assert!(theme_for_scheme("shardlane-native", true).bg < 0x808080);
    }

    #[test]
    fn theme_presets_are_unified_app_and_official_herdr_choices() {
        assert_eq!(THEME_PRESETS.len(), 17);
        assert!(THEME_PRESETS.iter().all(|preset| !preset.id.is_empty()));
        for preset in THEME_PRESETS {
            let dark = preset.category == ThemePresetCategory::Dark;
            assert_eq!(preset.appearance, if dark { "dark" } else { "light" });
            assert!(
                !preset.herdr_theme.is_empty(),
                "{} needs a Herdr theme",
                preset.id
            );
            let app_theme = theme_for_scheme(preset.scheme, dark);
            assert_eq!(
                app_theme.bg < 0x808080,
                dark,
                "{} appearance mismatch",
                preset.id
            );
        }
    }

    #[test]
    fn light_and_dark_cards_carry_exact_herdr_builtins() {
        let Some(one_light) = THEME_PRESETS.iter().find(|preset| preset.id == "one-light") else {
            panic!("one-light preset missing");
        };
        let Some(one_dark) = THEME_PRESETS.iter().find(|preset| preset.id == "one-dark") else {
            panic!("one-dark preset missing");
        };
        assert_eq!(one_light.herdr_theme, "one-light");
        assert_eq!(one_dark.herdr_theme, "one-dark");
    }
}
