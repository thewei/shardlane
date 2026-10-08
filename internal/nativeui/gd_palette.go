package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/codehl"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// The diff review's colors, ported from egoist/godiff palette.go (commit
// 88b89e0) so Shardlane's Changes/Diff surface reads exactly like Godiff.

// gdPalette is the colors of the review, in the light or the dark: those
// of codiff and of the diff renderer it uses.
type gdPalette struct {
	appBg      ui.Color // behind the cards: the gorex card's own paper, so the pinned-header bands continue it seamlessly
	code       ui.Color // the code's own color
	lineNumber ui.Color
	codeBg     ui.Color // the background of the cards
	addBg      ui.Color
	addGutter  ui.Color
	addWord    ui.Color
	addBar     ui.Color
	addText    ui.Color // +N counts
	delBg      ui.Color
	delGutter  ui.Color
	delWord    ui.Color
	delBar     ui.Color
	delText    ui.Color
	emptySide  ui.Color // the side of a split row without a line
	gapBg      ui.Color // "N unmodified lines"
	gapText    ui.Color
	cardBorder ui.Color
	headerBg   ui.Color
	pill       ui.Color // the background of counts and badges
	hover      ui.Color // what is under the pointer
	match      ui.Color // a match of the find bar
	matchNow   ui.Color // the current match
	selected   ui.Color // lines chosen with j and k
	viewed     ui.Color
	ref        ui.Color // commit hashes and generated files
	muted      ui.Color
	syntax     [codehl.NumClasses]ui.Color
}

var (
	gdLightPalette = gdPalette{
		appBg:      ui.RGBA(255, 255, 255, 0.95), // gorexStyle cardFocused (light)
		code:       ui.Hex("#111111"),
		lineNumber: ui.Hex("#9a9a9a"),
		codeBg:     ui.Hex("#ffffff"),
		addBg:      ui.Hex("#e2f7ea"),
		addGutter:  ui.Hex("#d3f3df"),
		addWord:    ui.RGBA(13, 190, 78, 0.20),
		addBar:     ui.Hex("#0dbe4e"),
		addText:    ui.RGB(31, 122, 68),
		delBg:      ui.Hex("#ffe6e8"),
		delGutter:  ui.Hex("#ffd9dc"),
		delWord:    ui.RGBA(255, 46, 63, 0.17),
		delBar:     ui.Hex("#ff2e3f"),
		delText:    ui.RGB(184, 49, 47),
		emptySide:  ui.Hex("#fafafa"),
		gapBg:      ui.Hex("#f4f4f4"),
		gapText:    ui.Hex("#8a8a8a"),
		cardBorder: ui.RGBA(17, 17, 17, 0.10),
		headerBg:   ui.Hex("#ffffff"),
		pill:       ui.RGBA(127, 127, 127, 0.11),
		hover:      ui.RGBA(127, 127, 127, 0.11),
		match:      ui.RGBA(255, 216, 92, 0.65),
		matchNow:   ui.RGBA(255, 176, 46, 0.96),
		selected:   ui.RGBA(61, 135, 245, 0.13),
		viewed:     ui.RGB(31, 122, 68),
		ref:        ui.Hex("#c56e0e"),
		muted:      ui.RGBA(17, 17, 17, 0.48),
	}
	gdDarkPalette = gdPalette{
		appBg:      ui.RGBA(36, 36, 41, 0.94), // gorexStyle cardFocused (dark)
		code:       ui.Hex("#c8c8c8"),
		lineNumber: ui.Hex("#6c6c6c"),
		codeBg:     ui.Hex("#1c1c1c"),
		addBg:      ui.Hex("#22382a"),
		addGutter:  ui.Hex("#2a4632"),
		addWord:    ui.RGBA(94, 204, 113, 0.22),
		addBar:     ui.Hex("#5ecc71"),
		addText:    ui.RGB(111, 208, 148),
		delBg:      ui.Hex("#40262a"),
		delGutter:  ui.Hex("#52302f"),
		delWord:    ui.RGBA(255, 103, 98, 0.24),
		delBar:     ui.Hex("#ff6762"),
		delText:    ui.RGB(238, 126, 126),
		emptySide:  ui.Hex("#202020"),
		gapBg:      ui.Hex("#262626"),
		gapText:    ui.Hex("#8a8a8a"),
		cardBorder: ui.RGBA(230, 230, 230, 0.10),
		headerBg:   ui.Hex("#232323"),
		pill:       ui.RGBA(127, 127, 127, 0.16),
		hover:      ui.RGBA(127, 127, 127, 0.16),
		match:      ui.RGBA(255, 216, 92, 0.38),
		matchNow:   ui.RGBA(255, 176, 46, 0.80),
		selected:   ui.RGBA(90, 150, 255, 0.12),
		viewed:     ui.RGB(111, 208, 148),
		ref:        ui.Hex("#eb9a3d"),
		muted:      ui.RGBA(230, 230, 230, 0.48),
	}
)

