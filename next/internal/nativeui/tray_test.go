package nativeui

import (
	"bytes"
	"image/png"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// TestTrayTitleTable pins the §17.3 status-first menu-bar title: actionable
// counts only, empty when quiet, never token totals.
func TestTrayTitleTable(t *testing.T) {
	cases := []struct {
		counts TrayCounts
		want   string
	}{
		{TrayCounts{}, ""},
		{TrayCounts{Attention: 2}, "⚠ 2"},
		{TrayCounts{Review: 1}, "✓ 1"},
		{TrayCounts{Working: 3}, "⚡ 3"},
		{TrayCounts{Attention: 2, Review: 1, Working: 3}, "⚠ 2  ✓ 1  ⚡ 3"},
		{TrayCounts{Attention: 0, Review: 0, Working: 1}, "⚡ 1"},
	}
	for _, tc := range cases {
		if got := TrayTitle(tc.counts); got != tc.want {
			t.Fatalf("TrayTitle(%+v) = %q, want %q", tc.counts, got, tc.want)
		}
	}
}

// TestTrayToolTipTable pins the §17.4 shared summary wording: disconnected
// wins over counts, a quiet connected workspace reads Ready.
func TestTrayToolTipTable(t *testing.T) {
	cases := []struct {
		counts    TrayCounts
		connected bool
		want      string
	}{
		{TrayCounts{}, true, "Shardlane — Ready"},
		{TrayCounts{Attention: 2, Review: 1, Working: 3}, true, "Shardlane — 2 need attention, 1 for review, 3 working"},
		{TrayCounts{Working: 2}, true, "Shardlane — 2 working"},
		{TrayCounts{Attention: 1}, true, "Shardlane — 1 needs attention"},
		{TrayCounts{Attention: 5, Review: 5, Working: 5}, false, "Shardlane — Herdr disconnected"},
		{TrayCounts{}, false, "Shardlane — Herdr disconnected"},
	}
	for _, tc := range cases {
		if got := TrayToolTip(tc.counts, tc.connected); got != tc.want {
			t.Fatalf("TrayToolTip(%+v, %t) = %q, want %q", tc.counts, tc.connected, got, tc.want)
		}
	}
}

func trayEntry(terminalID, paneID, title, project string, phase agent.AgentRuntimePhase, attention agent.OperationalState, unread, review, interaction bool) StatusCenterEntry {
	card := agent.AgentCardModel{
		Key:           agent.AgentKey{InstanceID: "inst", TerminalID: terminalID},
		Title:         title,
		ProjectName:   project,
		PaneID:        paneID,
		RuntimePhase:  phase,
		Attention:     attention,
		Unread:        unread,
		ReviewPending: review,
	}
	return StatusCenterEntry{Card: card, HasInteraction: interaction}
}

// TestTrayFingerprintStability pins the §18 fingerprint contract: entry
// order and navigation-irrelevant reshuffles never change it; agent
// enter/leave, status, unread/review, interaction attention, title/project
// identity, and connection state do.
func TestTrayFingerprintStability(t *testing.T) {
	base := StatusCenterSnapshot{NeedsAttention: 1, Working: 1, Entries: []StatusCenterEntry{
		trayEntry("t1", "p1", "Scout", "demo", agent.PhaseWorking, agent.OpNeedsAttention, true, false, false),
		trayEntry("t2", "p2", "Ranger", "other", agent.PhaseWorking, agent.OpWorking, false, false, false),
	}}

	unchanged := TrayFingerprint(base, true)
	reordered := StatusCenterSnapshot{NeedsAttention: 1, Working: 1, Entries: []StatusCenterEntry{
		base.Entries[1], base.Entries[0],
	}}
	if TrayFingerprint(reordered, true) != unchanged {
		t.Fatal("entry order must not change the tray fingerprint")
	}

	for name, mutate := range map[string]func(*StatusCenterSnapshot){
		"agent-leave": func(s *StatusCenterSnapshot) { s.Entries = s.Entries[:1]; s.Working = 0 },
		"status": func(s *StatusCenterSnapshot) {
			s.Entries[1].Card.Attention = agent.OpIdle
			s.Entries[1].Card.RuntimePhase = agent.PhaseIdle
		},
		"unread":   func(s *StatusCenterSnapshot) { s.Entries[1].Card.Unread = true },
		"review":   func(s *StatusCenterSnapshot) { s.Entries[1].Card.ReviewPending = true },
		"identity": func(s *StatusCenterSnapshot) { s.Entries[1].Card.Title = "Renamed" },
		"project":  func(s *StatusCenterSnapshot) { s.Entries[1].Card.ProjectName = "renamed" },
		"interaction": func(s *StatusCenterSnapshot) {
			s.Entries[1].HasInteraction = true
			s.Interactions = 1
		},
	} {
		changed := StatusCenterSnapshot{NeedsAttention: base.NeedsAttention, ReviewPending: base.ReviewPending, Working: base.Working}
		changed.Entries = append(changed.Entries, base.Entries...)
		mutate(&changed)
		if TrayFingerprint(changed, true) == unchanged {
			t.Fatalf("%s must change the tray fingerprint", name)
		}
	}

	if TrayFingerprint(base, false) == unchanged {
		t.Fatal("connection state must change the tray fingerprint")
	}
}

// fakeTrayPlatform records every applied projection for the §18 gating
// integration verification.
type fakeTrayPlatform struct {
	titles []string
	tips   []string
}

func (f *fakeTrayPlatform) SetTitle(title string) { f.titles = append(f.titles, title) }
func (f *fakeTrayPlatform) SetToolTip(tip string) { f.tips = append(f.tips, tip) }

func (f *fakeTrayPlatform) calls() int { return len(f.titles) + len(f.tips) }

// TestStatusTrayAppliesOnlyOnFingerprintChange drives the controller over
// the fake platform: first apply writes, identical snapshots are dropped,
// navigation-only changes (reordered entries, same fingerprint) do not
// rebuild, and real state changes do.
func TestStatusTrayAppliesOnlyOnFingerprintChange(t *testing.T) {
	platform := &fakeTrayPlatform{}
	tray := newStatusTray(platform)

	base := StatusCenterSnapshot{NeedsAttention: 2, ReviewPending: 1, Working: 3, Entries: []StatusCenterEntry{
		trayEntry("t1", "p1", "Scout", "demo", agent.PhaseWorking, agent.OpNeedsAttention, false, false, false),
		trayEntry("t2", "p2", "Ranger", "other", agent.PhaseWorking, agent.OpWorking, false, true, false),
	}}

	tray.apply(base, true)
	if platform.titles[0] != "⚠ 2  ✓ 1  ⚡ 3" || platform.tips[0] != "Shardlane — 2 need attention, 1 for review, 3 working" {
		t.Fatalf("first apply = %q / %q", platform.titles, platform.tips)
	}
	first := platform.calls()

	// Idle re-apply: no rebuild.
	tray.apply(base, true)
	if platform.calls() != first {
		t.Fatalf("identical snapshot rebuilt the tray: %d calls", platform.calls()-first)
	}

	// Navigation-only change: same fingerprint, no rebuild.
	reordered := StatusCenterSnapshot{NeedsAttention: 2, ReviewPending: 1, Working: 3, Entries: []StatusCenterEntry{
		base.Entries[1], base.Entries[0],
	}}
	tray.apply(reordered, true)
	if platform.calls() != first {
		t.Fatalf("reordered snapshot rebuilt the tray: %d calls", platform.calls()-first)
	}

	// A review clears: rebuild with the new wording.
	cleared := StatusCenterSnapshot{NeedsAttention: 2, Working: 3, Entries: []StatusCenterEntry{
		base.Entries[0],
		trayEntry("t2", "p2", "Ranger", "other", agent.PhaseWorking, agent.OpWorking, false, false, false),
	}}
	tray.apply(cleared, true)
	if platform.titles[1] != "⚠ 2  ⚡ 3" || platform.tips[1] != "Shardlane — 2 need attention, 3 working" {
		t.Fatalf("second apply = %q / %q", platform.titles, platform.tips)
	}

	// Disconnection: rebuild with the disconnected wording.
	tray.apply(cleared, false)
	if platform.tips[2] != "Shardlane — Herdr disconnected" {
		t.Fatalf("disconnect tooltip = %q", platform.tips[2])
	}
}

// TestUpdateTrayProjectsShellSnapshot is the minimal Shell-level
// integration: the projection reconcile path feeds the bound tray through
// the same BuildStatusCenterSnapshot the in-window panel consumes.
func TestUpdateTrayProjectsShellSnapshot(t *testing.T) {
	shell := NewShell()
	platform := &fakeTrayPlatform{}
	shell.tray = newStatusTray(platform)
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", Status: "working"},
			{TerminalID: "term-2", PaneID: "p2", Status: "blocked"},
		},
	}
	shell.reconcileWorkbench()
	shell.updateTray()
	if platform.titles[0] != "⚠ 1  ⚡ 1" {
		t.Fatalf("shell tray title = %q", platform.titles)
	}

	// Herdr unavailability re-words the tooltip.
	shell.offline = true
	shell.updateTray()
	if platform.tips[1] != "Shardlane — Herdr disconnected" {
		t.Fatalf("offline tooltip = %q", platform.tips[1])
	}

	// No tray bound: updateTray is a no-op (fallback rule §17).
	shell.tray = nil
	shell.offline = false
	shell.updateTray()
}

// TestTrayIconPNGIsTemplateGlyph verifies the generated menu-bar icon is a
// decodable 32x32 template PNG.
func TestTrayIconPNGIsTemplateGlyph(t *testing.T) {
	data, err := TrayIconPNG()
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		t.Fatalf("icon is not a valid PNG: %v", err)
	}
	if bounds := decoded.Bounds(); bounds.Dx() != 32 || bounds.Dy() != 32 {
		t.Fatalf("icon bounds = %v, want 32x32", bounds)
	}
}
