//! Host Context Transfer engine — one lossless continuation/handoff mechanism.
//!
//! [INPUT]: an exact source (a History `SessionFileRef` or a resolved live
//! source), the target Provider, the M2 `AgentLaunchIntent` (its prompt
//! field is overwritten by the engine with the briefing), an optional
//! continuation instruction, and a `TransferArtifactStore`.
//! [OUTPUT]: `run_context_transfer` / `run_context_transfer_with_ledger` —
//! capture a lossless snapshot → (small sessions inline / large sessions as
//! an artifact + accessibility verification) → build a deterministic
//! briefing → deliver exactly once through the M2 launch transaction
//! (claimable/replayable via the Host launch ledger) → return the launch
//! result plus snapshot metadata. M6 History cross-Provider and M7 Live
//! Handoff share this engine; never truncated, never a default model
//! summary.
//! [POS]: plan M5/M6/M7 / audit AF-02/AF-07/AF-08. This engine owns no
//! client focus.

use crate::agent_launch::{
    run_agent_launch, AgentLaunchIntent, AgentLaunchOutcome, AgentLaunchRuntime, ProjectPreparation,
};
use shardlane_history::models::SessionFileRef;
use shardlane_history::{
    artifact_accessible, build_transfer_briefing, capture_transfer_snapshot,
    transfer_payload_bytes, TransferArtifactStore, TransferError, TransferLimits, TransferPayload,
    TransferSnapshotMeta,
};
use std::fmt;

/// Where the lossless context comes from.
#[derive(Clone, Debug)]
pub enum ContextTransferSource {
    /// Indexed History session with its exact provider-native source.
    History(SessionFileRef),
    /// A live Conversation whose exact source was already resolved.
    Live(SessionFileRef),
}

#[derive(Clone, Debug)]
pub struct ContextTransferRequest {
    pub source: ContextTransferSource,
    pub target_provider: shardlane_history::AgentId,
    /// Launch inputs for the target (project/branch/mode/permission/attachments).
    /// `prompt` is overwritten with the deterministic briefing.
    pub launch: AgentLaunchIntent,
    /// Optional user continuation instruction. Empty is valid.
    pub instruction: Option<String>,
}

#[derive(Debug)]
pub enum ContextTransferFailure {
    Snapshot(TransferError),
    /// The artifact is not accessible to the target; the transfer pair is
    /// unavailable (fail closed — no partial-context fallback).
    ArtifactInaccessible,
    Launch(crate::agent_launch::AgentLaunchFailure),
}

