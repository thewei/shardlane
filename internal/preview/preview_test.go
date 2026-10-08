package preview

import (
	"errors"
	"testing"
)

// TestIsLoopbackURL pins P8 security boundary: only loopback targets allowed.
func TestIsLoopbackURL(t *testing.T) {
	valid := []string{
		"http://localhost",
		"http://localhost:3000",
		"https://localhost:8443",
		"http://127.0.0.1",
		"http://127.0.0.1:8080",
		"http://127.0.0.2:9000",
		"http://[::1]:5173",
		"http://[0:0:0:0:0:0:0:1]:3000",
	}

	for _, u := range valid {
		if !IsLoopbackURL(u) {
			t.Fatalf("expected valid loopback target: %q", u)
		}
	}

	invalid := []string{
		"http://google.com",
		"https://example.com/api",
		"http://192.168.1.1:3000",
		"http://10.0.0.1:8080",
		"file:///etc/passwd",
		"ftp://localhost",
		"javascript:alert(1)",
		"data:text/html,hello",
		"http://127.0.0.1.attacker.com",
		"http://localhost.evil.com",
		"",
	}

	for _, u := range invalid {
		if IsLoopbackURL(u) {
			t.Fatalf("expected invalid loopback target: %q", u)
		}
	}
}

// TestNormalizePreviewTarget pins scheme normalization and validation.
func TestNormalizePreviewTarget(t *testing.T) {
	clean, err := NormalizePreviewTarget("localhost:3000")
	if err != nil || clean != "http://localhost:3000" {
		t.Fatalf("expected http://localhost:3000, got %q (err=%v)", clean, err)
	}

	clean, err = NormalizePreviewTarget("http://127.0.0.1:8080/app")
	if err != nil || clean != "http://127.0.0.1:8080/app" {
		t.Fatalf("expected http://127.0.0.1:8080/app, got %q (err=%v)", clean, err)
	}

	_, err = NormalizePreviewTarget("https://external.com")
	if !errors.Is(err, ErrNotLoopback) {
		t.Fatalf("expected ErrNotLoopback, got %v", err)
	}
}
