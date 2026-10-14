package nativeui

import (
	"slices"
	"testing"

	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

func groupTestEntry(alias, file, group string) sshconfig.Entry {
	return sshconfig.Entry{Kind: sshconfig.KindHost, Patterns: []string{alias}, File: file, Group: group}
}

func groupAliases(g hostGroup) []string {
	out := make([]string, len(g.Entries))
	for i, e := range g.Entries {
		out[i] = e.Alias()
	}
	return out
}

// TestHostGroupsOrderDeclaredThenLooseThenIncluded pins the sidebar sections:
// the config's own groups first (declaration order, empty ones dropped), what
// the main file leaves ungrouped (a marker naming an undeclared group counts
// as none), then one group per Include file, labelled by its path.
func TestHostGroupsOrderDeclaredThenLooseThenIncluded(t *testing.T) {
	const main = "/Users/wilson/.ssh/config"
	const included = "/Users/wilson/.orbstack/ssh/config"
	entries := []sshconfig.Entry{
		groupTestEntry("orb", included, ""),
		groupTestEntry("web", main, "A"),
		groupTestEntry("nas", main, "B"),
		groupTestEntry("loose", main, ""),
		groupTestEntry("api", main, "A"),
		groupTestEntry("ghost", main, "ZZ"),
	}
	declared := []sshconfig.Group{{ID: "A", Name: "Development"}, {ID: "B", Name: "Personal"}, {ID: "C", Name: "Empty"}}

	got := hostGroups(entries, declared, main)
	want := []struct {
		label   string
		aliases []string
	}{
		{"Development", []string{"web", "api"}},
		{"Personal", []string{"nas"}},
		{"Ungrouped", []string{"loose", "ghost"}},
		{"~/.orbstack/ssh/config", []string{"orb"}},
	}
	if len(got) != len(want) {
		t.Fatalf("groups = %d, want %d: %+v", len(got), len(want), got)
	}
	keys := map[string]bool{}
	for i, w := range want {
		if got[i].Label != w.label || !slices.Equal(groupAliases(got[i]), w.aliases) {
			t.Errorf("group %d = %q %v, want %q %v", i, got[i].Label, groupAliases(got[i]), w.label, w.aliases)
		}
		if keys[got[i].Key] {
			t.Errorf("fold key %q repeats", got[i].Key)
		}
		keys[got[i].Key] = true
	}
	if got[3].Tip != included {
		t.Errorf("a file group's tooltip is its full path, got %q", got[3].Tip)
	}
}

// TestHostGroupsLeaveASimpleConfigFlat pins the no-op: nothing declared and
// every entry in the main file means one plain list, not one lonely header.
func TestHostGroupsLeaveASimpleConfigFlat(t *testing.T) {
	const main = "/Users/wilson/.ssh/config"
	entries := []sshconfig.Entry{groupTestEntry("a", main, ""), groupTestEntry("b", main, "")}
	if got := hostGroups(entries, nil, main); got != nil {
		t.Fatalf("flat config grouped as %+v", got)
	}
	// A declared group nobody uses is not a reason to group either.
	if got := hostGroups(entries, []sshconfig.Group{{ID: "A", Name: "Unused"}}, main); got != nil {
		t.Fatalf("an empty declared group grouped the list: %+v", got)
	}
	if got := hostGroups(nil, nil, main); got != nil {
		t.Fatalf("no entries grouped as %+v", got)
	}
}

// TestHostGroupsNameTheMainFileWhenOnlyFilesGroup pins the include-only case:
// the main file's hosts are named by the file, not called "ungrouped" next to
// nothing; and a declared group without a name still gets a header.
func TestHostGroupsNameTheMainFileWhenOnlyFilesGroup(t *testing.T) {
	const main = "/Users/wilson/.ssh/config"
	const included = "/Users/wilson/.ssh/extra"
	entries := []sshconfig.Entry{groupTestEntry("main", main, ""), groupTestEntry("inc", included, "")}
	got := hostGroups(entries, nil, main)
	if len(got) != 2 || got[0].Label != "~/.ssh/config" || got[1].Label != "~/.ssh/extra" {
		t.Fatalf("groups = %+v", got)
	}

	named := hostGroups([]sshconfig.Entry{groupTestEntry("x", main, "A")}, []sshconfig.Group{{ID: "A"}}, main)
	if len(named) != 1 || named[0].Label != "Group" {
		t.Fatalf("a nameless group = %+v", named)
	}
}
