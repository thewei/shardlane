package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 page_history.go 的 History 行摘要选择规则
 * [OUTPUT]: 测试标题/摘要去重，且项目保留在独立元数据行
 * [POS]: History 列表排版语义的纯函数回归测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestHistoryRowDescriptionAvoidsDuplicateTextAndProject(t *testing.T) {
	tests := []struct {
		title, description, project, want string
	}{
		{"Fix terminal permissions", "Fix terminal permissions", "Shardlane", ""},
		{"Fix terminal", "Fix terminal permissions, update the logic", "Shardlane", ""},
		{"Fix terminal permissions", "", "Shardlane", ""},
		{"Fix terminal permissions", "The fix adds a fallback.", "Shardlane", "The fix adds a fallback."},
		{"", "Only description is known", "Shardlane", "Only description is known"},
	}
	for _, tc := range tests {
		input := history.SessionSummary{
			Meta:        history.SessionMeta{Title: tc.title, ProjectName: tc.project},
			Description: tc.description,
		}
		if got := historyRowDescription(input); got != tc.want {
			t.Errorf("title %q description %q: got %q, want %q", tc.title, tc.description, got, tc.want)
		}
	}
}
