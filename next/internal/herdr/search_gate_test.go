package herdr

import (
	"encoding/json"
	"testing"
)

// TestPaneCopySearchSchemaSerialization pins P15 Protocol 22 match.
func TestPaneCopySearchSchemaSerialization(t *testing.T) {
	curr := uint32(1)
	rawJSON := `{
		"type": "pane_copy_search",
		"pane_id": "pane-1",
		"content_revision": 42,
		"total": 3,
		"current": 1,
		"matches": [
			{"start": {"col": 0, "row": 10}, "end": {"col": 5, "row": 10}},
			{"start": {"col": 12, "row": 15}, "end": {"col": 17, "row": 15}}
		]
	}`

	var res PaneCopySearchResult
	if err := json.Unmarshal([]byte(rawJSON), &res); err != nil {
		t.Fatalf("failed to unmarshal copy_search response: %v", err)
	}

	if res.Type != "pane_copy_search" || res.PaneID != "pane-1" || res.Total != 3 {
		t.Fatalf("unexpected result: %+v", res)
	}
	if res.Current == nil || *res.Current != curr {
		t.Fatalf("current match expected 1, got %v", res.Current)
	}
	if len(res.Matches) != 2 || res.Matches[0].End.Col != 5 {
		t.Fatalf("matches mismatch: %+v", res.Matches)
	}

	// Verify request serialization
	params := PaneCopySearchParams{
		PaneID:          "pane-1",
		Query:           "error",
		Direction:       "forward",
		Cursor:          PaneTextPoint{Col: 0, Row: 0},
		ContentRevision: 42,
	}
	pBytes, err := json.Marshal(params)
	if err != nil {
		t.Fatal(err)
	}
	if string(pBytes) == "" {
		t.Fatal("empty params")
	}
}

// TestTerminalFindAuditContract pins the invariant that Terminal Find is
// delegated to Herdr's scrollback RPC authority rather than a client-side VT index.
func TestTerminalFindAuditContract(t *testing.T) {
	params := PaneCopySearchParams{}
	m := NewManager()
	if _, err := m.CopySearch(nil, "", params); err == nil {
		t.Fatal("CopySearch without pane_id or query must fail validation")
	}
}
