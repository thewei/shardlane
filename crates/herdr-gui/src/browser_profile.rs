//! Browser Profile domain: configuration and session identity for WebView isolation.
//!
//! [INPUT]: ApplicationConfig (browser section)
//! [OUTPUT]: BrowserProfile, BrowserProfileId, BrowserSessionId, BrowserConfig
//! [POS]: Domain module for browser session identity; consumed by right_panel/webview

/// Unique identifier for a persistent browser profile (UUID-based).
pub(crate) type BrowserProfileId = String;

/// Unique identifier for a browser session (profile + purpose).
/// Used as the key in the WebView pool instead of URL.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BrowserSessionId {
    pub profile_id: BrowserProfileId,
    pub purpose: String,
}

impl BrowserSessionId {
    pub(crate) fn new(profile_id: impl Into<String>, purpose: impl Into<String>) -> Self {
        Self {
            profile_id: profile_id.into(),
            purpose: purpose.into(),
        }
    }

    pub(crate) fn default_session(purpose: impl Into<String>) -> Self {
        Self {
            profile_id: "default".to_string(),
            purpose: purpose.into(),
        }
    }
}

/// Configuration for a single browser profile.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct BrowserProfileConfig {
    pub id: BrowserProfileId,
    pub name: String,
    /// Whether this profile uses a persistent or ephemeral data store.
    #[serde(default)]
    pub ephemeral: bool,
}

/// Which destructive browser-profile action is currently armed
/// (audit E03: two-click confirm, mirroring the workspace dialog's pattern).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserProfileConfirmAction {
    ClearData,
    Delete,
}

/// Top-level browser configuration (lives inside ApplicationConfig).
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct BrowserConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub default_profile_id: Option<BrowserProfileId>,
    #[serde(default)]
    pub profiles: Vec<BrowserProfileConfig>,
}

fn default_enabled() -> bool {
    true
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_profile_id: None,
            profiles: Vec::new(),
        }
    }
}

impl BrowserConfig {
    pub(crate) fn is_empty(&self) -> bool {
        self.enabled && self.default_profile_id.is_none() && self.profiles.is_empty()
    }

    /// Get the default profile, or create a synthesized one if none configured.
    pub(crate) fn default_profile(&self) -> BrowserProfileConfig {
        let target_id = self.default_profile_id.as_deref().unwrap_or("default");
        self.profiles
            .iter()
            .find(|p| p.id == target_id)
            .cloned()
            .unwrap_or_else(|| BrowserProfileConfig {
                id: "default".to_string(),
                name: "Default".to_string(),
                ephemeral: false,
            })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn browser_config_default_is_empty() {
        let config = BrowserConfig::default();
        assert!(config.is_empty());
    }

    #[test]
    fn browser_config_default_profile_synthesized() {
        let config = BrowserConfig::default();
        let profile = config.default_profile();
        assert_eq!(profile.id, "default");
        assert_eq!(profile.name, "Default");
        assert!(!profile.ephemeral);
    }

    #[test]
    fn browser_config_finds_configured_profile() {
        let config = BrowserConfig {
            enabled: true,
            default_profile_id: Some("custom".to_string()),
            profiles: vec![BrowserProfileConfig {
                id: "custom".to_string(),
                name: "Work".to_string(),
                ephemeral: false,
            }],
        };
        let profile = config.default_profile();
        assert_eq!(profile.id, "custom");
        assert_eq!(profile.name, "Work");
    }

    #[test]
    fn session_id_equality() {
        let a = BrowserSessionId::new("default", "tab-1");
        let b = BrowserSessionId::new("default", "tab-1");
        let c = BrowserSessionId::new("default", "tab-2");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn session_id_hashing() {
        use std::collections::HashMap;
        let mut map: HashMap<BrowserSessionId, String> = HashMap::new();
        map.insert(
            BrowserSessionId::new("default", "panel"),
            "webview-1".to_string(),
        );
        assert!(map.contains_key(&BrowserSessionId::new("default", "panel")));
        assert!(!map.contains_key(&BrowserSessionId::new("work", "panel")));
    }
}
