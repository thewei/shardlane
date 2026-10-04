package conversation

import (
	"reflect"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

func echoRows() []history.TranscriptMessage {
	return []history.TranscriptMessage{
		{Seq: 0, Role: history.RoleUser, Kind: history.MessageText, Text: "run tests"},
		{Seq: 1, Role: history.RoleAssistant, Kind: history.MessageText, Text: "Running."},
		{Seq: 2, Role: history.RoleUser, Kind: history.MessageText, Text: "  run tests  "},
		{Seq: 3, Role: history.RoleAssistant, Kind: history.MessageText, Text: "Done."},
	}
}

// TestReconcilePendingEchoConsumesMatchingUserAfterBaseline pins the §12
// confirmation rule: the matching User row after the baseline consumes the
// echo exactly once, trimmed on both sides.
func TestReconcilePendingEchoConsumesMatchingUserAfterBaseline(t *testing.T) {
	echo := &PendingEcho{Text: "run tests", Baseline: 1, PaneID: "p1"}
	surviving, consumed := ReconcilePendingEcho(echoRows(), echo)
	if !consumed || surviving != nil {
		t.Fatalf("consumed = %v surviving = %#v", consumed, surviving)
	}
	// Exactly once: a consumed (nil) echo makes repeated calls no-ops.
	again, consumedAgain := ReconcilePendingEcho(echoRows(), surviving)
	if consumedAgain || again != nil {
		t.Fatalf("reconcile after consumption = %v %#v", consumedAgain, again)
	}
}

// TestReconcilePendingEchoIgnoresRowsBeforeBaseline pins the order rule:
// only rows at or after the baseline can confirm, so an identical earlier
// User row never consumes a newer submission.
func TestReconcilePendingEchoIgnoresRowsBeforeBaseline(t *testing.T) {
	echo := &PendingEcho{Text: "run tests", Baseline: 3, PaneID: "p1"}
	rows := []history.TranscriptMessage{
		{Seq: 0, Role: history.RoleUser, Kind: history.MessageText, Text: "run tests"},
		{Seq: 1, Role: history.RoleAssistant, Kind: history.MessageText, Text: "Done."},
	}
	surviving, consumed := ReconcilePendingEcho(rows, echo)
	if consumed || surviving != echo {
		t.Fatalf("pre-baseline echo consumed: %v %#v", consumed, surviving)
	}

	// A baseline beyond the projection also stays pending.
	future := &PendingEcho{Text: "run tests", Baseline: 99}
	if surviving, consumed := ReconcilePendingEcho(rows, future); consumed || surviving != future {
		t.Fatalf("future baseline consumed: %v %#v", consumed, surviving)
	}
}

// TestReconcilePendingEchoRequiresUserSemanticRow pins the semantic-source
// rule: assistant rows (and any non-User kind) with identical text never
// confirm — terminal echo text is not evidence.
func TestReconcilePendingEchoRequiresUserSemanticRow(t *testing.T) {
	echo := &PendingEcho{Text: "run tests", Baseline: 0}
	rows := []history.TranscriptMessage{
		{Seq: 0, Role: history.RoleAssistant, Kind: history.MessageText, Text: "run tests"},
		{Seq: 1, Role: history.RoleSystem, Kind: history.MessageMeta, Text: "run tests"},
	}
	if surviving, consumed := ReconcilePendingEcho(rows, echo); consumed || surviving != echo {
		t.Fatalf("non-User row consumed the echo: %v %#v", consumed, surviving)
	}

	// Differing text at or after the baseline leaves the echo pending.
	if surviving, consumed := ReconcilePendingEcho(echoRows(), &PendingEcho{Text: "other prompt", Baseline: 0}); consumed || surviving == nil {
		t.Fatalf("unrelated text consumed the echo: %v %#v", consumed, surviving)
	}
}

// TestReconcilePendingEchoNilNoop pins idempotence at the boundary.
func TestReconcilePendingEchoNilNoop(t *testing.T) {
	if surviving, consumed := ReconcilePendingEcho(echoRows(), nil); consumed || surviving != nil {
		t.Fatalf("nil echo reconcile = %v %#v", consumed, surviving)
	}
	if surviving, consumed := ReconcilePendingEcho(nil, &PendingEcho{Text: "x", Baseline: 0}); consumed || surviving == nil {
		t.Fatalf("empty projection consumed the echo: %v %#v", consumed, surviving)
	}
}

// TestReconcilePendingEchoDoesNotMutateProjection pins the read-only scan.
func TestReconcilePendingEchoDoesNotMutateProjection(t *testing.T) {
	rows := echoRows()
	echo := &PendingEcho{Text: "run tests", Baseline: 1}
	ReconcilePendingEcho(rows, echo)
	want := echoRows()
	if !reflect.DeepEqual(rows, want) {
		t.Fatal("reconciliation mutated the projection")
	}
}
