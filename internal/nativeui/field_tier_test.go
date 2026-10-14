package nativeui

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

/**
 * [INPUT]: 依赖 nativeui 包内非测试源码里 ui.TextInput 调用链上的 .Height(…) 字面量
 * [OUTPUT]: 对外提供单行输入框高度回归：显式高度只来自 Controls() 的档位，业务代码不再键入 26 / 28 / 30
 * [POS]: 2026-10-14 同组件同尺寸审计的守卫测试；Commit 主题输入框在行内编辑区是 28、在 Commit 弹窗里是 30，描述输入框又是 26
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// fieldHeightLiteral matches a single-line input whose call chain types a
// height as a number.
var fieldHeightLiteral = regexp.MustCompile(`ui\.TextInput\(c[^\n]*\.Height\(\d`)

// TestFieldHeightComesFromTheTiers pins the shared-component rule for
// single-line inputs: when a height is stated it is a Controls() tier, so the
// same commit subject is the same height in the inline composer and in the
// Commit dialog. Multi-line areas size to their content and are not covered.
func TestFieldHeightComesFromTheTiers(t *testing.T) {
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	fields := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		fields += strings.Count(string(src), "ui.TextInput(c")
		for _, line := range strings.Split(string(src), "\n") {
			if fieldHeightLiteral.MatchString(line) {
				t.Errorf("%s: %q types an input height; use a Controls() tier", name, strings.TrimSpace(line))
			}
		}
	}
	if fields < 6 {
		t.Fatalf("found only %d TextInput call sites; the scan no longer sees the sources", fields)
	}
}
