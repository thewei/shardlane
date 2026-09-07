//! Shared conversation timeline: provider-neutral row projection with
//! ChatGPT-style process semantics (shared by History and Chat).
//!
//! [INPUT]: depends on shardlane-history's TranscriptMessage/Role/MessageKind;
//! zero GPUI, zero app state, zero I/O — pure functions.
//! [OUTPUT]: derive_turns / folded_conversation_rows / rows_fingerprint and the
//! ConversationRow/ConversationTurn models.
//! [POS]: conversation projection primitives for herdr-gui agent_ui. The
//! presentation contract is the ChatGPT model: assistant text (narration AND
//! final answer alike) is NEVER folded — the narration between tool calls is
//! the visible progress report; thinking consolidates into ONE collapsible
//! block per turn — "Thought for Xs" when measurable — anchored at the first
//! thinking position (expansion keyed by the turn's first message seq,
//! presentational, owned by each surface); tool calls render as compact chip
//! rows, and consecutive runs (>= TOOL_COMPACT_MIN = 2; runs may span
//! messages once thinking is consolidated) collapse into one expandable
//! ToolGroup row. A turn
//! boundary is a Text-kind User message (plan §11.1). History provides a
//! static projection; Chat adds live busy state; the two do not share
//! lifecycles. The former turn-level
//! "Worked for Xs" mega-fold is retired: hiding the narration hid the progress.

use shardlane_history::{MessageKind, Role, TranscriptMessage};
use std::collections::HashSet;
use std::ops::Range;

/// Runs of at least this many consecutive tool-call chips compact into one
/// ToolGroup row (ChatGPT contract: repeated chips must not wall the
/// timeline; the narration between runs stays visible).
pub const TOOL_COMPACT_MIN: usize = 2;

/// One presentation turn: bounded by a Text-kind User message (plan §11.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationTurn {
    /// Index of the turn's first message (usually the user prompt row).
    pub start: usize,
    /// Half-open range [start, end) the turn covers; end is the next turn's
    /// start.
    pub range: Range<usize>,
}

/// Timeline rows; usize indices all point into the input messages slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConversationRow {
    /// User prompt (turn opening).
    UserPrompt(usize),
    /// Context compaction / system boundary row (from CompactSummary / system
    /// meta messages).
    ContextBoundary(usize),
    /// One consolidated thinking block per turn (all of the turn's thinking,
    /// collapsed to "Thought for Xs" by default; expansion is presentational
    /// and keyed by the turn's first message seq).
    TurnThinking(usize),
    /// Tool activity chip row (the toolth tool_call of an assistant message).
    ToolActivity { message: usize, tool: usize },
    /// Compact group replacing a run of at least TOOL_COMPACT_MIN consecutive
    /// ToolActivity rows. `message`/`tool` locate the run's first call and
    /// `count` is the number of hidden calls; expansion is keyed by
    /// `(messages[message].seq, tool)` and is caller-owned presentation
    /// state.
    ToolGroup {
        message: usize,
        tool: usize,
        count: usize,
    },
    /// Assistant text — narration and final answer alike. Text is never
    /// folded: it IS the progress report the user reads while tools run.
    Answer(usize),
    /// Turn footer (copy/time), inserted after the turn's last row — belongs
    /// to the turn as a whole, not to the last text message to arrive.
    ResponseFooter(usize),
    /// Working indicator row. Appears only when the caller explicitly provides
    /// a busy state: the History static projection passes false and never
    /// produces it; Chat produces it while Herdr is working.
    WorkingIndicator,
    /// Failed-run marker closing the last turn (Codex "Stopped" contract: the
    /// failure scene stays visible and the timeline says so). Produced only
    /// when the caller reports the live run failed; History never produces it.
    TurnStopped(usize),
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

