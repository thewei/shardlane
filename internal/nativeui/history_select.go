package nativeui

/**
 * [INPUT]: 依赖 Sessions 列表当前可见分组（会话或回收站条目）与 MyGo Checkbox/Button
 * [OUTPUT]: 提供 Sessions 多选模式（选择状态、分组全选、底部选择条）与批量「移入回收站」/回收站内的批量「放回」「永久删除」
 * [POS]: Sessions 页的批量整理入口；真正的文件搬动在 history_trash.go，本文件只管选择状态与选择条
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"

	"github.com/egoist/mygo/ui"
)

// historySelecting says whether rows are selectable: in select mode, and
// always in the Trash, where every action is about the ticked entries.
func (s *Shell) historySelecting() bool { return s.hist.selecting || s.hist.showTrash }

// selectionBarShown says whether the batch bar is up: while selecting, and
// in the Trash once something is ticked (its rows carry their own menu, so
// the bar only appears for a batch).
func (s *Shell) selectionBarShown() bool {
	if s.hist.showTrash {
		return len(s.hist.selected) > 0
	}
	return s.hist.selecting
}

// setHistorySelecting enters or leaves the multi-select mode; leaving drops
// the selection, and the inline preview never coexists with it.
func (s *Shell) setHistorySelecting(on bool) {
	s.hist.selecting = on
	s.clearHistorySelection()
	if on {
		s.closeHistoryPreview()
	}
}

func (s *Shell) clearHistorySelection() {
	s.hist.selected = nil
}

func (s *Shell) setHistorySelected(keys []string, on bool) {
	if s.hist.selected == nil {
		s.hist.selected = make(map[string]bool, len(keys))
	}
	for _, key := range keys {
		if on {
			s.hist.selected[key] = true
		} else {
			delete(s.hist.selected, key)
		}
	}
}

// sessionKeys lists the sessions of the given groups in view order.
func sessionKeys(groups []sessionGroupView) []string {
	var keys []string
	for _, group := range groups {
		for _, summary := range group.sessions {
			keys = append(keys, summary.Meta.Key)
		}
	}
	return keys
}

// selectedSessionKeys is the ticked subset of the visible sessions, in view
// order — what a batch action applies to.
func (s *Shell) selectedSessionKeys(groups []sessionGroupView) []string {
	var keys []string
	for _, key := range sessionKeys(groups) {
		if s.hist.selected[key] {
			keys = append(keys, key)
		}
	}
	return keys
}

// selectionMark draws a check box glyph that only shows state: its owner
// (the row or the header button around it) handles the click. A glyph, not
// the Checkbox widget — the widget writes through the pointer it was built
// with, which here would be a per-frame copy, and a passed-through widget
// swallows the owner's click.
func selectionMark(c *ui.Context, checked bool) {
	t := c.Theme()
	if checked {
		ui.Icon(c, iconTask).Size(16, 16).TextColor(t.Accent).Shrink(0).PassThrough()
		return
	}
	ui.Icon(c, iconSquare).Size(16, 16).TextColor(t.TextMuted).Shrink(0).PassThrough()
}

// groupSelectionBox is a project header's checkbox: checked when every
// session of the group is, a click ticks them all — or clears them when
// they all were.
func (s *Shell) groupSelectionBox(c *ui.Context, group sessionGroupView) {
	keys := sessionKeys([]sessionGroupView{group})
	all := len(keys) > 0
	for _, key := range keys {
		if !s.hist.selected[key] {
			all = false
			break
		}
	}
	hit := ui.ButtonBase(c).Padding(Spacing().XS).Radius(Radius().Control).Shrink(0).
		Label("Select every session in " + group.name)
	hit.Children(func() { selectionMark(c, all) })
	if hit.Clicked() {
		s.setHistorySelected(keys, !all)
	}
}

// historySelectionBar is the batch strip under the list while selecting:
// the count, select-all/clear, the batch actions — Move to Trash over
// sessions; Put Back and Delete Forever over Trash entries — and Done.
func (s *Shell) historySelectionBar(c *ui.Context, groups []sessionGroupView) {
	sp := Spacing()
	t := c.Theme()
	tokens := designTokens(t.Dark)
	selected := s.selectedSessionKeys(groups)
	visible := sessionKeys(groups)
	ui.Column(c).FillWidth().Shrink(0).Children(func() {
		ui.Box(c).FillWidth().Height(1).Background(tokens.BorderSubtle)
		ui.Row(c).FillWidth().Padding(sp.M, sp.XL).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			ui.Text(c, fmt.Sprintf("%d selected", len(selected))).FontSize(Typography().Body).FontWeight(WeightMedium)
			if len(selected) < len(visible) {
				if ui.Button(c, "Select All").Clicked() {
					s.setHistorySelected(visible, true)
				}
			}
			if len(selected) > 0 {
				if ui.Button(c, "Clear").Clicked() {
					s.clearHistorySelection()
				}
			}
			ui.Spacer(c)
			if s.hist.showTrash {
				if ui.Button(c, "Delete Forever…").Disabled(len(selected) == 0).Clicked() {
					s.confirmDeleteForever(selected)
				}
				if ui.PrimaryButton(c, "Put Back").Disabled(len(selected) == 0).Clicked() {
					s.restoreFromTrash(selected)
					s.clearHistorySelection()
				}
				return
			}
			if ui.PrimaryButton(c, "Move to Trash").Disabled(len(selected) == 0).Clicked() {
				s.moveSessionsToTrash(s.sessionMetasForKeys(selected))
				s.setHistorySelecting(false)
			}
			if ui.Button(c, "Done").Clicked() {
				s.setHistorySelecting(false)
			}
		})
	})
}
