package nativeui

import "github.com/egoist/mygo/ui"

// Design System v2 metric tokens (DS-01). Pages choose semantics; the token
// set owns every common font size, spacing step, radius and control height.
type TypographyTokens struct {
	Title     float32 // page title
	Section   float32 // card/section title
	Body      float32 // primary reading text
	BodySmall float32 // secondary reading text
	Caption   float32 // metadata line
	Micro     float32 // badges, footnotes
}

type SpaceTokens struct {
	XXS float32
	XS  float32
	S   float32
	M   float32
	L   float32
	XL  float32
}

type RadiusTokens struct {
	Control float32
	Row     float32
	Card    float32
	Pill    float32
}

type ControlTokens struct {
	CompactHeight float32
	RegularHeight float32
	RowHeight     float32
	IconSmall     float32
	IconRegular   float32
	// PaletteField is the command palette's query field height: the
	// palette's primary control, a step above a regular field.
	PaletteField float32
}

// Typography returns the shared type scale.
func Typography() TypographyTokens {
	return TypographyTokens{
		Title:     18,
		Section:   13,
		Body:      11.5,
		BodySmall: 10.5,
		Caption:   10,
		Micro:     9,
	}
}

// Spacing returns the shared spacing scale.
func Spacing() SpaceTokens {
	return SpaceTokens{XXS: 2, XS: 4, S: 6, M: 8, L: 12, XL: 20}
}

// Radius returns the shared corner-radius scale.
func Radius() RadiusTokens {
	return RadiusTokens{Control: 6, Row: 8, Card: 10, Pill: 999}
}

// Controls returns the shared control-size scale.
func Controls() ControlTokens {
	return ControlTokens{
		CompactHeight: 24,
		RegularHeight: 28,
		RowHeight:     31,
		IconSmall:     11,
		IconRegular:   14,
		PaletteField:  38,
	}
}

// StatusTone is the semantic status vocabulary (DS-02). Surfaces derive a
// tone from domain facts; colors come only from StatusColor.
type StatusTone uint8

const (
	ToneNeutral StatusTone = iota
	ToneInfo
	ToneWorking
	ToneSuccess
	ToneWarning
	ToneAttention
	ToneError
	ToneMuted
)

// StatusColor maps a semantic tone onto the active theme. It is the single
// color authority for status presentation.
func (t DesignTokens) StatusColor(tone StatusTone, dark bool) ui.Color {
	theme := shardlaneTheme(dark)
	switch tone {
	case ToneInfo, ToneWorking:
		return theme.Accent
	case ToneSuccess:
		return theme.Success
	case ToneWarning:
		return theme.Warning
	case ToneAttention:
		if dark {
			return ui.Hex("#ff9f0a")
		}
		return ui.Hex("#c93400")
	case ToneError:
		return theme.Danger
	case ToneMuted:
		return theme.TextMuted
	default:
		return theme.Text
	}
}

// StatusBackground is the soft surface wash behind notices and pills.
func (t DesignTokens) StatusBackground(tone StatusTone, dark bool) ui.Color {
	color := t.StatusColor(tone, dark)
	return color.Alpha(0.13)
}
