package conversation

import (
	"github.com/wh-studio/herdr-client/internal/history"
)

// ItemsFromTranscript converts normalized history messages into the
// provider-neutral conversation items (CONV-07): the History read adapter
// shares the exact ConversationWindow shape with Live.
func ItemsFromTranscript(messages []history.TranscriptMessage) []ConversationItem {
	items := make([]ConversationItem, 0, len(messages))
	for _, message := range messages {
		items = append(items, ItemFromTranscriptMessage(message))
	}
	return items
}

// ItemFromTranscriptMessage maps one normalized history message onto the
// provider-neutral item vocabulary.
func ItemFromTranscriptMessage(message history.TranscriptMessage) ConversationItem {
	kind := KindAssistant
	role := string(message.Role)
	switch {
	case message.Kind == history.MessageCompactSummary:
		kind = KindCompactSummary
	case message.Kind == history.MessageMeta:
		kind = KindMeta
	case message.Role == history.RoleUser:
		kind = KindUser
	case message.Role == history.RoleSystem:
		kind = KindMeta
	}
	item := ConversationItem{
		ID:          string(message.Kind) + "-" + itoa(message.Seq),
		Seq:         uint64(message.Seq),
		Kind:        kind,
		Role:        role,
		Text:        message.Text,
		Thinking:    message.Thinking,
		TimestampMS: message.Timestamp,
		Model:       message.Model,
		Truncated:   message.Truncated,
	}
	for _, call := range message.ToolCalls {
		item.ToolCalls = append(item.ToolCalls, ConversationToolCall{
			ID:           call.ID,
			Name:         call.Name,
			InputPreview: call.InputPreview,
			Output:       call.Output,
			IsError:      call.IsError,
		})
	}
	return item
}

// ProvisionalItem maps the live decoder's uncommitted streaming tail onto a
// provisional item (0.6 §7.3): it renders after the committed rows and is
// replaced when the next role boundary commits the row.
func ProvisionalItem(message history.TranscriptMessage) ConversationItem {
	item := ItemFromTranscriptMessage(message)
	item.ID = "pending-" + itoa(message.Seq)
	return item
}

func itoa(value int64) string {
	if value == 0 {
		return "0"
	}
	digits := ""
	negative := value < 0
	if negative {
		value = -value
	}
	for value > 0 {
		digits = string(rune('0'+value%10)) + digits
		value /= 10
	}
	if negative {
		return "-" + digits
	}
	return digits
}
