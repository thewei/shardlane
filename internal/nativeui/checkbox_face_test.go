package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 history_select.go 的 selectionMark、官方 ui.Checkbox 与 ui.Tester 像素采样
 * [OUTPUT]: 对外提供勾选框回归：Sessions 多选的勾选标记（只显示状态、点击归行/表头按钮）与官方 ui.Checkbox 的盒子和对勾逐像素一致（深浅、选中/未选中四种）
 * [POS]: 2026-10-14 第二轮审计：Sessions 的勾选标记曾是线描的 task/square 图标，与提交文件列表里官方 Checkbox 的实心强调色盒子不是同一张脸
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// TestSelectionMarkWearsTheCheckboxFace compares the 16×16 box of the
// official Checkbox with selectionMark's, pixel by pixel.
func TestSelectionMarkWearsTheCheckboxFace(t *testing.T) {
	const size = 16
	for _, dark := range []bool{false, true} {
		for _, on := range []bool{false, true} {
			tester := ui.NewTester(func(c *ui.Context) {
				ui.Row(c).Padding(10).Gap(24).Children(func() {
					value := on // the widget writes through its pointer: a per-frame copy
					ui.Checkbox(c, &value, "").Label("Official")
					ui.Row(c).Label("Mine").Children(func() { selectionMark(c, on) })
				})
			}, 160, 60)
			tester.SetDark(dark)
			settleFrames(tester, 3)

			a, okA := tester.Find("Official")
			b, okB := tester.Find("Mine")
			if !okA || !okB {
				t.Fatalf("marks missing; texts=%q", tester.Texts())
			}
			if b.W != size || b.H != size {
				t.Fatalf("selection mark is %.0fx%.0f, want %dx%d", b.W, b.H, size, size)
			}
			ax, ay := int(a.X+(a.W-size)/2), int(a.Y+(a.H-size)/2)
			bx, by := int(b.X), int(b.Y)
			worst := 0
			for dy := 0; dy < size; dy++ {
				for dx := 0; dx < size; dx++ {
					if d := rgbDiff(pixelAt(tester, ax+dx, ay+dy), pixelAt(tester, bx+dx, by+dy)); d > worst {
						worst = d
					}
				}
			}
			if worst > 3 {
				t.Fatalf("dark=%v checked=%v: the selection mark differs from the Checkbox box by %d at its worst pixel", dark, on, worst)
			}
		}
	}
}
