package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// fakeDockPlatform records all SetBadge invocations.
type fakeDockPlatform struct {
	badges []string
}

func (f *fakeDockPlatform) SetBadge(text string) {
	f.badges = append(f.badges, text)
}

// TestDockBadgeTextLogic pins P12 invariants: Dock badge = NeedsAttention + ReviewPending.
// Working agents do NOT increase the badge count, and zero clears the badge to "".
func TestDockBadgeTextLogic(t *testing.T) {
	cases := []struct {
		name      string
		snapshot  StatusCenterSnapshot
		wantBadge string
	}{
		{
			name:      "empty",
			snapshot:  StatusCenterSnapshot{},
			wantBadge: "",
		},
		{
			name: "working only does not show badge",
			snapshot: StatusCenterSnapshot{
				Working: 5,
			},
			wantBadge: "",
		},
		{
			name: "needs attention only",
			snapshot: StatusCenterSnapshot{
				NeedsAttention: 2,
				Working:        3,
			},
			wantBadge: "2",
		},
		{
			name: "review pending only",
			snapshot: StatusCenterSnapshot{
				ReviewPending: 4,
				Working:       1,
			},
			wantBadge: "4",
		},
		{
			name: "both attention and review sum up",
			snapshot: StatusCenterSnapshot{
				NeedsAttention: 3,
				ReviewPending:  2,
				Working:        8,
			},
			wantBadge: "5",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := DockBadgeText(tc.snapshot)
			if got != tc.wantBadge {
				t.Fatalf("expected badge %q, got %q", tc.wantBadge, got)
			}
		})
	}
}

// TestDockControllerDeduplication pins that duplicate badge values are not spammed.
func TestDockControllerDeduplication(t *testing.T) {
	platform := &fakeDockPlatform{}
	controller := newDockController(platform)

	snap1 := StatusCenterSnapshot{NeedsAttention: 2}
	snap2 := StatusCenterSnapshot{NeedsAttention: 2, Working: 5} // badge text still "2"
	snap3 := StatusCenterSnapshot{NeedsAttention: 0}             // badge text clears to ""

	controller.update(snap1)
	controller.update(snap2)
	controller.update(snap3)

	if len(platform.badges) != 2 {
		t.Fatalf("expected 2 platform calls due to deduplication, got %d: %v", len(platform.badges), platform.badges)
	}
	if platform.badges[0] != "2" || platform.badges[1] != "" {
		t.Fatalf("unexpected badge sequence: %v", platform.badges)
	}
}

// TestShellDockBadgeProjectionIntegration pins shell projection linkage to dock.
func TestShellDockBadgeProjectionIntegration(t *testing.T) {
	shell := NewShell()
	platform := &fakeDockPlatform{}
	shell.AttachDock(platform)

	// Update projection with 1 blocked (attention) + 1 working
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "t1", PaneID: "p1", Status: "blocked"},
			{TerminalID: "t2", PaneID: "p2", Status: "working"},
		},
	}
	shell.reconcileWorkbench()
	shell.updateDockBadge()

	if len(platform.badges) != 1 || platform.badges[0] != "1" {
		t.Fatalf("expected 1 badge call with '1', got %v", platform.badges)
	}
}
