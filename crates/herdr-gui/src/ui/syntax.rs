//! [INPUT]: std-only; no GPUI/theme coupling in the token layer (colors are
//! applied by the caller from the active surface theme).
//! [OUTPUT]: Lang detection (lang_for_path) and the bounded line highlighter
//! (Highlighter::line) producing Vec<SyntaxSpan> per line.
//! [POS]: ui's lightweight preview syntax layer. Custom code by necessity: no
//! highlighting API exists in gpui/gpui-component, and a dependency (syntect)
//! is disproportionate to a read-only preview. Per-line + one carried
//! block-comment flag — deliberately not a full parser.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lang {
    Plain,
    Rust,
    CFamily,
    JsTs,
    Python,
    Go,
    Shell,
    Json,
    Yaml,
    Markdown,
}

pub(crate) fn lang_for_path(name: &str) -> Lang {
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "rs" => Lang::Rust,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "swift" | "java" | "kt" | "kts" => {
            Lang::CFamily
        }
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" | "vue" | "svelte" => {
            Lang::JsTs
        }
        "py" | "pyi" => Lang::Python,
        "go" => Lang::Go,
        "sh" | "bash" | "zsh" => Lang::Shell,
        "json" | "jsonc" | "jsonl" => Lang::Json,
        "yaml" | "yml" | "toml" => Lang::Yaml,
        "md" | "mdx" | "markdown" => Lang::Markdown,
        _ => Lang::Plain,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyntaxKind {
    Plain,
    Keyword,
    String,
    Comment,
    Number,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SyntaxSpan {
    pub(crate) kind: SyntaxKind,
    pub(crate) text: String,
}

/// Carries the only cross-line state the preview honors: block comments
/// (`/* */`) and Markdown code fences. Everything else is per-line.
#[derive(Clone, Debug)]
pub(crate) struct Highlighter {
    lang: Lang,
    in_block_comment: bool,
    in_code_fence: bool,
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn push_plain(spans: &mut Vec<SyntaxSpan>, text: &str) {
    if !text.is_empty() {
        spans.push(SyntaxSpan {
            kind: SyntaxKind::Plain,
            text: text.to_string(),
        });
    }
}

impl Highlighter {
    pub(crate) fn new(lang: Lang) -> Self {
        Self {
            lang,
            in_block_comment: false,
            in_code_fence: false,
        }
    }

    fn keywords(&self) -> &'static [&'static str] {
        match self.lang {
            Lang::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
                "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
                "type", "unsafe", "use", "where", "while",
            ],
            Lang::CFamily | Lang::Go => &[
                "break",
                "case",
                "class",
                "const",
                "continue",
                "default",
                "defer",
                "do",
                "else",
                "enum",
                "extension",
                "for",
                "func",
                "go",
                "if",
                "import",
                "interface",
                "package",
                "private",
                "public",
                "return",
                "static",
                "struct",
                "switch",
                "type",
                "var",
                "void",
                "while",
            ],
            Lang::JsTs => &[
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "delete",
                "do",
                "else",
                "export",
                "extends",
                "finally",
                "for",
                "from",
                "function",
                "if",
                "import",
                "in",
                "instanceof",
                "interface",
                "let",
                "new",
                "of",
                "return",
                "switch",
                "this",
                "throw",
                "try",
                "type",
                "typeof",
                "var",
                "void",
                "while",
                "yield",
            ],
            Lang::Python => &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise",
                "return", "True", "try", "while", "with", "yield",
            ],
            Lang::Shell => &[
                "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in",
                "return", "then", "until", "while",
            ],
            Lang::Json | Lang::Yaml => &["true", "false", "null"],
            Lang::Markdown | Lang::Plain => &[],
        }
    }

    fn line_comment_prefix(&self) -> Option<&'static str> {
        match self.lang {
            Lang::Rust | Lang::CFamily | Lang::JsTs | Lang::Go => Some("//"),
            Lang::Python | Lang::Shell | Lang::Yaml => Some("#"),
            Lang::Json | Lang::Markdown | Lang::Plain => None,
        }
    }

    /// Highlights one line, updating the carried block-comment state.
    pub(crate) fn line(&mut self, line: &str) -> Vec<SyntaxSpan> {
        if self.lang == Lang::Markdown {
            return self.markdown_line(line);
        }
        if self.in_block_comment {
            return match line.find("*/") {
                Some(end) => {
                    self.in_block_comment = false;
                    let mut spans = vec![SyntaxSpan {
                        kind: SyntaxKind::Comment,
                        text: line[..end + 2].to_string(),
                    }];
                    if end + 2 < line.len() {
                        spans.extend(self.line(&line[end + 2..]));
                    }
                    spans
                }
                None => vec![SyntaxSpan {
                    kind: SyntaxKind::Comment,
                    text: line.to_string(),
                }],
            };
        }

        let mut spans: Vec<SyntaxSpan> = Vec::new();
        let mut plain_start = 0usize;
        let mut index = 0usize;
        let bytes = line.as_bytes();
        while index < bytes.len() {
            let rest = &line[index..];
            if let Some(prefix) = self.line_comment_prefix() {
                if rest.starts_with(prefix) {
                    push_plain(&mut spans, &line[plain_start..index]);
                    spans.push(SyntaxSpan {
                        kind: SyntaxKind::Comment,
                        text: rest.to_string(),
                    });
                    return spans;
                }
            }
            if rest.starts_with("/*") && matches!(self.lang, Lang::Rust | Lang::CFamily) {
                push_plain(&mut spans, &line[plain_start..index]);
                match line[index..].find("*/") {
                    Some(offset) => {
                        let end = index + offset + 2;
                        spans.push(SyntaxSpan {
                            kind: SyntaxKind::Comment,
                            text: line[index..end].to_string(),
                        });
                        index = end;
                        plain_start = end;
                    }
                    None => {
                        self.in_block_comment = true;
                        spans.push(SyntaxSpan {
                            kind: SyntaxKind::Comment,
                            text: line[index..].to_string(),
                        });
                        return spans;
                    }
                }
                continue;
            }
            let first = bytes[index] as char;
            if first == '"' || first == '\'' {
                let quote = bytes[index];
                let mut end = index + 1;
                while end < bytes.len() {
                    if bytes[end] == b'\\' {
                        end += 2;
                        continue;
                    }
                    if bytes[end] == quote {
                        end += 1;
                        break;
                    }
                    end += 1;
                }
                let end = end.min(bytes.len());
                push_plain(&mut spans, &line[plain_start..index]);
                spans.push(SyntaxSpan {
                    kind: SyntaxKind::String,
                    text: line[index..end].to_string(),
                });
                index = end;
                plain_start = end;
                continue;
            }
            let word_start = first.is_alphabetic() || first == '_';
            let digit_start =
                first.is_ascii_digit() && (index == 0 || !is_word_byte(bytes[index - 1]));
            if word_start || digit_start {
                let mut end = index;
                while end < bytes.len() && (is_word_byte(bytes[end]) || bytes[end] == b'.') {
                    end += 1;
                }
                let token = &line[index..end];
                let kind = if digit_start {
                    SyntaxKind::Number
                } else if self.keywords().contains(&token) {
                    SyntaxKind::Keyword
                } else {
                    index = end;
                    continue;
                };
                push_plain(&mut spans, &line[plain_start..index]);
                spans.push(SyntaxSpan {
                    kind,
                    text: token.to_string(),
                });
                index = end;
                plain_start = end;
                continue;
            }
            index += 1;
        }
        spans.push(SyntaxSpan {
            kind: SyntaxKind::Plain,
            text: line[plain_start..].to_string(),
        });
        spans
    }

    fn markdown_line(&mut self, line: &str) -> Vec<SyntaxSpan> {
        if line.starts_with("```") {
            self.in_code_fence = !self.in_code_fence;
            return vec![SyntaxSpan {
                kind: SyntaxKind::Keyword,
                text: line.to_string(),
            }];
        }
        if self.in_code_fence {
            return vec![SyntaxSpan {
                kind: SyntaxKind::String,
                text: line.to_string(),
            }];
        }
        if line.starts_with('#') {
            return vec![SyntaxSpan {
                kind: SyntaxKind::Keyword,
                text: line.to_string(),
            }];
        }
        vec![SyntaxSpan {
            kind: SyntaxKind::Plain,
            text: line.to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_line_separates_keywords_strings_and_comments() {
        let mut highlighter = Highlighter::new(Lang::Rust);
        let spans = highlighter.line("let s = \"hi\"; // tail");
        assert_eq!(spans[0].kind, SyntaxKind::Keyword);
        assert_eq!(spans[0].text, "let");
        assert!(spans
            .iter()
            .any(|span| span.kind == SyntaxKind::String && span.text == "\"hi\""));
        assert!(spans
            .iter()
            .any(|span| span.kind == SyntaxKind::Comment && span.text == "// tail"));
    }

    #[test]
    fn block_comment_state_carries_across_lines() {
        let mut highlighter = Highlighter::new(Lang::Rust);
        let first = highlighter.line("/* starts");
        assert_eq!(first[0].kind, SyntaxKind::Comment);
        let second = highlighter.line("still inside */ fn after()");
        assert_eq!(second[0].kind, SyntaxKind::Comment);
        assert!(second
            .iter()
            .any(|span| span.kind == SyntaxKind::Keyword && span.text == "fn"));
    }

    #[test]
    fn lang_detection_covers_common_extensions() {
        assert_eq!(lang_for_path("main.rs"), Lang::Rust);
        assert_eq!(lang_for_path("app.tsx"), Lang::JsTs);
        assert_eq!(lang_for_path("README.md"), Lang::Markdown);
        assert_eq!(lang_for_path("data.unknown"), Lang::Plain);
    }
}
