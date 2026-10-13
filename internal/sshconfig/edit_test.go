package sshconfig

import (
	"errors"
	"strings"
	"testing"
)

func mustLines(t *testing.T, content string) []string {
	t.Helper()
	return strings.Split(content, "\n")
}

func TestEditHostBlockReplacesInPlaceAndKeepsTheRest(t *testing.T) {
	file := mustLines(t, `Global something yes

Host web
  # the fleet entry
  HostName old.example.com
  User deploy
  ServerAliveInterval 30

Host db
  HostName db.internal
`)
	out, err := EditHostBlock(file, 3, "web", "web", []KV{
		{Key: "HostName", Value: "new.example.com"},
	}, nil)
	if err != nil {
		t.Fatal(err)
	}
	want := mustLines(t, `Global something yes

Host web
  # the fleet entry
  HostName new.example.com
  User deploy
  ServerAliveInterval 30

Host db
  HostName db.internal
`)
	if strings.Join(out, "\n") != strings.Join(want, "\n") {
		t.Fatalf("out =\n%s", strings.Join(out, "\n"))
	}
}

func TestEditHostBlockAppendsRemovesAndRenames(t *testing.T) {
	file := mustLines(t, "Host web\n\tHostName old\n\nHost db\n")
	out, err := EditHostBlock(file, 1, "web", "web1", []KV{
		{Key: "User", Value: "deploy"},
	}, []string{"hostname"})
	if err != nil {
		t.Fatal(err)
	}
	// The rename rewrites the header in the file's own style; the removed
	// directive disappears; the set directive appends after the last
	// content line, before the block's trailing blank; the following block
	// is untouched.
	want := mustLines(t, "Host web1\n  User deploy\n\nHost db\n")
	if strings.Join(out, "\n") != strings.Join(want, "\n") {
		t.Fatalf("out =\n%s", strings.Join(out, "\n"))
	}
}

func TestEditHostBlockRemovesEveryOccurrence(t *testing.T) {
	file := mustLines(t, "Host web\n  IdentityFile ~/.ssh/a\n  User u\n  IdentityFile ~/.ssh/b\n")
	out, err := EditHostBlock(file, 1, "web", "web", nil, []string{"identityfile"})
	if err != nil {
		t.Fatal(err)
	}
	want := mustLines(t, "Host web\n  User u\n")
	if strings.Join(out, "\n") != strings.Join(want, "\n") {
		t.Fatalf("out =\n%s", strings.Join(out, "\n"))
	}
}

func TestEditHostBlockStaleFence(t *testing.T) {
	file := mustLines(t, "Host web\n  User u\n")
	if _, err := EditHostBlock(file, 1, "old", "web", nil, nil); !errors.Is(err, ErrStaleBlock) {
		t.Fatalf("err = %v, want ErrStaleBlock", err)
	}
	if _, err := EditHostBlock(file, 99, "web", "web", nil, nil); !errors.Is(err, ErrStaleBlock) {
		t.Fatalf("err = %v, want ErrStaleBlock", err)
	}
	if _, err := EditHostBlock(file, 2, "u", "web", nil, nil); !errors.Is(err, ErrStaleBlock) {
		t.Fatalf("a directive line must not pass as a header, err = %v", err)
	}
}

func TestEditHostBlockRejectsBadHeader(t *testing.T) {
	file := mustLines(t, "Host web\n")
	for _, bad := range []string{"", "my host", `q"q`, "-oProxyCommand=echo"} {
		if _, err := EditHostBlock(file, 1, "web", bad, nil, nil); err == nil || errors.Is(err, ErrStaleBlock) {
			t.Fatalf("header %q: err = %v, want a validation error", bad, err)
		}
	}
}

func TestEditHostBlockToleratesEqualSyntaxAndMatchNeighbor(t *testing.T) {
	file := mustLines(t, "host=web\n  Port=2222\nMatch final\n  Compression yes\n")
	out, err := EditHostBlock(file, 1, "web", "web", []KV{{Key: "Port", Value: "2200"}}, nil)
	if err != nil {
		t.Fatal(err)
	}
	// `Keyword=value` is replaced in place; the Match block and its body
	// pass through untouched.
	want := mustLines(t, "host=web\n  Port 2200\nMatch final\n  Compression yes\n")
	if strings.Join(out, "\n") != strings.Join(want, "\n") {
		t.Fatalf("out =\n%s", strings.Join(out, "\n"))
	}
}

func TestAliasRejectsOptionLikePattern(t *testing.T) {
	entry := Entry{Kind: KindHost, Patterns: []string{"-oProxyCommand=echo"}}
	if entry.Alias() != "" {
		t.Fatalf("alias = %q, want empty", entry.Alias())
	}
}
