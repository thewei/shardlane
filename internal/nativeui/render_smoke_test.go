package nativeui

import (
	"context"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// TestRenderSmokeEvidence renders real Native UI frames for the 0.3.0
// History/Settings smoke evidence. It is skipped unless
// SHARDLANE_RENDER_SMOKE=<output-dir> is set, so normal test runs stay
// silent and no images are written.
func TestRenderSmokeEvidence(t *testing.T) {
	outDir := os.Getenv("SHARDLANE_RENDER_SMOKE")
	if outDir == "" {
		t.Skip("set SHARDLANE_RENDER_SMOKE=<dir> to render smoke evidence frames")
	}

	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl", codexFixtureRollout)
	catalog, err := history.OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()
	service := history.NewHistoryService(catalog, []history.SourceRoot{{
		Agent: history.AgentCodex, Directory: sessions, NativeID: testRolloutNativeID,
	}})
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	shell := NewShell(WithHistory(service))
	// Deterministic integration health for the Providers frame.
	shell.integrations.service = agent.NewIntegrationHealthServiceWithRunner(func(ctx context.Context, args []string) (string, error) {
		return "codex: current (v8) (/Users/demo/.codex/herdr-agent-state.sh)\n" +
			"claude: outdated (v9 < v10) (/Users/demo/.claude/hooks/herdr-agent-state.sh)\n", nil
	})
	render := func(name string) {
		tester := ui.NewTester(shell.View, 1280, 800)
		// Pump past the router's 250 ms slide transition so the frame shows
		// the settled page.
		for i := 0; i < 40; i++ {
			tester.Frame()
		}
		out := filepath.Join(outDir, name)
		file, err := os.Create(out)
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
		t.Logf("rendered %s", out)
	}

	shell.router.Replace("/workspace")
	render("01-workspace.png")
	shell.router.Replace("/history")
	render("02-history-list.png")
	shell.router.Replace("/history/codex:22222222-aaaa-bbbb-cccc-000000000002")
	render("03-history-detail.png")
	shell.router.Replace("/settings/general")
	render("04-settings-general.png")
	shell.router.Replace("/settings/terminal")
	render("05-settings-terminal.png")
	shell.router.Replace("/settings/providers")
	render("06-settings-providers.png")

	// Operational-status evidence (RENDER-06): working/blocked/done pills.
	shell.router.Replace("/workspace")
	shell.projection = projectionWithAgentStatus("working")
	render("08-status-working.png")
	shell.projection = projectionWithAgentStatus("blocked")
	render("09-status-blocked.png")
	shell.projection = projectionWithAgentStatus("done")
	render("10-status-done.png")
}

// projectionWithAgentStatus builds a minimal workspace projection whose
// single agent carries the given runtime status.
func projectionWithAgentStatus(status string) herdr.Projection {
	return herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		FocusedPaneID:    "p1",
		Projects:         []herdr.Project{{ID: "w1", Label: "Alpha", TabCount: 1}},
		Tabs:             []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main", PaneCount: 1}},
		Panes:            []herdr.Pane{{ID: "p1", TerminalID: "term-1", ProjectID: "w1", TabID: "t1"}},
		Agents:           []herdr.Agent{{PaneID: "p1", ProjectID: "w1", TabID: "t1", Name: "Scout", Status: status}},
	}
}
