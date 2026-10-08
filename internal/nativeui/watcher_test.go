package nativeui

import (
	"context"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

func TestCollectRuntimeEventBurstCoalescesUntilQuiet(t *testing.T) {
	events := make(chan herdr.RuntimeEvent, 8)
	events <- herdr.RuntimeEvent{Event: "pane.updated"}
	events <- herdr.RuntimeEvent{Event: "layout.updated"}

	start := time.Now()
	membership, streamOpen, ok := collectRuntimeEventBurst(
		context.Background(),
		events,
		herdr.RuntimeEvent{Event: "workspace.updated"},
		10*time.Millisecond,
		80*time.Millisecond,
	)
	if !ok || !streamOpen {
		t.Fatalf("batch result ok=%v streamOpen=%v", ok, streamOpen)
	}
	if membership {
		t.Fatal("non-membership burst reported pane membership change")
	}
	if elapsed := time.Since(start); elapsed < 8*time.Millisecond || elapsed > 70*time.Millisecond {
		t.Fatalf("quiet-window elapsed = %s", elapsed)
	}
}

func TestCollectRuntimeEventBurstTracksPaneMembershipAndClosedStream(t *testing.T) {
	events := make(chan herdr.RuntimeEvent, 4)
	events <- herdr.RuntimeEvent{Event: "pane.created"}
	close(events)

	membership, streamOpen, ok := collectRuntimeEventBurst(
		context.Background(),
		events,
		herdr.RuntimeEvent{Event: "pane.updated"},
		20*time.Millisecond,
		80*time.Millisecond,
	)
	if !ok {
		t.Fatal("closed stream should still produce one final reconciliation batch")
	}
	if streamOpen {
		t.Fatal("closed event stream reported open")
	}
	if !membership {
		t.Fatal("pane membership change was not retained across burst")
	}
}

func TestCollectRuntimeEventBurstHonorsMaxLatency(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	events := make(chan herdr.RuntimeEvent, 64)
	go func() {
		ticker := time.NewTicker(3 * time.Millisecond)
		defer ticker.Stop()
		for i := 0; i < 20; i++ {
			<-ticker.C
			events <- herdr.RuntimeEvent{Event: "pane.updated"}
		}
	}()

	start := time.Now()
	_, _, ok := collectRuntimeEventBurst(
		ctx,
		events,
		herdr.RuntimeEvent{Event: "pane.updated"},
		10*time.Millisecond,
		25*time.Millisecond,
	)
	if !ok {
		t.Fatal("max-latency batch cancelled unexpectedly")
	}
	elapsed := time.Since(start)
	if elapsed < 20*time.Millisecond || elapsed > 60*time.Millisecond {
		t.Fatalf("max-latency elapsed = %s", elapsed)
	}
}
