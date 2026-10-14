// Package sessiontrash is the Sessions Trash: the one place Shardlane moves a
// provider's session file, and only ever into — and back out of — a folder it
// owns. Deleting a session moves its .jsonl out of the provider's own history
// root into <trash>/<entry>/ beside a tiny manifest; Put Back moves it to the
// exact original path (never overwriting anything); Delete Forever and Empty
// remove entries from the Trash folder and nothing else.
//
// The manifest holds only where the file goes back to, which provider it
// belongs to and when it was deleted — never any conversation content. What
// the Trash list shows (title, project, counts) is read back from the moved
// file on demand through history.SummarizeSource, like every other history
// read, so no second copy of conversation-derived text exists on disk.
//
// A session is the one source file the history catalog indexes; provider
// sidecars are not tracked. The package sits beside internal/history (which
// stays read-only): it owns the file operations, history owns what a session
// is. It has no UI and no Herdr dependency.

package sessiontrash

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/internal/history"
)

// BusyWindow is how recently a session file may have been written and still
// be refused: a running Agent is most likely still appending to it, and
// moving the file under it would split the conversation across two places.
const BusyWindow = time.Minute

const (
	manifestName    = "entry.json"
	manifestVersion = 1
)

var (
	// ErrBusy refuses a session that was written to within BusyWindow.
	ErrBusy = errors.New("session was written to a moment ago")
	// ErrNotSession refuses any path that is not a regular .jsonl file inside
	// its provider's history root (or, for Put Back, no longer lies in it).
	ErrNotSession = errors.New("not a provider session file")
	// ErrExists refuses to put a session back over an existing file.
	ErrExists = errors.New("a file already exists at the original location")
	// ErrNotFound reports an id that names no Trash entry.
	ErrNotFound = errors.New("not in the Trash")
)

// Item is one trashed session.
type Item struct {
	// ID names the Trash entry; it is what Restore and Delete take.
	ID    string
	Agent history.AgentID
	// Original is where Restore puts the file back; File is where it sits
	// in the Trash now.
	Original  string
	File      string
	DeletedAt time.Time
	Size      int64
	// Summary is the list row read back from File by List (zero when the
	// file cannot be parsed — the entry can still be restored or deleted).
	// Trash itself returns it zero.
	Summary history.SessionSummary
}

// manifest is the whole persisted record of an entry.
type manifest struct {
	Version   int    `json:"version"`
	Agent     string `json:"agent"`
	Original  string `json:"original"`
	DeletedAt int64  `json:"deleted_at_ms"`
}

type cachedSummary struct {
	mtime   time.Time
	size    int64
	summary history.SessionSummary
}

// Store is the Trash over one folder. All methods are safe for concurrent
// use; mutations are serialized.
type Store struct {
	dir   string
	roots []history.SourceRoot
	now   func() time.Time

	mu        sync.Mutex
	summaries map[string]cachedSummary // trashed file path → its list row
}

// New returns a Trash kept in dir for sessions living under roots (the
// provider history roots — a session may only be trashed from, and restored
// to, a root of its own provider). The folder is created on first use.
func New(dir string, roots []history.SourceRoot) *Store {
	return &Store{dir: dir, roots: roots, now: time.Now, summaries: map[string]cachedSummary{}}
}

// Dir is the Trash folder.
func (s *Store) Dir() string { return s.dir }

