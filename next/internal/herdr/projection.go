package herdr

import (
	"fmt"
	"strings"
)

// Project is Shardlane's projection of one Herdr runtime workspace inside the
// currently selected top-level Herdr instance.
type Project struct {
	ID          string `json:"id"`
	Label       string `json:"label"`
	Number      uint   `json:"number"`
	Focused     bool   `json:"focused"`
	PaneCount   uint   `json:"pane_count"`
	TabCount    uint   `json:"tab_count"`
	ActiveTabID string `json:"active_tab_id"`
	AgentStatus string `json:"agent_status"`
	CWD         string `json:"cwd,omitempty"`
}

type Tab struct {
	ID          string `json:"id"`
	ProjectID   string `json:"project_id"`
	Label       string `json:"label"`
	Number      uint   `json:"number"`
	Focused     bool   `json:"focused"`
	PaneCount   uint   `json:"pane_count"`
	AgentStatus string `json:"agent_status"`
}

// PaneScroll is Herdr's authoritative scroll viewport for one Pane: the
// viewport sits OffsetFromBottom rows above the bottom of Herdr-owned
// scrollback, which is at most MaxOffsetFromBottom rows deep.
type PaneScroll struct {
	OffsetFromBottom    uint64 `json:"offset_from_bottom"`
	MaxOffsetFromBottom uint64 `json:"max_offset_from_bottom"`
	ViewportRows        uint64 `json:"viewport_rows"`
}

type Pane struct {
	ID          string `json:"id"`
	TerminalID  string `json:"terminal_id"`
	ProjectID   string `json:"project_id"`
	TabID       string `json:"tab_id"`
	Label       string `json:"label"`
	Focused     bool   `json:"focused"`
	Agent       string `json:"agent,omitempty"`
	AgentStatus string `json:"agent_status"`
	CWD         string `json:"cwd,omitempty"`
	Revision    uint64 `json:"revision"`
	// Scroll is the pane's scroll viewport from the snapshot; nil when the
	// runtime has not reported one yet.
	Scroll *PaneScroll `json:"scroll,omitempty"`
}

type LayoutRect struct {
	X      uint16 `json:"x"`
	Y      uint16 `json:"y"`
	Width  uint16 `json:"width"`
	Height uint16 `json:"height"`
}

type LayoutPane struct {
	PaneID  string     `json:"pane_id"`
	Focused bool       `json:"focused"`
	Rect    LayoutRect `json:"rect"`
}

type Layout struct {
	ProjectID     string       `json:"project_id"`
	TabID         string       `json:"tab_id"`
	Zoomed        bool         `json:"zoomed"`
	Area          LayoutRect   `json:"area"`
	FocusedPaneID string       `json:"focused_pane_id"`
	Panes         []LayoutPane `json:"panes"`
}

type Agent struct {
	TerminalID string `json:"terminal_id"`
	ProjectID  string `json:"project_id"`
	TabID      string `json:"tab_id"`
	PaneID     string `json:"pane_id"`
	Name       string `json:"name,omitempty"`
	Kind       string `json:"kind,omitempty"`
	Status     string `json:"status"`
	Focused    bool   `json:"focused"`
	CWD        string `json:"cwd,omitempty"`
	Revision   uint64 `json:"revision"`
	// AgentSession is the measured provider session identity from the live
	// schema's AgentInfo.agent_session; nil when the agent has none yet.
	AgentSession *AgentSessionIdentity `json:"agent_session,omitempty"`
}

// Projection is the frontend-safe shell projection of a single Herdr
// instance. It is disposable and can always be rebuilt from session.snapshot.
type Projection struct {
	Version          string    `json:"version"`
	Protocol         uint32    `json:"protocol"`
	FocusedProjectID string    `json:"focused_project_id,omitempty"`
	FocusedTabID     string    `json:"focused_tab_id,omitempty"`
	FocusedPaneID    string    `json:"focused_pane_id,omitempty"`
	Projects         []Project `json:"projects"`
	Tabs             []Tab     `json:"tabs"`
	Panes            []Pane    `json:"panes"`
	Layouts          []Layout  `json:"layouts"`
	Agents           []Agent   `json:"agents"`
}

// FocusedTerminalID returns the Herdr terminal owned by the currently focused
// Pane. Empty means the runtime has no attachable selected Pane yet.
func (p Projection) FocusedTerminalID() string {
	for _, pane := range p.Panes {
		if pane.ID == p.FocusedPaneID {
			return pane.TerminalID
		}
	}
	return ""
}

// FocusedLayout returns Herdr's authoritative layout for the selected Tab.
func (p Projection) FocusedLayout() (Layout, bool) {
	for _, layout := range p.Layouts {
		if layout.TabID == p.FocusedTabID {
			return layout, true
		}
	}
	return Layout{}, false
}

