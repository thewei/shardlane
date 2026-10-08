/**
 * [INPUT]: 依赖 history/models.go (TranscriptMessage, SessionFileRef), parse_utils.go (clipText, epochMS, toolInput, injectedUserContent)
 * [OUTPUT]: 对外提供 ParsePiTranscript, piParseState, newPiParseState
 * [POS]: history 包中 Pi / Oh My Pi 会话历史与 live append log 解析器，与 claude.go / codex.go 兄弟对称
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

package history

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
)

type piParseState struct {
	sessionID    string
	cwd          string
	createdAt    int64
	lastTS       int64
	model        *string
	tokensUsed   int64
	unknownLines uint32
	messages     []TranscriptMessage
	toolIndex    map[string][2]int // toolCallId -> [messageIndex, toolCallIndex]
}

func newPiParseState() *piParseState {
	return &piParseState{
		toolIndex: make(map[string][2]int),
	}
}

// ParsePiTranscript parses one provider-owned Pi / Oh My Pi JSONL source.
// It is read-only and does not mutate or normalize the external file.
func ParsePiTranscript(reference SessionFileRef) (ParsedTranscript, error) {
	file, err := os.Open(reference.FilePath)
	if err != nil {
		return ParsedTranscript{}, fmt.Errorf("open Pi history: %w", err)
	}
	defer file.Close()

	state := newPiParseState()
	reader := bufio.NewReaderSize(file, 1<<20)
	for {
		line, readErr := reader.ReadString('\n')
		if strings.TrimSpace(line) != "" {
			state.feedPiLine(line)
		}
		if readErr != nil {
			if readErr != io.EOF {
				state.unknownLines++
			}
			break
		}
	}
	return state.transcript(reference), nil
}

func (s *piParseState) transcript(reference SessionFileRef) ParsedTranscript {
	title := titleFromMessages(s.messages)
	if title == "" {
		title = Untitled
	}
	created := s.createdAt
	if created == 0 {
		created = reference.MtimeMS
	}
	updated := s.lastTS
	if updated == 0 {
		updated = reference.MtimeMS
	}
	var tokensUsed *int64
	if s.tokensUsed > 0 {
		val := s.tokensUsed
		tokensUsed = &val
	}
	messageCount := int64(0)
	for _, msg := range s.messages {
		if msg.Kind == MessageText {
			messageCount++
		}
	}
	return ParsedTranscript{
		Meta: SessionMeta{
			Key:          string(reference.Agent) + ":" + reference.NativeID,
			ID:           reference.NativeID,
			Agent:        reference.Agent,
			Title:        title,
			ProjectPath:  s.cwd,
			ProjectName:  projectName(s.cwd),
			FilePath:     reference.FilePath,
			CreatedAt:    created,
			UpdatedAt:    updated,
			MessageCount: messageCount,
			SizeBytes:    reference.SizeBytes,
			Model:        s.model,
			TokensUsed:   tokensUsed,
		},
		Mainline:         append([]TranscriptMessage(nil), s.messages...),
		Sidechains:       []SidechainInfo{},
		UnknownLineCount: s.unknownLines,
	}
}

func (s *piParseState) feedPiLine(line string) {
	line = strings.TrimSpace(line)
	if line == "" {
		return
	}
	var row map[string]any
	if err := json.Unmarshal([]byte(line), &row); err != nil {
		s.unknownLines++
		return
	}
	rowType, _ := row["type"].(string)
	switch rowType {
	case "session":
		if id, ok := row["id"].(string); ok && id != "" {
			s.sessionID = id
		}
		if cwd, ok := row["cwd"].(string); ok && cwd != "" {
			s.cwd = cwd
		}
		if ts := epochMS(row["timestamp"]); ts > 0 {
			s.createdAt = ts
		}
	case "message":
		messageObj, ok := row["message"].(map[string]any)
		if !ok {
			s.unknownLines++
			return
		}
		timestamp := epochMS(row["timestamp"])
		if timestamp == 0 {
			timestamp = epochMS(messageObj["timestamp"])
		}
		if timestamp > s.lastTS {
			s.lastTS = timestamp
		}
		role, _ := messageObj["role"].(string)
		content := messageObj["content"]
		switch role {
		case "user":
			text := piBlocksText(content)
			if strings.TrimSpace(text) != "" {
				s.pushUser(text, timestamp)
			}
		case "assistant":
			text := piBlocksText(content)
			thinking := piBlocksThinking(content)
			tools := piBlocksTools(content)
			if text == "" && thinking == nil && len(tools) == 0 {
				return
			}
			if modelStr, ok := messageObj["model"].(string); ok && modelStr != "" {
				s.model = &modelStr
			}
			if usage, ok := messageObj["usage"].(map[string]any); ok {
				if totalTokens, ok := usage["totalTokens"].(float64); ok && totalTokens > 0 {
					s.tokensUsed = int64(totalTokens)
				}
			}
			// Consecutive assistant turns (interrupted only by toolResult) merge
			// into one, matching Rust herdr-history PiSession::feed_line.
			if len(s.messages) == 0 || s.messages[len(s.messages)-1].Role != RoleAssistant {
				s.pushAssistant(timestamp)
			}
			lastIdx := len(s.messages) - 1
			last := &s.messages[lastIdx]
			if text != "" {
				if last.Text != "" {
					last.Text += "\n\n" + text
				} else {
					last.Text = text
				}
				if len(last.Text) > MaxMessageText {
					clipped, truncated := clipText(last.Text, MaxMessageText)
					last.Text = clipped
					last.Truncated = truncated
				}
			}
			if thinking != nil {
				if last.Thinking == nil {
					last.Thinking = thinking
				} else {
					combined := *last.Thinking + "\n\n" + *thinking
					clipped, _ := clipText(combined, MaxToolIO)
					last.Thinking = &clipped
				}
			}
			if s.model != nil {
				last.Model = s.model
			}
			for _, tool := range tools {
				if tool.ID != "" {
					s.toolIndex[tool.ID] = [2]int{lastIdx, len(last.ToolCalls)}
				}
				last.ToolCalls = append(last.ToolCalls, tool)
			}
		case "toolResult":
			callID, _ := messageObj["toolCallId"].(string)
			if callID == "" {
				return
			}
			pos, found := s.toolIndex[callID]
			if !found || pos[0] >= len(s.messages) || pos[1] >= len(s.messages[pos[0]].ToolCalls) {
				return
			}
			toolCall := &s.messages[pos[0]].ToolCalls[pos[1]]
			text := piToolResultText(content)
			if text != "" {
				clipped, _ := clipText(text, MaxToolIO)
				toolCall.Output = &clipped
			}
			if isErr, ok := messageObj["isError"].(bool); ok && isErr {
				toolCall.IsError = true
			}
		default:
			s.unknownLines++
		}
	case "model_change", "thinking_level_change", "custom":
		// Known non-content events skipped silently.
	default:
		s.unknownLines++
	}
}

func (s *piParseState) pushUser(text string, timestamp int64) {
	kind := MessageText
	if injectedUserContent(text) {
		kind = MessageMeta
	}
	clipped, truncated := clipText(text, MaxMessageText)
	s.messages = append(s.messages, TranscriptMessage{
		Seq:       int64(len(s.messages)),
		Role:      RoleUser,
		Kind:      kind,
		Text:      clipped,
		Truncated: truncated,
		ToolCalls: []ToolCall{},
		Timestamp: optionalTimestamp(timestamp),
	})
}

func (s *piParseState) pushAssistant(timestamp int64) {
	s.messages = append(s.messages, TranscriptMessage{
		Seq:       int64(len(s.messages)),
		Role:      RoleAssistant,
		Kind:      MessageText,
		ToolCalls: []ToolCall{},
		Timestamp: optionalTimestamp(timestamp),
	})
}

func piBlocksText(content any) string {
	switch v := content.(type) {
	case string:
		return strings.TrimSpace(v)
	case []any:
		var parts []string
		for _, item := range v {
			block, ok := item.(map[string]any)
			if !ok {
				continue
			}
			if block["type"] == "text" {
				if text, ok := block["text"].(string); ok {
					trimmed := strings.TrimSpace(text)
					if trimmed != "" {
						parts = append(parts, trimmed)
					}
				}
			}
		}
		return strings.Join(parts, "\n\n")
	default:
		return ""
	}
}

func piBlocksThinking(content any) *string {
	blocks, ok := content.([]any)
	if !ok {
		return nil
	}
	var parts []string
	for _, item := range blocks {
		block, ok := item.(map[string]any)
		if !ok {
			continue
		}
		if block["type"] == "thinking" {
			if thinking, ok := block["thinking"].(string); ok {
				trimmed := strings.TrimSpace(thinking)
				if trimmed != "" {
					parts = append(parts, trimmed)
				}
			}
		}
	}
	if len(parts) == 0 {
		return nil
	}
	combined := strings.Join(parts, "\n\n")
	clipped, _ := clipText(combined, MaxToolIO)
	return &clipped
}

func piBlocksTools(content any) []ToolCall {
	blocks, ok := content.([]any)
	if !ok {
		return nil
	}
	var tools []ToolCall
	for _, item := range blocks {
		block, ok := item.(map[string]any)
		if !ok {
			continue
		}
		if block["type"] == "toolCall" {
			id, _ := block["id"].(string)
			name, _ := block["name"].(string)
			if name == "" {
				name = "tool"
			}
			preview, full := toolInput(block["arguments"])
			tools = append(tools, ToolCall{
				ID:           id,
				Name:         name,
				Input:        full,
				InputPreview: preview,
			})
		}
	}
	return tools
}

func piToolResultText(content any) string {
	switch v := content.(type) {
	case string:
		return strings.TrimSpace(v)
	case []any:
		var parts []string
		for _, item := range v {
			if str, ok := item.(string); ok {
				trimmed := strings.TrimSpace(str)
				if trimmed != "" {
					parts = append(parts, trimmed)
				}
				continue
			}
			block, ok := item.(map[string]any)
			if !ok {
				continue
			}
			if text, ok := block["text"].(string); ok {
				trimmed := strings.TrimSpace(text)
				if trimmed != "" {
					parts = append(parts, trimmed)
				}
			} else if result, ok := block["result"].(string); ok {
				trimmed := strings.TrimSpace(result)
				if trimmed != "" {
					parts = append(parts, trimmed)
				}
			} else if output, ok := block["output"].(string); ok {
				trimmed := strings.TrimSpace(output)
				if trimmed != "" {
					parts = append(parts, trimmed)
				}
			}
		}
		return strings.Join(parts, "\n\n")
	case map[string]any:
		if text, ok := v["text"].(string); ok {
			return strings.TrimSpace(text)
		}
		data, err := json.Marshal(v)
		if err == nil {
			return string(data)
		}
	}
	return ""
}

// piRolloutNativeID extracts the stable UUID from a Pi session file stem:
// `<timestamp>_<uuid>` → `<uuid>`, falling back to the stem if no `_` exists.
func piRolloutNativeID(stem string) string {
	if idx := strings.LastIndexByte(stem, '_'); idx >= 0 {
		return stem[idx+1:]
	}
	return stem
}
