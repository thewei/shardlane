package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// TestAgentCardLabelDisambiguatesSharedProjects pins F169: several visible
// cards of one project share the same project-name title (F118), so their
// labels append the agent's own title; a lone project card keeps the plain
// project name.
func TestAgentCardLabelDisambiguatesSharedProjects(t *testing.T) {
	cards := []agent.AgentCardModel{
		{ProjectName: "herdr-client", Title: "keytest"},
		{ProjectName: "herdr-client", Title: "test-claude"},
		{ProjectName: "solo-project", Title: "forgotten-agent"},
		{ProjectName: "", Title: "title-only"},
		{ProjectName: "self-named", Title: "self-named"},
	}
	dups := duplicateAgentProjectLabels(cards)
	if !dups["herdr-client"] || dups["solo-project"] || dups["self-named"] {
		t.Fatalf("dup set = %+v", dups)
	}
	want := []struct {
		label    string
		includes bool
	}{
		{"herdr-client · keytest", true},
		{"herdr-client · test-claude", true},
		{"solo-project", false},
		{"title-only", false},
		{"self-named", false},
	}
	for i, tc := range want {
		label, includes := agentCardLabel(cards[i], dups)
		if label != tc.label || includes != tc.includes {
			t.Fatalf("card %d label = (%q, %v), want (%q, %v)", i, label, includes, tc.label, tc.includes)
		}
	}
}

// TestSidebarAgentCardsStayDistinguishablePerProject renders the sidebar's
// Agents section with two same-project agents and asserts both card labels
// carry the agent title, so the section never shows identical rows.
func TestSidebarAgentCardsStayDistinguishablePerProject(t *testing.T) {
	s := NewShell()
	s.loading = false
	// reconcileWorkbench normally builds these from the first applied
	// projection; seed them directly for the headless render. agentItems
	// gates on the projection's agents, so it needs entries too.
	s.workbench.markers = agent.NewMarkerStore()
	s.workbench.directory = agent.NewAgentDirectory()
	s.projection.Agents = []herdr.Agent{{PaneID: "p1"}, {PaneID: "p2"}}
	s.workbench.directory.Replace([]agent.AgentCardModel{
		{
			Key:         agent.AgentKey{TerminalID: "term-1"},
			Provider:    history.AgentPi,
			Title:       "keytest",
			ProjectName: "herdr-client",
			PaneID:      "p1",
		},
		{
			Key:         agent.AgentKey{TerminalID: "term-2"},
			Provider:    history.AgentPi,
			Title:       "test-claude",
			ProjectName: "herdr-client",
			PaneID:      "p2",
		},
	})

	tester := ui.NewTester(s.View, 1200, 800)
	// Label and tooltip both surface the same string, so count distinct
	// texts: both disambiguated labels must appear, and the bare duplicate
	// project title must not appear at all.
	joined := map[string]bool{}
	plain := 0
	for _, text := range tester.Texts() {
		switch text {
		case "herdr-client · keytest", "herdr-client · test-claude":
			joined[text] = true
		case "herdr-client":
			plain++
		}
	}
	if len(joined) != 2 {
		t.Fatalf("joined labels = %v, want both agent titles", joined)
	}
	if plain != 0 {
		t.Fatalf("bare duplicate title rendered %d times; the section must not show identical rows", plain)
	}
}
