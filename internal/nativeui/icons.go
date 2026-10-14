package nativeui

import "github.com/egoist/mygo/ui"

/**
 * [INPUT]: 依赖 github.com/egoist/mygo/ui 的 MustParseSVG
 * [OUTPUT]: 对外提供 iconWorkspace/iconFolder/iconTerminal/iconAgent/iconMinus/iconSquare/iconCopy 等 UI 矢量图标
 * [POS]: nativeui 的图标字典，被 titlebar/sidebar/terminal/breadcrumb 等呈现组件消费
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// iconStroke is the one stroke weight of every outline icon (24-unit grid):
// a light, SF-Symbols-like line. Heavier strokes made small glyphs read as
// clip-art, so weight is tuned here and nowhere else.
const iconStroke = "1.5"

func icon(shapes string) *ui.SVG {
	return ui.MustParseSVG([]byte(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="` + iconStroke + `" stroke-linecap="round" stroke-linejoin="round">` + shapes + `</svg>`))
}

// The icon language follows the same light outline vocabulary as Lucide/shadcn
// while remaining self-contained in the native executable.
var (
	iconWorkspace   = icon(`<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M8 5V3h8v2"/>`)
	iconGlobe       = icon(`<circle cx="12" cy="12" r="10"/><path d="M2 12h20"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>`)
	iconFolder      = icon(`<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>`)
	iconFolderPlus  = icon(`<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/><path d="M12 10v6"/><path d="M9 13h6"/>`)
	iconFolderOpen  = icon(`<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>`)
	iconTab         = icon(`<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 9h10"/>`)
	iconTerminal    = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3"/><path d="M13 15h4"/>`)
	iconChat        = icon(`<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>`)
	iconAgent       = icon(`<circle cx="12" cy="8" r="4"/><path d="M5 21a7 7 0 0 1 14 0"/>`)
	iconActivity    = icon(`<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>`)
	iconInfo        = icon(`<circle cx="12" cy="12" r="9"/><path d="M12 11v5"/><path d="M12 8h.01"/>`)
	iconSearch      = icon(`<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>`)
	iconHistory     = icon(`<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l3 2"/>`)
	iconSettings    = icon(`<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6V21h-4v-.1a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H3v-4h.1a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V3h4v.1a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9A1.7 1.7 0 0 0 21 10h.1v4H21a1.7 1.7 0 0 0-1.6 1z"/>`)
	iconPlus        = icon(`<path d="M12 5v14M5 12h14"/>`)
	iconEdit        = icon(`<path d="M17 3a2.85 2.85 0 0 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/><path d="m15 5 4 4"/>`)
	iconServer      = icon(`<rect x="3" y="3" width="18" height="7" rx="2"/><rect x="3" y="14" width="18" height="7" rx="2"/><path d="M7 6.5h.01"/><path d="M7 17.5h.01"/>`)
	iconRefresh     = icon(`<path d="M20 11a8 8 0 0 0-14.8-4L3 10"/><path d="M3 4v6h6"/><path d="M4 13a8 8 0 0 0 14.8 4L21 14"/><path d="M21 20v-6h-6"/>`)
	iconSplit       = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>`)
	iconSplitDown   = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/>`)
	iconZoom        = icon(`<path d="M8 3H3v5M16 3h5v5M8 21H3v-5M16 21h5v-5"/>`)
	iconArrowLeft   = icon(`<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>`)
	iconChevron     = icon(`<path d="m9 18 6-6-6-6"/>`)
	iconChevronDown = icon(`<path d="m6 9 6 6 6-6"/>`)
	iconBranch      = icon(`<circle cx="6" cy="6" r="3"/><path d="M6 9v3a3 3 0 0 0 3 3h6"/><circle cx="18" cy="18" r="3"/><path d="M18 15V9a3 3 0 0 0-3-3"/>`)
	iconFile        = icon(`<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>`)
	iconFilePlus    = icon(`<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/><path d="M12 11v6"/><path d="M9 14h6"/>`)
	iconGit         = icon(`<circle cx="12" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9"/><path d="M12 12v3"/>`)
	iconPin         = icon(`<path d="M12 17v5"/><path d="M9 3h6l1 7 3 3H5l3-3 1-7Z"/>`)
	iconEyeOff      = icon(`<path d="M9.88 9.88a3 3 0 1 0 4.24 4.24"/><path d="M10.73 5.08A10.4 10.4 0 0 1 12 5c7 0 10 7 10 7a13.2 13.2 0 0 1-1.67 2.68"/><path d="M6.61 6.61A13.5 13.5 0 0 0 2 12s3 7 10 7a9.7 9.7 0 0 0 5.39-1.61"/><path d="M2 2l20 20"/>`)
	iconTrash       = icon(`<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>`)
	// iconActions marks every pull-down of secondary actions (title bar, Git
	// toolbar, session detail): sliders, never a row of dots.
	iconActions = icon(`<path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4"/>`)
	// The remote-sync trio of the Git toolbar.
	iconFetch  = icon(`<path d="M12 13v8l-4-4"/><path d="m12 21 4-4"/><path d="M4.393 15.269A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.436 8.284"/>`)
	iconPull   = icon(`<path d="M12 17V3"/><path d="m6 11 6 6 6-6"/><path d="M19 21H5"/>`)
	iconPush   = icon(`<path d="m18 9-6-6-6 6"/><path d="M12 3v14"/><path d="M5 21h14"/>`)
	iconTask   = icon(`<rect x="3" y="3" width="18" height="18" rx="3"/><path d="m8 12 2.5 2.5L16 9"/>`)
	iconPanel  = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>`)
	iconMinus  = icon(`<path d="M5 12h14"/>`)
	iconSquare = icon(`<rect width="18" height="18" x="3" y="3" rx="2"/>`)
	// iconPlay runs the pane header's project command control: the Lucide
	// outline play triangle in the shared light stroke.
	iconPlay = icon(`<polygon points="6 3 20 12 6 21"/>`)
)
