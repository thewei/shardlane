package nativeui

/**
 * [INPUT]: 依赖只读 HistoryService 的列表状态、多选/行内预览状态和共享 MyGo Design Tokens
 * [OUTPUT]: 提供 Sessions 列表（Agent 标签/搜索/Select/Hidden 工具条、项目分组卡片与行、行菜单）、Provider 过滤、扫描中的 Updating 提示、Markdown 导出与本地隐藏
 * [POS]: History 的列表页；会话详情是 history_detail.go（共享 Chat 视图），本页不再渲染转录
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"strings"
	"time"

	"context"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/filesview"
	"github.com/wh-studio/herdr-client/internal/history"
	"os"
)

// distinctAgents returns the agent kinds present in the summaries, in
// first-seen order — the dynamic History filter rail source (F37).
func distinctAgents(summaries []history.SessionSummary) []history.AgentID {
	seen := make(map[history.AgentID]bool, 4)
	out := make([]history.AgentID, 0, 4)
	for _, summary := range summaries {
		id := summary.Meta.Agent
		if id == "" || seen[id] {
			continue
		}
		seen[id] = true
		out = append(out, id)
	}
	return out
}

// historyNav is the Sessions sidebar body (N19 #9: History is now the
// Sessions view): the index entry — search, provider filters, the recycle
// toggle, refresh and the count live in the page toolbar, so the sidebar
// foot holds only Back, as on every inner page.
func (s *Shell) historyNav(c *ui.Context) {
	path := s.router.Path()
	ui.Text(c, "Sessions").FontSize(Typography().Section).FontWeight(WeightSemibold).Padding(6, 8, 8)
	if navTileButton(c, iconHistory, TileBlue, "All sessions", path == routeHistory).Clicked() {
		s.showHistoryIndex()
	}
}

func (s *Shell) historyPage(c *ui.Context) {
	if !s.hist.loaded && !s.hist.loading && s.hist.errText == "" {
		s.requestHistoryList()
	}
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		contentColumn(c, listColumnWidth, func() {
			pageHeader(c, "Sessions", "")
			if s.hist.errText != "" {
				inlineNotice(c, ToneError, s.hist.errText, func() {
					s.hist.loading = false
					s.hist.errText = ""
					s.requestHistoryList()
				})
			}
			s.historyToolbar(c)
		})
		s.historyListBody(c)
	})
}

// historyToolbar is the Sessions control strip, laid out after magpie's:
// the Agent tabs lead, the search field, Select, the Hidden toggle and
// Refresh trail as one cluster. A control with nothing to act on stays out
// (one Agent needs no tabs, no hidden session needs no Hidden, an empty
// list needs no Select). A narrow window wraps the cluster onto a second
// line as a whole instead of squeezing the field to nothing.
func (s *Shell) historyToolbar(c *ui.Context) {
	sp := Spacing()
	k := gorexColorsOf(c.Theme().Dark)
	ui.Row(c).FillWidth().Padding(0, sp.XL, sp.M).Gap(sp.M).AlignItems(ui.Center).Wrap().Children(func() {
		s.historyAgentTabs(c)
		ui.Spacer(c)
		ui.Row(c).Shrink(0).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			field := ui.SearchField(c, &s.hist.query).Label("Search sessions").Width(200)
			if field.Changed() || field.Submitted() {
				s.applyHistoryFilters()
			}
			if len(s.hist.sessions) > 0 || s.hist.selecting {
				s.historySelectToggle(c)
			}
			if len(s.settings.Session.HiddenSessions) > 0 || s.hist.showHidden {
				s.historyHiddenToggle(c)
			}
			if gorexIconButton(c, k, iconRefresh, "Refresh", "Refresh", false, gorexIconBtn, gorexIconGlyph).Clicked() {
				s.refreshHistoryScan()
			}
		})
	})
}

// historyAgentTabs is the Agent switch: "All" plus one tab per Provider in
// the catalog, each carrying its session total. A catalog with a single
// Provider needs no switch — every row already names its Agent.
func (s *Shell) historyAgentTabs(c *ui.Context) {
	if len(s.hist.providers) < 2 {
		return
	}
	t := c.Theme()
	chosen := 0
	for i, id := range s.hist.providers {
		if string(id) == s.hist.provider {
			chosen = i + 1
		}
	}
	tabs := ui.SegmentedBase(c, &chosen, len(s.hist.providers)+1)
	tabs.Track.Label("Agent").AlignItems(ui.Center).Shrink(0).Padding(t.Space(0.5)).Gap(t.Space(0.5)).
		Radius(t.Radius).Background(t.Surface).Border(1, t.Border)
	tabs.Track.Children(func() {
		for i := 0; i <= len(s.hist.providers); i++ {
			name, count := "All", 0
			var provider history.AgentID
			if i == 0 {
				for _, id := range s.hist.providers {
					count += s.historyTabCount(id)
				}
			} else {
				provider = s.hist.providers[i-1]
				name, count = provider.DisplayName(), s.historyTabCount(provider)
			}
			// Segment metrics follow ui.Segmented's own, so every segmented
			// control in the app shares one height; the inner radius is
			// concentric with the track's.
			tab := tabs.Segment(i).Padding(t.Space(1), t.Space(3)).Gap(Spacing().S).
				Radius(t.Radius - t.Space(0.5)).AlignItems(ui.Center).Label(name)
			switch {
			case i == chosen:
				tab.Background(t.SurfacePressed).Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, 0.12))
			case tab.Hovered():
				tab.Background(t.SurfaceHover)
			}
			tab.Children(func() {
				if provider != "" {
					markView(c, agentMark(provider, t.Dark), 14, t.TextMuted)
				}
				ui.Text(c, name).SingleLine()
				if count > 0 {
					ui.Text(c, itoa(count)).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine()
				}
			})
		}
	})
	if tabs.Track.Changed() {
		s.hist.provider = ""
		if chosen > 0 && chosen <= len(s.hist.providers) {
			s.hist.provider = string(s.hist.providers[chosen-1])
		}
		s.applyHistoryFilters()
	}
}

// historyTabCount is a Provider's tab total, matching what the list below
// shows: the catalog total less the sessions the user hid, or — in the
// Hidden view — the hidden ones. Hidden keys carry their Provider as prefix.
func (s *Shell) historyTabCount(provider history.AgentID) int {
	hidden := 0
	prefix := string(provider) + ":"
	for _, key := range s.settings.Session.HiddenSessions {
		if strings.HasPrefix(key, prefix) {
			hidden++
		}
	}
	if s.hist.showHidden {
		return hidden
	}
	return max(s.hist.counts[provider]-hidden, 0)
}

// historySelectToggle enters or leaves the multi-select mode.
func (s *Shell) historySelectToggle(c *ui.Context) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	toggle := ui.ButtonBase(c).Height(Controls().RegularHeight).Padding(0, Spacing().L).Gap(Spacing().S).
		Radius(Radius().Control).AlignItems(ui.Center).Label("Select sessions").Tooltip("Select sessions")
	if tint, ok := rowTint(tokens, s.hist.selecting, toggle.Hovered()); ok {
		toggle.Background(tint)
	}
	toggle.Children(func() {
		ui.Icon(c, iconTask).Size(13, 13).TextColor(t.TextMuted).PassThrough()
		ui.Text(c, "Select").FontSize(Typography().Body).PassThrough()
	})
	if toggle.Clicked() {
		s.setHistorySelecting(!s.hist.selecting)
	}
}

// historyHiddenToggle switches the list between the sessions and the ones
// the user hid, with the hidden total beside its label.
func (s *Shell) historyHiddenToggle(c *ui.Context) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	toggle := ui.ButtonBase(c).Height(Controls().RegularHeight).Padding(0, Spacing().L).Gap(Spacing().S).
		Radius(Radius().Control).AlignItems(ui.Center).Label("Hidden sessions").Tooltip("Hidden sessions")
	if tint, ok := rowTint(tokens, s.hist.showHidden, toggle.Hovered()); ok {
		toggle.Background(tint)
	}
	toggle.Children(func() {
		ui.Icon(c, iconEyeOff).Size(13, 13).TextColor(t.TextMuted).PassThrough()
		ui.Text(c, "Hidden").FontSize(Typography().Body).PassThrough()
		if hidden := len(s.settings.Session.HiddenSessions); hidden > 0 {
			ui.Text(c, itoa(hidden)).FontSize(Typography().Caption).TextColor(t.TextMuted).PassThrough()
		}
	})
	if toggle.Clicked() {
		s.hist.showHidden = !s.hist.showHidden
		s.closeHistoryPreview()
		s.clearHistorySelection()
	}
}

func (s *Shell) historyListBody(c *ui.Context) {
	t := c.Theme()
	switch {
	case s.hist.service == nil:
		emptyState(c, "Sessions unavailable", "")
	case (s.hist.loading || s.hist.scanning) && len(s.hist.sessions) == 0:
		loadingState(c, "Loading sessions…")
	case s.hist.errText != "" && len(s.hist.sessions) == 0:
		emptyState(c, "Couldn't load sessions", "Try Refresh.")
	case s.hist.loaded && len(s.hist.sessions) == 0 && historyFiltersActive(s.hist.query, s.hist.provider):
		ui.Column(c).Grow(1).Center().Gap(Spacing().M).Children(func() {
			ui.Text(c, "No matching sessions").FontWeight(WeightSemibold)
			ui.Text(c, "Try another search or agent.").FontSize(Typography().BodySmall).TextColor(t.TextMuted)
			if ui.Button(c, "Clear filters").Clicked() {
				s.clearHistoryFilters()
			}
		})
	case s.hist.loaded && len(s.hist.sessions) == 0:
		emptyState(c, "No sessions yet", "")
	default:
		sp := Spacing()
		groups := s.groupedSessionView()
		shown := 0
		for _, group := range groups {
			shown += len(group.sessions)
		}
		count := fmt.Sprintf("%d session%s", shown, pluralS(shown))
		switch {
		case s.hist.showHidden:
			count = fmt.Sprintf("%d hidden", shown)
		case len(s.hist.sessions) >= HistoryListLimit:
			// The query is bounded: say so rather than imply a catalog total.
			count = fmt.Sprintf("Latest %d session%s", shown, pluralS(shown))
		}
		searching := strings.TrimSpace(s.hist.query) != ""
		ui.Scroll(c).Grow(1).Children(func() {
			contentColumn(c, listColumnWidth, func() {
				ui.Column(c).FillWidth().Padding(0, sp.XL, sp.XL).Gap(sp.L).Children(func() {
					ui.Text(c, count).FontSize(Typography().Caption).TextColor(t.TextMuted).Padding(0, 0, 0, sp.XS)
					for i, group := range groups {
						// The latest project opens by default; a search opens
						// every project it matched in.
						s.sessionGroup(c, group, i == 0 || searching)
					}
					if s.hist.loading || s.hist.scanning {
						// Rows are already visible while the catalog is still
						// being read (batched scan); say so instead of leaving
						// a partial page looking complete.
						ui.Text(c, "Updating…").FontSize(Typography().Caption).TextColor(t.TextMuted)
					}
				})
			})
		})
		if s.hist.selecting {
			s.historySelectionBar(c, groups)
		}
	}
}

// sessionGroupView is one project's slice of the filtered session list.
type sessionGroupView struct {
	path     string
	name     string
	sessions []history.SessionSummary
}

// groupedSessionView groups the filtered sessions by project path — the
// Sessions view is project-grouped, only (N19 #9) — the group order
// following each project's most recent session.
func (s *Shell) groupedSessionView() []sessionGroupView {
	hidden := s.hiddenSessionSet()
	groups := map[string]*sessionGroupView{}
	order := []string{}
	for _, summary := range s.hist.sessions {
		if summary.Meta.Archived {
			continue
		}
		if s.hist.showHidden != hidden[summary.Meta.Key] {
			continue
		}
		path := summary.Meta.ProjectPath
		if path == "" {
			path = "No project"
		}
		group, seen := groups[path]
		if !seen {
			name := summary.Meta.ProjectName
			if name == "" {
				name = path
			}
			group = &sessionGroupView{path: path, name: name}
			groups[path] = group
			order = append(order, path)
		}
		group.sessions = append(group.sessions, summary)
	}
	view := make([]sessionGroupView, 0, len(order))
	for _, path := range order {
		view = append(view, *groups[path])
	}
	for i := 0; i < len(view); i++ {
		for j := i + 1; j < len(view); j++ {
			if sessionGroupLatest(view[j]) > sessionGroupLatest(view[i]) {
				view[i], view[j] = view[j], view[i]
			}
		}
	}
	return view
}

func sessionGroupLatest(group sessionGroupView) int64 {
	latest := int64(0)
	for _, summary := range group.sessions {
		if summary.Meta.UpdatedAt > latest {
			latest = summary.Meta.UpdatedAt
		}
	}
	return latest
}

// sessionGroup renders one collapsible project group as a single card: the
// header (chevron, name, folder, count) sits inside it over the session rows.
func (s *Shell) sessionGroup(c *ui.Context, group sessionGroupView, defaultOpen bool) {
	sp := Spacing()
	t := c.Theme()
	tokens := designTokens(t.Dark)
	open, chosen := s.hist.groupOpen[group.path]
	if !chosen {
		open = defaultOpen
	}
	collapsed := !open
	ui.Column(c).FillWidth().Radius(Radius().Card).Background(tokens.Panel).
		Border(1, tokens.BorderSubtle).Clip().Children(func() {
		header := ui.ButtonBase(c).FillWidth().Height(36).Padding(0, rowInset).Gap(sp.S).AlignItems(ui.Center).
			Label("Project " + group.name)
		if header.Hovered() {
			header.Background(tokens.PanelHover)
		}
		header.Children(func() {
			chevron := iconChevronDown
			if collapsed {
				chevron = iconChevron
			}
			ui.Icon(c, chevron).Size(11, 11).TextColor(t.TextMuted).Shrink(0).PassThrough()
			if s.hist.selecting {
				s.groupSelectionBox(c, group)
			}
			ui.Text(c, group.name).FontSize(Typography().Body).FontWeight(WeightSemibold).SingleLine().Shrink(0).PassThrough()
			ui.Text(c, compactSearchPath(group.path)).Font(gdCodeFont()).FontSize(Typography().Micro).TextColor(t.TextMuted).
				SingleLine().Ellipsis("…").Grow(1).MinWidth(0).Tooltip(group.path)
			ui.Text(c, fmt.Sprintf("%d session%s", len(group.sessions), pluralS(len(group.sessions)))).FontSize(Typography().Caption).TextColor(t.TextMuted).Shrink(0).PassThrough()
		})
		if header.Clicked() {
			if s.hist.groupOpen == nil {
				s.hist.groupOpen = map[string]bool{}
			}
			s.hist.groupOpen[group.path] = collapsed
		}
		if collapsed {
			return
		}
		for i, summary := range group.sessions {
			summary := summary
			if i == 0 {
				ui.Box(c).FillWidth().Height(1).Shrink(0).Background(tokens.BorderSubtle)
			} else {
				ui.Row(c).FillWidth().Height(1).Shrink(0).Children(func() {
					ui.Box(c).Width(rowInset + 24 + Spacing().L).Shrink(0)
					ui.Box(c).Grow(1).FillHeight().Background(tokens.BorderSubtle)
				})
			}
			s.sessionRow(c, summary)
			if !s.hist.selecting && s.hist.preview.key == summary.Meta.Key {
				s.sessionPreview(c, summary)
			}
		}
	})
}

// sessionRow renders one session as a quiet list row: agent mark, title over
// a muted fact line, the token total, and the date — which gives way to a
// pull-down chevron under the pointer. A click unfolds the inline preview (or, while
// selecting, ticks the row); everything else lives in the menu and the
// preview's actions.
func (s *Shell) sessionRow(c *ui.Context, summary history.SessionSummary) {
	sp := Spacing()
	t := c.Theme()
	tokens := designTokens(t.Dark)
	key := summary.Meta.Key
	title := fallbackText(summary.Meta.Title, history.Untitled)
	menu := func(m *ui.Menu) { s.sessionMenu(m, summary.Meta, true) }
	selecting := s.hist.selecting
	selected := selecting && s.hist.selected[key]
	row := ui.ButtonBase(c).FillWidth().Padding(sp.M, rowInset).Gap(sp.L).AlignItems(ui.Center).Label(title)
	switch {
	case selected:
		row.Background(tokens.SidebarSelect)
	case row.Hovered(), !selecting && s.hist.preview.key == key:
		row.Background(tokens.PanelHover)
	}
	row.Children(func() {
		if selecting {
			selectionMark(c, selected)
		}
		agentTile(c, summary.Meta.Agent, 24)
		ui.Column(c).Grow(1).MinWidth(0).Gap(2).PassThrough().Children(func() {
			ui.Text(c, title).FontSize(Typography().Body).FontWeight(WeightMedium).SingleLine().Ellipsis("…").Tooltip(title).PassThrough()
			if facts := sessionFactLine(summary.Meta, s.hist.provider == ""); facts != "" {
				ui.Text(c, facts).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine().Ellipsis("…").PassThrough()
			}
		})
		// The token total keeps its slot even when unknown, so the dates
		// below it stay on one vertical line.
		ui.Column(c).Width(historyMetricWidth).Shrink(0).AlignItems(ui.End).Gap(2).PassThrough().Children(func() {
			if used := summary.Meta.TokensUsed; used != nil && *used > 0 {
				ui.Text(c, agent.FormatTokenCount(*used)).FontSize(Typography().Body).FontWeight(WeightSemibold).SingleLine().PassThrough()
				ui.Text(c, "tokens").FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine().PassThrough()
			}
		})
		if row.Hovered() && !selecting {
			// 22 wide plus the margin fills the date's footprint, so the
			// hover swap does not shift the row.
			more := ui.ButtonBase(c).Size(22, 22).Margin(0, 0, 0, historyDateWidth-22).Radius(5).Shrink(0).Center().
				Label("Actions for " + title)
			if more.Hovered() {
				more.Background(tokens.SidebarHover)
			}
			more.Children(func() { ui.Icon(c, iconChevronDown).Size(12, 12).TextColor(t.TextMuted) })
			more.Menu(menu)
		} else {
			ui.Text(c, historyShortDate(summary.Meta.UpdatedAt, time.Now())).FontSize(Typography().Caption).
				TextColor(t.TextMuted).Width(historyDateWidth).TextAlign(ui.End).SingleLine().Shrink(0).PassThrough()
		}
	})
	row.ContextMenu(menu)
	if row.Clicked() {
		if selecting {
			s.setHistorySelected([]string{key}, !selected)
		} else {
			s.toggleHistoryPreview(key)
		}
	}
}

// Fixed slots of the right-hand row columns.
const (
	historyMetricWidth float32 = 56
	// historyDateWidth fits the longest list date ("Mar 4, 2025").
	historyDateWidth float32 = 72
)

// sessionMenu is the one action list of a session (the row's pull-down and
// right-click menu, and the detail header's, share it). Open only makes
// sense from the list.
func (s *Shell) sessionMenu(m *ui.Menu, meta history.SessionMeta, withOpen bool) {
	if withOpen {
		if m.Item("Open").Chosen() {
			s.router.Push("/history/" + meta.Key)
		}
		m.Separator()
	}
	s.continueMenuItems(m, meta)
	m.Separator()
	if m.Item("Copy Resume Command").Chosen() {
		s.copySessionResume(meta)
	}
	if m.Item("Copy Session ID").Chosen() {
		s.copyToClipboard(meta.ID)
	}
	if meta.FilePath != "" && m.Item("Show Source File in Finder").Chosen() {
		mygo.Shell.ShowItemInFolder(meta.FilePath)
	}
	if m.Item("Export as Markdown…").Chosen() {
		s.exportSessionMarkdown(meta)
	}
	m.Separator()
	label := "Hide"
	if s.hiddenSessionSet()[meta.Key] {
		label = "Restore"
	}
	if m.Item(label).Chosen() {
		s.toggleSessionHidden(meta.Key)
	}
}

// sessionFactLine is the muted line under a session title: the Agent (only
// when the list mixes Agents), model, message count and source size, when
// known. The token total has its own column.
func sessionFactLine(meta history.SessionMeta, withAgent bool) string {
	var parts []string
	if withAgent {
		parts = append(parts, meta.Agent.DisplayName())
	}
	if meta.Model != nil && *meta.Model != "" {
		parts = append(parts, *meta.Model)
	}
	parts = append(parts, fmt.Sprintf("%d message%s", meta.MessageCount, pluralS(int(meta.MessageCount))))
	if meta.SizeBytes > 0 {
		parts = append(parts, filesview.FormatFileSize(meta.SizeBytes))
	}
	return strings.Join(parts, " · ")
}

// historyShortDate is the list date: the time today, the month and day this
// year, the full date before that.
func historyShortDate(ms int64, now time.Time) string {
	if ms <= 0 {
		return ""
	}
	at := time.UnixMilli(ms).Local()
	now = now.Local()
	switch {
	case at.Year() == now.Year() && at.YearDay() == now.YearDay():
		return at.Format("15:04")
	case at.Year() == now.Year():
		return at.Format("Jan 2")
	default:
		return at.Format("Jan 2, 2006")
	}
}

// sessionResumeCommand builds the provider's own resume invocation prefixed
// with the project folder — the copyable "继续" command.
func sessionResumeCommand(meta history.SessionMeta) string {
	cd := ""
	if meta.ProjectPath != "" {
		cd = fmt.Sprintf("cd %q && ", meta.ProjectPath)
	}
	switch meta.Agent {
	case history.AgentClaudeCode:
		return cd + fmt.Sprintf("claude --resume %s", meta.ID)
	case history.AgentCodex:
		return cd + fmt.Sprintf("codex resume %s", meta.ID)
	case history.AgentPi:
		return cd + fmt.Sprintf("pi --resume %s", meta.ID)
	default:
		return cd + fmt.Sprintf("# no known resume command for %s (session %s)", meta.Agent, meta.ID)
	}
}

// exportSessionMarkdown assembles the full conversation as Markdown over
// paged catalog windows and saves it through the native save dialog (the
// reference view's 导出 Markdown). Provider files are read-only inputs.
func (s *Shell) exportSessionMarkdown(meta history.SessionMeta) {
	service := s.hist.service
	if service == nil {
		return
	}
	s.dispatch(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		var b strings.Builder
		fmt.Fprintf(&b, "# %s\n\n", fallbackText(meta.Title, history.Untitled))
		if meta.ProjectPath != "" {
			fmt.Fprintf(&b, "- Project: %s\n", meta.ProjectPath)
		}
		if meta.GitBranch != nil && *meta.GitBranch != "" {
			fmt.Fprintf(&b, "- Branch: %s\n", *meta.GitBranch)
		}
		if meta.Model != nil && *meta.Model != "" {
			fmt.Fprintf(&b, "- Model: %s\n", *meta.Model)
		}
		fmt.Fprintf(&b, "- Session: %s\n- Exported: %s\n\n---\n\n", meta.ID, time.Now().Format("2006-01-02 15:04"))

		for start := 0; ; start += history.WindowLimit {
			pageStart := start
			window, err := service.Open(ctx, history.ConversationWindowRequest{
				ConversationID: meta.Key,
				StartIndex:     &pageStart,
				Limit:          history.WindowLimit,
			})
			if err != nil {
				s.applyOnWindow(func() { s.status = "Export failed: " + err.Error() })
				return
			}
			for _, message := range window.Messages {
				writeSessionMessage(&b, message)
			}
			if len(window.Messages) == 0 || window.Start+len(window.Messages) >= window.TotalMessages {
				break
			}
		}

		markdown := b.String()
		s.applyOnWindow(func() {
			name := strings.Map(func(r rune) rune {
				if r == '/' || r == ':' || r == '\\' {
					return '-'
				}
				return r
			}, fallbackText(meta.Title, history.Untitled))
			path, err := mygo.Dialog.Save(mygo.SaveDialogOptions{
				Parent:      s.win,
				Title:       "Export session as Markdown",
				DefaultPath: name + ".md",
				ButtonLabel: "Export",
			})
			if err != nil || path == "" {
				return
			}
			if writeErr := os.WriteFile(path, []byte(markdown), 0o644); writeErr != nil {
				s.status = "Export failed: " + writeErr.Error()
				return
			}
			s.status = "Session exported to " + path
		})
	})
}

// writeSessionMessage renders one transcript message as Markdown.
func writeSessionMessage(b *strings.Builder, message history.TranscriptMessage) {
	switch message.Role {
	case history.RoleUser:
		fmt.Fprintf(b, "## User\n\n%s\n\n", message.Text)
	case history.RoleAssistant:
		fmt.Fprintf(b, "## Assistant\n\n")
		if message.Thinking != nil && *message.Thinking != "" {
			b.WriteString("> " + strings.ReplaceAll(*message.Thinking, "\n", "\n> ") + "\n\n")
		}
		if message.Text != "" {
			b.WriteString(message.Text + "\n\n")
		}
		for _, call := range message.ToolCalls {
			fmt.Fprintf(b, "**%s**", call.Name)
			if call.IsError {
				b.WriteString(" (error)")
			}
			b.WriteString("\n")
			if call.InputPreview != "" {
				fmt.Fprintf(b, "```\n%s\n```\n", call.InputPreview)
			}
			if call.Output != nil && *call.Output != "" {
				fmt.Fprintf(b, "```\n%s\n```\n", *call.Output)
			}
			b.WriteString("\n")
		}
	default:
		if message.Text != "" {
			fmt.Fprintf(b, "> %s\n\n", message.Text)
		}
	}
}

// hiddenSessionSet reads the local recycle list as a set.
func (s *Shell) hiddenSessionSet() map[string]bool {
	hidden := map[string]bool{}
	for _, key := range s.settings.Session.HiddenSessions {
		hidden[key] = true
	}
	return hidden
}

// toggleSessionHidden hides or restores one session locally — provider
// files are never touched (the history catalog is read-only).
func (s *Shell) toggleSessionHidden(key string) {
	s.setSessionsHidden([]string{key}, !s.hiddenSessionSet()[key])
}

// copySessionResume copies the resume command for one session.
func (s *Shell) copySessionResume(meta history.SessionMeta) {
	s.copyToClipboard(sessionResumeCommand(meta))
}

// historyFiltersActive indicates a scoped view, not an empty catalog.
func historyFiltersActive(query, provider string) bool {
	return strings.TrimSpace(query) != "" || provider != ""
}

