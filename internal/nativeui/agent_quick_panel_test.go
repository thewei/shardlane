package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖 StatusCenterEntry 的目录分桶
 * [OUTPUT]: 钉住浮层的状态分组：按优先级排序、空组省略、各组内保持快照顺序
 * [POS]: agent_quick_panel 分组的纯函数回归；整面板渲染与点击直达见 quick_panel_test.go，不访问用户实例或修改运行时
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestQuickPanelGroupsFollowThePriorityOrder(t *testing.T) {
	entry := func(name string, bucket agent.AgentDirectoryBucket) StatusCenterEntry {
		return StatusCenterEntry{Card: agent.AgentCardModel{Title: name}, Bucket: bucket}
	}
	groups := quickPanelGroups([]StatusCenterEntry{
		entry("idle-1", agent.BucketIdle),
		entry("work-1", agent.BucketWorking),
		entry("attn-1", agent.BucketNeedsAttention),
		entry("work-2", agent.BucketWorking),
		entry("attn-2", agent.BucketNeedsAttention),
	})
	var labels []string
	for _, group := range groups {
		labels = append(labels, group.Label)
	}
	if len(groups) != 3 || labels[0] != "Needs attention" || labels[1] != "Working" || labels[2] != "Idle" {
		t.Fatalf("groups = %q, want Needs attention, Working, Idle (Review is empty and omitted)", labels)
	}
	if groups[0].Tone != ToneAttention || groups[1].Tone != ToneWorking || groups[2].Tone != ToneMuted {
		t.Fatalf("group tones = %v %v %v", groups[0].Tone, groups[1].Tone, groups[2].Tone)
	}
	if got := groups[1].Entries; len(got) != 2 || got[0].Card.Title != "work-1" || got[1].Card.Title != "work-2" {
		t.Fatalf("a group must keep the snapshot order, got %+v", got)
	}
	if quickPanelGroups(nil) != nil && len(quickPanelGroups(nil)) != 0 {
		t.Fatal("no entries, no groups")
	}
	review := quickPanelGroups([]StatusCenterEntry{entry("done-1", agent.BucketReadyForReview)})
	if len(review) != 1 || review[0].Label != "Review" || review[0].Tone != ToneSuccess {
		t.Fatalf("review group = %+v", review)
	}
}
