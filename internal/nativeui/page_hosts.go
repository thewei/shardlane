package nativeui

/**
 * [INPUT]: 依赖 internal/sshconfig（本地 ~/.ssh/config 行级索引与条目事实）、Shell 机器投影（machineInstances/machineStates）、共享 DS 组件
 * [OUTPUT]: /hosts 内页入口 showViewHosts/hostsPage：主机条目详情、Herdr 机器详情与空态
 * [POS]: 本地客户端能力页（类比 Git Workbench 的本地事实投影）：只读展示 + 显式保存；连接与机器管理一律走 Herdr 既有通道，不建第二注册表
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
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
	files    []string

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
			emptyState(c, "No SSH config",
				"Create ~/.ssh/config to keep host aliases; ssh reads it on every connection.")
			if ui.PrimaryButton(c, "Create ~/.ssh/config").Clicked() {
				s.openHostEditor(s.hosts.mainPath, true)
			}
		})
	default:
		emptyState(c, "Select a host", "Pick an entry in the sidebar to connect, inspect or edit it.")
	}
}

// hostEntryDetail renders one Host/Match block: identity, provenance, the
// indexed directive facts, and the actions its shape allows.
func (s *Shell) hostEntryDetail(c *ui.Context, entry sshconfig.Entry) {
	t := c.Theme()
	sp := Spacing()
	alias := entry.Alias()
	ui.Column(c).FillWidth().Grow(1).MinHeight(0).Children(func() {
		contentColumn(c, formColumnWidth, func() {
			ui.Column(c).FillWidth().Padding(sp.L, 0, sp.XL).Gap(sp.L).Children(func() {
				ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
					ui.Text(c, entry.Title()).FontSize(Typography().Title).
						FontWeight(WeightBold).Grow(1).MinWidth(0).SingleLine()
					if entry.Kind == sshconfig.KindMatch {
						statusPill(c, "match", ToneNeutral)
					} else if alias == "" {
						statusPill(c, "pattern", ToneNeutral)
					}
					if entry.File != s.hosts.mainPath {
						statusPill(c, "included", ToneInfo)
					}
				})
				ui.Textf(c, "%s:%d", compactSearchPath(entry.File), entry.Line).
					FontSize(Typography().Caption).TextColor(t.TextMuted)

				ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
					if alias != "" {
						if ui.PrimaryButton(c, "Connect").Clicked() {
							s.connectHost(alias)
						}
						if ui.Button(c, "Add Herdr Machine…").Clicked() {
							s.registerHostMachine(alias)
						}
					} else {
						ui.Text(c, hostsConnectHint(entry)).FontSize(Typography().Caption).
							TextColor(t.TextMuted).Grow(1)
					}
					if compactButton(c, "Edit File").Clicked() {
						s.openHostEditor(entry.File, false)
					}
				})

				s.hostsFormSection(c, entry)

				settingsCard(c, "Directives", func() {
					facts := entry.Facts()
					if len(facts) == 0 {
						ui.Text(c, "No directives in this block.").FontSize(Typography().BodySmall).TextColor(t.TextMuted)
					}
					for _, kv := range facts {
						formRow(c, kv.Key, kv.Value, nil)
					}
				})
				s.hostEffectiveSection(c, alias)
			})
		})
	})
}

// hostsConnectHint explains why an entry has no Connect action: ssh itself
// resolves patterns and Match blocks, so the page never guesses a name.
func hostsConnectHint(entry sshconfig.Entry) string {
	if entry.Kind == sshconfig.KindMatch {
		return "Match blocks are resolved by ssh itself — connect from a concrete host."
	}
	if len(entry.Patterns) > 1 {
		return "This line lists several patterns — edit the file to connect to one of them."
	}
	return "Wildcard patterns are resolved by ssh — connect from a concrete alias."
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
	label := inst.DisplayName
	if label == "" {
		label = machine.Label
	}
	if label == "" {
		label = machine.ID
	}
	sp := Spacing()
	t := c.Theme()
	ui.Column(c).FillWidth().Grow(1).MinHeight(0).Children(func() {
		contentColumn(c, formColumnWidth, func() {
			ui.Column(c).FillWidth().Padding(sp.L, 0, sp.XL).Gap(sp.L).Children(func() {
				ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
					ui.Text(c, label).FontSize(Typography().Title).
						FontWeight(WeightBold).Grow(1).MinWidth(0).SingleLine()
					state, known := s.machineStates[machine.ID]
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
				})
				settingsCard(c, "Machine", func() {
					formRow(c, "SSH target", machine.Target, nil)
					if machine.Session != "" {
						formRow(c, "Remote session", machine.Session, nil)
					}
					enabled := "disabled"
					if machine.Enabled {
						enabled = "enabled"
					}
					formRow(c, "Profile", enabled, nil)
				})
				ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
					if ui.PrimaryButton(c, "Open Workspace").Clicked() {
						s.openInstance(inst.Name)
					}
					ui.Text(c, "Machines are Herdr-managed profiles — add, rename or forget them from the Session selector.").
						FontSize(Typography().Caption).TextColor(t.TextMuted).Grow(1)
				})
				if state, known := s.machineStates[machine.ID]; known && state.Attention && state.Message != "" {
					ui.Text(c, state.Message).FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
			})
		})
	})
}
