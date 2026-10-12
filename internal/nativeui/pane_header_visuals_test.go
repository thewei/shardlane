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

func TestPaneHeaderVisualsProcess(t *testing.T) {
	s := NewShell()
	s.serviceIndex = sidebarServiceIndex{
		Panes: map[string]paneServiceActivity{
			"p-node": {Proc: "node"},
			"p-git":  {Proc: "git"},
			"p-zsh":  {Proc: "zsh"},
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
}

func TestPaneStackInfoAndStackedVisuals(t *testing.T) {
	if iconTerminalStacked2 == nil {
		t.Fatalf("iconTerminalStacked2 must not be nil")
	}
	if iconTerminalStacked3 == nil {
		t.Fatalf("iconTerminalStacked3 must not be nil")
	}

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
