package history

import (
	"encoding/json"
	"strings"
)

// parseCodexResponseItem interprets one response_item payload. Semantics are
// ported from the audited Rust adapter: message rows become transcript text,
// reasoning rows merge into a trailing empty assistant host, tool calls attach
// to the current assistant host, and tool outputs resolve through call_id.
func (s *codexParseState) parseCodexResponseItem(payload map[string]any, timestamp int64) {
	itemType, _ := payload["type"].(string)
	switch itemType {
	case "message":
		role, _ := payload["role"].(string)
		var parts []string
		switch content := payload["content"].(type) {
		case []any:
			for _, block := range content {
				object, ok := block.(map[string]any)
				if !ok {
					continue
				}
				blockType, _ := object["type"].(string)
				if blockType == "input_text" || blockType == "output_text" || blockType == "text" {
					if text, ok := object["text"].(string); ok {
						parts = append(parts, text)
					}
				}
			}
		case string:
			parts = append(parts, content)
		}
		text := strings.TrimSpace(strings.Join(parts, "\n\n"))
		if text == "" {
			return
		}
		message := TranscriptMessage{
			Text:      text,
			Timestamp: optionalTimestamp(timestamp),
		}
		switch role {
		case "user":
			message.Role = RoleUser
			message.Kind = userKind(text)
		case "assistant":
			message.Role = RoleAssistant
			message.Kind = MessageText
		default:
			message.Role = RoleSystem
			message.Kind = MessageMeta
		}
		clipped, truncated := clipText(text, MaxMessageText)
		message.Text, message.Truncated = clipped, truncated
		s.pushCodex(message)
	case "reasoning":
		summary, ok := payload["summary"].([]any)
		if !ok {
			return
		}
		var parts []string
		for _, item := range summary {
			object, ok := item.(map[string]any)
			if !ok {
				continue
			}
			if text, ok := object["text"].(string); ok {
				parts = append(parts, text)
			}
		}
		thinking := strings.Join(parts, "\n\n")
		if thinking == "" {
			return
		}
		clipped, _ := clipText(thinking, MaxToolIO)
		if last := s.lastMessage(); last != nil && last.Role == RoleAssistant && last.Text == "" && last.Thinking == nil {
			value := clipped
			last.Thinking = &value
			return
		}
		host := TranscriptMessage{Role: RoleAssistant, Kind: MessageText, Timestamp: optionalTimestamp(timestamp)}
		host.Thinking = &clipped
		s.pushCodex(host)
	case "function_call", "custom_tool_call", "local_shell_call":
		callID, _ := payload["call_id"].(string)
		if callID == "" {
			callID, _ = payload["id"].(string)
		}
		name, ok := payload["name"].(string)
		if !ok || name == "" {
			name = "exec"
		}
		rawInput := ""
		if value, ok := payload["arguments"].(string); ok {
			rawInput = value
		} else if value, ok := payload["input"].(string); ok {
			rawInput = value
		} else if action, ok := payload["action"]; ok {
			data, err := json.Marshal(action)
			if err == nil {
				rawInput = string(data)
			}
		}
		previewSource := codexPreviewSource(rawInput)
		clippedInput, _ := clipText(rawInput, MaxToolIO)
		call := ToolCall{
			ID:           callID,
			Name:         name,
			InputPreview: codexMakePreview(previewSource),
		}
		if rawInput != "" {
			value := clippedInput
			call.Input = &value
		}
		last := s.lastMessage()
		if last == nil || last.Role != RoleAssistant || last.Kind != MessageText {
			s.pushCodex(TranscriptMessage{Role: RoleAssistant, Kind: MessageText, Timestamp: optionalTimestamp(timestamp)})
			last = s.lastMessage()
		}
		if last == nil {
			return
		}
		last.ToolCalls = append(last.ToolCalls, call)
		if callID != "" {
			s.toolIndex[callID] = [2]int{len(s.messages) - 1, len(last.ToolCalls) - 1}
		}
	case "function_call_output", "custom_tool_call_output":
		callID, _ := payload["call_id"].(string)
		location, ok := s.toolIndex[callID]
		if !ok {
			return
		}
		output := ""
		switch value := payload["output"].(type) {
		case string:
			output = value
		case map[string]any:
			if content, ok := value["content"].(string); ok {
				output = content
			} else {
				data, err := json.Marshal(value)
				if err == nil {
					output = string(data)
				}
			}
		}
		message := s.messages[location[0]]
		if location[1] >= len(message.ToolCalls) {
			return
		}
		clipped, _ := clipText(output, MaxToolIO)
		message.ToolCalls[location[1]].Output = &clipped
	}
}

func (s *codexParseState) lastMessage() *TranscriptMessage {
	if len(s.messages) == 0 {
		return nil
	}
	return &s.messages[len(s.messages)-1]
}

// codexPreviewSource parses raw tool input as JSON for preview extraction,
// degrading to the raw string when it is not JSON — matching the Rust
// preview_source behavior.
func codexPreviewSource(rawInput string) any {
	var value any
	if err := json.Unmarshal([]byte(rawInput), &value); err != nil {
		return rawInput
	}
	return value
}

// codexMakePreview builds the compact single-line tool preview pinned by the
// Rust make_preview: preferred scalar keys first, compact JSON fallback, at
// most 200 characters.
func codexMakePreview(input any) string {
	const maxPreview = 200
	candidate := ""
	switch value := input.(type) {
	case map[string]any:
		for _, key := range []string{"command", "file_path", "path", "pattern", "query", "url", "description"} {
			if text, ok := value[key].(string); ok && strings.TrimSpace(text) != "" {
				candidate = text
				break
			}
		}
		if candidate == "" {
			if data, err := json.Marshal(value); err == nil {
				candidate = string(data)
			}
		}
	case string:
		candidate = value
	default:
		if input != nil {
			if data, err := json.Marshal(input); err == nil {
				candidate = string(data)
			}
		}
	}
	singleLine := strings.Join(strings.Fields(candidate), " ")
	runes := []rune(singleLine)
	if len(runes) > maxPreview {
		return string(runes[:maxPreview]) + "…"
	}
	return singleLine
}
