package history

/**
 * [INPUT]: 依赖内存 Catalog 测试夹具与真实 HistoryService.List 实现
 * [OUTPUT]: 验证 Provider/Project 过滤先于 LIMIT，旧记录不会被较新的其它 Provider 截断
 * [POS]: History 元数据查询语义回归测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"testing"
)

// TestHistoryServiceProviderCounts pins the Sessions tab source: per-Provider
// totals over the whole catalog (archived rows excluded), most recently
// active Provider first.
func TestHistoryServiceProviderCounts(t *testing.T) {
	service, _ := newTestService(t)
	cases := []struct {
		key, agent string
		updated    int64
		archived   bool
	}{
		{"codex:a", string(AgentCodex), 3000, false},
		{"codex:b", string(AgentCodex), 1000, false},
		{"codex:gone", string(AgentCodex), 2000, true},
		{"pi:a", string(AgentPi), 4000, false},
		{"claude:a", string(AgentClaudeCode), 500, false},
	}
	for _, tc := range cases {
		meta := catalogTestSession(tc.key, tc.key, "/nowhere/"+tc.key, "/work/a")
		meta.Agent = AgentID(tc.agent)
		meta.UpdatedAt = tc.updated
		meta.Archived = tc.archived
		service.Catalog().Put(meta, tc.updated, "")
	}
	counts, err := service.ProviderCounts(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	want := []ProviderCount{{AgentPi, 1}, {AgentCodex, 2}, {AgentClaudeCode, 1}}
	if len(counts) != len(want) {
		t.Fatalf("provider counts = %+v, want %+v", counts, want)
	}
	for i := range want {
		if counts[i] != want[i] {
			t.Fatalf("provider counts = %+v, want %+v", counts, want)
		}
	}
}

func TestHistoryServiceFiltersBeforeLimit(t *testing.T) {
	service, _ := newTestService(t)
	cases := []struct {
		key, agent, project string
		updated             int64
	}{
		{"codex:newest", string(AgentCodex), "/work/b", 3000},
		{"codex:newer", string(AgentCodex), "/work/a", 2000},
		{"claude:older", string(AgentClaudeCode), "/work/a", 1000},
	}
	for _, tc := range cases {
		meta := catalogTestSession(tc.key, tc.key, "/nowhere/"+tc.key, tc.project)
		meta.Agent = AgentID(tc.agent)
		meta.UpdatedAt = tc.updated
		service.Catalog().Put(meta, tc.updated, "")
	}
	providers, err := service.AvailableProviders(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(providers) != 2 || providers[0] != AgentCodex || providers[1] != AgentClaudeCode {
		t.Fatalf("full provider catalog = %+v", providers)
	}
	got, err := service.List(context.Background(), HistoryQuery{Provider: AgentClaudeCode, Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0].Meta.Key != "claude:older" {
		t.Fatalf("older matching provider was truncated by global limit: %+v", got)
	}
	got, err = service.List(context.Background(), HistoryQuery{ProjectPath: "/work/a", Provider: AgentClaudeCode, Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0].Meta.Key != "claude:older" {
		t.Fatalf("combined Project and Provider scope = %+v", got)
	}
	got, err = service.List(context.Background(), HistoryQuery{ProjectPath: "/work/b", Provider: AgentClaudeCode, Limit: 1})
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 0 {
		t.Fatalf("non-matching Project/Provider scope = %+v", got)
	}
}

// TestHistoryServiceProjectScopeCoversSubfolders pins what a project scope
// means: the folder and everything below it, so a session an Agent started in
// a repository's subfolder still belongs to the repository, while a sibling
// that merely shares the name prefix does not. The list, the search and the
// Provider totals all use the one rule.
func TestHistoryServiceProjectScopeCoversSubfolders(t *testing.T) {
	service, _ := newTestService(t)
	for _, tc := range []struct {
		key, agent, project string
		updated             int64
	}{
		{"claude:root", string(AgentClaudeCode), "/work/a", 4000},
		{"codex:sub", string(AgentCodex), "/work/a/pkg/web", 3000},
		{"codex:sibling", string(AgentCodex), "/work/ab", 2000},
		{"pi:other", string(AgentPi), "/work/b", 1000},
	} {
		meta := catalogTestSession(tc.key, "deploy "+tc.key, "/nowhere/"+tc.key, tc.project)
		meta.Agent = AgentID(tc.agent)
		meta.UpdatedAt = tc.updated
		service.Catalog().Put(meta, tc.updated, "")
	}
	ctx := context.Background()
	keys := func(found []SessionSummary) []string {
		out := make([]string, 0, len(found))
		for _, summary := range found {
			out = append(out, summary.Meta.Key)
		}
		return out
	}
	equal := func(got, want []string) bool {
		if len(got) != len(want) {
			return false
		}
		for i := range want {
			if got[i] != want[i] {
				return false
			}
		}
		return true
	}
	for _, scope := range []string{"/work/a", "/work/./a/", "/work/a/"} {
		listed, err := service.List(ctx, HistoryQuery{ProjectPath: scope, Limit: 10})
		if err != nil {
			t.Fatal(err)
		}
		if want := []string{"claude:root", "codex:sub"}; !equal(keys(listed), want) {
			t.Errorf("List under %q = %v, want %v", scope, keys(listed), want)
		}
	}
	searched, err := service.List(ctx, HistoryQuery{ProjectPath: "/work/a", Search: "deploy", Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"claude:root", "codex:sub"}; !equal(keys(searched), want) {
		t.Errorf("Search under /work/a = %v, want %v", keys(searched), want)
	}
	counts, err := service.ProviderCountsIn(ctx, "/work/a")
	if err != nil {
		t.Fatal(err)
	}
	if want := []ProviderCount{{AgentClaudeCode, 1}, {AgentCodex, 1}}; len(counts) != 2 || counts[0] != want[0] || counts[1] != want[1] {
		t.Errorf("Provider totals under /work/a = %+v, want %+v", counts, want)
	}
	all, err := service.ProviderCountsIn(ctx, "")
	if err != nil {
		t.Fatal(err)
	}
	total := 0
	for _, count := range all {
		total += count.Count
	}
	if total != 4 {
		t.Errorf("no scope counts %d sessions, want all 4", total)
	}
	root, err := service.List(ctx, HistoryQuery{ProjectPath: "/", Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if len(root) != 4 {
		t.Errorf("the filesystem root scopes %d sessions, want all 4", len(root))
	}
}

func TestProjectContains(t *testing.T) {
	for _, tc := range []struct {
		root, path string
		want       bool
	}{
		{"/work/a", "/work/a", true},
		{"/work/a", "/work/a/pkg", true},
		{"/work/./a/", "/work/a/pkg/../x", true},
		{"/work/a", "/work/ab", false},
		{"/work/a/pkg", "/work/a", false},
		{"/", "/work/a", true},
		{"", "/work/a", false},
		{"/work/a", "", false},
	} {
		if got := ProjectContains(tc.root, tc.path); got != tc.want {
			t.Errorf("ProjectContains(%q, %q) = %v, want %v", tc.root, tc.path, got, tc.want)
		}
	}
}
