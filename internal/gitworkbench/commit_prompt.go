package gitworkbench

/**
 * [INPUT]: reviewed ChangesSnapshot and explicit selected paths
 * [OUTPUT]: backward-compatible safe metadata-only Commit Message prompt
 * [POS]: compatibility façade for the single PrepareCommitPrompt privacy policy
 * [PROTOCOL]: update header when changing implementation; check CLAUDE.md
 */

// MaxCommitPromptBytes remains exported for callers that bound the legacy
// prompt. The canonical Preview path is always at or below this limit.
const MaxCommitPromptBytes = maxCommitPreviewBytes

// CommitMessagePrompt returns a privacy-preserving, metadata-only prompt.
// For explicitly approved code snippets, use PrepareCommitPrompt with opt-in.
// Deprecated: prefer PrepareCommitPrompt, which exposes privacy exclusions.
func CommitMessagePrompt(snap *ChangesSnapshot, selected []string) string {
	return PrepareCommitPrompt(snap, selected, false).Text
}
