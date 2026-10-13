package history

import "testing"

// TestCleanTitleCandidateStripsSkillBlocks pins F166: a session whose first
// message opens with a skill invocation must not surface the injected XML
// (or its attribute-carrying open tag, which overruns the generic tag scan)
// as the user-visible title.
func TestCleanTitleCandidateStripsSkillBlocks(t *testing.T) {
	raw := `<skill name="think" location="/Users/wilson/.agents/skills/think/SKILL.md">
References are relative to /Users/wilson/.agents/skills/think.
# Think: Design and Validate Before You Build…
</skill>
帮我把面板可以拖宽`
	if got, want := cleanTitleCandidate(raw), "帮我把面板可以拖宽"; got != want {
		t.Fatalf("cleanTitleCandidate = %q, want %q", got, want)
	}
	// A message that is only a skill block has no title candidate at all —
	// the adapter falls back to its next candidate.
	if got := cleanTitleCandidate("<skill name=\"x\">only</skill>"); got != "" {
		t.Fatalf("skill-only message produced %q, want empty", got)
	}
	// Ordinary prose with a short inline tag keeps the existing behavior.
	if got, want := cleanTitleCandidate("fix the <b>bold</b> path"), "fix the bold path"; got != want {
		t.Fatalf("plain text = %q, want %q", got, want)
	}
}
