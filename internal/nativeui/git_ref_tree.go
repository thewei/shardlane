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
	if count > 0 {
		return
	}
	label := "None"
	if strings.TrimSpace(s.gitRefsFilter) != "" {
		label = "No matches"
	}
	ui.Text(c, label).Padding(2, 0, 4, treeGlyphX(0)).FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
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

// gitRefKinds names a ref tree's rows for assistive technology and tests.
var gitRefKinds = map[string]string{"local": "Branch", "remote": "Remote branch", "tag": "Tag"}

// gitRenderRefTree draws one slash-grouped ref tree from level (0 for the
// rows directly under a section).
func (s *Shell) gitRenderRefTree(c *ui.Context, scope string, roots []*gitRefNode, level int, click func(string), menu func(string, *ui.Menu)) {
	if len(roots) == 0 {
		s.gitEmptySection(c, 0)
		return
	}
	leafGlyph := iconBranch
	if scope == "tag" {
		leafGlyph = iconTag
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
		item := gitTreeItem{title: n.name, tooltip: value, kind: gitRefKinds[scope], glyph: leafGlyph,
			level: level, folder: folder, closed: closed, current: current}
		if folder {
			item.kind, item.glyph = "Folder", iconFolder
		}
		s.gitTreeRow(c, item, func() {
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
		visit(root, level)
	}
}

// gitTreeItem is one row of a Git sidebar tree.
type gitTreeItem struct {
	title, tooltip, kind    string
	glyph                   *ui.SVG
	level                   int
	folder, closed, current bool
}

// gitTreeRow draws one row on the shared sidebar grid. A stash subject can
// repeat; its title carries the unique stash reference.
func (s *Shell) gitTreeRow(c *ui.Context, it gitTreeItem, action func(), menu func(*ui.Menu)) {
	var trailing func()
	if it.current {
		trailing = func() {
			ui.Icon(c, iconCheck).FontSize(11).TextColor(c.Theme().Accent).Shrink(0)
		}
	}
	row := s.sidebarTreeRow(c, sidebarTreeSpec{
		Key: "git-ref:" + it.title + "@" + it.tooltip, Title: it.title, Label: it.kind + " " + it.title,
		Tooltip: it.tooltip, Level: it.level, Folder: it.folder, Expanded: !it.closed,
		Glyph: it.glyph, Current: it.current, Trailing: trailing, Menu: menu,
	})
	if row.Clicked() {
		action()
	}
}
