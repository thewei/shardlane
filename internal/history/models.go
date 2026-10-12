/**
 * [INPUT]: 依赖 Go 标准库 strings 与 encoding/json
 * [OUTPUT]: 对外提供 AgentID 枚举、AllAgents 清单、ParseAgentID 别名归一化与 DisplayName 呈现、会话元数据/消息模型
 * [POS]: internal/history 的核心数据契约，被整个客户端的 Agent 投影、会话目录与 UI 视觉层消费
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Package history owns Shardlane's read-only coding-agent history domain.
// It never mutates provider-owned history stores and never owns runtime state.
package history

import "strings"

const (
	MaxMessageText = 32 * 1024
	MaxToolIO      = 16 * 1024
	MaxTitle       = 80
	Untitled       = "Untitled"
)

type AgentID string

const (
	AgentClaudeCode  AgentID = "claude-code"
	AgentCodex       AgentID = "codex"
	AgentCopilot     AgentID = "copilot"
	AgentCursor      AgentID = "cursor"
	AgentOpenCode    AgentID = "opencode"
	AgentCommandCode AgentID = "command-code"
	AgentKiro        AgentID = "kiro"
	AgentGemini      AgentID = "gemini"
	AgentPi          AgentID = "pi"
	AgentOMP         AgentID = "omp"
	AgentGrok        AgentID = "grok"
	AgentKimi        AgentID = "kimi"
	AgentAntigravity AgentID = "antigravity"
	AgentDSH         AgentID = "dsh"
	AgentQoder       AgentID = "qoder"
)

var AllAgents = []AgentID{
	AgentClaudeCode,
	AgentCodex,
	AgentCopilot,
	AgentCursor,
	AgentOpenCode,
	AgentCommandCode,
	AgentKiro,
	AgentGemini,
	AgentPi,
	AgentOMP,
	AgentGrok,
	AgentKimi,
	AgentAntigravity,
	AgentDSH,
	AgentQoder,
}

func ParseAgentID(value string) (AgentID, bool) {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "claude", "claude-code", "claude_code":
		return AgentClaudeCode, true
	case "codex", "codex-cli":
		return AgentCodex, true
	case "copilot":
		return AgentCopilot, true
	case "cursor":
		return AgentCursor, true
	case "opencode":
		return AgentOpenCode, true
	case "command-code", "commandcode":
		return AgentCommandCode, true
	case "kiro":
		return AgentKiro, true
	case "gemini":
		return AgentGemini, true
	case "pi", "pi-coding-agent":
		return AgentPi, true
	case "omp", "oh-my-pi":
		return AgentOMP, true
	case "grok":
		return AgentGrok, true
	case "kimi":
		return AgentKimi, true
	case "antigravity", "agy", "antigravity-cli":
		return AgentAntigravity, true
	case "dsh":
		return AgentDSH, true
	case "qoder", "qoder-cli":
		return AgentQoder, true
	default:
		candidate := AgentID(value)
		for _, agent := range AllAgents {
			if candidate == agent {
				return candidate, true
			}
		}
		return "", false
	}
}

func (a AgentID) DisplayName() string {
	switch a {
	case AgentClaudeCode:
		return "Claude Code"
	case AgentCodex:
		return "Codex"
	case AgentCopilot:
		return "Copilot CLI"
	case AgentCursor:
		return "Cursor"
	case AgentOpenCode:
		return "OpenCode"
	case AgentCommandCode:
		return "Command Code"
	case AgentKiro:
		return "Kiro"
	case AgentGemini:
		return "Gemini CLI"
	case AgentPi:
		return "Pi"
	case AgentOMP:
		return "Oh My Pi"
	case AgentGrok:
		return "Grok Build"
	case AgentKimi:
		return "Kimi Code"
	case AgentAntigravity:
		return "Antigravity CLI"
	case AgentDSH:
		return "DeepSeek Harness"
	case AgentQoder:
		return "Qoder"
	default:
		return string(a)
	}
}

type SessionFileRef struct {
	Agent     AgentID `json:"agent"`
	NativeID  string  `json:"native_id"`
	FilePath  string  `json:"file_path"`
	MtimeMS   int64   `json:"mtime_ms"`
	SizeBytes int64   `json:"size"`
}

type SessionMeta struct {
	Key          string  `json:"key"`
	ID           string  `json:"id"`
	Agent        AgentID `json:"agent"`
	Title        string  `json:"title"`
	ProjectPath  string  `json:"project_path"`
	ProjectName  string  `json:"project_name"`
	FilePath     string  `json:"file_path"`
	CreatedAt    int64   `json:"created_at"`
	UpdatedAt    int64   `json:"updated_at"`
	MessageCount int64   `json:"message_count"`
	SizeBytes    int64   `json:"size_bytes"`
	GitBranch    *string `json:"git_branch,omitempty"`
	Model        *string `json:"model,omitempty"`
	TokensUsed   *int64  `json:"tokens_used,omitempty"`
	Archived     bool    `json:"archived"`
	Source       *string `json:"source,omitempty"`
	// MtimeMS is the provider source file's last-modified time (0 when the
	// catalog row predates the column); the usage aggregator's mtime
	// invalidation reads it.
	MtimeMS int64 `json:"mtime_ms"`
}

type SessionSummary struct {
	Meta        SessionMeta `json:"meta"`
	Description string      `json:"description"`
}

type ToolCall struct {
	ID           string  `json:"id"`
	Name         string  `json:"name"`
	InputPreview string  `json:"input_preview"`
	Input        *string `json:"input,omitempty"`
	Output       *string `json:"output,omitempty"`
	IsError      bool    `json:"is_error"`
	SidechainRef *string `json:"sidechain_ref,omitempty"`
}

type MessageKind string

const (
	MessageText           MessageKind = "text"
	MessageMeta           MessageKind = "meta"
	MessageCompactSummary MessageKind = "compact-summary"
)

type Role string

const (
	RoleUser      Role = "user"
	RoleAssistant Role = "assistant"
	RoleSystem    Role = "system"
)

type TranscriptMessage struct {
	Seq       int64       `json:"seq"`
	Role      Role        `json:"role"`
	Kind      MessageKind `json:"kind"`
	Text      string      `json:"text"`
	Truncated bool        `json:"truncated"`
	ToolCalls []ToolCall  `json:"tool_calls"`
	Thinking  *string     `json:"thinking,omitempty"`
	Timestamp *int64      `json:"timestamp,omitempty"`
	Model     *string     `json:"model,omitempty"`
	// ContextTokens is the provider-recorded size of the request context at
	// this message (input + cache + output of the last usage report), when
	// the transcript states one. Presentation (context ring) only.
	ContextTokens *int64 `json:"context_tokens,omitempty"`
}

type SidechainInfo struct {
	ID          string  `json:"id"`
	AgentType   *string `json:"agent_type,omitempty"`
	Description *string `json:"description,omitempty"`
	ToolUseID   *string `json:"tool_use_id,omitempty"`
}

type ParsedTranscript struct {
	Meta             SessionMeta         `json:"meta"`
	Mainline         []TranscriptMessage `json:"mainline"`
	Sidechains       []SidechainInfo     `json:"sidechains"`
	UnknownLineCount uint32              `json:"unknown_line_count"`
}
