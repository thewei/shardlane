package herdr

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSlugifyInstanceNameMatchesWorkspacePolicy(t *testing.T) {
	cases := map[string]string{
		"My Workspace": "my-workspace",
		"  A/B C  ":    "a-b-c",
		"项目":           "",
		"Dev_01":       "dev-01",
	}
	for input, want := range cases {
		if got := slugifyInstanceName(input); got != want {
			t.Fatalf("slugifyInstanceName(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestWriteDisplayNameUsesSessionMetadata(t *testing.T) {
	home, err := os.MkdirTemp("/tmp", "shardlane-instance-meta-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(home) })
	t.Setenv("HOME", home)
	if err := writeDisplayName("work", "工作空间"); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(home, ".config", "herdr", "sessions", "work", "workspace.json")
	if _, err := os.Stat(path); err != nil {
		t.Fatal(err)
	}
	if got := readDisplayName("work"); got != "工作空间" {
		t.Fatalf("display name = %q", got)
	}
}

func TestDeleteInstanceRejectsDefaultBeforeCLI(t *testing.T) {
	if err := NewManager().DeleteInstance("default"); err == nil {
		t.Fatal("expected default workspace deletion to be rejected")
	}
}
