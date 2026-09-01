//! [INPUT]: Read/write boundary over external Agent history files/SQLite and the
//!          Shardlane-owned catalog.
//! [OUTPUT]: Exposes normalized history models, the adapter roster, scanning/
//!           watching, the live decoder, locator, and Herdr continuation intent;
//!           provides no runtime.
//! [POS]: shardlane-history crate root, bridging provider semantic parsing and
//!        the read-only projection in herdr-gui.

pub mod adapters;
pub mod catalog;
pub mod insights;
pub mod live;
pub mod locator;
pub mod models;
pub mod resume;
pub mod scanner;
pub mod sources;
pub mod transfer;
pub mod watcher;

pub use adapters::{
    adapter_for, adapter_ix_for, create_adapters, create_adapters_with, normalize_custom_root,
    path_owns, AgentHistoryAdapter,
};
pub use catalog::{CachedTranscriptWindow, HistoryCatalog};
pub use insights::{
    compute_insights, AgentTally, DayActivity, InsightsSnapshot, ModelTally, ProjectTally,
};
pub use live::registry::{
    capabilities as provider_capabilities, exposed_agents, live_capable_agents, provider_exposed,
    resolve_agent_alias, LiveCapability, ProviderCapabilities, ProviderExposure, PROVIDERS,
};
pub use live::{LiveChange, LiveDecoder, LiveFacts, LiveSession, LiveSnapshot, LiveSync};
pub use locator::{resolve_session_source_locator, SessionSourceLocator};
pub use models::{
    AgentId, ConversationMeta, ConversationRef, IndexUnit, MessageKind, ParsedConversation,
    ParsedTranscript, Role, SearchHit, SessionSummary, ToolCall, TranscriptMessage,
};
pub use resume::{prepare_resume, resume_supported, ResumeCommand, ResumeIntent};
pub use scanner::{scan, ScanReport};
pub use sources::{
    CustomHistoryRoot, HistoryAdapterRoster, HistorySourceKey, HistorySourceKind,
    HistorySourceLocation, HistorySourcePolicy,
};
pub use transfer::{
    artifact_accessible, build_transfer_briefing, capture_transfer_snapshot,
    transfer_payload_bytes, TransferArtifactRef, TransferArtifactStore, TransferError,
    TransferLimits, TransferPayload, TransferSnapshot, TransferSnapshotMeta, TransferSourceKind,
    DEFAULT_ARTIFACT_MAX_COUNT, DEFAULT_ARTIFACT_MAX_TOTAL_BYTES, DEFAULT_ARTIFACT_TTL_MS,
    DEFAULT_INLINE_LIMIT_BYTES,
};
pub use watcher::HistoryWatcher;
