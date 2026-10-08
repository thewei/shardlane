package nativeui

import (
	"unicode/utf8"

	"github.com/wh-studio/herdr-client/internal/history"
)

// Presentation bounds for the Native History detail, pinned by the 0.3.0
// plan: long bodies preview 1,800 chars / 18 lines, long thinking previews
// 700 chars / 10 lines. Full content is created only after explicit
// expansion.
const (
	historyMessagePreviewChars = 1800
	historyMessagePreviewLines = 18
	historyThinkingChars       = 700
	historyThinkingLines       = 10
)

type historyBlockKind uint8

const (
	historyBlockText historyBlockKind = iota
	historyBlockMeta
	historyBlockCompactSummary
)

type historyToolCard struct {
	Call history.ToolCall
}

// historyBlock is one presentation block of the Native transcript detail. It
// is toolkit-independent: pure data the page renders with native widgets.
type historyBlock struct {
	Seq       int64
	Kind      historyBlockKind
	Role      history.Role
	Text      string
	Truncated bool
	Thinking  string
	Tools     []historyToolCard
	Timestamp *int64
}

// clipNativeText truncates to max bytes on a rune boundary with an explicit
// truncation marker, mirroring the history package's clip contract.
func clipNativeText(text string, max int) string {
	if len(text) <= max {
		return text
	}
	end := max
	for end > 0 && !utf8.RuneStart(text[end]) {
		end--
	}
	return text[:end] + "\n… (truncated)"
}

// clipPreview truncates text to both a character and a line bound,
// Unicode-safely, appending an explicit marker when clipped.
func clipPreview(text string, maxChars, maxLines int) (string, bool) {
	lineBound := cutAfterLines(text, maxLines)
	lineTruncated := len(lineBound) < len(text)
	clipped := clipNativeText(lineBound, maxChars)
	return clipped, lineTruncated || len(clipped) < len(lineBound)
}

// cutAfterLines returns text up to the start of line maxLines.
func cutAfterLines(text string, maxLines int) string {
	if maxLines < 0 {
		return ""
	}
	count := 0
	for index := 0; index < len(text); index++ {
		if text[index] != '\n' {
			continue
		}
		count++
		if count >= maxLines {
			return text[:index]
		}
	}
	return text
}

// historyBlocksFromMessages converts normalized transcript messages into the
// bounded presentation blocks. Tool calls attach to their host message as
// cards; thinking stays with its message and is collapsed at render time.
func historyBlocksFromMessages(messages []history.TranscriptMessage) []historyBlock {
	blocks := make([]historyBlock, 0, len(messages))
	for _, message := range messages {
		block := historyBlock{
			Seq:       message.Seq,
			Role:      message.Role,
			Timestamp: message.Timestamp,
		}
		switch message.Kind {
		case history.MessageMeta:
			block.Kind = historyBlockMeta
			block.Text = message.Text
		case history.MessageCompactSummary:
			block.Kind = historyBlockCompactSummary
			block.Text = message.Text
		default:
			block.Kind = historyBlockText
			preview, truncated := clipPreview(message.Text, historyMessagePreviewChars, historyMessagePreviewLines)
			block.Text = preview
			block.Truncated = truncated || message.Truncated
		}
		if message.Thinking != nil && *message.Thinking != "" {
			preview, _ := clipPreview(*message.Thinking, historyThinkingChars, historyThinkingLines)
			block.Thinking = preview
		}
		for _, call := range message.ToolCalls {
			block.Tools = append(block.Tools, historyToolCard{Call: call})
		}
		if block.Text == "" && block.Thinking == "" && len(block.Tools) == 0 {
			// C83: meta blocks (SYSTEM markers) used to render as empty cards
			// when the provider emitted header entries with no body — three
			// timestamp-only cards at the top of a Codex session. A block
			// with nothing to show is skipped regardless of kind.
			continue
		}
		blocks = append(blocks, block)
	}
	return blocks
}
