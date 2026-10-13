package history

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

// userKind classifies user-authored text: injected provider scaffolding is
// presented as metadata, not as a real user message.
func userKind(text string) MessageKind {
	if injectedUserContent(text) {
		return MessageMeta
	}
	return MessageText
}

// titleFromMessages derives the session title from the first real user
// message, matching the Rust title_from_messages contract.
func titleFromMessages(messages []TranscriptMessage) string {
	for _, message := range messages {
		if message.Role == RoleUser && message.Kind == MessageText {
			title := cleanTitleCandidate(message.Text)
			if title != "" {
				return title
			}
		}
	}
	return ""
}

// listJSONLRefs walks dir recursively and returns one reference per non-empty
// .jsonl file. A missing root is a legitimate empty source, not an error;
// a walk failure is, because contents cannot be confirmed.
func listJSONLRefs(dir string, agent AgentID, nativeID func(stem string) string) ([]SessionFileRef, error) {
	info, err := os.Stat(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return []SessionFileRef{}, nil
		}
		return nil, err
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("history root %s is not a directory", dir)
	}
	references := []SessionFileRef{}
	err = filepath.WalkDir(dir, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if !entry.Type().IsRegular() {
			return nil
		}
		name := entry.Name()
		stem, ok := strings.CutSuffix(name, ".jsonl")
		if !ok {
			return nil
		}
		meta, err := entry.Info()
		if err != nil || meta.Size() == 0 {
			return nil
		}
		references = append(references, SessionFileRef{
			Agent:     agent,
			NativeID:  nativeID(stem),
			FilePath:  path,
			MtimeMS:   meta.ModTime().UnixMilli(),
			SizeBytes: meta.Size(),
		})
		return nil
	})
	if err != nil {
		return nil, err
	}
	return references, nil
}
