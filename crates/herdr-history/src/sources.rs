// SPDX-License-Identifier: MIT
// Source-policy value types are Shardlane-owned and contain no runtime state.

//! [INPUT]: Depends on models::AgentId and the adapter data-root/custom-root
//! contract.
//! [OUTPUT]: Exposes the history source policy, location snapshots, and
//! HistoryAdapterRoster so GUI configuration, scanner, watcher, detail, and
//! export share one roster generation.
//! [POS]: Source SSOT of shardlane-history; expresses only user choices and
//! adapter instance orchestration — never writes the History SQLite and never
//! owns external provider files.

use crate::adapters::{self, AgentHistoryAdapter};
use crate::models::{normalize_path_key, AgentId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Kind of a user-visible source. Default paths are always derived by the
/// adapter; config stores only the disable intent, avoiding a second copy of
/// absolute-path truth.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistorySourceKind {
    Default,
    Custom,
}

/// Stable key for one source. The path is the normalized data root.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct HistorySourceKey {
    pub agent: AgentId,
    pub path: PathBuf,
}

impl HistorySourceKey {
    pub fn new(agent: AgentId, path: PathBuf) -> Self {
        Self { agent, path }
    }
}

/// Source location snapshot consumed by Settings and background scans.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistorySourceLocation {
    pub key: HistorySourceKey,
    pub kind: HistorySourceKind,
    pub enabled: bool,
    /// Whether a multi-root adapter (currently Codex/OpenCode) may disable
    /// just this root.
    pub individually_removable: bool,
}

impl HistorySourceLocation {
    pub fn agent(&self) -> AgentId {
        self.key.agent
    }

    pub fn path(&self) -> &PathBuf {
        &self.key.path
    }
}

/// Platform-independent projection of sources in ApplicationConfig. The actual
/// JSON is owned by the GUI layer; this type contains no SQLite-derived data,
/// so deleting the database never loses user configuration.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySourcePolicy {
    pub custom_roots: Vec<CustomHistoryRoot>,
    pub disabled_defaults: HashSet<HistorySourceKey>,
    pub disabled_customs: HashSet<HistorySourceKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CustomHistoryRoot {
    pub agent: AgentId,
    pub path: PathBuf,
}

impl HistorySourcePolicy {
    pub fn custom_root(agent: AgentId, path: PathBuf) -> CustomHistoryRoot {
        CustomHistoryRoot { agent, path }
    }

    pub fn is_disabled(&self, kind: HistorySourceKind, key: &HistorySourceKey) -> bool {
        match kind {
            HistorySourceKind::Default => self.disabled_defaults.contains(key),
            HistorySourceKind::Custom => self.disabled_customs.contains(key),
        }
    }

    /// When removing a source, also clear stale disabled keys for the same
    /// path so a rename/edit leaves no invisible ghost configuration.
    pub fn remove_custom_root(&mut self, agent: AgentId, path: &PathBuf) {
        self.custom_roots
            .retain(|root| !(root.agent == agent && root.path == *path));
        self.disabled_customs
            .retain(|key| !(key.agent == agent && paths_related(&key.path, path)));
    }

    /// Replace a custom root without losing its enabled/disabled intent. This is
    /// configuration-only; no provider files or catalog rows are touched.
    pub fn replace_custom_root(&mut self, agent: AgentId, old: &PathBuf, new: PathBuf) {
        let was_disabled = self
            .disabled_customs
            .iter()
            .any(|key| key.agent == agent && paths_related(&key.path, old));
        self.remove_custom_root(agent, old);
        let new_key = HistorySourceKey::new(agent, new.clone());
        self.custom_roots
            .push(CustomHistoryRoot { agent, path: new });
        if was_disabled {
            self.set_enabled(HistorySourceKind::Custom, new_key, false);
        }
    }

