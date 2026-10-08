// Package herdr defines client protocol bindings to the authoritative Herdr daemon.
//
// P15 Terminal Find Capability Gate (0.9 P15 / WIX-190..194):
// Protocol audit conclusion: `pane.copy_search` exists in Herdr Protocol 22.
// Shardlane strictly delegates terminal search to Herdr's semantic scrollback
// authority and never implements a client-side VT shadow index.
package herdr

import (
	"context"
	"encoding/json"
	"errors"
)

// PaneTextPoint defines coordinates in Herdr scrollback.
type PaneTextPoint struct {
	Col uint32 `json:"col"`
	Row uint32 `json:"row"`
}

// PaneTextRange defines a matched span in Herdr scrollback.
type PaneTextRange struct {
	Start PaneTextPoint `json:"start"`
	End   PaneTextPoint `json:"end"`
}

// PaneCopySearchParams is the exact parameter payload for `pane.copy_search` in Protocol 22.
type PaneCopySearchParams struct {
	PaneID          string         `json:"pane_id"`
	Query           string         `json:"query"`
	Direction       string         `json:"direction"` // "forward" | "backward"
	Cursor          PaneTextPoint  `json:"cursor"`
	ContentRevision uint64         `json:"content_revision"`
	Previous        *PaneTextRange `json:"previous,omitempty"`
}

// PaneCopySearchResult is the exact response result from `pane.copy_search`.
type PaneCopySearchResult struct {
	Type            string          `json:"type"` // "pane_copy_search"
	PaneID          string          `json:"pane_id"`
	ContentRevision uint64          `json:"content_revision"`
	Matches         []PaneTextRange `json:"matches"`
	Total           uint64          `json:"total"`
	Current         *uint32         `json:"current,omitempty"`
	CurrentGlobal   *uint64         `json:"current_global,omitempty"`
}

// CopySearch executes semantic terminal scrollback search via Herdr.
func (m *Manager) CopySearch(ctx context.Context, session string, params PaneCopySearchParams) (*PaneCopySearchResult, error) {
	if params.PaneID == "" || params.Query == "" {
		return nil, errors.New("pane_id and query are required")
	}
	if params.Direction == "" {
		params.Direction = "forward"
	}

	socket, err := m.reachableSocket(session)
	if err != nil {
		return nil, err
	}

	var raw json.RawMessage
	if err := callRPCWithContext(ctx, socket, "pane.copy_search", params, false, &raw); err != nil {
		return nil, err
	}

	var result PaneCopySearchResult
	if err := decodeTypedResult(raw, "pane_copy_search", &result); err != nil {
		return nil, err
	}

	return &result, nil
}
