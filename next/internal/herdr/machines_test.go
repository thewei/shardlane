package herdr

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestMachineInstanceKeyRoundTrip(t *testing.T) {
	key := MachineInstanceKey("prof-1")
	if key != "machine/prof-1" {
		t.Fatalf("key = %q", key)
	}
	ref := ParseRef(key)
	if !ref.IsRemote() || ref.Machine != "prof-1" || ref.Session != "" {
		t.Fatalf("ParseRef(machine key) = %+v", ref)
	}
	local := ParseRef("work")
	if local.IsRemote() || local.Session != "work" {
		t.Fatalf("ParseRef(local key) = %+v", local)
	}
	if got := ParseRef("").Session; got != defaultSession {
		t.Fatalf("empty key should normalize to default session, got %q", got)
	}
}

func TestGuardLocalInstanceRejectsMachineKeys(t *testing.T) {
	if err := guardLocalInstance(MachineInstanceKey("prof-1"), "starting a Herdr session"); err == nil {
		t.Fatal("expected machine key to be rejected")
	}
	if err := guardLocalInstance("work", "starting a Herdr session"); err != nil {
		t.Fatalf("local session should pass the guard: %v", err)
	}
	if NewManager().EnsureRunning(MachineInstanceKey("prof-1")) == nil {
		t.Fatal("EnsureRunning must reject machine keys")
	}
	if _, err := NewManager().RenameInstance(MachineInstanceKey("prof-1"), "x"); err == nil {
		t.Fatal("RenameInstance must reject machine keys")
	}
	if NewManager().DeleteInstance(MachineInstanceKey("prof-1")) == nil {
		t.Fatal("DeleteInstance must reject machine keys")
	}
}

func TestDecodeMachineListShapes(t *testing.T) {
	// Bare array with aliased field names.
	doc := []map[string]any{{
		"profile_id":     "p1",
		"label":          "Build machine",
		"ssh_target":     "deploy@workbox",
		"remote_session": "agents",
		"enabled":        false,
	}}
	raw, _ := json.Marshal(doc)
	machines := decodeMachineList(raw)
	if len(machines) != 1 {
		t.Fatalf("machines = %d", len(machines))
	}
	m := machines[0]
	if m.ID != "p1" || m.Label != "Build machine" || m.Target != "deploy@workbox" || m.Session != "agents" || m.Enabled {
		t.Fatalf("machine = %+v", m)
	}

	// Wrapped object, missing enabled defaults to enabled, label falls back
	// to the target.
	wrapped := map[string]any{"machines": []any{map[string]any{
		"id":     "p2",
		"target": "gpu-box",
	}}}
	raw, _ = json.Marshal(wrapped)
	machines = decodeMachineList(raw)
	if len(machines) != 1 || machines[0].ID != "p2" || !machines[0].Enabled || machines[0].Label != "gpu-box" {
		t.Fatalf("machines = %+v", machines)
	}

	// Documents without ids are skipped; garbage decodes empty.
	if got := decodeMachineList([]byte(`[{"label":"no id"}]`)); len(got) != 0 {
		t.Fatalf("expected no machines without ids, got %+v", got)
	}
	if got := decodeMachineList([]byte("not json")); len(got) != 0 {
		t.Fatalf("garbage should decode to no machines, got %+v", got)
	}
}

func TestDecodeMachineStatesShapes(t *testing.T) {
	doc := []map[string]any{
		{"profile_id": "p1", "reachable": true},
		{"profile_id": "p2", "reachable": false, "error": "auth required"},
		{"profile_id": "p3", "state": "reachable"},
		{"profile_id": "p4", "state": "attention (! auth)"},
		{"profile_id": "p5"},
	}
	raw, _ := json.Marshal(doc)
	states := decodeMachineStates(raw)
	if len(states) != 5 {
		t.Fatalf("states = %d", len(states))
	}
	if !states["p1"].Reachable || states["p1"].Attention {
		t.Fatalf("p1 = %+v", states["p1"])
	}
	if states["p2"].Reachable || !states["p2"].Attention || states["p2"].Message != "auth required" {
		t.Fatalf("p2 = %+v", states["p2"])
	}
	if !states["p3"].Reachable {
		t.Fatalf("p3 = %+v", states["p3"])
	}
	if !states["p4"].Attention || states["p4"].Reachable {
		t.Fatalf("p4 = %+v", states["p4"])
	}
	if states["p5"] != (MachineState{}) {
		t.Fatalf("unknown fact must stay zero state, p5 = %+v", states["p5"])
	}
}

func TestBridgeSocketPaths(t *testing.T) {
	home, err := os.MkdirTemp("/tmp", "shardlane-bridge-paths-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(home) })
	t.Setenv("HOME", home)

	got := bridgeSocketPath("deploy@workbox", "")
	if want := filepath.Join(home, ".shardlane", "ssh-bridges", "deploy_workbox_default.sock"); got != want {
		t.Fatalf("bridgeSocketPath = %q, want %q", got, want)
	}
	named := bridgeSocketPath("Deploy@WorkBox", "agents")
	if named == got {
		t.Fatal("different (target, session) pairs must not share a bridge socket")
	}
	// The slug is sanitized: no separators survive.
	if base := filepath.Base(named); base != "deploy_workbox_agents.sock" {
		t.Fatalf("slug = %q", base)
	}
}

func TestRemoteSocketPathMirrorsLocalLayout(t *testing.T) {
	if got, want := remoteSocketPath("/Users/deploy", ""), "/Users/deploy/.config/herdr/herdr.sock"; got != want {
		t.Fatalf("remoteSocketPath(default) = %q, want %q", got, want)
	}
	if got, want := remoteSocketPath("/Users/deploy", "agents"), "/Users/deploy/.config/herdr/sessions/agents/herdr.sock"; got != want {
		t.Fatalf("remoteSocketPath(named) = %q, want %q", got, want)
	}
}
