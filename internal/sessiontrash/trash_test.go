package sessiontrash

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/history"
)

// fixture is one Trash over a fake home: real provider roots (Claude Code,
// Codex, Pi, OMP laid out by history.DefaultRoots) and a trash directory
// beside them, so every test runs the real file operations on temp dirs and
// never touches a user's session.
type fixture struct {
	t     *testing.T
	home  string
	store *Store
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	t.Setenv("CODEX_HOME", "")
	home := t.TempDir()
	return &fixture{
		t:     t,
		home:  home,
		store: New(filepath.Join(t.TempDir(), "trash"), history.DefaultRoots(home)),
	}
}

const claudeSession = "" +
	"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:00Z\",\"message\":{\"content\":\"Fix the parser\"}}\n" +
	"{\"type\":\"assistant\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:01Z\",\"message\":{\"id\":\"m1\",\"model\":\"claude-sonnet-4-5\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}\n"

// claude writes one Claude Code session an hour old (well outside the busy
// window) and returns the catalog meta the shell would hand to Trash.
func (f *fixture) claude(project, id string) history.SessionMeta {
	f.t.Helper()
	path := filepath.Join(f.home, ".claude", "projects", project, id+".jsonl")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		f.t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(claudeSession), 0o600); err != nil {
		f.t.Fatal(err)
	}
	old := time.Now().Add(-time.Hour)
	if err := os.Chtimes(path, old, old); err != nil {
		f.t.Fatal(err)
	}
	return history.SessionMeta{
		Key: string(history.AgentClaudeCode) + ":" + id, ID: id, Agent: history.AgentClaudeCode, FilePath: path,
	}
}

func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// TestTrashThenRestoreRoundTripsTheFile pins the whole reversible path: the
// session leaves its provider directory, shows up in the Trash with the
// title read back from the moved file, and Restore returns the same bytes
// (and modification time) to the very same place.
func TestTrashThenRestoreRoundTripsTheFile(t *testing.T) {
	f := newFixture(t)
	meta := f.claude("-work-demo", "abc-123")
	before, err := os.Stat(meta.FilePath)
	if err != nil {
		t.Fatal(err)
	}

	item, err := f.store.Trash(meta)
	if err != nil {
		t.Fatalf("Trash: %v", err)
	}
	if exists(meta.FilePath) {
		t.Fatal("the provider file is still in place after Trash")
	}
	if !exists(item.File) || item.Original != meta.FilePath || item.Agent != history.AgentClaudeCode {
		t.Fatalf("item = %+v", item)
	}

	items, err := f.store.List()
	if err != nil {
		t.Fatal(err)
	}
	if len(items) != 1 || items[0].ID != item.ID {
		t.Fatalf("List = %+v", items)
	}
	if got := items[0].Summary.Meta; got.Title != "Fix the parser" || got.ProjectPath != "/work/demo" || got.ID != "abc-123" {
		t.Fatalf("summary read back from the trashed file = %+v", got)
	}
	if got := f.store.Count(); got != 1 {
		t.Fatalf("Count = %d", got)
	}

	restored, err := f.store.Restore(item.ID)
	if err != nil {
		t.Fatalf("Restore: %v", err)
	}
	if restored.Original != meta.FilePath {
		t.Fatalf("restored to %q, want %q", restored.Original, meta.FilePath)
	}
	if !bytes.Equal(mustRead(t, meta.FilePath), []byte(claudeSession)) {
		t.Fatal("restored bytes differ from the original")
	}
	after, err := os.Stat(meta.FilePath)
	if err != nil {
		t.Fatal(err)
	}
	if !after.ModTime().Equal(before.ModTime()) {
		t.Fatalf("mtime %v became %v: the session would sort as just-updated", before.ModTime(), after.ModTime())
	}
	if f.store.Count() != 0 || exists(item.File) {
		t.Fatal("Restore must empty the entry it consumed")
	}
}

// TestTrashRefusesRecentlyWrittenSession pins the live-session guard: a file
// written within the busy window may still be growing under a running Agent,
// so it stays where it is and the Trash stays empty.
func TestTrashRefusesRecentlyWrittenSession(t *testing.T) {
	f := newFixture(t)
	meta := f.claude("-work-demo", "live-1")
	now := time.Now()
	if err := os.Chtimes(meta.FilePath, now, now); err != nil {
		t.Fatal(err)
	}

	_, err := f.store.Trash(meta)
	if !errors.Is(err, ErrBusy) {
		t.Fatalf("err = %v, want ErrBusy", err)
	}
	if !exists(meta.FilePath) {
		t.Fatal("a refused session must not move")
	}
	if f.store.Count() != 0 {
		t.Fatal("a refused session left a Trash entry behind")
	}
}

