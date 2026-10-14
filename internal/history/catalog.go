package history

/**
 * [INPUT]: 依赖 Scanner 观察到的 Provider 会话文件元数据（标题/计数/项目/描述行），不依赖任何持久化存储
 * [OUTPUT]: 对外提供纯内存 Catalog：Put/RemoveMissing/KnownFiles 写入面与 catalog_read.go 的有界查询面
 * [POS]: 会话列表索引层；只缓存列表行元数据，从不保存会话内容——正文永远由 HistoryService.Open 现读原始文件
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"strings"
	"sync"
)

// DescriptionMaxChars bounds the session description preview.
const DescriptionMaxChars = 240

// Catalog is Shardlane's in-memory index over read-only provider history. It
// holds one metadata row per session (title, counts, project, a bounded
// description line) and is rebuilt from the provider files by the Scanner —
// nothing is persisted and no conversation content is kept: transcripts are
// always read from the original files on demand.
type Catalog struct {
	mu      sync.RWMutex
	entries map[string]*catalogEntry // session key → row
	byPath  map[string]string        // provider file path → session key
}

type catalogEntry struct {
	meta        SessionMeta
	projectKey  string
	description string
}

// sourceStamp is the change signal of one provider file.
type sourceStamp struct {
	mtimeMS int64
	size    int64
}

// NewCatalog returns an empty catalog.
func NewCatalog() *Catalog {
	return &Catalog{entries: map[string]*catalogEntry{}, byPath: map[string]string{}}
}

// KnownFiles maps indexed provider file paths to the stamp they were indexed
// at, the scanner's change signal.
func (c *Catalog) KnownFiles() map[string]sourceStamp {
	c.mu.RLock()
	defer c.mu.RUnlock()
	known := make(map[string]sourceStamp, len(c.entries))
	for _, entry := range c.entries {
		known[entry.meta.FilePath] = sourceStamp{mtimeMS: entry.meta.MtimeMS, size: entry.meta.SizeBytes}
	}
	return known
}

// NormalizedProjectKey preserves spelling while normalizing components; it
// never touches the filesystem.
func NormalizedProjectKey(path string) string {
	trimmed := strings.TrimSpace(path)
	if trimmed == "" {
		return ""
	}
	return NormalizePathKey(trimmed)
}

// previewText is a text-kind message's text plus its tool-call summaries.
func previewText(message TranscriptMessage) string {
	parts := []string{message.Text}
	for _, call := range message.ToolCalls {
		parts = append(parts, call.Name+" "+call.InputPreview)
	}
	return strings.Join(parts, "\n")
}

// descriptionFromMessages builds the list-page preview: first user message,
// whitespace-collapsed, Unicode-safe truncation.
func descriptionFromMessages(messages []TranscriptMessage) string {
	candidate := ""
	for _, userOnly := range []bool{true, false} {
		for _, message := range messages {
			if message.Kind != MessageText || userOnly && message.Role != RoleUser {
				continue
			}
			if text := previewText(message); strings.TrimSpace(text) != "" {
				candidate = text
				break
			}
		}
		if candidate != "" {
			break
		}
	}
	var collapsed strings.Builder
	pendingSpace := false
	count := 0
	for _, ch := range candidate {
		if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' || ch == '\v' || ch == '\f' || ch == 0x85 || ch == 0xA0 {
			pendingSpace = collapsed.Len() > 0
			continue
		}
		if count >= DescriptionMaxChars {
			collapsed.WriteByte(0xE2)
			collapsed.WriteByte(0x80)
			collapsed.WriteByte(0xA6)
			break
		}
		if pendingSpace {
			collapsed.WriteByte(' ')
			pendingSpace = false
		}
		collapsed.WriteRune(ch)
		count++
	}
	return collapsed.String()
}

// Put inserts or replaces one session row. mtimeMS is the provider file's
// modification time at observation; a provider file backs at most one row.
func (c *Catalog) Put(meta SessionMeta, mtimeMS int64, description string) {
	meta.MtimeMS = mtimeMS
	c.mu.Lock()
	defer c.mu.Unlock()
	if previous, ok := c.entries[meta.Key]; ok {
		delete(c.byPath, previous.meta.FilePath)
	}
	if owner, ok := c.byPath[meta.FilePath]; ok && owner != meta.Key {
		delete(c.entries, owner)
	}
	c.entries[meta.Key] = &catalogEntry{
		meta:        meta,
		projectKey:  NormalizedProjectKey(meta.ProjectPath),
		description: description,
	}
	c.byPath[meta.FilePath] = meta.Key
}

// Forget drops the rows of the given session keys and frees their file
// paths, the immediate follow-up of a session leaving its provider root (it
// was moved to the Trash): the next scan would only notice at its cleanup
// step. Unknown keys are ignored; the number of rows dropped is returned.
func (c *Catalog) Forget(keys ...string) int {
	c.mu.Lock()
	defer c.mu.Unlock()
	removed := 0
	for _, key := range keys {
		entry, ok := c.entries[key]
		if !ok {
			continue
		}
		delete(c.entries, key)
		delete(c.byPath, entry.meta.FilePath)
		removed++
	}
	return removed
}

// RemoveMissing drops rows whose file paths were not observed by the last
// scan. Unobserved roots never reach here (cleanup only follows a complete
// observation).
func (c *Catalog) RemoveMissing(seenPaths map[string]bool) int {
	c.mu.Lock()
	defer c.mu.Unlock()
	removed := 0
	for key, entry := range c.entries {
		if seenPaths[entry.meta.FilePath] {
			continue
		}
		delete(c.entries, key)
		delete(c.byPath, entry.meta.FilePath)
		removed++
	}
	return removed
}
