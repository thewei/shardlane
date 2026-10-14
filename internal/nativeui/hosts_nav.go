package nativeui

/**
 * [INPUT]: 依赖 Shell.hosts 索引态与分组（hostGroups）、machineInstances/machineMenuSuffix、sidebarTreeRow/sidebarSectionHeader/sectionLabel
 * [OUTPUT]: /hosts 的侧栏导航：SSH Config 条目（配置自带分组时按组可折叠；行悬停连接、连接/注册机器/编辑文件上下文菜单 hostEntryMenu）+ Herdr Machines；列表头的图标刷新是该页唯一的 Refresh；列表是 listNav 的一个焦点组，光标移动即选中（分组头除外）
 * [POS]: 内页侧栏宿主（sidebar.go 按 routeHosts 分派到这里）；行与分组头与 Git/Files 共用同一套树网格；机器管理动作留在 Session selector，不复制第二实现
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/sshconfig"
)

// hostsNav is the Hosts page's sidebar body: the config index above (under
// collapsible group headers when the config groups itself), the saved Herdr
// machines below. The list's one refresh is an icon in its head —
// the same control every list-backed sidebar uses (the project tree's) — and
// an action never wears a navigation row's face. The listing scrolls
// (quick-panel pattern) — a long config must never overflow the sidebar
// column.
func (s *Shell) hostsNav(c *ui.Context) {
	k := gorexColorsOf(c.Theme().Dark)
	// No inset of its own: sidebarInnerNav already insets every inner page's
	// sidebar body, and a second pass pushed this list 14pt down and 9pt in.
	ui.Column(c).Grow(1).MinHeight(0).Gap(3).Children(func() {
		ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
			sectionLabel(c, "SSH Config")
			ui.Box(c).Grow(1).PassThrough()
			if gorexIconButton(c, k, iconRefresh, "Refresh", "Refresh", false, gorexPaneBtn, gorexPaneGlyph).Clicked() {
				s.refreshHostsIndex(true)
				s.refreshMachineStatus(true)
			}
		})
		s.navList(c, &s.nav.hosts, true, ui.Scroll(c).Grow(1).MinHeight(0), func() {
			s.hostsNavEntries(c)
			s.nav.hosts.barrier() // the machines are not part of the last group
			sectionLabel(c, "Herdr Machines")
			s.hostsNavMachines(c)
		})
	})
}

// followHostCursor is the Hosts list's OnMove: the selection follows the
// keyboard cursor, as a click selects, except on a group header.
func (s *Shell) followHostCursor(key string) {
	if !strings.HasPrefix(key, hostGroupKeyPrefix) {
		s.hosts.selected = key
	}
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
		key := hostMachineKeyPrefix + inst.Machine.ID
		label := s.machineNavLabel(inst)
		_, clicked := s.sidebarTreeRow(c, sidebarTreeSpec{
			Key: key, Title: label, Tooltip: label, Glyph: iconServer, Selected: s.hosts.selected == key, Nav: &s.nav.hosts,
		})
		if clicked {
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
	groups := hostGroups(s.hosts.entries, s.hosts.groups, s.hostsMainFile())
	if groups == nil {
		for _, entry := range s.hosts.entries {
			s.hostsNavRow(c, entry)
		}
		return
	}
	for _, g := range groups {
		closed := s.hosts.closed[g.Key]
		_, clicked := s.sidebarSectionHeader(c, sidebarSectionSpec{
			Key: hostGroupKeyPrefix + g.Key, Title: g.Label, Count: len(g.Entries), Closed: closed, Tooltip: g.Tip,
			Nav: &s.nav.hosts,
		})
		if clicked {
			if s.hosts.closed == nil {
				s.hosts.closed = map[string]bool{}
			}
			s.hosts.closed[g.Key] = !closed
		}
		if closed {
			continue
		}
		for _, entry := range g.Entries {
			s.hostsNavRow(c, entry)
		}
	}
}

// hostsNavRow is one config entry: select on click, Connect on the hover
// icon of a connectable alias, the full action menu on right-click.
func (s *Shell) hostsNavRow(c *ui.Context, entry sshconfig.Entry) {
	key := hostEntryKey(entry)
	spec := sidebarTreeSpec{
		Key: key, Title: entry.Title(), Tooltip: entry.Title(), Glyph: iconGlobe,
		Selected: s.hosts.selected == key, Nav: &s.nav.hosts,
		Menu: func(m *ui.Menu) { s.hostEntryMenu(m, entry) },
	}
	if alias := entry.Alias(); alias != "" {
		spec.Action = &treeRowAction{Label: "Connect", Glyph: iconTerminal, Run: func() { s.connectHost(alias) }}
	}
	if _, clicked := s.sidebarTreeRow(c, spec); clicked {
		s.hosts.selected = key
	}
}

// hostEntryMenu is one host's actions, the same list on its sidebar row and
// in the title bar's Actions menu. Patterns and Match blocks are ssh's own
// business, so only a concrete alias connects or registers.
func (s *Shell) hostEntryMenu(m *ui.Menu, entry sshconfig.Entry) {
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