// Trash moves one session's file into the Trash. It refuses anything that is
// not a regular .jsonl file inside one of the session's provider roots, and
// anything written to within BusyWindow; a refused session never moves and
// leaves no entry behind.
func (s *Store) Trash(meta history.SessionMeta) (Item, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	source := filepath.Clean(meta.FilePath)
	if _, err := s.locate(meta.Agent, source); err != nil {
		return Item{}, err
	}
	info, err := os.Lstat(source)
	if err != nil {
		return Item{}, err
	}
	if !info.Mode().IsRegular() {
		return Item{}, ErrNotSession
	}
	if s.now().Sub(info.ModTime()) < BusyWindow {
		return Item{}, ErrBusy
	}

	id, err := newID(s.now())
	if err != nil {
		return Item{}, err
	}
	entry := filepath.Join(s.dir, id)
	if err := os.MkdirAll(s.dir, 0o700); err != nil {
		return Item{}, err
	}
	if err := os.Mkdir(entry, 0o700); err != nil {
		return Item{}, err
	}
	// Manifest first: a crash between the two steps leaves a payload-less
	// entry List cleans up, never a payload nobody knows the origin of.
	deleted := s.now()
	if err := writeManifest(entry, manifest{
		Version: manifestVersion, Agent: string(meta.Agent), Original: source, DeletedAt: deleted.UnixMilli(),
	}); err != nil {
		_ = os.RemoveAll(entry)
		return Item{}, err
	}
	dest := filepath.Join(entry, filepath.Base(source))
	if err := moveFile(source, dest); err != nil {
		_ = os.RemoveAll(entry)
		return Item{}, err
	}
	return Item{
		ID: id, Agent: meta.Agent, Original: source, File: dest, DeletedAt: deleted, Size: info.Size(),
	}, nil
}

// Restore puts a trashed session back at its original path, recreating the
// project folder if it is gone, and removes the entry. It refuses to
// overwrite anything and re-validates the manifest's path against the
// provider roots (the manifest is not trusted blindly).
func (s *Store) Restore(id string) (Item, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	item, err := s.load(id)
	if err != nil {
		return Item{}, err
	}
	root, err := s.locate(item.Agent, item.Original)
	if err != nil {
		return Item{}, err
	}
	if _, err := os.Lstat(item.Original); err == nil {
		return Item{}, ErrExists
	}
	parent := filepath.Dir(item.Original)
	if err := os.MkdirAll(parent, 0o755); err != nil {
		return Item{}, err
	}
	if !realWithin(root.Directory, parent) {
		return Item{}, ErrNotSession
	}
	if err := moveFile(item.File, item.Original); err != nil {
		return Item{}, err
	}
	_ = os.RemoveAll(filepath.Join(s.dir, id))
	delete(s.summaries, item.File)
	return item, nil
}

// Delete removes one entry from the Trash for good.
func (s *Store) Delete(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	entry, err := s.entryDir(id)
	if err != nil {
		return err
	}
	s.forget(entry)
	return os.RemoveAll(entry)
}

// Empty removes every Trash entry for good and reports how many it removed.
// Anything else someone left in the folder stays.
func (s *Store) Empty() (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	ids, err := s.ids()
	if err != nil {
		return 0, err
	}
	removed := 0
	var failure error
	for _, id := range ids {
		entry := filepath.Join(s.dir, id)
		s.forget(entry)
		if err := os.RemoveAll(entry); err != nil {
			failure = errors.Join(failure, err)
			continue
		}
		removed++
	}
	return removed, failure
}

// Count is the number of entries, without reading any of them.
func (s *Store) Count() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	ids, _ := s.ids()
	return len(ids)
}

// List returns every entry, newest deletion first, each with its list row
// read back from the trashed file (cached per file stamp, so repeated calls
// do not reparse). An entry whose payload is gone is removed; one whose
// manifest cannot be read is left alone and not listed.
func (s *Store) List() ([]Item, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	ids, err := s.ids()
	if err != nil {
		return nil, err
	}
	items := make([]Item, 0, len(ids))
	live := make(map[string]bool, len(ids))
	for _, id := range ids {
		item, err := s.load(id)
		switch {
		case errors.Is(err, errOrphan):
			_ = os.RemoveAll(filepath.Join(s.dir, id))
			continue
		case err != nil:
			continue
		}
		item.Summary = s.summarize(item)
		live[item.File] = true
		items = append(items, item)
	}
	for file := range s.summaries {
		if !live[file] {
			delete(s.summaries, file)
		}
	}
	sort.Slice(items, func(i, j int) bool {
		if !items[i].DeletedAt.Equal(items[j].DeletedAt) {
			return items[i].DeletedAt.After(items[j].DeletedAt)
		}
		return items[i].ID > items[j].ID
	})
	return items, nil
}

