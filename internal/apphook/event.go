package apphook

import (
	"time"
)

/**
 * [INPUT]: 无外部依赖，接收原生 Go 类型与时间规范
 * [OUTPUT]: 对外提供 StatusEvent、SessionEvent 结构体与 Handler 回调模型
 * [POS]: internal/apphook 的核心事件模型契约，由 server.go 解析生成，由 nativeui 消费
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// StatusEvent carries one reported agent status transition.
type StatusEvent struct {
	PaneID      string    `json:"pane_id"`
	WorkspaceID string    `json:"workspace_id,omitempty"`
	Agent       string    `json:"agent,omitempty"`
	Status      string    `json:"status"` // "working", "blocked", "idle", "done", "released", "launch_pending"
	SessionID   string    `json:"session_id,omitempty"`
	CWD         string    `json:"cwd,omitempty"`
	RawEvent    string    `json:"event,omitempty"`
	Text        string    `json:"text,omitempty"`
	Timestamp   time.Time `json:"timestamp"`
}

// SessionEvent carries one reported agent session identity binding.
type SessionEvent struct {
	PaneID             string    `json:"pane_id"`
	Agent              string    `json:"agent"`
	AgentSessionID     string    `json:"agent_session_id"`
	AgentSessionPath   string    `json:"agent_session_path,omitempty"`
	Source             string    `json:"source,omitempty"`
	SessionStartSource string    `json:"session_start_source,omitempty"`
	Seq                int64     `json:"seq,omitempty"`
	Timestamp          time.Time `json:"timestamp"`
}

// Handler contains event callbacks dispatched by the hook server.
type Handler struct {
	OnStatus  func(StatusEvent)
	OnSession func(SessionEvent)
}
