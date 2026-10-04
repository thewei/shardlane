package history

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
)

var claudeKnownSkipTypes = map[string]bool{
	"queue-operation":       true,
	"mode":                  true,
	"last-prompt":           true,
	"permission-mode":       true,
	"file-history-snapshot": true,
	"file-history-delta":    true,
	"pr-link":               true,
	"frame-link":            true,
	"attachment":            true,
	"summary":               true,
}

type claudePending struct {
	msgID     string
	text      []string
	thinking  []string
	tools     []ToolCall
	timestamp *int64
	model     *string
}

type claudeParseState struct {
	messages      []TranscriptMessage
	customTitle   string
	fallbackTitle string
	cwd           string
	gitBranch     *string
	model         *string
	tokensUsed    int64
	createdAt     int64
	updatedAt     int64
	unknownLines  uint32
	pending       *claudePending
	toolIndex     map[string][2]int
}

func newClaudeParseState() *claudeParseState {
	return &claudeParseState{toolIndex: make(map[string][2]int)}
}

// ParseClaudeTranscript parses one provider-owned Claude Code JSONL source.
// It is read-only and does not mutate or normalize the external file.
func ParseClaudeTranscript(reference SessionFileRef) (ParsedTranscript, error) {
	file, err := os.Open(reference.FilePath)
	if err != nil {
		return ParsedTranscript{}, fmt.Errorf("open Claude history: %w", err)
	}
	defer file.Close()

	state := newClaudeParseState()
	reader := bufio.NewReaderSize(file, 1<<20)
	for {
		line, readErr := reader.ReadString('\n')
		if strings.TrimSpace(line) != "" {
			state.feedClaudeLine(line)
		}
		if readErr != nil {
			if readErr != io.EOF {
				state.unknownLines++
			}
			break
		}
	}
	state.flushPending()
	return state.transcript(reference), nil
}

// materializeClaudePending converts one accumulated assistant turn into a
// TranscriptMessage; ok is false when nothing user-visible accumulated.
func materializeClaudePending(pending *claudePending) (TranscriptMessage, bool) {
	if len(pending.text) == 0 && len(pending.thinking) == 0 && len(pending.tools) == 0 {
		return TranscriptMessage{}, false
	}
	text, truncated := clipText(strings.Join(pending.text, "\n\n"), MaxMessageText)
	var thinking *string
	if len(pending.thinking) > 0 {
		value, _ := clipText(strings.Join(pending.thinking, "\n\n"), MaxToolIO)
		thinking = &value
	}
	message := TranscriptMessage{
		Role:      RoleAssistant,
		Kind:      MessageText,
		Text:      text,
		Truncated: truncated,
		ToolCalls: append([]ToolCall(nil), pending.tools...),
		Thinking:  thinking,
		Timestamp: pending.timestamp,
		Model:     pending.model,
	}
	return message, true
}

// pendingTail materializes the unflushed assistant accumulation without
// mutating parse state, so the live projection can render the provisional
// streaming tail before the next role/msgID boundary commits it (0.6 §7.3).
func (s *claudeParseState) pendingTail() *TranscriptMessage {
	if s.pending == nil {
		return nil
	}
	message, ok := materializeClaudePending(s.pending)
	if !ok {
		return nil
	}
	message.Seq = int64(len(s.messages))
	return &message
}

func (s *claudeParseState) push(message TranscriptMessage) {
	message.Seq = int64(len(s.messages))
	s.messages = append(s.messages, message)
}

func (s *claudeParseState) flushPending() {
	if s.pending == nil {
		return
	}
	pending := s.pending
	s.pending = nil
	message, ok := materializeClaudePending(pending)
	if !ok {
		return
	}
	messageIndex := len(s.messages)
	for toolIndex, tool := range message.ToolCalls {
		if tool.ID != "" {
			s.toolIndex[tool.ID] = [2]int{messageIndex, toolIndex}
		}
	}
	s.push(message)
}

func (s *claudeParseState) transcript(reference SessionFileRef) ParsedTranscript {
	title := s.customTitle
	if title == "" {
		title = s.fallbackTitle
	}
	if title == "" {
		title = Untitled
	}
	created := s.createdAt
	if created == 0 {
		created = reference.MtimeMS
	}
	updated := s.updatedAt
	if updated == 0 {
		updated = reference.MtimeMS
	}
	var tokensUsed *int64
	if s.tokensUsed > 0 {
		value := s.tokensUsed
		tokensUsed = &value
	}
	messageCount := int64(0)
	for _, message := range s.messages {
		if message.Kind == MessageText {
			messageCount++
		}
	}
	return ParsedTranscript{
		Meta: SessionMeta{
			Key:          string(AgentClaudeCode) + ":" + reference.NativeID,
			ID:           reference.NativeID,
			Agent:        AgentClaudeCode,
			Title:        title,
			ProjectPath:  s.cwd,
			ProjectName:  projectName(s.cwd),
			FilePath:     reference.FilePath,
			CreatedAt:    created,
			UpdatedAt:    updated,
			MessageCount: messageCount,
			SizeBytes:    reference.SizeBytes,
			GitBranch:    s.gitBranch,
			Model:        s.model,
			TokensUsed:   tokensUsed,
		},
		Mainline:         append([]TranscriptMessage(nil), s.messages...),
		Sidechains:       []SidechainInfo{},
		UnknownLineCount: s.unknownLines,
	}
}

func (s *claudeParseState) feedClaudeLine(line string) {
	var row map[string]any
	if err := json.Unmarshal([]byte(line), &row); err != nil {
		s.unknownLines++
		return
	}
	rowType, _ := row["type"].(string)
	if rowType == "custom-title" {
		if title, _ := row["customTitle"].(string); strings.TrimSpace(title) != "" {
			s.customTitle = strings.TrimSpace(title)
		}
		return
	}
	if rowType != "user" && rowType != "assistant" && rowType != "system" {
		if rowType == "" || !claudeKnownSkipTypes[rowType] {
			s.unknownLines++
		}
		return
	}

	if s.cwd == "" {
		s.cwd, _ = row["cwd"].(string)
	}
	if branch, _ := row["gitBranch"].(string); branch != "" {
		s.gitBranch = stringPointer(branch)
	}
	timestamp := epochMS(row["timestamp"])
	if timestamp > 0 {
		if s.createdAt == 0 {
			s.createdAt = timestamp
		}
		if timestamp > s.updatedAt {
			s.updatedAt = timestamp
		}
	}
	if sidechain, _ := row["isSidechain"].(bool); sidechain {
		return
	}

	switch rowType {
	case "system":
		s.feedClaudeSystem(row, timestamp)
	case "user":
		s.feedClaudeUser(row, timestamp)
	case "assistant":
		s.feedClaudeAssistant(row, timestamp)
	}
}

func stringPointer(value string) *string {
	return &value
}
