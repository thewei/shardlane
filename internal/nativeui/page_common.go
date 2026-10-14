package nativeui

import "strings"

func fallbackText(values ...string) string {
	for _, value := range values {
		if value = strings.TrimSpace(value); value != "" {
			return value
		}
	}
	return ""
}

func compactSearchPath(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	parts := strings.Split(value, "/")
	if len(parts) > 3 && parts[1] == "Users" {
		return "~/" + strings.Join(parts[3:], "/")
	}
	return value
}
