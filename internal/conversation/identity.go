package conversation

import (
	"crypto/sha256"
	"encoding/hex"
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