// errOrphan marks an entry directory without a payload.
var errOrphan = errors.New("trash entry without a file")

// ids lists the entry directories (names that are valid entry ids) — foreign
// files someone left in the folder are never entries.
func (s *Store) ids() ([]string, error) {
	children, err := os.ReadDir(s.dir)
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	var ids []string
	for _, child := range children {
		if child.IsDir() && validID(child.Name()) {
			ids = append(ids, child.Name())
		}
	}
	return ids, nil
}

// entryDir resolves an id to its entry directory, refusing anything that is
// not a real, non-symlink directory named by a valid id.
func (s *Store) entryDir(id string) (string, error) {
	if !validID(id) {
		return "", ErrNotFound
	}
	entry := filepath.Join(s.dir, id)
	info, err := os.Lstat(entry)
	if err != nil || !info.IsDir() {
		return "", ErrNotFound
	}
	return entry, nil
}

// load reads one entry: the manifest and the payload file beside it.
func (s *Store) load(id string) (Item, error) {
	entry, err := s.entryDir(id)
	if err != nil {
		return Item{}, err
	}
	data, err := os.ReadFile(filepath.Join(entry, manifestName))
	if err != nil {
		return Item{}, err
	}
	var m manifest
	if err := json.Unmarshal(data, &m); err != nil {
		return Item{}, err
	}
	agent, ok := history.ParseAgentID(m.Agent)
	if m.Version != manifestVersion || !ok || m.Original == "" {
		return Item{}, fmt.Errorf("trash entry %s has an unusable manifest", id)
	}
	children, err := os.ReadDir(entry)
	if err != nil {
		return Item{}, err
	}
	for _, child := range children {
		if child.Name() == manifestName || !child.Type().IsRegular() {
			continue
		}
		info, err := child.Info()
		if err != nil {
			return Item{}, err
		}
		return Item{
			ID: id, Agent: agent, Original: m.Original, File: filepath.Join(entry, child.Name()),
			DeletedAt: time.UnixMilli(m.DeletedAt), Size: info.Size(),
		}, nil
	}
	return Item{}, errOrphan
}

// summarize reads the entry's list row from its file, reusing the cached row
// while the file stamp is unchanged. A file that cannot be parsed yields a
// zero summary (cached too, so a broken file is not reparsed on every list).
func (s *Store) summarize(item Item) history.SessionSummary {
	info, err := os.Stat(item.File)
	if err != nil {
		return history.SessionSummary{}
	}
	if cached, ok := s.summaries[item.File]; ok && cached.size == info.Size() && cached.mtime.Equal(info.ModTime()) {
		return cached.summary
	}
	summary := history.SessionSummary{}
	if nativeID, ok := s.nativeID(item); ok {
		if read, err := history.SummarizeSource(history.SessionFileRef{
			Agent: item.Agent, NativeID: nativeID, FilePath: item.File,
			MtimeMS: info.ModTime().UnixMilli(), SizeBytes: info.Size(),
		}); err == nil {
			summary = read
		}
	}
	s.summaries[item.File] = cachedSummary{mtime: info.ModTime(), size: info.Size(), summary: summary}
	return summary
}

// nativeID derives the provider's native session id from the trashed file's
// own name (entries keep the original base name), the way the scanner does.
func (s *Store) nativeID(item Item) (string, bool) {
	stem := strings.TrimSuffix(filepath.Base(item.File), ".jsonl")
	for _, root := range s.roots {
		if root.Agent == item.Agent && root.NativeID != nil {
			return root.NativeID(stem), true
		}
	}
	return "", false
}

func (s *Store) forget(entry string) {
	prefix := entry + string(os.PathSeparator)
	for file := range s.summaries {
		if strings.HasPrefix(file, prefix) {
			delete(s.summaries, file)
		}
	}
}

