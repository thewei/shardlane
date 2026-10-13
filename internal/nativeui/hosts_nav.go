package nativeui

/**
 * [INPUT]: 依赖 Shell.hosts 索引态、machineInstances/machineMenuSuffix、navButton/sectionLabel
 * [OUTPUT]: /hosts 的侧栏导航：SSH Config 条目（连接/注册机器/编辑文件上下文菜单）+ Herdr Machines + Refresh
 * [POS]: 内页侧栏宿主（sidebar.go 按 routeHosts 分派到这里）；机器管理动作留在 Session selector，不复制第二实现
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// hostsNav is the Hosts page's sidebar body: the config index above, the
// saved Herdr machines below, one refresh action at the foot. The listing
// scrolls (quick-panel pattern) — a long config must never overflow the
// sidebar column.
func (s *Shell) hostsNav(c *ui.Context) {
	ui.Column(c).Grow(1).MinHeight(0).Padding(14, 9, 9).Gap(3).Children(func() {
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			sectionLabel(c, "SSH Config")
			s.hostsNavEntries(c)
			sectionLabel(c, "Herdr Machines")
			s.hostsNavMachines(c)
		})
		if navButton(c, iconRefresh, "Refresh", "", false).Clicked() {
			s.refreshHostsIndex(true)
			s.refreshMachineStatus(true)
		}
	})
}

// hostsNavMachines renders the saved Herdr machines below the config list.
func (s *Shell) hostsNavMachines(c *ui.Context) {
	t := c.Theme()
	machines := s.machineInstances()
	if len(machines) == 0 {
		ui.Text(c, "None saved").FontSize(Typography().BodySmall).TextColor(t.TextMuted).Padding(6, 8)
		return
	}
	for _, inst := range machines {
		inst := inst
		key := hostMachineKeyPrefix + inst.Machine.ID
		b := navButton(c, iconGlobe, s.machineNavLabel(inst), "", s.hosts.selected == key)
		if b.Clicked() {
			s.hosts.selected = key
		}
	}
}

func (s *Shell) hostsNavEntries(c *ui.Context) {
	t := c.Theme()
	switch {
	case s.hosts.loading && !s.hosts.loaded:
		ui.Row(c).Padding(6, 8).Gap(6).AlignItems(ui.Center).Children(func() {
			ui.Spinner(c).Size(14, 14)
			ui.Text(c, "Reading…").FontSize(Typography().BodySmall).TextColor(t.TextMuted)
		})
		return
	case s.hosts.missing:
		ui.Text(c, "No config file").FontSize(Typography().BodySmall).TextColor(t.TextMuted).Padding(6, 8)
		return
	case s.hosts.err != "" && len(s.hosts.entries) == 0:
		ui.Text(c, "Config unavailable").FontSize(Typography().BodySmall).TextColor(t.TextMuted).Padding(6, 8)
		return
	case len(s.hosts.entries) == 0:
		ui.Text(c, "No hosts yet").FontSize(Typography().BodySmall).TextColor(t.TextMuted).Padding(6, 8)
		return
	}
	for _, entry := range s.hosts.entries {
		entry := entry
		key := hostEntryKey(entry)
		b := navButton(c, iconGlobe, entry.Title(), "", s.hosts.selected == key)
		if b.Clicked() {
			s.hosts.selected = key
		}
		b.ContextMenu(func(m *ui.Menu) {
			if alias := entry.Alias(); alias != "" {
				if m.Item("Connect").Chosen() {
					s.connectHost(alias)
				}
				if m.Item("Add Herdr Machine…").Chosen() {
					s.registerHostMachine(alias)
				}
			}
			if m.Item("Edit File").Chosen() {
				s.openHostEditor(entry.File, false)
			}
		})
	}
}

// machineNavLabel renders one saved machine row: display name plus the same
// availability suffix the Session selector uses.
func (s *Shell) machineNavLabel(inst herdr.Instance) string {
	label := inst.DisplayName
	if label == "" && inst.Machine != nil {
		label = inst.Machine.Label
	}
	if label == "" && inst.Machine != nil {
		label = inst.Machine.ID
	}
	suffix := ""
	if inst.Machine != nil {
		state, known := s.machineStates[inst.Machine.ID]
		suffix = machineMenuSuffix(inst.Machine, state, known)
	}
	return fmt.Sprintf("%s%s", label, suffix)
}
