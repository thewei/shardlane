package herdr

import (
	"context"
	"encoding/json"
	"fmt"
)

// Launch transport over the live Herdr socket API. Every request shape,
// result discriminator, and field below was verified against the bundled
// protocol schema (`herdr api schema`, protocol 22, schema_version 1) —
// nothing here is invented:
//
//	agent.start  params {name, kind, pane_id, args?, timeout_ms? (3000..300000)}
//	             result  {type:"agent_started", agent: AgentInfo, argv}
//	agent.prompt params {target, text, wait?} result {type:"agent_prompted", agent}
//	agent.wait   params {target, until?: AgentStatus[], timeout_ms?} result {type:"wait_matched"}
//	agent.get    params {target} result {type:"agent_info", agent: AgentInfo}
//	tab.create   params {workspace_id?, cwd?, label?, focus? (default false)}
//	             result  {type:"tab_created", tab: TabInfo, root_pane: PaneInfo}
//	workspace.create params {cwd?, focus? (default false), label?, source_workspace_id?}
//	             result  {type:"workspace_created", workspace, tab, root_pane}
//	pane.process_info params {pane_id?} result {type:"pane_process_info", process_info}

// launchAgentInfo is the verified AgentInfo subset the launch transaction
// consumes.
type launchAgentInfo struct {
	PaneID        string  `json:"pane_id"`
	TabID         string  `json:"tab_id"`
	WorkspaceID   string  `json:"workspace_id"`
	AgentStatus   string  `json:"agent_status"`
	InteractiveRd bool    `json:"interactive_ready"`
	LaunchPending bool    `json:"launch_pending"`
	Revision      int64   `json:"revision"`
	Agent         *string `json:"agent"`
	AgentSession  *struct {
		Agent  string `json:"agent"`
		Kind   string `json:"kind"`
		Source string `json:"source"`
		Value  string `json:"value"`
	} `json:"agent_session"`
}

type launchTabRef struct {
	TabID string `json:"tab_id"`
}

type launchPaneRef struct {
	PaneID string `json:"pane_id"`
}

type launchWorkspaceRef struct {
	WorkspaceID string `json:"workspace_id"`
}

// decodeTypedResult enforces the discriminated result type before decoding
// the payload, so a mismatching server response fails loudly instead of
// decoding into the wrong shape.
func decodeTypedResult(raw json.RawMessage, wantType string, payload any) error {
	var discriminator struct {
		Type string `json:"type"`
	}
	if err := json.Unmarshal(raw, &discriminator); err != nil {
		return fmt.Errorf("decode %s result: %w", wantType, err)
	}
	if discriminator.Type != wantType {
		return fmt.Errorf("unexpected %s result type %q", wantType, discriminator.Type)
	}
	if payload == nil {
		return nil
	}
	return json.Unmarshal(raw, payload)
}

// CreateTabWithoutFocus creates one Tab in an existing workspace. The
// protocol default for `focus` is false and Shardlane never sends true:
// navigation stays client-local.
func (m *Manager) CreateTabWithoutFocus(ctx context.Context, session, workspaceID, cwd string) (tabID, paneID string, err error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return "", "", err
	}
	params := map[string]any{"workspace_id": workspaceID, "cwd": cwd, "focus": false}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "tab.create", params, true, &raw); err != nil {
		return "", "", err
	}
	var result struct {
		Tab      launchTabRef  `json:"tab"`
		RootPane launchPaneRef `json:"root_pane"`
	}
	if err := decodeTypedResult(raw, "tab_created", &result); err != nil {
		return "", "", err
	}
	return result.Tab.TabID, result.RootPane.PaneID, nil
}

// CreateWorkspaceWithoutFocus creates one runtime workspace plus its root
// tab/pane without moving global focus.
func (m *Manager) CreateWorkspaceWithoutFocus(ctx context.Context, session, cwd string) (workspaceID, tabID, paneID string, err error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return "", "", "", err
	}
	params := map[string]any{"cwd": cwd, "focus": false}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "workspace.create", params, true, &raw); err != nil {
		return "", "", "", err
	}
	var result struct {
		Workspace launchWorkspaceRef `json:"workspace"`
		Tab       launchTabRef       `json:"tab"`
		RootPane  launchPaneRef      `json:"root_pane"`
	}
	if err := decodeTypedResult(raw, "workspace_created", &result); err != nil {
		return "", "", "", err
	}
	return result.Workspace.WorkspaceID, result.Tab.TabID, result.RootPane.PaneID, nil
}

// AgentStartParams are the schema-verified agent.start parameters.
type AgentStartParams struct {
	Name      string
	Kind      string
	PaneID    string
	Args      []string
	TimeoutMS int64
}

// AgentStarted is the verified agent.start result.
type AgentStarted struct {
	AgentInfo launchAgentInfo
	Argv      []string
}

