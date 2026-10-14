package nativeui

/**
 * [INPUT]: gdPalette 语法色与代码/行号色、框架 editor.DefaultTheme
 * [OUTPUT]: codeEditorTheme：把 Git 评审的语法配色穿到框架 editor 上
 * [POS]: 同一份代码在 Files 与 Git 评审里同色——语法色的单一来源仍是 gd_palette；选区/查找/缩进线等结构色沿用框架默认
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/wh-studio/herdr-client/internal/codehl"
)

// codeEditorTheme dresses the framework's code editor in the review's
// palette: its code color, line numbers and syntax classes. Kinds the
// review has no class for (variables, operators, punctuation) read as
// plain code, as in the review; links and inline code borrow the tag and
// string colors.
func codeEditorTheme(t *ui.Theme) *editor.Theme {
	th := editor.DefaultTheme(t)
	p := gdPaletteFor(t)
	th.Text = p.code
	th.LineNumber = p.lineNumber
	set := func(class codehl.Class, kinds ...editor.TokenKind) {
		for _, kind := range kinds {
			th.Tokens[kind].Color = p.syntax[class]
		}
	}
	set(codehl.Plain, editor.TokenText, editor.TokenVariable, editor.TokenOperator, editor.TokenPunctuation)
	set(codehl.Comment, editor.TokenComment)
	set(codehl.Preproc, editor.TokenMeta)
	set(codehl.Keyword, editor.TokenKeyword)
	set(codehl.Type, editor.TokenType)
	set(codehl.LangConst, editor.TokenConstant)
	set(codehl.Function, editor.TokenFunction)
	set(codehl.Exception, editor.TokenInvalid)
	set(codehl.Number, editor.TokenNumber)
	set(codehl.String, editor.TokenString, editor.TokenCode)
	set(codehl.Escape, editor.TokenEscape)
	set(codehl.Regexp, editor.TokenRegexp)
	set(codehl.Tag, editor.TokenTag, editor.TokenLink)
	set(codehl.Attribute, editor.TokenAttribute)
	set(codehl.Property, editor.TokenProperty)
	set(codehl.Heading, editor.TokenHeading)
	set(codehl.Inserted, editor.TokenInserted)
	set(codehl.Deleted, editor.TokenDeleted)
	return th
}