    /// Remove every user override for one provider and re-enable all of its
    /// built-in roots. The catalog remains untouched and is reconciled by the
    /// caller after the next roster generation is installed.
    pub fn restore_defaults(&mut self, agent: AgentId) {
        self.custom_roots.retain(|root| root.agent != agent);
        self.disabled_defaults.retain(|key| key.agent != agent);
        self.disabled_customs.retain(|key| key.agent != agent);
    }

    /// Find the configured custom root that owns a projected adapter root. A
    /// Codex/OpenCode custom adapter can expose multiple data roots, so exact
    /// equality is not sufficient here.
    pub fn custom_root_for_location(&self, agent: AgentId, location: &PathBuf) -> Option<PathBuf> {
        self.custom_roots
            .iter()
            .find(|root| root.agent == agent && paths_related(&root.path, location))
            .map(|root| root.path.clone())
    }

    /// Validate a user-selected custom source against the current provider
    /// roster. UI callers can report the returned text without duplicating path
    /// boundary/overlap rules in the presentation layer.
    pub fn validate_custom_root(
        &self,
        agent: AgentId,
        candidate: &std::path::Path,
        locations: &[HistorySourceLocation],
    ) -> Result<(), String> {
        if candidate.as_os_str().is_empty() {
            return Err("Choose a non-empty folder".to_string());
        }
        if !candidate.is_absolute() {
            return Err("History source must be an absolute path".to_string());
        }
        let candidate_path = candidate.to_path_buf();
        for location in locations
            .iter()
            .filter(|location| location.agent() == agent)
        {
            let existing = location.path().to_string_lossy();
            if paths_related(&candidate_path, location.path()) {
                return Err(format!(
                    "History source overlaps the existing {} root {}",
                    match location.kind {
                        HistorySourceKind::Default => "default",
                        HistorySourceKind::Custom => "custom",
                    },
                    existing
                ));
            }
        }
        Ok(())
    }

    pub fn set_enabled(&mut self, kind: HistorySourceKind, key: HistorySourceKey, enabled: bool) {
        let disabled = match kind {
            HistorySourceKind::Default => &mut self.disabled_defaults,
            HistorySourceKind::Custom => &mut self.disabled_customs,
        };
        if enabled {
            disabled.remove(&key);
        } else {
            disabled.insert(key);
        }
    }

    /// Normalize a policy into its canonical stored form. Paths deliberately
    /// keep the user's spelling (including symlink and Finder aliases): source
    /// paths emitted by adapters use the same configured root, and resolving
    /// only the policy side through `canonicalize` would make ownership checks
    /// disagree for `/var` ↔ `/private/var` on macOS. Component-level cleanup
    /// goes through the shared [`normalize_path_key`] — the same single rule
    /// the catalog `project_key` derives from.
    pub fn normalized(mut self) -> Self {
        let mut seen_custom = HashSet::new();
        self.custom_roots.retain_mut(|root| {
            root.path = normalize_path_key(&root.path);
            if root.path.as_os_str().is_empty() {
                return false;
            }
            seen_custom.insert((root.agent, root.path.clone()))
        });
        self.disabled_defaults = self
            .disabled_defaults
            .into_iter()
            .map(|key| HistorySourceKey::new(key.agent, normalize_path_key(&key.path)))
            .filter(|key| !key.path.as_os_str().is_empty())
            .collect();
        self.disabled_customs = self
            .disabled_customs
            .into_iter()
            .map(|key| HistorySourceKey::new(key.agent, normalize_path_key(&key.path)))
            .filter(|key| !key.path.as_os_str().is_empty())
            .filter(|key| {
                seen_custom
                    .iter()
                    .any(|(agent, path)| *agent == key.agent && paths_related(path, &key.path))
            })
            .collect();
        self
    }
}

/// One snapshot of source configuration, environment, and filesystem. `active`
/// is the only adapter set consumable by scanner/watcher/detail/export;
/// `locations` keeps disabled rows for Settings display.
pub struct HistoryAdapterRoster {
    pub active: Vec<Box<dyn AgentHistoryAdapter>>,
    pub locations: Vec<HistorySourceLocation>,
}

