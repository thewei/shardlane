/**
 * [INPUT]: 依赖 history.AgentID/herdr.Agent、brands.go 的 agentMark/agentProvider/brandPNG
 * [OUTPUT]: 对外提供 TestBrandAssetNameAntigravity、TestAgentMarkAntigravity、TestAgentProviderResolution、TestAgentGlyphAntigravity
 * [POS]: Agent 品牌位图资产、Antigravity 图标解析与回退机制的单元测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

func TestBrandAssetNameAntigravity(t *testing.T) {
	for _, dark := range []bool{true, false} {
		asset := brandAssetName(history.AgentAntigravity, dark)
		if asset != "antigravity" {
			t.Fatalf("brandAssetName(AgentAntigravity, dark=%v) = %q, want 'antigravity'", dark, asset)
		}
	}
}

func TestBrandPNGDecodesAntigravity(t *testing.T) {
	bmp := brandPNG("antigravity")
	if bmp == nil {
		t.Fatalf("brandPNG('antigravity') returned nil, expected decoded ui.Bitmap")
	}
}

func TestAgentMarkAntigravity(t *testing.T) {
	for _, dark := range []bool{true, false} {
		mark := agentMark(history.AgentAntigravity, dark)
		if mark.bmp == nil {
			t.Fatalf("agentMark(AgentAntigravity, dark=%v) bmp is nil, got mark: %+v", dark, mark)
		}
	}
}

func TestAgentProviderResolution(t *testing.T) {
	cases := []struct {
		name string
		agent *herdr.Agent
		want history.AgentID
	}{
		{
			name:  "nil agent",
			agent: nil,
			want:  "",
		},
		{
			name:  "kind antigravity",
			agent: &herdr.Agent{Kind: "antigravity"},
			want:  history.AgentAntigravity,
		},
		{
			name:  "kind agy",
			agent: &herdr.Agent{Kind: "agy"},
			want:  history.AgentAntigravity,
		},
		{
			name:  "kind antigravity-cli",
			agent: &herdr.Agent{Kind: "antigravity-cli"},
			want:  history.AgentAntigravity,
		},
		{
			name: "kind empty session agy",
			agent: &herdr.Agent{
				Kind: "",
				AgentSession: &herdr.AgentSessionIdentity{Agent: "agy"},
			},
			want: history.AgentAntigravity,
		},
		{
			name: "kind empty session antigravity-cli",
			agent: &herdr.Agent{
				Kind: "",
				AgentSession: &herdr.AgentSessionIdentity{Agent: "antigravity-cli"},
			},
			want: history.AgentAntigravity,
		},
		{
			name:  "kind claude-code",
			agent: &herdr.Agent{Kind: "claude-code"},
			want:  history.AgentClaudeCode,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := agentProvider(tc.agent)
			if got != tc.want {
				t.Errorf("agentProvider() = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestAgentGlyphAntigravity(t *testing.T) {
	svg := agentGlyph(history.AgentAntigravity)
	if svg == nil || svg == iconAgent {
		t.Fatalf("agentGlyph(AgentAntigravity) should return glyphAntigravity, got %+v", svg)
	}
}
