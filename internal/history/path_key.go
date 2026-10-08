package history

import "path/filepath"

// NormalizePathKey matches shardlane-history's normalization rule: collapse
// redundant separators and "." components without filesystem canonicalization.
// Empty input remains empty instead of becoming ".".
func NormalizePathKey(value string) string {
	if value == "" {
		return ""
	}
	return filepath.Clean(value)
}