// TestTrashOnlyTouchesProviderSessionFiles pins the path guard: only a
// regular .jsonl file inside one of the provider's own roots may enter the
// Trash — not a file elsewhere, not another provider's root, not a
// traversal, not a symlink out of the root, not a directory.
func TestTrashOnlyTouchesProviderSessionFiles(t *testing.T) {
	f := newFixture(t)
	// A day on: the symlink created below would otherwise be refused as
	// "just written" before the file-type guard is ever reached.
	f.store.now = func() time.Time { return time.Now().Add(24 * time.Hour) }
	good := f.claude("-work-demo", "ok-1")

	outside := filepath.Join(t.TempDir(), "elsewhere.jsonl")
	if err := os.WriteFile(outside, []byte(claudeSession), 0o600); err != nil {
		t.Fatal(err)
	}
	old := time.Now().Add(-time.Hour)
	_ = os.Chtimes(outside, old, old)

	notJSONL := filepath.Join(filepath.Dir(good.FilePath), "notes.txt")
	if err := os.WriteFile(notJSONL, []byte("hi"), 0o600); err != nil {
		t.Fatal(err)
	}
	_ = os.Chtimes(notJSONL, old, old)

	link := filepath.Join(filepath.Dir(good.FilePath), "link.jsonl")
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}

	// Built by hand: filepath.Join would clean the dot-dot segments away
	// before the guard ever saw them.
	sep := string(os.PathSeparator)
	// A project folder that is really a symlink out of the root: the file's
	// own entry is a plain file, only its resolved parent gives it away.
	leak := t.TempDir()
	if err := os.WriteFile(filepath.Join(leak, "leak.jsonl"), []byte(claudeSession), 0o600); err != nil {
		t.Fatal(err)
	}
	_ = os.Chtimes(filepath.Join(leak, "leak.jsonl"), old, old)
	linkedDir := filepath.Join(f.home, ".claude", "projects", "linked")
	if err := os.Symlink(leak, linkedDir); err != nil {
		t.Fatal(err)
	}

	traversal := filepath.Dir(good.FilePath) + sep + ".." + sep + ".." + sep + ".." + sep + "outside-" + filepath.Base(outside)
	if err := os.WriteFile(filepath.Join(f.home, "outside-"+filepath.Base(outside)), []byte(claudeSession), 0o600); err != nil {
		t.Fatal(err)
	}

	cases := map[string]history.SessionMeta{
		"file outside every root":     {Agent: history.AgentClaudeCode, FilePath: outside},
		"wrong provider root":         {Agent: history.AgentCodex, FilePath: good.FilePath},
		"not a jsonl file":            {Agent: history.AgentClaudeCode, FilePath: notJSONL},
		"symlink":                     {Agent: history.AgentClaudeCode, FilePath: link},
		"symlinked project folder":    {Agent: history.AgentClaudeCode, FilePath: filepath.Join(linkedDir, "leak.jsonl")},
		"dot-dot traversal":           {Agent: history.AgentClaudeCode, FilePath: traversal},
		"the provider root itself":    {Agent: history.AgentClaudeCode, FilePath: filepath.Join(f.home, ".claude", "projects")},
		"a project directory":         {Agent: history.AgentClaudeCode, FilePath: filepath.Dir(good.FilePath)},
		"empty path":                  {Agent: history.AgentClaudeCode},
		"unknown provider":            {Agent: history.AgentID("nope"), FilePath: good.FilePath},
		"relative path":               {Agent: history.AgentClaudeCode, FilePath: "projects/x.jsonl"},
		"root-relative escape string": {Agent: history.AgentClaudeCode, FilePath: "../x.jsonl"},
	}
	for name, meta := range cases {
		if _, err := f.store.Trash(meta); err == nil {
			t.Errorf("%s: Trash succeeded", name)
		}
	}
	if !exists(outside) || !exists(notJSONL) || !exists(link) || !exists(good.FilePath) || !exists(filepath.Join(leak, "leak.jsonl")) {
		t.Fatal("a refused request moved or removed something")
	}
	if f.store.Count() != 0 {
		t.Fatalf("refused requests left %d Trash entries", f.store.Count())
	}
	if _, err := f.store.Trash(good); err != nil {
		t.Fatalf("the legitimate session was refused: %v", err)
	}
}

