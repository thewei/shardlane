package nativeui

/**
 * [INPUT]: manually pasted candidate messages, native commit draft and explicit replacement actions
 * [OUTPUT]: guards against unreviewed overwrite, oversize/invalid input, and accidental Git mutation
 * [POS]: Commit AI response handoff regression; no external AI/service/clipboard use
 * [PROTOCOL]: update header on change and check CLAUDE.md
 */

import (
	"strings"
	"testing"
)

func TestParseCommitCandidate(t *testing.T) {
	for _, tc := range []struct {
		input, subject, body string
	}{
		{"fix: resolve sync issue", "fix: resolve sync issue", ""},
		{"feat: add Git workflow\n\n- support merge\n- handle conflicts", "feat: add Git workflow", "- support merge\n- handle conflicts"},
		{"```text\nfix: handle escaped paths\n\nMore context\n```", "fix: handle escaped paths", "More context"},
	} {
		subject, body, err := parseCommitCandidate(tc.input)
		if err != nil || subject != tc.subject || body != tc.body {
			t.Fatalf("parse(%q) => %q, %q, %v", tc.input, subject, body, err)
		}
	}
	for _, invalid := range []string{"", "\x00secret", strings.Repeat("s", 73), strings.Repeat("x", maxCommitCandidateBytes+1), "```text\nno closure"} {
		if _, _, err := parseCommitCandidate(invalid); err == nil {
			t.Errorf("invalid suggestion %q was accepted", invalid[:min(len(invalid), 60)])
		}
	}
}

func TestManualAISuggestionRequiresReplacementApproval(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.Subject = "fix: existing manual work"
	s.surface.commit.Body = "keep me"
	s.commitAIResponse = "feat: new suggestion\n\nProposed body"
	s.applyManualAISuggestion()
	if s.surface.commit.Subject != "fix: existing manual work" || s.surface.commit.Body != "keep me" || !s.commitAIReplaceConfirm {
		t.Fatal("suggested message overwrote unsaved manual edits without consent")
	}
	s.commitAIReplaceConfirm = false // user chose to keep existing message
	if s.surface.commit.Subject != "fix: existing manual work" {
		t.Fatal("keep current must preserve existing draft")
	}
	s.applyManualAISuggestion()
	s.replaceCommitMessageWithSuggestion()
	if s.surface.commit.Subject != "feat: new suggestion" || s.surface.commit.Body != "Proposed body" || s.commitAIResponse != "" {
		t.Fatal("explicit replacement did not populate exactly the commit draft")
	}
	if s.surface.commit.InFlight || s.git.committing {
		t.Fatal("accepting a suggestion must not start a commit")
	}
	s.cancelCommitSurface()
	if s.commitAIReplaceConfirm || s.commitPromptPreviewOpen || s.commitAIResponse != "" {
		t.Fatal("manual AI suggestion state leaked after commit editor close")
	}
}

func TestManualAISuggestionRejectsInvalidWithoutAlteringDraft(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.commitAIResponse = strings.Repeat("x", 90)
	s.applyManualAISuggestion()
	if s.surface.commit.Subject != "" || s.commitAIError == "" || s.commitAIReplaceConfirm {
		t.Fatal("invalid candidate must leave the commit draft unchanged with an error")
	}
}
