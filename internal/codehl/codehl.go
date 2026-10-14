// Code tokenizing for the diff review, ported from egoist/godiff
// internal/highlight (commit 88b89e0, 2026-10-05) under the product decision
// to adopt Godiff's review presentation; since 2026-10-14 the lexing is
// MyGo's own (ui/editor tokenizers, the ones the Files code view paints
// with) instead of Chroma's, so one engine colors code everywhere.
// Package codehl colors source code by its tokens, in the palettes of
// codiff's Licht and Dunkel themes.
package codehl

import (
	"github.com/egoist/mygo/ui/editor"
)

// Class is the kind of a token, which picks its color.
type Class uint8

const (
	Plain Class = iota
	Comment
	Preproc
	Keyword
	Type
	LangConst
	Function
	Exception
	Number
	String
	Escape
	Regexp
	Tag
	Attribute
	Property
	Heading
	Inserted
	Deleted
	NumClasses
)

// Seg is a run of a line's bytes of one class.
type Seg struct {
	Start, End int32
	Class      Class
}

// MaxBytes bounds the files worth highlighting.
const MaxBytes = 1 << 20

// KindClasses is the one table pairing the review's color classes with the
// editor's token kinds. Lines colors a diff's lines through it and the Files
// code view dresses the editor's theme with it, so the same code reads in
// the same colors on both surfaces. Kinds the review has no class for
// (emphasis, strong) read as plain code.
var KindClasses = []struct {
	Class Class
	Kinds []editor.TokenKind
}{
	{Plain, []editor.TokenKind{editor.TokenText, editor.TokenVariable, editor.TokenOperator, editor.TokenPunctuation}},
	{Comment, []editor.TokenKind{editor.TokenComment}},
	{Preproc, []editor.TokenKind{editor.TokenMeta}},
	{Keyword, []editor.TokenKind{editor.TokenKeyword}},
	{Type, []editor.TokenKind{editor.TokenType}},
	{LangConst, []editor.TokenKind{editor.TokenConstant}},
	{Function, []editor.TokenKind{editor.TokenFunction}},
	{Exception, []editor.TokenKind{editor.TokenInvalid}},
	{Number, []editor.TokenKind{editor.TokenNumber}},
	{String, []editor.TokenKind{editor.TokenString, editor.TokenCode}},
	{Escape, []editor.TokenKind{editor.TokenEscape}},
	{Regexp, []editor.TokenKind{editor.TokenRegexp}},
	{Tag, []editor.TokenKind{editor.TokenTag, editor.TokenLink}},
	{Attribute, []editor.TokenKind{editor.TokenAttribute}},
	{Property, []editor.TokenKind{editor.TokenProperty}},
	{Heading, []editor.TokenKind{editor.TokenHeading}},
	{Inserted, []editor.TokenKind{editor.TokenInserted}},
	{Deleted, []editor.TokenKind{editor.TokenDeleted}},
}

var classOfKind = func() map[editor.TokenKind]Class {
	m := map[editor.TokenKind]Class{}
	for _, group := range KindClasses {
		for _, kind := range group.Kinds {
			m[kind] = group.Class
		}
	}
	return m
}()

// ClassOf returns the class a token kind is colored as, Plain for the kinds
// the review does not color.
func ClassOf(kind editor.TokenKind) Class { return classOfKind[kind] }

// Lines tokenizes lines of the file named name, and returns the segments of
// each line, nil for plain text: a file of a language MyGo has no tokenizer
// for, or one past MaxBytes. It is safe to call from several goroutines.
func Lines(name string, lines []string) [][]Seg {
	lang := editor.LanguageFor(name)
	if lang == nil || lang.Tokenizer == nil || len(lines) == 0 {
		return nil
	}
	size := 0
	for _, l := range lines {
		size += len(l) + 1
	}
	if size > MaxBytes {
		return nil
	}
	out := make([][]Seg, len(lines))
	var (
		tokens []editor.Token
		state  uint64
	)
	for row, line := range lines {
		tokens, state = lang.Tokenizer.Tokenize(line, state, tokens[:0])
		start := 0
		for _, tok := range tokens {
			end := min(tok.End, len(line))
			if class := ClassOf(tok.Kind); class != Plain && end > start {
				segs := out[row]
				if n := len(segs); n > 0 && segs[n-1].Class == class && int(segs[n-1].End) == start {
					segs[n-1].End = int32(end)
				} else {
					segs = append(segs, Seg{Start: int32(start), End: int32(end), Class: class})
				}
				out[row] = segs
			}
			start = max(start, end)
		}
	}
	return out
}
