package nativeui

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

/**
 * [INPUT]: 依赖 nativeui 包内非测试源码里 ui.Button / ui.PrimaryButton 调用链上的 .FontSize(…) 字面覆盖
 * [OUTPUT]: 对外提供按钮字号回归：Button 的字号只来自 compactButton / compactPrimaryButton 两档（或默认档），业务代码不在按钮上内联覆盖
 * [POS]: 2026-10-14 同组件同尺寸审计的守卫测试；Commit 两个主按钮曾各自内联 Body / BodySmall，与同排兄弟按钮高度不一
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// buttonFontOverride matches a Button or PrimaryButton that restates a font
// size in its own call chain.
var buttonFontOverride = regexp.MustCompile(`ui\.(?:Button|PrimaryButton)\(c[^\n]*\)\.FontSize\(`)

// TestButtonSizeComesFromTheTiers pins the shared-component rule for buttons:
// a Button is the regular tier by default and the compact tier through
// compactButton / compactPrimaryButton — never a size restated at the call
// site, which is how two buttons in one row ended up two heights.
func TestButtonSizeComesFromTheTiers(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	buttons := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") || name == "ds_components.go" {
			continue // the tiers themselves are where a size is set
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		buttons += strings.Count(string(src), "ui.Button(c") + strings.Count(string(src), "ui.PrimaryButton(c")
		for _, line := range strings.Split(string(src), "\n") {
			if buttonFontOverride.MatchString(line) {
				t.Errorf("%s: %q restates a button font size; use the default tier or compactButton / compactPrimaryButton", name, strings.TrimSpace(line))
			}
		}
	}
	if buttons < 30 {
		t.Fatalf("found only %d Button call sites; the scan no longer sees the sources", buttons)
	}
}
