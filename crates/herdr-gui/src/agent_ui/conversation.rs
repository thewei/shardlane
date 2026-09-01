//! Shared conversation timeline: provider-neutral row projection and Worked
//! folding (shared by History and Chat).
//!
//! [INPUT]: depends on shardlane-history's TranscriptMessage/Role/MessageKind;
//! zero GPUI, zero app state, zero I/O — pure functions.
//! [OUTPUT]: derive_turns / folded_conversation_rows / rows_fingerprint and the
//! ConversationRow/ConversationTurn models.
//! [POS]: conversation projection primitives for herdr-gui `agent_ui`. The row
//! folding / answer demarcation / footer placement algorithms are adapted to
//! TranscriptMessage input: thinking → Reasoning rows, tool_calls → ToolActivity
//! rows, non-empty text → Answer rows; a turn boundary is a Text-kind User
//! message (plan §11.1). History provides a static projection; Chat provides a
//! live projection with busy state; the two do not share lifecycles.

use shardlane_history::{MessageKind, Role, TranscriptMessage};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// One presentation turn: bounded by a Text-kind User message (plan §11.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationTurn {
    /// Index of the turn's first message (usually the user prompt row).
    pub start: usize,
    /// Half-open range `[start, end)` the turn covers; end is the next turn's
    /// start.
    pub range: Range<usize>,
}

/// Timeline rows; usize indices all point into the input `messages` slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConversationRow {
    /// User prompt (turn opening; folding never touches it).
    UserPrompt(usize),
    /// Context compaction / system boundary row (from CompactSummary / system
    /// meta messages).
    ContextBoundary(usize),
    /// Reasoning/thinking work block (from an assistant message's thinking).
    Reasoning(usize),
    /// Tool activity row (the `tool`th tool_call of an assistant message).
    ToolActivity { message: usize, tool: usize },
    /// Assistant answer text (non-empty text).
    Answer(usize),
    /// A settled turn's "Worked …" fold row, value is the turn index; anchored
    /// at the top of the turn (where work starts), expansion restores the
    /// original order of the turn's hidden rows.
    TurnFold(usize),
    /// Turn footer (copy/time), inserted after the turn's last visible row —
    /// belongs to the turn as a whole, not to the last text message to arrive.
    ResponseFooter(usize),
    /// Working indicator row. Appears only when the caller explicitly provides
    /// a busy state: the History static projection passes false and never
    /// produces it; Chat produces it when Herdr is working and an active turn
    /// exists.
    WorkingIndicator,
}

/// Derive presentation turns: a Text-kind User message opens a new turn;
/// Meta/CompactSummary and system rows join the current turn without opening
/// one. Orphan messages before the first user prompt form turn 0.
pub fn derive_turns(messages: &[TranscriptMessage]) -> Vec<ConversationTurn> {
    let mut boundaries: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::User && message.kind == MessageKind::Text)
        .map(|(index, _)| index)
        .collect();
    if boundaries.first() != Some(&0) {
        boundaries.insert(0, 0);
    }
    boundaries
        .iter()
        .enumerate()
        .map(|(turn, &start)| {
            let end = boundaries.get(turn + 1).copied().unwrap_or(messages.len());
            ConversationTurn {
                start,
                range: start..end,
            }
        })
        .filter(|turn| !turn.range.is_empty())
        .collect()
}

/// Rows one turn produces over its message range (for the version without
/// UserPrompt see `turn_work_rows`). A single assistant message expands in the
/// fixed order Reasoning → ToolActivity → Answer (the persisted format loses
/// the inter-block arrival order; this order matches the folding semantics).
fn turn_message_rows(
    messages: &[TranscriptMessage],
    range: Range<usize>,
    include_prompt: bool,
) -> Vec<ConversationRow> {
    let mut rows = Vec::new();
    for index in range {
        let message = &messages[index];
        match message.role {
            Role::User => {
                if include_prompt {
                    rows.push(ConversationRow::UserPrompt(index));
                }
            }
            Role::Assistant => {
                if message
                    .thinking
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
                {
                    rows.push(ConversationRow::Reasoning(index));
                }
                for tool in 0..message.tool_calls.len() {
                    rows.push(ConversationRow::ToolActivity {
                        message: index,
                        tool,
                    });
                }
                if !message.text.trim().is_empty() {
                    rows.push(ConversationRow::Answer(index));
                }
            }
            Role::System => {
                if message.kind == MessageKind::CompactSummary || !message.text.trim().is_empty() {
                    rows.push(ConversationRow::ContextBoundary(index));
                }
            }
        }
    }
    rows
}

