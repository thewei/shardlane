package nativeui

/**
 * [INPUT]: user-supplied AI message text from a trusted manual paste
 * [OUTPUT]: validated local Commit subject/body candidate, never a Git mutation
 * [POS]: Native Commit modal handoff; provider-independent, no external request
 * [PROTOCOL]: update header on change and check CLAUDE.md
 */

import (
	"errors"
	"strings"
	"unicode/utf8"
)

const maxCommitCandidateBytes = 4096

// parseCommitCandidate deliberately accepts only the *message*, not an AI
// protocol response. The user reviews and approves the result before the
// existing Commit transaction can use the draft.
func parseCommitCandidate(text string) (subject, body string, err error) {
	if len(text) > maxCommitCandidateBytes {
		return "", "", errors.New("AI suggestion is too long; paste at most 4096 bytes")
	}
	raw := strings.TrimSpace(strings.ReplaceAll(text, "\r\n", "\n"))
	if strings.ContainsRune(raw, 0) {
		return "", "", errors.New("AI suggestion contains unsupported control characters")
	}
	if strings.HasPrefix(raw, "```") {
		lines := strings.Split(raw, "\n")
		if len(lines) < 3 || !strings.HasPrefix(strings.TrimSpace(lines[len(lines)-1]), "```") {
			return "", "", errors.New("AI response code fence is incomplete")
		}
		raw = strings.TrimSpace(strings.Join(lines[1:len(lines)-1], "\n"))
	}
	lines := strings.SplitN(raw, "\n", 2)
	subject = strings.TrimSpace(lines[0])
	if subject == "" {
		return "", "", errors.New("AI suggestion needs a subject on the first line")
	}
	if utf8.RuneCountInString(subject) > 72 {
		return "", "", errors.New("AI suggestion subject exceeds 72 characters; edit it before applying")
	}
	if len(lines) > 1 {
		body = strings.TrimSpace(lines[1])
	}
	return subject, body, nil
}

func (s *Shell) resetCommitAIFlow() {
	s.resetPiCommitSuggestion()
	s.commitPromptPreviewOpen = false
	s.commitPromptIncludeExcerpt = false
	s.commitPromptTextVisible = false
	s.commitAIManualEntryOpen = false
	s.commitAIResponse = ""
	s.commitAIError = ""
	s.commitAIReplaceConfirm = false
}

func (s *Shell) applyManualAISuggestion() {
	subject, body, err := parseCommitCandidate(s.commitAIResponse)
	if err != nil {
		s.commitAIError = err.Error()
		return
	}
	s.commitAIError = ""
	if s.surface.commit.Subject != "" || s.surface.commit.Body != "" {
		s.commitAIReplaceConfirm = true
		return
	}
	s.surface.commit.Subject, s.surface.commit.Body = subject, body
	s.commitAIResponse = ""
	s.commitAIReplaceConfirm = false
	s.commitPromptPreviewOpen = false
}

func (s *Shell) replaceCommitMessageWithSuggestion() {
	subject, body, err := parseCommitCandidate(s.commitAIResponse)
	if err != nil {
		s.commitAIError = err.Error()
		s.commitAIReplaceConfirm = false
		return
	}
	s.surface.commit.Subject, s.surface.commit.Body = subject, body
	s.commitAIResponse = ""
	s.commitAIError = ""
	s.commitAIReplaceConfirm = false
	s.commitPromptPreviewOpen = false
}
