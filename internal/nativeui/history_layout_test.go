package nativeui

/**
 * [INPUT]: 依赖真实 HistoryService/Catalog 夹具、官方 MyGo headless Tester 与 History 页面状态
 * [OUTPUT]: 验证 Sessions 的 Agent 标签（含数量）、隐藏扣减、有界计数文案、行指标列与悬停占位
 * [POS]: Sessions 页版式回归（参照 magpie 会话历史模块），不依赖真实 Agent 或用户终端
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// newMultiAgentHistoryShell seeds a real catalog with three Providers: two Pi
// sessions (one with a token total), one Codex and one Claude Code session.
func newMultiAgentHistoryShell(t *testing.T) *Shell {
	t.Helper()
	catalog := history.NewCatalog()
	updated := time.Date(2025, 3, 4, 9, 0, 0, 0, time.Local).UnixMilli()
	tokens := int64(507_000)
	for i, seed := range []struct {
		agent  history.AgentID
		id     string
		title  string
		tokens *int64
	}{
		{history.AgentPi, "p1", "Tighten sidebar spacing", &tokens},
		{history.AgentPi, "p2", "Rename the settings groups", nil},
		{history.AgentCodex, "c1", "Fix the flaky terminal test", nil},
		{history.AgentClaudeCode, "k1", "Review the release notes", nil},
	} {
		meta := history.SessionMeta{
			Key: string(seed.agent) + ":" + seed.id, ID: seed.id, Agent: seed.agent, Title: seed.title,
			ProjectPath: "/work/alpha", ProjectName: "alpha", FilePath: "/nowhere/" + seed.id,
			UpdatedAt: updated - int64(i)*1000, MessageCount: 6, TokensUsed: seed.tokens,
		}
		catalog.Put(meta, meta.UpdatedAt, "")
	}
	return NewShell(WithHistory(history.NewHistoryService(catalog, nil)))
}

// TestHistoryAgentTabsCarryCounts pins the Sessions tab strip: "All" plus one
// tab per catalog Provider, counts that follow the hidden list, and a click
// that scopes the list to that Provider.
func TestHistoryAgentTabsCarryCounts(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame() // the first frame requests the list; this one renders it
	for _, want := range []string{"All", "Pi", "Codex", "Claude Code"} {
		if _, ok := tester.Find(want); !ok {
			t.Fatalf("tab %q missing; texts=%q", want, tester.Texts())
		}
	}
	for provider, want := range map[history.AgentID]int{history.AgentPi: 2, history.AgentCodex: 1, history.AgentClaudeCode: 1} {
		if got := shell.historyTabCount(provider); got != want {
			t.Fatalf("%s count = %d, want %d", provider, got, want)
		}
	}

	// Hiding a session takes it off its Provider's count.
	shell.toggleSessionHidden("pi:p2")
	if got := shell.historyTabCount(history.AgentPi); got != 1 {
		t.Fatalf("Pi count after hiding one = %d, want 1", got)
	}
	if got := shell.historyTabCount(history.AgentCodex); got != 1 {
		t.Fatalf("hiding a Pi session changed the Codex count to %d", got)
	}
	// In the Hidden view the tabs count the hidden sessions instead.
	shell.hist.showHidden = true
	if pi, codex := shell.historyTabCount(history.AgentPi), shell.historyTabCount(history.AgentCodex); pi != 1 || codex != 0 {
		t.Fatalf("Hidden view counts: Pi=%d Codex=%d, want 1 and 0", pi, codex)
	}
	shell.hist.showHidden = false
	shell.toggleSessionHidden("pi:p2")

	if err := tester.Click("Pi"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.hist.provider != string(history.AgentPi) {
		t.Fatalf("Pi tab left provider = %q", shell.hist.provider)
	}
	if !tester.HasText("Tighten sidebar spacing") || tester.HasText("Fix the flaky terminal test") {
		t.Fatalf("Pi tab list is not scoped to Pi; texts=%q", tester.Texts())
	}
	// The scoped list drops the Agent name from each row; the tab names it.
	if tester.HasText("Pi · ") {
		t.Fatalf("scoped rows still repeat the Agent name; texts=%q", tester.Texts())
	}
	if err := tester.Click("All"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.hist.provider != "" || !tester.HasText("Fix the flaky terminal test") {
		t.Fatalf("All tab did not restore the list; provider=%q texts=%q", shell.hist.provider, tester.Texts())
	}
}

// TestHistoryRowShowsTokenTotalAndFacts pins the row's right-hand figure: the
// token total has its own column (compact), the fact line carries the rest.
func TestHistoryRowShowsTokenTotalAndFacts(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("507k") || !tester.HasText("tokens") {
		t.Fatalf("token column missing; texts=%q", tester.Texts())
	}
	if tester.HasText("507000 tokens") {
		t.Fatalf("token total leaked into the fact line; texts=%q", tester.Texts())
	}
	if !tester.HasText("Pi · 6 messages") {
		t.Fatalf("mixed list must name the Agent in the fact line; texts=%q", tester.Texts())
	}
}

// TestHistoryRowHoverKeepsColumnsInPlace pins the stable trailing slot: the
// "…" menu button that replaces the date under the pointer fills the date's
// footprint, so the token column (and the title column) do not shift when
// the pointer enters the row.
func TestHistoryRowHoverKeepsColumnsInPlace(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	stamp := historyShortDate(time.Date(2025, 3, 4, 9, 0, 0, 0, time.Local).UnixMilli(), time.Now())
	date, ok := tester.Find(stamp)
	if !ok {
		t.Fatalf("date %q missing; texts=%q", stamp, tester.Texts())
	}
	tokens, ok := tester.Find("507k")
	if !ok {
		t.Fatalf("token total missing; texts=%q", tester.Texts())
	}
	title, ok := tester.Find("Tighten sidebar spacing")
	if !ok {
		t.Fatalf("row title missing; texts=%q", tester.Texts())
	}
	tester.Move(title.X+title.W/2, title.Y+title.H/2)
	tester.Frame()
	more, ok := tester.Find("More actions for Tighten sidebar spacing")
	if !ok {
		t.Fatalf("hovered row has no menu button; texts=%q", tester.Texts())
	}
	if got, want := more.X+more.W, date.X+date.W; got < want-0.5 || got > want+0.5 {
		t.Fatalf("menu button ends at %.1f, date ended at %.1f", got, want)
	}
	after, ok := tester.Find("507k")
	if !ok {
		t.Fatalf("token total vanished on hover; texts=%q", tester.Texts())
	}
	if after.X < tokens.X-0.5 || after.X > tokens.X+0.5 {
		t.Fatalf("hover moved the token column from x=%.1f to x=%.1f", tokens.X, after.X)
	}
}

// TestHistoryCountCaptionIsHonestWhenBounded pins that a list cut at the
// query limit says "Latest N" instead of implying a catalog total.
func TestHistoryCountCaptionIsHonestWhenBounded(t *testing.T) {
	shell := NewShell()
	shell.hist.service = newFakeHistoryView()
	shell.hist.loaded = true
	for i := 0; i < HistoryListLimit; i++ {
		shell.hist.sessions = append(shell.hist.sessions, summaryTitled(fmt.Sprintf("codex:%03d", i), fmt.Sprintf("Session %03d", i)))
	}
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText(fmt.Sprintf("Latest %d sessions", HistoryListLimit)) {
		t.Fatalf("bounded list must say Latest %d; texts=%q", HistoryListLimit, tester.Texts())
	}
	shell.hist.sessions = shell.hist.sessions[:3]
	tester = ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("3 sessions") || tester.HasText("Latest") {
		t.Fatalf("short list must show its plain count; texts=%q", tester.Texts())
	}
}

// TestHistoryToolbarWrapsInNarrowWindow pins that the control strip folds
// onto a second line instead of squeezing the search field away: at the
// narrowest window every control stays inside the content width.
func TestHistoryToolbarWrapsInNarrowWindow(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	shell.setSessionsHidden([]string{"pi:p2"}, true) // the Hidden toggle shows once something is hidden
	const width = 720
	tester := ui.NewTester(shell.View, width, 600)
	tester.Frame()
	for _, label := range []string{"Claude Code", "Select", "Hidden", "Search sessions"} {
		rect, ok := tester.Find(label)
		if !ok {
			t.Fatalf("%q missing at %d wide; texts=%q", label, width, tester.Texts())
		}
		if rect.X+rect.W > width {
			t.Fatalf("%q overflows the window: %+v", label, rect)
		}
	}
}

// TestHistoryToolbarKeepsOutControlsWithNothingToAct pins the one rule behind
// the Agent tabs (one Agent needs no switch), carried to the other toggles:
// Hidden appears once a session is hidden, and stays while its view is open.
func TestHistoryToolbarKeepsOutControlsWithNothingToAct(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if tester.HasText("Hidden") {
		t.Fatalf("Hidden shows with nothing hidden; texts=%q", tester.Texts())
	}

	shell.setSessionsHidden([]string{"pi:p2"}, true)
	tester.Frame()
	if !tester.HasText("Hidden") {
		t.Fatalf("Hidden missing once a session is hidden; texts=%q", tester.Texts())
	}

	if err := tester.Click("Hidden sessions"); err != nil {
		t.Fatal(err)
	}
	shell.setSessionsHidden([]string{"pi:p2"}, false)
	tester.Frame()
	if !shell.hist.showHidden || !tester.HasText("Hidden") {
		t.Fatalf("the open Hidden view lost its way back; texts=%q", tester.Texts())
	}
}

// TestHistoryEmptyListWhileScanningReadsAsLoading pins the cold-start gap:
// the catalog is rebuilt in memory on every launch, so an empty list during
// that scan means "still reading", never "No sessions yet".
func TestHistoryEmptyListWhileScanningReadsAsLoading(t *testing.T) {
	shell := NewShell(WithHistory(history.NewHistoryService(history.NewCatalog(), nil)))
	shell.router.Replace(routeHistory)
	shell.hist.scanning = true
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	tester.Frame()
	if tester.HasText("No sessions yet") || !tester.HasText("Loading sessions…") {
		t.Fatalf("an empty list mid-scan must read as loading; texts=%q", tester.Texts())
	}

	shell.hist.scanning = false
	shell.hist.loaded = false
	tester.Frame()
	tester.Frame()
	if !tester.HasText("No sessions yet") {
		t.Fatalf("a finished scan with nothing found must say so; texts=%q", tester.Texts())
	}
}
