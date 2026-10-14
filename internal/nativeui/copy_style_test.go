package nativeui

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

/**
 * [INPUT]: 依赖 nativeui 包内非测试源码里 emptyState / workspaceEmptyBanner 的字面标题与菜单项（.Item("…")）的字面标签
 * [OUTPUT]: 对外提供文案大小写回归：空态标题句首大写（专有名词除外），菜单项 Title Case（短介词/冠词小写）——同一类组件只有一种写法
 * [POS]: 2026-10-14 文案统一的守卫测试；新增空态/菜单项时由它拦下大小写漂移
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// emptyTitle matches the literal title a page hands to a shared empty state.
var emptyTitle = regexp.MustCompile(`(?:emptyState\(c|workspaceEmptyBanner\(c, [^,]+), "([^"]+)"`)

// TestEmptyStateTitlesAreSentenceCase pins one voice for the shared empty
// states: "Nothing to commit", never "Nothing to Commit". Dialog and menu
// titles keep Title Case; an empty state reads as a sentence.
func TestEmptyStateTitlesAreSentenceCase(t *testing.T) {
	proper := map[string]bool{"Git": true, "SSH": true}
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	seen := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		for _, match := range emptyTitle.FindAllStringSubmatch(string(src), -1) {
			seen++
			words := strings.Fields(match[1])
			for _, word := range words[1:] {
				if first := word[0]; first >= 'A' && first <= 'Z' && !proper[word] {
					t.Errorf("%s: empty-state title %q capitalizes %q; titles are sentence case", name, match[1], word)
				}
			}
		}
	}
	if seen < 10 {
		t.Fatalf("matched only %d empty-state titles; the pattern no longer finds the call sites", seen)
	}
}

// menuItemLabel matches the literal label of a context-menu or pull-down item.
var menuItemLabel = regexp.MustCompile(`\.Item\("([^"]+)"`)

// TestMenuItemsAreTitleCase pins one voice for menu items: "Copy Branch Name",
// never "Copy branch name". Articles and short prepositions stay lowercase
// (HIG title style); tooltips and empty states are the sentence-case voices.
func TestMenuItemsAreTitleCase(t *testing.T) {
	small := map[string]bool{
		"a": true, "an": true, "the": true, "and": true, "or": true, "in": true, "into": true,
		"to": true, "of": true, "on": true, "as": true, "from": true, "with": true, "for": true, "at": true, "by": true,
	}
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	seen := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		for _, match := range menuItemLabel.FindAllStringSubmatch(string(src), -1) {
			seen++
			for _, word := range strings.Fields(match[1]) {
				if first := word[0]; first >= 'a' && first <= 'z' && !small[word] {
					t.Errorf("%s: menu item %q starts %q with a lowercase letter; menu items are Title Case", name, match[1], word)
				}
			}
		}
	}
	if seen < 40 {
		t.Fatalf("matched only %d menu items; the pattern no longer finds the call sites", seen)
	}
}
