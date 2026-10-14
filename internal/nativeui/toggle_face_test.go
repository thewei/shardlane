package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/sessiontrash"
)

/**
 * [INPUT]: 依赖 gorexToggleFace、Sessions 工具栏的 Select/Trash 开关与 ui.Tester 像素采样
 * [OUTPUT]: 对外提供开关「开」态回归：文字开关与标题栏图标开关共用同一枚中性底色，不再是蓝色选中色
 * [POS]: 2026-10-14 高亮色统一决策的守卫测试；与 gorex_style.go 的 gorexToggleFace 同生共死
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// TestToggleOnFaceIsTheSharedNeutralChip pins the highlight rule: a button
// that is on wears the neutral chip every toggle shares; the blue selection
// tint belongs to list rows only.
func TestToggleOnFaceIsTheSharedNeutralChip(t *testing.T) {
	toggles := []struct {
		label string
		turn  func(s *Shell, on bool)
	}{
		{"Select sessions", func(s *Shell, on bool) { s.hist.selecting = on }},
		{"Trash", func(s *Shell, on bool) { s.hist.showTrash = on }},
	}
	for _, dark := range []bool{false, true} {
		for _, toggle := range toggles {
			sample := func(on bool) [4]uint8 {
				shell := newMultiAgentHistoryShell(t)
				shell.router.Replace(routeHistory)
				shell.hist.trash = &fakeTrash{items: []sessiontrash.Item{trashItemFixture("1700000000000-aaaaaaaa", "Old one")}} // Select and Trash only exist beside a Trash
				toggle.turn(shell, on)
				tester := ui.NewTester(shell.View, 1200, 700)
				tester.SetDark(dark)
				settleFrames(tester, 3)
				button, ok := tester.Find(toggle.label)
				if !ok {
					t.Fatalf("%q missing; texts=%q", toggle.label, tester.Texts())
				}
				// The right padding: inside the button, clear of icon and text.
				return pixelAt(tester, int(button.X+button.W)-4, int(button.Y+button.H/2))
			}
			off, on := sample(false), sample(true)
			if rgbDiff(off, on) < 12 {
				t.Fatalf("dark=%v %s: on face %v is not distinguishable from off %v", dark, toggle.label, on, off)
			}
			spread := func(p [4]uint8) int {
				lo, hi := int(p[0]), int(p[0])
				for _, v := range p[:3] {
					lo, hi = min(lo, int(v)), max(hi, int(v))
				}
				return hi - lo
			}
			if spread(on) > 4 {
				t.Fatalf("dark=%v %s: on face %v is tinted; a toggle wears the neutral chip, only list rows take the blue selection", dark, toggle.label, on)
			}
		}
	}
}
