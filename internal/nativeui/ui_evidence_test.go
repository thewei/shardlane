package nativeui

/**
 * [INPUT]: Headless MyGo renderer, a synthetic multi-workspace projection (agents, split panes) and a seeded history catalog
 * [OUTPUT]: Optional dark/light PNGs of Workspace, Settings pages, History (all, one Agent tab, a session detail), Files, Hosts (seeded temp ssh config) and the Agent panel for visual review
 * [POS]: Visual QA only, enabled by SHARDLANE_UI_EVIDENCE; never touches a live Herdr session
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

// evidenceHistory seeds a catalog with a handful of Codex sessions so the
// History page renders a realistic list.
func evidenceHistory(t *testing.T) *history.HistoryService {
	t.Helper()
	sessions := filepath.Join(t.TempDir(), "sessions")
	titles := []string{
		"Refactor the sidebar tree", "Fix flaky terminal resize test", "Add dark mode to settings",
		"Investigate slow startup", "Write release notes for 0.10", "Polish the diff review layout",
	}
	for i, title := range titles {
		body := strings.ReplaceAll(codexFixtureRollout, "Build history", title)
		body = strings.ReplaceAll(body, "2026-08-02T09", fmt.Sprintf("2026-08-%02dT09", 2+i))
		name := fmt.Sprintf("rollout-2026-08-%02dT09-15-00-%08d-aaaa-bbbb-cccc-0000000000%02d.jsonl", 2+i, i+1, i+1)
		writeCodexSessionFile(t, sessions, name, body)
	}
	catalog := history.NewCatalog()
	service := history.NewHistoryService(catalog, []history.SourceRoot{{
		Agent: history.AgentCodex, Directory: sessions, NativeID: testRolloutNativeID,
	}})
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	// Pi and Claude Code sessions in other projects, so the Sessions tab
	// strip, group cards and token column render as with a real catalog.
	model, tokens, bigTokens := "glm-5.3-flash", int64(38_400), int64(1_240_000)
	for i, seed := range []struct {
		agent   history.AgentID
		title   string
		project string
		tokens  *int64
		size    int64
	}{
		{history.AgentPi, "Overhaul the chrome and drop the AI flavor", "/Users/wilson/Workspaces/wh-studio/herdr-client", &bigTokens, 7_800_000},
		{history.AgentPi, "Move the inner-page back button into the sidebar", "/Users/wilson/Workspaces/wh-studio/herdr-client", &tokens, 410_000},
		{history.AgentClaudeCode, "Review the migration roadmap", "/Users/wilson/Workspaces/labs-portal-dev", &tokens, 96_000},
		{history.AgentClaudeCode, "Untangle the billing webhook retries", "/Users/wilson/Workspaces/labs-portal-dev", nil, 52_000},
	} {
		updated := time.Now().Add(-time.Duration(i+1) * 5 * time.Hour).UnixMilli()
		meta := history.SessionMeta{
			Key: fmt.Sprintf("%s:evidence-%d", seed.agent, i), ID: fmt.Sprintf("evidence-%d", i), Agent: seed.agent,
			Title: seed.title, ProjectPath: seed.project, ProjectName: filepath.Base(seed.project),
			FilePath: fmt.Sprintf("/nowhere/evidence-%d", i), UpdatedAt: updated, MessageCount: int64(12 + i*37),
			SizeBytes: seed.size, Model: &model, TokensUsed: seed.tokens,
		}
		catalog.Put(meta, updated, "")
	}
	return service
}

// evidenceShell builds a Shell with several workspaces, two agent panes and
// one plain shell pane split side by side, all headless.
func evidenceShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell(WithHistory(evidenceHistory(t)))
	shell.loading = false
	shell.activeInstance = "default"
	area := herdr.LayoutRect{Width: 120, Height: 40}
	shell.projection = herdr.Projection{
		Protocol: 22, Version: "0.9.3",
		FocusedProjectID: "w1", FocusedTabID: "t1", FocusedPaneID: "p1",
		Projects: []herdr.Project{
			{ID: "w1", Label: "herdr-client", TabCount: 2, PaneCount: 3, CWD: "/Users/wilson/Workspaces/wh-studio/herdr-client"},
			{ID: "w2", Label: "labs-portal-dev", TabCount: 1, PaneCount: 1, CWD: "/Users/wilson/Workspaces/labs-portal-dev"},
			{ID: "w3", Label: "getaipages", TabCount: 2, PaneCount: 2, CWD: "/Users/wilson/Workspaces/getaipages"},
		},
		Tabs: []herdr.Tab{
			{ID: "t1", ProjectID: "w1", Label: "Coding", PaneCount: 2},
			{ID: "t2", ProjectID: "w1", Label: "Dev", PaneCount: 1},
			{ID: "t3", ProjectID: "w2", Label: "Platform", PaneCount: 1},
			{ID: "t4", ProjectID: "w3", Label: "Sites", PaneCount: 1},
			{ID: "t5", ProjectID: "w3", Label: "Docs", PaneCount: 1},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term-1", ProjectID: "w1", TabID: "t1", Label: "pi", CWD: "/Users/wilson/Workspaces/wh-studio/herdr-client"},
			{ID: "p2", TerminalID: "term-2", ProjectID: "w1", TabID: "t1", Label: "zsh", CWD: "/Users/wilson/Workspaces/wh-studio/herdr-client"},
			{ID: "p3", TerminalID: "term-3", ProjectID: "w1", TabID: "t2", Label: "server", CWD: "/Users/wilson/Workspaces/wh-studio/herdr-client"},
			{ID: "p4", TerminalID: "term-4", ProjectID: "w2", TabID: "t3", Label: "claude", CWD: "/Users/wilson/Workspaces/labs-portal-dev"},
			{ID: "p5", TerminalID: "term-5", ProjectID: "w3", TabID: "t4", Label: "codex", CWD: "/Users/wilson/Workspaces/getaipages"},
			{ID: "p6", TerminalID: "term-6", ProjectID: "w3", TabID: "t5", Label: "zsh", CWD: "/Users/wilson/Workspaces/getaipages"},
		},
		Layouts: []herdr.Layout{{
			ProjectID: "w1", TabID: "t1", Area: area,
			Panes: []herdr.LayoutPane{
				{PaneID: "p1", Focused: true, Rect: herdr.LayoutRect{Width: 70, Height: 40}},
				{PaneID: "p2", Rect: herdr.LayoutRect{X: 70, Width: 50, Height: 40}},
			},
		}},
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", ProjectID: "w1", TabID: "t1", Name: "Pi", Kind: "pi", Status: "working"},
			{TerminalID: "term-4", PaneID: "p4", ProjectID: "w2", TabID: "t3", Name: "Claude Code", Kind: "claude-code", Status: "blocked"},
			{TerminalID: "term-5", PaneID: "p5", ProjectID: "w3", TabID: "t4", Name: "Codex", Kind: "codex", Status: "done"},
		},
	}
	shell.reconcileLocalSelection(shell.projection)
	shell.reconcileWorkbench()
	shell.selectedProjectID, shell.selectedTabID, shell.selectedPaneID = "w1", "t1", "p1"

	script := map[string]string{
		"term-1": "\x1b[2J\x1b[1;1H\x1b[1mπ\x1b[0m Pi  \x1b[2mgpt-5 · 41k tokens\x1b[0m\r\n\r\n\x1b[36m●\x1b[0m Running tests\r\n  \x1b[32m✓\x1b[0m go test ./internal/nativeui\r\n  \x1b[32m✓\x1b[0m go vet ./...\r\n\r\n> Make the sidebar quieter_",
		"term-2": "\x1b[2J\x1b[1;1H\x1b[32m➜\x1b[0m \x1b[36mherdr-client\x1b[0m \x1b[34mgit:(\x1b[31mnext\x1b[34m)\x1b[0m git status -sb\r\n## next...origin/next\r\n M internal/nativeui/theme.go\r\n M internal/nativeui/icons.go\r\n\x1b[32m➜\x1b[0m \x1b[36mherdr-client\x1b[0m \x1b[34mgit:(\x1b[31mnext\x1b[34m)\x1b[0m _",
	}
	rects := map[string]herdr.LayoutRect{
		"term-1": {Width: 70, Height: 40}, "term-2": {X: 70, Width: 50, Height: 40},
	}
	for key, text := range script {
		term, err := terminal.New(terminal.Options{Conn: newScriptPipe(text)})
		if err != nil {
			t.Skip("terminal library unavailable:", err)
		}
		t.Cleanup(func() { _ = term.Close() })
		pane := paneByTerminal(shell.projection, key)
		shell.terminals[key] = &terminalSurface{
			key: key, paneID: pane.ID, term: term, label: pane.Label, area: area, rect: rects[key],
			focused: key == "term-1", cwd: pane.CWD,
		}
	}
	shell.terminals["term-1"].agent, shell.terminals["term-1"].agentStatus = "pi", "working"
	shell.router.Replace(routeWorkspace)
	return shell
}

func paneByTerminal(p herdr.Projection, terminalID string) herdr.Pane {
	for _, pane := range p.Panes {
		if pane.TerminalID == terminalID {
			return pane
		}
	}
	return herdr.Pane{}
}

func TestUIEvidence(t *testing.T) {
	dir := os.Getenv("SHARDLANE_UI_EVIDENCE")
	if dir == "" {
		t.Skip("set SHARDLANE_UI_EVIDENCE to render UI review PNGs")
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, dark := range []bool{true, false} {
		mode := "light"
		if dark {
			mode = "dark"
		}
		shell := evidenceShell(t)
		tt := ui.NewTester(shell.View, 1280, 820)
		tt.SetDark(dark)
		render := func(name string) {
			// Let the route transition finish: its clock is real time.
			time.Sleep(350 * time.Millisecond)
			for i := 0; i < 3; i++ {
				tt.Frame()
			}
			file, err := os.Create(filepath.Join(dir, fmt.Sprintf("%s-%s.png", name, mode)))
			if err != nil {
				t.Fatal(err)
			}
			defer file.Close()
			if err := png.Encode(file, tt.Image()); err != nil {
				t.Fatal(err)
			}
		}
		render("01-workspace")
		for i, path := range []string{
			"/settings/general", "/settings/terminal", "/settings/providers",
			"/settings/git", "/settings/diagnostics",
		} {
			shell.router.Push(path)
			render(fmt.Sprintf("%02d-%s", 2+i, strings.ReplaceAll(strings.TrimPrefix(path, "/"), "/", "-")))
		}
		shell.router.Push("/history")
		render("08-history")
		if err := tt.Click("Pi"); err != nil {
			t.Fatal(err)
		}
		render("08b-history-pi")
		shell.hist.provider = ""
		shell.applyHistoryFilters()
		for _, summary := range shell.hist.sessions {
			if summary.Meta.Agent == history.AgentCodex {
				shell.router.Replace("/history/" + summary.Meta.Key)
				break
			}
		}
		render("08c-history-detail")
		shell.router.Push("/files")
		render("09-files")

		// Files with a file open: the native code view over a seeded temp
		// root (this package's own themed Go source), then a Markdown file
		// whose Source choice shows it natively.
		code := t.TempDir()
		source, err := os.ReadFile("code_theme.go")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(code, "code_theme.go"), source, 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(code, "README.md"), []byte("# Notes\n\nA **short** readme with `code` and a [link](https://example.com).\n\n- one\n- two\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		shell.projection.Projects[0].CWD = code
		for i := range shell.projection.Panes {
			if shell.projection.Panes[i].ProjectID == "w1" {
				shell.projection.Panes[i].CWD = code
			}
		}
		tt.Frame()
		tt.Frame() // the root swap and its directory read land
		shell.openFilePreview(filepath.Join(code, "code_theme.go"))
		render("09b-files-code")
		shell.setMarkdownSource(true)
		shell.openFilePreview(filepath.Join(code, "README.md"))
		render("09c-files-markdown-source")

		// Hosts reads a seeded temp config, never the developer's real one.
		config := filepath.Join(t.TempDir(), "config")
		if err := os.WriteFile(config, []byte("Host dev-box\n  HostName 10.0.0.12\n  User wilson\n\nHost staging\n  HostName staging.example.com\n  User deploy\n  Port 2222\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		index, err := sshconfig.IndexPath(config)
		if err != nil {
			t.Fatal(err)
		}
		shell.hosts.loaded, shell.hosts.mainPath, shell.hosts.entries, shell.hosts.files = true, index.MainPath, index.Entries, index.Files
		if len(index.Entries) > 0 {
			shell.hosts.selected = hostEntryKey(index.Entries[0])
		}
		shell.router.Push("/hosts")
		render("11-hosts")

		// The Agent activity panel at its content-driven height: the
		// Agents list grouped by state, then the Services tab over a seeded
		// port.
		shell.portsForRoot, shell.observedPortsList, shell.portsRefreshedAt = code, []uint16{3000}, time.Now()
		// The finished Codex agent waits for review (its trailing mark shows).
		shell.workbench.markers.Observe(agent.AgentKey{InstanceID: "default", TerminalID: "term-5"},
			agent.TransitionReadyForReview, false)
		shell.reconcileWorkbenchMarkersOnly()
		for i, tab := range []int{0, 1} {
			shell.quickPanelTab = tab
			panel := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, shell.quickPanelContentHeight())
			panel.SetDark(dark)
			panel.Frame()
			name := []string{"10-agent-panel", "10b-agent-services"}[i]
			file, err := os.Create(filepath.Join(dir, fmt.Sprintf("%s-%s.png", name, mode)))
			if err != nil {
				t.Fatal(err)
			}
			if err := png.Encode(file, panel.Image()); err != nil {
				t.Fatal(err)
			}
			file.Close()
		}
		shell.quickPanelTab = 0
	}
}
