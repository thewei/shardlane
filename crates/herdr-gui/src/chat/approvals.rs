//! Chat approval pure model: request lifecycle state machine, grid-text
//! signature hashing, and the never-blind-send guard (zero GPUI, zero I/O).
//!
//! [INPUT]: Depends only on std (hashing) and the provider-neutral approval
//! facts shape mirrored from shardlane-history's LiveFacts.
//! [OUTPUT]: ApprovalKind / ApprovalState / ApprovalOption / ApprovalRequest,
//! grid_text_hash, guard_matches, and the transition helpers consumed by
//! chat::surface and chat::model.
//! [POS]: The approval state domain of herdr-gui chat. The iron rule here is
//! never-blind-send: a key sequence may only be sent when the current RAW TUI
//! grid text hash equals the request signature captured while that exact menu
//! was on screen; every mismatch/timeout path degrades explicitly to a
//! handle-it-in-Terminal presentation instead of firing bytes.
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

// ============================================================================
// 审批请求（provider 中立事实 + 本地生命周期状态）
// ============================================================================

/// Provider-neutral approval kind. Only kinds the live decoder can derive from
/// real provider events are ever constructed; nothing here is guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApprovalKind {
    /// Provider asks to execute/apply something (Codex exec/patch approvals).
    Tool,
    /// Reserved: permission-style approvals from other providers (never built
    /// for Codex; the decoder decides, not the GUI).
    Permission,
    /// Reserved: plan-review approvals (same rule as Permission).
    Plan,
}

impl ApprovalKind {
    /// Compact chip label (provider-neutral; the decoded prompt carries the
    /// provider's own words).
    pub(crate) fn label(self) -> &'static str {
        match self {
            ApprovalKind::Tool => "Tool",
            ApprovalKind::Permission => "Permission",
            ApprovalKind::Plan => "Plan",
        }
    }
}

/// Lifecycle of one locally observed approval request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ApprovalState {
    /// Menu seen in the live stream; waiting for the user's choice.
    #[default]
    Waiting,
    /// Guard did not match at click time: the menu has changed and not a
    /// single key was sent. Terminal-only from here.
    Stale,
    /// Keys sent; waiting for the frame confirmation that the menu is gone.
    Sent,
    /// Frame confirmed the menu disappeared.
    Done,
    /// Confirmation timed out: the outcome is unknown on this surface.
    Unknown,
}

/// One replayable choice. label is the provider's own action name, keys are
/// the exact bytes to write to the hosted PTY (decided by the provider
/// adapter's verified rules — never invented at this layer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApprovalOption {
    pub(crate) label: String,
    pub(crate) keys: Vec<u8>,
}

/// A decoded approval request bound to one exact grid state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApprovalRequest {
    /// Stable provider id (Codex call_id); a new id means a new request.
    pub(crate) id: String,
    pub(crate) kind: ApprovalKind,
    /// The provider's own text (command / patch summary / reason).
    pub(crate) prompt: String,
    pub(crate) options: Vec<ApprovalOption>,
    /// RAW TUI grid text hash captured while this exact menu was on screen.
    pub(crate) signature: u64,
    /// Whether the surface already captured the signature (a request rendered
    /// before the first frame arrives stays Waiting-uncaptured and can never
    /// pass the guard — fail-closed).
    pub(crate) signature_captured: bool,
    pub(crate) state: ApprovalState,
}

impl ApprovalRequest {
    /// Bind a decoded live request. The signature is captured by the surface
    /// as soon as a RAW frame is available; until then the guard fails closed.
    pub(crate) fn new_waiting(
        id: String,
        kind: ApprovalKind,
        prompt: String,
        options: Vec<ApprovalOption>,
    ) -> Self {
        Self {
            id,
            kind,
            prompt,
            options,
            signature: 0,
            signature_captured: false,
            state: ApprovalState::Waiting,
        }
    }

    /// Capture the guard signature (idempotent: the first capture wins; a
    /// recapture with a different grid must NOT re-arm a stale request).
    pub(crate) fn capture_signature(&mut self, grid_hash: u64) {
        if self.signature_captured {
            return;
        }
        self.signature = grid_hash;
        self.signature_captured = true;
    }

