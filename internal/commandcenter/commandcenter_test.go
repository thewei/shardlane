package commandcenter

import (
	"testing"
)

func paletteActions() []Action {
	return []Action{
		{ID: "nav.pane.p1", Title: "Pane: editor", Section: "Navigation", Kind: TargetPane, TargetID: "p1", ScopeFilter: ScopeNavigation, Available: true},
		{ID: "nav.pane.p2", Title: "Pane: tests", Section: "Navigation", Kind: TargetPane, TargetID: "p2", ScopeFilter: ScopeNavigation, Available: true},
		{ID: "agent.open.scout", Title: "Agent: Scout", Section: "Agents", Keywords: []string{"open agent", "scout"}, Kind: TargetAgent, TargetID: "term-1", Available: true},
		{ID: "agent.open.ranger", Title: "Agent: Ranger", Section: "Agents", Kind: TargetAgent, TargetID: "term-2", Available: true},
		{ID: "history.recent.1", Title: "History: Fix the parser", Section: "History", Kind: TargetHistory, TargetID: "claude-code:s1", Available: true},
		{ID: "app.new-task", Title: "New Task", Section: "App", Shortcut: "⇧⌘N", Keywords: []string{"create agent", "start"}, Kind: TargetNewTask, Available: true},
		{ID: "app.status-center", Title: "Status Center", Section: "App", Kind: TargetRoute, TargetID: "/status-center", Available: true},
		{ID: "app.settings", Title: "Settings", Section: "App", Kind: TargetSetting, TargetID: "/settings/appearance", Available: true},
		{ID: "app.hidden", Title: "Hidden Action", Section: "App", Kind: TargetRoute, Available: false},
	}
}

// TestQueryEmptyListsAllAvailable pins the zero-query behavior: every
// available action in stable section/title order; unavailable actions never
// surface.
func TestQueryEmptyListsAllAvailable(t *testing.T) {
	index := Build(paletteActions())
	results := index.Query("", ScopeAll)
	if len(results) != 8 {
		t.Fatalf("results = %d, want 8 (hidden action excluded)", len(results))
	}
	// Stable order: Agents < App < History < Navigation (section asc).
	if results[0].Action.Section != "Agents" || results[len(results)-1].Action.Section != "Navigation" {
		t.Fatalf("ordering broken: first=%q last=%q", results[0].Action.Title, results[len(results)-1].Action.Title)
	}
	for i, result := range results {
		if result.Rank != i {
			t.Fatalf("rank not normalized: %d/%d", result.Rank, i)
		}
	}
}

// TestQueryRankingClasses pins the §P1 ranking: exact beats prefix beats
// substring beats fuzzy, with keywords counted at their class.
func TestQueryRankingClasses(t *testing.T) {
	index := Build(paletteActions())

	// Exact title first, prefix-class second.
	results := index.Query("new task", ScopeAll)
	if len(results) == 0 || results[0].Action.ID != "app.new-task" {
		t.Fatalf("exact ranking broken: %+v", results)
	}

	// Prefix beats substring: "status" prefixes "Status Center".
	results = index.Query("status", ScopeAll)
	if results[0].Action.ID != "app.status-center" {
		t.Fatalf("prefix ranking broken: %+v", results)
	}

	// Keywords participate at their own class: "scout" is a keyword prefix
	// of the Scout agent action.
	results = index.Query("scout", ScopeAll)
	if len(results) == 0 || results[0].Action.ID != "agent.open.scout" {
		t.Fatalf("keyword ranking broken: %+v", results)
	}

	// Fuzzy subsequence: "stus" matches "Status Center" only fuzzily (and
	// nothing else in the palette).
	results = index.Query("stus", ScopeAll)
	if len(results) != 1 || results[0].Action.ID != "app.status-center" || results[0].matchClass != 3 {
		t.Fatalf("fuzzy ranking broken: %+v", results)
	}

	// No match at all.
	if results := index.Query("zzzzzz", ScopeAll); len(results) != 0 {
		t.Fatalf("no-match query returned %d results", len(results))
	}
}

// TestQueryScope pins the scopes: Navigation excludes non-navigation
// actions; All includes everything.
func TestQueryScope(t *testing.T) {
	index := Build(paletteActions())
	nav := index.Query("", ScopeNavigation)
	for _, result := range nav {
		if result.Action.Section != "Navigation" {
			t.Fatalf("navigation scope leaked %q", result.Action.Title)
		}
	}
	if len(nav) != 2 {
		t.Fatalf("navigation results = %d, want 2", len(nav))
	}
	all := index.Query("", ScopeAll)
	if len(all) != 8 {
		t.Fatalf("all results = %d, want 8", len(all))
	}
}

// TestQueryIsDeterministicAndPure pins snapshot purity: repeated queries
// return identical results and never mutate the index.
func TestQueryIsDeterministicAndPure(t *testing.T) {
	index := Build(paletteActions())
	first := index.Query("agent", ScopeAll)
	second := index.Query("agent", ScopeAll)
	if len(first) != len(second) {
		t.Fatalf("query is not deterministic: %d vs %d", len(first), len(second))
	}
	for i := range first {
		if first[i].Action.ID != second[i].Action.ID || first[i].Rank != second[i].Rank {
			t.Fatalf("result %d drifted: %+v vs %+v", i, first[i], second[i])
		}
	}
	// The index is immutable: registry contents unchanged by queries.
	if got := len(index.Actions()); got != len(paletteActions()) {
		t.Fatalf("index mutated: %d actions", got)
	}
}

// TestFuzzySubsequence pins the fuzzy tier predicate directly.
func TestFuzzySubsequence(t *testing.T) {
	if !isSubsequence("stus", "status center") || !isSubsequence("nt", "new task") {
		t.Fatal("valid subsequences rejected")
	}
	if isSubsequence("sz", "status center") {
		t.Fatal("invalid subsequence accepted")
	}
	if !isSubsequence("", "anything") {
		t.Fatal("empty query must match")
	}
}
