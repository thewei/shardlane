package sshconfig

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"testing"
)

func write(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestIndexHostsMatchsAndSyntaxVariants(t *testing.T) {
	dir := t.TempDir()
	config := filepath.Join(dir, "config")
	write(t, config, `# leading comment

Host web
    HostName web.example.com
    User deploy

HOST db backup-db
	HostName db.internal

host=legacy
  Port 2222

Match final exec "true"
  Compression yes

Host *
  ServerAliveInterval 30
`)
	ix, err := IndexPath(config)
	if err != nil {
		t.Fatal(err)
	}
	// The indexer records absolute paths; on macOS /var is a symlink to
	// /private/var, so compare against the resolved path.
	resolved, err := filepath.EvalSymlinks(config)
	if err != nil {
		t.Fatal(err)
	}
	if len(ix.Entries) != 5 {
		t.Fatalf("entries = %d, want 5: %+v", len(ix.Entries), ix.Entries)
	}
	web := ix.Entries[0]
	if web.Kind != KindHost || len(web.Patterns) != 1 || web.Patterns[0] != "web" {
		t.Fatalf("web entry = %+v", web)
	}
	if web.Alias() != "web" {
		t.Fatalf("web alias = %q", web.Alias())
	}
	facts := web.Facts()
	if len(facts) != 2 || facts[0].Key != "HostName" || facts[0].Value != "web.example.com" || facts[1].Key != "User" {
		t.Fatalf("web facts = %+v", facts)
	}
	if got := ix.Entries[1].Title(); got != "db backup-db" {
		t.Fatalf("multi-pattern title = %q", got)
	}
	if ix.Entries[1].Alias() != "" {
		t.Fatal("multi-pattern entry must not be connectable")
	}
	// Keyword is case-insensitive and accepts Keyword=value.
	if got := ix.Entries[2].Alias(); got != "legacy" {
		t.Fatalf("legacy alias = %q", got)
	}
	match := ix.Entries[3]
	if match.Kind != KindMatch || match.Match != `final exec "true"` {
		t.Fatalf("match entry = %+v", match)
	}
	if match.Alias() != "" {
		t.Fatal("match entry must not be connectable")
	}
	if len(match.Facts()) != 1 || match.Facts()[0].Key != "Compression" {
		t.Fatalf("match facts = %+v", match.Facts())
	}
	if got := ix.Entries[4].Alias(); got != "" {
		t.Fatalf("wildcard alias = %q", got)
	}
	for i, e := range ix.Entries {
		if e.File != resolved || e.Line == 0 || len(e.Lines) == 0 {
			t.Fatalf("entry %d missing provenance: %+v", i, e)
		}
	}
}

func TestIndexQuotedAndWildcardAliases(t *testing.T) {
	dir := t.TempDir()
	config := filepath.Join(dir, "config")
	write(t, config, "Host \"my host\" web*\n  HostName example.com\n\nHost a!b\n")
	ix, err := IndexPath(config)
	if err != nil {
		t.Fatal(err)
	}
	if got := ix.Entries[0].Patterns; len(got) != 2 || got[0] != "my host" || got[1] != "web*" {
		t.Fatalf("patterns = %+v", got)
	}
	if got := ix.Entries[0].Alias(); got != "" {
		t.Fatalf("quoted-space alias should still be literal, got %q", got)
	}
	// "my host" is one literal pattern: connectable via shell quoting.
	first := Entry{Kind: KindHost, Patterns: ix.Entries[0].Patterns[:1]}
	if first.Alias() != "my host" {
		t.Fatalf("literal alias = %q", first.Alias())
	}
	if got := ix.Entries[1].Alias(); got != "" {
		t.Fatalf("negation pattern alias = %q", got)
	}
}

func TestIndexIncludesGlobCycleAndOrder(t *testing.T) {
	home := t.TempDir()
	sshDir := filepath.Join(home, ".ssh")
	main := filepath.Join(sshDir, "config")
	write(t, main, "Include conf.d/*\nHost local\n  HostName 127.0.0.1\n")
	write(t, filepath.Join(sshDir, "conf.d", "a.conf"), "Include c.conf\nHost a\n")
	write(t, filepath.Join(sshDir, "conf.d", "b.conf"), "Host b\n")
	// c.conf includes a.conf back: the cycle guard must stop the walk.
	write(t, filepath.Join(sshDir, "c.conf"), "Include conf.d/a.conf\nHost c\n")
	// Relative Include arguments resolve against $HOME/.ssh.
	t.Setenv("HOME", home)

	ix, err := IndexPath(main)
	if err != nil {
		t.Fatal(err)
	}
	var titles []string
	for _, e := range ix.Entries {
		titles = append(titles, e.Title())
	}
	// a.conf's Include precedes its Host line, so c's entries expand
	// exactly at the Include's position — document order, like ssh.
	want := []string{"c", "a", "b", "local"}
	if len(titles) != len(want) {
		t.Fatalf("titles = %v, want %v", titles, want)
	}
	for i := range want {
		if titles[i] != want[i] {
			t.Fatalf("titles = %v, want %v (include order wrong at %d)", titles, want, i)
		}
	}
	if len(ix.Files) != 4 {
		t.Fatalf("files = %v", ix.Files)
	}
	resolvedMain, err := filepath.EvalSymlinks(main)
	if err != nil {
		t.Fatal(err)
	}
	if ix.MainPath != main || ix.Files[0] != resolvedMain {
		t.Fatalf("main file not first: %v", ix.Files)
	}
}

func TestIndexMissingMainFile(t *testing.T) {
	_, err := IndexPath(filepath.Join(t.TempDir(), "config"))
	if !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("err = %v, want fs.ErrNotExist", err)
	}
}

func TestIndexUnreadableIncludeDegradesToWarning(t *testing.T) {
	dir := t.TempDir()
	config := filepath.Join(dir, "config")
	write(t, config, "Include missing.conf\nHost local\n")
	ix, err := IndexPath(config)
	if err != nil {
		t.Fatal(err)
	}
	if len(ix.Entries) != 1 || ix.Entries[0].Alias() != "local" {
		t.Fatalf("entries = %+v", ix.Entries)
	}
	if len(ix.Warnings) == 0 {
		t.Fatal("expected a warning for the unreadable include")
	}
}

func TestSplitKeywordVariants(t *testing.T) {
	cases := []struct {
		line            string
		keyword, rest   string
		comment         bool
	}{
		{"", "", "", true},
		{"   # note", "", "", true},
		{"Host web", "Host", "web", false},
		{" HostName=ex.com ", "HostName", "ex.com", false},
		{"proxycommand ssh -W %h:%p j", "proxycommand", "ssh -W %h:%p j", false},
		{"identityfile ~/.ssh/id_ed25519 # not a comment", "identityfile", "~/.ssh/id_ed25519 # not a comment", false},
	}
	for _, tc := range cases {
		keyword, rest, comment := splitKeyword(tc.line)
		if keyword != tc.keyword || rest != tc.rest || comment != tc.comment {
			t.Fatalf("splitKeyword(%q) = (%q, %q, %v), want (%q, %q, %v)",
				tc.line, keyword, rest, comment, tc.keyword, tc.rest, tc.comment)
		}
	}
}
