package agent

// AgentMarkers are the client-owned presentation markers (0.5 §3.3). They
// are never runtime authority and never manufactured from an initial
// projection — only transitions create them.
type AgentMarkers struct {
	Unread        bool
	ReviewPending bool
}

// MarkerStore holds per-Agent client markers, bounded to live Agent keys.
type MarkerStore struct {
	markers map[AgentKey]AgentMarkers
}

func NewMarkerStore() *MarkerStore {
	return &MarkerStore{markers: make(map[AgentKey]AgentMarkers)}
}

func (s *MarkerStore) Get(key AgentKey) AgentMarkers {
	return s.markers[key]
}

// Observe applies one classified transition to the stored markers.
// selected records whether the Agent is currently the selected/visited one:
// attention transitions create unread only when the Agent is not on screen.
func (s *MarkerStore) Observe(key AgentKey, transition AgentTransition, selected bool) AgentMarkers {
	markers := s.markers[key]
	switch transition {
	case TransitionReadyForReview:
		markers.ReviewPending = true
		if !selected {
			markers.Unread = true
		}
	case TransitionNeedsAttention, TransitionFailed:
		if !selected {
			markers.Unread = true
		}
	case TransitionReviewCleared:
		markers.ReviewPending = false
	case TransitionFirstObservation, TransitionNone,
		TransitionStartedWorking, TransitionReady:
		// Initial projection and non-marking transitions never manufacture
		// markers.
	}
	if !markers.Unread && !markers.ReviewPending {
		delete(s.markers, key)
		return AgentMarkers{}
	}
	s.markers[key] = markers
	return markers
}

// Visit clears unread only: visiting the Agent never clears review-pending.
func (s *MarkerStore) Visit(key AgentKey) AgentMarkers {
	markers := s.markers[key]
	markers.Unread = false
	return s.store(key, markers)
}

// MarkReviewed is the explicit action: clears review-pending and unread.
func (s *MarkerStore) MarkReviewed(key AgentKey) AgentMarkers {
	return s.store(key, AgentMarkers{})
}

// Release removes both markers with the Agent.
func (s *MarkerStore) Release(key AgentKey) {
	delete(s.markers, key)
}

// Retain prunes the store to the live Agent key set (bounded stores).
func (s *MarkerStore) Retain(live map[AgentKey]bool) {
	for key := range s.markers {
		if !live[key] {
			delete(s.markers, key)
		}
	}
}

func (s *MarkerStore) store(key AgentKey, markers AgentMarkers) AgentMarkers {
	if !markers.Unread && !markers.ReviewPending {
		delete(s.markers, key)
		return AgentMarkers{}
	}
	s.markers[key] = markers
	return markers
}
