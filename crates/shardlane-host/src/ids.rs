//! Opaque identifiers exposed by the Shardlane Host boundary.
//!
//! Product clients treat these values as opaque strings. Only the Host resolver may
//! interpret a `ProjectId`; a filesystem path is never the public identifier itself.
//!

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

opaque_id!(WorkspaceId);
opaque_id!(ProjectId);
opaque_id!(TabId);
opaque_id!(PaneId);
opaque_id!(AgentRef);
opaque_id!(ScriptId);
opaque_id!(ConversationId);
opaque_id!(InteractionId);
opaque_id!(BridgeRequestId);

const PROJECT_ID_PREFIX: &str = "prj_1_";

/// Internal lookup material encoded into an opaque `ProjectId`.
///
/// This is deliberately not serializable as a remote DTO. It is a Host-side bridge to
/// the existing ProjectKey/runtime Workspace correlation and does not create a second
/// persistent Project database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectLocator {
    RuntimeWorkspace(String),
    ProjectPath(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectIdError {
    UnsupportedVersion,
    InvalidEncoding,
    InvalidLocator,
}

impl fmt::Display for ProjectIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion => formatter.write_str("unsupported ProjectId version"),
            Self::InvalidEncoding => formatter.write_str("invalid ProjectId encoding"),
            Self::InvalidLocator => formatter.write_str("invalid ProjectId locator"),
        }
    }
}

impl std::error::Error for ProjectIdError {}

pub fn project_id_for_runtime_workspace(workspace_id: &str) -> ProjectId {
    encode_project_locator("runtime", workspace_id)
}

pub fn project_id_for_path(project_path: &str) -> ProjectId {
    encode_project_locator("path", project_path)
}

/// The one canonical "no project could be resolved" ProjectId (C10). Two
/// ad-hoc sentinels used to encode different fake ids for the same concept
/// (a fabricated empty-runtime id and this orphan path id); consumers treat
/// ProjectIds as opaque, so one shared value keeps "unresolved" consistent.
pub fn unresolved_project_id() -> ProjectId {
    project_id_for_path("shardlane://orphan")
}

pub fn resolve_project_id(project_id: &ProjectId) -> Result<ProjectLocator, ProjectIdError> {
    let encoded = project_id
        .as_str()
        .strip_prefix(PROJECT_ID_PREFIX)
        .ok_or(ProjectIdError::UnsupportedVersion)?;
    let payload = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| ProjectIdError::InvalidEncoding)?;
    let payload = String::from_utf8(payload).map_err(|_| ProjectIdError::InvalidEncoding)?;
    let (kind, value) = payload
        .split_once(':')
        .ok_or(ProjectIdError::InvalidLocator)?;
    if value.is_empty() {
        return Err(ProjectIdError::InvalidLocator);
    }
    match kind {
        "runtime" => Ok(ProjectLocator::RuntimeWorkspace(value.to_string())),
        "path" => Ok(ProjectLocator::ProjectPath(value.to_string())),
        _ => Err(ProjectIdError::InvalidLocator),
    }
}

fn encode_project_locator(kind: &str, value: &str) -> ProjectId {
    let encoded = URL_SAFE_NO_PAD.encode(format!("{kind}:{value}"));
    ProjectId::new(format!("{PROJECT_ID_PREFIX}{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_project_id_is_opaque_and_round_trips() {
        let path = "/Users/example/work/demo";
        let id = project_id_for_path(path);
        assert_ne!(id.as_str(), path);
        assert!(!id.as_str().contains(path));
        assert_eq!(
            resolve_project_id(&id),
            Ok(ProjectLocator::ProjectPath(path.to_string()))
        );
    }

    #[test]
    fn runtime_project_id_round_trips_without_global_focus() {
        let id = project_id_for_runtime_workspace("workspace-42");
        assert_eq!(
            resolve_project_id(&id),
            Ok(ProjectLocator::RuntimeWorkspace("workspace-42".to_string()))
        );
    }

    #[test]
    fn project_id_rejects_unknown_versions_and_invalid_payloads() {
        let unknown = ProjectId::new("prj_2_Zm9v");
        assert_eq!(
            resolve_project_id(&unknown),
            Err(ProjectIdError::UnsupportedVersion)
        );

        let invalid = ProjectId::new("prj_1_not-valid-utf8-____");
        assert!(resolve_project_id(&invalid).is_err());
    }
}
