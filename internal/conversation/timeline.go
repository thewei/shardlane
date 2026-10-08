package conversation

import "strings"

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
			})
		case KindAssistant:
			if current == nil || current.UserRow {
				flush()
				current = &TimelineTurn{FirstSeq: item.Seq}
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
			}
			for _, call := range item.ToolCalls {
				current.ToolRuns = append(current.ToolRuns, call)
			}
			if strings.Contains(strings.ToLower(item.Text), "aborted") || item.Truncated && strings.Contains(item.Text, "stopped") {
				current.Failed = true
			}
			current.LastSeq = item.Seq
		case KindTool:
			if current == nil || current.UserRow {
				flush()
				current = &TimelineTurn{FirstSeq: item.Seq}
			}
			for _, call := range item.ToolCalls {
				current.ToolRuns = append(current.ToolRuns, call)
			}
			current.LastSeq = item.Seq
		case KindMeta, KindCompactSummary, KindReasoning, KindActivity:
			// Meta/compact rows do not open turns; reasoning merges into the
			// current turn's thinking when one is open.
			if current != nil && item.Kind == KindReasoning && item.Text != "" {
				merged := consolidateThinking(deref(current.Thinking), item.Text)
				current.Thinking = &merged
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

func consolidateThinking(existing, addition string) string {
	if existing == "" {
		return addition
	}
	return existing + "\n\n" + addition
}
