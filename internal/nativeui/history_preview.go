package nativeui

/**
 * [INPUT]: 依赖只读 HistoryService.Open 的尾部窗口（Tail）、Sessions 列表行与 MyGo Text/Button
 * [OUTPUT]: 提供行内会话预览的状态机（点击展开/收起、代际隔离的懒加载）、最近消息摘录与预览面板
 * [POS]: Sessions 列表的就地阅读层；每次展开现读原始会话文件，不缓存、不落盘、不跳详情页
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

const (
	// previewWindow is the tail window opened to find the excerpt; the
	// excerpt itself is the last previewMessages text messages in it.
	previewWindow   = 16
	previewMessages = 4
	// previewMaxChars bounds one excerpt line before the view clamps its
	// lines.
	previewMaxChars = 600
	previewMaxLines = 4
)

// historyPreview is the single open inline preview: which session, and the
// bounded excerpt read from its original file. The zero value is closed.
type historyPreview struct {
	key     string
	loading bool
	errText string
	total   int
	excerpt []previewLine
}

// previewLine is one excerpt message: who said it and a clipped body.
type previewLine struct {
	role history.Role
	text string
}

// toggleHistoryPreview opens the preview of one session, or closes it when
// it is the open one. Only one preview is open at a time.
func (s *Shell) toggleHistoryPreview(key string) {
	if s.hist.preview.key == key {
		s.closeHistoryPreview()
		return
	}
	s.openHistoryPreview(key)
}

// openHistoryPreview reads the session's newest messages on the background
// lane. A newer open (or close) bumps the generation, so a late result can
// never fill another row's preview.
func (s *Shell) openHistoryPreview(key string) {
	service := s.hist.service
	if service == nil || key == "" {
		return
	}
	if s.hist.previewCancel != nil {
		s.hist.previewCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.hist.previewCancel = cancel
	generation := s.hist.previewGen.Add(1)
	s.hist.preview = historyPreview{key: key, loading: true}

	s.dispatch(func() {
		window, err := service.Open(ctx, history.ConversationWindowRequest{
			ConversationID: key,
			Tail:           true,
			Limit:          previewWindow,
		})
		s.applyOnWindow(func() {
			if generation != s.hist.previewGen.Load() {
				return
			}
			preview := historyPreview{key: key}
			if err != nil {
				preview.errText = err.Error()
			} else {
				preview.total = window.TotalMessages
				preview.excerpt = previewExcerpt(window.Messages)
			}
			s.hist.preview = preview
		})
	})
}

// closeHistoryPreview drops the open preview and any read still in flight.
func (s *Shell) closeHistoryPreview() {
	if s.hist.previewCancel != nil {
		s.hist.previewCancel()
		s.hist.previewCancel = nil
	}
	s.hist.previewGen.Add(1)
	s.hist.preview = historyPreview{}
}

// previewExcerpt picks the last previewMessages readable messages — user and
// assistant text, never tool traffic, thinking or meta rows — oldest first.
func previewExcerpt(messages []history.TranscriptMessage) []previewLine {
	var picked []previewLine
	for i := len(messages) - 1; i >= 0 && len(picked) < previewMessages; i-- {
		message := messages[i]
		if message.Kind != history.MessageText || (message.Role != history.RoleUser && message.Role != history.RoleAssistant) {
			continue
		}
		text := strings.TrimSpace(message.Text)
		if text == "" {
			continue
		}
		picked = append(picked, previewLine{role: message.Role, text: previewClip(text)})
	}
	for left, right := 0, len(picked)-1; left < right; left, right = left+1, right-1 {
		picked[left], picked[right] = picked[right], picked[left]
	}
	return picked
}

// previewClip bounds one excerpt body on a rune boundary.
func previewClip(text string) string {
	runes := []rune(text)
	if len(runes) <= previewMaxChars {
		return text
	}
	return strings.TrimSpace(string(runes[:previewMaxChars])) + "…"
}

// sessionPreview is the panel under an open row: the latest messages, then
// the row's actions. It indents to the title column so the excerpt reads as
// part of the row above.
func (s *Shell) sessionPreview(c *ui.Context, summary history.SessionSummary) {
	sp := Spacing()
	t := c.Theme()
	tokens := designTokens(t.Dark)
	preview := s.hist.preview
	meta := summary.Meta
	ui.Column(c).FillWidth().Padding(0, rowInset, sp.M, rowInset+24+sp.L).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.M, sp.L).Gap(sp.M).Radius(Radius().Row).Background(tokens.Content).Children(func() {
			switch {
			case preview.loading:
				ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(func() {
					ui.Spinner(c).Size(14, 14)
					ui.Text(c, "Loading…").FontSize(Typography().Caption).TextColor(t.TextMuted)
				})
			case preview.errText != "":
				ui.Text(c, "Couldn't read this session.").FontSize(Typography().BodySmall)
				ui.Text(c, "The source file may have moved. Refresh the list to check.").
					FontSize(Typography().Caption).TextColor(t.TextMuted)
			case len(preview.excerpt) == 0:
				ui.Text(c, "No messages to show.").FontSize(Typography().BodySmall).TextColor(t.TextMuted)
			default:
				ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
					for _, line := range preview.excerpt {
						ui.Row(c).FillWidth().Gap(sp.L).Children(func() {
							// The row above already names the Agent; a display name
							// like "Claude Code" does not fit the speaker column.
							speaker := "You"
							if line.role == history.RoleAssistant {
								speaker = "Assistant"
							}
							ui.Text(c, speaker).FontSize(Typography().Caption).FontWeight(WeightMedium).
								TextColor(t.TextMuted).Width(historyMetricWidth + 8).Shrink(0).SingleLine().Ellipsis("…")
							ui.Text(c, line.text).FontSize(Typography().BodySmall).Grow(1).MinWidth(0).MaxLines(previewMaxLines)
						})
					}
				})
				if preview.total > len(preview.excerpt) {
					ui.Text(c, fmt.Sprintf("Latest messages of %d", preview.total)).
						FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
			}
			ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
				if ui.Button(c, "Open").Clicked() {
					s.router.Push("/history/" + meta.Key)
				}
				if ui.Button(c, "Continue").Disabled(!s.canContinue()).Clicked() {
					s.requestContinue(meta, nil)
				}
				ui.Spacer(c)
				label := "Hide"
				if s.hiddenSessionSet()[meta.Key] {
					label = "Restore"
				}
				if ui.Button(c, label).Clicked() {
					s.toggleSessionHidden(meta.Key)
					s.closeHistoryPreview()
				}
			})
		})
	})
}
