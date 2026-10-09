package gitworkbench

import "testing"

/**
 * [INPUT]: BuildTree, FilterTree, Flatten and empty/unmatched search inputs
 * [OUTPUT]: Proves unmatched Git file filters are valid empty trees, never a crash
 * [POS]: Git tree-domain regression for UI-007 filtered-empty-state recovery
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestFlattenUnmatchedTreeIsEmpty(t *testing.T) {
	tree := BuildTree([]ChangeFile{{Path: "src/main.go", Status: StatusModified}}, true)
	filtered := FilterTree(tree, "does-not-exist")
	if filtered != nil {
		t.Fatalf("unexpected match: %+v", filtered)
	}
	if got := Flatten(filtered, nil); len(got) != 0 {
		t.Fatalf("unmatched tree has %d visible rows, want zero", len(got))
	}
	if FilterTree(nil, "anything") != nil {
		t.Fatal("nil tree should stay nil when filtered")
	}
	if got := Flatten(nil, map[string]bool{"src": true}); len(got) != 0 {
		t.Fatalf("nil tree produced %d rows", len(got))
	}
}
