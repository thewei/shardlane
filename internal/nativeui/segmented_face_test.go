package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 ds_components.go 的 segmentedTrack/segmentFace、官方 ui.Segmented 与 ui.Tester 像素采样、gdLayoutControl
 * [OUTPUT]: 对外提供分段控件回归：用 SegmentedBase 搭的分段控件（Sessions 的 Agent 标签、Git 的 Split/Unified 图标段）选中段与官方 ui.Segmented 同一块「从轨道里凸起」的芯片（浅色是页面色、深色是按下面），Split/Unified 仍可点
 * [POS]: 2026-10-14 第二轮审计：Git 的 Split/Unified 曾是手搓的无边框控件，Sessions 的 Agent 标签在浅色下选中色比官方深一档
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// TestSegmentedHelpersWearTheOfficialFace pins one face for every segmented
// control: the one built from SegmentedBase by segmentedTrack/segmentFace
// (icons, counts — ui.Segmented takes labels only) paints its chosen segment
// exactly as ui.Segmented does, light and dark.
func TestSegmentedHelpersWearTheOfficialFace(t *testing.T) {
	for _, dark := range []bool{false, true} {
		official, built := 0, 0
		tester := ui.NewTester(func(c *ui.Context) {
			ui.Column(c).FillWidth().Padding(10).Gap(8).Children(func() {
				ui.Segmented(c, &official, "Official A", "Official B").Label("Official")
				tabs := ui.SegmentedBase(c, &built, 2)
				segmentedTrack(c, tabs, "Built")
				tabs.Track.Children(func() {
					for i, name := range []string{"Built A", "Built B"} {
						segmentFace(c, tabs, i, i == built, name).Children(func() { ui.Text(c, name).SingleLine() })
					}
				})
			})
		}, 500, 200)
		tester.SetDark(dark)
		settleFrames(tester, 3)

		a, okA := tester.Find("Official A") // the text: its segment has no label, so step left into the padding
		b, okB := tester.Find("Built A")    // the segment itself: step right of its edge
		if !okA || !okB {
			t.Fatalf("segments missing; texts=%q", tester.Texts())
		}
		want := pixelAt(tester, int(a.X)-5, int(a.Y+a.H/2))
		got := pixelAt(tester, int(b.X)+3, int(b.Y+b.H/2))
		if rgbDiff(want, got) > 3 {
			t.Fatalf("dark=%v: the chosen segment of a SegmentedBase control is %v, ui.Segmented paints %v", dark, got, want)
		}
	}
}

// The Git toolbar's Split/Unified switch is such a control: clicking a side
// chooses it, and the chosen side is the one the diff is laid out in.
func TestDiffLayoutControlIsASegmentedControl(t *testing.T) {
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	shell.router.Push(routeWorkspace)
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester := ui.NewTester(shell.View, 1280, 900)
	settleFrames(tester, 3)
	if _, ok := tester.Find("Diff layout"); !ok {
		t.Fatalf("the layout control is missing its track; texts=%q", tester.Texts())
	}
	for _, split := range []bool{true, false, true} {
		label := map[bool]string{true: "Split", false: "Unified"}[split]
		if shell.git.splitLayout == split {
			continue
		}
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
		settleFrames(tester, 2)
		if shell.git.splitLayout != split {
			t.Fatalf("clicking %s left splitLayout=%v", label, shell.git.splitLayout)
		}
	}
}
