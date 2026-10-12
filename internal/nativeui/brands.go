/**
 * [INPUT]: 依赖 history.AgentID/herdr.Agent、embed brands/*.png 以及 ui.NewBitmap
 * [OUTPUT]: 对外提供 agentMark、agentProvider、markView 以及 Shell.agentMarkForPane
 * [POS]: Agent 品牌位图资产与视觉标识解析器，统一侧栏卡片、窗口与窗格标题、浮层的品牌 Logo 映射
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

package nativeui

import (
	"embed"
	"image/png"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// Agent brand marks, carried over from the Rust Shardlane shell
// (crates/herdr-gui/assets/brands): small full-color provider logos shown
// wherever a live Agent appears. Unknown providers fall back to the
// monochrome person glyph.

//go:embed brands/*.png
var brandFS embed.FS

var (
	brandOnce   sync.Once
	brandBitmap = map[string]*ui.Bitmap{}
	brandMu     sync.Mutex
)

// brandPNG decodes and caches one embedded brand bitmap by asset name.
func brandPNG(name string) *ui.Bitmap {
	brandOnce.Do(func() {
		entries, err := brandFS.ReadDir("brands")
		if err != nil {
			return
		}
		for _, entry := range entries {
			file := entry.Name()
			data, err := brandFS.ReadFile("brands/" + file)
			if err != nil {
				continue
			}
			img, err := png.Decode(strings.NewReader(string(data)))
			if err != nil {
				continue
			}
			brandBitmap[strings.TrimSuffix(file, ".png")] = ui.NewBitmap(img)
		}
	})
	brandMu.Lock()
	defer brandMu.Unlock()
	return brandBitmap[name]
}

// brandAssetName mirrors the Rust shell's agent_brand_icon mapping
// (crates/herdr-gui/src/assets.rs): dark picks the full-color mark, light
// the tinted "light" variant when the provider ships one.
func brandAssetName(provider history.AgentID, dark bool) string {
	switch provider {
	case history.AgentClaudeCode:
		return "claude-code"
	case history.AgentCodex:
		return "codex"
	case history.AgentCommandCode:
		return "command-code"
	case history.AgentCopilot:
		if dark {
			return "copilot"
		}
		return "copilot-light"
	case history.AgentCursor:
		if dark {
			return "cursor"
		}
		return "cursor-light"
	case history.AgentOpenCode:
		if dark {
			return "opencode"
		}
		return "opencode-light"
	case history.AgentKiro:
		return "kiro"
	case history.AgentGemini:
		return "gemini"
	case history.AgentPi:
		if dark {
			return "pi"
		}
		return "pi-light"
	case history.AgentOMP:
		return "omp"
	case history.AgentGrok:
		if dark {
			return "grok"
		}
		return "grok-light"
	case history.AgentKimi:
		if dark {
			return "kimi"
		}
		return "kimi-light"
	case history.AgentAntigravity:
		return "antigravity"
	case history.AgentDSH:
		return "deepseek"
	default:
		return ""
	}
}

// visualMark is what agent rows render as their identity: a brand bitmap
// when the provider ships one, else the monochrome fallback glyph.
type visualMark struct {
	bmp *ui.Bitmap
	svg *ui.SVG
}

func markView(c *ui.Context, mark visualMark, size float32, color ui.Color) {
	if mark.bmp != nil {
		ui.Image(c, mark.bmp).Size(size, size)
		return
	}
	if mark.svg != nil {
		ui.Icon(c, mark.svg).Size(size, size).TextColor(color)
	}
}

// agentMark resolves a provider to its visual mark.
func agentMark(provider history.AgentID, dark bool) visualMark {
	if name := brandAssetName(provider, dark); name != "" {
		if bmp := brandPNG(name); bmp != nil {
			return visualMark{bmp: bmp}
		}
	}
	return visualMark{svg: agentGlyph(provider)}
}

// agentProvider extracts the canonical provider AgentID from a live Herdr Agent,
// checking Kind first, then falling back to AgentSession.Agent if Kind is empty.
func agentProvider(a *herdr.Agent) history.AgentID {
	if a == nil {
		return ""
	}
	if provider, ok := history.ParseAgentID(a.Kind); ok {
		return provider
	}
	if a.AgentSession != nil {
		if provider, ok := history.ParseAgentID(a.AgentSession.Agent); ok {
			return provider
		}
	}
	return ""
}

// agentMarkForPane resolves the mark a Pane row should show: the bound
// Agent's brand, or the plain terminal glyph for shells.
func (s *Shell) agentMarkForPane(paneID string, dark bool) (visualMark, bool) {
	a := s.paneAgent(paneID)
	if a == nil {
		return visualMark{svg: iconTerminal}, false
	}
	provider := agentProvider(a)
	return agentMark(provider, dark), true
}