impl Default for HistoryAdapterRoster {
    fn default() -> Self {
        Self::new(&HistorySourcePolicy::default())
    }
}

impl HistoryAdapterRoster {
    pub fn new(policy: &HistorySourcePolicy) -> Self {
        let policy = policy.clone().normalized();
        let base = adapters::create_adapters();
        let mut locations = Vec::new();

        // Build customs by borrowing the same set of default templates first,
        // then move those default adapters into active.
        let custom_adapters = policy
            .custom_roots
            .iter()
            .filter_map(|custom| {
                let template = base
                    .iter()
                    .find(|adapter| adapter.agent() == custom.agent)?;
                let path = adapters::normalize_custom_root(custom.agent, custom.path.clone());
                Some((custom.agent, template.with_custom_root(path)))
            })
            .collect::<Vec<_>>();

        let mut active = Vec::new();
        for adapter in base {
            let agent = adapter.agent();
            let roots = adapter.data_roots();
            let disabled = roots
                .iter()
                .filter(|path| {
                    policy.is_disabled(
                        HistorySourceKind::Default,
                        &HistorySourceKey::new(agent, (*path).clone()),
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            let individually_removable = adapter.supports_individual_root_removal();
            locations.extend(roots.iter().cloned().map(|path| {
                let key = HistorySourceKey::new(agent, path);
                HistorySourceLocation {
                    enabled: !policy.is_disabled(HistorySourceKind::Default, &key),
                    key,
                    kind: HistorySourceKind::Default,
                    individually_removable,
                }
            }));

            if disabled.is_empty() {
                active.push(adapter);
            } else if disabled.len() < roots.len() {
                if let Some(filtered) = adapter.excluding_data_roots(&disabled) {
                    if !filtered.data_roots().is_empty() {
                        active.push(filtered);
                    }
                }
            }
        }

        for (agent, custom_adapter) in custom_adapters {
            let roots = custom_adapter.data_roots();
            let mut disabled = Vec::new();
            for root in roots.iter().cloned() {
                let key = HistorySourceKey::new(agent, root);
                let root_enabled = !policy.is_disabled(HistorySourceKind::Custom, &key);
                if !root_enabled {
                    disabled.push(key.path.clone());
                }
                locations.push(HistorySourceLocation {
                    key,
                    kind: HistorySourceKind::Custom,
                    enabled: root_enabled,
                    individually_removable: custom_adapter.supports_individual_root_removal(),
                });
            }
            if disabled.is_empty() {
                active.push(custom_adapter);
            } else if disabled.len() < roots.len() {
                if let Some(filtered) = custom_adapter.excluding_data_roots(&disabled) {
                    if !filtered.data_roots().is_empty() {
                        active.push(filtered);
                    }
                }
            }
        }

        Self { active, locations }
    }

    pub fn adapter_for(&self, agent: AgentId, file_path: &str) -> Option<&dyn AgentHistoryAdapter> {
        adapters::adapter_for(&self.active, agent, file_path)
    }

    /// Resolve only when the path is actually owned by an enabled root. This
    /// strict variant is used for transcript parsing and live-source binding;
    /// unlike the legacy `adapter_for` fallback it cannot send a disabled or
    /// unknown path to the first adapter of the same provider.
    pub fn adapter_for_source(
        &self,
        agent: AgentId,
        file_path: &str,
    ) -> Option<&dyn AgentHistoryAdapter> {
        let mut best: Option<(usize, &dyn AgentHistoryAdapter)> = None;
        for adapter in self
            .active
            .iter()
            .filter(|adapter| adapter.agent() == agent)
        {
            for root in adapter.data_roots() {
                let root = root.to_string_lossy();
                if adapters::path_owns(&root, file_path)
                    && best.is_none_or(|(length, _)| root.len() > length)
                {
                    best = Some((root.len(), adapter.as_ref()));
                }
            }
        }
        best.map(|(_, adapter)| adapter)
    }

    /// Whether an exact source path is inside one of the currently enabled
    /// roots. `adapter_for` intentionally falls back to the first adapter for
    /// legacy single-root callers; live binding must not use that fallback for
    /// a disabled/custom location.
    pub fn owns_active_path(&self, agent: AgentId, file_path: &str) -> bool {
        self.active
            .iter()
            .filter(|adapter| adapter.agent() == agent)
            .flat_map(|adapter| adapter.data_roots())
            .any(|root| adapters::path_owns(&root.to_string_lossy(), file_path))
    }
}

impl std::fmt::Debug for HistoryAdapterRoster {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HistoryAdapterRoster")
            .field("active_count", &self.active.len())
            .field("locations", &self.locations)
            .finish()
    }
}

fn paths_related(left: &PathBuf, right: &PathBuf) -> bool {
    if left == right {
        return true;
    }
    let left = left.to_string_lossy();
    let right = right.to_string_lossy();
    adapters::path_owns(&left, &right) || adapters::path_owns(&right, &left)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_enable_disable_and_remove_clear_stale_keys() {
        let mut policy = HistorySourcePolicy::default();
        let key = HistorySourceKey::new(AgentId::ClaudeCode, PathBuf::from("/tmp/claude"));
        policy.set_enabled(HistorySourceKind::Custom, key.clone(), false);
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::ClaudeCode,
            path: key.path.clone(),
        });
        policy.set_enabled(HistorySourceKind::Custom, key.clone(), true);
        assert!(!policy.is_disabled(HistorySourceKind::Custom, &key));
        policy.set_enabled(HistorySourceKind::Custom, key.clone(), false);
        policy.remove_custom_root(AgentId::ClaudeCode, &key.path);
        assert!(policy.custom_roots.is_empty());
        assert!(policy.disabled_customs.is_empty());
    }

