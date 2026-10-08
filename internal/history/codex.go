package history

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
)

// codexParseState is the line-level interpretation state for one Codex
// rollout JSONL file. It mirrors the audited Rust CodexSession semantics:
// response_item rows are the real transcript, event_msg user/agent messages
// are only a fallback view used when no real content exists.
type codexParseState struct {
	messages       []TranscriptMessage
	eventFallback  []TranscriptMessage
	toolIndex      map[string][2]int
	sawSessionMeta bool
	cwd            string
	gitBranch      *string
	model          *string
	source         *string
	tokensUsed     int64
	createdAt      int64
	updatedAt      int64
	unknownLines   uint32
}

// ParseCodexTranscript parses one provider-owned Codex rollout JSONL source.
// It is read-only and does not mutate or normalize the external file. The
// Codex state database (state_5.sqlite) is deliberately not consulted here;
// the rollout itself is the parsing authority.
func ParseCodexTranscript(reference SessionFileRef) (ParsedTranscript, error) {
	file, err := os.Open(reference.FilePath)
	if err != nil {
		return ParsedTranscript{}, fmt.Errorf("open Codex history: %w", err)
	}
	defer file.Close()

	state := newCodexParseState()
	reader := bufio.NewReaderSize(file, 1<<20)
	for {
		line, readErr := reader.ReadString('\n')
		if strings.TrimSpace(line) != "" {
			state.feedCodexLine(line)
		}
		if readErr != nil {
			if readErr != io.EOF {
				state.unknownLines++
			}
			break
		}
	}
	return state.codexTranscript(reference), nil
}

func newCodexParseState() *codexParseState {
	return &codexParseState{toolIndex: make(map[string][2]int)}
}

// codexFriendlySource maps the session_meta originator onto the display fact
// pinned by the Rust adapter.
func codexFriendlySource(originator string) *string {
	switch originator {
	case "codex_cli_rs", "codex-tui":
		value := "CLI"
		return &value
	case "codex_exec":
		value := "exec"
		return &value
	case "codex_vscode":
		value := "IDE extension"
		return &value
	case "codex_work_desktop":
		value := "Codex Desktop"
		return &value
	case "":
		return nil
	default:
		return &originator
	}
}

// rolloutNativeID extracts the stable native session id from a rollout file
// stem: "rollout-YYYY-MM-DDTHH-MM-SS-<uuid>.jsonl" yields <uuid>.
func rolloutNativeID(stem string) string {
	if rest, ok := strings.CutPrefix(stem, "rollout-"); ok {
		if len(rest) > 20 && rest[10] == 'T' {
			return rest[20:]
		}
	}
	return stem
}

func (s *codexParseState) pushCodex(message TranscriptMessage) {
	message.Seq = int64(len(s.messages))
	s.messages = append(s.messages, message)
}

func (s *codexParseState) pushFallback(message TranscriptMessage) {
	message.Seq = int64(len(s.eventFallback))
	s.eventFallback = append(s.eventFallback, message)
}

// codexFallbackActive mirrors the Rust fallback rule: the event view is
// presented only while no real text content exists.
func (s *codexParseState) codexFallbackActive() bool {
	for _, message := range s.messages {
		if message.Kind == MessageText && message.Text != "" {
			return false
		}
	}
	return len(s.eventFallback) > 0
}

func (s *codexParseState) codexMessages() []TranscriptMessage {
	if s.codexFallbackActive() {
		return s.eventFallback
	}
	return s.messages
}

func (s *codexParseState) codexTranscript(reference SessionFileRef) ParsedTranscript {
	messages := s.codexMessages()
	title := titleFromMessages(messages)
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
	for _, message := range messages {
		if message.Kind == MessageText {
			messageCount++
		}
	}
	return ParsedTranscript{
		Meta: SessionMeta{
			Key:          string(AgentCodex) + ":" + reference.NativeID,
			ID:           reference.NativeID,
			Agent:        AgentCodex,
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
			Source:       s.source,
		},
		Mainline:         append([]TranscriptMessage(nil), messages...),
		Sidechains:       []SidechainInfo{},
		UnknownLineCount: s.unknownLines,
	}
}

func (s *codexParseState) feedCodexLine(line string) {
	var row map[string]any
	if err := json.Unmarshal([]byte(line), &row); err != nil {
		s.unknownLines++
		return
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
	rowType, _ := row["type"].(string)
	payload, hasPayload := row["payload"]
	if !hasPayload {
		if rowType != "compacted" && rowType != "world_state" {
			s.unknownLines++
		}
		return
	}
	payloadObject, isObject := payload.(map[string]any)

	switch rowType {
	case "session_meta":
		if s.sawSessionMeta || !isObject {
			return
		}
		s.sawSessionMeta = true
		s.cwd, _ = payloadObject["cwd"].(string)
		originator, _ := payloadObject["originator"].(string)
		s.source = codexFriendlySource(originator)
		if git, ok := payloadObject["git"].(map[string]any); ok {
			if branch, ok := git["branch"].(string); ok && branch != "" {
				s.gitBranch = stringPointer(branch)
			}
		}
	case "turn_context":
		if !isObject {
			return
		}
		if s.cwd == "" {
			s.cwd, _ = payloadObject["cwd"].(string)
		}
		if model, ok := payloadObject["model"].(string); ok && model != "" {
			s.model = stringPointer(model)
		}
	case "response_item":
		if isObject {
			s.parseCodexResponseItem(payloadObject, timestamp)
		}
	case "event_msg":
		if !isObject {
			return
		}
		eventType, _ := payloadObject["type"].(string)
		switch eventType {
		case "token_count":
			if info, ok := payloadObject["info"].(map[string]any); ok {
				if usage, ok := info["total_token_usage"].(map[string]any); ok {
					if total, ok := usage["total_tokens"].(float64); ok {
						s.tokensUsed = int64(total)
					}
				}
			}
		case "user_message":
			if text, ok := payloadObject["message"].(string); ok && strings.TrimSpace(text) != "" {
				trimmed := strings.TrimSpace(text)
				s.pushFallback(TranscriptMessage{
					Role:      RoleUser,
					Kind:      userKind(trimmed),
					Text:      trimmed,
					Timestamp: optionalTimestamp(timestamp),
				})
			}
		case "agent_message":
			if text, ok := payloadObject["message"].(string); ok && strings.TrimSpace(text) != "" {
				trimmed := strings.TrimSpace(text)
				s.pushFallback(TranscriptMessage{
					Role:      RoleAssistant,
					Kind:      MessageText,
					Text:      trimmed,
					Timestamp: optionalTimestamp(timestamp),
				})
			}
		}
	case "compacted":
		s.pushCodex(TranscriptMessage{
			Role:      RoleSystem,
			Kind:      MessageCompactSummary,
			Text:      "── Context compacted ──",
			Timestamp: optionalTimestamp(timestamp),
		})
	case "world_state":
	default:
		s.unknownLines++
	}
}
