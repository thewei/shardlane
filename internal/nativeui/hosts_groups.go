package nativeui

/**
 * [INPUT]: 依赖 internal/sshconfig 的条目与只读分组声明（SSH Config Editor 式注释标记）
 * [OUTPUT]: 对外提供 hostGroup / hostGroups：Hosts 侧栏 SSH Config 列表的分组模型（声明组 → 其余主文件条目 → 各 Include 文件）
 * [POS]: Hosts 侧栏的呈现模型；分组只读自配置文件自己的注释，不建第二注册表、不写回文件
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import "github.com/wh-studio/herdr-client/internal/sshconfig"

// hostGroup is one section of the Hosts sidebar: a named run of config
// entries under a collapsible header.
type hostGroup struct {
	Key     string // stable fold key
	Label   string
	Tip     string // the full path of a file group
	Entries []sshconfig.Entry
}

// hostGroups sorts the index into sidebar sections. Order: the groups the
// config declares for itself (declaration order), then what the main file
// leaves ungrouped, then one group per Include file. It returns nil when
// there is nothing to group — no declared group with members and no included
// file — so a simple config stays one plain list.
func hostGroups(entries []sshconfig.Entry, declared []sshconfig.Group, mainPath string) []hostGroup {
	declaredAt := make(map[string]int, len(declared))
	groups := make([]hostGroup, 0, len(declared)+2)
	for i, g := range declared {
		declaredAt[g.ID] = i
		label := g.Name
		if label == "" {
			label = "Group"
		}
		groups = append(groups, hostGroup{Key: "group:" + g.ID, Label: label})
	}
	var rest, included []hostGroup
	includedAt := map[string]int{}
	var loose hostGroup
	for _, entry := range entries {
		if i, ok := declaredAt[entry.Group]; ok && entry.Group != "" {
			groups[i].Entries = append(groups[i].Entries, entry)
			continue
		}
		if mainPath != "" && entry.File != mainPath {
			i, seen := includedAt[entry.File]
			if !seen {
				i = len(included)
				includedAt[entry.File] = i
				included = append(included, hostGroup{
					Key: "file:" + entry.File, Label: compactSearchPath(entry.File), Tip: entry.File,
				})
			}
			included[i].Entries = append(included[i].Entries, entry)
			continue
		}
		loose.Entries = append(loose.Entries, entry)
	}

	kept := groups[:0]
	for _, g := range groups {
		if len(g.Entries) > 0 {
			kept = append(kept, g)
		}
	}
	if len(kept) == 0 && len(included) == 0 {
		return nil
	}
	if len(loose.Entries) > 0 {
		loose.Key, loose.Label = "ungrouped", "Ungrouped"
		if len(kept) == 0 {
			// Only files group this config: name the main one rather than
			// calling its hosts "ungrouped".
			loose.Key, loose.Label, loose.Tip = "file:"+mainPath, compactSearchPath(mainPath), mainPath
		}
		rest = append(rest, loose)
	}
	return append(append(kept, rest...), included...)
}
