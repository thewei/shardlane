package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖 mygo/ui 的 ButtonBase、Spinner，agent_visual 的品牌 mark 与状态点、git_meta 的 git 事实
 * [OUTPUT]: 对外提供 Shell.agentCardRow（侧栏 Agents 区与 Agent 活动浮层共享的两行卡片）
 * [POS]: Agent 卡片的唯一呈现实现（2026-10-07）：品牌 mark + 角落状态点 + 项目/标题行 + git 事实行；工作中卡片尾随 Spinner（card.Working，与目录 Working 桶同一语义）；点击语义留给调用方
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// agentCardRow is the two-line Agent card shared by the sidebar's Agents
// section and the floating agent activity panel (2026-10-07): the brand
// mark with the operational state as a corner dot, the project (or agent
// title) on the first line, and the git facts (or the agent title) on the
// second. Status words are deliberately absent — the dot is the state,
// and a working agent spins at the row's trailing edge so mid-flight work
// reads at a glance. Click handling stays with the caller; the returned
// element carries the Clicked/ContextMenu surface.
func (s *Shell) agentCardRow(c *ui.Context, card agent.AgentCardModel, selected bool) *ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	mark := agentMark(card.Provider, t.Dark)
	label := card.ProjectName
	if label == "" {
		label = card.Title
	}
	var row *ui.Element
	ui.Box(c).Key("agent:" + card.PaneID).Children(func() {
		row = ui.ButtonBase(c).
			FillWidth().
			MinHeight(38).
			Radius(6).
			Padding(0, 8).
			Gap(8).
			AlignItems(ui.Center).
			Label(label)
		if tint, ok := rowTint(tokens, selected, row.Hovered()); ok {
			row.Background(tint)
		}
		row.Children(func() {
			ui.Box(c).Size(18, 18).Shrink(0).Children(func() {
				markView(c, mark, 18, t.TextMuted)
				// Always shown; gray means idle (round three follow-up).
				ui.Box(c).Absolute().Left(11).Top(10).Children(func() {
					statusDot(c, operationalState(card.Attention))
				})
			})
			ui.Column(c).Grow(1).MinWidth(0).Gap(0).Children(func() {
				ui.Text(c, label).FontSize(Typography().Body).SingleLine()
				if !s.agentFactsView(c, card) && card.Title != label {
					ui.Text(c, card.Title).FontSize(9).TextColor(t.TextMuted).SingleLine()
				}
			})
			// A working (or launching) agent spins at the trailing edge:
			// mid-flight work reads at a glance, with no status words —
			// the row stays word-free by contract, so the spinner is
			// unnamed; AT already reads it as a progress indicator inside
			// the labeled row. The spinner paints only while its row
			// shows (mygo drives the frames), so idle agents cost nothing.
			if card.Working() {
				ui.Spinner(c)
			}
			if card.Unread {
				ui.Box(c).Size(6, 6).Radius(3).Background(tokens.StatusColor(ToneAttention, t.Dark)).Shrink(0)
			}
		})
	})
	return row
}

// agentFactsView renders the git facts of the card's Pane cwd with
// distinct glyphs and colors (2026-10-05 round three): branch behind a
// branch glyph, the changed-file count behind a file glyph, and the
// additions/deletions in success/danger colors. It reports false until
// the background meta produced facts (the caller then falls back to the
// agent title).
func (s *Shell) agentFactsView(c *ui.Context, card agent.AgentCardModel) bool {
	t := c.Theme()
	cwd := ""
	if pane := paneByID(s.projection, card.PaneID); pane != nil {
		cwd = pane.CWD
	}
	branch, adds, dels, files, ok := s.gitMetaForCWD(cwd)
	if !ok || (branch == "" && adds == 0 && dels == 0 && files == 0) {
		return false
	}
	ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
		if branch != "" {
			ui.Icon(c, iconBranch).Size(9, 9).TextColor(t.TextMuted)
			ui.Text(c, branch).FontSize(9).TextColor(t.TextMuted).SingleLine()
		}
		if files != 0 {
			ui.Icon(c, iconFile).Size(9, 9).TextColor(t.TextMuted)
			ui.Text(c, itoa(files)).FontSize(9).TextColor(t.TextMuted)
		}
		if adds != 0 {
			ui.Text(c, "+"+itoa(adds)).FontSize(9).TextColor(t.Success)
		}
		if dels != 0 {
			ui.Text(c, "−"+itoa(dels)).FontSize(9).TextColor(t.Danger)
		}
	})
	return true
}