// StartAgent starts a supported interactive agent in an existing pane
// (agent.start). Executed exactly once by the launch transaction; an
// unwritable/unreadable exchange surfaces as DeliveryUncertainError, which
// the transaction reconciles instead of repeating.
func (m *Manager) StartAgent(ctx context.Context, session string, params AgentStartParams) (AgentStarted, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return AgentStarted{}, err
	}
	wire := map[string]any{
		"name":    params.Name,
		"kind":    params.Kind,
		"pane_id": params.PaneID,
		"args":    params.Args,
	}
	if params.TimeoutMS > 0 {
		wire["timeout_ms"] = params.TimeoutMS
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "agent.start", wire, true, &raw); err != nil {
		return AgentStarted{}, err
	}
	var result struct {
		Agent launchAgentInfo `json:"agent"`
		Argv  []string        `json:"argv"`
	}
	if err := decodeTypedResult(raw, "agent_started", &result); err != nil {
		return AgentStarted{}, err
	}
	return AgentStarted{AgentInfo: result.Agent, Argv: result.Argv}, nil
}

// PromptAgent submits exactly one semantic prompt to an agent target
// (agent.prompt {target, text}).
func (m *Manager) PromptAgent(ctx context.Context, session, target, text string) error {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return err
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "agent.prompt", map[string]any{"target": target, "text": text}, true, &raw); err != nil {
		return err
	}
	return decodeTypedResult(raw, "agent_prompted", nil)
}

// WaitAgentState blocks until the agent target reaches one of the requested
// Herdr statuses (agent.wait {target, until, timeout_ms}).
func (m *Manager) WaitAgentState(ctx context.Context, session, target string, until []string, timeoutMS int64) error {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return err
	}
	params := map[string]any{"target": target}
	if len(until) > 0 {
		params["until"] = until
	}
	if timeoutMS > 0 {
		params["timeout_ms"] = timeoutMS
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "agent.wait", params, false, &raw); err != nil {
		return err
	}
	return decodeTypedResult(raw, "wait_matched", nil)
}

// AgentByPane reads one agent through its pane (agent.get {target: pane_id}).
// A missing agent decodes to (nil, nil) — the launch transaction treats that
// as "not present yet" inside its bounded identity window.
func (m *Manager) AgentByPane(ctx context.Context, session, paneID string) (*AgentInfo, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return nil, err
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "agent.get", map[string]any{"target": paneID}, false, &raw); err != nil {
		return nil, err
	}
	var result struct {
		Agent launchAgentInfo `json:"agent"`
	}
	if err := decodeTypedResult(raw, "agent_info", &result); err != nil {
		return nil, err
	}
	info := AgentInfo{
		PaneID:           result.Agent.PaneID,
		TabID:            result.Agent.TabID,
		WorkspaceID:      result.Agent.WorkspaceID,
		AgentStatus:      result.Agent.AgentStatus,
		InteractiveReady: result.Agent.InteractiveRd,
		LaunchPending:    result.Agent.LaunchPending,
		Revision:         result.Agent.Revision,
		Agent:            result.Agent.Agent,
	}
	if result.Agent.AgentSession != nil {
		info.AgentSession = &AgentSessionIdentity{
			Agent:  result.Agent.AgentSession.Agent,
			Kind:   result.Agent.AgentSession.Kind,
			Source: result.Agent.AgentSession.Source,
			Value:  result.Agent.AgentSession.Value,
		}
	}
	return &info, nil
}

// ShellReady reports whether the pane's shell is interactive: a live shell
// process and no foreground process beyond it — the verified
// pane.process_info projection.
func (m *Manager) ShellReady(ctx context.Context, session, paneID string) (bool, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return false, err
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "pane.process_info", map[string]any{"pane_id": paneID}, false, &raw); err != nil {
		return false, err
	}
	var result struct {
		ProcessInfo struct {
			ShellPID            *int `json:"shell_pid"`
			ForegroundProcesses []struct {
				PID  uint32 `json:"pid"`
				Name string `json:"name"`
			} `json:"foreground_processes"`
		} `json:"process_info"`
	}
	if err := decodeTypedResult(raw, "pane_process_info", &result); err != nil {
		return false, err
	}
	return result.ProcessInfo.ShellPID != nil && len(result.ProcessInfo.ForegroundProcesses) == 0, nil
}

// SendAgentKeys sends verified named keys to an agent target
// (agent.send_keys {target, keys}) — the Plan-mode post-ready operation, a
// named-key operation on an exact Agent target, never raw terminal text.
func (m *Manager) SendAgentKeys(ctx context.Context, session, target string, keys []string) error {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return err
	}
	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "agent.send_keys", map[string]any{"target": target, "keys": keys}, true, &raw); err != nil {
		return err
	}
	return decodeTypedResult(raw, "ok", nil)
}

// AgentInfo is the exported verified subset for consumers above the adapter.
type AgentInfo struct {
	PaneID           string
	TabID            string
	WorkspaceID      string
	AgentStatus      string
	InteractiveReady bool
	LaunchPending    bool
	Revision         int64
	Agent            *string
	AgentSession     *AgentSessionIdentity
}

type AgentSessionIdentity struct {
	Agent  string
	Kind   string
	Source string
	Value  string
}

// callRPCWithContext mirrors callRPC with context cancellation support for
// the launch transaction's cancellable phases.
func callRPCWithContext(ctx context.Context, socketPath, method string, params any, mutating bool, result any) error {
	done := make(chan error, 1)
	go func() {
		done <- callRPC(socketPath, method, params, mutating, result)
	}()
	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		return fmt.Errorf("%s canceled: %w", method, ctx.Err())
	}
}