    /// Terminal-side resolution: the provider stream progressed past the
    /// request without this surface sending anything (the user answered in
    /// the TUI). The chip's job is over either way.
    pub(crate) fn resolve_externally(&mut self) {
        if matches!(self.state, ApprovalState::Waiting) {
            self.state = ApprovalState::Done;
        }
    }
}

// ============================================================================
// 守卫：RAW 网格文本哈希 + 永不盲发
// ============================================================================

/// FNV-1a 64 over the RAW TUI grid text (one entry per visible row). Any cell
/// change anywhere flips the hash — deliberately conservative: the failure
/// direction of a spurious mismatch is Stale (safe), never a blind send.
pub(crate) fn grid_text_hash(rows: &[String]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for row in rows {
        for byte in row.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        // Row separator byte keeps "ab" + "c" from colliding with "a" + "bc".
        hash ^= u64::from(b'\n');
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The guard: replay is allowed only against the exact grid state the request
/// was bound to. An uncaptured signature never matches (fail-closed).
pub(crate) fn guard_matches(current_grid_text_hash: u64, req: &ApprovalRequest) -> bool {
    req.signature_captured && req.signature == current_grid_text_hash
        && req.state == ApprovalState::Waiting
}

// ============================================================================
// 单测：全状态迁移 + 守卫
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApprovalRequest {
        ApprovalRequest::new_waiting(
            "call-1".to_string(),
            ApprovalKind::Tool,
            "cargo test".to_string(),
            vec![ApprovalOption {
                label: "Approve".to_string(),
                keys: vec![b'y'],
            }],
        )
    }

    #[test]
    fn waiting_uncaptured_guard_fails_closed() {
        let mut req = request();
        req.capture_signature(grid_text_hash(&["menu".to_string()]));
        assert!(req.signature_captured);
        // Same grid → guard passes.
        assert!(guard_matches(grid_text_hash(&["menu".to_string()]), &req));
        // Any other grid → rejected.
        assert!(!guard_matches(grid_text_hash(&["menu v2".to_string()]), &req));
    }

    #[test]
    fn uncaptured_signature_never_matches() {
        let req = request();
        assert!(!req.signature_captured);
        assert!(!guard_matches(0, &req));
    }

    #[test]
    fn stale_state_rejects_guard_even_with_matching_grid() {
        let mut req = request();
        req.capture_signature(42);
        req.state = ApprovalState::Stale;
        assert!(!guard_matches(42, &req));
    }

    #[test]
    fn full_transition_matrix() {
        // Waiting → Stale (guard mismatch path).
        let mut req = request();
        assert_eq!(req.state, ApprovalState::Waiting);
        req.state = ApprovalState::Stale;
        assert_eq!(req.state, ApprovalState::Stale);

        // Waiting → Sent → Done (happy path).
        let mut req = request();
        req.state = ApprovalState::Sent;
        req.state = ApprovalState::Done;
        assert_eq!(req.state, ApprovalState::Done);

        // Waiting → Sent → Unknown (confirmation timeout path).
        let mut req = request();
        req.state = ApprovalState::Sent;
        req.state = ApprovalState::Unknown;
        assert_eq!(req.state, ApprovalState::Unknown);

        // Waiting → Done (external resolution: answered in Terminal).
        let mut req = request();
        req.resolve_externally();
        assert_eq!(req.state, ApprovalState::Done);
    }

    #[test]
    fn resolve_externally_leaves_terminal_states_alone() {
        let mut req = request();
        req.state = ApprovalState::Sent;
        req.resolve_externally();
        assert_eq!(req.state, ApprovalState::Sent);
    }

    #[test]
    fn signature_capture_is_first_write_wins() {
        let mut req = request();
        req.capture_signature(1);
        req.capture_signature(2);
        assert_eq!(req.signature, 1);
    }

    #[test]
    fn grid_hash_separates_rows_and_changes_with_content() {
        let a = grid_text_hash(&["ab".to_string(), "c".to_string()]);
        let b = grid_text_hash(&["a".to_string(), "bc".to_string()]);
        assert_ne!(a, b);
        let empty = grid_text_hash(&[]);
        assert_ne!(a, empty);
    }

    #[test]
    fn kind_labels_are_stable() {
        assert_eq!(ApprovalKind::Tool.label(), "Tool");
        assert_eq!(ApprovalKind::Permission.label(), "Permission");
        assert_eq!(ApprovalKind::Plan.label(), "Plan");
    }
}
