// Package commandcenter owns the fast command entry point (0.9 P1) and the
// shared Action Registry (0.9 P2): one snapshot-based index over navigation,
// agents, history, tools and app actions with exact > prefix > substring >
// fuzzy ranking. Opening the palette performs zero filesystem, network or
// Herdr RPC work — every result is a typed stable target revalidated by the
// executor at action time.
package commandcenter

import (
	"sort"
	"strings"
)

// Scope narrows a palette query (0.9 P1): All is the Cmd/Ctrl+K and
// Cmd/Ctrl+Shift+P palette; Navigation is the Cmd/Ctrl+P fast navigation
// entry. The palette is the app's single search surface (2026-10-06: the
// standalone Search page was removed) — it matches navigation, agents,
// history destinations and app actions; the History page's own list filter
// remains that page's scoped tool.
type Scope string

const (
	ScopeAll        Scope = "all"
	ScopeNavigation Scope = "navigation"
)

// TargetKind is the typed stable target of one result; the executor
// revalidates it against the application snapshot before acting.
type TargetKind string

const (
	TargetPane    TargetKind = "pane"
	TargetTab     TargetKind = "tab"
	TargetProject TargetKind = "project"
	TargetAgent   TargetKind = "agent"
	TargetRoute   TargetKind = "route"
	TargetHistory TargetKind = "history"
	TargetSetting TargetKind = "setting"
	TargetNewTask TargetKind = "new-task"
)

// Action is the shared action metadata (0.9 P2): the single source consumed
// by the Command Center, the shortcut reference, native menus and selected
// toolbar surfaces. Availability is derived by the builder from the
// application snapshot and capabilities — never from render-time IO.
type Action struct {
	ID       string
	Title    string
	Section  string
	Shortcut string
	Keywords []string
	// Target carries the typed stable destination.
	Kind     TargetKind
	TargetID string
	// ScopeFilter restricts the action to a palette scope; empty means All.
	ScopeFilter Scope
	// Available gates execution and display; the builder derives it from
	// snapshot facts (an agent exists, a pane is attachable, ...).
	Available bool
}

// Result is one ranked palette hit.
type Result struct {
	Action Action
	// Rank orders equal-match-class results deterministically.
	Rank int
	// matchClass mirrors the §P1 ranking: 0 exact, 1 prefix, 2 substring,
	// 3 fuzzy.
	matchClass int
}

// Index is the immutable, snapshot-built palette index. Build creates it;
// Query is pure.
type Index struct {
	actions []Action
}

// Build indexes the snapshot-derived actions. Unavailable actions are kept
// for the shortcut reference but never surface as palette results.
func Build(actions []Action) *Index {
	copied := make([]Action, len(actions))
	copy(copied, actions)
	return &Index{actions: copied}
}

// Actions returns the registry contents (shortcut reference / menus).
func (ix *Index) Actions() []Action {
	out := make([]Action, len(ix.actions))
	copy(out, ix.actions)
	return out
}

// Query ranks the visible actions for one palette query. An empty query
// lists every available action in stable section/title order.
func (ix *Index) Query(text string, scope Scope) []Result {
	text = strings.ToLower(strings.TrimSpace(text))
	results := make([]Result, 0, len(ix.actions))
	for _, action := range ix.actions {
		if !action.Available {
			continue
		}
		if !scopeAllows(action, scope) {
			continue
		}
		class, ok := matchClass(action, text)
		if !ok {
			continue
		}
		results = append(results, Result{Action: action, matchClass: class})
	}
	if text == "" {
		for i := range results {
			results[i].matchClass = 0
		}
	}
	sort.SliceStable(results, func(i, j int) bool {
		if results[i].matchClass != results[j].matchClass {
			return results[i].matchClass < results[j].matchClass
		}
		if results[i].Action.Section != results[j].Action.Section {
			return results[i].Action.Section < results[j].Action.Section
		}
		return results[i].Action.Title < results[j].Action.Title
	})
	for i := range results {
		results[i].Rank = i
	}
	return results
}

func scopeAllows(action Action, scope Scope) bool {
	if scope == ScopeNavigation {
		// The Cmd+P fast entry is strictly navigation: only actions tagged
		// for it surface there.
		return action.ScopeFilter == ScopeNavigation
	}
	return true
}

// matchClass applies the §P1 ranking: exact → prefix → substring → fuzzy
// (subsequence), across title, keywords and target id.
func matchClass(action Action, text string) (int, bool) {
	if text == "" {
		return 0, true
	}
	title := strings.ToLower(action.Title)
	if title == text {
		return 0, true
	}
	if strings.HasPrefix(title, text) {
		return 1, true
	}
	if strings.Contains(title, text) {
		return 2, true
	}
	for _, keyword := range action.Keywords {
		keyword = strings.ToLower(keyword)
		if keyword == text || strings.HasPrefix(keyword, text) {
			return 1, true
		}
		if strings.Contains(keyword, text) {
			return 2, true
		}
	}
	if isSubsequence(text, title) {
		return 3, true
	}
	return 0, false
}

// isSubsequence reports whether query appears as an in-order subsequence of
// the candidate (the fuzzy tier).
func isSubsequence(query, candidate string) bool {
	if query == "" {
		return true
	}
	i := 0
	for j := 0; j < len(candidate) && i < len(query); j++ {
		if query[i] == candidate[j] {
			i++
		}
	}
	return i == len(query)
}
