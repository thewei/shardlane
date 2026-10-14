package nativeui

/**
 * [INPUT]: 依赖只读 HistoryService 的列表状态、多选/行内预览状态和共享 MyGo Design Tokens
 * [OUTPUT]: 提供 Sessions 列表（Agent 标签/搜索/Select/Trash 工具条（刷新在侧栏项目头）、项目分组卡片与行、行菜单）、Provider 过滤、扫描中的 Updating 提示、Markdown 导出与删除入口（回收站见 history_trash.go）
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

func (s *Shell) historyPage(c *ui.Context) {
	s.syncHistoryScope()
	if !s.hist.loaded && !s.hist.loading && s.hist.errText == "" {
		s.requestHistoryList()
	}
	if s.hist.trash != nil && !s.hist.trashRequested {
		s.requestTrashList()
	}
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		contentColumn(c, listColumnWidth, func() {
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
// the Agent tabs lead, the search field, Select and the Trash toggle (with
// Empty Trash inside the Trash) trail as one cluster; Refresh is the
// sidebar header's. A control with nothing to act on stays out (one Agent
// needs no tabs, an empty Trash needs no toggle, an empty list needs no
// Select). A narrow window wraps the cluster onto a second line as a whole
// instead of squeezing the field to nothing.
func (s *Shell) historyToolbar(c *ui.Context) {
	sp := Spacing()
	ui.Row(c).FillWidth().Padding(pageTopInset(), sp.XL, sp.M).Gap(sp.M).AlignItems(ui.Center).Wrap().Children(func() {
		s.historyAgentTabs(c)
		ui.Spacer(c)
		ui.Row(c).Shrink(0).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			field := ui.SearchField(c, &s.hist.query).Label("Search sessions").Width(200)
			if field.Changed() || field.Submitted() {
				s.applyHistoryFilters()
			}
			if s.hist.trash != nil && !s.hist.showTrash && (len(s.hist.sessions) > 0 || s.hist.selecting) {
				s.historySelectToggle(c)
			}
			if s.trashAvailable() {
				s.historyTrashToggle(c)
			}
			if s.hist.showTrash && len(s.trashScoped()) > 0 {
				if ui.Button(c, "Empty Trash").Clicked() {
					s.confirmEmptyTrash()
				}
			}
		})
	})
}

// historyAgentTabs is the Agent switch: "All" plus one tab per Provider in
// the catalog, each carrying its session total. A catalog with a single
// Provider needs no switch — every row already names its Agent.
func (s *Shell) historyAgentTabs(c *ui.Context) {
	providers := s.historyProviders()
	if len(providers) < 2 {
		return
	}
	t := c.Theme()
	chosen := 0
	for i, id := range providers {
		if string(id) == s.hist.provider {
			chosen = i + 1
		}
	}
	tabs := ui.SegmentedBase(c, &chosen, len(providers)+1)
	segmentedTrack(c, tabs, "Agent")
	tabs.Track.Children(func() {
		for i := 0; i <= len(providers); i++ {
			name, count := "All", 0
			var provider history.AgentID
			if i == 0 {
				for _, id := range providers {
					count += s.historyTabCount(id)
				}
			} else {
				provider = providers[i-1]
				name, count = provider.DisplayName(), s.historyTabCount(provider)
			}
			tab := segmentFace(c, tabs, i, i == chosen, name)
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
		if chosen > 0 && chosen <= len(providers) {
			s.hist.provider = string(providers[chosen-1])
		}
		s.applyHistoryFilters()
	}
}

// historyTabCount is a Provider's tab total, matching what the list below
// shows: the scope's total, or — in the Trash — what the Trash holds of it
// within the scope.
func (s *Shell) historyTabCount(provider history.AgentID) int {
	if !s.hist.showTrash {
		return s.hist.counts[provider]
	}
	count := 0
	for _, item := range s.trashScoped() {
		if item.Agent == provider {
			count++
		}
	}
	return count
}

// historySelectToggle enters or leaves the multi-select mode.
func (s *Shell) historySelectToggle(c *ui.Context) {
	t := c.Theme()
	toggle := ui.ButtonBase(c).Height(Controls().RegularHeight).Padding(0, Spacing().L).Gap(Spacing().S).
		Radius(Radius().Control).AlignItems(ui.Center).Label("Select sessions").Tooltip("Select sessions")
	gorexToggleFace(toggle, gorexColorsOf(t.Dark), s.hist.selecting)
	toggle.Children(func() {
		ui.Icon(c, iconTask).Size(13, 13).TextColor(t.TextMuted).PassThrough()
		ui.Text(c, "Select").FontSize(Typography().Body).PassThrough()
	})
	if toggle.Clicked() {
		s.setHistorySelecting(!s.hist.selecting)
	}
}

func (s *Shell) historyListBody(c *ui.Context) {
	switch {
	case s.hist.service == nil:
		emptyState(c, "Sessions unavailable", "")
	case s.hist.showTrash:
		s.trashListBody(c)
	case (s.hist.loading || s.hist.scanning) && len(s.hist.sessions) == 0:
		loadingState(c, "Loading sessions…")
	case s.hist.errText != "" && len(s.hist.sessions) == 0:
		emptyState(c, "Couldn't load sessions", "Try Refresh.")
	case s.hist.loaded && len(s.hist.sessions) == 0 && historyFiltersActive(s.hist.query, s.hist.provider):
		s.noMatchingSessions(c)
	case s.hist.loaded && len(s.hist.sessions) == 0 && s.hist.scopeRoot != "" && s.hist.allTotal > 0:
		s.noProjectSessions(c, "No sessions in this project")
	case s.hist.loaded && len(s.hist.sessions) == 0:
		emptyState(c, "No sessions yet", "")
	default:
		s.historyGroupsBody(c)
	}
}

// trashListBody is the Trash view's body: loading, empty, filtered to
// nothing, or the same project-grouped list the sessions use.
func (s *Shell) trashListBody(c *ui.Context) {
	switch rows := s.trashRows(); {
	case !s.hist.trashLoaded:
		loadingState(c, "Loading Trash…")
	case len(rows) == 0 && historyFiltersActive(s.hist.query, s.hist.provider):
		s.noMatchingSessions(c)
	case len(rows) == 0 && s.historyScopeRoot() != "" && len(s.hist.trashItems) > 0:
		s.noProjectSessions(c, "Nothing from this project in the Trash")
	case len(rows) == 0:
		emptyState(c, "Trash is empty", "Deleted sessions wait here until you delete them for good.")
	default:
		s.historyGroupsBody(c)
	}
}

// noMatchingSessions is the empty state of a search or Agent tab that matches
// nothing, with the way out.
func (s *Shell) noMatchingSessions(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).Center().Gap(Spacing().M).Children(func() {
		ui.Text(c, "No matching sessions").FontWeight(WeightSemibold)
		ui.Text(c, "Try another search or agent.").FontSize(Typography().BodySmall).TextColor(t.TextMuted)
		if ui.Button(c, "Clear Filters").Clicked() {
			s.clearHistoryFilters()
		}
	})
}

// historyGroupsBody is the project-grouped session list with its count line
// (and the selection bar under it), shared by the sessions and the Trash.
func (s *Shell) historyGroupsBody(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	groups := s.groupedSessionView()
	shown := 0
	for _, group := range groups {
		shown += len(group.sessions)
	}
	count := fmt.Sprintf("%d session%s", shown, pluralS(shown))
	switch {
	case s.hist.showTrash:
		count = fmt.Sprintf("%d in Trash · Nothing is deleted automatically", shown)
	case len(s.hist.sessions) >= HistoryListLimit:
		// The query is bounded: say so rather than imply a catalog total.
		count = fmt.Sprintf("Latest %d session%s", shown, pluralS(shown))
	}
	searching := strings.TrimSpace(s.hist.query) != ""
	s.navList(c, &s.nav.sessions, true, ui.Scroll(c).Grow(1), func() {
		contentColumn(c, listColumnWidth, func() {
			ui.Column(c).FillWidth().Padding(0, sp.XL, sp.XL).Gap(sp.L).Children(func() {
				ui.Text(c, count).FontSize(Typography().Caption).TextColor(t.TextMuted).Padding(0, 0, 0, sp.XS)
				for i, group := range groups {
					// The latest project opens by default; a search opens
					// every project it matched in, and so does the Trash —
					// what is there is what Put Back can reach.
					s.sessionGroup(c, group, i == 0 || searching || s.hist.showTrash)
				}
				if !s.hist.showTrash && (s.hist.loading || s.hist.scanning) {
					// Rows are already visible while the catalog is still
					// being read (batched scan); say so instead of leaving
					// a partial page looking complete.
					ui.Text(c, "Updating…").FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
			})
		})
	})
	if s.selectionBarShown() {
		s.historySelectionBar(c, groups)
	}
}

// sessionGroupView is one project's slice of the filtered session list.
type sessionGroupView struct {
	path     string
	name     string
	sessions []history.SessionSummary
}

// sessionsInView is the list the page shows: the catalog's sessions, or the
// Trash's entries.
func (s *Shell) sessionsInView() []history.SessionSummary {
	if s.hist.showTrash {
		return s.trashRows()
	}
	return s.hist.sessions
}

// groupedSessionView groups the filtered sessions by project path — the
// Sessions view is project-grouped, only (N19 #9) — the group order
// following each project's most recent session.
func (s *Shell) groupedSessionView() []sessionGroupView {
	groups := map[string]*sessionGroupView{}
	order := []string{}
	for _, summary := range s.sessionsInView() {
		if summary.Meta.Archived {
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
		groupKey := "group:" + group.path
		header := ui.ButtonBase(c.Key(groupKey)).FillWidth().Height(36).Padding(0, rowInset).Gap(sp.S).AlignItems(ui.Center).
			FocusRing(false).Label("Project " + group.name)
		switch {
		case s.nav.sessions.showsCursor(header):
			header.Background(tokens.SidebarSelect)
		case header.Hovered():
			header.Background(tokens.PanelHover)
		}
		activated := s.nav.sessions.row(c, header, navRow{Key: groupKey, Section: true, Folder: true, Expanded: open})
		header.Children(func() {
			treeChevron(c, true, open, t.TextMuted)
			if s.historySelecting() {
				s.groupSelectionBox(c, group)
			}
			ui.Text(c, group.name).FontSize(Typography().Body).FontWeight(WeightSemibold).SingleLine().Shrink(0).PassThrough()
			ui.Text(c, compactSearchPath(group.path)).Font(gdCodeFont()).FontSize(Typography().Micro).TextColor(t.TextMuted).
				SingleLine().Ellipsis("…").Grow(1).MinWidth(0).Tooltip(group.path)
			ui.Text(c, fmt.Sprintf("%d session%s", len(group.sessions), pluralS(len(group.sessions)))).FontSize(Typography().Caption).TextColor(t.TextMuted).Shrink(0).PassThrough()
		})
		if header.Clicked() || activated {
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
			if !s.historySelecting() && s.hist.preview.key == summary.Meta.Key {
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
	if s.hist.showTrash {
		menu = func(m *ui.Menu) { s.trashMenu(m, summary.Meta) }
	}
	selecting := s.historySelecting()
	selected := selecting && s.hist.selected[key]
	row := ui.ButtonBase(c.Key("session:"+key)).FillWidth().Padding(sp.M, rowInset).Gap(sp.L).AlignItems(ui.Center).
		FocusRing(false).Label(title)
	switch {
	case selected, s.nav.sessions.showsCursor(row):
		row.Background(tokens.SidebarSelect)
	case row.Hovered(), !selecting && s.hist.preview.key == key:
		row.Background(tokens.PanelHover)
	}
	// The inline preview is the row's fold: → opens it, ← closes it, and
	// while selecting (or in the Trash) a row is a plain leaf that ticks.
	activated := s.nav.sessions.row(c, row, navRow{Key: key, Folder: !selecting, Expanded: !selecting && s.hist.preview.key == key})
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
			// The button plus the margin fills the date's footprint, so the
			// hover swap does not shift the row.
			more := gorexIconButton(c, gorexColorsOf(t.Dark), iconChevronDown, "Actions for "+title, "", false, gorexRowBtn, gorexRowGlyph).
				Margin(0, 0, 0, historyDateWidth-gorexRowBtn).Shrink(0)
			more.Menu(menu)
		} else {
			ui.Text(c, historyShortDate(summary.Meta.UpdatedAt, time.Now())).FontSize(Typography().Caption).
				TextColor(t.TextMuted).Width(historyDateWidth).TextAlign(ui.End).SingleLine().Shrink(0).PassThrough()
		}
	})
	row.ContextMenu(menu)
	if row.Clicked() || activated {
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
	if meta.FilePath != "" && m.Item("Reveal Source File in Finder").Chosen() {
		mygo.Shell.ShowItemInFolder(meta.FilePath)
	}
	if m.Item("Export as Markdown…").Chosen() {
		s.exportSessionMarkdown(meta)
	}
	if s.hist.trash != nil {
		m.Separator()
		if m.Item("Move to Trash").Chosen() {
			s.moveSessionsToTrash([]history.SessionMeta{meta})
		}
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

// copySessionResume copies the resume command for one session.
func (s *Shell) copySessionResume(meta history.SessionMeta) {
	s.copyToClipboard(sessionResumeCommand(meta))
}

// historyFiltersActive indicates a scoped view, not an empty catalog.
func historyFiltersActive(query, provider string) bool {
	return strings.TrimSpace(query) != "" || provider != ""
}