    #[test]
    fn roster_keeps_missing_default_locations_visible() {
        let roster = HistoryAdapterRoster::new(&HistorySourcePolicy::default());
        assert_eq!(roster.active.len(), AgentId::ALL.len());
        assert!(roster
            .locations
            .iter()
            .any(|location| location.agent() == AgentId::ClaudeCode));
    }

    #[test]
    fn roster_path_ownership_excludes_disabled_custom_source() {
        let mut policy = HistorySourcePolicy::default();
        let custom = PathBuf::from("/tmp/shardlane-source-test");
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::ClaudeCode,
            path: custom.clone(),
        });
        let roster = HistoryAdapterRoster::new(&policy);
        assert!(roster
            .adapter_for_source(
                AgentId::ClaudeCode,
                "/tmp/shardlane-source-test/project/session.jsonl"
            )
            .is_some());
        assert!(roster.owns_active_path(
            AgentId::ClaudeCode,
            "/tmp/shardlane-source-test/project/session.jsonl"
        ));

        policy.set_enabled(
            HistorySourceKind::Custom,
            HistorySourceKey::new(AgentId::ClaudeCode, custom.clone()),
            false,
        );
        let disabled = HistoryAdapterRoster::new(&policy);
        assert!(disabled
            .adapter_for_source(
                AgentId::ClaudeCode,
                "/tmp/shardlane-source-test/project/session.jsonl"
            )
            .is_none());
        assert!(!disabled.owns_active_path(
            AgentId::ClaudeCode,
            "/tmp/shardlane-source-test/project/session.jsonl"
        ));
    }

    #[test]
    fn roster_can_disable_one_root_of_a_multi_root_custom_adapter() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root = temp.path().join("codex");
        std::fs::create_dir_all(root.join("sessions")).unwrap_or_else(|error| panic!("{error}"));
        std::fs::create_dir_all(root.join("archived_sessions"))
            .unwrap_or_else(|error| panic!("{error}"));

        let mut policy = HistorySourcePolicy::default();
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::Codex,
            path: root.clone(),
        });
        let initial = HistoryAdapterRoster::new(&policy);
        let sessions = initial
            .locations
            .iter()
            .find(|location| {
                location.kind == HistorySourceKind::Custom
                    && location.agent() == AgentId::Codex
                    && location.path().ends_with("sessions")
            })
            .map(|location| location.key.clone())
            .unwrap_or_else(|| panic!("missing custom Codex sessions location"));
        policy.set_enabled(HistorySourceKind::Custom, sessions, false);

        let partial = HistoryAdapterRoster::new(&policy);
        assert!(!partial.owns_active_path(
            AgentId::Codex,
            &root.join("sessions/2026/rollout.jsonl").to_string_lossy()
        ));
        assert!(partial.owns_active_path(
            AgentId::Codex,
            &root
                .join("archived_sessions/2026/rollout.jsonl")
                .to_string_lossy()
        ));
    }

    #[test]
    fn custom_root_validation_rejects_relative_and_overlapping_paths() {
        let mut policy = HistorySourcePolicy::default();
        let custom = PathBuf::from("/tmp/shardlane-custom");
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::ClaudeCode,
            path: custom.clone(),
        });
        let roster = HistoryAdapterRoster::new(&policy);
        assert!(policy
            .validate_custom_root(
                AgentId::ClaudeCode,
                PathBuf::from("relative").as_path(),
                &roster.locations
            )
            .is_err());
        assert!(policy
            .validate_custom_root(
                AgentId::ClaudeCode,
                PathBuf::from("/tmp/shardlane-custom/archive").as_path(),
                &roster.locations,
            )
            .is_err());
        assert!(policy
            .validate_custom_root(
                AgentId::ClaudeCode,
                PathBuf::from("/tmp/other-provider-root").as_path(),
                &roster.locations,
            )
            .is_ok());
    }

    #[test]
    fn restore_defaults_removes_only_one_provider_overrides() {
        let mut policy = HistorySourcePolicy::default();
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::ClaudeCode,
            path: PathBuf::from("/tmp/claude-custom"),
        });
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::Codex,
            path: PathBuf::from("/tmp/codex-custom"),
        });
        policy.set_enabled(
            HistorySourceKind::Default,
            HistorySourceKey::new(AgentId::ClaudeCode, PathBuf::from("/tmp/claude-default")),
            false,
        );
        policy.restore_defaults(AgentId::ClaudeCode);
        assert!(policy
            .custom_roots
            .iter()
            .all(|root| root.agent != AgentId::ClaudeCode));
        assert!(policy
            .disabled_defaults
            .iter()
            .all(|key| key.agent != AgentId::ClaudeCode));
        assert!(policy
            .custom_roots
            .iter()
            .any(|root| root.agent == AgentId::Codex));
    }

    #[test]
    fn replace_custom_root_preserves_disabled_intent() {
        let mut policy = HistorySourcePolicy::default();
        let old = PathBuf::from("/tmp/old-source");
        let new = PathBuf::from("/tmp/new-source");
        policy.custom_roots.push(CustomHistoryRoot {
            agent: AgentId::ClaudeCode,
            path: old.clone(),
        });
        policy.set_enabled(
            HistorySourceKind::Custom,
            HistorySourceKey::new(AgentId::ClaudeCode, old.clone()),
            false,
        );
        policy.replace_custom_root(AgentId::ClaudeCode, &old, new.clone());
        assert!(policy
            .custom_root_for_location(AgentId::ClaudeCode, &new)
            .is_some());
        assert!(policy.is_disabled(
            HistorySourceKind::Custom,
            &HistorySourceKey::new(AgentId::ClaudeCode, new),
        ));
    }
}
