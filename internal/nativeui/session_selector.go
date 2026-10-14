package nativeui

import (
	"path/filepath"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// sidebarSection is one collapsible block of the Workspace sidebar: the shared
// tree header (chevron, semibold title, count — the same row the Git
// references and the Hosts groups wear) over its items while open. The fold
// state is the caller's open flag.
func (s *Shell) sidebarSection(c *ui.Context, spec sidebarSectionSpec, open *bool, items func()) {
	ui.Column(c).Shrink(0).Children(func() {
		spec.Closed = !*open
		if _, clicked := s.sidebarSectionHeader(c, spec); clicked {
			*open = !*open
		}
		if *open {
			items()
		}
	})
}

// workspaceSection renders the Workspace section (2026-10-05 round three:
// "Projects" renamed to match the Herdr fact — a sidebar entry IS a Herdr
// Workspace) with a "+" on the header that creates one from a system
// folder.
func (s *Shell) workspaceSection(c *ui.Context) {
	add := &treeRowAction{Label: "New Workspace from Folder…", Glyph: iconPlus, Run: s.createWorkspaceFromFolder}
	s.sidebarSection(c, sidebarSectionSpec{
		Key: "section:workspace", Title: "Workspace", Count: len(s.projection.Projects), Action: add,
		Menu: func(m *ui.Menu) {
			if m.Item(add.Label).Chosen() {
				add.Run()
			}
		},
	}, &s.projectsOpen, func() {
		s.projectItems(c)
	})
}

// createWorkspaceFromFolder picks a system folder and creates a Herdr
// Workspace rooted there (workspace.create stays the runtime authority).
func (s *Shell) createWorkspaceFromFolder() {
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{
			Title:     "Choose a folder for the new Workspace",
			Directory: true,
		})
		s.applyOnUI(func() {
			if err != nil || len(paths) == 0 || paths[0] == "" {
				return
			}
			cwd := paths[0]
			label := filepath.Base(cwd)
			if s.activeInstance == "" {
				s.status = "Choose a Session first"
				return
			}
			if s.activeIsRemote() {
				// The folder picker can only see this machine's disk; a
				// remote workspace needs a path that exists on the remote.
				s.status = "Switch to a local Session to add a Workspace from a local folder"
				return
			}
			s.mutateSelecting(func() (herdr.Projection, error) {
				return s.runtime.CreateProject(s.activeInstance, cwd, label)
			})
		})
	}()
}

// sessionSelector is the sidebar footer (2026-10-05 round three): the
// workspace switcher renamed to what it binds — a Herdr Session — with
// left-aligned small text and a trailing chevron.
func (s *Shell) sessionSelector(c *ui.Context) {
	t := c.Theme()
	workspaceLabel := "Choose Session"
	if active := s.activeInstanceInfo(); active != nil {
		workspaceLabel = active.DisplayName
	}

	var selector ui.Element
	ui.Box(c).Grow(1).Children(func() {
		selector = ui.ButtonBase(c).
			FillWidth().
			Height(28).
			Radius(6).
			Padding(0, 8).
			Gap(6).
			AlignItems(ui.Center).
			Label("Session: " + workspaceLabel)
		if selector.Hovered() {
			selector.Background(t.SurfaceHover)
		}
		selector.Children(func() {
			leading := iconWorkspace
			if s.activeIsRemote() {
				leading = iconGlobe
			}
			ui.Icon(c, leading).Size(12, 12).TextColor(t.TextMuted).Shrink(0)
			ui.Text(c, workspaceLabel).FontSize(Typography().Caption).
				TextColor(t.Text).Grow(1).MinWidth(0).SingleLine()
			ui.Icon(c, iconChevronDown).Size(10, 10).TextColor(t.TextMuted).Shrink(0)
		})
	})
	selector.Menu(func(m *ui.Menu) {
		// Local sessions first: Herdr opens local immediately; machines are
		// background connections, never a reason to delay the local list.
		for _, instance := range s.instances {
			instance := instance
			if instance.Machine != nil {
				continue
			}
			if m.Item(instance.DisplayName).Checked(instance.Name == s.activeInstance).Chosen() {
				s.openInstance(instance.Name)
			}
		}
		if machines := s.machineInstances(); len(machines) > 0 {
			m.Separator()
			for _, instance := range machines {
				instance := instance
				item := m.Item(instance.DisplayName + s.machineSuffix(instance)).
					Checked(instance.Name == s.activeInstance)
				if !instance.Machine.Enabled {
					item = item.Disabled(true)
				}
				if item.Chosen() {
					s.openInstance(instance.Name)
				}
			}
		}
		// Availability checks are fresh SSH round trips; the menu open is the
		// natural moment to refresh them (throttled, background).
		s.refreshMachineStatus(false)
		m.Separator()
		if m.Item("Add Remote Machine…").Chosen() {
			s.openTextDialog("add-machine", "", "Add Remote Machine", "SSH target (user@host)", "")
		}
		if active := s.activeInstanceInfo(); active != nil {
			if active.Machine != nil {
				// Remote instances are pinned to their machine profile, so
				// the per-instance actions map onto profile management.
				if m.Item("Rename Remote Machine…").Chosen() {
					s.openTextDialog("rename-machine", active.Machine.ID, "Rename Remote Machine", "Name", active.Machine.Label)
				}
				if m.Item("Forget Remote Machine…").Chosen() {
					s.openConfirm("forget-machine", active.Machine.ID,
						"Forget "+active.DisplayName+"?",
						"This removes the saved SSH machine from Herdr. The remote session and its panes keep running on that device; Shardlane disconnects from it.")
				}
			} else {
				if m.Item("Rename Session…").Chosen() {
					s.openTextDialog("rename-workspace", active.Name, "Rename Session", "Name", active.DisplayName)
				}
				if !active.Default {
					if m.Item("Delete Session…").Chosen() {
						s.openConfirm("delete-workspace", active.Name, "Delete Session?", "This stops and deletes the Herdr session. Runtime data owned by that session will no longer be available.")
					}
				}
			}
		}
		m.Separator()
		if m.Item("New Session…").Chosen() {
			s.openTextDialog("create-workspace", "", "New Session", "Name", "")
		}
	})
}
