package nativeui

/**
 * [INPUT]: Files / Git references / Hosts 侧栏在 headless Tester 下的真键盘输入（Tester.Key / Tab；过滤框用 TypeKey）与焦点（Tester.Focused）
 * [OUTPUT]: listNav 的回归测试：方向键移动与边界、←/→ 折叠与层级（叶子上 → 不动）、Enter/Space 打开、Home/End、进页开场聚焦、单一 Tab 停靠点、对话框后焦点回到光标行、过滤框输入不被劫持、侧栏列表不绑定任何字母键（扫源码）、光标脸（键盘用过才亮、Tab 进入即亮、离开/重进复暗、点击不亮）、Hosts 选择跟随
 * [POS]: 与 sidebar_tree_test.go / hosts_test.go 并列的树/列表交互契约；真焦点是光标，所以断言都落在 Focused 与各树自己的状态上
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// pressKey presses one key and lets the frames its move needs run. A row has
// no text input, so macOS sends no text with the key (the platform asks the
// input method only while a text field is focused); Tester.TypeKey would
// deliver the letter into whatever the key just opened.
func pressKey(tester *ui.Tester, mods ui.Modifiers, key ui.Key) {
	tester.Key(mods, key)
	tester.Frame()
	tester.Frame()
}

// navFilesShell opens the Files route over a small project. Rows in tree
// order once alpha is open: alpha (one.txt, two.txt), beta, gamma.txt, zeta.md.
func navFilesShell(t *testing.T) (*Shell, *ui.Tester, string) {
	t.Helper()
	root, err := os.MkdirTemp("", "navfiles")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	for _, dir := range []string{"alpha", "beta"} {
		if err := os.Mkdir(filepath.Join(root, dir), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	for _, file := range []string{"alpha/one.txt", "alpha/two.txt", "gamma.txt", "zeta.md"} {
		if err := os.WriteFile(filepath.Join(root, file), []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	s := rightPanelTestShell(t, root)
	s.showViewFiles()
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 4)
	return s, tester, root
}

func wantCursor(t *testing.T, tester *ui.Tester, label string) {
	t.Helper()
	if !tester.Focused(label) {
		t.Fatalf("cursor is not on %q; texts=%q", label, tester.Texts())
	}
}

// Arriving on /files puts the keyboard in the tree: no click, no Tab.
func TestFilesTreeTakesTheKeyboardOnEntry(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	wantCursor(t, tester, "Folder alpha")
}

// With a file already open the cursor starts on it, not on the first row.
func TestFilesTreeEntryFocusesTheOpenFile(t *testing.T) {
	root, err := os.MkdirTemp("", "navfiles")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	for _, file := range []string{"a.txt", "b.txt", "c.txt"} {
		if err := os.WriteFile(filepath.Join(root, file), []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	s := rightPanelTestShell(t, root)
	s.showViewFiles() // entering the route syncs the root and clears the selection
	s.rightPanel.selectedFilePath = filepath.Join(root, "b.txt")
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 4)
	wantCursor(t, tester, "File b.txt")
}

// A cold start shows a spinner first; the keyboard still lands on the first
// row once the listing arrives, however many frames later.
func TestFilesTreeEntryFocusWaitsForTheListing(t *testing.T) {
	root, err := os.MkdirTemp("", "navfiles")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(root) })
	for _, file := range []string{"a.txt", "b.txt"} {
		if err := os.WriteFile(filepath.Join(root, file), []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	s := rightPanelTestShell(t, root)
	s.showViewFiles()
	s.rightPanel.loadingDirs = map[string]bool{s.rightPanel.currentRoot: true} // the read is still out
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 6)
	if onScreen(tester, "File a.txt") {
		t.Fatal("the listing must not be here yet")
	}
	entries, err := filesview.ListDirect(s.rightPanel.currentRoot, s.rightPanel.currentRoot)
	if err != nil {
		t.Fatal(err)
	}
	delete(s.rightPanel.loadingDirs, s.rightPanel.currentRoot)
	s.rightPanel.cachedDirEntries[s.rightPanel.currentRoot] = entries
	settleFrames(tester, 4)
	wantCursor(t, tester, "File a.txt")
}

func TestFilesTreeArrowsMoveAndStopAtTheEnds(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyUp)
	wantCursor(t, tester, "Folder alpha") // not a ring: ↑ at the top stays
	for _, want := range []string{"Folder beta", "File gamma.txt", "File zeta.md", "File zeta.md"} {
		pressKey(tester, 0, ui.KeyDown)
		wantCursor(t, tester, want)
	}
	pressKey(tester, 0, ui.KeyUp)
	wantCursor(t, tester, "File gamma.txt")
	// MyGo's own focus group would wrap round; a list is not a ring.
	pressKey(tester, 0, ui.KeyEnd)
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "File zeta.md")
	pressKey(tester, 0, ui.KeyHome)
	pressKey(tester, 0, ui.KeyUp)
	wantCursor(t, tester, "Folder alpha")
}

func TestFilesTreeHomeAndEndJumpToTheEnds(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyEnd)
	wantCursor(t, tester, "File zeta.md")
	pressKey(tester, 0, ui.KeyHome)
	wantCursor(t, tester, "Folder alpha")
}

func TestFilesTreeLeftAndRightWalkTheHierarchy(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyRight)
	if !onScreen(tester, "File one.txt") {
		t.Fatalf("→ on a closed folder must open it; texts=%q", tester.Texts())
	}
	wantCursor(t, tester, "Folder alpha")
	pressKey(tester, 0, ui.KeyRight)
	wantCursor(t, tester, "File one.txt") // open folder: → enters it
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "File two.txt")
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Folder alpha") // ← on a file climbs to its folder
	pressKey(tester, 0, ui.KeyLeft)
	if onScreen(tester, "File one.txt") {
		t.Fatalf("← on an open folder must fold it; texts=%q", tester.Texts())
	}
	wantCursor(t, tester, "Folder alpha")
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Folder alpha") // a top-level closed folder has nowhere up
}

// A file opens on Enter or Space — the row's own click — and → leaves it
// alone: a leaf has nothing to unfold.
func TestFilesTreeEnterAndSpaceOpenTheFile(t *testing.T) {
	s, tester, root := navFilesShell(t)
	pressKey(tester, 0, ui.KeyDown)
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "File gamma.txt")
	pressKey(tester, 0, ui.KeyRight)
	if got := s.rightPanel.selectedFilePath; got != "" {
		t.Fatalf("→ on a file must not open it: selected %q", got)
	}
	wantCursor(t, tester, "File gamma.txt")
	pressKey(tester, 0, ui.KeyEnter)
	if got, want := s.rightPanel.selectedFilePath, filepath.Join(root, "gamma.txt"); got != want {
		t.Fatalf("Enter opens the file: selected %q, want %q", got, want)
	}
	wantCursor(t, tester, "File gamma.txt")
	pressKey(tester, 0, ui.KeyDown)
	pressKey(tester, 0, ui.KeySpace)
	if got, want := s.rightPanel.selectedFilePath, filepath.Join(root, "zeta.md"); got != want {
		t.Fatalf("Space opens the file: selected %q, want %q", got, want)
	}
}

// A dialog opened from a row (its menu) takes the keyboard for its field;
// closing it hands the keyboard back to the row, so the arrows keep working
// without a click.
func TestFilesTreeKeyboardSurvivesADialog(t *testing.T) {
	s, tester, root := navFilesShell(t)
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "Folder beta")
	s.filesNewFile(root)
	settleFrames(tester, 3)
	if !s.dialogOpen {
		t.Fatal("the new-file dialog did not open")
	}
	tester.Key(0, ui.KeyEscape)
	settleFrames(tester, 3)
	if s.dialogOpen {
		t.Fatal("Escape must close the dialog")
	}
	wantCursor(t, tester, "Folder beta") // the dialog hands the keyboard back
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "File gamma.txt")
}

// The tree is one Tab stop: Tab leaves it instead of walking its rows, and
// Shift+Tab comes back to the row the cursor left.
func TestFilesTreeIsOneTabStop(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyRight) // open alpha
	pressKey(tester, 0, ui.KeyDown)  // cursor on one.txt
	wantCursor(t, tester, "File one.txt")
	tester.Key(0, ui.KeyTab)
	tester.Frame()
	for _, label := range []string{"Folder alpha", "File one.txt", "File two.txt", "Folder beta", "File gamma.txt", "File zeta.md"} {
		if tester.Focused(label) {
			t.Fatalf("Tab must leave the tree, not step onto %q", label)
		}
	}
	tester.Key(ui.Shift, ui.KeyTab)
	tester.Frame()
	wantCursor(t, tester, "File one.txt")
	if rowFace(t, tester, "File one.txt") == rowFace(t, tester, "File two.txt") {
		t.Fatal("a cursor that arrived by Tab must show")
	}
}

// A click puts the cursor where the user pointed; the keys continue from
// there.
func TestFilesTreeKeysContinueFromAClick(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	if err := tester.Click("gamma.txt"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	wantCursor(t, tester, "File gamma.txt")
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "File zeta.md")
}

// rowFace is the colour a row paints at its right edge, past its text: the
// face — selection, hover, or none — it wears.
func rowFace(t *testing.T, tester *ui.Tester, label string) [3]uint8 {
	t.Helper()
	rect, ok := tester.Find(label)
	if !ok {
		t.Fatalf("no %q; texts=%q", label, tester.Texts())
	}
	c := tester.Image().RGBAAt(int(rect.X+rect.W-6), int(rect.Y+rect.H/2))
	return [3]uint8{c.R, c.G, c.B}
}

// Once the keyboard has used it, the cursor row wears the selection face so
// the keys can see where they are; the rows around it keep the sidebar's own
// surface.
func TestFilesTreeCursorRowWearsTheSelectionFace(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyUp) // wakes the cursor on the first row
	wantCursor(t, tester, "Folder alpha")
	onCursor, off := rowFace(t, tester, "Folder alpha"), rowFace(t, tester, "Folder beta")
	if onCursor == off {
		t.Fatalf("the cursor row looks like its neighbours: %v", onCursor)
	}
	pressKey(tester, 0, ui.KeyDown)
	if moved := rowFace(t, tester, "Folder beta"); moved != onCursor {
		t.Fatalf("the face must follow the cursor: beta %v, want alpha's former %v", moved, onCursor)
	}
}

// A list the mouse just opened looks exactly as it did before keys existed:
// the cursor waits on its first row, unlit, until a key has woken it.
func TestFilesTreeCursorStaysDarkUntilTheKeyboardUsesIt(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	wantCursor(t, tester, "Folder alpha")
	if rowFace(t, tester, "Folder alpha") != rowFace(t, tester, "Folder beta") {
		t.Fatal("a cursor the keyboard has not used must not look selected")
	}
	pressKey(tester, 0, ui.KeyUp)
	if rowFace(t, tester, "Folder alpha") == rowFace(t, tester, "Folder beta") {
		t.Fatal("the first key must wake the cursor")
	}
}

// Leaving the list with the keyboard puts the cursor out; a later mouse
// click there is a click, not a cursor.
func TestFilesTreeCursorGoesDarkWhenTheKeyboardLeaves(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyDown) // beta, lit
	tester.Key(0, ui.KeyTab)
	tester.Frame()
	tester.Frame()
	if err := tester.Click("Folder beta"); err != nil {
		t.Fatal(err)
	}
	tester.Move(1000, 700) // off the row: its hover face is not the cursor's
	settleFrames(tester, 3)
	wantCursor(t, tester, "Folder beta")
	if rowFace(t, tester, "Folder beta") != rowFace(t, tester, "File gamma.txt") {
		t.Fatal("a clicked row must not wear the keyboard's cursor face")
	}
}

// Coming back to the page starts over unlit.
func TestFilesTreeCursorIsDarkAgainOnReEntry(t *testing.T) {
	s, tester, _ := navFilesShell(t)
	pressKey(tester, 0, ui.KeyUp)
	s.showViewTerminal() // no click: the cursor never gets to see the keyboard leave
	settleFrames(tester, 4)
	s.showViewFiles()
	settleFrames(tester, 4)
	wantCursor(t, tester, "Folder alpha")
	if rowFace(t, tester, "Folder alpha") != rowFace(t, tester, "Folder beta") {
		t.Fatal("re-entering the page must not light the cursor")
	}
}

// Letters typed into a filter field above the tree are text: no list binds a
// letter (see TestSidebarListsBindNoLetters).
func TestGitRefsFilterKeepsItsLetters(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	if err := tester.Click("Filter Git references"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.TypeKey(0, ui.KeyJ, "j")
	tester.TypeKey(0, ui.KeyH, "h")
	tester.Frame()
	if shell.gitRefsFilter != "jh" {
		t.Fatalf("filter text = %q, want the typed letters", shell.gitRefsFilter)
	}
}

// The Git references tree: section headers are rows too, ←/→ fold them.
func TestGitRefsTreeKeysFoldSectionsAndWalkRefs(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	if err := tester.Click("Toggle Branches"); err != nil { // fold it, cursor on its header
		t.Fatal(err)
	}
	tester.Frame()
	section := shell.gitFoldKey("section/Branches")
	folder := shell.gitFoldKey("local/feature")
	if !shell.gitTreeClosed[section] {
		t.Fatal("click must fold Branches")
	}
	wantCursor(t, tester, "Toggle Branches")
	pressKey(tester, 0, ui.KeyRight)
	if shell.gitTreeClosed[section] {
		t.Fatal("→ on a closed section must open it")
	}
	pressKey(tester, 0, ui.KeyRight)
	wantCursor(t, tester, "Folder feature") // open section: → enters it
	pressKey(tester, 0, ui.KeyRight)
	wantCursor(t, tester, "Branch login") // open folder: → enters it
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Folder feature")
	pressKey(tester, 0, ui.KeyLeft)
	if !shell.gitTreeClosed[folder] || onScreen(tester, "Branch login") {
		t.Fatalf("← on an open folder must fold it; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Toggle Branches") // folded folder: ← climbs to the section
	pressKey(tester, 0, ui.KeyLeft)
	if !shell.gitTreeClosed[section] {
		t.Fatal("← on an open section header must fold it")
	}
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "Toggle Remotes")
}

// Git has its own window-level j/k for the diff: the references tree must
// not take the keyboard on entry.
func TestGitRefsTreeDoesNotTakeTheKeyboardOnEntry(t *testing.T) {
	_, tester, _ := treeTestShell(t)
	for _, label := range []string{"Toggle Worktrees", "Toggle Branches", "Folder feature", "Branch develop"} {
		if tester.Focused(label) {
			t.Fatalf("the references tree stole the focus onto %q", label)
		}
	}
}

// Hosts: the selection follows the cursor and groups fold like any section.
func TestHostsListKeysSelectAsTheyMove(t *testing.T) {
	s, tester := hostsTestShell(t, groupedHostsConfig, nil)
	wantCursor(t, tester, "alpha-web") // the first host: nothing was selected yet
	if s.hosts.selected != "" {
		t.Fatalf("opening on a row must not select it; selected %q", s.hosts.selected)
	}
	if rowFace(t, tester, "alpha-web") != rowFace(t, tester, "alpha-api") {
		t.Fatal("the parked cursor must not make an unselected host look selected")
	}
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "alpha-api")
	if want := hostEntryKey(s.hosts.entries[1]); s.hosts.selected != want {
		t.Fatalf("selection %q must follow the cursor to %q", s.hosts.selected, want)
	}
	pressKey(tester, 0, ui.KeyLeft) // a host in a group climbs to its header
	wantCursor(t, tester, "Toggle Development")
	if want := hostEntryKey(s.hosts.entries[1]); s.hosts.selected != want {
		t.Fatalf("a header is not selectable: selection moved to %q", s.hosts.selected)
	}
	pressKey(tester, 0, ui.KeyLeft)
	if onScreen(tester, "alpha-web") {
		t.Fatalf("← on an open group header must fold it; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyRight)
	if !onScreen(tester, "alpha-web") {
		t.Fatalf("→ on a closed group header must open it; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyEnd)
	wantCursor(t, tester, "loose-box")
}

// The saved machines below the config list are not part of the last group:
// there is no header above them for ← to climb to.
func TestHostsMachineRowsSitOutsideTheGroups(t *testing.T) {
	s, tester := hostsTestShell(t, groupedHostsConfig, nil)
	inst := herdr.Instance{
		Name: "workbox", DisplayName: "workbox",
		Machine: &herdr.Machine{ID: "m1", Label: "workbox", Target: "deploy@workbox", Enabled: true},
	}
	s.instances = []herdr.Instance{inst}
	s.machineStates = map[string]herdr.MachineState{"m1": {Reachable: true}}
	settleFrames(tester, 3)
	label := s.machineNavLabel(inst)
	pressKey(tester, 0, ui.KeyEnd)
	wantCursor(t, tester, label)
	if s.hosts.selected != hostMachineKeyPrefix+"m1" {
		t.Fatalf("the cursor on a machine selects it; selected %q", s.hosts.selected)
	}
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, label)
}

// The sidebar lists answer the arrows, Home/End, Enter and Space — no letter
// keys, so there is no mode to enter and nothing to learn. A single-letter key
// is only ever part of a Cmd/Super/Alt chord (shortcuts.go is the registry of
// those), with one documented exception: the Diff view's window-level j/k
// hunk walk (the godiff reference) in gd_surface.go. The scan must still see
// that pair, or the pattern has gone stale.
func TestSidebarListsBindNoLetters(t *testing.T) {
	letter := regexp.MustCompile(`ui\.Key[A-Z]\b`)
	chord := regexp.MustCompile(`ui\.(?:Cmd|Super|Alt)\b`)
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	diffHits := 0
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") || name == "shortcuts.go" {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		for i, line := range strings.Split(string(src), "\n") {
			if !letter.MatchString(line) || chord.MatchString(line) {
				continue
			}
			if name == "gd_surface.go" {
				diffHits++
				continue
			}
			t.Errorf("%s:%d binds a bare letter key: %s", name, i+1, strings.TrimSpace(line))
		}
	}
	if diffHits < 2 {
		t.Fatalf("the scan found %d letter bindings in gd_surface.go, want its j/k pair: the pattern is stale", diffHits)
	}
}
