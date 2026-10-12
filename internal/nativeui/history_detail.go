package nativeui

/**
 * [INPUT]: 依赖共享 Chat 状态的只读绑定（bindChatWindow）、chatWebviewBody 的 WebView 页面与原生时间线回退
 * [OUTPUT]: 提供 /history/{id} 会话详情页：头部（标题、Provider · 项目 · 模型、操作菜单）+ 共享 Chat 视图（只读、最新消息优先、向上懒加载）
 * [POS]: Sessions 详情与 Chat 共用同一条阅读管线；每次打开现读原始会话文件，页面不保存任何会话内容，也没有第二套转录渲染
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// historyDetailPage shows one past session through the shared Chat view.
// Only the page the router is on binds the chat (detached from every pane):
// the Router keeps building the outgoing page while it slides away, and two
// pages binding in turn would reload the session file every frame. The
// binding is released once no detail page builds any more
// (releaseIdleHistoryChat), so the Workspace chat and the inspector never
// see this transcript and reopening always reads the file again.
func (s *Shell) historyDetailPage(c *ui.Context, key string) {
	s.hist.detailFrame = s.hist.viewFrame
	if s.router.Path() == routeHistory+"/"+key && (!s.chatReadOnly || s.chatConversationID != key) {
		s.bindChatWindow(key, true)
	}
	sp := Spacing()
	t := c.Theme()
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		s.historyDetailHeader(c, key)
		if !s.chatReadOnly || s.chatConversationID != key {
			return
		}
		switch {
		case s.chatLoading:
			loadingState(c, "Loading conversation…")
		case len(s.chatTurns) == 0 && s.chatOutcome != "":
			ui.Column(c).Grow(1).Center().Gap(sp.M).Children(func() {
				ui.Text(c, "Couldn't read this session.").FontWeight(WeightSemibold)
				ui.Text(c, "The source file may have moved. Refresh the list and try again.").
					FontSize(Typography().BodySmall).TextColor(t.TextMuted)
				if ui.Button(c, "Back to Sessions").Clicked() {
					s.showHistoryIndex()
				}
			})
		case len(s.chatTurns) == 0:
			emptyState(c, "No messages", "")
		default:
			if s.chatWebviewBody(c) {
				return
			}
			s.historyDetailNativeBody(c)
		}
	})
}

// releaseIdleHistoryChat runs at the start of every frame: a read-only
// binding whose page did not build in the previous frame belongs to a route
// that was left (the exit animation is over), so it is dropped.
func (s *Shell) releaseIdleHistoryChat() {
	if s.chatReadOnly && s.hist.detailFrame+1 < s.hist.viewFrame {
		s.releaseHistoryChat()
	}
}

// historyDetailNativeBody is the fallback when the WebView chat is off or
// could not be created: the same derived timeline rows the native Chat uses,
// without a composer, plus an Earlier control for the lazy upward paging.
func (s *Shell) historyDetailNativeBody(c *ui.Context) {
	sp := Spacing()
	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.M, sp.XL).Gap(sp.S).Children(func() {
			if s.chatHasOlder {
				if ui.Button(c, "Earlier messages").Clicked() {
					s.chatLoadOlder()
				}
			}
			for i, turn := range s.chatTurns {
				s.chatTimelineRow(c, turn, i)
			}
		})
	})
}

// historyDetailMeta is what the header knows about the open session: the
// meta the transcript parse returned, else the list row it was opened from.
func (s *Shell) historyDetailMeta(key string) *history.SessionMeta {
	if s.chatSession != nil && s.chatSession.Key == key {
		return s.chatSession
	}
	for _, summary := range s.hist.sessions {
		if summary.Meta.Key == key {
			meta := summary.Meta
			return &meta
		}
	}
	return nil
}

// historyDetailHeader is the title band: the session title over its facts,
// and the session's action menu at the trailing edge. Back lives at the foot
// of the sidebar, the same place on every inner page.
func (s *Shell) historyDetailHeader(c *ui.Context, key string) {
	sp := Spacing()
	t := c.Theme()
	tokens := designTokens(t.Dark)
	meta := s.historyDetailMeta(key)
	title, subtitle := "Session", ""
	if meta != nil {
		title = fallbackText(meta.Title, history.Untitled)
		facts := []string{meta.ProjectName, meta.Agent.DisplayName()}
		if s.chatModel != "" {
			facts = append(facts, s.chatModel)
		}
		subtitle = strings.Join(nonEmptyStrings(facts), " · ")
	}
	ui.Row(c).FillWidth().Padding(sp.L, sp.XL).Gap(sp.M).AlignItems(ui.Center).Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
			// The list rows carry a tooltip for truncated titles; so does this.
			ui.Text(c, title).FontSize(Typography().Heading).FontWeight(WeightSemibold).
				SingleLine().Ellipsis("…").Tooltip(title)
			if subtitle != "" {
				ui.Text(c, subtitle).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine().Ellipsis("…")
			}
		})
		if meta != nil {
			snapshot := *meta
			more := ui.ButtonBase(c).Size(28, 28).Radius(Radius().Control).Shrink(0).Center().
				Label("More actions for " + title)
			if more.Hovered() {
				more.Background(tokens.SidebarHover)
			}
			more.Children(func() { ui.Icon(c, iconEllipsis).Size(14, 14).TextColor(t.TextMuted) })
			more.Menu(func(m *ui.Menu) { s.sessionMenu(m, snapshot, false) })
		}
	})
}

// nonEmptyStrings drops the blank entries, keeping order.
func nonEmptyStrings(values []string) []string {
	out := make([]string, 0, len(values))
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			out = append(out, value)
		}
	}
	return out
}
