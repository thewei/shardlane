package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/agent"
)

func TestAgentNotificationIDRoundTrip(t *testing.T) {
	cases := []struct {
		instance string
		terminal string
	}{
		{"default", "term-1"},
		{"w44", "tty-abc123"},
		{"instance-with-dashes", "t-1"},
	}
	for _, tc := range cases {
		id := agentNotificationID(tc.instance, tc.terminal)
		key, ok := parseAgentNotificationID(id)
		if !ok {
			t.Fatalf("parse failed for %q", id)
		}
		want := agent.AgentKey{InstanceID: tc.instance, TerminalID: tc.terminal}
		if key != want {
			t.Fatalf("roundtrip mismatch: got %+v want %+v", key, want)
		}
	}
}

func TestParseAgentNotificationIDRejectsForeignShapes(t *testing.T) {
	foreign := []string{
		"",
		"other-app/instance/terminal",
		agentNotificationIDPrefix,
		agentNotificationIDPrefix + "instance-only",
		agentNotificationIDPrefix + "/terminal",
		agentNotificationIDPrefix + "instance/",
		agentNotificationIDPrefix + "instance/t1/more",
	}
	for _, id := range foreign {
		if _, ok := parseAgentNotificationID(id); ok {
			t.Fatalf("foreign ID %q must fail closed", id)
		}
	}
}

func TestAgentNotificationGroupPerWorkspace(t *testing.T) {
	if got := agentNotificationGroup("w1"); got != "shardlane-workspace/w1" {
		t.Fatalf("unexpected group %q", got)
	}
}

func TestRouteAgentNotificationNavigatesToOwningPane(t *testing.T) {
	shell := workbenchTestShell(t)
	shell.router.Replace(routeHistory)

	id := agentNotificationID("default", "term-2")
	shell.routeAgentNotification(id)

	if shell.selectedPaneID != "p2" {
		t.Fatalf("click must land on the agent's pane, got %q", shell.selectedPaneID)
	}
	if shell.router.Path() != routeWorkspace {
		t.Fatalf("click must route to the workspace, got %q", shell.router.Path())
	}
	card, ok := shell.workbench.directory.Get(agent.AgentKey{InstanceID: "default", TerminalID: "term-2"})
	if !ok || card.Unread {
		t.Fatalf("click must record the visit and clear unread: %+v ok=%v", card, ok)
	}
}

func TestRouteAgentNotificationStaleAgentFailsClosed(t *testing.T) {
	shell := workbenchTestShell(t)
	shell.router.Replace("/settings")

	// An agent that has left the projection: no navigation, recoverable toast.
	shell.routeAgentNotification(agentNotificationID("default", "term-gone"))
	if shell.router.Path() != "/settings" {
		t.Fatalf("stale click must not navigate, got %q", shell.router.Path())
	}
	if shell.pendingToast != "Agent has ended" {
		t.Fatalf("stale click must surface a recoverable toast, got %q", shell.pendingToast)
	}

	// Foreign IDs stay silent — other apps' notifications are not ours.
	shell.pendingToast = ""
	shell.routeAgentNotification("some-other-app/w1/p1")
	if shell.pendingToast != "" {
		t.Fatalf("foreign ID must be ignored, got toast %q", shell.pendingToast)
	}
}
