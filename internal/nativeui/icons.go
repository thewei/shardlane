package nativeui

import "github.com/egoist/mygo/ui"

func icon(shapes string) *ui.SVG {
	return ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">` + shapes + `</svg>`))
}

// The icon language follows the same light outline vocabulary as Lucide/shadcn
// while remaining self-contained in the native executable.
var (
	iconWorkspace   = icon(`<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M8 5V3h8v2"/>`)
	iconGlobe       = icon(`<circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>`)
	iconFolder      = icon(`<path d="M3 7h7l2 2h9v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M3 7V5a2 2 0 0 1 2-2h5l2 2"/>`)
	iconFolderOpen  = icon(`<path d="M3 7h7l2 2h9"/><path d="M3 7V5a2 2 0 0 1 2-2h5l2 2"/><path d="M3 10h18l-2 9H5z"/>`)
	iconTab         = icon(`<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 9h10"/>`)
	iconTerminal    = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3"/><path d="M13 15h4"/>`)
	iconChat        = icon(`<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>`)
	iconAgent       = icon(`<circle cx="12" cy="8" r="4"/><path d="M5 21a7 7 0 0 1 14 0"/>`)
	iconAgentBot    = icon(`<path d="M12 8V4H8"/><rect x="4" y="8" width="16" height="12" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>`)
	iconSearch      = icon(`<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>`)
	iconHistory     = icon(`<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l3 2"/>`)
	iconSettings    = icon(`<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6V21h-4v-.1a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H3v-4h.1a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V3h4v.1a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9A1.7 1.7 0 0 0 21 10h.1v4H21a1.7 1.7 0 0 0-1.6 1z"/>`)
	iconPlus        = icon(`<path d="M12 5v14M5 12h14"/>`)
	iconRefresh     = icon(`<path d="M20 11a8 8 0 0 0-14.8-4L3 10"/><path d="M3 4v6h6"/><path d="M4 13a8 8 0 0 0 14.8 4L21 14"/><path d="M21 20v-6h-6"/>`)
	iconSplit       = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>`)
	iconSplitDown   = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/>`)
	iconZoom        = icon(`<path d="M8 3H3v5M16 3h5v5M8 21H3v-5M16 21h5v-5"/>`)
	iconArrowLeft   = icon(`<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>`)
	iconChevron     = icon(`<path d="m9 18 6-6-6-6"/>`)
	iconChevronDown = icon(`<path d="m6 9 6 6 6-6"/>`)
	iconBranch      = icon(`<circle cx="6" cy="6" r="3"/><path d="M6 9v3a3 3 0 0 0 3 3h6"/><circle cx="18" cy="18" r="3"/><path d="M18 15V9a3 3 0 0 0-3-3"/>`)
	iconFile        = icon(`<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>`)
	iconChanges     = icon(`<path d="M14 3h5a2 2 0 0 1 2 2v7"/><path d="M8 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5"/><path d="M12 9v6M9 12h6"/><path d="M14 17h6M17 14v6"/>`)
	iconPin         = icon(`<path d="M12 17v5"/><path d="M9 3h6l1 7 3 3H5l3-3 1-7Z"/>`)
	iconEllipsis    = icon(`<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>`)
	iconTask        = icon(`<rect x="3" y="3" width="18" height="18" rx="3"/><path d="m8 12 2.5 2.5L16 9"/>`)
	iconPanel       = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>`)
)
