//! Chat — the semantic sidecar presentation of the same Herdr Agent (Chat View).
//!
//! [INPUT]: Depends on chat::model (pure state machine), chat::approvals
//! (approval lifecycle + never-blind-send guard) and chat::surface (GUI domain).
//! [OUTPUT]: Provides ChatUi, WorkSurfaceMode, ApprovalState, and related
//! types to the crate.
//! [POS]: Root of the herdr-gui `chat` module. Claude/Codex/Pi still run only inside
//! the Herdr TUI; Chat never launches provider processes and does no TUI/ANSI
//! inference; ordinary prompts go only through the Herdr agent.prompt; blocked/
//! unsupported explicitly falls back to Terminal. Lifecycle: ChatUi is disposable,
//! live-source I/O lives only in background tasks, and the render path performs
//! zero file access.

pub(crate) mod model;
pub(crate) mod approvals;
pub(crate) mod surface;

pub(crate) use model::{plan_row_splice, RowSplice, WorkSurfaceMode};
pub(crate) use approvals::ApprovalState;
pub(crate) use surface::ChatUi;
