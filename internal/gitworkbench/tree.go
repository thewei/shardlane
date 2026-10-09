package gitworkbench

import (
	"sort"
	"strings"
)

// TreeNode is one node of the changed-path tree: a directory aggregate or a
// changed file leaf (plan §11.1: tree projection is separate from parsing).
type TreeNode struct {
	Name     string // display segment
	Path     string // full repo-relative path
	Dir      bool
	File     *ChangeFile // leaf only
	Children []*TreeNode
}

// Additions/Deletions aggregate the subtree totals for directory rows.
func (n *TreeNode) Additions() int {
	if !n.Dir {
		if n.File != nil {
			return n.File.Additions
		}
		return 0
	}
	sum := 0
	for _, c := range n.Children {
		sum += c.Additions()
	}
	return sum
}

func (n *TreeNode) Deletions() int {
	if !n.Dir {
		if n.File != nil {
			return n.File.Deletions
		}
		return 0
	}
	sum := 0
	for _, c := range n.Children {
		sum += c.Deletions()
	}
	return sum
}

// FileCount counts leaves under the node.
func (n *TreeNode) FileCount() int {
	if !n.Dir {
		return 1
	}
	sum := 0
	for _, c := range n.Children {
		sum += c.FileCount()
	}
	return sum
}

// BuildTree projects changed files into a dirs-first tree. compactOneChild
// collapses single-child directory chains ("a/b/c.txt" under one row).
func BuildTree(files []ChangeFile, compactOneChild bool) *TreeNode {
	root := &TreeNode{Name: "", Path: "", Dir: true}
	for i := range files {
		file := &files[i]
		parts := strings.Split(filepathSlash(file.Path), "/")
		node := root
		last := len(parts) - 1
		for depth, part := range parts {
			if part == "" {
				continue
			}
			isLeaf := depth == last
			if isLeaf {
				leaf := &TreeNode{Name: part, Path: file.Path, File: file}
				node.Children = append(node.Children, leaf)
			} else {
				path := strings.Join(parts[:depth+1], "/")
				child := node.childDir(part, path)
				node = child
			}
		}
	}
	sortTree(root)
	if compactOneChild {
		compact(root)
	}
	return root
}

func filepathSlash(p string) string { return strings.ReplaceAll(p, "\\", "/") }

func (n *TreeNode) childDir(name, path string) *TreeNode {
	for _, c := range n.Children {
		if c.Dir && c.Name == name {
			return c
		}
	}
	child := &TreeNode{Name: name, Path: path, Dir: true}
	n.Children = append(n.Children, child)
	return child
}

// sortTree orders dirs first, then case-insensitive stable by name.
func sortTree(node *TreeNode) {
	sort.SliceStable(node.Children, func(i, j int) bool {
		a, b := node.Children[i], node.Children[j]
		if a.Dir != b.Dir {
			return a.Dir
		}
		return strings.ToLower(a.Name) < strings.ToLower(b.Name)
	})
	for _, c := range node.Children {
		sortTree(c)
	}
}

// compact merges single-child directory chains into one display node whose
// Name spans "a/b" and whose Path stays the deepest directory.
func compact(node *TreeNode) {
	for _, c := range node.Children {
		if c.Dir {
			compact(c)
		}
	}
	for i := 0; i < len(node.Children); i++ {
		child := node.Children[i]
		if !child.Dir || len(child.Children) != 1 || !child.Children[0].Dir {
			continue
		}
		grand := child.Children[0]
		grand.Name = child.Name + "/" + grand.Name
		node.Children[i] = grand
		i-- // re-examine; the new child may itself be single-dir
	}
}

// FilterTree recursively keeps nodes whose own text or a descendant matches
// the query (case-insensitive substring). Returns nil when nothing matches.
func FilterTree(node *TreeNode, query string) *TreeNode {
	if node == nil {
		return nil
	}
	query = strings.ToLower(strings.TrimSpace(query))
	if query == "" {
		return node
	}
	var filter func(n *TreeNode) *TreeNode
	filter = func(n *TreeNode) *TreeNode {
		selfMatch := strings.Contains(strings.ToLower(n.Name), query) ||
			strings.Contains(strings.ToLower(n.Path), query)
		if !n.Dir {
			if selfMatch {
				return n
			}
			return nil
		}
		kept := &TreeNode{Name: n.Name, Path: n.Path, Dir: true}
		for _, c := range n.Children {
			if fc := filter(c); fc != nil {
				kept.Children = append(kept.Children, fc)
			}
		}
		if len(kept.Children) == 0 {
			if selfMatch {
				// Directory name matches: keep its whole subtree visible.
				return n
			}
			return nil
		}
		return kept
	}
	return filter(node)
}

// FlatRow is one visible row of the expanded-tree projection.
type FlatRow struct {
	Node  *TreeNode
	Depth int
}

// Flatten projects the tree into visible rows given the collapsed-dir set.
// Directories default to expanded (changes trees are small); a key present
// with value true marks a user-collapsed directory.
func Flatten(node *TreeNode, collapsed map[string]bool) []FlatRow {
	// FilterTree returns nil for an unmatched search. Empty results are a
	// normal UI state, not a malformed tree or reason to panic.
	if node == nil {
		return nil
	}
	var rows []FlatRow
	var walk func(n *TreeNode, depth int)
	walk = func(n *TreeNode, depth int) {
		for _, c := range n.Children {
			rows = append(rows, FlatRow{Node: c, Depth: depth})
			if c.Dir && !collapsed[c.Path] {
				walk(c, depth+1)
			}
		}
	}
	walk(node, 0)
	return rows
}

// EnsureAncestorsOpen clears the collapsed marks on every directory above
// path so a freshly selected file's row is visible.
func EnsureAncestorsOpen(collapsed map[string]bool, path string) {
	parts := strings.Split(filepathSlash(path), "/")
	for i := 1; i < len(parts); i++ {
		delete(collapsed, strings.Join(parts[:i], "/"))
	}
}
