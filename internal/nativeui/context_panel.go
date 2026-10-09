package nativeui

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 Router 当前页面、History 只读元数据与通用 Design Tokens
 * [OUTPUT]: 提供 contextPanelForRoute、Shell.historyContextPanel、Shell.historyContextMeta
 * [POS]: nativeui 右侧上下文面板的页面归属门禁；只投影 History 事实，绝不替代 Workspace 工具或重复加载 transcript
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

type contextPanelKind uint8

const (
	contextPanelNone contextPanelKind = iota
	contextPanelWorkspace
	contextPanelHistory
)

// contextPanelForRoute keeps panel ownership exclusive. An unavailable
// context hides the panel without changing its saved open/tool state.
func contextPanelForRoute(path string) contextPanelKind {
	switch {
	case path == routeWorkspace:
		return contextPanelWorkspace
	case path == routeHistory, path == "/history-projects", strings.HasPrefix(path, "/history/"):
		return contextPanelHistory
	default:
		return contextPanelNone
	}
}

func (s *Shell) activeContextPanel() contextPanelKind {
	return contextPanelForRoute(s.router.Path())
}

// historyContextMeta never reuses a previous conversation when navigation is
// pending; the route key is the identity fence.
func (s *Shell) historyContextMeta() (history.SessionMeta, bool) {
	path := s.router.Path()
	if !strings.HasPrefix(path, "/history/") {
		return history.SessionMeta{}, false
	}
	key := strings.TrimPrefix(path, "/history/")
	if key == "" {
		return history.SessionMeta{}, false
	}
	if detail := s.hist.detail; detail != nil && detail.key == key && !detail.loading && detail.errText == "" && detail.meta.Key == key {
		return detail.meta, true
	}
	for _, summary := range s.hist.sessions {
		if summary.Meta.Key == key {
			return summary.Meta, true
		}
	}
	return history.SessionMeta{}, false
}

// historyContextPanel renders only metadata for the current History route;
// the center remains the transcript owner. It never invokes a History query.
func (s *Shell) historyContextPanel(c *ui.Context) {
	t := c.Theme()
	tokens := s.tokens(t.Dark)
	sp := Spacing()
	typ := Typography()
	path := s.router.Path()
	ui.Column(c).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(sp.L, sp.L).Gap(sp.S).AlignItems(ui.Center).
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			ui.Icon(c, iconHistory).Size(14, 14).TextColor(t.TextMuted)
			ui.Text(c, "History details").FontSize(typ.Section).FontWeight(650).Grow(1)
			ui.Text(c, "READ ONLY").FontSize(typ.Micro).TextColor(t.TextMuted)
		})
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.L).Gap(sp.L).Children(func() {
				meta, ok := s.historyContextMeta()
				if !ok {
					switch {
					case path == routeHistory:
						s.historyOverviewCard(c, fmt.Sprintf("%d conversations in view", len(s.hist.sessions)), "Open a conversation to inspect its provider, project, timestamps and source.")
					case path == "/history-projects":
						s.historyOverviewCard(c, fmt.Sprintf("%d project conversations", len(s.historyProjects)), "Select a conversation to see its details here.")
					default:
						s.historyOverviewCard(c, "Loading conversation", "The details follow the selected conversation.")
					}
					return
				}
				settingsCard(c, "Conversation", func() {
					ui.Text(c, fallbackText(meta.Title, history.Untitled)).FontSize(typ.Body).FontWeight(650).
						Tooltip(meta.Title)
					providerBadge(c, meta.Agent)
					historyFact(c, "Project", fallbackText(meta.ProjectName, "Unknown project"), meta.ProjectPath)
					historyFact(c, "Messages", fmt.Sprintf("%d", meta.MessageCount), "")
					historyFact(c, "Updated", historyTimestamp(meta.UpdatedAt), "")
					historyFact(c, "Created", historyTimestamp(meta.CreatedAt), "")
				})
				if meta.GitBranch != nil && *meta.GitBranch != "" || meta.Model != nil && *meta.Model != "" {
					settingsCard(c, "Environment", func() {
						if meta.GitBranch != nil {
							historyFact(c, "Branch", *meta.GitBranch, "")
						}
						if meta.Model != nil {
							historyFact(c, "Model", *meta.Model, "")
						}
					})
				}
				if meta.FilePath != "" {
					settingsCard(c, "Source", func() {
						historyFact(c, "Provider file", filepath.Base(meta.FilePath), meta.FilePath)
						if ui.Button(c, "Copy source path").Clicked() {
							s.copyToClipboard(meta.FilePath)
						}
					})
				}
			})
		})
	})
}

func (s *Shell) historyOverviewCard(c *ui.Context, title, description string) {
	settingsCard(c, "Context", func() {
		ui.Text(c, title).FontSize(Typography().Body).FontWeight(650)
		ui.Text(c, description).FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
	})
}

func historyFact(c *ui.Context, label, value, tooltip string) {
	if value == "" {
		return
	}
	ui.Column(c).FillWidth().Gap(2).Children(func() {
		ui.Text(c, label).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
		field := ui.Text(c, value).FontSize(Typography().BodySmall).SingleLine()
		if tooltip != "" {
			field.Tooltip(tooltip)
		} else {
			field.Tooltip(value)
		}
	})
}
