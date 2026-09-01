//! Shardlane Workspace configuration data model: the `workspaces` section of
//! `config.json`.
//!
//! A Shardlane Workspace is a user-owned client-side grouping (persisted in
//! config.json), a different concept from the Herdr runtime Workspace (the
//! source of the Project projection).
//!
//! [INPUT]: depends on serde (config serialization); zero runtime/GUI
//! coupling
//! [OUTPUT]: exposes WorkspacesConfig/WorkspaceConfig,
//! DEFAULT_WORKSPACE_ID, WORKSPACE_COLOR_PALETTE, parse_workspace_color
//! [POS]: the configuration data layer of shardlane-host; GUI settings.rs
//! (embedded in ApplicationConfig + re-exported) and the shardlane-remote
//! bootstrap (membership resolution) share the same serde facts

use serde::{Deserialize, Serialize};

pub const DEFAULT_WORKSPACE_ID: &str = "workspace-main";
pub const WORKSPACE_COLOR_PALETTE: [&str; 8] = [
    "#7C8CFF", "#56B6C2", "#98C379", "#E5C07B", "#E06C75", "#C678DD", "#61AFEF", "#D19A66",
];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct WorkspacesConfig {
    pub active_id: String,
    /// Ordered Shardlane Workspace list. Order is the Sidebar switcher order.
    pub items: Vec<WorkspaceConfig>,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            active_id: DEFAULT_WORKSPACE_ID.to_string(),
            items: vec![WorkspaceConfig::default()],
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub id: String,
    pub name: String,
    /// Hex RGB used by the compact Sidebar Workspace switcher.
    pub color: String,
    /// Project paths explicitly assigned to this Workspace. Projects not assigned anywhere
    /// fall back to the first Workspace so existing installs never lose visible Projects.
    pub project_paths: Vec<String>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            id: DEFAULT_WORKSPACE_ID.to_string(),
            name: "Default".to_string(),
            color: WORKSPACE_COLOR_PALETTE[0].to_string(),
            project_paths: Vec::new(),
        }
    }
}

impl WorkspaceConfig {
    pub fn color_rgb(&self) -> u32 {
        parse_workspace_color(&self.color).unwrap_or(0x7C8CFF)
    }
}

pub fn parse_workspace_color(value: &str) -> Option<u32> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    u32::from_str_radix(value, 16)
        .ok()
        .filter(|value| *value <= 0x00ff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_config_defaults_and_round_trip() {
        let config = WorkspacesConfig::default();
        assert_eq!(config.active_id, DEFAULT_WORKSPACE_ID);
        assert_eq!(config.items.len(), 1);
        assert_eq!(config.items[0].color_rgb(), 0x7C8CFF);

        let raw = serde_json::json!({
            "active_id": "ws-2",
            "items": [{ "id": "ws-2", "name": "Work", "color": "#56B6C2", "project_paths": ["/tmp/demo"] }]
        });
        let parsed: WorkspacesConfig = serde_json::from_value(raw).unwrap_or_default();
        assert_eq!(parsed.active_id, "ws-2");
        assert_eq!(parsed.items[0].color_rgb(), 0x56B6C2);
        assert_eq!(parsed.items[0].project_paths, vec!["/tmp/demo".to_string()]);
    }

    #[test]
    fn parse_workspace_color_rejects_malformed_values() {
        assert_eq!(parse_workspace_color("#7C8CFF"), Some(0x7C8CFF));
        assert_eq!(
            parse_workspace_color("7C8CFF"),
            Some(0x7C8CFF),
            "settings legacy semantics tolerate a missing #"
        );
        assert_eq!(parse_workspace_color("#12345"), None);
        assert_eq!(parse_workspace_color("#GGGGGG"), None);
    }
}
