package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

func TestMachineMenuSuffix(t *testing.T) {
	machine := &herdr.Machine{ID: "p1", Label: "workbox", Enabled: true}
	disabled := &herdr.Machine{ID: "p2", Label: "gpu", Enabled: false}

	cases := []struct {
		name    string
		machine *herdr.Machine
		state   herdr.MachineState
		known   bool
		want    string
	}{
		{"unknown stays quiet", machine, herdr.MachineState{}, false, ""},
		{"reachable", machine, herdr.MachineState{Reachable: true}, true, " — reachable"},
		{"unreachable", machine, herdr.MachineState{}, true, " — unreachable"},
		{"attention without detail", machine, herdr.MachineState{Attention: true}, true, " — needs attention"},
		{"attention with detail", machine, herdr.MachineState{Attention: true, Message: "ssh: permission denied (publickey)"}, true, " — ssh: permission denied (publickey)"},
		{"disabled wins", disabled, herdr.MachineState{Reachable: true}, true, " — disabled"},
	}
	for _, tc := range cases {
		if got := machineMenuSuffix(tc.machine, tc.state, tc.known); got != tc.want {
			t.Fatalf("%s: suffix = %q, want %q", tc.name, got, tc.want)
		}
	}
}

func TestMachineMenuSuffixTruncatesLongErrors(t *testing.T) {
	long := make([]rune, 120)
	for i := range long {
		long[i] = 'x'
	}
	suffix := machineMenuSuffix(&herdr.Machine{ID: "p", Enabled: true}, herdr.MachineState{Attention: true, Message: string(long)}, true)
	runes := []rune(suffix)
	// " — " + 45 kept runes + ellipsis keeps platform menus readable.
	if len(runes) != 3+45+1 {
		t.Fatalf("suffix length = %d", len(runes))
	}
	if runes[len(runes)-1] != '…' {
		t.Fatalf("truncated suffix must end with an ellipsis: %q", suffix)
	}
}
