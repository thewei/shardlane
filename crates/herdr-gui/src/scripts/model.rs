//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides the script data model and persistence (ScriptKind/ScriptStatus/ScriptDefinition/ScriptRecord/ScriptRegistry, registry persistence, id/icon/keybinding normalization)
//! [POS]: The data-model slice of the scripts module, mechanically split out of scripts.rs
use super::*;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScriptKind {
    Command,
    #[default]
    Service,
    Debugger,
}

impl ScriptKind {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "command" | "cmd" | "once" => Some(Self::Command),
            "service" | "server" | "long" => Some(Self::Service),
            "debugger" | "debug" | "gdb" | "lldb" => Some(Self::Debugger),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Service => "Service",
            Self::Debugger => "Debugger",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScriptStatus {
    Starting,
    Running,
    Failed,
    #[default]
    Stopped,
}

impl ScriptStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Failed => "Failed",
            Self::Stopped => "Stopped",
        }
    }
}

pub(super) fn default_script_icon() -> String {
    "terminal".to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct ScriptDefinition {
    pub id: String,
    pub project_path: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_script_icon")]
    pub icon: String,
    pub keybinding: Option<String>,
    /// User-authored command script. The native editor keeps one command per line; execution
    /// compiles non-empty lines into one fail-fast shell chain.
    pub command: String,
    pub kind: ScriptKind,
    pub one_shot: bool,
    pub close_on_complete: bool,
    pub last_run_at_ms: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Default for ScriptDefinition {
    fn default() -> Self {
        Self {
            id: String::new(),
            project_path: String::new(),
            name: String::new(),
            description: String::new(),
            icon: default_script_icon(),
            keybinding: None,
            command: String::new(),
            kind: ScriptKind::default(),
            one_shot: false,
            close_on_complete: false,
            last_run_at_ms: None,
            tags: Vec::new(),
        }
    }
}

impl ScriptDefinition {
    pub(crate) fn assembled_command(&self) -> String {
        self.command
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" && ")
    }

    pub(crate) fn is_one_shot(&self) -> bool {
        self.one_shot
    }

    pub(crate) fn command_summary(&self) -> String {
        self.command
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScriptRuntimeProjection {
    pub status: ScriptStatus,
    pub pid: Option<u32>,
    pub ports: Vec<u16>,
    pub started_at_ms: Option<u64>,
    pub last_error: Option<String>,
}

/// Read-only projection of a long-running service already executing in a Herdr Pane.
///
/// This is intentionally not persisted: Herdr remains the runtime authority, and Shardlane
/// only surfaces a Pane as an observed Script when its foreground process owns a listening port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObservedService {
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub pane_name: String,
    pub command: String,
    pub pid: u32,
    pub ports: Vec<u16>,
}

impl ObservedService {
    pub(crate) fn ports_label(&self) -> String {
        match self.ports.as_slice() {
            [] => String::new(),
            [port] => format!(":{port}"),
            ports => format!(":{} +{}", ports[0], ports.len() - 1),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct ScriptRecord {
    #[serde(flatten)]
    pub definition: ScriptDefinition,
    /// Current Herdr correlation only. `project_path` remains the stable semantic identity.
    pub workspace_id: String,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
    /// Disposable runtime projection. Rebuilt from Herdr/process inspection after load.
    #[serde(skip)]
    pub runtime: ScriptRuntimeProjection,
}

impl Deref for ScriptRecord {
    type Target = ScriptDefinition;

    fn deref(&self) -> &Self::Target {
        &self.definition
    }
}

impl DerefMut for ScriptRecord {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.definition
    }
}

impl ScriptRecord {
    #[cfg(test)]
    pub(crate) fn display_label(&self) -> String {
        match self.runtime.ports.as_slice() {
            [] => self.name.clone(),
            [port] => format!("{} · :{port}", self.name),
            ports => format!("{} · :{} +{}", self.name, ports[0], ports.len() - 1),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct ScriptRegistry {
    /// serde alias: compatible with the `"tasks"` root key of pre-2026-08-25
    /// tasks.json; reads old, writes new.
    #[serde(default, alias = "tasks")]
    pub scripts: Vec<ScriptRecord>,
}

impl ScriptRegistry {
    pub(crate) fn load() -> Self {
        let path = script_registry_path();
        if !path.exists() {
            // One-time migration of the legacy tasks.json: read (resolved via the
            // alias) → write scripts.json → remove the old file.
            let legacy = settings::app_data_dir().join("tasks.json");
            if legacy.exists() {
                let migrated = std::fs::read_to_string(&legacy)
                    .ok()
                    .and_then(|json| serde_json::from_str::<Self>(&json).ok())
                    .unwrap_or_default();
                migrated.save();
                let _ = std::fs::remove_file(&legacy);
                return migrated;
            }
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save(&self) {
        let dir = settings::app_data_dir();
        if let Err(error) = std::fs::create_dir_all(&dir) {
            lag_log(format_args!("script.registry mkdir failed error={error}"));
            return;
        }
        let path = script_registry_path();
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(error) = std::fs::write(&path, json) {
                    lag_log(format_args!("script.registry write failed error={error}"));
                }
            }
            Err(error) => lag_log(format_args!("script.registry encode failed error={error}")),
        }
    }

    pub(crate) fn grouped_by_project(&self) -> HashMap<ProjectKey, Vec<&ScriptRecord>> {
        let mut grouped = HashMap::<ProjectKey, Vec<&ScriptRecord>>::new();
        for script in &self.scripts {
            let Some(project_key) = ProjectKey::from_project_path(&script.project_path) else {
                continue;
            };
            grouped.entry(project_key).or_default().push(script);
        }
        grouped
    }

    pub(super) fn runtime_probe_scripts(&self) -> Vec<ScriptRecord> {
        self.scripts
            .iter()
            .filter(|script| script.tab_id.is_some() && script.pane_id.is_some())
            .cloned()
            .collect()
    }

    pub(super) fn get_mut(&mut self, script_id: &str) -> Option<&mut ScriptRecord> {
        self.scripts
            .iter_mut()
            .find(|script| script.id == script_id)
    }
}

fn script_registry_path() -> std::path::PathBuf {
    settings::app_data_dir().join("scripts.json")
}

pub(super) fn next_script_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("script-{nanos:x}")
}

pub(super) fn script_icon_slug(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "asterisk" => "asterisk",
        "bot" => "bot",
        "star" => "star",
        _ => "terminal",
    }
    .to_string()
}

pub(crate) fn script_icon_name(value: &str) -> ComponentIconName {
    match script_icon_slug(value).as_str() {
        "asterisk" => ComponentIconName::Asterisk,
        "bot" => ComponentIconName::Bot,
        "star" => ComponentIconName::Star,
        _ => ComponentIconName::SquareTerminal,
    }
}

pub(super) fn normalize_script_keybinding(raw: &str) -> Result<Option<String>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let normalized = raw.to_ascii_lowercase().replace(' ', "").replace('-', "+");
    let mut control = false;
    let mut alt = false;
    let mut shift = false;
    let mut command = false;
    let mut key = None::<String>;
    for part in normalized.split('+').filter(|part| !part.is_empty()) {
        match part {
            "ctrl" | "control" => control = true,
            "alt" | "option" | "opt" => alt = true,
            "cmd" | "command" | "meta" => command = true,
            "shift" => shift = true,
            value if key.is_none() => key = Some(value.to_string()),
            _ => return Err("Keybinding must contain exactly one key".to_string()),
        }
    }
    let Some(key) = key else {
        return Err("Keybinding is missing a key".to_string());
    };
    let is_function_key = key
        .strip_prefix('f')
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number));
    if !control && !alt && !shift && !command && !is_function_key {
        return Err("Script keybindings must use a modifier or a function key".to_string());
    }
    let mut parts = Vec::new();
    if control {
        parts.push("ctrl".to_string());
    }
    if alt {
        parts.push("alt".to_string());
    }
    if shift {
        parts.push("shift".to_string());
    }
    if command {
        parts.push("cmd".to_string());
    }
    parts.push(key);
    let normalized = parts.join("+");
    const RESERVED: &[&str] = &[
        "f1",
        "cmd+,",
        "cmd+k",
        "cmd+r",
        "cmd+b",
        "shift+cmd+a",
        "cmd+v",
        "cmd+c",
        "cmd+a",
        "shift+cmd+r",
        "cmd+t",
        "cmd+w",
        "cmd+]",
        "shift+cmd+]",
        "alt+cmd+left",
        "alt+cmd+right",
        "alt+cmd+up",
        "alt+cmd+down",
        "alt+shift+cmd+left",
        "alt+shift+cmd+right",
        "alt+shift+cmd+up",
        "alt+shift+cmd+down",
        "shift+cmd+w",
        "cmd+left",
        "cmd+right",
        "shift+cmd+left",
        "shift+cmd+right",
    ];
    if RESERVED.contains(&normalized.as_str()) {
        return Err(format!("{normalized} is reserved by Shardlane"));
    }
    Ok(Some(normalized))
}

/// ScriptRecord construction helper shared across test modules (cfg(test), not
/// part of the product surface).
#[cfg(test)]
pub(super) fn script_record(id: &str) -> ScriptRecord {
    ScriptRecord {
        definition: ScriptDefinition {
            id: id.to_string(),
            ..ScriptDefinition::default()
        },
        ..ScriptRecord::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_kind_accepts_product_vocabulary() {
        assert_eq!(ScriptKind::parse("service"), Some(ScriptKind::Service));
        assert_eq!(ScriptKind::parse("command"), Some(ScriptKind::Command));
        assert_eq!(ScriptKind::parse("gdb"), Some(ScriptKind::Debugger));
        assert_eq!(ScriptKind::parse("other"), None);
    }

    #[test]
    fn script_multiline_commands_compile_to_one_fail_fast_shell_chain() {
        let script = ScriptDefinition {
            command: " pnpm install \n\n pnpm build\n cargo test ".into(),
            kind: ScriptKind::Command,
            one_shot: true,
            ..ScriptDefinition::default()
        };
        assert_eq!(
            script.assembled_command(),
            "pnpm install && pnpm build && cargo test"
        );
        assert_eq!(
            script.command_summary(),
            "pnpm install · pnpm build · cargo test"
        );
        assert!(script.is_one_shot());
    }

    #[test]
    fn script_keybinding_normalization_is_stable_and_rejects_reserved_shortcuts() {
        assert_eq!(
            normalize_script_keybinding("cmd-shift-j"),
            Ok(Some("shift+cmd+j".into()))
        );
        assert_eq!(
            normalize_script_keybinding(" option + ctrl + f8 "),
            Ok(Some("ctrl+alt+f8".into()))
        );
        assert!(normalize_script_keybinding("r").is_err());
        assert!(normalize_script_keybinding("cmd+r").is_err());
    }

    #[test]
    fn registry_groups_scripts_by_stable_project_path_not_runtime_correlation() {
        let registry = ScriptRegistry {
            scripts: vec![
                ScriptRecord {
                    definition: ScriptDefinition {
                        id: "a".into(),
                        project_path: "/work/a".into(),
                        name: "Vite".into(),
                        ..ScriptDefinition::default()
                    },
                    workspace_id: "w-stale".into(),
                    ..ScriptRecord::default()
                },
                ScriptRecord {
                    definition: ScriptDefinition {
                        id: "b".into(),
                        project_path: "/work/b".into(),
                        name: "API".into(),
                        ..ScriptDefinition::default()
                    },
                    workspace_id: "w-stale".into(),
                    ..ScriptRecord::default()
                },
            ],
        };
        let grouped = registry.grouped_by_project();
        let Some(key) = ProjectKey::from_project_path("/work/a") else {
            panic!("project key");
        };
        let scripts = grouped
            .get(&key)
            .unwrap_or_else(|| panic!("missing /work/a"));
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].id, "a");
    }

    #[test]
    fn script_display_label_surfaces_single_port() {
        let script = ScriptRecord {
            definition: ScriptDefinition {
                name: "Web Dev".into(),
                ..ScriptDefinition::default()
            },
            runtime: ScriptRuntimeProjection {
                ports: vec![5173],
                ..ScriptRuntimeProjection::default()
            },
            ..ScriptRecord::default()
        };
        assert_eq!(script.display_label(), "Web Dev · :5173");
    }

    #[test]
    fn script_registry_deserializes_legacy_tasks_root_key() {
        // Pre-2026-08-25 tasks.json used the `"tasks"` root key; the alias must
        // keep parsing old files.
        let legacy = r#"{"tasks":[{"id":"t1","project_path":"/p","name":"Dev",
            "command":"npm run dev","kind":"service","one_shot":false,
            "close_on_complete":false,"keybinding":null}]}"#;
        let registry = serde_json::from_str::<ScriptRegistry>(legacy).unwrap_or_default();
        assert_eq!(registry.scripts.len(), 1);
        assert_eq!(registry.scripts[0].definition.id, "t1");
        // The write side uses the new key.
        let Ok(saved) = serde_json::to_value(&registry) else {
            panic!("serialize failed")
        };
        assert!(saved.get("scripts").is_some());
        assert!(saved.get("tasks").is_none());
    }

    #[test]
    fn script_registry_persists_definition_and_correlation_but_not_runtime_projection() {
        let registry = ScriptRegistry {
            scripts: vec![ScriptRecord {
                definition: ScriptDefinition {
                    id: "script-1".into(),
                    project_path: "/tmp/project".into(),
                    name: "API".into(),
                    command: "cargo run".into(),
                    kind: ScriptKind::Service,
                    ..ScriptDefinition::default()
                },
                workspace_id: "workspace-1".into(),
                tab_id: Some("tab-1".into()),
                pane_id: Some("pane-1".into()),
                runtime: ScriptRuntimeProjection {
                    status: ScriptStatus::Running,
                    pid: Some(42),
                    ports: vec![3000],
                    started_at_ms: Some(123),
                    last_error: Some("transient".into()),
                },
            }],
        };
        let json = serde_json::to_string(&registry).unwrap_or_else(|error| panic!("{error}"));
        assert!(json.contains("\"project_path\""));
        assert!(json.contains("\"tab_id\""));
        for transient in ["status", "pid", "ports", "started_at_ms", "last_error"] {
            assert!(!json.contains(&format!("\"{transient}\"")));
        }

        let decoded: ScriptRegistry =
            serde_json::from_str(&json).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            decoded.scripts[0].definition,
            registry.scripts[0].definition
        );
        assert_eq!(decoded.scripts[0].workspace_id, "workspace-1");
        assert_eq!(decoded.scripts[0].tab_id.as_deref(), Some("tab-1"));
        assert_eq!(decoded.scripts[0].pane_id.as_deref(), Some("pane-1"));
        assert_eq!(
            decoded.scripts[0].runtime,
            ScriptRuntimeProjection::default()
        );
    }

    #[test]
    fn script_registry_get_mut_updates_script_definition_in_place() {
        let mut registry = ScriptRegistry {
            scripts: vec![ScriptRecord {
                definition: ScriptDefinition {
                    id: "script-edit-1".into(),
                    name: "Old Name".into(),
                    command: "cargo check".into(),
                    kind: ScriptKind::Command,
                    one_shot: true,
                    close_on_complete: true,
                    ..ScriptDefinition::default()
                },
                workspace_id: "w1".into(),
                tab_id: Some("t1".into()),
                pane_id: Some("p1".into()),
                ..ScriptRecord::default()
            }],
        };

        if let Some(record) = registry.get_mut("script-edit-1") {
            record.name = "New Name".into();
            record.command = "cargo test".into();
            record.kind = ScriptKind::Service;
            record.one_shot = false;
        }

        let updated = &registry.scripts[0];
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.command, "cargo test");
        assert_eq!(updated.kind, ScriptKind::Service);
        assert!(!updated.one_shot);
        assert_eq!(updated.workspace_id, "w1");
        assert_eq!(updated.tab_id.as_deref(), Some("t1"));
        assert_eq!(updated.pane_id.as_deref(), Some("p1"));
    }
}
