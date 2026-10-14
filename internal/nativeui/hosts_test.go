package nativeui

import (
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

func TestSSHCommandLine(t *testing.T) {
	cases := []struct {
		alias string
		want  string
	}{
		{"web", "ssh web\n"},
		{"my_server-1.example.com", "ssh my_server-1.example.com\n"},
		{"deploy@web", "ssh deploy@web\n"},
		// Quoted Host patterns carry spaces; the line must keep them one word.
		{"my host", "ssh 'my host'\n"},
		{"it's", "ssh 'it'\\''s'\n"},
	}
	for _, tc := range cases {
		if got := sshCommandLine(tc.alias); got != tc.want {
			t.Fatalf("sshCommandLine(%q) = %q, want %q", tc.alias, got, tc.want)
		}
	}
}

func TestSafeAlias(t *testing.T) {
	for _, alias := range []string{"web", "a.b-c_d@e", "0"} {
		if !safeAlias(alias) {
			t.Fatalf("safeAlias(%q) = false", alias)
		}
	}
	for _, alias := range []string{"", "my host", "a;b", "$(x)", "-o", "a'b"} {
		if safeAlias(alias) {
			t.Fatalf("safeAlias(%q) = true", alias)
		}
	}
}

func TestHostFormDeltaAndValidation(t *testing.T) {
	entry := sshconfig.Entry{Kind: sshconfig.KindHost, Patterns: []string{"web"}, Lines: []string{
		"Host web",
		"  HostName old.example.com",
		"  User deploy",
		"  # kept comment",
	}}
	sets, removes, header := hostFormDelta(entry, hostsFormState{
		alias: "web", hostName: "new.example.com", user: "deploy",
	})
	if header != "web" || len(removes) != 0 {
		t.Fatalf("header = %q, removes = %v", header, removes)
	}
	if len(sets) != 1 || sets[0].Key != "HostName" || sets[0].Value != "new.example.com" {
		t.Fatalf("sets = %+v", sets)
	}
	// Clearing a set directive becomes a removal; a fresh ProxyJump is a set.
	sets, removes, _ = hostFormDelta(entry, hostsFormState{
		alias: "web", proxyJump: "jumphost",
	})
	if len(sets) != 1 || sets[0].Key != "ProxyJump" {
		t.Fatalf("sets = %+v", sets)
	}
	if len(removes) != 2 {
		t.Fatalf("removes = %v, want hostname and user", removes)
	}
	// Validation: port must be numeric and in range.
	if err := validateHostForm(hostsFormState{alias: "web", port: "0"}); err == nil {
		t.Fatal("port 0 must be rejected")
	}
	if err := validateHostForm(hostsFormState{alias: "web", port: "22"}); err != nil {
		t.Fatalf("port 22 rejected: %v", err)
	}
	if err := validateHostForm(hostsFormState{alias: ""}); err == nil ||
		!strings.Contains(err.Error(), "required") {
		t.Fatalf("empty alias: err = %v", err)
	}
}

// TestHostActionRowSharesOneButtonTier pins the host detail's action row: its
// buttons are one size — "Edit File" once sat in the dense tier beside two
// full-size buttons.
func TestHostActionRowSharesOneButtonTier(t *testing.T) {
	config := filepath.Join(t.TempDir(), "config")
	if err := os.WriteFile(config, []byte("Host dev-box\n  HostName 10.0.0.12\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	index, err := sshconfig.IndexPath(config)
	if err != nil {
		t.Fatal(err)
	}
	s := NewShell()
	s.hosts.loaded, s.hosts.mainPath, s.hosts.entries, s.hosts.files = true, index.MainPath, index.Entries, index.Files
	s.hosts.selected = hostEntryKey(index.Entries[0])
	s.router.Replace(routeHosts)
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 3)
	var height float32
	for i, label := range []string{"Connect", "Add Herdr Machine…", "Edit File"} {
		rect, ok := tester.Find(label)
		if !ok {
			t.Fatalf("%q missing from the action row; texts=%q", label, tester.Texts())
		}
		if i == 0 {
			height = rect.H
		} else if math.Abs(float64(rect.H-height)) > 0.5 {
			t.Errorf("%q is %.1f high, Connect %.1f — one row, one button size", label, rect.H, height)
		}
	}
}
