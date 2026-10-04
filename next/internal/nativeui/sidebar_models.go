package nativeui

import "time"

// RecentItem is presentation-only metadata. M3 History will provide these
// values from the read-only HistoryService without loading transcript bodies.
type RecentItem struct {
	ID        string
	Title     string
	Subtitle  string
	Kind      string
	UpdatedAt time.Time
}

func (s *Shell) projectExpanded(id string) bool {
	if expanded, ok := s.expandedProjects[id]; ok {
		return expanded
	}
	return id != "" && id == s.selectedProjectID
}

func (s *Shell) tabExpanded(id string) bool {
	if expanded, ok := s.expandedTabs[id]; ok {
		return expanded
	}
	return id != "" && id == s.selectedTabID
}

func (s *Shell) setProjectExpanded(id string, expanded bool) {
	if id == "" {
		return
	}
	s.expandedProjects[id] = expanded
}

func (s *Shell) setTabExpanded(id string, expanded bool) {
	if id == "" {
		return
	}
	s.expandedTabs[id] = expanded
}

func (s *Shell) reconcileTreeExpansion(previous, next string, projects bool) {
	if next == "" || previous == next {
		return
	}
	if projects {
		s.expandedProjects[next] = true
		return
	}
	s.expandedTabs[next] = true
}
