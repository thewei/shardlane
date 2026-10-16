//go:build windows && (amd64 || arm64)

package windows

import "testing"

// TestChromiumLanguage checks the tags WebView2's browser is given for
// Chinese, which Chromium names by region.
func TestChromiumLanguage(t *testing.T) {
	for tag, want := range map[string]string{
		"zh-Hans":    "zh-CN",
		"zh-Hant":    "zh-TW",
		"zh-Hant-HK": "zh-HK",
		"zh-TW":      "zh-TW",
		"zh-Hans-SG": "zh-CN",
		"zh":         "zh-CN",
		"en-GB":      "en-GB",
		"sr-Latn":    "sr-Latn",
	} {
		if got := chromiumLanguage(tag); got != want {
			t.Errorf("chromiumLanguage(%q) = %q, want %q", tag, got, want)
		}
	}
}
