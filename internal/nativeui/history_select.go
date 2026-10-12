package nativeui

/**
 * [INPUT]: 依赖 Sessions 列表当前可见分组、settings.Session.HiddenSessions 与 MyGo Checkbox/Button
 * [OUTPUT]: 提供 Sessions 多选模式（选择状态、分组全选、底部选择条）与批量 Hide/Restore 的单一写入口 setSessionsHidden
 * [POS]: Sessions 页的批量整理入口；只改本地隐藏列表，绝不触碰 Provider 会话文件
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/settings"
)

// hiddenSessionsLimit bounds the local hide list.
const hiddenSessionsLimit = 500

// setSessionsHidden hides or restores sessions in one settings write — the
// local list only, provider files are never touched. Keys keep the order
// given; the list stops growing at hiddenSessionsLimit.
func (s *Shell) setSessionsHidden(keys []string, hide bool) {
	if len(keys) == 0 {
		return
	}
	touched := make(map[string]bool, len(keys))
	for _, key := range keys {
		touched[key] = true
	}
	s.applySettings(func(v *settings.Settings) error {
		next := make([]string, 0, len(v.Session.HiddenSessions)+len(keys))
		for _, existing := range v.Session.HiddenSessions {
			if !touched[existing] {
				next = append(next, existing)
			}
		}
		if hide {
			added := make(map[string]bool, len(keys))
			for _, key := range keys {
				if len(next) >= hiddenSessionsLimit {
					break
				}
				if !added[key] {
					added[key] = true
					next = append(next, key)
				}
			}
		}
		v.Session.HiddenSessions = next
		return nil
	})
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
// the count, select-all/clear, the Hide (or Restore, in the Hidden view)
// action and Done.
func (s *Shell) historySelectionBar(c *ui.Context, groups []sessionGroupView) {
	sp := Spacing()
	t := c.Theme()
	tokens := designTokens(t.Dark)
	selected := s.selectedSessionKeys(groups)
	visible := sessionKeys(groups)
	verb := "Hide"
	if s.hist.showHidden {
		verb = "Restore"
	}
	ui.Column(c).FillWidth().Shrink(0).Children(func() {
		ui.Box(c).FillWidth().Height(1).Background(tokens.BorderSubtle)
		ui.Row(c).FillWidth().Padding(sp.M, sp.XL).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			ui.Text(c, fmt.Sprintf("%d selected", len(selected))).FontSize(Typography().Body).FontWeight(WeightMedium)
			if len(selected) < len(visible) {
				if ui.Button(c, "Select all").Clicked() {
					s.setHistorySelected(visible, true)
				}
			}
			if len(selected) > 0 {
				if ui.Button(c, "Clear").Clicked() {
					s.clearHistorySelection()
				}
			}
			ui.Spacer(c)
			if ui.PrimaryButton(c, verb).Disabled(len(selected) == 0).Clicked() {
				done := "Hid"
				if s.hist.showHidden {
					done = "Restored"
				}
				s.setSessionsHidden(selected, !s.hist.showHidden)
				s.status = fmt.Sprintf("%s %d session%s", done, len(selected), pluralS(len(selected)))
				s.setHistorySelecting(false)
			}
			if ui.Button(c, "Done").Clicked() {
				s.setHistorySelecting(false)
			}
		})
	})
}