// TestRestoreNeverOverwrites pins that Put Back refuses when something
// already sits at the original location, and then leaves the trashed copy
// exactly where it was.
func TestRestoreNeverOverwrites(t *testing.T) {
	f := newFixture(t)
	meta := f.claude("-work-demo", "dup-1")
	item, err := f.store.Trash(meta)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(meta.FilePath, []byte("a newer file"), 0o600); err != nil {
		t.Fatal(err)
	}

	if _, err := f.store.Restore(item.ID); !errors.Is(err, ErrExists) {
		t.Fatalf("err = %v, want ErrExists", err)
	}
	if string(mustRead(t, meta.FilePath)) != "a newer file" {
		t.Fatal("Restore overwrote the file at the original location")
	}
	if !exists(item.File) || f.store.Count() != 1 {
		t.Fatal("a refused Restore must keep the trashed copy")
	}
}

// TestRestoreRecreatesAMissingProjectDirectory pins that a project directory
// removed in the meantime does not strand the session in the Trash.
func TestRestoreRecreatesAMissingProjectDirectory(t *testing.T) {
	f := newFixture(t)
	meta := f.claude("-work-gone", "gone-1")
	item, err := f.store.Trash(meta)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.RemoveAll(filepath.Dir(meta.FilePath)); err != nil {
		t.Fatal(err)
	}
	if _, err := f.store.Restore(item.ID); err != nil {
		t.Fatalf("Restore: %v", err)
	}
	if !bytes.Equal(mustRead(t, meta.FilePath), []byte(claudeSession)) {
		t.Fatal("restored bytes differ")
	}
}

