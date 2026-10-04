package nativeui

// Icons the Godiff-style review needs that the shell's shared set does not
// already define (icons.go). Same light outline vocabulary.
var (
	iconCopy      = icon(`<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>`)
	iconOpen      = icon(`<path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6"/><path d="m21 3-9 9"/><path d="M15 3h6v6"/>`)
	iconClose     = icon(`<path d="M18 6 6 18"/><path d="m6 6 12 12"/>`)
	iconCheck     = icon(`<path d="M20 6 9 17l-5-5"/>`)
	iconUnified   = icon(`<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/>`)
	iconCommit    = icon(`<circle cx="12" cy="12" r="3"/><line x1="3" x2="9" y1="12" y2="12"/><line x1="15" x2="21" y1="12" y2="12"/>`)
	iconFileDiff  = icon(`<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M9 10h6"/><path d="M12 13V7"/><path d="M9 17h6"/>`)
	iconExpand    = icon(`<path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/>`)
	iconArrowUp   = icon(`<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>`)
	iconArrowDown = icon(`<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>`)
	iconEye       = icon(`<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>`)
	iconTag       = icon(`<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/>`)
)
