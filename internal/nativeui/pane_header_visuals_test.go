package nativeui

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

/**
 * [INPUT]: 依赖 nativeui 的 Shell/paneHeaderVisuals/paneStackInfo/shortDir/samePath/procFriendlyName, herdr
 * [OUTPUT]: 对外提供 TestPaneHeaderVisualsAgent/TestPaneHeaderVisualsProcess/TestShortDirAndSamePath/TestPaneStackInfoAndStackedVisuals
 * [POS]: 窗格卡片头部视觉解析、Agent 图标与名称解析、跨平台路径格式化以及多终端堆叠状态的单元测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestPaneHeaderVisualsAgent(t *testing.T) {
	s := NewShell()
	s.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{PaneID: "p-claude", Kind: "claude-code", Name: "Refactor Task", Status: "working"},
			{PaneID: "p-codex", Kind: "codex", Status: "idle"},
			{PaneID: "p-antigravity", Kind: "antigravity", Status: "working"},
			{PaneID: "p-agy-session", AgentSession: &herdr.AgentSessionIdentity{Agent: "antigravity-cli"}, Status: "idle"},
		},
	}

	surfaceClaude := &terminalSurface{paneID: "p-claude", cwd: "/Users/test/project"}
	markClaude, titleClaude, _ := s.paneHeaderVisuals(surfaceClaude, false)
	if markClaude.bmp == nil && markClaude.svg == nil {
		t.Fatalf("claude pane mark should not be empty")
	}
	if titleClaude != "Refactor Task" {
		t.Fatalf("titleClaude = %q, want 'Refactor Task'", titleClaude)
	}

	surfaceCodex := &terminalSurface{paneID: "p-codex", cwd: "/Users/test/project"}
	markCodex, titleCodex, _ := s.paneHeaderVisuals(surfaceCodex, false)
	if markCodex.bmp == nil && markCodex.svg == nil {
		t.Fatalf("codex pane mark should not be empty")
	}
	if titleCodex != "Codex" {
		t.Fatalf("titleCodex = %q, want 'Codex'", titleCodex)
	}

	surfaceAG := &terminalSurface{paneID: "p-antigravity", cwd: "/Users/test/project"}
	markAG, titleAG, _ := s.paneHeaderVisuals(surfaceAG, false)
	if markAG.bmp == nil {
		t.Fatalf("antigravity pane mark should have brand bitmap, got %+v", markAG)
	}
	if titleAG != "Antigravity CLI" {
		t.Fatalf("titleAG = %q, want 'Antigravity CLI'", titleAG)
	}

	surfaceAGSession := &terminalSurface{paneID: "p-agy-session", cwd: "/Users/test/project"}
	markAGSession, titleAGSession, _ := s.paneHeaderVisuals(surfaceAGSession, false)
	if markAGSession.bmp == nil {
		t.Fatalf("antigravity session pane mark should have brand bitmap, got %+v", markAGSession)
	}
	if titleAGSession != "Antigravity CLI" {
		t.Fatalf("titleAGSession = %q, want 'Antigravity CLI'", titleAGSession)
	}
}

// A name Shardlane generated for Herdr (pi-3fa91c, or the longer form older
// builds minted) is plumbing: the header shows the provider, a name someone
// chose stays.
func TestPaneHeaderVisualsHidesGeneratedAgentNames(t *testing.T) {
	s := NewShell()
	s.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{PaneID: "p-new", Kind: "pi", Name: "pi-3fa91c", Status: "idle"},
			{PaneID: "p-old", Kind: "pi", Name: "pi-continue-01a12e5f-b856-71d7-8", Status: "idle"},
			{PaneID: "p-chosen", Kind: "pi", Name: "Review pass", Status: "idle"},
		},
	}
	for paneID, want := range map[string]string{"p-new": "Pi", "p-old": "Pi", "p-chosen": "Review pass"} {
		if _, title, _ := s.paneHeaderVisuals(&terminalSurface{paneID: paneID, cwd: "/Users/test/project"}, false); title != want {
			t.Errorf("%s title = %q, want %q", paneID, title, want)
		}
	}
}

func TestPaneHeaderVisualsProcess(t *testing.T) {
	s := NewShell()
	s.serviceIndex = sidebarServiceIndex{
		Panes: map[string]paneServiceActivity{
			"p-node":        {Proc: "node"},
			"p-git":         {Proc: "git"},
			"p-zsh":         {Proc: "zsh"},
			"p-antigravity": {Proc: "antigravity"},
			"p-mongo":       {Proc: "mongod"},
			"p-nginx":       {Proc: "nginx"},
			"p-ollama":      {Proc: "ollama"},
			"p-uv":          {Proc: "uv"},
		},
	}

	surfaceNode := &terminalSurface{paneID: "p-node", cwd: "/Users/test/app"}
	markNode, titleNode, _ := s.paneHeaderVisuals(surfaceNode, false)
	if markNode.svg == nil || markNode.svg == iconTerminal {
		t.Fatalf("node mark should be node brand SVG, got %+v", markNode)
	}
	if titleNode != "Node" {
		t.Fatalf("titleNode = %q, want 'Node'", titleNode)
	}

	surfaceGit := &terminalSurface{paneID: "p-git", cwd: "/Users/test/repo"}
	markGit, titleGit, _ := s.paneHeaderVisuals(surfaceGit, false)
	if markGit.svg == nil || markGit.svg == iconTerminal {
		t.Fatalf("git mark should be git brand SVG, got %+v", markGit)
	}
	if titleGit != "Git" {
		t.Fatalf("titleGit = %q, want 'Git'", titleGit)
	}

	surfaceZsh := &terminalSurface{paneID: "p-zsh", label: "My Shell", cwd: "/Users/test"}
	markZsh, titleZsh, _ := s.paneHeaderVisuals(surfaceZsh, false)
	if markZsh.svg != iconTerminal {
		t.Fatalf("shell mark should fall back to iconTerminal, got %+v", markZsh)
	}
	if titleZsh != "My Shell" {
		t.Fatalf("titleZsh = %q, want 'My Shell'", titleZsh)
	}

	surfaceAG := &terminalSurface{paneID: "p-antigravity", cwd: "/Users/test"}
	markAG, titleAG, _ := s.paneHeaderVisuals(surfaceAG, false)
	if markAG.bmp == nil {
		t.Fatalf("antigravity process mark should be brand bitmap, got %+v", markAG)
	}
	if titleAG != "Antigravity CLI" {
		t.Fatalf("titleAG = %q, want 'Antigravity CLI'", titleAG)
	}

	surfaceMongo := &terminalSurface{paneID: "p-mongo", cwd: "/Users/test"}
	markMongo, titleMongo, _ := s.paneHeaderVisuals(surfaceMongo, false)
	if markMongo.svg == nil || markMongo.svg == iconTerminal {
		t.Fatalf("mongo mark should be mongodb SVG, got %+v", markMongo)
	}
	if titleMongo != "MongoDB" {
		t.Fatalf("titleMongo = %q, want 'MongoDB'", titleMongo)
	}

	surfaceNginx := &terminalSurface{paneID: "p-nginx", cwd: "/Users/test"}
	markNginx, titleNginx, _ := s.paneHeaderVisuals(surfaceNginx, false)
	if markNginx.svg == nil || markNginx.svg == iconTerminal {
		t.Fatalf("nginx mark should be nginx SVG, got %+v", markNginx)
	}
	if titleNginx != "Nginx" {
		t.Fatalf("titleNginx = %q, want 'Nginx'", titleNginx)
	}

	surfaceOllama := &terminalSurface{paneID: "p-ollama", cwd: "/Users/test"}
	markOllama, titleOllama, _ := s.paneHeaderVisuals(surfaceOllama, false)
	if markOllama.svg == nil || markOllama.svg == iconTerminal {
		t.Fatalf("ollama mark should be ollama SVG, got %+v", markOllama)
	}
	if titleOllama != "Ollama" {
		t.Fatalf("titleOllama = %q, want 'Ollama'", titleOllama)
	}

	surfaceUv := &terminalSurface{paneID: "p-uv", cwd: "/Users/test"}
	markUv, titleUv, _ := s.paneHeaderVisuals(surfaceUv, false)
	if markUv.svg == nil || markUv.svg == iconTerminal {
		t.Fatalf("uv mark should be python SVG, got %+v", markUv)
	}
	if titleUv != "uv" {
		t.Fatalf("titleUv = %q, want 'uv'", titleUv)
	}
}

func TestShortDirAndSamePath(t *testing.T) {
	home, err := os.UserHomeDir()
	if err == nil && home != "" {
		if got := shortDir(home); got != "~" {
			t.Fatalf("shortDir(home) = %q, want '~'", got)
		}
		sub := filepath.Join(home, "projects", "repo")
		if got := shortDir(sub); got != "~"+string(filepath.Separator)+"projects"+string(filepath.Separator)+"repo" {
			t.Fatalf("shortDir(sub) = %q", got)
		}
	}

	deep := filepath.Join(string(filepath.Separator)+"a", "b", "c", "d", "e", "f", "g")
	shortDeep := shortDir(deep)
	if len(shortDeep) == 0 || shortDeep[:3] != "…" {
		t.Fatalf("shortDir(deep) = %q, want starting with ellipsis", shortDeep)
	}

	if !samePath("/foo/bar", "/foo/bar") {
		t.Fatalf("samePath identical paths returned false")
	}
}

func TestShellAndFriendlyNames(t *testing.T) {
	if !isShell("zsh") || !isShell("bash") || !isShell("pwsh") || !isShell("cmd.exe") {
		t.Fatalf("isShell failed to recognize common shells")
	}
	if isShell("node") || isShell("python3") || isShell("git") {
		t.Fatalf("isShell incorrectly identified non-shell programs as shells")
	}

	if got := procFriendlyName("lazygit"); got != "Git Changes" {
		t.Fatalf("procFriendlyName(lazygit) = %q, want 'Git Changes'", got)
	}
	if got := procFriendlyName("claude"); got != "Claude Code" {
		t.Fatalf("procFriendlyName(claude) = %q, want 'Claude Code'", got)
	}
	if got := procFriendlyName("docker-compose.exe"); got != "Docker Compose" {
		t.Fatalf("procFriendlyName(docker-compose.exe) = %q, want 'Docker Compose'", got)
	}
	if got := procFriendlyName("antigravity"); got != "Antigravity CLI" {
		t.Fatalf("procFriendlyName(antigravity) = %q, want 'Antigravity CLI'", got)
	}
	if got := procFriendlyName("nginx"); got != "Nginx" {
		t.Fatalf("procFriendlyName(nginx) = %q, want 'Nginx'", got)
	}
	if got := procFriendlyName("caddy"); got != "Caddy" {
		t.Fatalf("procFriendlyName(caddy) = %q, want 'Caddy'", got)
	}
	if got := procFriendlyName("mongod"); got != "MongoDB" {
		t.Fatalf("procFriendlyName(mongod) = %q, want 'MongoDB'", got)
	}
	if got := procFriendlyName("sqlite3"); got != "SQLite" {
		t.Fatalf("procFriendlyName(sqlite3) = %q, want 'SQLite'", got)
	}
	if got := procFriendlyName("ollama"); got != "Ollama" {
		t.Fatalf("procFriendlyName(ollama) = %q, want 'Ollama'", got)
	}
	if got := procFriendlyName("kubectl"); got != "Kubectl" {
		t.Fatalf("procFriendlyName(kubectl) = %q, want 'Kubectl'", got)
	}
}

func TestPaneStackInfoAndStackedVisuals(t *testing.T) {
	s := NewShell()
	s.projection = herdr.Projection{
		FocusedProjectID: "prj", FocusedTabID: "tab", FocusedPaneID: "p1",
		Tabs: []herdr.Tab{
			{ID: "tab", ProjectID: "prj"},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "t1", ProjectID: "prj", TabID: "tab"},
			{ID: "p2", TerminalID: "t2", ProjectID: "prj", TabID: "tab"},
			{ID: "p3", TerminalID: "t3", ProjectID: "prj", TabID: "tab"},
		},
		Layouts: []herdr.Layout{
			{
				ProjectID: "prj", TabID: "tab", Zoomed: false,
				Panes: []herdr.LayoutPane{
					{PaneID: "p1"},
					{PaneID: "p2"},
				},
			},
		},
	}

	// 2 panes, unzoomed (split layout side-by-side)
	isStacked, count := s.paneStackInfo("p1")
	if isStacked || count != 2 {
		t.Fatalf("unzoomed split tab should not be stacked: isStacked=%v, count=%d", isStacked, count)
	}

	// 2 panes, zoomed (maximized)
	s.projection.Layouts[0].Zoomed = true
	isStacked, count = s.paneStackInfo("p1")
	if !isStacked || count != 2 {
		t.Fatalf("zoomed 2-pane tab should be stacked: isStacked=%v, count=%d", isStacked, count)
	}

	// 3 panes, zoomed
	s.projection.Layouts[0].Panes = append(s.projection.Layouts[0].Panes, herdr.LayoutPane{PaneID: "p3"})
	isStacked, count = s.paneStackInfo("p1")
	if !isStacked || count != 3 {
		t.Fatalf("zoomed 3-pane tab should be stacked: isStacked=%v, count=%d", isStacked, count)
	}

	// Non-existent pane
	isStacked, count = s.paneStackInfo("non-existent")
	if isStacked || count != 1 {
		t.Fatalf("non-existent pane should return false, 1: isStacked=%v, count=%d", isStacked, count)
	}
}
