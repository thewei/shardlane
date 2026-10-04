package nativeui

import (
	"github.com/egoist/mygo/ui"
)

// Breadcrumb switchers (2026-10-05 header review): every segment of the
// title-bar path is a button that opens a popover listing its siblings —
// Projects, the selected Project's Tabs, the selected Tab's Panes — for
// one-click switching, mirroring the Rust Shardlane behavior. Pane rows
// carry the bound Agent's provider glyph with the state dot at its corner.

type crumbSegment struct {
	label string
	kind  string // "project" | "tab" | "pane"
	id    string
}

// breadcrumbSegments resolves the title-bar path with identities.
func (s *Shell) breadcrumbSegments() []crumbSegment {
	var path []crumbSegment
	if p := s.selectedProject(); p != nil {
		path = append(path, crumbSegment{label: p.Label, kind: "project", id: p.ID})
	}
	if t := s.selectedTab(); t != nil {
		path = append(path, crumbSegment{label: t.Label, kind: "tab", id: t.ID})
	}
	if p := s.selectedPane(); p != nil && s.selectedTab() != nil && s.selectedTab().PaneCount > 1 {
		// The popover lists the Tab's Panes, so the segment carries the Tab id.
		path = append(path, crumbSegment{label: p.Label, kind: "pane", id: p.TabID})
	}
	return path
}

// breadcrumbRow renders the switcher path plus the ⋯ overflow button.
func (s *Shell) breadcrumbRow(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	segments := s.breadcrumbSegments()
	if len(segments) == 0 {
		ui.Text(c, "Herdr").TextColor(t.TextMuted).Grow(1).SingleLine()
		return
	}
	ui.Row(c).Height(20).Gap(2).AlignItems(ui.Center).Children(func() {
		for i, segment := range segments {
			segment := segment
			last := i == len(segments)-1
			var trigger *ui.Element
			ui.Box(c).Shrink(0).Children(func() {
				b := ui.ButtonBase(c).
					Shrink(0).
					Radius(Radius().Row).
					Padding(2, sp.S).
					Label(segment.label)
				// Clickable affordance is the hover tint and the popover;
				// only the current segment reads as selected. The tints are
				// the gorex header's (2026-10-06): a white pill for the
				// current segment, a quiet face on hover.
				k := gorexColorsOf(t.Dark)
				switch {
				case last:
					b.Background(k.tabActive)
				case b.Hovered():
					b.Background(k.hover)
				}
				b.Children(func() {
					ui.Row(c).Gap(4).AlignItems(ui.Center).Children(func() {
						ui.Text(c, segment.label).FontSize(Typography().Caption).
							FontWeight(600).SingleLine().Shrink(0)
						if !last {
							ui.Text(c, "›").FontSize(Typography().Micro).
								TextColor(t.TextMuted).Shrink(0)
						}
					})
				})
				trigger = b
			})
			if trigger.Clicked() {
				// One popover at a time.
				s.crumbOpen = [3]bool{}
				s.crumbOpen[i] = true
			}
			if s.crumbOpen[i] {
				s.crumbPopover(c, trigger, i, segment)
			}
		}
		s.headerOverflow(c)
	})
}

// crumbPopover lists the segment's siblings for quick switching.
func (s *Shell) crumbPopover(c *ui.Context, anchor *ui.Element, index int, segment crumbSegment) {
	ui.Popover(c, anchor, &s.crumbOpen[index], func() {
		sp := Spacing()
		ui.Column(c).Width(300).MaxHeight(420).Padding(sp.S).Gap(1).Children(func() {
			switch segment.kind {
			case "project":
				s.crumbProjectRows(c)
			case "tab":
				// The segment id is the selected Tab; its siblings list is
				// scoped by the selected Project.
				s.crumbTabRows(c, s.selectedProjectID)
			case "pane":
				s.crumbPaneRows(c, segment.id)
			}
		})
	})
}

func (s *Shell) crumbProjectRows(c *ui.Context) {
	for _, project := range s.projection.Projects {
		project := project
		row := crumbItemRow(c, visualMark{svg: iconFolder}, project.Label, opUnknown, project.ID == s.selectedProjectID)
		if row.Clicked() {
			s.crumbOpen = [3]bool{}
			s.setProjectExpanded(project.ID, true)
			s.selectProject(project.ID)
		}
	}
}

func (s *Shell) crumbTabRows(c *ui.Context, projectID string) {
	for _, tab := range tabsForProject(s.projection, projectID) {
		tab := tab
		row := crumbItemRow(c, visualMark{svg: iconTab}, tab.Label, opUnknown, tab.ID == s.selectedTabID)
		if row.Clicked() {
			s.crumbOpen = [3]bool{}
			s.selectTab(tab.ID)
		}
	}
}

func (s *Shell) crumbPaneRows(c *ui.Context, tabID string) {
	panes := panesForTab(s.projection, tabID)
	// The switcher is a jump surface: the same actionable-first order as
	// the sidebar tree, so the pane to reach is on top.
	s.sortPanesActionable(panes)
	for _, pane := range panes {
		pane := pane
		dark := c.Theme().Dark
		mark, isAgent := s.agentMarkForPane(pane.ID, dark)
		dot := opUnknown
		if isAgent {
			if a := s.paneAgent(pane.ID); a != nil {
				dot = normalizeRuntimeStatus(a.Status)
			}
		}
		row := crumbItemRow(c, mark, pane.Label, dot, pane.ID == s.selectedPaneID)
		if row.Clicked() {
			s.crumbOpen = [3]bool{}
			s.selectPane(pane.ID)
		}
	}
}

// crumbItemRow is one switcher row: glyph (with the agent state dot at
// its corner when bound), label, selected tint.
func crumbItemRow(c *ui.Context, mark visualMark, label string, dot operationalState, selected bool) *ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	var row *ui.Element
	ui.Box(c).Children(func() {
		row = ui.ButtonBase(c).
			FillWidth().
			Height(28).
			Radius(6).
			Padding(0, 8).
			Gap(7).
			Label(label)
		if tint, ok := rowTint(tokens, selected, row.Hovered()); ok {
			row.Background(tint)
		}
		row.Children(func() {
			if dot != opUnknown {
				ui.Box(c).Size(14, 14).Shrink(0).Children(func() {
					markView(c, mark, 14, t.TextMuted)
					ui.Box(c).Absolute().Left(9).Top(7).Children(func() {
						statusDot(c, dot)
					})
				})
			} else {
				ui.Box(c).Shrink(0).Children(func() { markView(c, mark, 14, t.TextMuted) })
			}
			ui.Text(c, label).FontSize(Typography().Caption).Grow(1).SingleLine()
		})
	})
	return row
}
