package nativeui

/**
 * [INPUT]: 依赖 utf8 符文边界
 * [OUTPUT]: 提供 clipNativeText/clipPreview/cutAfterLines：按字节与行数的 Unicode 安全截断，带显式截断标记
 * [POS]: Inspector 摘要与工具输出预览共用的文本截断工具；不依赖任何框架或会话状态
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import "unicode/utf8"

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

// clipPreview truncates text to both a line and a character bound,
// Unicode-safely. Only the character bound appends the truncation marker.
func clipPreview(text string, maxChars, maxLines int) string {
	return clipNativeText(cutAfterLines(text, maxLines), maxChars)
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