/// Rows one turn produces over its message range. Per assistant message the
/// fixed order is Reasoning then Answer then ToolActivity: thinking announces,
/// narration explains, then the action chips follow (the persisted format
/// loses the inter-block arrival order; narrate-then-act is the dominant
/// provider pattern, so text-before-tools is the faithful default).
fn turn_message_rows(
    messages: &[TranscriptMessage],
    range: Range<usize>,
    include_prompt: bool,
    turn_index: usize,
) -> Vec<ConversationRow> {
    let mut rows = Vec::new();
    let mut first_thinking_pos: Option<usize> = None;
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
                    // One consolidated block per turn, anchored where the
                    // first thinking would have rendered.
                    if first_thinking_pos.is_none() {
                        first_thinking_pos = Some(rows.len());
                    }
                }
                if !message.text.trim().is_empty() {
                    rows.push(ConversationRow::Answer(index));
                }
                for tool in 0..message.tool_calls.len() {
                    rows.push(ConversationRow::ToolActivity {
                        message: index,
                        tool,
                    });
                }
            }
            Role::System => {
                if message.kind == MessageKind::CompactSummary || !message.text.trim().is_empty() {
                    rows.push(ConversationRow::ContextBoundary(index));
                }
            }
        }
    }
    if let Some(pos) = first_thinking_pos {
        rows.insert(pos, ConversationRow::TurnThinking(turn_index));
    }
    rows
}

pub fn turn_has_visible_text(messages: &[TranscriptMessage], turn: &ConversationTurn) -> bool {
    messages[turn.range.clone()]
        .iter()
        .any(|message| message.role == Role::Assistant && !message.text.trim().is_empty())
}

/// Visible row sequence (ChatGPT presentation contract):
///
/// - Every row is visible; nothing is hidden behind a turn-level fold. The
///   narration between tools stays readable and the thinking/tool detail
///   collapses per row (presentational, owned by each surface).
/// - Runs of TOOL_COMPACT_MIN+ consecutive tool chips compact into one
///   ToolGroup row unless the caller marks that group expanded.
/// - Turns with assistant text get a ResponseFooter after their last row;
///   the still-running last turn (when in_progress) gets its footer only
///   after it settles.
/// - When show_working is true a WorkingIndicator closes the timeline
///   (supplied by the caller only in Chat's live projection).
pub fn folded_conversation_rows(
    messages: &[TranscriptMessage],
    turns: &[ConversationTurn],
    in_progress: bool,
    show_working: bool,
    last_turn_failed: bool,
    expanded_tool_groups: &HashSet<(i64, usize)>,
) -> Vec<ConversationRow> {
    let last_turn = turns.len().checked_sub(1);
    let mut rows: Vec<ConversationRow> = Vec::new();
    for (turn_index, turn) in turns.iter().enumerate() {
        let mut turn_rows = turn_message_rows(messages, turn.range.clone(), true, turn_index);
        compact_tool_runs(&mut turn_rows, messages, expanded_tool_groups);
        rows.extend(turn_rows);
        if turn_has_visible_text(messages, turn) && !(in_progress && Some(turn_index) == last_turn)
        {
            rows.push(ConversationRow::ResponseFooter(turn_index));
        }
    }
    if show_working && !turns.is_empty() {
        rows.push(ConversationRow::WorkingIndicator);
    }
    if last_turn_failed && !turns.is_empty() {
        rows.push(ConversationRow::TurnStopped(turns.len() - 1));
    }
    rows
}

/// The turn's full thinking chain (all assistant messages concatenated in
/// order; empty when the turn never thought).
pub fn turn_thinking_text(messages: &[TranscriptMessage], turn: &ConversationTurn) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for message in &messages[turn.range.clone()] {
        if let Some(text) = message
            .thinking
            .as_ref()
            .filter(|text| !text.trim().is_empty())
        {
            parts.push(text.trim());
        }
    }
    parts.join("\n\n")
}