func init() {
	// Licht and Dunkel, codiff's themes.
	light := map[codehl.Class]string{
		codehl.Comment: "#919191", codehl.Preproc: "#adadad", codehl.Keyword: "#352de3",
		codehl.Type: "#c56e0e", codehl.LangConst: "#626fc9", codehl.Function: "#284181",
		codehl.ClassName: "#bb28c7", codehl.Exception: "#f93232", codehl.Number: "#dd3c2f",
		codehl.String: "#00a33f", codehl.Escape: "#00a33f", codehl.Regexp: "#699d36",
		codehl.Tag: "#0072c8", codehl.Attribute: "#0072c8", codehl.Property: "#444444",
		codehl.Heading: "#111111", codehl.Inserted: "#1a7f37", codehl.Deleted: "#c4232e",
	}
	dark := map[codehl.Class]string{
		codehl.Comment: "#919191", codehl.Preproc: "#adadad", codehl.Keyword: "#6a93cf",
		codehl.Type: "#eb9a3d", codehl.LangConst: "#7b8cfd", codehl.Function: "#7b8cfd",
		codehl.ClassName: "#f06efb", codehl.Exception: "#f93232", codehl.Number: "#db584d",
		codehl.String: "#52ce81", codehl.Escape: "#52ce81", codehl.Regexp: "#699d36",
		codehl.Tag: "#6a93cf", codehl.Attribute: "#6a93cf", codehl.Property: "#c8c8c8",
		codehl.Heading: "#e8e8e8", codehl.Inserted: "#56d364", codehl.Deleted: "#ff7b72",
	}
	gdLightPalette.syntax[codehl.Plain] = gdLightPalette.code
	gdDarkPalette.syntax[codehl.Plain] = gdDarkPalette.code
	for k, v := range light {
		gdLightPalette.syntax[k] = ui.Hex(v)
	}
	for k, v := range dark {
		gdDarkPalette.syntax[k] = ui.Hex(v)
	}
}

func gdPaletteFor(t *ui.Theme) *gdPalette {
	if t.Dark {
		return &gdDarkPalette
	}
	return &gdLightPalette
}

// gdCardRadius rounds the corners of the files' cards.
const gdCardRadius = 10

// gdStatusLetterColor is the color of a file's status letter in the tree.
func gdStatusLetterColor(status gitworkbench.ChangeStatus, pal *gdPalette, t *ui.Theme) ui.Color {
	switch status {
	case gitworkbench.StatusAdded, gitworkbench.StatusUntracked:
		return pal.addBar
	case gitworkbench.StatusDeleted:
		return pal.delBar
	case gitworkbench.StatusRenamed, gitworkbench.StatusCopied:
		return t.Accent
	case gitworkbench.StatusConflicted:
		return t.Danger
	}
	return ui.Hex("#d4a72c")
}
