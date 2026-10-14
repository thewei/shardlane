package nativeui

import (
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
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

// hostsTestShell opens the Hosts page over a temp SSH config — never the
// developer's own. "{dir}" in the text stands for the temp directory, so a
// config can Include the extra files written beside it.
func hostsTestShell(t *testing.T, config string, extras map[string]string) (*Shell, *ui.Tester) {
	t.Helper()
	// Not t.TempDir(): its path carries the test's name, and the page prints
	// paths — a test called "…NoConnect" would then "show" Connect.
	dir, err := os.MkdirTemp("", "hosts")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	for name, text := range extras {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(strings.ReplaceAll(text, "{dir}", dir)), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	path := filepath.Join(dir, "config")
	if err := os.WriteFile(path, []byte(strings.ReplaceAll(config, "{dir}", dir)), 0o600); err != nil {
		t.Fatal(err)
	}
	index, err := sshconfig.IndexPath(path)
	if err != nil {
		t.Fatal(err)
	}
	s := NewShell()
	s.hosts.loaded, s.hosts.mainPath, s.hosts.entries, s.hosts.groups, s.hosts.files =
		true, index.MainPath, index.Entries, index.Groups, index.Files
	s.router.Replace(routeHosts)
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 3)
	return s, tester
}

// onScreen reports whether a label is drawn exactly — HasText is a substring
// match, which a path or a longer label can satisfy by accident.
func onScreen(tester *ui.Tester, label string) bool {
	_, ok := tester.Find(label)
	return ok
}

func textsWithPrefix(tester *ui.Tester, prefix string) []string {
	var out []string
	for _, text := range tester.Texts() {
		if strings.HasPrefix(text, prefix) {
			out = append(out, text)
		}
	}
	return out
}

const groupedHostsConfig = `#SCE_GROUP:A:::Development

Host alpha-web
  HostName a.example.com
#SCEGroup A

Host alpha-api
  HostName b.example.com
#SCEGroup A

#SCE_GROUP:B:::Personal

Host nas-box
  HostName c.example.com
#SCEGroup B

Host loose-box
  HostName d.example.com
`

// TestHostsDetailHeadingCarriesTheActions pins the detail's anatomy: one
// heading row with the host, where it is defined and its two actions — the
// primary Connect and the pencil — and none of the old stacked cards, long
// button captions or toggle text.
func TestHostsDetailHeadingCarriesTheActions(t *testing.T) {
	s, tester := hostsTestShell(t, "Host dev-box\n  HostName 10.0.0.12\n  User deploy\n", nil)
	s.hosts.selected = hostEntryKey(s.hosts.entries[0])
	settleFrames(tester, 3)

	for _, want := range []string{"dev-box", "Connect", "Edit", "HostName", "10.0.0.12", "User", "deploy", "Effective config"} {
		if !tester.HasText(want) {
			t.Errorf("host detail lacks %q; texts=%q", want, tester.Texts())
		}
	}
	for _, gone := range []string{"Edit Host…", "Add Herdr Machine…", "Directives", "Show effective config", "included"} {
		if onScreen(tester, gone) {
			t.Errorf("host detail still says %q", gone)
		}
	}
	// Find answers with the label's own rect for Connect and the button's for
	// the pencil; what must agree is the line they sit on.
	connect, ok1 := tester.Find("Connect")
	edit, ok2 := tester.Find("Edit")
	if !ok1 || !ok2 {
		t.Fatal("Connect/Edit missing")
	}
	if math.Abs(float64((connect.Y+connect.H/2)-(edit.Y+edit.H/2))) > 0.5 {
		t.Errorf("the heading's controls share one line: Connect %+v, Edit %+v", connect, edit)
	}
}

// TestHostsHeaderActionsFollowTheSelection pins the title bar's Actions: they
// exist only for a selected host and carry the actions the heading does not.
func TestHostsHeaderActionsFollowTheSelection(t *testing.T) {
	s, tester := hostsTestShell(t, "Host dev-box\n  HostName 10.0.0.12\n\nHost *\n  ServerAliveInterval 30\n", nil)
	if spec := s.hostsRouteHeader(); spec.actions != nil {
		t.Fatal("no host selected: the bar offers no Actions")
	}
	if onScreen(tester, "Actions") {
		t.Fatalf("no host selected, yet the bar shows Actions; texts=%q", tester.Texts())
	}
	s.hosts.selected = hostEntryKey(s.hosts.entries[0])
	settleFrames(tester, 3)
	if err := tester.Click("Actions"); err != nil {
		t.Fatalf("a selected host puts Actions in the bar: %v", err)
	}
	menu := tester.Menu()
	for _, want := range []string{"Connect", "Add Herdr Machine…", "Edit File"} {
		if !containsText(menu, want) {
			t.Errorf("Actions lacks %q; menu=%q", want, menu)
		}
	}
	tester.CloseMenu()

	// A wildcard block is ssh's own business: only the file is the user's.
	s.hosts.selected = hostEntryKey(s.hosts.entries[1])
	settleFrames(tester, 3)
	if err := tester.Click("Actions"); err != nil {
		t.Fatal(err)
	}
	menu = tester.Menu()
	if containsText(menu, "Connect") || containsText(menu, "Add Herdr Machine…") || !containsText(menu, "Edit File") {
		t.Errorf("a wildcard block offers Edit File only; menu=%q", menu)
	}
	tester.CloseMenu()
}

// TestHostsPatternDetailNamesWhyItHasNoConnect pins the non-host detail: no
// Connect, the subtitle says what the block is, and the pencil opens the
// file's raw editor (the structured form is for plain hosts only).
func TestHostsPatternDetailNamesWhyItHasNoConnect(t *testing.T) {
	s, tester := hostsTestShell(t, "Host *\n  ServerAliveInterval 30\n\nMatch exec \"true\"\n  Compression yes\n", nil)
	s.hosts.selected = hostEntryKey(s.hosts.entries[0])
	settleFrames(tester, 3)
	if onScreen(tester, "Connect") || onScreen(tester, "Effective config") {
		t.Fatalf("a wildcard block has no Connect or effective config; texts=%q", tester.Texts())
	}
	if got := hostSubtitle(s.hosts.entries[0]); !strings.HasPrefix(got, "Wildcard pattern · ") {
		t.Errorf("wildcard subtitle = %q", got)
	}
	if got := hostSubtitle(s.hosts.entries[1]); !strings.HasPrefix(got, "Match rule · ") {
		t.Errorf("match subtitle = %q", got)
	}
	queue := make(chan func(), 2)
	s.uiApplyOverride = func(fn func()) { queue <- fn }
	if err := tester.Click("Edit File"); err != nil {
		t.Fatalf("the pencil of a wildcard block is Edit File: %v", err)
	}
	select {
	case apply := <-queue:
		apply()
	case <-time.After(3 * time.Second):
		t.Fatal("the raw editor never opened")
	}
	settleFrames(tester, 3)
	if !s.hosts.editing || s.hosts.editorFile != s.hosts.entries[0].File {
		t.Errorf("Edit File opens the block's own file in the raw editor; editing=%v file=%q", s.hosts.editing, s.hosts.editorFile)
	}
}

// TestHostsEditInPlace pins the structured edit: the directives list turns
// into inputs, Cancel/Save replace Connect/Edit, a clean Cancel leaves at
// once and a dirty one asks first.
func TestHostsEditInPlace(t *testing.T) {
	s, tester := hostsTestShell(t, "Host dev-box\n  HostName 10.0.0.12\n  User deploy\n  IdentityFile ~/.ssh/id_dev\n", nil)
	s.hosts.selected = hostEntryKey(s.hosts.entries[0])
	settleFrames(tester, 3)

	if err := tester.Click("Edit"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	f := &s.hosts.form
	if !f.open || f.hostName != "10.0.0.12" || f.user != "deploy" || f.alias != "dev-box" {
		t.Fatalf("the draft is prefilled from the block: %+v", *f)
	}
	for _, want := range []string{"Cancel", "Save", "Host", "HostName", "User", "Port", "ProxyJump", "IdentityFile", "Comments and other directives stay as they are."} {
		if !tester.HasText(want) {
			t.Errorf("edit mode lacks %q; texts=%q", want, tester.Texts())
		}
	}
	for _, gone := range []string{"Connect", "Effective config"} {
		if onScreen(tester, gone) {
			t.Errorf("edit mode still shows %q", gone)
		}
	}

	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if f.open || !tester.HasText("Connect") {
		t.Fatalf("a clean Cancel leaves edit mode at once; open=%v", f.open)
	}

	if err := tester.Click("Edit"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	f.user = "someone-else"
	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if !f.open || !f.confirmClose || !tester.HasText("Discard unsaved changes?") {
		t.Fatalf("a dirty Cancel asks first; open=%v confirm=%v", f.open, f.confirmClose)
	}
	if err := tester.Click("Keep Editing"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if !f.open || f.confirmClose || f.user != "someone-else" {
		t.Fatalf("Keep Editing keeps the draft; %+v", *f)
	}
	_ = tester.Click("Cancel")
	settleFrames(tester, 3)
	if err := tester.Click("Discard"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if f.open || !tester.HasText("Connect") {
		t.Fatalf("Discard drops the draft and returns to the detail; open=%v", f.open)
	}
}

// TestHostsFormSavesOnReturn pins Return in a field as Save, the way every
// Settings field commits, and that the save lands on the file.
func TestHostsFormSavesOnReturn(t *testing.T) {
	s, tester := hostsTestShell(t, "Host dev-box\n  HostName 10.0.0.12\n", nil)
	s.hosts.selected = hostEntryKey(s.hosts.entries[0])
	settleFrames(tester, 3)
	if err := tester.Click("Edit"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	field, ok := tester.Find("HostName")
	if !ok {
		t.Fatal("HostName input missing")
	}
	tester.ClickAt(field.X+8, field.Y+field.H/2)
	tester.Key(ui.Super, ui.KeyA)
	tester.Type("10.9.9.9")
	tester.Key(0, ui.KeyEnter)

	file := s.hosts.entries[0].File
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		settleFrames(tester, 1)
		if data, err := os.ReadFile(file); err == nil && strings.Contains(string(data), "10.9.9.9") {
			if strings.Contains(string(data), "10.0.0.12") {
				t.Fatalf("the old value survived the save: %q", data)
			}
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	data, _ := os.ReadFile(file)
	t.Fatalf("Return in a field did not save; draft=%+v file=%q", s.hosts.form, data)
}

// TestHostsSidebarGroupsFoldAndCount pins the grouped sidebar: the config's
// own groups become collapsible headers with their member counts, ungrouped
// hosts keep a header of their own, and a fold hides exactly that group's rows.
func TestHostsSidebarGroupsFoldAndCount(t *testing.T) {
	s, tester := hostsTestShell(t, groupedHostsConfig, nil)
	for _, want := range []string{"Development", "Personal", "Ungrouped", "alpha-web", "alpha-api", "nas-box", "loose-box"} {
		if !tester.HasText(want) {
			t.Fatalf("grouped sidebar lacks %q; texts=%q", want, tester.Texts())
		}
	}
	texts := tester.Texts()
	for i, text := range texts {
		if text == "Development" && (i+1 >= len(texts) || texts[i+1] != "2") {
			t.Errorf("the Development header carries its count 2, got %q", texts[i+1:min(i+3, len(texts))])
		}
	}

	if err := tester.Click("Development"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if onScreen(tester, "alpha-web") || onScreen(tester, "alpha-api") {
		t.Errorf("a folded group hides its rows; texts=%q", tester.Texts())
	}
	if !tester.HasText("nas-box") || !tester.HasText("loose-box") {
		t.Errorf("folding one group leaves the others open; texts=%q", tester.Texts())
	}
	if !s.hosts.closed["group:A"] {
		t.Errorf("the fold is remembered by group key; closed=%v", s.hosts.closed)
	}
	if err := tester.Click("Development"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if !tester.HasText("alpha-web") {
		t.Errorf("a second click unfolds; texts=%q", tester.Texts())
	}
}

// TestHostsSidebarStaysFlatWithoutGroups pins that a config with nothing to
// group shows no header at all — and that a symlinked ~/.ssh/config, which the
// index reads back under its target's path, is not mistaken for an include.
func TestHostsSidebarStaysFlatWithoutGroups(t *testing.T) {
	_, tester := hostsTestShell(t, "Host alpha\n  HostName a.example.com\n\nHost beta\n  HostName b.example.com\n", nil)
	if got := textsWithPrefix(tester, "Toggle "); len(got) != 0 {
		t.Fatalf("a plain config has no group headers, got %q", got)
	}
	if !tester.HasText("alpha") || !tester.HasText("beta") {
		t.Fatalf("rows missing; texts=%q", tester.Texts())
	}
}

func TestHostsSidebarSymlinkedConfigStaysFlat(t *testing.T) {
	dir := t.TempDir()
	real := filepath.Join(dir, "real-config")
	if err := os.WriteFile(real, []byte("Host alpha\n  HostName a.example.com\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(dir, "config")
	if err := os.Symlink(real, link); err != nil {
		t.Fatal(err)
	}
	index, err := sshconfig.IndexPath(link)
	if err != nil {
		t.Fatal(err)
	}
	if index.Entries[0].File == index.MainPath {
		t.Skip("the index did not resolve the link here; nothing to pin")
	}
	s := NewShell()
	s.hosts.loaded, s.hosts.mainPath, s.hosts.entries, s.hosts.files = true, index.MainPath, index.Entries, index.Files
	s.router.Replace(routeHosts)
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 3)
	if got := textsWithPrefix(tester, "Toggle "); len(got) != 0 {
		t.Fatalf("a linked config is not an include, yet the sidebar grouped it under %q", got)
	}
	if !tester.HasText("alpha") {
		t.Fatalf("row missing; texts=%q", tester.Texts())
	}
}

// TestHostsSelectedRowWearsTheSelectionFace pins the sidebar selection: the
// selected host's row is tinted, its neighbour is not.
func TestHostsSelectedRowWearsTheSelectionFace(t *testing.T) {
	s, tester := hostsTestShell(t, "Host alpha\n  HostName a.example.com\n\nHost beta\n  HostName b.example.com\n", nil)
	s.hosts.selected = hostEntryKey(s.hosts.entries[0])
	settleFrames(tester, 3)
	picked, ok1 := tester.Find("alpha")
	plain, ok2 := tester.Find("beta")
	if !ok1 || !ok2 {
		t.Fatalf("rows missing; texts=%q", tester.Texts())
	}
	img := tester.Image()
	at := func(r ui.Rect) [4]uint8 {
		c := img.RGBAAt(int(picked.X+80), int(r.Y+r.H/2))
		return [4]uint8{c.R, c.G, c.B, c.A}
	}
	if at(picked) == at(plain) {
		t.Errorf("the selected row looks like its neighbour: %v", at(picked))
	}
}

// TestHostsSidebarGroupsIncludedFiles pins the include-only case: each file
// is a group named by its path and the main file's hosts get a header too.
func TestHostsSidebarGroupsIncludedFiles(t *testing.T) {
	s, tester := hostsTestShell(t, "Include {dir}/extra\n\nHost main-box\n  HostName m.example.com\n", map[string]string{
		"extra": "Host extra-box\n  HostName e.example.com\n",
	})
	if len(s.hosts.files) != 2 {
		t.Fatalf("files = %v", s.hosts.files)
	}
	for _, file := range s.hosts.files {
		if !tester.HasText("Toggle " + compactSearchPath(file)) {
			t.Errorf("no group header for %s; texts=%q", file, tester.Texts())
		}
	}
	for _, want := range []string{"main-box", "extra-box"} {
		if !tester.HasText(want) {
			t.Errorf("row %q missing", want)
		}
	}
}

// TestHostsRowOffersConnectUnderThePointer pins the row's hover icon: a
// connectable alias shows Connect while hovered, a pattern never does, and
// nothing is drawn while the pointer is elsewhere.
func TestHostsRowOffersConnectUnderThePointer(t *testing.T) {
	_, tester := hostsTestShell(t, "Host alpha\n  HostName a.example.com\n\nHost *\n  ServerAliveInterval 30\n", nil)
	if onScreen(tester, "Connect") {
		t.Fatalf("nothing hovered, yet Connect is drawn; texts=%q", tester.Texts())
	}
	row, ok := tester.Find("alpha")
	if !ok {
		t.Fatal("alpha row missing")
	}
	tester.Move(row.X+row.W/2, row.Y+row.H/2)
	settleFrames(tester, 3)
	if !tester.HasText("Connect") {
		t.Fatalf("hovering a connectable row offers Connect; texts=%q", tester.Texts())
	}
	wild, ok := tester.Find("*")
	if !ok {
		t.Fatal("wildcard row missing")
	}
	tester.Move(wild.X+wild.W/2, wild.Y+wild.H/2)
	settleFrames(tester, 3)
	if onScreen(tester, "Connect") {
		t.Fatalf("a pattern row never offers Connect; texts=%q", tester.Texts())
	}
}

// TestHostsMachineDetail pins the machine page: heading with its reachability
// and the one primary action, the facts in a grouped list, and a single short
// pointer to where machines are managed.
func TestHostsMachineDetail(t *testing.T) {
	s, tester := hostsTestShell(t, "Host dev-box\n  HostName 10.0.0.12\n", nil)
	s.instances = []herdr.Instance{{
		Name: "workbox", DisplayName: "workbox",
		Machine: &herdr.Machine{ID: "m1", Label: "workbox", Target: "deploy@workbox", Enabled: true},
	}}
	s.machineStates = map[string]herdr.MachineState{"m1": {Reachable: true}}
	s.hosts.selected = hostMachineKeyPrefix + "m1"
	settleFrames(tester, 3)
	for _, want := range []string{"workbox", "reachable", "Open Workspace", "SSH target", "deploy@workbox", "Profile", "enabled", "Rename or forget this machine from the Session selector."} {
		if !tester.HasText(want) {
			t.Errorf("machine detail lacks %q; texts=%q", want, tester.Texts())
		}
	}
	if onScreen(tester, "Machines are Herdr-managed profiles — add, rename or forget them from the Session selector.") {
		t.Error("the long machine footnote is back")
	}
}