// TestRestoreRefusesATamperedOriginal pins that the manifest is not trusted
// blindly: an original path that no longer lies inside a provider root is
// refused, and the trashed copy stays.
func TestRestoreRefusesATamperedOriginal(t *testing.T) {
	f := newFixture(t)
	item, err := f.store.Trash(f.claude("-work-demo", "t-1"))
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(t.TempDir(), "planted.jsonl")
	entry := filepath.Join(f.store.dir, item.ID)
	manifest := mustRead(t, filepath.Join(entry, manifestName))
	tampered := strings.Replace(string(manifest), strings.ReplaceAll(item.Original, `\`, `\\`), target, 1)
	if tampered == string(manifest) {
		t.Fatal("the test could not rewrite the manifest")
	}
	if err := os.WriteFile(filepath.Join(entry, manifestName), []byte(tampered), 0o600); err != nil {
		t.Fatal(err)
	}

	if _, err := f.store.Restore(item.ID); !errors.Is(err, ErrNotSession) {
		t.Fatalf("err = %v, want ErrNotSession", err)
	}
	if exists(target) || !exists(item.File) {
		t.Fatal("a tampered manifest steered the file out of the Trash")
	}
}

// TestRestoreNeverCreatesFoldersOutsideTheRoots pins the order of Put Back's
// checks: a manifest path outside the provider roots is refused before any
// folder is created for it, not after.
func TestRestoreNeverCreatesFoldersOutsideTheRoots(t *testing.T) {
	f := newFixture(t)
	item, err := f.store.Trash(f.claude("-work-demo", "t-2"))
	if err != nil {
		t.Fatal(err)
	}
	planted := filepath.Join(t.TempDir(), "new-folder", "planted.jsonl")
	entry := filepath.Join(f.store.dir, item.ID)
	manifest := mustRead(t, filepath.Join(entry, manifestName))
	tampered := strings.Replace(string(manifest), strings.ReplaceAll(item.Original, `\`, `\\`), planted, 1)
	if err := os.WriteFile(filepath.Join(entry, manifestName), []byte(tampered), 0o600); err != nil {
		t.Fatal(err)
	}

	if _, err := f.store.Restore(item.ID); !errors.Is(err, ErrNotSession) {
		t.Fatalf("err = %v, want ErrNotSession", err)
	}
	if exists(filepath.Dir(planted)) {
		t.Fatal("Restore created a folder outside the provider roots")
	}
}

// TestTrashFailureLeavesNothingBehind pins the rollback: when the move
// itself cannot finish (the project folder is read-only, so the source
// cannot be unlinked), the session stays exactly where it was and no entry
// is left in the Trash.
func TestTrashFailureLeavesNothingBehind(t *testing.T) {
	f := newFixture(t)
	meta := f.claude("-work-locked", "l-1")
	dir := filepath.Dir(meta.FilePath)
	if err := os.Chmod(dir, 0o500); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = os.Chmod(dir, 0o700) }()

	if _, err := f.store.Trash(meta); err == nil {
		t.Skip("the folder is writable regardless of its mode (running as root?)")
	}
	_ = os.Chmod(dir, 0o700)
	if !bytes.Equal(mustRead(t, meta.FilePath), []byte(claudeSession)) {
		t.Fatal("a failed Trash changed the source")
	}
	if f.store.Count() != 0 {
		t.Fatalf("a failed Trash left %d entries", f.store.Count())
	}
	entries, _ := os.ReadDir(f.store.dir)
	if len(entries) != 0 {
		t.Fatalf("a failed Trash left %d files in the Trash folder", len(entries))
	}
}

// TestDeleteAndEmptyOnlyRemoveTrashEntries pins permanent deletion: Delete
// removes exactly one entry, Empty removes every entry, ids that are not
// entry ids (traversals, absolute paths, blanks) are refused, and neither
// call ever reaches outside the Trash directory or removes foreign files
// someone left inside it.
func TestDeleteAndEmptyOnlyRemoveTrashEntries(t *testing.T) {
	f := newFixture(t)
	first, err := f.store.Trash(f.claude("-work-demo", "d-1"))
	if err != nil {
		t.Fatal(err)
	}
	second, err := f.store.Trash(f.claude("-work-demo", "d-2"))
	if err != nil {
		t.Fatal(err)
	}

	sentinel := filepath.Join(filepath.Dir(f.store.dir), "sentinel.txt")
	if err := os.WriteFile(sentinel, []byte("keep"), 0o600); err != nil {
		t.Fatal(err)
	}
	stray := filepath.Join(f.store.dir, "notes.txt")
	if err := os.WriteFile(stray, []byte("keep"), 0o600); err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{"", ".", "..", "../sentinel.txt", "/etc", filepath.Dir(f.store.dir), first.ID + "/..", "notes.txt", "12-zz"} {
		if err := f.store.Delete(id); !errors.Is(err, ErrNotFound) {
			t.Errorf("Delete(%q) = %v, want ErrNotFound", id, err)
		}
		if _, err := f.store.Restore(id); !errors.Is(err, ErrNotFound) {
			t.Errorf("Restore(%q) = %v, want ErrNotFound", id, err)
		}
	}
	if !exists(sentinel) || !exists(stray) || f.store.Count() != 2 {
		t.Fatal("a refused id removed something")
	}

	if err := f.store.Delete(first.ID); err != nil {
		t.Fatal(err)
	}
	if exists(first.File) || !exists(second.File) {
		t.Fatal("Delete must remove exactly the named entry")
	}
	if err := f.store.Delete(first.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("second Delete = %v, want ErrNotFound", err)
	}

	removed, err := f.store.Empty()
	if err != nil || removed != 1 {
		t.Fatalf("Empty = %d, %v; want 1", removed, err)
	}
	if f.store.Count() != 0 || !exists(sentinel) || !exists(stray) {
		t.Fatal("Empty must clear every entry and nothing else")
	}
}

// TestListIsNewestFirstAndHealsOrphans pins the listing contract: newest
// deletion first, an entry whose payload is gone (a crash between manifest
// and move) is cleaned up instead of listed, and a payload that cannot be
// parsed still lists — it can be put back or deleted like any other.
func TestListIsNewestFirstAndHealsOrphans(t *testing.T) {
	f := newFixture(t)
	clock := time.Now()
	f.store.now = func() time.Time { clock = clock.Add(time.Hour); return clock }

	older, err := f.store.Trash(f.claude("-work-demo", "o-1"))
	if err != nil {
		t.Fatal(err)
	}
	// A payload the parser cannot even open (no read permission) — the
	// parsers are lenient with malformed lines, so this is the real failure.
	broken := f.claude("-work-demo", "b-1")
	if err := os.Chmod(broken.FilePath, 0); err != nil {
		t.Fatal(err)
	}
	newer, err := f.store.Trash(broken)
	if err != nil {
		t.Fatal(err)
	}

	orphan := filepath.Join(f.store.dir, "1760000000000-deadbeef")
	if err := os.MkdirAll(orphan, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(orphan, manifestName), []byte(`{"version":1,"agent":"claude-code","original":"/x/y.jsonl","deleted_at_ms":1}`), 0o600); err != nil {
		t.Fatal(err)
	}

	items, err := f.store.List()
	if err != nil {
		t.Fatal(err)
	}
	if len(items) != 2 || items[0].ID != newer.ID || items[1].ID != older.ID {
		t.Fatalf("order = %+v", items)
	}
	if items[0].Summary.Meta.Key != "" {
		t.Fatalf("an unreadable payload must list without a summary, got %+v", items[0].Summary.Meta)
	}
	if exists(orphan) {
		t.Fatal("the payload-less entry was not cleaned")
	}
	if _, err := f.store.Restore(newer.ID); err != nil {
		t.Fatalf("an unreadable session must still restore: %v", err)
	}
}

// TestTrashEntriesAreCrashSafeAndPrivate pins the on-disk shape: each entry
// is its own directory holding a manifest and the file under its original
// base name, created owner-only, and the manifest carries nothing about the
// conversation — only where the file goes back to, which provider it
// belongs to and when it was deleted.
func TestTrashEntriesAreCrashSafeAndPrivate(t *testing.T) {
	f := newFixture(t)
	item, err := f.store.Trash(f.claude("-work-demo", "shape-1"))
	if err != nil {
		t.Fatal(err)
	}
	entry := filepath.Join(f.store.dir, item.ID)
	info, err := os.Stat(entry)
	if err != nil || info.Mode().Perm() != 0o700 {
		t.Fatalf("entry dir = %v, %v; want 0700", info, err)
	}
	names := map[string]bool{}
	children, err := os.ReadDir(entry)
	if err != nil {
		t.Fatal(err)
	}
	for _, child := range children {
		names[child.Name()] = true
	}
	if len(names) != 2 || !names[manifestName] || !names["shape-1.jsonl"] {
		t.Fatalf("entry holds %v", names)
	}
	manifest := string(mustRead(t, filepath.Join(entry, manifestName)))
	for _, leaked := range []string{"Fix the parser", "Done.", "/work/demo"} {
		if strings.Contains(manifest, leaked) {
			t.Fatalf("manifest persists conversation content %q: %s", leaked, manifest)
		}
	}
}

// TestTrashSerializesConcurrentCallers runs the mutating calls against each
// other under -race: every distinct session ends up in the Trash once.
func TestTrashSerializesConcurrentCallers(t *testing.T) {
	f := newFixture(t)
	const n = 12
	metas := make([]history.SessionMeta, n)
	for i := range metas {
		metas[i] = f.claude("-work-demo", "c-"+string(rune('a'+i)))
	}
	var wg sync.WaitGroup
	for _, meta := range metas {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if _, err := f.store.Trash(meta); err != nil {
				t.Errorf("Trash: %v", err)
			}
			_, _ = f.store.List()
		}()
	}
	wg.Wait()
	if got := f.store.Count(); got != n {
		t.Fatalf("Count = %d, want %d", got, n)
	}
}

// TestCopyThenRemoveIsTheCrossVolumeFallback pins the fallback used when
// rename crosses volumes (a CODEX_HOME on another disk): the bytes and the
// modification time arrive intact and only then is the source removed.
func TestCopyThenRemoveIsTheCrossVolumeFallback(t *testing.T) {
	dir := t.TempDir()
	from, to := filepath.Join(dir, "from.jsonl"), filepath.Join(dir, "to.jsonl")
	if err := os.WriteFile(from, []byte(claudeSession), 0o600); err != nil {
		t.Fatal(err)
	}
	old := time.Now().Add(-48 * time.Hour).Truncate(time.Second)
	if err := os.Chtimes(from, old, old); err != nil {
		t.Fatal(err)
	}
	if err := copyThenRemove(from, to); err != nil {
		t.Fatal(err)
	}
	if exists(from) || !bytes.Equal(mustRead(t, to), []byte(claudeSession)) {
		t.Fatal("copyThenRemove did not move the file")
	}
	if info, _ := os.Stat(to); !info.ModTime().Equal(old) {
		t.Fatalf("mtime = %v, want %v", info.ModTime(), old)
	}
	if err := copyThenRemove(filepath.Join(dir, "missing.jsonl"), filepath.Join(dir, "x.jsonl")); err == nil || exists(filepath.Join(dir, "x.jsonl")) {
		t.Fatal("a failed copy must leave no destination behind")
	}

	// It never overwrites either: an existing destination is refused and
	// both files stay as they were.
	src, taken := filepath.Join(dir, "src.jsonl"), filepath.Join(dir, "taken.jsonl")
	for path, body := range map[string]string{src: "mine", taken: "theirs"} {
		if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := copyThenRemove(src, taken); !errors.Is(err, ErrExists) {
		t.Fatalf("err = %v, want ErrExists", err)
	}
	if string(mustRead(t, taken)) != "theirs" || string(mustRead(t, src)) != "mine" {
		t.Fatal("a refused copy touched a file")
	}
}
