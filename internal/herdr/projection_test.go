package herdr

import "testing"

func TestProjectSnapshotBuildsProductProjection(t *testing.T) {
	focusedProject := "w1"
	focusedTab := "t1"
	focusedPane := "p1"
	cwd := "/tmp/demo"
	paneTitle := "editor"
	agent := "claude"
	projection := projectSnapshot(sessionSnapshot{
		Version:            "0.9.3",
		Protocol:           22,
		FocusedWorkspaceID: &focusedProject,
		FocusedTabID:       &focusedTab,
		FocusedPaneID:      &focusedPane,
		Workspaces: []workspaceInfo{{
			ID: "w1", Number: 1, Label: "demo", Focused: true,
			PaneCount: 1, TabCount: 1, ActiveTabID: "t1", AgentStatus: "working",
		}},
		Tabs: []tabInfo{{
			ID: "t1", WorkspaceID: "w1", Number: 1, Label: "main",
			Focused: true, PaneCount: 1, AgentStatus: "working",
		}},
		Panes: []paneInfo{{
			ID: "p1", TerminalID: "term-1", WorkspaceID: "w1", TabID: "t1",
			Title: &paneTitle, Focused: true, Agent: &agent, AgentStatus: "working",
			CWD: &cwd, Revision: 7,
		}},
		Agents: []agentInfo{{
			TerminalID: "term-1", WorkspaceID: "w1", TabID: "t1", PaneID: "p1",
			Agent: &agent, AgentStatus: "working", Focused: true, CWD: &cwd, Revision: 7,
		}},
	})

	if projection.FocusedProjectID != "w1" || projection.FocusedTabID != "t1" || projection.FocusedPaneID != "p1" {
		t.Fatalf("focus projection = %#v", projection)
	}
	if len(projection.Projects) != 1 || projection.Projects[0].CWD != cwd {
		t.Fatalf("projects = %#v", projection.Projects)
	}
	if len(projection.Tabs) != 1 || projection.Tabs[0].ProjectID != "w1" {
		t.Fatalf("tabs = %#v", projection.Tabs)
	}
	if len(projection.Panes) != 1 || projection.Panes[0].Label != paneTitle || projection.Panes[0].Agent != agent {
		t.Fatalf("panes = %#v", projection.Panes)
	}
	if len(projection.Agents) != 1 || projection.Agents[0].Kind != agent {
		t.Fatalf("agents = %#v", projection.Agents)
	}
}

func TestProjectSnapshotUsesPaneCWDForProject(t *testing.T) {
	cwd := "/repo/from-pane"
	projection := projectSnapshot(sessionSnapshot{
		Version:  "0.9.3",
		Protocol: 22,
		Workspaces: []workspaceInfo{{
			ID: "w1", Label: "project", ActiveTabID: "t1", AgentStatus: "idle",
		}},
		Panes: []paneInfo{{
			ID: "p1", TerminalID: "term-1", WorkspaceID: "w1", TabID: "t1",
			AgentStatus: "idle", CWD: &cwd,
		}},
	})
	if got := projection.Projects[0].CWD; got != cwd {
		t.Fatalf("project cwd = %q, want %q", got, cwd)
	}
}

// TestProjectSnapshotMapsAgentSessionIdentity pins the measured
// AgentInfo.agent_session mapping (live schema, protocol 22): the typed
// provider session identity flows to the projection and stays absent when
// the agent has none.
func TestProjectSnapshotMapsAgentSessionIdentity(t *testing.T) {
	agent := "codex"
	projection := projectSnapshot(sessionSnapshot{
		Version:  "0.9.3",
		Protocol: 22,
		Workspaces: []workspaceInfo{{
			ID: "w1", Label: "project", ActiveTabID: "t1", AgentStatus: "working",
		}},
		Agents: []agentInfo{{
			TerminalID: "term-1", WorkspaceID: "w1", TabID: "t1", PaneID: "p1",
			Agent: &agent, AgentStatus: "working", Revision: 3,
			AgentSession: &struct {
				Agent  string `json:"agent"`
				Kind   string `json:"kind"`
				Source string `json:"source"`
				Value  string `json:"value"`
			}{Agent: "codex", Kind: "id", Source: "rollout", Value: "session-1"},
		}},
	})
	if len(projection.Agents) != 1 {
		t.Fatalf("agents = %#v", projection.Agents)
	}
	identity := projection.Agents[0].AgentSession
	if identity == nil || identity.Agent != "codex" || identity.Kind != "id" ||
		identity.Source != "rollout" || identity.Value != "session-1" {
		t.Fatalf("agent session identity = %#v", identity)
	}

	plain := projectSnapshot(sessionSnapshot{
		Version:  "0.9.3",
		Protocol: 22,
		Workspaces: []workspaceInfo{{
			ID: "w1", Label: "project", ActiveTabID: "t1", AgentStatus: "working",
		}},
		Agents: []agentInfo{{
			TerminalID: "term-1", WorkspaceID: "w1", TabID: "t1", PaneID: "p1",
			Agent: &agent, AgentStatus: "working", Revision: 3,
		}},
	})
	if plain.Agents[0].AgentSession != nil {
		t.Fatalf("identity must stay absent without agent_session: %#v", plain.Agents[0].AgentSession)
	}
}

func TestFocusedTerminalID(t *testing.T) {
	projection := Projection{
		FocusedPaneID: "p2",
		Panes: []Pane{
			{ID: "p1", TerminalID: "term-1"},
			{ID: "p2", TerminalID: "term-2"},
		},
	}
	if got := projection.FocusedTerminalID(); got != "term-2" {
		t.Fatalf("FocusedTerminalID = %q", got)
	}
}
