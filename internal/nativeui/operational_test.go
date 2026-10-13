package nativeui

import (
	"testing"
)

func TestNormalizeRuntimeStatus(t *testing.T) {
	cases := map[string]operationalState{
		"blocked": opNeedsAttention, "failed": opNeedsAttention, "ERROR": opNeedsAttention,
		"working": opWorking, "pending": opWorking, "launch_pending": opWorking,
		"done": opReadyForReview, "completed": opReadyForReview,
		"idle": opIdle, "": opIdle, "mystery": opIdle,
		"resolved": opResolved,
	}
	for raw, want := range cases {
		if got := normalizeRuntimeStatus(raw); got != want {
			t.Fatalf("normalizeRuntimeStatus(%q) = %v, want %v", raw, got, want)
		}
	}
	if operationalPriority(opNeedsAttention) >= operationalPriority(opWorking) ||
		operationalPriority(opWorking) >= operationalPriority(opReadyForReview) ||
		operationalPriority(opReadyForReview) >= operationalPriority(opIdle) {
		t.Fatal("attention priority order is wrong")
	}
}
