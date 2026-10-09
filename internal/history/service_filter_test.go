package history

/**
 * [INPUT]: 依赖内存隔离 Catalog 测试夹具与真实 HistoryService.List 实现
 * [OUTPUT]: 验证 Provider/Project 过滤先于 LIMIT，旧记录不会被较新的其它 Provider 截断
 * [POS]: History 元数据查询 SQL 语义回归测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"testing"
)

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
		if err := service.Catalog().WriteSession(meta, tc.updated, nil); err != nil {
			t.Fatal(err)
		}
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
