package conversation

import "strings"

// TimelineStep kinds — the ordered presentation segments of a turn.
const (
	StepNarration = "narration"
	StepThinking  = "thinking"
	StepTool      = "tool"
)

// TimelineStep is one ordered segment inside a turn: thinking, narration and
// tool calls render in their original interleaving (the turn's time chain)
// instead of grouped buckets. Consecutive same-kind segments merge — the
// chain shows boundaries, not provider chunking.
type TimelineStep struct {
	Kind string
	// Text carries narration or thinking content for those kinds.
	Text string
	// Tool is the invocation for kind tool.
	Tool *ConversationToolCall
}

// TimelineTurn is the ChatGPT-contract presentation row (0.6 §13): the
// assistant's narration stays visible, thinking consolidates into one
// collapsible block per turn, and tool activity compacts only when two or
// more consecutive tool runs appear.
type TimelineTurn struct {
	// Narration is the visible assistant text of this turn (possibly empty
	// while the turn is still streaming).
	Narration string
	// Thinking holds the consolidated thinking text for the turn; nil when
	// the turn had none.
	Thinking *string
	// ToolRuns are the compact per-call chip rows.
	ToolRuns []ConversationToolCall
	// Steps is the turn's ordered time chain (narration/thinking/tool);
	// presentation renders from this when non-empty. The consolidated
	// fields above remain the summary surface.
	Steps []TimelineStep
	// Compacted marks two-or-more consecutive tool runs folded into one
	// expandable group.
	Compacted bool
	// Failed marks a failed/aborted turn (the partial scene stays visible).
	Failed bool
	// UserRow carries a user message (its own row kind).
	UserRow    bool
	Text       string
	FirstSeq   uint64
	LastSeq    uint64
	Streamable bool
	// TsMS is the turn's first item timestamp, when the provider recorded
	// one; presentation only (hover meta, work-block summaries).
	TsMS *int64 `json:"ts_ms,omitempty"`
	// Live marks the provisional streaming tail turn: still uncommitted in
	// the live decoder, replaced at the next role boundary. Set by the live
	// bind path after derivation.
	Live bool
}

// DeriveTimeline turns the normalized item stream into Chat presentation
// rows: user rows verbatim; assistant turns with visible narration,
// consolidated thinking and compacted tool runs; failed turns keep their
// partial scene.
func DeriveTimeline(items []ConversationItem) []TimelineTurn {
	turns := make([]TimelineTurn, 0, len(items))
	var current *TimelineTurn

	flush := func() {
		if current != nil {
			turns = append(turns, *current)
			current = nil
		}
	}

	for _, item := range items {
		switch item.Kind {
		case KindUser:
			flush()
			turns = append(turns, TimelineTurn{
				UserRow:  true,
				Text:     item.Text,
				FirstSeq: item.Seq, LastSeq: item.Seq,
				TsMS: item.TimestampMS,
			})
		case KindAssistant:
			if current == nil || current.UserRow {
				flush()
				current = &TimelineTurn{FirstSeq: item.Seq, TsMS: item.TimestampMS}
			}
			if current.TsMS == nil {
				current.TsMS = item.TimestampMS
			}
			if item.Text != "" {
				if current.Narration != "" {
					current.Narration += "\n\n" + item.Text
				} else {
					current.Narration = item.Text
				}
			}
			if item.Thinking != nil && *item.Thinking != "" {
				merged := consolidateThinking(deref(current.Thinking), *item.Thinking)
				current.Thinking = &merged
				// Thinking precedes its narration inside one item: the model
				// thinks, then writes.
				addTimelineStep(current, TimelineStep{Kind: StepThinking, Text: *item.Thinking})
			}
			if item.Text != "" {
				addTimelineStep(current, TimelineStep{Kind: StepNarration, Text: item.Text})
			}
			for _, call := range item.ToolCalls {
				current.ToolRuns = append(current.ToolRuns, call)
				addTimelineStep(current, TimelineStep{Kind: StepTool, Tool: &call})
			}
			if strings.Contains(strings.ToLower(item.Text), "aborted") || item.Truncated && strings.Contains(item.Text, "stopped") {
				current.Failed = true
			}
			current.LastSeq = item.Seq
		case KindTool:
			if current == nil || current.UserRow {
				flush()
				current = &TimelineTurn{FirstSeq: item.Seq, TsMS: item.TimestampMS}
			}
			if current.TsMS == nil {
				current.TsMS = item.TimestampMS
			}
			for _, call := range item.ToolCalls {
				current.ToolRuns = append(current.ToolRuns, call)
				addTimelineStep(current, TimelineStep{Kind: StepTool, Tool: &call})
			}
			current.LastSeq = item.Seq
		case KindMeta, KindCompactSummary, KindReasoning, KindActivity:
			// Meta/compact rows do not open turns; reasoning merges into the
			// current turn's thinking when one is open.
			if current != nil && item.Kind == KindReasoning && item.Text != "" {
				merged := consolidateThinking(deref(current.Thinking), item.Text)
				current.Thinking = &merged
				addTimelineStep(current, TimelineStep{Kind: StepThinking, Text: item.Text})
			}
		}
	}
	flush()

	for index := range turns {
		turns[index].Compacted = len(turns[index].ToolRuns) >= 2
	}
	return turns
}

func deref(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

// addTimelineStep appends one ordered segment, merging consecutive
// same-kind text steps so the chain shows phase boundaries rather than
// provider chunking. Tool steps never merge — each call stays its own row.
func addTimelineStep(turn *TimelineTurn, step TimelineStep) {
	if len(turn.Steps) > 0 {
		last := &turn.Steps[len(turn.Steps)-1]
		if last.Kind == step.Kind && step.Kind != StepTool {
			last.Text = consolidateThinking(last.Text, step.Text)
			return
		}
	}
	turn.Steps = append(turn.Steps, step)
}

func consolidateThinking(existing, addition string) string {
	if existing == "" {
		return addition
	}
	return existing + "\n\n" + addition
}
