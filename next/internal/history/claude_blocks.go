package history

import (
	"encoding/json"
	"strings"
)

func (s *claudeParseState) feedClaudeSystem(row map[string]any, timestamp int64) {
	s.flushPending()
	subtype, _ := row["subtype"].(string)
	if subtype == "compact_boundary" {
		s.push(TranscriptMessage{
			Role:      RoleSystem,
			Kind:      MessageCompactSummary,
			Text:      "── Context compacted ──",
			ToolCalls: []ToolCall{},
			Timestamp: optionalTimestamp(timestamp),
		})
		return
	}
	content, _ := row["content"].(string)
	if content == "" {
		return
	}
	text, truncated := clipText(content, MaxToolIO)
	s.push(TranscriptMessage{
		Role:      RoleSystem,
		Kind:      MessageMeta,
		Text:      text,
		Truncated: truncated,
		ToolCalls: []ToolCall{},
		Timestamp: optionalTimestamp(timestamp),
	})
}

func (s *claudeParseState) feedClaudeUser(row map[string]any, timestamp int64) {
	s.flushPending()
	message, _ := row["message"].(map[string]any)
	if message == nil {
		return
	}

	var parts []string
	switch content := message["content"].(type) {
	case string:
		parts = append(parts, content)
	case []any:
		for _, rawBlock := range content {
			block, _ := rawBlock.(map[string]any)
			switch blockType, _ := block["type"].(string); blockType {
			case "text":
				if text, _ := block["text"].(string); text != "" {
					parts = append(parts, text)
				}
			case "image":
				parts = append(parts, "[image]")
			case "tool_result":
				s.applyClaudeToolResult(block)
			}
		}
	}

	text := strings.TrimSpace(strings.Join(parts, "\n\n"))
	if text == "" {
		return
	}
	kind := MessageText
	if compact, _ := row["isCompactSummary"].(bool); compact {
		kind = MessageCompactSummary
	} else if meta, _ := row["isMeta"].(bool); meta || injectedUserContent(text) {
		kind = MessageMeta
	}
	if kind == MessageText && s.fallbackTitle == "" {
		s.fallbackTitle = cleanTitleCandidate(text)
	}
	clipped, truncated := clipText(text, MaxMessageText)
	s.push(TranscriptMessage{
		Role:      RoleUser,
		Kind:      kind,
		Text:      clipped,
		Truncated: truncated,
		ToolCalls: []ToolCall{},
		Timestamp: optionalTimestamp(timestamp),
	})
}

func (s *claudeParseState) applyClaudeToolResult(block map[string]any) {
	id, _ := block["tool_use_id"].(string)
	index, ok := s.toolIndex[id]
	if !ok || index[0] >= len(s.messages) || index[1] >= len(s.messages[index[0]].ToolCalls) {
		return
	}
	output := stringifyToolResult(block["content"])
	output, _ = clipText(output, MaxToolIO)
	tool := &s.messages[index[0]].ToolCalls[index[1]]
	tool.Output = &output
	tool.IsError, _ = block["is_error"].(bool)
}

func (s *claudeParseState) feedClaudeAssistant(row map[string]any, timestamp int64) {
	message, _ := row["message"].(map[string]any)
	if message == nil {
		return
	}
	msgID, _ := message["id"].(string)
	needNew := s.pending == nil
	if s.pending != nil && s.pending.msgID != "" && msgID != "" && s.pending.msgID != msgID {
		needNew = true
	}
	if needNew {
		s.flushPending()
		s.pending = &claudePending{msgID: msgID, timestamp: optionalTimestamp(timestamp)}
	}
	current := s.pending
	if current == nil {
		return
	}
	if current.msgID == "" {
		current.msgID = msgID
	}

	if model, _ := message["model"].(string); model != "" && model != "<synthetic>" {
		current.model = stringPointer(model)
		s.model = stringPointer(model)
	}
	if usage, _ := message["usage"].(map[string]any); usage != nil {
		s.tokensUsed += numberI64(usage["input_tokens"])
		s.tokensUsed += numberI64(usage["output_tokens"])
		s.tokensUsed += numberI64(usage["cache_creation_input_tokens"])
	}

	switch content := message["content"].(type) {
	case string:
		if strings.TrimSpace(content) != "" {
			current.text = append(current.text, content)
		}
	case []any:
		for _, rawBlock := range content {
			block, _ := rawBlock.(map[string]any)
			switch blockType, _ := block["type"].(string); blockType {
			case "text":
				if text, _ := block["text"].(string); strings.TrimSpace(text) != "" {
					current.text = append(current.text, text)
				}
			case "thinking":
				if text, _ := block["thinking"].(string); strings.TrimSpace(text) != "" {
					current.thinking = append(current.thinking, text)
				}
			case "tool_use":
				current.tools = append(current.tools, claudeToolCall(block))
			}
		}
	}
}

func claudeToolCall(block map[string]any) ToolCall {
	id, _ := block["id"].(string)
	name, _ := block["name"].(string)
	if name == "" {
		name = "tool"
	}
	preview, full := toolInput(block["input"])
	return ToolCall{
		ID:           id,
		Name:         name,
		InputPreview: preview,
		Input:        full,
	}
}

func optionalTimestamp(value int64) *int64 {
	if value <= 0 {
		return nil
	}
	return &value
}

func numberI64(value any) int64 {
	switch number := value.(type) {
	case float64:
		return int64(number)
	case json.Number:
		value, _ := number.Int64()
		return value
	default:
		return 0
	}
}
