package history

import (
	"encoding/json"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

func clipText(text string, max int) (string, bool) {
	if len(text) <= max {
		return text, false
	}
	end := max
	for end > 0 && !utf8.RuneStart(text[end]) {
		end--
	}
	return text[:end] + "\n… (truncated)", true
}

func epochMS(value any) int64 {
	switch value := value.(type) {
	case string:
		parsed, err := time.Parse(time.RFC3339Nano, value)
		if err != nil {
			return 0
		}
		return parsed.UnixMilli()
	case float64:
		if value > 1e12 {
			return int64(value)
		}
		if value > 0 {
			return int64(value * 1000)
		}
	case json.Number:
		number, err := strconv.ParseFloat(string(value), 64)
		if err == nil {
			return epochMS(number)
		}
	}
	return 0
}

func projectName(cwd string) string {
	if cwd == "" {
		return "Unknown project"
	}
	name := filepath.Base(cwd)
	if name == "." || name == string(filepath.Separator) || name == "" {
		return "Unknown project"
	}
	return name
}

func cleanTitleCandidate(raw string) string {
	text := stripTagBlock(raw, "system-reminder")
	text = stripTagBlock(text, "local-command-caveat")
	text = stripTagBlock(text, "local-command-stdout")

	args := extractTag(text, "command-args")
	name := extractTag(text, "command-name")
	if strings.TrimSpace(args) != "" || strings.TrimSpace(name) != "" {
		if strings.TrimSpace(args) != "" {
			text = args
		} else {
			text = name
		}
	}

	var out strings.Builder
	for index := 0; index < len(text); {
		if text[index] != '<' {
			out.WriteByte(text[index])
			index++
			continue
		}
		closeIndex := strings.IndexByte(text[index:], '>')
		if closeIndex < 0 || closeIndex > 61 {
			out.WriteByte('<')
			index++
			continue
		}
		out.WriteByte(' ')
		index += closeIndex + 1
	}
	compact := strings.Join(strings.Fields(out.String()), " ")
	runes := []rune(compact)
	if len(runes) > MaxTitle {
		return string(runes[:MaxTitle]) + "…"
	}
	return compact
}

func stripTagBlock(text, tag string) string {
	open := "<" + tag + ">"
	closeTag := "</" + tag + ">"
	for {
		start := strings.Index(text, open)
		if start < 0 {
			return text
		}
		endRelative := strings.Index(text[start:], closeTag)
		if endRelative < 0 {
			return text[:start] + " "
		}
		end := start + endRelative + len(closeTag)
		text = text[:start] + " " + text[end:]
	}
}

func extractTag(text, tag string) string {
	open := "<" + tag + ">"
	closeTag := "</" + tag + ">"
	start := strings.Index(text, open)
	if start < 0 {
		return ""
	}
	body := text[start+len(open):]
	end := strings.Index(body, closeTag)
	if end < 0 {
		return ""
	}
	return body[:end]
}

func injectedUserContent(text string) bool {
	trimmed := strings.TrimLeft(text, " \t\r\n")
	prefixes := []string{
		"<recommended_plugins",
		"<environment_context",
		"<user_instructions",
		"<permissions",
		"<workspace",
		"<system-",
		"<context ",
		"<session_context",
		"IMPORTANT: Do NOT read",
		"Caveat: The messages below",
		"# Files pasted by the user",
	}
	for _, prefix := range prefixes {
		if strings.HasPrefix(trimmed, prefix) {
			return true
		}
	}
	return strings.Contains(trimmed, "/.codex/plugins/") ||
		(strings.Contains(trimmed, "/plugins/cache/") && strings.Contains(trimmed, "SKILL.md"))
}

func stringifyToolResult(content any) string {
	switch value := content.(type) {
	case string:
		return value
	case []any:
		var parts []string
		for _, item := range value {
			block, _ := item.(map[string]any)
			switch block["type"] {
			case "text":
				if text, ok := block["text"].(string); ok {
					parts = append(parts, text)
				}
			case "image":
				parts = append(parts, "[image]")
			}
		}
		return strings.Join(parts, "\n")
	case map[string]any:
		data, _ := json.Marshal(value)
		return string(data)
	default:
		return ""
	}
}

func toolInput(input any) (preview string, full *string) {
	if input == nil {
		return "", nil
	}
	data, err := json.MarshalIndent(input, "", "  ")
	if err != nil || string(data) == "null" {
		return "", nil
	}
	text, _ := clipText(string(data), MaxToolIO)
	full = &text

	if object, ok := input.(map[string]any); ok {
		for _, key := range []string{"command", "file_path", "path", "pattern", "query", "url", "description"} {
			if value, ok := object[key].(string); ok && strings.TrimSpace(value) != "" {
				preview, _ := clipText(strings.TrimSpace(value), 200)
				return preview, full
			}
		}
	}
	preview, _ = clipText(strings.Join(strings.Fields(string(data)), " "), 200)
	return preview, full
}
