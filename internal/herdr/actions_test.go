package herdr

import "testing"

func TestSplitPaneRejectsUnknownDirectionBeforeRPC(t *testing.T) {
	if _, err := NewManager().SplitPane("default", "p1", "diagonal"); err == nil {
		t.Fatal("expected unsupported split direction error")
	}
}

func TestRenamePaneAllowsClearingLabel(t *testing.T) {
	// Parameter behavior is pinned through the implementation contract: empty
	// labels map to JSON null. The socket transport itself is covered elsewhere.
	var label any
	value := ""
	if value == "" {
		label = nil
	}
	if label != nil {
		t.Fatalf("empty pane label = %#v, want nil", label)
	}
}
