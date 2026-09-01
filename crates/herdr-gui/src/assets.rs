//! [INPUT]: Depends on gpui AssetSource, base64
//! [OUTPUT]: Exposes Assets (AssetSource), agent_brand_icon
//! [POS]: Unified embedded static asset bundle for crates/herdr-gui

use base64::{engine::general_purpose::STANDARD, Engine as _};
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// Central embedded asset source for Shardlane chrome and Agent identity.
///
/// Interface SVGs are Lucide (ISC). Agent marks use MIT-licensed lobe-icons assets
/// and are intentionally rendered as PNGs so their original brand colors are never
/// inherited from text styling. Exception: `brands/command-code` is Command Code's
/// own favicon (vendor-provided, monochrome), fetched from commandcode.ai.
pub struct Assets;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        fn lookup_icon(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $name, ".svg") =>
                    Some(include_bytes!(concat!("../assets/icons/", $name, ".svg"))),)*
                _ => None,
            }
        }
    };
}

macro_rules! brands {
    ($($name:literal),* $(,)?) => {
        fn lookup_brand(path: &str) -> Option<&'static str> {
            match path {
                $(concat!("brands/", $name, ".png") =>
                    Some(include_str!(concat!("../assets/brands/", $name, ".b64"))),)*
                _ => None,
            }
        }
    };
}

icons!(
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "arrow-up",
    "case-sensitive",
    "chart-column",
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "chevrons-up-down",
    "compose",
    "copy",
    "corner-down-right",
    "ellipsis",
    "eye",
    "eye-off",
    "external-link",
    "file",
    "folder",
    "folder-new",
    "file-types/biome",
    "file-types/bun",
    "file-types/c",
    "file-types/certificate",
    "file-types/cmake",
    "file-types/console",
    "file-types/cpp",
    "file-types/csharp",
    "file-types/css",
    "file-types/database",
    "file-types/docker",
    "file-types/editorconfig",
    "file-types/eslint",
    "file-types/git",
    "file-types/go",
    "file-types/html",
    "file-types/image",
    "file-types/java",
    "file-types/javascript",
    "file-types/json",
    "file-types/kotlin",
    "file-types/lua",
    "file-types/makefile",
    "file-types/markdown",
    "file-types/nodejs",
    "file-types/npm",
    "file-types/php",
    "file-types/pnpm",
    "file-types/prettier",
    "file-types/python",
    "file-types/react",
    "file-types/readme",
    "file-types/ruby",
    "file-types/rust",
    "file-types/sass",
    "file-types/settings",
    "file-types/svelte",
    "file-types/swift",
    "file-types/tailwindcss",
    "file-types/typescript",
    "file-types/vite",
    "file-types/vue",
    "file-types/xml",
    "file-types/yaml",
    "file-types/yarn",
    "file-types/zig",
    "file-types/zip",
    "git-branch",
    "globe",
    "github",
    "info",
    "layers",
    "list-filter",
    "loader-circle",
    "lock",
    "lock-open",
    "panel-left",
    "panel-right",
    "paperclip",
    "pencil",
    "pin",
    "plus",
    "refresh-cw",
    "replace",
    "rotate-cw",
    "search",
    "smartphone",
    "sparkle",
    "square-pen",
    "star",
    "terminal",
    "trash",
    "window-maximize",
    "window-minimize",
    "window-restore",
    "x",
);