/// Rows a turn produces (without the user prompt row: the prompt is the title
/// the fold lives under and does not participate in folding).
fn turn_work_rows(messages: &[TranscriptMessage], turn: &ConversationTurn) -> Vec<ConversationRow> {
    turn_message_rows(messages, turn.range.clone(), false)
}

/// Start row index of the answer within a turn (relative to `turn_work_rows`):
/// the first row of the trailing run of consecutive Answer rows. Everything
/// before it (reasoning/tools/intermediate narration) counts as work and
/// participates in folding; a turn with no text at all has no answer and folds
/// entirely.
fn turn_answer_position(turn_rows: &[ConversationRow]) -> usize {
    let is_answer = |row: &ConversationRow| matches!(row, ConversationRow::Answer(_));
    let Some(last_answer) = turn_rows.iter().rposition(is_answer) else {
        return turn_rows.len();
    };
    turn_rows[..last_answer]
        .iter()
        .rposition(|row| !is_answer(row))
        .map_or(0, |index| index + 1)
}

/// Work step count shown on a turn's fold row (number of work rows hidden by
/// the fold; 0 means no foldable work).
pub fn turn_work_step_count(
    messages: &[TranscriptMessage],
    turns: &[ConversationTurn],
    turn_index: usize,
) -> usize {
    let Some(turn) = turns.get(turn_index) else {
        return 0;
    };
    let work_rows = turn_work_rows(messages, turn);
    turn_answer_position(&work_rows).min(work_rows.len())
}

/// Folded visible row sequence (folded_transcript_row_kinds semantics):
///
/// - Settled turns: all work before the answer folds into a single `TurnFold`,
///   anchored at the top of the turn (where work starts); expansion restores
///   the original order, the answer stays visible.
/// - Turns in `running_turns` stay expanded (live Chat's active turns).
/// - Turns with copyable answer text get a `ResponseFooter` inserted after
///   their last visible row.
/// - When `busy_working` is true a `WorkingIndicator` is appended to the end
///   (supplied by the caller only in Chat's live projection).
pub fn folded_conversation_rows(
    messages: &[TranscriptMessage],
    turns: &[ConversationTurn],
    running_turns: &HashSet<usize>,
    expanded_turns: &HashSet<usize>,
    busy_working: bool,
) -> Vec<ConversationRow> {
    // Per turn: fold anchor (first hidden row → TurnFold position), hidden row
    // set, footer.
    struct TurnFoldPlan {
        anchor_row: ConversationRow,
        hidden: Vec<ConversationRow>,
    }
    let mut plans: Vec<Option<TurnFoldPlan>> = Vec::with_capacity(turns.len());
    let mut answer_by_turn: HashMap<usize, bool> = HashMap::new();

    for (turn_index, turn) in turns.iter().enumerate() {
        if running_turns.contains(&turn_index) {
            plans.push(None);
            continue;
        }
        let work_rows = turn_work_rows(messages, turn);
        let answer_start = turn_answer_position(&work_rows);
        if work_rows.is_empty() {
            plans.push(None);
            continue;
        }
        let has_visible_answer = answer_start < work_rows.len();
        let hidden: Vec<ConversationRow> = work_rows[..answer_start].to_vec();
        let Some(anchor_row) = hidden.first().copied() else {
            // No foldable work (pure-answer turn): no fold, but a footer may
            // still apply.
            plans.push(None);
            answer_by_turn.insert(turn_index, has_visible_answer);
            continue;
        };
        answer_by_turn.insert(turn_index, has_visible_answer);
        plans.push(Some(TurnFoldPlan { anchor_row, hidden }));
    }

    let hidden_by_row: HashSet<ConversationRow> = plans
        .iter()
        .flatten()
        .flat_map(|plan| plan.hidden.iter().copied())
        .collect();
    let expanded = |turn_index: usize| expanded_turns.contains(&turn_index);

    let mut rows: Vec<ConversationRow> = Vec::new();
    for (turn_index, turn) in turns.iter().enumerate() {
        let plan: Option<&TurnFoldPlan> = plans[turn_index].as_ref();
        let turn_rows = turn_message_rows(messages, turn.range.clone(), true);
        for row in turn_rows {
            if let Some(inserted_plan) = plan {
                if inserted_plan.anchor_row == row {
                    rows.push(ConversationRow::TurnFold(turn_index));
                }
            }
            let is_hidden = hidden_by_row.contains(&row) && plan.is_some() && !expanded(turn_index);
            if !is_hidden {
                rows.push(row);
            }
        }
        // The footer belongs to the turn as a whole: insert it after the turn's
        // last visible row. Foldability and having a visible answer are
        // independent (pure-answer turns get a footer too).
        if answer_by_turn.get(&turn_index).copied().unwrap_or(false)
            && !running_turns.contains(&turn_index)
        {
            rows.push(ConversationRow::ResponseFooter(turn_index));
        }
    }
    // Show Working only when busy and an active turn exists (a failed drive can
    // drop busy on its own while the turn stays marked running — "Working"
    // overlaid on a failure would mislead).
    if busy_working && !running_turns.is_empty() {
        rows.push(ConversationRow::WorkingIndicator);
    }
    rows
}