impl fmt::Display for ContextTransferFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => write!(formatter, "{error}"),
            Self::ArtifactInaccessible => formatter
                .write_str("the full-context artifact is not accessible to the target agent"),
            Self::Launch(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ContextTransferFailure {}

#[derive(Clone, Debug)]
pub struct ContextTransferOutcome {
    pub launch: AgentLaunchOutcome,
    pub snapshot: TransferSnapshotMeta,
    /// SHA-256 of the delivered briefing text (audit/diagnostics).
    pub briefing_sha256: String,
}

/// Execute one context transfer: snapshot → briefing → canonical launch.
/// The briefing is delivered exactly once as the target's initial prompt by
/// the M2 transaction; this engine never resends.
pub fn run_context_transfer<R: AgentLaunchRuntime, P: ProjectPreparation>(
    runtime: &R,
    preparation: &P,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
    request: &ContextTransferRequest,
) -> Result<ContextTransferOutcome, ContextTransferFailure> {
    run_context_transfer_inner(runtime, preparation, store, limits, request, None)
}

/// Context transfer variant used by product operations. The launch shares the
/// Host operation ledger so a response-loss retry replays the same target and
/// never creates a second tab/Agent.
pub fn run_context_transfer_with_ledger<R: AgentLaunchRuntime, P: ProjectPreparation>(
    runtime: &R,
    preparation: &P,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
    request: &ContextTransferRequest,
    ledger: &crate::conversation_delivery::LaunchOperationLedger,
) -> Result<ContextTransferOutcome, ContextTransferFailure> {
    run_context_transfer_inner(runtime, preparation, store, limits, request, Some(ledger))
}

fn run_context_transfer_inner<R: AgentLaunchRuntime, P: ProjectPreparation>(
    runtime: &R,
    preparation: &P,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
    request: &ContextTransferRequest,
    ledger: Option<&crate::conversation_delivery::LaunchOperationLedger>,
) -> Result<ContextTransferOutcome, ContextTransferFailure> {
    let source = match &request.source {
        ContextTransferSource::History(source) | ContextTransferSource::Live(source) => source,
    };
    let snapshot = capture_transfer_snapshot(source, store, limits)
        .map_err(ContextTransferFailure::Snapshot)?;
    let artifact = match &snapshot.payload {
        TransferPayload::Inline(_) => None,
        TransferPayload::Artifact(reference) => {
            if !artifact_accessible(reference) {
                return Err(ContextTransferFailure::ArtifactInaccessible);
            }
            Some(reference.clone())
        }
    };
    let bytes =
        transfer_payload_bytes(&snapshot, store).map_err(ContextTransferFailure::Snapshot)?;
    let briefing = build_transfer_briefing(
        &snapshot,
        &bytes,
        request.instruction.as_deref(),
        artifact.as_ref(),
    );
    let briefing_sha256 = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(briefing.as_bytes());
        let digest = hasher.finalize();
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    };

    let mut launch_intent = request.launch.clone();
    launch_intent.agent = request.target_provider;
    launch_intent.prompt = briefing;
    let launch = match ledger {
        Some(ledger) => {
            let fingerprint = format!(
                "context-transfer|source-agent={}|source-native={}|source-path={}|target={}|instruction={}|launch={}",
                source.agent.as_str(),
                source.native_id,
                source.file_path,
                request.target_provider.as_str(),
                request.instruction.as_deref().unwrap_or_default().trim(),
                crate::agent_launch::agent_launch_fingerprint(&request.launch),
            );
            crate::agent_launch::run_agent_launch_with_ledger_fingerprint(
                runtime,
                preparation,
                &launch_intent,
                ledger,
                fingerprint,
            )
        }
        None => run_agent_launch(runtime, preparation, &launch_intent),
    }
    .map_err(ContextTransferFailure::Launch)?;

    Ok(ContextTransferOutcome {
        launch,
        snapshot: snapshot.meta,
        briefing_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_launch::{AgentLaunchMode, AgentLaunchRuntime, AgentPermission};
    use crate::herdr::{AgentStartedResult, TabCreatedResult, WorkspaceCreatedResult};
    use crate::ids::AgentRef;
    use shardlane_history::AgentId;
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct MockLaunch {
        prompt_texts: RefCell<Vec<String>>,
    }

    struct MockError(&'static str);
    impl fmt::Display for MockError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    fn ready_agent() -> crate::herdr::Agent {
        crate::herdr::Agent {
            pane_id: Some("pane-1".into()),
            interactive_ready: true,
            agent_session: Some(crate::herdr::AgentSessionInfo {
                agent: "codex".into(),
                kind: "id".into(),
                source: "herdr:codex".into(),
                value: "native-9".into(),
            }),
            revision: 1,
            ..Default::default()
        }
    }

    impl AgentLaunchRuntime for MockLaunch {
        type Error = MockError;
        fn create_tab_without_focus(
            &self,
            _workspace_id: Option<&str>,
            _cwd: &str,
        ) -> Result<TabCreatedResult, Self::Error> {
            Ok(TabCreatedResult {
                tab: crate::herdr::Tab {
                    tab_id: "tab-1".into(),
                    workspace_id: Some("ws-1".into()),
                    label: None,
                    title: None,
                    terminal_title: None,
                    agent_status: None,
                    pane_count: None,
                    focused: false,
                },
                root_pane: crate::herdr::Pane {
                    pane_id: "pane-1".into(),
                    ..Default::default()
                },
            })
        }
        fn create_workspace_without_focus(
            &self,
            _cwd: &str,
        ) -> Result<WorkspaceCreatedResult, Self::Error> {
            Err(MockError("unexpected workspace creation"))
        }
        fn workspace_id_for_path(&self, _path: &str) -> Result<Option<String>, Self::Error> {
            Ok(Some("ws-1".into()))
        }
        fn start_agent(
            &self,
            _params: &crate::herdr::AgentStartParams,
        ) -> Result<AgentStartedResult, Self::Error> {
            Ok(AgentStartedResult {
                agent: ready_agent(),
                _argv: Vec::new(),
            })
        }
        fn wait_agent_idle(&self, _target: &str, _timeout_ms: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn wait_shell_ready(&self, _pane_id: &str, _timeout_ms: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn wait_agent_settled(
            &self,
            _target: &str,
            _timeout_ms: u64,
        ) -> Result<crate::agent_launch::SettledStatus, Self::Error> {
            Ok(crate::agent_launch::SettledStatus::Idle)
        }
        fn agent_by_pane(
            &self,
            _pane_id: &str,
        ) -> Result<Option<crate::herdr::Agent>, Self::Error> {
            Ok(Some(ready_agent()))
        }
        fn prompt_agent_once(&self, _target: &str, text: &str) -> Result<(), Self::Error> {
            self.prompt_texts.borrow_mut().push(text.to_string());
            Ok(())
        }
        fn send_agent_keys(&self, _target: &str, _keys: &[String]) -> Result<(), Self::Error> {
            Ok(())
        }
        fn rename_tab(&self, _tab_id: &str, _label: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn rename_pane(&self, _pane_id: &str, _label: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn close_tab(&self, _tab_id: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn pane_layout(&self, _pane_id: &str) -> Result<crate::herdr::PaneLayout, Self::Error> {
            Ok(crate::herdr::PaneLayout::default())
        }
    }

    struct FixedPreparation(PathBuf);
    impl ProjectPreparation for FixedPreparation {
        fn prepare(
            &self,
            _path: &str,
            _branch: &str,
        ) -> Result<crate::agent_launch::PreparedProject, String> {
            Ok(crate::agent_launch::PreparedProject {
                cwd: self.0.clone(),
                worktree_created: false,
            })
        }
    }

    fn request(dir: &std::path::Path) -> (ContextTransferRequest, SessionFileRef) {
        let path = dir.join("history-session.jsonl");
        std::fs::write(&path, "FULL-CONTEXT\n").unwrap_or_else(|error| panic!("{error}"));
        let source = SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: "history-1".into(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: 0,
        };
        let request = ContextTransferRequest {
            source: ContextTransferSource::History(source.clone()),
            target_provider: AgentId::Codex,
            launch: AgentLaunchIntent {
                operation_id: "test-transfer".into(),
                workspace_id: None,
                project_path: "/work/demo".into(),
                branch: String::new(),
                mode: AgentLaunchMode::Build,
                permission: AgentPermission::AskApproval,
                agent: AgentId::Codex,
                prompt: String::new(),
                attachments: Vec::new(),
                skip_initial_prompt: false,
                extra_args: Vec::new(),
            },
            instruction: Some(" Continue with Codex ".into()),
        };
        (request, source)
    }

    #[test]
    fn transfer_delivers_one_briefing_with_full_context_and_instruction() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let (request, _source) = request(dir.path());
        let runtime = MockLaunch::default();
        let outcome = run_context_transfer(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(runtime.prompt_texts.borrow().len(), 1);
        let briefing = runtime.prompt_texts.borrow()[0].clone();
        assert!(briefing.contains("FULL-CONTEXT"));
        assert!(briefing.contains("Claude Code session history-1"));
        assert_eq!(briefing.matches("Continue with Codex").count(), 1);
        assert_eq!(outcome.launch.pane_id, "pane-1");
        assert_eq!(outcome.snapshot.provider, AgentId::ClaudeCode);
        assert_eq!(outcome.briefing_sha256.len(), 64);
    }

    #[test]
    fn missing_source_fails_closed_without_launch() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let (mut request, mut source) = request(dir.path());
        source.file_path = dir
            .path()
            .join("missing.jsonl")
            .to_string_lossy()
            .into_owned();
        request.source = ContextTransferSource::History(source);
        let runtime = MockLaunch::default();
        let result = run_context_transfer(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request,
        );
        assert!(matches!(result, Err(ContextTransferFailure::Snapshot(_))));
        assert!(runtime.prompt_texts.borrow().is_empty());
    }

    #[test]
    fn launch_failure_maps_to_the_transfer_phase() {
        use crate::agent_launch::AgentLaunchFailure;
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let (request, _) = request(dir.path());
        // Empty prompt/project validation fails inside the launch transaction
        // only when the briefing is empty — instead force a launch failure via
        // an unwritable project path through preparation.
        struct FailingPreparation;
        impl ProjectPreparation for FailingPreparation {
            fn prepare(
                &self,
                _path: &str,
                _branch: &str,
            ) -> Result<crate::agent_launch::PreparedProject, String> {
                Err("git worktree add failed".into())
            }
        }
        let runtime = MockLaunch::default();
        let result = run_context_transfer(
            &runtime,
            &FailingPreparation,
            &store,
            &TransferLimits::default(),
            &request,
        );
        assert!(matches!(
            result,
            Err(ContextTransferFailure::Launch(
                AgentLaunchFailure::BranchPreparationFailed(_)
            ))
        ));
        assert!(runtime.prompt_texts.borrow().is_empty());
    }

    #[test]
    fn empty_instruction_is_a_valid_transfer() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let (mut request, _) = request(dir.path());
        request.instruction = None;
        let runtime = MockLaunch::default();
        let outcome = run_context_transfer(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(runtime.prompt_texts.borrow().len(), 1);
        assert!(!runtime.prompt_texts.borrow()[0].contains("Continuation instruction:"));
        assert!(outcome.launch.identity.is_some());
    }

    #[test]
    fn live_sources_use_the_same_engine() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let (mut request, source) = request(dir.path());
        request.source = ContextTransferSource::Live(source);
        let runtime = MockLaunch::default();
        let outcome = run_context_transfer(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            outcome.launch.identity.map(|identity| identity.agent_ref),
            Some(AgentRef::new("pane-1"))
        );
    }
}
