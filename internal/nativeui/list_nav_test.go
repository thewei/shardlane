package nativeui

/**
 * [INPUT]: Files / Git references / Hosts 侧栏在 headless Tester 下的真键盘输入（Tester.Key / Tab；过滤框用 TypeKey）与焦点（Tester.Focused）
 * [OUTPUT]: listNav 的回归测试：Vim 键位移动与边界、h/l 折叠与层级、o/l 打开、gg/G、Ctrl-d/u、Files 行键（a/A/r/d/y）、进页开场聚焦、单一 Tab 停靠点、过滤框输入不被劫持、光标脸（键盘用过才亮、Tab 进入即亮、离开/重进复暗、点击不亮）、Hosts 选择跟随
 * [POS]: 与 sidebar_tree_test.go / hosts_test.go 并列的树/列表交互契约；真焦点是光标，所以断言都落在 Focused 与各树自己的状态上
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"os"
	"path/filepath"
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

// pressText presses a run of plain letters, each as its own key.
func pressText(tester *ui.Tester, keys string) {
	for _, r := range keys {
		pressKey(tester, 0, letterKey(r))
	}
}

func letterKey(r rune) ui.Key {
	return map[rune]ui.Key{
		'a': ui.KeyA, 'd': ui.KeyD, 'g': ui.KeyG, 'h': ui.KeyH, 'j': ui.KeyJ, 'k': ui.KeyK,
		'l': ui.KeyL, 'o': ui.KeyO, 'r': ui.KeyR, 'u': ui.KeyU, 'y': ui.KeyY,
	}[r]
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

func TestFilesTreeJKMoveAndStopAtTheEnds(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressText(tester, "k")
	wantCursor(t, tester, "Folder alpha") // not a ring: k at the top stays
	for _, want := range []string{"Folder beta", "File gamma.txt", "File zeta.md", "File zeta.md"} {
		pressText(tester, "j")
		wantCursor(t, tester, want)
	}
	pressText(tester, "k")
	wantCursor(t, tester, "File gamma.txt")
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "File zeta.md")
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

func TestFilesTreeLAndHWalkTheHierarchy(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressText(tester, "l")
	if !onScreen(tester, "File one.txt") {
		t.Fatalf("l on a closed folder must open it; texts=%q", tester.Texts())
	}
	wantCursor(t, tester, "Folder alpha")
	pressText(tester, "l")
	wantCursor(t, tester, "File one.txt") // open folder: l enters it
	pressText(tester, "j")
	wantCursor(t, tester, "File two.txt")
	pressText(tester, "h")
	wantCursor(t, tester, "Folder alpha") // h on a file climbs to its folder
	pressText(tester, "h")
	if onScreen(tester, "File one.txt") {
		t.Fatalf("h on an open folder must fold it; texts=%q", tester.Texts())
	}
	wantCursor(t, tester, "Folder alpha")
	pressText(tester, "h")
	wantCursor(t, tester, "Folder alpha") // a top-level closed folder has nowhere up
	pressKey(tester, 0, ui.KeyRight)
	if !onScreen(tester, "File one.txt") {
		t.Fatalf("→ must open the folder like l; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyLeft)
	if onScreen(tester, "File one.txt") {
		t.Fatalf("← must fold the folder like h; texts=%q", tester.Texts())
	}
}

func TestFilesTreeLAndOOpenTheFile(t *testing.T) {
	s, tester, root := navFilesShell(t)
	pressText(tester, "jj") // gamma.txt
	pressText(tester, "l")
	if got, want := s.rightPanel.selectedFilePath, filepath.Join(root, "gamma.txt"); got != want {
		t.Fatalf("l on a file opens it: selected %q, want %q", got, want)
	}
	pressText(tester, "j")
	pressText(tester, "o")
	if got, want := s.rightPanel.selectedFilePath, filepath.Join(root, "zeta.md"); got != want {
		t.Fatalf("o opens the file: selected %q, want %q", got, want)
	}
	wantCursor(t, tester, "File zeta.md")
	pressText(tester, "k")
	pressKey(tester, 0, ui.KeyEnter)
	if got, want := s.rightPanel.selectedFilePath, filepath.Join(root, "gamma.txt"); got != want {
		t.Fatalf("Enter opens the file: selected %q, want %q", got, want)
	}
}

func TestFilesTreeGGAndGJumpToTheEnds(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, ui.Shift, ui.KeyG)
	wantCursor(t, tester, "File zeta.md")
	pressText(tester, "gg")
	wantCursor(t, tester, "Folder alpha")
	pressKey(tester, 0, ui.KeyEnd)
	wantCursor(t, tester, "File zeta.md")
	pressKey(tester, 0, ui.KeyHome)
	wantCursor(t, tester, "Folder alpha")
	// A lone g is only the first half of gg: it moves nothing, and a key in
	// between starts the chord over.
	pressText(tester, "jjg")
	wantCursor(t, tester, "File gamma.txt")
	pressText(tester, "j")
	wantCursor(t, tester, "File zeta.md")
	pressText(tester, "g")
	wantCursor(t, tester, "File zeta.md")
}

func TestFilesTreeCtrlDAndCtrlUJumpHalfAPage(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressKey(tester, ui.Ctrl, ui.KeyD)
	wantCursor(t, tester, "File zeta.md") // ten rows overshoot a short tree: stops at the end
	pressKey(tester, ui.Ctrl, ui.KeyU)
	wantCursor(t, tester, "Folder alpha")
}

// The menu's file actions answer on the focused row's keys; the dialogs are
// the menu's own, so Delete still asks.
func TestFilesTreeRowKeysRunTheMenuActions(t *testing.T) {
	s, tester, root := navFilesShell(t)
	gamma := filepath.Join(root, "gamma.txt")
	pressText(tester, "jj")
	wantCursor(t, tester, "File gamma.txt")

	pressText(tester, "a")
	if !s.dialogOpen || s.dialogKind != "files-new-file" || s.dialogTarget != root {
		t.Fatalf("a: dialog %v kind %q target %q, want new file in %q", s.dialogOpen, s.dialogKind, s.dialogTarget, root)
	}
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	tester.Frame()
	if s.dialogOpen {
		t.Fatal("Escape must close the dialog")
	}
	wantCursor(t, tester, "File gamma.txt") // the dialog hands the keyboard back

	pressKey(tester, ui.Shift, ui.KeyA)
	if !s.dialogOpen || s.dialogKind != "files-new-dir" || s.dialogTarget != root {
		t.Fatalf("A: dialog %v kind %q target %q, want new folder in %q", s.dialogOpen, s.dialogKind, s.dialogTarget, root)
	}
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	tester.Frame()

	pressText(tester, "r")
	if !s.dialogOpen || s.dialogKind != "files-rename" || s.dialogTarget != gamma || s.dialogValue != "gamma.txt" {
		t.Fatalf("r: dialog %v kind %q target %q value %q, want rename of %q", s.dialogOpen, s.dialogKind, s.dialogTarget, s.dialogValue, gamma)
	}
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	tester.Frame()

	pressText(tester, "d")
	if !s.confirmOpen || s.confirmKind != "files-delete" || s.confirmTarget != gamma {
		t.Fatalf("d: confirm %v kind %q target %q, want delete of %q", s.confirmOpen, s.confirmKind, s.confirmTarget, gamma)
	}
	if _, err := os.Stat(gamma); err != nil {
		t.Fatalf("d must only ask: %v", err)
	}
	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if _, err := os.Stat(gamma); err != nil {
		t.Fatalf("Cancel must keep the file: %v", err)
	}
}

// New file / folder beside a file lands in its folder, and on a folder row
// inside it.
func TestFilesTreeNewKeyTargetsTheRowsFolder(t *testing.T) {
	s, tester, root := navFilesShell(t)
	pressText(tester, "k") // wakes the cursor on alpha; row keys wait for it
	pressText(tester, "a")
	if s.dialogTarget != filepath.Join(root, "alpha") {
		t.Fatalf("a on a folder: target %q, want inside alpha", s.dialogTarget)
	}
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	tester.Frame()
	pressText(tester, "ll") // open alpha, step onto one.txt
	wantCursor(t, tester, "File one.txt")
	pressText(tester, "a")
	if s.dialogTarget != filepath.Join(root, "alpha") {
		t.Fatalf("a on a file: target %q, want its folder alpha", s.dialogTarget)
	}
}

func TestFilesTreeYCopiesThePath(t *testing.T) {
	out := filepath.Join(t.TempDir(), "clip")
	bin := t.TempDir()
	if err := os.WriteFile(filepath.Join(bin, "pbcopy"), []byte("#!/bin/sh\ncat > \"$CLIP_OUT\"\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("CLIP_OUT", out)
	_, tester, root := navFilesShell(t)
	pressText(tester, "jj")
	pressText(tester, "y")
	got, err := os.ReadFile(out)
	if err != nil {
		t.Fatalf("y must copy through the clipboard tool: %v", err)
	}
	if want := filepath.Join(root, "gamma.txt"); string(got) != want {
		t.Fatalf("clipboard %q, want %q", got, want)
	}
}

// The tree is one Tab stop: Tab leaves it instead of walking its rows, and
// Shift+Tab comes back to the row the cursor left.
func TestFilesTreeIsOneTabStop(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressText(tester, "lj") // open alpha, cursor on one.txt
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
	pressText(tester, "j")
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
	pressText(tester, "k") // wakes the cursor on the first row
	wantCursor(t, tester, "Folder alpha")
	onCursor, off := rowFace(t, tester, "Folder alpha"), rowFace(t, tester, "Folder beta")
	if onCursor == off {
		t.Fatalf("the cursor row looks like its neighbours: %v", onCursor)
	}
	pressText(tester, "j")
	if moved := rowFace(t, tester, "Folder beta"); moved != onCursor {
		t.Fatalf("the face must follow the cursor: beta %v, want alpha's former %v", moved, onCursor)
	}
}

// A list the mouse just opened looks exactly as it did before keys existed:
// the cursor waits on its first row, unlit, and the keys that change things
// do nothing until a motion key has woken it.
func TestFilesTreeCursorStaysDarkUntilTheKeyboardUsesIt(t *testing.T) {
	s, tester, _ := navFilesShell(t)
	wantCursor(t, tester, "Folder alpha")
	if rowFace(t, tester, "Folder alpha") != rowFace(t, tester, "Folder beta") {
		t.Fatal("a cursor the keyboard has not used must not look selected")
	}
	pressText(tester, "a")
	if s.dialogOpen {
		t.Fatal("a acted on a cursor nobody can see")
	}
	pressText(tester, "k")
	if rowFace(t, tester, "Folder alpha") == rowFace(t, tester, "Folder beta") {
		t.Fatal("the first key must wake the cursor")
	}
	pressText(tester, "a")
	if !s.dialogOpen {
		t.Fatal("a must act once the cursor shows")
	}
}

// Leaving the list with the keyboard puts the cursor out; a later mouse
// click there is a click, not a cursor.
func TestFilesTreeCursorGoesDarkWhenTheKeyboardLeaves(t *testing.T) {
	_, tester, _ := navFilesShell(t)
	pressText(tester, "j") // beta, lit
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
	pressText(tester, "k")
	s.showViewTerminal() // no click: the cursor never gets to see the keyboard leave
	settleFrames(tester, 4)
	s.showViewFiles()
	settleFrames(tester, 4)
	wantCursor(t, tester, "Folder alpha")
	if rowFace(t, tester, "Folder alpha") != rowFace(t, tester, "Folder beta") {
		t.Fatal("re-entering the page must not light the cursor")
	}
}

// Letters typed into a filter field above the tree are text, not motions.
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

// The Git references tree: section headers are rows too, h/l fold them.
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
	pressText(tester, "l")
	if shell.gitTreeClosed[section] {
		t.Fatal("l on a closed section must open it")
	}
	pressText(tester, "l")
	wantCursor(t, tester, "Folder feature") // open section: l enters it
	pressText(tester, "l")
	wantCursor(t, tester, "Branch login") // open folder: l enters it
	pressText(tester, "h")
	wantCursor(t, tester, "Folder feature")
	pressText(tester, "h")
	if !shell.gitTreeClosed[folder] || onScreen(tester, "Branch login") {
		t.Fatalf("h on an open folder must fold it; texts=%q", tester.Texts())
	}
	pressText(tester, "h")
	wantCursor(t, tester, "Toggle Branches") // folded folder: h climbs to the section
	pressText(tester, "h")
	if !shell.gitTreeClosed[section] {
		t.Fatal("h on an open section header must fold it")
	}
	pressText(tester, "j")
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
	pressText(tester, "j")
	wantCursor(t, tester, "alpha-api")
	if want := hostEntryKey(s.hosts.entries[1]); s.hosts.selected != want {
		t.Fatalf("selection %q must follow the cursor to %q", s.hosts.selected, want)
	}
	pressText(tester, "h") // a host in a group climbs to its header
	wantCursor(t, tester, "Toggle Development")
	if want := hostEntryKey(s.hosts.entries[1]); s.hosts.selected != want {
		t.Fatalf("a header is not selectable: selection moved to %q", s.hosts.selected)
	}
	pressText(tester, "h")
	if onScreen(tester, "alpha-web") {
		t.Fatalf("h on an open group header must fold it; texts=%q", tester.Texts())
	}
	pressText(tester, "l")
	if !onScreen(tester, "alpha-web") {
		t.Fatalf("l on a closed group header must open it; texts=%q", tester.Texts())
	}
	pressKey(tester, ui.Shift, ui.KeyG)
	wantCursor(t, tester, "loose-box")
}

// The saved machines below the config list are not part of the last group:
// there is no header above them for h to climb to.
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
	pressKey(tester, ui.Shift, ui.KeyG)
	wantCursor(t, tester, label)
	if s.hosts.selected != hostMachineKeyPrefix+"m1" {
		t.Fatalf("the cursor on a machine selects it; selected %q", s.hosts.selected)
	}
	pressText(tester, "h")
	wantCursor(t, tester, label)
}