/// Thinking span for the "Thought for Xs" label: first → last timestamp among
/// the turn's thinking messages (both seconds and milliseconds occur in the
/// persisted format; normalized by magnitude). None when unmeasurable.
pub fn turn_thinking_duration(
    messages: &[TranscriptMessage],
    turn: &ConversationTurn,
) -> Option<std::time::Duration> {
    let stamps: Vec<i64> = messages[turn.range.clone()]
        .iter()
        .filter(|message| {
            message
                .thinking
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
        })
        .filter_map(|message| message.timestamp)
        .filter(|ts| *ts > 0)
        .collect();
    let first = *stamps.first()?;
    let last = *stamps.last()?;
    let (first, last) = normalize_stamp_pair(first, last);
    (last > first).then(|| std::time::Duration::from_millis((last - first) as u64))
}

fn normalize_stamp_pair(a: i64, b: i64) -> (i64, i64) {
    fn to_ms(ts: i64) -> i64 {
        if ts > 1_000_000_000_000 {
            ts
        } else {
            ts * 1000
        }
    }
    (to_ms(a), to_ms(b))
}

/// Replace maximal runs of consecutive ToolActivity rows (never spanning a
/// turn: UserPrompt/Answer/Reasoning rows break the run) with a single
/// ToolGroup when the run is long enough and not user-expanded.
fn compact_tool_runs(
    rows: &mut Vec<ConversationRow>,
    messages: &[TranscriptMessage],
    expanded_tool_groups: &HashSet<(i64, usize)>,
) {
    if rows
        .iter()
        .filter(|row| matches!(row, ConversationRow::ToolActivity { .. }))
        .count()
        < TOOL_COMPACT_MIN
    {
        return;
    }
    let mut out: Vec<ConversationRow> = Vec::with_capacity(rows.len());
    let mut run: Vec<ConversationRow> = Vec::new();
    // Runs may span message boundaries (the consolidated TurnThinking row no
    // longer breaks them): keep the actual rows so expansion re-emits them
    // exactly.
    fn flush(
        out: &mut Vec<ConversationRow>,
        run: &mut Vec<ConversationRow>,
        messages: &[TranscriptMessage],
        expanded_tool_groups: &HashSet<(i64, usize)>,
    ) {
        let compactable = run.len() >= TOOL_COMPACT_MIN;
        let expanded = run.first().is_some_and(|first| match first {
            ConversationRow::ToolActivity { message, tool } => messages
                .get(*message)
                .map(|m| m.seq)
                .is_some_and(|seq| expanded_tool_groups.contains(&(seq, *tool))),
            _ => false,
        });
        if compactable && !expanded {
            if let Some(ConversationRow::ToolActivity { message, tool }) = run.first().copied() {
                out.push(ConversationRow::ToolGroup {
                    message,
                    tool,
                    count: run.len(),
                });
                run.clear();
                return;
            }
        }
        if expanded {
            // Expanded groups keep their header row as the anchor; the member
            // rows follow (the surfaces indent them visually).
            if let Some(ConversationRow::ToolActivity { message, tool }) = run.first().copied() {
                out.push(ConversationRow::ToolGroup {
                    message,
                    tool,
                    count: run.len(),
                });
            }
        }
        out.append(run);
    }
    for row in rows.drain(..) {
        if matches!(row, ConversationRow::ToolActivity { .. }) {
            run.push(row);
        } else {
            flush(&mut out, &mut run, messages, expanded_tool_groups);
            out.push(row);
        }
    }
    flush(&mut out, &mut run, messages, expanded_tool_groups);
    *rows = out;
}

