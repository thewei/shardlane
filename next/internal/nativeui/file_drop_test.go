package nativeui

import (
	"errors"
	"strings"
	"testing"
)

// TestSafeShellQuoteBasicPaths pins P13 quoting rules.
func TestSafeShellQuoteBasicPaths(t *testing.T) {
	cases := []struct {
		paths []string
		want  string
	}{
		{
			paths: []string{},
			want:  "",
		},
		{
			paths: []string{"/usr/bin/git"},
			want:  "/usr/bin/git",
		},
		{
			paths: []string{"/Users/alice/My Documents/file.pdf"},
			want:  "'/Users/alice/My Documents/file.pdf'",
		},
		{
			paths: []string{"/path/with'quote.txt"},
			want:  "'/path/with'\\''quote.txt'",
		},
		{
			paths: []string{"/path/with$variable", "/path/with&ampersand;"},
			want:  "'/path/with$variable' '/path/with&ampersand;'",
		},
		{
			paths: []string{"file1.txt", "file 2.txt"},
			want:  "file1.txt 'file 2.txt'",
		},
	}

	for _, tc := range cases {
		got, err := SafeShellQuote(tc.paths)
		if err != nil {
			t.Fatalf("unexpected error for %v: %v", tc.paths, err)
		}
		if got != tc.want {
			t.Fatalf("SafeShellQuote(%v) = %q, want %q", tc.paths, got, tc.want)
		}
		// Invariant: must never end in a newline
		if strings.HasSuffix(got, "\n") || strings.HasSuffix(got, "\r") {
			t.Fatalf("quoted output must never contain trailing execution newlines: %q", got)
		}
	}
}

// TestSafeShellQuoteRejectsControlCharacters pins execution prevention:
// any newline or carriage return inside the path is strictly rejected.
func TestSafeShellQuoteRejectsControlCharacters(t *testing.T) {
	dangerous := []string{
		"/path/with\nrm -rf /",
		"/path/with\r\nmalicious",
		"/path/with\x00null",
		"/path/with\x1bescape",
	}

	for _, p := range dangerous {
		_, err := SafeShellQuote([]string{p})
		if !errors.Is(err, ErrControlCharacters) {
			t.Fatalf("expected ErrControlCharacters for dangerous path %q, got: %v", p, err)
		}
	}
}

// TestSafeShellQuoteBounds pins limits: max 256 paths and max 64 KiB.
func TestSafeShellQuoteBounds(t *testing.T) {
	// Over 256 paths
	manyPaths := make([]string, 257)
	for i := range manyPaths {
		manyPaths[i] = "file.txt"
	}
	_, err := SafeShellQuote(manyPaths)
	if !errors.Is(err, ErrTooManyPaths) {
		t.Fatalf("expected ErrTooManyPaths, got %v", err)
	}

	// Over 64 KiB
	longName := strings.Repeat("a", 1000)
	longPaths := make([]string, 70)
	for i := range longPaths {
		longPaths[i] = "/" + longName
	}
	_, err = SafeShellQuote(longPaths)
	if !errors.Is(err, ErrGeneratedTextTooBig) {
		t.Fatalf("expected ErrGeneratedTextTooBig, got %v", err)
	}
}
