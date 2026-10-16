package nativeui

/**
 * [INPUT]: 官方 MyGo headless Tester、Git 侧栏夹具（分支/标签/贮藏/工作树）与 Files 目录夹具
 * [OUTPUT]: 验证侧栏层级共用的网格（首层对齐分区标题、逐层缩进一格、导向线连续）、分区不再有 "···"、悬停 "+" 与右键菜单、Git/Files 共用头部的项目/分支快速切换、折叠头同一语法与唯一 disclosure chevron
 * [POS]: 侧栏树与头部交互回归；网格常量只在 sidebar_tree.go 一处生效
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// treeTestShell is a diff-mode shell whose reference lists give every tree
// shape: first-level leaves, a folder with two leaves, tags, a stash.
func treeTestShell(t *testing.T) (*Shell, *ui.Tester, string) {
	t.Helper()
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	shell.git.refreshing = true // keep the seeded snapshot
	shell.git.branches = []gitworkbench.Branch{
		{Name: "main", Current: true},
		{Name: "develop"},
		{Name: "feature/login"},
		{Name: "feature/signup"},
	}
	shell.git.tags = []gitworkbench.Tag{{Name: "v1.0"}}
	shell.git.stashes = []gitworkbench.Stash{{Ref: "stash@{0}", Subject: "WIP parser"}}
	shell.git.worktrees = []gitworkbench.Worktree{{Path: root, Branch: "main"}}
	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	tester.Frame()
	return shell, tester, root
}

func near(a, b float32) bool { return a-b < 0.75 && b-a < 0.75 }

func TestTreeGridContract(t *testing.T) {
	if got, want := treeGlyphX(0), treeRowPad+treeChevronW+treeGlyphGap; got != want {
		t.Fatalf("first-level glyph x = %v, want chevron slot + gap = %v", got, want)
	}
	if got := treeGlyphX(3) - treeGlyphX(2); got != TreeIndentWidth {
		t.Fatalf("a level steps %v, want one indent %v", got, float32(TreeIndentWidth))
	}
	if got, want := treeGuideX(1), treeChevronX(1)+treeChevronW/2; got != want {
		t.Fatalf("guide runs at %v, want the middle of the chevron slot %v", got, want)
	}
}

func TestTreeRowHeightFollowsDensity(t *testing.T) {
	shell := NewShell()
	want := map[string][2]float32{"compact": {24, 22}, "": {26, 24}, "comfortable": {30, 28}}
	for density, heights := range want {
		shell.settings.General.Density = density
		if got := shell.sidebarRowHeight(); got != heights[0] {
			t.Fatalf("density %q: workspace row %v, want %v", density, got, heights[0])
		}
		if got := shell.treeRowHeight(); got != heights[1] {
			t.Fatalf("density %q: tree row %v, want %v", density, got, heights[1])
		}
	}
}

// First-level rows start their glyph exactly where the section title starts
// (the title is 12+5 DIP right of that), and every level steps one indent.
func TestGitSidebarFirstLevelSitsUnderTheSectionTitle(t *testing.T) {
	_, tester, _ := treeTestShell(t)
	title, ok := tester.Find("Branches")
	if !ok {
		t.Fatalf("no Branches title; texts=%q", tester.Texts())
	}
	leaf, ok1 := tester.Find("develop")
	folder, ok2 := tester.Find("feature")
	nested, ok3 := tester.Find("login")
	if !ok1 || !ok2 || !ok3 {
		t.Fatalf("rows missing (develop=%v feature=%v login=%v); texts=%q", ok1, ok2, ok3, tester.Texts())
	}
	titleColumn := title.X + treeGlyphW + treeTitleGap
	if !near(leaf.X, titleColumn) || !near(folder.X, titleColumn) {
		t.Fatalf("first-level titles at %.1f/%.1f, want %.1f (glyph under the section title %.1f)", leaf.X, folder.X, titleColumn, title.X)
	}
	if !near(nested.X-leaf.X, TreeIndentWidth) {
		t.Fatalf("nested row steps %.1f, want one indent %v", nested.X-leaf.X, float32(TreeIndentWidth))
	}
}

func rgbDiff(a, b [4]uint8) int {
	d := 0
	for i := 0; i < 3; i++ {
		if a[i] > b[i] {
			d += int(a[i] - b[i])
		} else {
			d += int(b[i] - a[i])
		}
	}
	return d
}

func pixelAt(tester *ui.Tester, x, y int) [4]uint8 {
	img := tester.Image()
	i := img.PixOffset(x, y)
	return [4]uint8{img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3]}
}

// The guide of a nested row runs through its parent's chevron column and
// stays unbroken across stacked rows; first-level rows draw none.
func TestGitSidebarGuidesRunThroughNestedRows(t *testing.T) {
	_, tester, _ := treeTestShell(t)
	title, _ := tester.Find("Branches")
	login, ok := tester.Find("login")
	develop, ok2 := tester.Find("develop")
	if !ok || !ok2 {
		t.Fatalf("rows missing; texts=%q", tester.Texts())
	}
	guideX := int(title.X-treeGlyphX(0)) + int(treeGuideX(0)) // sidebar-left + guide offset
	reference := pixelAt(tester, guideX-4, int(login.Y+login.H/2))
	onGuide := func(y int) bool {
		a, b := pixelAt(tester, guideX-1, y), pixelAt(tester, guideX, y)
		return rgbDiff(a, reference) > 12 || rgbDiff(b, reference) > 12
	}
	rowH := NewShell().treeRowHeight()
	top := int(math.Ceil(float64(login.Y + login.H/2 - rowH/2)))
	for y := top; y < top+int(2*rowH)-1; y++ { // login's whole row and the next one
		if !onGuide(y) {
			t.Fatalf("guide broken at y=%d (rows span %d..%d); the lines must run unbroken across rows", y, top, top+int(2*rowH)-1)
		}
	}
	// A first-level row has no ancestor within the tree, so no guide.
	y := int(develop.Y + develop.H/2)
	if onGuide(y) {
		t.Fatalf("first-level row draws a guide at x=%d", guideX)
	}
}

func TestGitSidebarSectionsHaveNoEllipsisButtons(t *testing.T) {
	_, tester, _ := treeTestShell(t)
	for _, name := range []string{"Worktrees", "Branches", "Tags", "Stashes"} {
		if _, ok := tester.Find(name + " actions"); ok {
			t.Fatalf("%s still has a \"···\" actions button", name)
		}
	}
	if tester.HasText("···") {
		t.Fatalf("a section still draws \"···\"; texts=%q", tester.Texts())
	}
}

func TestGitSectionPlusAppearsUnderThePointerAndOpensTheCreateDialog(t *testing.T) {
	shell, tester, root := treeTestShell(t)
	if _, ok := tester.Find("New branch"); ok {
		t.Fatal("the + must stay hidden until the pointer is on the section")
	}
	title, _ := tester.Find("Branches")
	tester.Move(title.X+4, title.Y+title.H/2)
	tester.Frame()
	tester.Frame()
	if _, ok := tester.Find("New branch"); !ok {
		t.Fatalf("hovering Branches must show its +; texts=%q", tester.Texts())
	}
	if err := tester.Click("New branch"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !shell.dialogOpen || shell.dialogKind != "git-create-branch" || shell.dialogTarget != root || shell.dialogAction != "Create" {
		t.Fatalf("dialog = open %v kind %q target %q action %q", shell.dialogOpen, shell.dialogKind, shell.dialogTarget, shell.dialogAction)
	}
	if shell.gitTreeClosed[shell.gitFoldKey("section/Branches")] {
		t.Fatal("pressing + must not fold the section")
	}
	if !tester.HasText("Create") || !tester.HasText("Branch name") {
		t.Fatalf("create dialog shows Create / Branch name; texts=%q", tester.Texts())
	}
}

func TestGitSectionContextMenusCarryTheCreationActions(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	for _, c := range []struct{ section, item, kind string }{
		{"Branches", "New Branch…", "git-create-branch"},
		{"Tags", "New Tag…", "git-create-tag"},
		{"Worktrees", "Add Worktree…", "git-add-worktree"},
	} {
		shell.dialogOpen = false
		tester.Frame() // the dialog's modal leaves before the next click
		if err := tester.RightClick("Toggle " + c.section); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if menu := tester.Menu(); len(menu) == 0 || !strings.Contains(strings.Join(menu, "|"), c.item) {
			t.Fatalf("%s menu = %q, want %q", c.section, menu, c.item)
		}
		if err := tester.ChooseMenuItem(c.item); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if !shell.dialogOpen || shell.dialogKind != c.kind {
			t.Fatalf("%s: dialog open %v kind %q, want %q", c.item, shell.dialogOpen, shell.dialogKind, c.kind)
		}
	}
	// Remotes only list references: nothing to create.
	if err := tester.RightClick("Toggle Remotes"); err == nil {
		tester.Frame()
		if menu := tester.Menu(); len(menu) != 0 {
			t.Fatalf("Remotes menu = %q, want none", menu)
		}
	}
}

func TestCreateDialogRefusesAChangedRepository(t *testing.T) {
	shell, _, _ := treeTestShell(t)
	shell.openCreateDialog("git-create-tag", "New Tag", "Tag name")
	target := shell.dialogTarget
	shell.git.root = "/somewhere/else"
	shell.submitTextDialog("git-create-tag", target, "v2")
	if shell.pendingToast == "" || shell.git.opBusy {
		t.Fatalf("a create submitted after the repository changed must be refused (toast %q, busy %v)", shell.pendingToast, shell.git.opBusy)
	}
}

func gitOut(t *testing.T, dir string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v: %v\n%s", args, err, out)
	}
	return strings.TrimSpace(string(out))
}

func TestCreateDialogsMakeTheBranchAndTheTag(t *testing.T) {
	dir := t.TempDir()
	gitScript(t, dir,
		[]string{"init", "-q", "-b", "main"},
		[]string{"-c", "user.email=a@b.c", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"},
	)
	shell := gitSurfaceTestShell(t, dir)
	shell.git.rootOf[dir] = dir
	shell.git.refreshing = true
	shell.openCreateDialog("git-create-tag", "New Tag", "Tag name")
	shell.submitTextDialog(shell.dialogKind, shell.dialogTarget, "v9")
	// The Git lanes run on goroutines and headless drops their UI-lane
	// completions, so the repository itself is the arbiter.
	waitFor(t, 5*time.Second, func() bool { return gitOut(t, dir, "tag", "--list", "v9") == "v9" })
	shell.openNewBranchDialog()
	shell.submitTextDialog(shell.dialogKind, shell.dialogTarget, "topic")
	waitFor(t, 5*time.Second, func() bool { return gitOut(t, dir, "branch", "--show-current") == "topic" })
}

// The shared sidebar head: project and branch each open a search pull-down.
func TestSidebarHeadSwitchesProjectsAndKeepsTheGitView(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	other := t.TempDir()
	shell.projection.Projects = append(shell.projection.Projects, herdr.Project{ID: "w2", Label: "elsewhere", CWD: other})
	shell.projection.Tabs = append(shell.projection.Tabs, herdr.Tab{ID: "t2", Label: "main", ProjectID: "w2"})
	shell.projection.Panes = append(shell.projection.Panes, herdr.Pane{ID: "p2", ProjectID: "w2", TerminalID: "term-2", Label: "editor", TabID: "t2", CWD: other})
	shell.git.rootOf[other] = other
	tester.Frame()

	if err := tester.Click("Switch project"); err != nil {
		t.Fatalf("project chip missing: %v; texts=%q", err, tester.Texts())
	}
	tester.Frame()
	tester.Frame()
	if !shell.projectMenuOpen || !tester.HasText("elsewhere") || !tester.HasText("Search projects") {
		t.Fatalf("project pull-down: open %v; texts=%q", shell.projectMenuOpen, tester.Texts())
	}
	if err := tester.Click("elsewhere"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.selectedProjectID != "w2" || shell.projectMenuOpen {
		t.Fatalf("selected %q, menu open %v", shell.selectedProjectID, shell.projectMenuOpen)
	}
	if shell.surface.current() != WorkspaceSurfaceDiff || shell.git.root != other {
		t.Fatalf("surface %v root %q: switching a project from the Git view must stay in it, on the new repository", shell.surface.current(), shell.git.root)
	}
}

func TestSidebarHeadProjectSearchFiltersAndEnterPicksTheFirstMatch(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	other := t.TempDir()
	shell.projection.Projects = append(shell.projection.Projects, herdr.Project{ID: "w2", Label: "elsewhere", CWD: other})
	shell.projection.Tabs = append(shell.projection.Tabs, herdr.Tab{ID: "t2", Label: "main", ProjectID: "w2"})
	shell.projection.Panes = append(shell.projection.Panes, herdr.Pane{ID: "p2", ProjectID: "w2", TerminalID: "term-2", Label: "editor", TabID: "t2", CWD: other})
	shell.git.rootOf[other] = other
	if err := tester.Click("Switch project"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	tester.Type("else")
	tester.Frame()
	if tester.HasText("No matching projects") || !tester.HasText("elsewhere") {
		t.Fatalf("search lost the match; texts=%q", tester.Texts())
	}
	tester.Key(0, ui.KeyEnter)
	tester.Frame()
	if shell.selectedProjectID != "w2" {
		t.Fatalf("Enter must pick the first match; selected %q", shell.selectedProjectID)
	}
}

func TestSidebarHeadBranchPulldownSearchesSwitchesAndCreates(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	// Fold the Branches section so only the pull-down lists branch names.
	shell.gitTreeClosed[shell.gitFoldKey("section/Branches")] = true
	tester.Frame()
	if err := tester.Click("Switch branch"); err != nil {
		t.Fatalf("branch chip missing: %v; texts=%q", err, tester.Texts())
	}
	tester.Frame()
	tester.Frame()
	for _, want := range []string{"Search branches", "develop", "feature/login", "New Branch…"} {
		if !tester.HasText(want) {
			t.Fatalf("branch pull-down missing %q; texts=%q", want, tester.Texts())
		}
	}
	tester.Type("sign")
	tester.Frame()
	if tester.HasText("feature/login") || !tester.HasText("feature/signup") {
		t.Fatalf("search must keep only matches; texts=%q", tester.Texts())
	}
	// The seeded snapshot has local changes, so the switch asks first.
	if err := tester.Click("feature/signup"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.branchMenuOpen || !shell.confirmOpen || shell.confirmKind != "git-switch-branch" || shell.confirmTarget != "feature/signup" {
		t.Fatalf("switch: menu open %v confirm %v %q/%q", shell.branchMenuOpen, shell.confirmOpen, shell.confirmKind, shell.confirmTarget)
	}
	shell.confirmOpen = false
	tester.Frame() // the confirm's modal leaves before the next click
	if err := tester.Click("Switch branch"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if err := tester.Click("New Branch…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.branchMenuOpen || !shell.dialogOpen || shell.dialogKind != "git-create-branch" {
		t.Fatalf("New Branch…: menu open %v dialog %v %q", shell.branchMenuOpen, shell.dialogOpen, shell.dialogKind)
	}
}

func TestSidebarHeadBranchRowsCarryMergeAndDeleteInTheirContextMenu(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	shell.gitTreeClosed[shell.gitFoldKey("section/Branches")] = true
	tester.Frame()
	if err := tester.Click("Switch branch"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if err := tester.RightClick("develop"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	menu := strings.Join(tester.Menu(), "|")
	if !strings.Contains(menu, "Merge into Current…") || !strings.Contains(menu, "Delete Branch…") {
		t.Fatalf("branch row menu = %q", menu)
	}
	if err := tester.ChooseMenuItem("Merge into Current…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !shell.mergeDialogOpen || shell.mergeTarget != "develop" || shell.branchMenuOpen {
		t.Fatalf("merge dialog %v target %q menu open %v", shell.mergeDialogOpen, shell.mergeTarget, shell.branchMenuOpen)
	}
}

// The chip's pull-down opens again after it closed (the 2026-10-06 pin:
// a click-counter toggle used to swallow every click after the first).
func TestSidebarHeadPulldownReopens(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	for round := 1; round <= 2; round++ {
		if err := tester.Click("Switch branch"); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		tester.Frame()
		if !shell.branchMenuOpen || !tester.HasText("Search branches") {
			t.Fatalf("round %d: pull-down did not open; texts=%q", round, tester.Texts())
		}
		tester.ClickAt(900, 700) // outside: the backdrop closes it
		tester.Frame()
		if shell.branchMenuOpen {
			t.Fatalf("round %d: outside click did not close the pull-down", round)
		}
	}
}

func TestSidebarHeadWithoutRepositoryHidesTheBranchChip(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	shell.git.root = ""
	shell.git.snapshot = nil
	tester.Frame()
	if _, ok := tester.Find("Switch branch"); ok {
		t.Fatal("no repository, no branch to switch")
	}
	if _, ok := tester.Find("Switch project"); !ok {
		t.Fatalf("the project chip stays; texts=%q", tester.Texts())
	}
}

// Files uses the same head and the same grid: file titles share the folder
// titles' column, and the old text "Refresh" button is an icon.
func TestFilesSidebarSharesTheHeadAndTheGrid(t *testing.T) {
	dir := t.TempDir()
	if err := os.MkdirAll(filepath.Join(dir, "src"), 0o755); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"README.md", filepath.Join("src", "main.go")} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("x\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	shell := gitSurfaceTestShell(t, dir)
	shell.git.rootOf[dir] = dir
	shell.git.refreshing = true
	shell.router.Push(routeFiles)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	tester.Frame()
	tester.Frame()

	for _, want := range []string{"Switch project", "Switch branch", "Refresh files"} {
		if _, ok := tester.Find(want); !ok {
			t.Fatalf("Files head missing %q; texts=%q", want, tester.Texts())
		}
	}
	if _, ok := tester.Find("Refresh"); ok {
		t.Fatal("the text Refresh button must be gone")
	}
	folder, ok1 := tester.Find("src")
	file, ok2 := tester.Find("README.md")
	if !ok1 || !ok2 {
		t.Fatalf("rows missing; texts=%q", tester.Texts())
	}
	if !near(folder.X, file.X) {
		t.Fatalf("folder title at %.1f, file title at %.1f: siblings share one title column", folder.X, file.X)
	}
	project, _ := tester.Find("demo")
	if !near(folder.X-project.X, treeGlyphW+treeTitleGap) && !near(folder.X, project.X) {
		t.Fatalf("project title at %.1f vs tree titles at %.1f: the head must sit on the grid", project.X, folder.X)
	}
}

// workspaceTreeShell is a Workspace-route shell with two projects, one tab
// holding two panes, and the first pane selected.
func workspaceTreeShell() *Shell {
	s := NewShell()
	s.loading = false
	s.instances = []herdr.Instance{{Name: "default", DisplayName: "Workspace Alpha", Running: true, Default: true}}
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		FocusedPaneID:    "p1",
		Projects: []herdr.Project{
			{ID: "w1", Label: "Project Alpha", TabCount: 1, PaneCount: 2},
			{ID: "w2", Label: "Project Menu", TabCount: 0, PaneCount: 0},
		},
		Tabs: []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "Main Tab", PaneCount: 2}},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term1", ProjectID: "w1", TabID: "t1", Label: "Pane One"},
			{ID: "p2", TerminalID: "term2", ProjectID: "w1", TabID: "t1", Label: "Pane Two"},
		},
	}
	s.selectedProjectID = "w1"
	s.selectedTabID = "t1"
	s.selectedPaneID = "p1"
	s.selected = "pane:p1"
	return s
}

// The Workspace tree is a plain list: no row carries a "…" button. The only
// hover action is a project's "+" (New Tab); everything else is the row's
// right-click menu.
func TestWorkspaceTreeRowsDropTheEllipsisAndProjectsOfferNewTabOnHover(t *testing.T) {
	s := workspaceTreeShell()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	for _, text := range tester.Texts() {
		if strings.Contains(text, "More actions") {
			t.Fatalf("a Workspace tree row still carries a kebab: %q", text)
		}
	}
	if tester.HasText("New tab in Project Menu") {
		t.Fatal("the + belongs under the pointer only")
	}
	row, ok := tester.Find("Project Menu")
	if !ok {
		t.Fatalf("project row missing: %q", tester.Texts())
	}
	tester.Move(row.X+row.W/2, row.Y+row.H/2)
	tester.Frame()
	tester.Frame()
	if !tester.HasText("New tab in Project Menu") {
		t.Fatalf("hovering a project must offer + New Tab: %q", tester.Texts())
	}
	if err := tester.Click("New tab in Project Menu"); err != nil {
		t.Fatal(err)
	}
	if s.selectedProjectID != "w1" {
		t.Fatalf("pressing + also activated the row: selected project %q", s.selectedProjectID)
	}
	tab, _ := tester.Find("Main Tab")
	tester.Move(tab.X+tab.W/2, tab.Y+tab.H/2)
	tester.Frame()
	tester.Frame()
	if tester.HasText("New tab in Main Tab") || tester.HasText("Actions for Main Tab") {
		t.Fatalf("tab rows have no hover action: %q", tester.Texts())
	}
}

// The title bar's secondary actions are one labeled pull-down, not "…".
func TestTitleBarActionsPulldownCarriesTheWorkspaceMenu(t *testing.T) {
	s := workspaceTreeShell()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if err := tester.Click("Actions"); err != nil {
		t.Fatalf("title bar has no Actions pull-down: %v; texts=%q", err, tester.Texts())
	}
	tester.Frame()
	menu := tester.Menu()
	for _, want := range []string{"Split Right", "Rename Pane…", "New Tab", "Refresh Runtime"} {
		if !containsText(menu, want) {
			t.Fatalf("Actions menu %q missing %q", menu, want)
		}
	}
	tester.CloseMenu()
}

// The Actions menu's New Tab is the project row's New Tab (newProjectTab): it
// needs a project the runtime still reports, and offers nothing otherwise.
func TestTitleBarNewTabNeedsAProjectTheRuntimeStillHas(t *testing.T) {
	s := workspaceTreeShell()
	s.selectedProjectID = "gone"
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if err := tester.Click("Actions"); err != nil {
		t.Fatalf("title bar has no Actions pull-down: %v; texts=%q", err, tester.Texts())
	}
	tester.Frame()
	menu := tester.Menu()
	if !containsText(menu, "Refresh Runtime") {
		t.Fatalf("Actions menu did not open: %q", menu)
	}
	if containsText(menu, "New Tab") {
		t.Fatalf("Actions menu offers New Tab for a project that is gone: %q", menu)
	}
	tester.CloseMenu()
}

// TestSectionHeadersWearOneGrammar pins the collapsible header across the
// surfaces that have one: the Git references (left rail), the Changes panes
// (centre column) and the Workspace sections all draw the same row — one
// height, the title the same distance after the chevron slot, one title size.
func TestSectionHeadersWearOneGrammar(t *testing.T) {
	_, git, _ := treeTestShell(t)
	ws := ui.NewTester(rightPanelTestShell(t, t.TempDir()).View, 1200, 800)
	settleFrames(ws, 3)
	probes := []struct {
		name   string
		tester *ui.Tester
		title  string
	}{
		{"git references", git, "Branches"},
		{"changes panes", git, "Unstaged Files"},
		{"changes panes", git, "Staged Files"},
		{"workspace sections", ws, "Agents"},
		{"workspace sections", ws, "Workspace"},
	}
	var want [3]float32
	for i, p := range probes {
		row, ok := p.tester.Find("Toggle " + p.title)
		if !ok {
			t.Fatalf("%s: no header row %q; texts=%q", p.name, "Toggle "+p.title, p.tester.Texts())
		}
		title, ok := p.tester.Find(p.title)
		if !ok {
			t.Fatalf("%s: no title %q", p.name, p.title)
		}
		got := [3]float32{title.X - row.X, row.H, title.H}
		if i == 0 {
			want = got
			continue
		}
		for k, label := range []string{"title offset", "row height", "title height"} {
			if math.Abs(float64(got[k]-want[k])) > 0.5 {
				t.Errorf("%s %q: %s %.1f, the Git references' %.1f", p.name, p.title, label, got[k], want[k])
			}
		}
	}
}

// TestOneDisclosureChevron pins the section header's chevron to the one
// rotating treeChevron: no second collapsible header with a chevron of its
// own — hand-rotated (Animate "rot") or swapped between the right- and
// down-pointing glyphs. The two other chevrons are deliberate: the diff file
// card's own 15pt one, and the Workspace tree rows' hover chevron.
func TestOneDisclosureChevron(t *testing.T) {
	rotates := regexp.MustCompile(`Animate\("rot"`)
	swaps := regexp.MustCompile(`\biconChevron\b`)
	allowedRotate := map[string]bool{"sidebar_tree.go": true, "gd_surface.go": true}
	allowedSwap := map[string]bool{"icons.go": true, "components.go": true}
	files, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	rotated, swapped := map[string]int{}, map[string]int{}
	for _, name := range files {
		if strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatal(err)
		}
		rotated[name] = len(rotates.FindAllIndex(src, -1))
		swapped[name] = len(swaps.FindAllIndex(src, -1))
	}
	for name, n := range rotated {
		if n > 0 && !allowedRotate[name] {
			t.Errorf("%s hand-rotates a chevron (%d×): a collapsible header draws treeChevron", name, n)
		}
	}
	for name, n := range swapped {
		if n > 0 && !allowedSwap[name] {
			t.Errorf("%s swaps the right-pointing chevron (%d×): a collapsible header draws treeChevron", name, n)
		}
	}
	if rotated["sidebar_tree.go"] == 0 || rotated["gd_surface.go"] == 0 || swapped["components.go"] == 0 {
		t.Fatalf("the scan no longer finds the known chevrons (rotate %v, swap %v)", rotated, swapped)
	}
}