type sessionSnapshotResult struct {
	Type     string          `json:"type"`
	Snapshot sessionSnapshot `json:"snapshot"`
}

type sessionSnapshot struct {
	Version            string          `json:"version"`
	Protocol           uint32          `json:"protocol"`
	FocusedWorkspaceID *string         `json:"focused_workspace_id"`
	FocusedTabID       *string         `json:"focused_tab_id"`
	FocusedPaneID      *string         `json:"focused_pane_id"`
	Workspaces         []workspaceInfo `json:"workspaces"`
	Tabs               []tabInfo       `json:"tabs"`
	Panes              []paneInfo      `json:"panes"`
	Layouts            []layoutInfo    `json:"layouts"`
	Agents             []agentInfo     `json:"agents"`
}

type workspaceInfo struct {
	ID          string `json:"workspace_id"`
	Number      uint   `json:"number"`
	Label       string `json:"label"`
	Focused     bool   `json:"focused"`
	PaneCount   uint   `json:"pane_count"`
	TabCount    uint   `json:"tab_count"`
	ActiveTabID string `json:"active_tab_id"`
	AgentStatus string `json:"agent_status"`
}

type tabInfo struct {
	ID          string `json:"tab_id"`
	WorkspaceID string `json:"workspace_id"`
	Number      uint   `json:"number"`
	Label       string `json:"label"`
	Focused     bool   `json:"focused"`
	PaneCount   uint   `json:"pane_count"`
	AgentStatus string `json:"agent_status"`
}

type paneInfo struct {
	ID                    string      `json:"pane_id"`
	TerminalID            string      `json:"terminal_id"`
	WorkspaceID           string      `json:"workspace_id"`
	TabID                 string      `json:"tab_id"`
	Label                 *string     `json:"label"`
	Title                 *string     `json:"title"`
	TerminalTitleStripped *string     `json:"terminal_title_stripped"`
	Focused               bool        `json:"focused"`
	Agent                 *string     `json:"agent"`
	DisplayAgent          *string     `json:"display_agent"`
	AgentStatus           string      `json:"agent_status"`
	CWD                   *string     `json:"cwd"`
	ForegroundCWD         *string     `json:"foreground_cwd"`
	Revision              uint64      `json:"revision"`
	Scroll                *PaneScroll `json:"scroll"`
}

type layoutInfo struct {
	WorkspaceID   string           `json:"workspace_id"`
	TabID         string           `json:"tab_id"`
	Zoomed        bool             `json:"zoomed"`
	Area          LayoutRect       `json:"area"`
	FocusedPaneID string           `json:"focused_pane_id"`
	Panes         []layoutPaneInfo `json:"panes"`
}

type layoutPaneInfo struct {
	PaneID  string     `json:"pane_id"`
	Focused bool       `json:"focused"`
	Rect    LayoutRect `json:"rect"`
}

type agentInfo struct {
	TerminalID    string  `json:"terminal_id"`
	WorkspaceID   string  `json:"workspace_id"`
	TabID         string  `json:"tab_id"`
	PaneID        string  `json:"pane_id"`
	Name          *string `json:"name"`
	Agent         *string `json:"agent"`
	DisplayAgent  *string `json:"display_agent"`
	AgentStatus   string  `json:"agent_status"`
	Focused       bool    `json:"focused"`
	CWD           *string `json:"cwd"`
	ForegroundCWD *string `json:"foreground_cwd"`
	Revision      uint64  `json:"revision"`
	AgentSession  *struct {
		Agent  string `json:"agent"`
		Kind   string `json:"kind"`
		Source string `json:"source"`
		Value  string `json:"value"`
	} `json:"agent_session"`
}

func (m *Manager) Projection(session string) (Projection, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return Projection{}, err
	}
	return m.projectionReadyAt(socket)
}

func (m *Manager) projectionReadyAt(socket string) (Projection, error) {
	var result sessionSnapshotResult
	if err := callRPC(socket, "session.snapshot", map[string]any{}, false, &result); err != nil {
		return Projection{}, err
	}
	if result.Type != "session_snapshot" {
		return Projection{}, fmt.Errorf("Herdr session.snapshot returned %q", result.Type)
	}
	if result.Snapshot.Protocol < minSupportedProtocol {
		return Projection{}, fmt.Errorf("incompatible Herdr protocol: need %d+, got %d", minSupportedProtocol, result.Snapshot.Protocol)
	}
	return projectSnapshot(result.Snapshot), nil
}

