package nativeui

/**
 * [INPUT]: Git reference names, native list rows, local repo projection and selection state
 * [OUTPUT]: Slash-grouped, filterable, collapsible Git ref tree rendering helpers
 * [POS]: Git reference tree reuse only, no read-time Git IO or mutation authority
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"sort"
	"strings"

	"github.com/egoist/mygo/ui"
)

// gitRefNode is a disposable slash-delimited tree. The only persistent state
// is the collapsed-folder key, never a copy of the repository reference data.
type gitRefNode struct {
	name     string
	full     string
	children []*gitRefNode
	leaf     bool
}

func gitRefTree(names []string, filter string) []*gitRefNode {
	var roots []*gitRefNode
	q := strings.ToLower(strings.TrimSpace(filter))
	for _, full := range names {
		if q != "" && !strings.Contains(strings.ToLower(full), q) {
			continue
		}
		siblings := &roots
		prefix := ""
		for _, name := range strings.Split(full, "/") {
			if name == "" {
				continue
			}
			if prefix != "" {
				prefix += "/"
			}
			prefix += name
			var node *gitRefNode
			for _, candidate := range *siblings {
				if candidate.name == name {
					node = candidate
					break
				}
			}
			if node == nil {
				node = &gitRefNode{name: name, full: prefix}
				*siblings = append(*siblings, node)
			}
			siblings = &node.children
			if prefix == full {
				node.leaf = true
			}
		}
	}
	var sortTree func([]*gitRefNode)
	sortTree = func(nodes []*gitRefNode) {
		sort.Slice(nodes, func(i, j int) bool {
			ai, aj := len(nodes[i].children) > 0, len(nodes[j].children) > 0
			if ai != aj {
				return ai
			}
			return strings.ToLower(nodes[i].name) < strings.ToLower(nodes[j].name)
		})
		for _, node := range nodes {
			sortTree(node.children)
		}
	}
	sortTree(roots)
	return roots
}

func gitRefMatches(filter string, values ...string) bool {
	q := strings.ToLower(strings.TrimSpace(filter))
	if q == "" {
		return true
	}
	for _, value := range values {
		if strings.Contains(strings.ToLower(value), q) {
			return true
		}
	}
	return false
}

func (s *Shell) gitEmptySection(c *ui.Context, count int) {
	if count == 0 {
		ui.Text(c, "No matching items").Padding(4, 18).FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
	}
}

// A filtered tree starts fully expanded, yet a user's click must still fold
// it visibly. Query-scoped keys keep search-only folds separate from normal
// browsing and prevent old closed folders from hiding matching results.
func (s *Shell) gitFoldKey(normal string) string {
	if q := strings.ToLower(strings.TrimSpace(s.gitRefsFilter)); q != "" {
		return "search/" + q + "/" + normal
	}
	return normal
}

func (s *Shell) gitRenderRefTree(c *ui.Context, scope string, roots []*gitRefNode, depth int, click func(string), menu func(string, *ui.Menu)) {
	if len(roots) == 0 {
		s.gitEmptySection(c, 0)
		return
	}
	var visit func(*gitRefNode, int)
	visit = func(n *gitRefNode, level int) {
		folder := len(n.children) > 0
		key := s.gitFoldKey(scope + "/" + n.full)
		closed := s.gitTreeClosed[key]
		value := n.full
		// HEAD/current branch is identified by the snapshot, never inferred from name.
		current := false
		if scope == "local" {
			for _, b := range s.git.branches {
				if b.Name == value {
					current = b.Current
					break
				}
			}
		}
		s.gitTreeRow(c, n.name, value, level, folder, closed, current, func() {
			if folder {
				s.gitTreeClosed[key] = !s.gitTreeClosed[key]
			} else {
				click(value)
			}
		}, func(m *ui.Menu) {
			if n.leaf {
				menu(value, m)
			}
			if folder && m.Item("Collapse folder").Chosen() {
				s.gitTreeClosed[key] = true
			}
		})
		if folder && !closed {
			for _, child := range n.children {
				visit(child, level+1)
			}
		}
	}
	for _, root := range roots {
		visit(root, depth)
	}
}

func (s *Shell) gitTreeRow(c *ui.Context, title, tooltip string, depth int, folder, closed, current bool, action func(), menu func(*ui.Menu)) {
	t := c.Theme()
	inset := float32(5 + depth*12)
	// A stash subject can repeat; title includes the unique stash reference.
	row := ui.ButtonBase(c.Key("git-ref:"+title+"@"+tooltip)).Height(23).FillWidth().MinWidth(0).Padding(0, 6).
		Gap(5).Radius(4).FocusRing(false).Label(title).Tooltip(tooltip).AlignItems(ui.Center)
	if current {
		row.Background(t.Accent.Alpha(.11)).TextColor(t.Accent)
	} else if row.Hovered() {
		row.Background(t.SurfaceHover)
	}
	row.ContextMenu(menu)
	row.Children(func() {
		ui.Box(c).Width(inset).Shrink(0)
		if folder {
			chevron := iconChevronDown
			if closed {
				chevron = iconChevron
			}
			ui.Icon(c, chevron).FontSize(10).TextColor(t.TextMuted)
		} else {
			ui.Box(c).Width(11).Shrink(0)
		}
		svg := iconBranch
		if folder {
			svg = iconFolder
		}
		ui.Icon(c, svg).FontSize(12).TextColor(t.TextMuted)
		ui.Text(c, title).FontSize(Typography().BodySmall).SingleLine().Grow(1).MinWidth(0)
		if current {
			ui.Text(c, "✓").FontSize(Typography().BodySmall).TextColor(t.Accent)
		}
	})
	if row.Clicked() {
		action()
	}
}