/// Row fingerprint: every structural field the folding reads participates in
/// the mix, so the frame path can skip refolding based on it. Cheap mixing
/// rather than a real hash; kept in sync with `folded_conversation_rows`.
pub fn rows_fingerprint(
    messages: &[TranscriptMessage],
    turns: &[ConversationTurn],
    running_turns: &HashSet<usize>,
    expanded_turns: &HashSet<usize>,
    busy_working: bool,
) -> u64 {
    let mut hash = mix(EMPTY_FINGERPRINT, busy_working as u64);
    hash = mix(hash, messages.len() as u64);
    for message in messages {
        hash = mix(hash, message.role as u64);
        hash = mix(hash, message.kind as u64);
        hash = mix(hash, message.text.trim().is_empty() as u64);
        hash = mix(
            hash,
            message
                .thinking
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty()) as u64,
        );
        hash = mix(hash, message.tool_calls.len() as u64);
        for tool in &message.tool_calls {
            hash = mix(hash, tool.output.as_ref().is_some() as u64);
        }
    }
    hash = mix(hash, turns.len() as u64);
    for turn in turns {
        hash = mix(hash, turn.start as u64);
        hash = mix(hash, turn.range.end as u64);
    }
    hash = mix(hash, running_turns.len() as u64);
    // HashSet has no stable iteration order: merge members with an
    // order-independent sum.
    let running_sum = running_turns.iter().fold(0u64, |sum, turn| {
        sum.wrapping_add(mix(EMPTY_FINGERPRINT, *turn as u64))
    });
    hash = mix(hash, running_sum);
    let expanded_sum = expanded_turns.iter().fold(0u64, |sum, turn| {
        sum.wrapping_add(mix(EMPTY_FINGERPRINT, *turn as u64))
    });
    mix(mix(hash, expanded_turns.len() as u64), expanded_sum)
}

/// The "no conversation" fingerprint; also the seed for every other
/// fingerprint.
pub const EMPTY_FINGERPRINT: u64 = 0xcbf2_9ce4_8422_2325;

const FINGERPRINT_PRIME: u64 = 0x0000_0100_0000_01b3;

fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(FINGERPRINT_PRIME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shardlane_history::{MessageKind, ToolCall};

    fn user(text: &str) -> TranscriptMessage {
        TranscriptMessage {
            seq: 0,
            role: Role::User,
            kind: MessageKind::Text,
            text: text.into(),
            truncated: false,
            tool_calls: Vec::new(),
            thinking: None,
            timestamp: None,
            model: None,
        }
    }

    fn meta(text: &str) -> TranscriptMessage {
        TranscriptMessage {
            kind: MessageKind::Meta,
            ..user(text)
        }
    }

    fn assistant(text: &str, thinking: Option<&str>, tools: Vec<ToolCall>) -> TranscriptMessage {
        TranscriptMessage {
            seq: 0,
            role: Role::Assistant,
            kind: MessageKind::Text,
            text: text.into(),
            truncated: false,
            tool_calls: tools,
            thinking: thinking.map(str::to_string),
            timestamp: None,
            model: None,
        }
    }

    fn tool(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "Bash".into(),
            input_preview: String::new(),
            input: None,
            output: Some("ok".into()),
            is_error: false,
            sidechain_ref: None,
        }
    }

    fn turns_of(messages: &[TranscriptMessage]) -> Vec<ConversationTurn> {
        derive_turns(messages)
    }

    fn folded(
        messages: &[TranscriptMessage],
        running: &HashSet<usize>,
        expanded: &HashSet<usize>,
        busy: bool,
    ) -> Vec<ConversationRow> {
        let turns = turns_of(messages);
        folded_conversation_rows(messages, &turns, running, expanded, busy)
    }

    #[test]
    fn user_message_boundaries_start_turns() {
        let messages = vec![
            user("first question"),
            assistant("first answer", None, vec![]),
            meta("compact meta"),
            user("second question"),
            assistant("second answer", None, vec![]),
        ];
        let turns = turns_of(&messages);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].range, 0..3);
        assert_eq!(turns[1].range, 3..5);

        // Meta rows do not open a new turn.
        let meta_only = vec![
            assistant("greeting", None, vec![]),
            meta("<context>x</context>"),
        ];
        assert_eq!(turns_of(&meta_only).len(), 1);
    }

    #[test]
    fn settled_work_folds_while_answer_stays_visible() {
        let messages = vec![
            user("fix it"),
            assistant("done", Some("thought first"), vec![tool("t1")]),
        ];
        let rows = folded(&messages, &HashSet::new(), &HashSet::new(), false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnFold(0),
                ConversationRow::Answer(1),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn fold_anchor_sits_at_top_of_work() {
        // The turn shows its fold row where work starts; the run of answer text
        // from the last work to the end of the turn is fully visible
        // (answer = trailing text run, possibly spanning multiple messages).
        let messages = vec![
            user("task"),
            assistant("interlude", Some("thought"), vec![tool("t1")]),
            assistant("final answer", None, vec![]),
        ];
        let rows = folded(&messages, &HashSet::new(), &HashSet::new(), false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnFold(0),
                ConversationRow::Answer(1),
                ConversationRow::Answer(2),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn running_turn_never_folds() {
        let messages = vec![
            user("task"),
            assistant("still working", Some("thought"), vec![tool("t1")]),
        ];
        let running: HashSet<usize> = [0].into();
        let rows = folded(&messages, &running, &HashSet::new(), true);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::Reasoning(1),
                ConversationRow::ToolActivity {
                    message: 1,
                    tool: 0
                },
                ConversationRow::Answer(1),
                ConversationRow::WorkingIndicator,
            ]
        );
    }

    #[test]
    fn expanded_fold_restores_original_order() {
        // Expansion restores the hidden rows' original order (the fold header
        // stays at its anchor), and the answer run and footer keep their
        // original relative positions.
        let messages = vec![
            user("task"),
            assistant("midway", Some("thought"), vec![tool("t1")]),
            assistant("final", None, vec![]),
        ];
        let expanded: HashSet<usize> = [0].into();
        let rows = folded(&messages, &HashSet::new(), &expanded, false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnFold(0),
                ConversationRow::Reasoning(1),
                ConversationRow::ToolActivity {
                    message: 1,
                    tool: 0
                },
                ConversationRow::Answer(1),
                ConversationRow::Answer(2),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn tool_only_turn_folds_entirely_without_footer() {
        let messages = vec![
            user("run a command"),
            assistant("", Some("thought"), vec![tool("t1")]),
        ];
        let rows = folded(&messages, &HashSet::new(), &HashSet::new(), false);
        assert_eq!(
            rows,
            vec![ConversationRow::UserPrompt(0), ConversationRow::TurnFold(0),],
            "a turn with no answer folds entirely and has no footer"
        );
    }

    #[test]
    fn pure_answer_turn_needs_no_fold() {
        let messages = vec![user("hello"), assistant("hello!", None, vec![])];
        let rows = folded(&messages, &HashSet::new(), &HashSet::new(), false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::Answer(1),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn working_indicator_requires_explicit_busy_state() {
        let messages = vec![user("q"), assistant("a", None, vec![])];
        // Static (History) projection: busy=false never produces a Working row.
        assert!(!folded(&messages, &HashSet::new(), &HashSet::new(), false)
            .contains(&ConversationRow::WorkingIndicator));
        // Live busy with no running turn: also none (a failed/finished busy
        // must not masquerade as Working).
        assert!(!folded(&messages, &HashSet::new(), &HashSet::new(), true)
            .contains(&ConversationRow::WorkingIndicator));
        let running: HashSet<usize> = [0].into();
        assert!(folded(&messages, &running, &HashSet::new(), true)
            .contains(&ConversationRow::WorkingIndicator));
    }

    #[test]
    fn fingerprint_tracks_every_fold_input() {
        let base = vec![
            user("task"),
            assistant("midway", Some("thought"), vec![tool("t1")]),
            assistant("final", None, vec![]),
        ];
        let empty: HashSet<usize> = HashSet::new();
        let expanded: HashSet<usize> = [0].into();
        let running: HashSet<usize> = [0].into();

        let base_hash = rows_fingerprint(&base, &turns_of(&base), &empty, &empty, false);
        assert_eq!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), &empty, &empty, false),
            "the fingerprint is stable for identical input"
        );
        assert_ne!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), &expanded, &empty, false),
            "a changed expanded set changes the fingerprint"
        );
        assert_ne!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), &running, &empty, false),
            "a changed running set changes the fingerprint"
        );
        assert_ne!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), &empty, &empty, true),
            "a changed busy state changes the fingerprint"
        );

        let mut grew = base.clone();
        grew.push(user("follow-up"));
        assert_ne!(
            base_hash,
            rows_fingerprint(&grew, &turns_of(&grew), &empty, &empty, false),
            "message growth changes the fingerprint"
        );

        // The expanded set is order-independent.
        let a: HashSet<usize> = [1, 2, 3].into();
        let b: HashSet<usize> = [3, 1, 2].into();
        assert_eq!(
            rows_fingerprint(&base, &turns_of(&base), &empty, &a, false),
            rows_fingerprint(&base, &turns_of(&base), &empty, &b, false),
        );
    }

    #[test]
    fn blank_messages_do_not_become_rows() {
        let messages = vec![
            user("task"),
            TranscriptMessage {
                text: String::new(),
                ..assistant("", None, vec![])
            },
            TranscriptMessage {
                role: Role::System,
                kind: MessageKind::Text,
                text: "   ".into(),
                ..assistant("", None, vec![])
            },
            assistant("final", None, vec![]),
        ];
        let rows = folded(&messages, &HashSet::new(), &HashSet::new(), false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::Answer(3),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn context_boundary_compact_summary_becomes_row() {
        let messages = vec![
            user("task"),
            TranscriptMessage {
                role: Role::System,
                kind: MessageKind::CompactSummary,
                text: "── Context compacted ──".into(),
                ..assistant("", None, vec![])
            },
            assistant("final", None, vec![]),
        ];
        // Expanded turn shows ContextBoundary
        let mut expanded = HashSet::new();
        expanded.insert(0);
        let rows = folded(&messages, &HashSet::new(), &expanded, false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnFold(0),
                ConversationRow::ContextBoundary(1),
                ConversationRow::Answer(2),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    /// History↔Live projection bridge: messages from the same provider fixture
    /// decoded incrementally via the live path (arbitrary splits + settle)
    /// project, through the shared projection, to exactly the same
    /// turn/row/fold sequence as the whole decoded at once.
    #[test]
    fn live_incremental_source_projects_identically_to_direct_decode() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("parity.jsonl");
        let lines = [
            r#"{"type":"user","cwd":"/w","timestamp":"2026-08-01T01:00:00Z","message":{"content":"task"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","content":[{"type":"thinking","thinking":"idea"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-01T01:00:02Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-01T01:00:03Z","message":{"id":"m2","content":[{"type":"text","text":"done"}]}}"#,
            r#"{"type":"user","timestamp":"2026-08-01T01:00:04Z","message":{"content":"follow-up"}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-01T01:00:05Z","message":{"id":"m3","content":[{"type":"text","text":"answer"}]}}"#,
        ];
        let content = lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<Vec<_>>()
            .join("");
        std::fs::write(&path, &content)?;

        fn append_bytes(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
            let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
            std::io::Write::write_all(&mut file, bytes)?;
            Ok(())
        }

        fn decode(
            path: &std::path::Path,
            content: &str,
            splits: &[usize],
        ) -> anyhow::Result<Vec<shardlane_history::TranscriptMessage>> {
            std::fs::write(path, b"")?;
            let mut session =
                shardlane_history::live::LiveSession::open(shardlane_history::ConversationRef {
                    agent: shardlane_history::AgentId::ClaudeCode,
                    native_id: "parity".into(),
                    file_path: path.to_string_lossy().to_string(),
                    mtime_ms: 0,
                    size: 0,
                })?;
            let bytes = content.as_bytes();
            let mut start = 0usize;
            for &cut in splits {
                let end = cut.min(bytes.len()).max(start);
                if end == start {
                    continue;
                }
                append_bytes(path, &bytes[start..end])?;
                session.sync()?;
                start = end;
            }
            if start < bytes.len() {
                append_bytes(path, &bytes[start..])?;
                session.sync()?;
            }
            session.settle();
            Ok(session.snapshot().messages)
        }

        let direct = decode(&path, &content, &[usize::MAX])?;
        for splits in [
            vec![1, 17, 130, 401],
            vec![7, 13, 29],
            vec![content.len() / 2],
        ] {
            let incremental = decode(&path, &content, &splits)?;
            assert_eq!(incremental, direct, "splits {splits:?}");

            let turns = derive_turns(&direct);
            let empty: HashSet<usize> = HashSet::new();
            let expanded: HashSet<usize> = [0].into();
            assert_eq!(
                folded_conversation_rows(&incremental, &turns, &empty, &expanded, false),
                folded_conversation_rows(&direct, &turns, &empty, &expanded, false),
                "splits {splits:?}"
            );
        }
        Ok(())
    }
}