brands!(
    "antigravity",
    "claude-code",
    "codex",
    "command-code",
    "copilot",
    "copilot-light",
    "cursor",
    "cursor-light",
    "deepseek",
    "gemini",
    "grok",
    "grok-light",
    "kimi",
    "kimi-light",
    "kiro",
    "omp",
    "opencode",
    "opencode-light",
    "pi",
    "pi-light",
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = lookup_icon(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(encoded) = lookup_brand(path) {
            return STANDARD
                .decode(encoded.trim())
                .map(Cow::Owned)
                .map(Some)
                .map_err(Into::into);
        }

        match gpui_component_assets::Assets.load(path) {
            Ok(asset) => Ok(asset),
            Err(err) if err.to_string().starts_with("could not find asset at path") => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

/// Returns the original-color Agent brand image for known coding Agents.
/// Unknown runtime agents deliberately fall back to a Lucide terminal icon.
/// Monochrome glyphs (copilot/cursor/opencode/pi/grok/kimi) render white on dark
/// and use their `-light` ink variant in light mode; colored brands stay put.
/// Qoder intentionally has no mapping until a redistributable official asset is
/// verified; callers fall back to the neutral Bot icon.
pub fn agent_brand_icon(agent: &str, dark: bool) -> Option<&'static str> {
    match agent.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude code" => Some("brands/claude-code.png"),
        "codex" | "codex-cli" => Some("brands/codex.png"),
        "copilot" | "copilot-cli" | "github-copilot" => Some(if dark {
            "brands/copilot.png"
        } else {
            "brands/copilot-light.png"
        }),
        "cursor" | "cursor-agent" => Some(if dark {
            "brands/cursor.png"
        } else {
            "brands/cursor-light.png"
        }),
        "opencode" | "open-code" => Some(if dark {
            "brands/opencode.png"
        } else {
            "brands/opencode-light.png"
        }),
        "kiro" => Some("brands/kiro.png"),
        "gemini" | "gemini-cli" => Some("brands/gemini.png"),
        "pi" => Some(if dark {
            "brands/pi.png"
        } else {
            "brands/pi-light.png"
        }),
        "omp" | "oh-my-pi" => Some("brands/omp.png"),
        "grok" | "grok-build" => Some(if dark {
            "brands/grok.png"
        } else {
            "brands/grok-light.png"
        }),
        "kimi" | "kimi-code" => Some(if dark {
            "brands/kimi.png"
        } else {
            "brands/kimi-light.png"
        }),
        "antigravity" | "antigravity-cli" | "agy" => Some("brands/antigravity.png"),
        "commandcode" | "command-code" | "command code" => Some("brands/command-code.png"),
        "dsh" | "deepseek" | "deepseek-harness" => Some("brands/deepseek.png"),
        "qoder" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_interface_assets_are_embedded_and_themeable() {
        for path in [
            "icons/search.svg",
            "icons/folder.svg",
            "icons/refresh-cw.svg",
            "icons/terminal.svg",
            "icons/chevron-right.svg",
            "icons/layers.svg",
            "icons/pin.svg",
        ] {
            let asset = Assets
                .load(path)
                .unwrap_or_else(|err| panic!("{path}: {err}"))
                .unwrap_or_else(|| panic!("missing interface asset {path}"));
            assert!(asset.starts_with(b"<!-- @license lucide-static"));
            assert!(
                asset
                    .windows(b"currentColor".len())
                    .any(|window| window == b"currentColor"),
                "{path} must inherit the active theme color"
            );
        }
    }

    #[test]
    fn gpui_component_icons_fall_back_to_default_asset_bundle() {
        for path in [
            "icons/book-open.svg",
            "icons/bot.svg",
            "icons/folder-open.svg",
            "icons/panel-left.svg",
            "icons/panel-left-open.svg",
            "icons/settings.svg",
            "icons/square-terminal.svg",
        ] {
            let asset = Assets
                .load(path)
                .unwrap_or_else(|err| panic!("{path}: {err}"))
                .unwrap_or_else(|| panic!("missing component icon {path}"));
            assert!(!asset.is_empty(), "empty component icon {path}");
            assert!(
                asset
                    .windows(b"currentColor".len())
                    .any(|window| window == b"currentColor"),
                "{path} must inherit the active theme color"
            );
        }
    }

    #[test]
    fn brand_assets_decode_to_png_bytes() {
        let brand_paths = [
            "brands/antigravity.png",
            "brands/claude-code.png",
            "brands/codex.png",
            "brands/copilot.png",
            "brands/copilot-light.png",
            "brands/cursor.png",
            "brands/cursor-light.png",
            "brands/deepseek.png",
            "brands/gemini.png",
            "brands/grok.png",
            "brands/grok-light.png",
            "brands/kimi.png",
            "brands/kimi-light.png",
            "brands/kiro.png",
            "brands/omp.png",
            "brands/opencode.png",
            "brands/opencode-light.png",
            "brands/pi.png",
            "brands/pi-light.png",
        ];
        for path in brand_paths {
            let asset = Assets
                .load(path)
                .unwrap_or_else(|err| panic!("{path}: {err}"))
                .unwrap_or_else(|| panic!("missing brand asset {path}"));
            assert_eq!(
                &asset[..8],
                b"\x89PNG\r\n\x1a\n",
                "{path} must decode to PNG"
            );
        }
    }

    #[test]
    fn brand_lookup_handles_aliases_and_appearance_variants() {
        assert_eq!(
            agent_brand_icon("cursor-agent", false),
            Some("brands/cursor-light.png")
        );
        assert_eq!(
            agent_brand_icon("Claude Code", true),
            Some("brands/claude-code.png")
        );
        assert_eq!(agent_brand_icon("pi", true), Some("brands/pi.png"));
        assert_eq!(agent_brand_icon("pi", false), Some("brands/pi-light.png"));
        assert_eq!(agent_brand_icon("omp", true), Some("brands/omp.png"));
        assert_eq!(
            agent_brand_icon("grok-build", false),
            Some("brands/grok-light.png")
        );
        assert_eq!(agent_brand_icon("kimi-code", true), Some("brands/kimi.png"));
        assert_eq!(
            agent_brand_icon("antigravity", false),
            Some("brands/antigravity.png")
        );
        assert_eq!(agent_brand_icon("dsh", true), Some("brands/deepseek.png"));
        assert_eq!(agent_brand_icon("qoder", true), None);
        assert_eq!(agent_brand_icon("unknown-agent", true), None);
    }

    #[test]
    fn every_provider_resolves_a_brand_icon_via_slug_and_protocol_label() {
        use shardlane_history::AgentId;

        // Sidebar/History/Chat icon keys come from three sources: the history slug, the
        // Herdr protocol short label (the display_agent/agent projection of agent_identity),
        // and display aliases. Providers with confirmed brand assets must pass the PNG
        // regression; a Preview provider whose assets are not yet confirmed redistributable
        // (currently Qoder) may fall back to the UI's neutral Bot icon.
        for agent in AgentId::ALL {
            let slug = agent.as_str();
            let label = crate::agent_cli::herdr_agent_id(agent);
            for key in [slug, label] {
                let Some(path) = agent_brand_icon(key, true) else {
                    assert_eq!(
                        agent,
                        AgentId::Qoder,
                        "only an explicitly preview/generic provider may lack a brand asset: {agent:?}"
                    );
                    continue;
                };
                let asset = Assets
                    .load(path)
                    .unwrap_or_else(|err| panic!("{path}: {err}"))
                    .unwrap_or_else(|| panic!("missing brand asset {path}"));
                assert_eq!(
                    &asset[..8],
                    b"\x89PNG\r\n\x1a\n",
                    "{path} must decode to PNG"
                );
            }
        }
        // Command Code's three key forms (protocol label / history slug / display name).
        for key in ["commandcode", "command-code", "command code"] {
            assert_eq!(
                agent_brand_icon(key, true),
                Some("brands/command-code.png"),
                "key={key}"
            );
        }
        // Antigravity's Herdr protocol short label (the value agent_identity actually sends).
        assert_eq!(
            agent_brand_icon("agy", true),
            Some("brands/antigravity.png")
        );
    }
}
