package nativeui

/**
 * [INPUT]: 依赖 internal/sshconfig（本地 ~/.ssh/config 行级索引与条目事实）、Shell 机器投影（machineInstances/machineStates）、共享 DS 组件
 * [OUTPUT]: /hosts 内页入口 showViewHosts/hostsPage：主机条目详情（标题行主操作 + 一个分组指令列表，编辑时就地变输入框）、Herdr 机器详情与空态
 * [POS]: 本地客户端能力页（类比 Git Workbench 的本地事实投影）：只读展示 + 显式保存；连接与机器管理一律走 Herdr 既有通道，不建第二注册表
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

// hostsPageState is session-only presentation state for the Hosts page. The
// index and editor text live in memory only; the config file is written
// exclusively through an explicit editor save (hosts_actions.go).
type hostsPageState struct {
	loaded  bool
	loading bool
	missing bool
	err     string

	mainPath string
	entries  []sshconfig.Entry
	groups   []sshconfig.Group
	files    []string

	// closed folds the sidebar's groups by hostGroup.Key; session-only.
	closed map[string]bool

	selected string // hostEntryKey or hostMachineKeyPrefix+profile id

	editing            bool
	editorFile         string
	editorText         string
	editorLoaded       string
	editorErr          string
	editorConfirmClose bool
	saving             bool

	effOpen bool
	effBusy bool
	effErr  string
	effFor  string
	eff     []sshconfig.KV

	// form is the structured single-host draft (hosts_form.go); the raw
	// editor above stays the full-fidelity editing path.
	form hostsFormState
}

const hostMachineKeyPrefix = "machine:"

// hostEntryKey namespaces one index entry by its defining file and line.
func hostEntryKey(entry sshconfig.Entry) string {
	return "host:" + entry.File + ":" + itoa(entry.Line)
}

// showViewHosts toggles the Hosts workbench; a pending Commit draft must be
// explicitly dismissed first, like every other destination switch.
func (s *Shell) showViewHosts() {
	if s.holdCommitNavigation() {
		return
	}
	if s.router.Path() == routeHosts {
		s.showViewTerminal()
		return
	}
	s.router.Push(routeHosts)
}

// hostsPage is the route content: the detail center, content only. The title
// bar names the page and the config it indexes (route_header.go) and the
// sidebar owns the one Refresh. The host list lives in the shared sidebar
// (hostsNav); the page never touches the config file on the render lane —
// ensureHostsIndex refreshes the index in the background.
func (s *Shell) hostsPage(c *ui.Context) {
	s.ensureHostsIndex()
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		switch {
		case s.hosts.editing:
			s.hostsEditor(c)
		case strings.HasPrefix(s.hosts.selected, hostMachineKeyPrefix):
			s.hostMachineDetail(c)
		default:
			if entry, ok := s.selectedHostEntry(); ok {
				s.hostEntryDetail(c, entry)
			} else {
				s.hostsEmptyState(c)
			}
		}
	})
}

// hostsMainFile is the main config as entries name it. The index resolves
// symlinks for the files it reads, so a linked ~/.ssh/config reads back under
// its target's path — comparing entries against mainPath would call every
// host of such a config "included".
func (s *Shell) hostsMainFile() string {
	if len(s.hosts.files) > 0 {
		return s.hosts.files[0]
	}
	return s.hosts.mainPath
}

// selectedHostEntry resolves the selected key against the current index;
// a stale key (refreshed index, edited file) falls back to the empty state.
func (s *Shell) selectedHostEntry() (sshconfig.Entry, bool) {
	if !strings.HasPrefix(s.hosts.selected, "host:") {
		return sshconfig.Entry{}, false
	}
	for _, entry := range s.hosts.entries {
		if hostEntryKey(entry) == s.hosts.selected {
			return entry, true
		}
	}
	return sshconfig.Entry{}, false
}

func (s *Shell) hostsEmptyState(c *ui.Context) {
	switch {
	case s.hosts.loading && !s.hosts.loaded:
		loadingState(c, "Reading SSH config…")
	case s.hosts.err != "":
		ui.Column(c).Grow(1).Center().Gap(Spacing().S).Children(func() {
			emptyState(c, "SSH config unavailable", s.hosts.err)
			if ui.Button(c, "Retry").Clicked() {
				s.refreshHostsIndex(true)
			}
		})
	case s.hosts.missing:
		ui.Column(c).Grow(1).Center().Gap(Spacing().S).Children(func() {
			emptyState(c, "No SSH config", "ssh reads host aliases from ~/.ssh/config.")
			if ui.PrimaryButton(c, "Create ~/.ssh/config").Clicked() {
				s.openHostEditor(s.hosts.mainPath, true)
			}
		})
	default:
		emptyState(c, "Select a host", "")
	}
}

// hostsDetailPage is the scrolling, centered column every Hosts detail sits
// in — the same inset and rhythm as Settings and the Agent inspector.
func hostsDetailPage(c *ui.Context, body func()) {
	sp := Spacing()
	ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
		contentColumn(c, formColumnWidth, func() {
			ui.Column(c).FillWidth().Padding(pageTopInset(), sp.XL, sp.XL).Gap(sp.XL).Children(body)
		})
	})
}

// hostEntryDetail renders one Host/Match block: a heading with its primary
// actions, the block's directives in one grouped list that turns into inputs
// while the block is edited, and the system client's effective config on
// demand.
func (s *Shell) hostEntryDetail(c *ui.Context, entry sshconfig.Entry) {
	t := c.Theme()
	f := &s.hosts.form
	editing := f.open && f.entryKey == hostEntryKey(entry) && hostFormEditable(entry)
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		if editing && f.err != "" {
			inlineNotice(c, ToneError, f.err, nil)
		}
		hostsDetailPage(c, func() {
			s.hostHeading(c, entry, editing)
			if editing && f.confirmClose {
				ui.Row(c).FillWidth().Gap(Spacing().S).AlignItems(ui.Center).Children(func() {
					hostsDiscardPrompt(c, func() { f.confirmClose = false }, func() { s.hosts.form = hostsFormState{} })
				})
			}
			s.hostFacts(c, entry, editing)
			if editing {
				ui.Text(c, "Comments and other directives stay as they are.").
					FontSize(Typography().Caption).TextColor(t.TextMuted)
				return
			}
			s.hostEffectiveSection(c, entry.Alias())
		})
	})
}

// hostHeading is the detail's title row: the host and where it is defined on
// the left, its actions on the right. Connect is the one primary action; the
// pencil edits in place (a structured form for a plain host, the file's raw
// editor for anything ssh resolves itself). Editing swaps both for
// Cancel/Save, so the row never changes height.
func (s *Shell) hostHeading(c *ui.Context, entry sshconfig.Entry, editing bool) {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	k := gorexColorsOf(t.Dark)
	ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(sp.XXS).Children(func() {
			ui.Text(c, entry.Title()).FontSize(typ.Heading).FontWeight(WeightSemibold).
				SingleLine().Ellipsis("…").MinWidth(0).Tooltip(entry.Title())
			ui.Text(c, hostSubtitle(entry)).FontSize(typ.Caption).TextColor(t.TextMuted).
				SingleLine().Ellipsis("…").MinWidth(0)
		})
		if editing {
			if s.hosts.form.saving {
				ui.Spinner(c).Size(14, 14)
			}
			if ui.Button(c, "Cancel").Clicked() {
				s.cancelHostForm()
			}
			if ui.PrimaryButton(c, "Save").Clicked() {
				s.saveHostForm(entry)
			}
			return
		}
		if alias := entry.Alias(); alias != "" {
			if ui.PrimaryButton(c, "Connect").Clicked() {
				s.connectHost(alias)
			}
		}
		if hostFormEditable(entry) {
			if gorexIconButton(c, k, iconEdit, "Edit", "Edit", false, gorexIconBtn, gorexIconGlyph).Clicked() {
				s.openHostForm(entry)
			}
		} else if gorexIconButton(c, k, iconEdit, "Edit File", "Edit File", false, gorexIconBtn, gorexIconGlyph).Clicked() {
			s.openHostEditor(entry.File, false)
		}
	})
}

// hostSubtitle is the heading's quiet second line: where the block is
// defined, after a word for what it is when it is not a plain host — the
// reason it has no Connect.
func hostSubtitle(entry sshconfig.Entry) string {
	where := fmt.Sprintf("%s:%d", compactSearchPath(entry.File), entry.Line)
	switch {
	case entry.Kind == sshconfig.KindMatch:
		return "Match rule · " + where
	case entry.Alias() != "":
		return where
	case len(entry.Patterns) > 1:
		return "Several patterns · " + where
	}
	return "Wildcard pattern · " + where
}

// hostFacts is the block's directives in one grouped list. While the host is
// edited its alias and the managed directives lead as inputs and the rest of
// the block stays read-only beneath them.
func (s *Shell) hostFacts(c *ui.Context, entry sshconfig.Entry, editing bool) {
	facts := entry.Facts()
	var rows []settingsRow
	if editing {
		rows = s.hostFormRows(c, entry)
		facts = unmanagedDirectives(facts)
	}
	for _, kv := range facts {
		kv := kv
		rows = append(rows, row(kv.Key, "", func() { valueText(c, kv.Value) }))
	}
	if len(rows) == 0 {
		ui.Text(c, "No directives.").FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
		return
	}
	settingsGroup(c, "", rows...)
}

// hostMachineDetail renders one saved Herdr machine profile read-only: the
// profile is Herdr's own client state, so the page offers no editing here.
func (s *Shell) hostMachineDetail(c *ui.Context) {
	id := strings.TrimPrefix(s.hosts.selected, hostMachineKeyPrefix)
	var inst *herdr.Instance
	for i := range s.instances {
		if m := s.instances[i].Machine; m != nil && m.ID == id {
			inst = &s.instances[i]
			break
		}
	}
	if inst == nil || inst.Machine == nil {
		s.hostsEmptyState(c)
		return
	}
	machine := inst.Machine
	label := fallbackText(inst.DisplayName, machine.Label, machine.ID)
	sp := Spacing()
	t := c.Theme()
	state, known := s.machineStates[machine.ID]
	hostsDetailPage(c, func() {
		ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
			ui.Text(c, label).FontSize(Typography().Heading).FontWeight(WeightSemibold).
				SingleLine().Ellipsis("…").Shrink(1).MinWidth(0).Tooltip(label)
			if known {
				switch {
				case state.Reachable:
					statusPill(c, "reachable", ToneSuccess)
				case state.Attention:
					statusPill(c, "needs attention", ToneAttention)
				default:
					statusPill(c, "unreachable", ToneNeutral)
				}
			}
			ui.Box(c).Grow(1).PassThrough()
			if ui.PrimaryButton(c, "Open Workspace").Clicked() {
				s.openInstance(inst.Name)
			}
		})
		rows := []settingsRow{row("SSH target", "", func() { valueText(c, machine.Target) })}
		if machine.Session != "" {
			rows = append(rows, row("Remote session", "", func() { valueText(c, machine.Session) }))
		}
		enabled := "disabled"
		if machine.Enabled {
			enabled = "enabled"
		}
		rows = append(rows, row("Profile", "", func() { valueText(c, enabled) }))
		settingsGroup(c, "", rows...)
		ui.Column(c).FillWidth().Gap(sp.XXS).Children(func() {
			if known && state.Attention && state.Message != "" {
				ui.Text(c, state.Message).FontSize(Typography().Caption).TextColor(t.TextMuted)
			}
			ui.Text(c, "Rename or forget this machine from the Session selector.").
				FontSize(Typography().Caption).TextColor(t.TextMuted)
		})
	})
}
