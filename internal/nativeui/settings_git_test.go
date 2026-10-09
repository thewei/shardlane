package nativeui

/**
 * [INPUT]: persisted git preferences, Native settings Git page and MyGo Tester
 * [OUTPUT]: default commit policy, actionable save/restore and Pi path configuration
 * [POS]: UI-013 Git settings acceptance, no process launch or Git mutation
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/settings"
)

func TestGitPreferencesPageDefaultAndSave(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/git")
	tester := ui.NewTester(s.View, 1200, 800)
	for _, title := range []string{"Git & AI", "Commit Message", "Save Commit Rules", "Restore Default Rules", "Pi Agent", "Save Pi Path"} {
		if !tester.HasText(title) {
			t.Fatalf("Git Settings missing %q: %q", title, tester.Texts())
		}
	}
	if !strings.Contains(s.settingsGitPromptDraft, "Conventional Commits") {
		t.Fatal("default commit policy missing")
	}
	s.settingsGitPromptDraft = "fix(scope): describe precise changes"
	if err := tester.Click("Save Commit Rules"); err != nil {
		t.Fatal(err)
	}
	if s.settings.Workbench.CommitPrompt != "fix(scope): describe precise changes" {
		t.Fatal("Commit rules were not persisted through settings service")
	}
	s.settingsPiBinaryDraft = "/tmp/explicit-pi"
	if err := tester.Click("Save Pi Path"); err != nil {
		t.Fatal(err)
	}
	if s.settings.Workbench.PiExecutable != "/tmp/explicit-pi" {
		t.Fatal("Pi path not persisted")
	}
	if err := tester.Click("Restore Default Rules"); err != nil {
		t.Fatal(err)
	}
	if s.settings.Workbench.CommitPrompt != "" || s.settingsGitPromptDraft != settings.DefaultCommitPrompt {
		t.Fatal("Reset default prompt failed")
	}
}
