package sshconfig

/**
 * [INPUT]: 一个配置文件的行切片与其中一个 host 块的位置及现状
 * [OUTPUT]: EditHostBlock —— 单块定向改写：重命名 Host 头、按需 set/remove 指令；块内其余行（注释、未知指令）与块外所有行逐字保留
 * [POS]: 结构化表单编辑的写路径：单块作用域 + 陈旧围栏（块头与索引不符即拒绝），绝不序列化整个文件——raw 编辑器仍是全量入口
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"errors"
	"strings"
)

// ErrStaleBlock reports that the block being edited no longer matches the
// indexed state — the file changed since the page loaded it. Callers must
// refresh their index instead of retrying.
var ErrStaleBlock = errors.New("host block changed since it was loaded")

// EditHostBlock rewrites the host block starting at blockLine (1-based) in
// lines. The block header must still read `Host expectedHeader` — the stale
// fence. The header pattern is renamed to header when it differs, each set
// directive is replaced in place (first occurrence, indentation preserved)
// or appended at the block end, and every occurrence of a remove directive
// is deleted. Every other line — comments and unknown directives included —
// passes through byte for byte, and lines outside the block are never
// touched. ssh_config's first-value-wins rule keeps any later duplicate of
// a replaced key harmless.
func EditHostBlock(lines []string, blockLine int, expectedHeader, header string, sets []KV, removes []string) ([]string, error) {
	if blockLine < 1 || blockLine > len(lines) {
		return nil, ErrStaleBlock
	}
	if header == "" || strings.HasPrefix(header, "-") || strings.ContainsFunc(header, func(r rune) bool {
		return r == ' ' || r == '\t' || r == '"'
	}) {
		return nil, errors.New("host name must be one token without whitespace")
	}
	idx := blockLine - 1
	keyword, rest, comment := splitKeyword(lines[idx])
	if comment || !strings.EqualFold(keyword, "host") || rest != expectedHeader {
		return nil, ErrStaleBlock
	}
	end := blockEnd(lines, idx)
	body := lines[idx+1:end]
	// Appends land after the last content line, so a block's trailing
	// blank lines stay between blocks instead of stranding a directive.
	trailing := len(body)
	for trailing > 0 && strings.TrimSpace(body[trailing-1]) == "" {
		trailing--
	}
	block := applyBlockEdit(body[:trailing], sets, removes)
	block = append(block, body[trailing:]...)
	out := make([]string, 0, len(lines)-(end-idx-1)+len(block))
	out = append(out, lines[:idx+1]...)
	if !strings.EqualFold(header, expectedHeader) {
		out[idx] = "Host " + header
	}
	out = append(out, block...)
	out = append(out, lines[end:]...)
	return out, nil
}

// blockEnd returns the exclusive end of the block starting at start: the
// next Host/Match keyword line, or EOF. A Host keyword also terminates an
// open Match block (ssh_config(5)), so the scan is exact for both kinds.
func blockEnd(lines []string, start int) int {
	for i := start + 1; i < len(lines); i++ {
		keyword, _, comment := splitKeyword(lines[i])
		if comment {
			continue
		}
		switch strings.ToLower(keyword) {
		case "host", "match":
			return i
		}
	}
	return len(lines)
}

// applyBlockEdit filters removals, then replaces or appends each set within
// one block's body lines.
func applyBlockEdit(block []string, sets []KV, removes []string) []string {
	drop := make(map[string]bool, len(removes))
	for _, key := range removes {
		drop[strings.ToLower(strings.TrimSpace(key))] = true
	}
	kept := make([]string, 0, len(block)+len(sets))
	for _, line := range block {
		keyword, _, comment := splitKeyword(line)
		if comment || !drop[strings.ToLower(keyword)] {
			kept = append(kept, line)
		}
	}
	for _, set := range sets {
		name := strings.TrimSpace(set.Key)
		if name == "" || set.Value == "" {
			continue
		}
		replaced := false
		for i, line := range kept {
			keyword, _, comment := splitKeyword(line)
			if comment || !strings.EqualFold(keyword, name) {
				continue
			}
			kept[i] = indentOf(line) + name + " " + set.Value
			replaced = true
			break
		}
		if !replaced {
			kept = append(kept, "  "+name+" "+set.Value)
		}
	}
	return kept
}

// indentOf returns a line's leading whitespace so a replaced directive keeps
// the file's own indentation style.
func indentOf(line string) string {
	trimmed := strings.TrimLeft(line, " \t")
	return line[:len(line)-len(trimmed)]
}
