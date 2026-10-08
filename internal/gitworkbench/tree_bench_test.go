package gitworkbench

import (
	"fmt"
	"testing"
	"time"
)

// BenchmarkChangedTreeFilter1K measures the plan §35 target: 1000 changed
// files tree filter/rebuild after snapshot ready, target < 16 ms.
func BenchmarkChangedTreeFilter1K(b *testing.B) {
	files := make([]ChangeFile, 1000)
	for i := range files {
		files[i] = ChangeFile{Path: fmt.Sprintf("dir%d/sub%d/file%d.go", i%23, i%7, i), Status: StatusModified}
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		tree := BuildTree(files, true)
		FilterTree(tree, "file5")
	}
}

func TestChangedTreeFilter1KUnder16MS(t *testing.T) {
	files := make([]ChangeFile, 1000)
	for i := range files {
		files[i] = ChangeFile{Path: fmt.Sprintf("dir%d/sub%d/file%d.go", i%23, i%7, i), Status: StatusModified}
	}
	start := time.Now()
	tree := BuildTree(files, true)
	FilterTree(tree, "file5")
	Flatten(tree, nil)
	elapsed := time.Since(start)
	if elapsed > 16*time.Millisecond {
		t.Fatalf("1000-file tree filter/rebuild took %v (target <16ms)", elapsed)
	}
}
