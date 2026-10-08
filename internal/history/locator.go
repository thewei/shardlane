package history

type SourceLocatorKind string

const (
	LocatorNativeID     SourceLocatorKind = "native-id"
	LocatorFilePath     SourceLocatorKind = "file-path"
	LocatorMetadataOnly SourceLocatorKind = "metadata-only"
)

type SessionSourceLocator struct {
	Kind     SourceLocatorKind
	Agent    AgentID
	Identity string
}

func (l SessionSourceLocator) NativeIdentity() string { return l.Identity }

// ResolveSessionSourceLocator mirrors the measured Herdr live protocol:
// kind=path is an exact source path; kind=id is usable only for providers with
// an authoritative live semantic source. Unknown kinds are never guessed.
func ResolveSessionSourceLocator(agent AgentID, kind, source, value string) SessionSourceLocator {
	_ = source // retained in the boundary for future measured protocol shapes.
	switch {
	case kind == "path":
		return SessionSourceLocator{Kind: LocatorFilePath, Agent: agent, Identity: value}
	case kind == "id" && liveSemanticCapable(agent):
		return SessionSourceLocator{Kind: LocatorNativeID, Agent: agent, Identity: value}
	default:
		return SessionSourceLocator{Kind: LocatorMetadataOnly, Agent: agent, Identity: value}
	}
}

func liveSemanticCapable(agent AgentID) bool {
	switch agent {
	case AgentClaudeCode, AgentCodex, AgentCursor, AgentCommandCode, AgentPi, AgentOMP, AgentKimi, AgentAntigravity:
		return true
	default:
		return false
	}
}
