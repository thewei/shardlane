package sshconfig

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestParseEffectiveOutput(t *testing.T) {
	kvs := parseEffectiveOutput([]byte("user deploy\nhostname web.example.com\n\nidentityfile ~/.ssh/id_a\nidentityfile ~/.ssh/id_b\n\r\n"))
	if len(kvs) != 4 {
		t.Fatalf("kvs = %+v", kvs)
	}
	if kvs[0].Key != "user" || kvs[0].Value != "deploy" {
		t.Fatalf("kv[0] = %+v", kvs[0])
	}
	if kvs[2].Key != "identityfile" || kvs[2].Value != "~/.ssh/id_a" {
		t.Fatalf("kv[2] = %+v", kvs[2])
	}
}

// TestResolveTarget runs the whole path against a fake `ssh` on PATH so the
// command shape (ssh -G <alias>) is pinned without a real network client.
func TestResolveTarget(t *testing.T) {
	bin := t.TempDir()
	script := "#!/bin/sh\nfor arg in \"$@\"; do case \"$arg\" in -*) ;; *) echo \"ARG=$arg\" >&2;; esac; done\n" +
		"cat <<'EOF'\nuser deploy\nhostname web.example.com\nport 2200\nEOF\n"
	if err := os.WriteFile(filepath.Join(bin, "ssh"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))

	target, err := ResolveTarget(context.Background(), "web")
	if err != nil {
		t.Fatal(err)
	}
	if target != "deploy@web.example.com" {
		t.Fatalf("target = %q", target)
	}
}

func TestEffectiveConfigRejectsOptionLikeAlias(t *testing.T) {
	_, err := EffectiveConfig(context.Background(), "-ProxyCommand=evil")
	if err == nil || !strings.Contains(err.Error(), "invalid host alias") {
		t.Fatalf("err = %v, want invalid host alias", err)
	}
	// A rejected alias must fail before spawning ssh; with no ssh on a
	// broken PATH the error would instead be an exec failure.
	if _, err := EffectiveConfig(context.Background(), ""); err == nil || errors.Is(err, os.ErrNotExist) && strings.Contains(err.Error(), "executable file not found") {
		t.Fatalf("empty alias must be rejected locally, got %v", err)
	}
}
