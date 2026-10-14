package nativeui

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 dialogs.go 的 modalTitle 与三种自建对话框（Merge、Commit、文本输入）、nativeui 包内非测试源码里 Typography().Title / typ.Title 的使用点、ui.Tester 无头渲染
 * [OUTPUT]: 对外提供对话框标题回归：自建对话框的标题是同一个字号（Merge/Commit 曾用页标题字号 20，文本输入用 16）、Title 字号只属于标题栏的页标题
 * [POS]: 2026-10-14 第二轮审计「同角色不同实现」扫描的守卫测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// TestModalsShareOneTitleSize renders the three dialogs the app builds itself
// and compares the height of their title lines: same size, same face.
func TestModalsShareOneTitleSize(t *testing.T) {
	heightOf := func(open func(s *Shell), title string) float32 {
		t.Helper()
		s := gitSurfaceTestShell(t, t.TempDir())
		s.showSurface(WorkspaceSurfaceDiff)
		open(s)
		tester := ui.NewTester(s.View, 1200, 800)
		tester.Frame()
		tester.Frame()
		rect, ok := tester.Find(title)
		if !ok {
			t.Fatalf("dialog title %q not on screen: %q", title, tester.Texts())
		}
		return rect.H
	}
	merge := heightOf(func(s *Shell) {
		s.mergeDialogOpen, s.mergeRepoRoot, s.mergeTarget = true, s.git.root, "develop"
	}, "Merge Branch")
	commit := heightOf(func(s *Shell) { s.openCommitSurface() }, "Commit Changes")
	prompt := heightOf(func(s *Shell) { s.openTextDialog("rename-tab", "t1", "Rename Tab", "Name", "Main") }, "Rename Tab")
	if merge != prompt || commit != prompt {
		t.Fatalf("dialog titles differ in size: Merge %.1f, Commit %.1f, text prompt %.1f", merge, commit, prompt)
	}
}

// pageTitleSize matches the 20-point Title token however it is reached.
var pageTitleSize = regexp.MustCompile(`Typography\(\)\.Title\b|\btyp\.Title\b`)

// TestTitleSizeIsForPageTitles keeps the 20-point Title token where it
// belongs: the page title in the title bar (route_header.go). A dialog or a
// card that wants a heading takes Heading.
func TestTitleSizeIsForPageTitles(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	scanned := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") || name == "route_header.go" {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		scanned++
		for i, line := range strings.Split(string(src), "\n") {
			if pageTitleSize.MatchString(line) && !strings.HasPrefix(strings.TrimSpace(line), "//") {
				t.Errorf("%s:%d uses the page-title size outside route_header.go; dialogs and cards take Typography().Heading", name, i+1)
			}
		}
	}
	if scanned < 100 {
		t.Fatalf("scanned only %d files; the scan no longer sees the sources", scanned)
	}
}