// locate finds the provider root of agent that holds path, requiring an
// absolute .jsonl path strictly inside it whose parent directory really
// resolves inside the root too (a symlinked folder is not a way out).
func (s *Store) locate(agent history.AgentID, path string) (history.SourceRoot, error) {
	if !filepath.IsAbs(path) || filepath.Ext(path) != ".jsonl" {
		return history.SourceRoot{}, ErrNotSession
	}
	for _, root := range s.roots {
		if root.Agent != agent || !within(root.Directory, path) {
			continue
		}
		if parent := filepath.Dir(path); exists(parent) && !realWithin(root.Directory, parent) {
			return history.SourceRoot{}, ErrNotSession
		}
		return root, nil
	}
	return history.SourceRoot{}, ErrNotSession
}

// within reports whether path lies strictly inside dir, lexically.
func within(dir, path string) bool {
	rel, err := filepath.Rel(filepath.Clean(dir), filepath.Clean(path))
	return err == nil && rel != "." && rel != ".." && !strings.HasPrefix(rel, ".."+string(os.PathSeparator))
}

// realWithin is within over symlink-resolved paths; an unresolvable path is
// not within anything.
func realWithin(dir, path string) bool {
	realDir, err := filepath.EvalSymlinks(dir)
	if err != nil {
		return false
	}
	realPath, err := filepath.EvalSymlinks(path)
	if err != nil {
		return false
	}
	rel, err := filepath.Rel(realDir, realPath)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(os.PathSeparator))
}

func exists(path string) bool {
	_, err := os.Lstat(path)
	return err == nil
}

// validID accepts exactly what newID makes: milliseconds, a dash and eight
// lowercase hex digits — so an id can never be a path.
func validID(id string) bool {
	millis, suffix, ok := strings.Cut(id, "-")
	if !ok || millis == "" || len(millis) > 20 || len(suffix) != 8 {
		return false
	}
	for _, r := range millis {
		if r < '0' || r > '9' {
			return false
		}
	}
	for _, r := range suffix {
		if (r < '0' || r > '9') && (r < 'a' || r > 'f') {
			return false
		}
	}
	return true
}

func newID(now time.Time) (string, error) {
	var raw [4]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return "", err
	}
	return fmt.Sprintf("%d-%s", now.UnixMilli(), hex.EncodeToString(raw[:])), nil
}

// writeManifest persists the entry record atomically.
func writeManifest(entry string, m manifest) error {
	data, err := json.Marshal(m)
	if err != nil {
		return err
	}
	tmp := filepath.Join(entry, manifestName+".tmp")
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, filepath.Join(entry, manifestName))
}

// moveFile moves a regular file to a path that must not exist yet: a hard
// link (atomic, refuses an existing target) followed by removing the source,
// so even a file appearing at the destination in the last instant is never
// overwritten. Where linking is impossible — another volume, or a
// filesystem without hard links — the file is copied instead.
func moveFile(from, to string) error {
	err := os.Link(from, to)
	switch {
	case err == nil:
		if rmErr := os.Remove(from); rmErr != nil {
			_ = os.Remove(to)
			return rmErr
		}
		return nil
	case errors.Is(err, fs.ErrExist):
		return ErrExists
	default:
		return copyThenRemove(from, to)
	}
}

// copyThenRemove copies from → to (exclusive create, bytes and modification
// time preserved, flushed to disk) and only then removes from. A failed copy
// leaves no destination behind and the source untouched.
func copyThenRemove(from, to string) (err error) {
	src, err := os.Open(from)
	if err != nil {
		return err
	}
	defer src.Close()
	info, err := src.Stat()
	if err != nil {
		return err
	}
	dst, err := os.OpenFile(to, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		if errors.Is(err, fs.ErrExist) {
			return ErrExists
		}
		return err
	}
	defer func() {
		if err != nil {
			_ = dst.Close()
			_ = os.Remove(to)
		}
	}()
	if _, err = io.Copy(dst, src); err != nil {
		return err
	}
	if err = dst.Sync(); err != nil {
		return err
	}
	if err = dst.Close(); err != nil {
		return err
	}
	if err = os.Chtimes(to, info.ModTime(), info.ModTime()); err != nil {
		return err
	}
	return os.Remove(from)
}
