package conversation

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
)

// ConversationID is the opaque public identity of one conversation. Native
// UI never parses provider paths or native ids out of it (0.6 §4).
type ConversationID string

// LocatorKind distinguishes the required locator classes (0.6 §4.1).
type LocatorKind string

const (
	LocatorLive    LocatorKind = "live"
	LocatorHistory LocatorKind = "history"
)

// SessionIdentity is the typed AgentSessionInfo fingerprint input: the
// exact provider + locator kind + source + value binding of one live
// session (0.6 §4.2).
type SessionIdentity struct {
	Provider string
	Kind     string // "id" | "path" | ...
	Source   string
	Value    string
}

// Fingerprint derives the session-exact fingerprint. A stale Conversation
// mutation whose bound fingerprint no longer matches the pane's current
// occupant must fail closed — never retarget to the replacement occupant.
func (s SessionIdentity) Fingerprint() string {
	sum := sha256.Sum256([]byte(s.Provider + "\x00" + s.Kind + "\x00" + s.Source + "\x00" + s.Value))
	return hex.EncodeToString(sum[:])
}

// ConversationID builds the opaque public id: the locator class plus the
// stable native key, e.g. "history:claude-code:sess-1".
func NewConversationID(kind LocatorKind, provider, nativeKey string) (ConversationID, error) {
	provider = strings.TrimSpace(provider)
	nativeKey = strings.TrimSpace(nativeKey)
	if kind == "" || provider == "" || nativeKey == "" {
		return "", fmt.Errorf("conversation id requires kind, provider and native key")
	}
	return ConversationID(string(kind) + ":" + provider + ":" + nativeKey), nil
}

// ParseConversationID decomposes an opaque id for routing back to its
// source. Unknown shapes fail closed.
func ParseConversationID(id ConversationID) (LocatorKind, string, string, error) {
	parts := strings.SplitN(string(id), ":", 3)
	if len(parts) != 3 || parts[0] == "" || parts[1] == "" || parts[2] == "" {
		return "", "", "", fmt.Errorf("malformed conversation id %q", id)
	}
	switch LocatorKind(parts[0]) {
	case LocatorLive, LocatorHistory:
	default:
		return "", "", "", fmt.Errorf("unknown conversation locator %q", parts[0])
	}
	return LocatorKind(parts[0]), parts[1], parts[2], nil
}

// NormalizePathKeySafe is a small shared helper for display trimming.
func NormalizePathKeySafe(path string) string {
	return strings.TrimSpace(path)
}