/// Row fingerprint: every structural field the projection reads participates
/// in the mix, so the frame path can skip re-projection based on it. Cheap
/// mixing rather than a real hash; kept in sync with
/// folded_conversation_rows.
pub fn rows_fingerprint(
    messages: &[TranscriptMessage],
    turns: &[ConversationTurn],
    in_progress: bool,
    show_working: bool,
    last_turn_failed: bool,
    expanded_tool_groups: &HashSet<(i64, usize)>,
) -> u64 {
    let mut hash = mix(EMPTY_FINGERPRINT, in_progress as u64);
    hash = mix(hash, show_working as u64);
    hash = mix(hash, last_turn_failed as u64);
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
    // HashSet has no stable iteration order: merge members with an
    // order-independent sum.
    let expanded_sum = expanded_tool_groups.iter().fold(0u64, |sum, (seq, tool)| {
        sum.wrapping_add(mix(mix(EMPTY_FINGERPRINT, *seq as u64), *tool as u64))
    });
    hash = mix(mix(hash, expanded_tool_groups.len() as u64), expanded_sum);
    hash
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

    fn rows_of(messages: &[TranscriptMessage], busy: bool) -> Vec<ConversationRow> {
        rows_with_expanded(messages, busy, busy, false, &HashSet::new())
    }

    fn rows_with_expanded(
        messages: &[TranscriptMessage],
        in_progress: bool,
        show_working: bool,
        last_turn_failed: bool,
        expanded_tool_groups: &HashSet<(i64, usize)>,
    ) -> Vec<ConversationRow> {
        folded_conversation_rows(
            messages,
            &turns_of(messages),
            in_progress,
            show_working,
            last_turn_failed,
            expanded_tool_groups,
        )
    }

    fn message_seq(messages: &[TranscriptMessage], index: usize) -> i64 {
        messages[index].seq
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
    fn narration_and_answer_stay_visible_tools_are_chips() {
        let messages = vec![
            user("task"),
            assistant(
                "checking the layout first",
                Some("thought"),
                vec![tool("t1")],
            ),
            assistant("final answer", None, vec![]),
        ];
        assert_eq!(
            rows_of(&messages, false),
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnThinking(0),
                ConversationRow::Answer(1),
                ConversationRow::ToolActivity {
                    message: 1,
                    tool: 0
                },
                ConversationRow::Answer(2),
                ConversationRow::ResponseFooter(0),
            ],
            "text is never folded: narration AND answer stay in the timeline"
        );
    }

    #[test]
    fn turn_thinking_consolidates_into_one_block() {
        // All of the turn's thinking merges into ONE block at the first
        // thinking position — per-message pills fragmented every provider
        // message into its own think-act island (user feedback 2026-09-06).
        let messages = vec![
            user("task"),
            assistant("let me look", Some("thought"), vec![tool("t1"), tool("t2")]),
            assistant("", Some("more thought"), vec![tool("t3")]),
        ];
        assert_eq!(
            rows_of(&messages, false),
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnThinking(0),
                ConversationRow::Answer(1),
                // The consolidated thinking block no longer fragments the
                // chain: the run crosses the message boundary and compacts
                // into one group.
                ConversationRow::ToolGroup {
                    message: 1,
                    tool: 0,
                    count: 3
                },
                ConversationRow::ResponseFooter(0),
            ]
        );

        let turns = turns_of(&messages);
        assert_eq!(
            turn_thinking_text(&messages, &turns[0]),
            "thought\n\nmore thought"
        );
    }

    #[test]
    fn tool_only_turn_has_no_footer() {
        let messages = vec![
            user("run a command"),
            assistant("", Some("thought"), vec![tool("t1")]),
        ];
        assert_eq!(
            rows_of(&messages, false),
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::TurnThinking(0),
                // A single call stays an individual chip (TOOL_COMPACT_MIN+ groups).
                ConversationRow::ToolActivity {
                    message: 1,
                    tool: 0,
                },
            ],
            "a turn without text has no footer (nothing to copy)"
        );
    }

    #[test]
    fn pure_answer_turn_gets_footer() {
        let messages = vec![user("hello"), assistant("hello!", None, vec![])];
        assert_eq!(
            rows_of(&messages, false),
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::Answer(1),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn busy_state_suppresses_last_turn_footer_and_appends_working() {
        let messages = vec![
            user("first"),
            assistant("first answer", None, vec![]),
            user("second"),
            assistant("working on it", Some("thought"), vec![tool("t1")]),
        ];
        let busy = rows_of(&messages, true);
        assert!(busy.contains(&ConversationRow::ResponseFooter(0)));
        assert!(
            !busy.contains(&ConversationRow::ResponseFooter(1)),
            "the still-running last turn gets its footer only after settling"
        );
        assert_eq!(busy.last(), Some(&ConversationRow::WorkingIndicator));

        // Settled: both footers, no indicator.
        let settled = rows_of(&messages, false);
        assert!(settled.contains(&ConversationRow::ResponseFooter(0)));
        assert!(settled.contains(&ConversationRow::ResponseFooter(1)));
        assert!(!settled.contains(&ConversationRow::WorkingIndicator));
    }

    #[test]
    fn working_indicator_requires_explicit_busy_state() {
        let messages = vec![user("q"), assistant("a", None, vec![])];
        // Static (History) projection: busy=false never produces a Working row.
        assert!(!rows_of(&messages, false).contains(&ConversationRow::WorkingIndicator));
        // Busy with at least one turn: indicator.
        assert!(rows_of(&messages, true).contains(&ConversationRow::WorkingIndicator));
        // Busy with no turns at all (empty conversation): no indicator.
        assert!(
            folded_conversation_rows(&[], &turns_of(&[]), true, true, false, &HashSet::new())
                .is_empty()
        );
    }

    #[test]
    fn fingerprint_tracks_every_projection_input() {
        let base = vec![
            user("task"),
            assistant("midway", Some("thought"), vec![tool("t1")]),
            assistant("final", None, vec![]),
        ];

        let empty_groups: HashSet<(i64, usize)> = HashSet::new();
        let base_hash =
            rows_fingerprint(&base, &turns_of(&base), false, false, false, &empty_groups);
        assert_eq!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), false, false, false, &empty_groups),
            "the fingerprint is stable for identical input"
        );
        assert_ne!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), true, true, false, &empty_groups),
            "a changed busy state changes the fingerprint"
        );
        assert_ne!(
            base_hash,
            rows_fingerprint(&base, &turns_of(&base), false, false, true, &empty_groups),
            "a changed failed state changes the fingerprint"
        );
        let expanded_groups: HashSet<(i64, usize)> = [(0, 0)].into();
        assert_ne!(
            base_hash,
            rows_fingerprint(
                &base,
                &turns_of(&base),
                false,
                false,
                false,
                &expanded_groups
            ),
            "a changed tool-group expansion changes the fingerprint"
        );

        let mut grew = base.clone();
        grew.push(user("follow-up"));
        assert_ne!(
            base_hash,
            rows_fingerprint(&grew, &turns_of(&grew), false, false, false, &empty_groups),
            "message growth changes the fingerprint"
        );

        // Text-only growth inside one message keeps the structural
        // fingerprint stable: row signatures carry the content deltas.
        let mut longer = base.clone();
        longer[2].text = "final answer with more text".into();
        assert_eq!(
            base_hash,
            rows_fingerprint(
                &longer,
                &turns_of(&longer),
                false,
                false,
                false,
                &empty_groups
            )
        );
    }

    #[test]
    fn failed_run_appends_stopped_marker_to_last_turn() {
        let messages = vec![
            user("first"),
            assistant("first answer", None, vec![]),
            user("second"),
            assistant("partial progress", Some("thought"), vec![tool("t1")]),
        ];
        let rows = rows_with_expanded(&messages, false, false, true, &HashSet::new());
        assert_eq!(rows.last(), Some(&ConversationRow::TurnStopped(1)));
        // The partial content stays fully visible above the marker.
        assert!(rows.contains(&ConversationRow::Answer(3)));
        assert!(rows.contains(&ConversationRow::ResponseFooter(1)));

        // Not failed: no marker anywhere.
        assert!(!rows_of(&messages, false)
            .iter()
            .any(|row| matches!(row, ConversationRow::TurnStopped(_))));
    }

    #[test]
    fn long_tool_runs_compact_into_groups() {
        let messages = vec![
            user("task"),
            assistant(
                "running the checks",
                None,
                vec![tool("t1"), tool("t2"), tool("t3"), tool("t4")],
            ),
        ];
        let rows = rows_of(&messages, false);
        assert_eq!(
            rows,
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::Answer(1),
                ConversationRow::ToolGroup {
                    message: 1,
                    tool: 0,
                    count: 4
                },
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    #[test]
    fn short_tool_runs_still_compact() {
        // MIN=2 (user feedback 2026-09-06): even a pair of calls collapses —
        // the chips were the noise.
        let messages = vec![
            user("task"),
            assistant("checking", None, vec![tool("t1"), tool("t2"), tool("t3")]),
        ];
        let rows = rows_of(&messages, false);
        assert_eq!(
            rows[2],
            ConversationRow::ToolGroup {
                message: 1,
                tool: 0,
                count: 3
            }
        );
    }

    #[test]
    fn expanded_tool_groups_stay_individual() {
        // Expanded groups keep the group header as the anchor; the member
        // rows follow it (surfaces indent them).
        let messages = vec![
            user("task"),
            assistant(
                "running the checks",
                None,
                vec![tool("t1"), tool("t2"), tool("t3"), tool("t4")],
            ),
        ];
        let seq = message_seq(&messages, 1);
        let expanded: HashSet<(i64, usize)> = [(seq, 0)].into();
        let rows = rows_with_expanded(&messages, false, false, false, &expanded);
        assert!(matches!(
            rows[2],
            ConversationRow::ToolGroup {
                message: 1,
                tool: 0,
                count: 4
            }
        ));
        assert!(matches!(
            rows[3],
            ConversationRow::ToolActivity {
                message: 1,
                tool: 0
            }
        ));
        assert!(matches!(
            rows[6],
            ConversationRow::ToolActivity {
                message: 1,
                tool: 3
            }
        ));
    }

    #[test]
    fn text_rows_break_tool_runs() {
        let messages = vec![
            user("task"),
            assistant("", None, vec![tool("t1"), tool("t2")]),
            assistant("narration", None, vec![tool("t3"), tool("t4")]),
        ];
        let rows = rows_of(&messages, false);
        // The narration stays visible between the runs and keeps them two
        // SEPARATE groups (never merged across narration).
        assert!(rows.contains(&ConversationRow::Answer(2)));
        assert!(rows.contains(&ConversationRow::ToolGroup {
            message: 1,
            tool: 0,
            count: 2
        }));
        assert!(rows.contains(&ConversationRow::ToolGroup {
            message: 2,
            tool: 0,
            count: 2
        }));
        let answer_ix = rows
            .iter()
            .position(|row| *row == ConversationRow::Answer(2));
        let group1_ix = rows
            .iter()
            .position(|row| matches!(row, ConversationRow::ToolGroup { message: 1, .. }));
        let group2_ix = rows
            .iter()
            .position(|row| matches!(row, ConversationRow::ToolGroup { message: 2, .. }));
        assert!(
            group1_ix < answer_ix && answer_ix < group2_ix,
            "narration separates the groups"
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
        assert_eq!(
            rows_of(&messages, false),
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
        assert_eq!(
            rows_of(&messages, false),
            vec![
                ConversationRow::UserPrompt(0),
                ConversationRow::ContextBoundary(1),
                ConversationRow::Answer(2),
                ConversationRow::ResponseFooter(0),
            ]
        );
    }

    /// History↔Live projection bridge: messages from the same provider fixture
    /// decoded incrementally via the live path (arbitrary splits + settle)
    /// project, through the shared projection, to exactly the same
    /// turn/row sequence as the whole decoded at once.
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
            assert_eq!(
                folded_conversation_rows(
                    &incremental,
                    &turns,
                    false,
                    false,
                    false,
                    &HashSet::new()
                ),
                folded_conversation_rows(&direct, &turns, false, false, false, &HashSet::new()),
                "splits {splits:?}"
            );
        }
        Ok(())
    }
}