func projectSnapshot(snapshot sessionSnapshot) Projection {
	projection := Projection{
		Version:  snapshot.Version,
		Protocol: snapshot.Protocol,
		Projects: make([]Project, 0, len(snapshot.Workspaces)),
		Tabs:     make([]Tab, 0, len(snapshot.Tabs)),
		Panes:    make([]Pane, 0, len(snapshot.Panes)),
		Layouts:  make([]Layout, 0, len(snapshot.Layouts)),
		Agents:   make([]Agent, 0, len(snapshot.Agents)),
	}
	if snapshot.FocusedWorkspaceID != nil {
		projection.FocusedProjectID = *snapshot.FocusedWorkspaceID
	}
	if snapshot.FocusedTabID != nil {
		projection.FocusedTabID = *snapshot.FocusedTabID
	}
	if snapshot.FocusedPaneID != nil {
		projection.FocusedPaneID = *snapshot.FocusedPaneID
	}

	projectCWD := make(map[string]string)
	for _, pane := range snapshot.Panes {
		cwd := firstString(pane.ForegroundCWD, pane.CWD)
		if cwd != "" && projectCWD[pane.WorkspaceID] == "" {
			projectCWD[pane.WorkspaceID] = cwd
		}
		projection.Panes = append(projection.Panes, Pane{
			ID:          pane.ID,
			TerminalID:  pane.TerminalID,
			ProjectID:   pane.WorkspaceID,
			TabID:       pane.TabID,
			Label:       firstString(pane.Label, pane.Title, pane.TerminalTitleStripped, stringPtr(pane.ID)),
			Focused:     pane.Focused,
			Agent:       firstString(pane.DisplayAgent, pane.Agent),
			AgentStatus: pane.AgentStatus,
			CWD:         cwd,
			Revision:    pane.Revision,
			Scroll:      pane.Scroll,
		})
	}
	for _, layout := range snapshot.Layouts {
		projected := Layout{
			ProjectID:     layout.WorkspaceID,
			TabID:         layout.TabID,
			Zoomed:        layout.Zoomed,
			Area:          layout.Area,
			FocusedPaneID: layout.FocusedPaneID,
			Panes:         make([]LayoutPane, 0, len(layout.Panes)),
		}
		for _, pane := range layout.Panes {
			projected.Panes = append(projected.Panes, LayoutPane{
				PaneID:  pane.PaneID,
				Focused: pane.Focused,
				Rect:    pane.Rect,
			})
		}
		projection.Layouts = append(projection.Layouts, projected)
	}
	for _, agent := range snapshot.Agents {
		cwd := firstString(agent.ForegroundCWD, agent.CWD)
		if cwd != "" && projectCWD[agent.WorkspaceID] == "" {
			projectCWD[agent.WorkspaceID] = cwd
		}
		projection.Agents = append(projection.Agents, Agent{
			TerminalID: agent.TerminalID,
			ProjectID:  agent.WorkspaceID,
			TabID:      agent.TabID,
			PaneID:     agent.PaneID,
			Name:       firstString(agent.Name),
			Kind:       firstString(agent.DisplayAgent, agent.Agent),
			Status:     agent.AgentStatus,
			Focused:    agent.Focused,
			CWD:        cwd,
			Revision:   agent.Revision,
		})
		if agent.AgentSession != nil {
			projection.Agents[len(projection.Agents)-1].AgentSession = &AgentSessionIdentity{
				Agent:  agent.AgentSession.Agent,
				Kind:   agent.AgentSession.Kind,
				Source: agent.AgentSession.Source,
				Value:  agent.AgentSession.Value,
			}
		}
	}
	for _, workspace := range snapshot.Workspaces {
		projection.Projects = append(projection.Projects, Project{
			ID:          workspace.ID,
			Label:       displayLabel(workspace.Label, workspace.ID),
			Number:      workspace.Number,
			Focused:     workspace.Focused,
			PaneCount:   workspace.PaneCount,
			TabCount:    workspace.TabCount,
			ActiveTabID: workspace.ActiveTabID,
			AgentStatus: workspace.AgentStatus,
			CWD:         projectCWD[workspace.ID],
		})
	}
	for _, tab := range snapshot.Tabs {
		projection.Tabs = append(projection.Tabs, Tab{
			ID:          tab.ID,
			ProjectID:   tab.WorkspaceID,
			Label:       displayLabel(tab.Label, tab.ID),
			Number:      tab.Number,
			Focused:     tab.Focused,
			PaneCount:   tab.PaneCount,
			AgentStatus: tab.AgentStatus,
		})
	}
	return projection
}

func firstString(values ...*string) string {
	for _, value := range values {
		if value != nil {
			if text := strings.TrimSpace(*value); text != "" {
				return text
			}
		}
	}
	return ""
}

func stringPtr(value string) *string { return &value }

func displayLabel(label, fallback string) string {
	if label = strings.TrimSpace(label); label != "" {
		return label
	}
	return fallback
}
