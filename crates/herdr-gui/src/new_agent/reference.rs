//! Composer reference core: @ file / slash-command dual triggers, fuzzy
//! scoring, row ranking, trigger replacement, and submission dialect encoding
//! (the same composer-autocomplete semantics as before, ported to Rust).
//!
//! [INPUT]: shardlane_history's AgentId (dialect judgment); std pure logic with zero UI coupling.
//! [OUTPUT]: Provides ReferenceTrigger/detect_reference_trigger, fuzzy_score,
//! rank_reference_rows, ComposerCommand/CommandScope, merge_commands,
//! expand_command_submission, expand_command_template, byte_to_utf16_position.
//! [POS]: The reference pure-function layer of new_agent; consumed jointly by reference_index.rs
//! (the catalog) and the provider (popup mounting); unit tests pin the original semantics
//! plus this repo's dialect differences.

use shardlane_history::AgentId;

/// Popup row cap (same value as the original COMPOSER_AUTOCOMPLETE_CAP).
pub(super) const REFERENCE_ROW_CAP: usize = 64;

/// Trigger kinds: line-leading `/` commands, token-leading `@` files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReferenceKind {
    Command,
    File,
}

/// The active trigger at the cursor. `start`/`end` are byte offsets (the
/// replacement range).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReferenceTrigger {
    pub(super) kind: ReferenceKind,
    pub(super) query: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// Detect @ file / line-leading command triggers at the cursor (same semantics
/// as the original detectComposerTrigger).
///
/// - `/`: the cursor's line prefix starts with `/` and the query segment has no
///   whitespace → Command;
/// - `@`: the nearest non-whitespace token before the cursor starts with `@` →
///   File;
/// - otherwise None. `cursor` must land on a char boundary (callers come from
///   Rope offsets).
pub(super) fn detect_reference_trigger(text: &str, cursor: usize) -> Option<ReferenceTrigger> {
    let end = cursor.min(text.len());
    if !text.is_char_boundary(end) {
        return None;
    }
    let before = &text[..end];
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_prefix = &text[line_start..end];
    if let Some(query) = line_prefix.strip_prefix('/') {
        if query.chars().any(char::is_whitespace) {
            return None;
        }
        return Some(ReferenceTrigger {
            kind: ReferenceKind::Command,
            query: query.to_string(),
            start: line_start,
            end,
        });
    }

    let token_start = before
        .char_indices()
        .rev()
        .find_map(|(i, ch)| ch.is_whitespace().then_some(i + ch.len_utf8()))
        .unwrap_or(0);
    let token = &text[token_start..end];
    if let Some(query) = token.strip_prefix('@') {
        return Some(ReferenceTrigger {
            kind: ReferenceKind::File,
            query: query.to_string(),
            start: token_start,
            end,
        });
    }
    None
}

/// Fuzzy scoring (same semantics as the original palette-search fuzzyScore):
/// contiguous hits take absolute priority (earlier and shorter is better);
/// otherwise subsequence scoring — contiguous +200 / scattered +40, word
/// boundaries (whitespace and `/_.#-`) +100, earlier positions get a
/// `50-index` bonus. Returns None on no match.
pub(super) fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let needle = query.trim().to_lowercase();
    let haystack = candidate.to_lowercase();
    if needle.is_empty() {
        return Some(0);
    }
    if let Some(contiguous) = haystack.find(&needle) {
        return Some(100_000 + needle.len() as i64 * 1_000 - contiguous as i64);
    }
    let mut score: i64 = 0;
    let mut next_needle = 0;
    let needle_chars: Vec<char> = needle.chars().collect();
    let haystack_chars: Vec<char> = haystack.chars().collect();
    let mut previous_match: i64 = -2;
    for (index, ch) in haystack_chars.iter().enumerate() {
        if next_needle >= needle_chars.len() {
            break;
        }
        if *ch != needle_chars[next_needle] {
            continue;
        }
        score += if index as i64 == previous_match + 1 {
            200
        } else {
            40
        };
        if index == 0 {
            score += 100;
        } else {
            let prev = haystack_chars[index - 1];
            if matches!(prev, ' ' | '/' | '_' | '.' | '#' | '-') {
                score += 100;
            }
        }
        score += (50 - index as i64).max(0);
        previous_match = index as i64;
        next_needle += 1;
    }
    (next_needle == needle_chars.len()).then_some(score)
}

/// Reference popup row: a file path or a command. `insert` is the full text
/// replacing the trigger range upon acceptance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReferenceRow {
    pub(super) kind: ReferenceKind,
    pub(super) label: String,
    pub(super) detail: Option<String>,
    pub(super) insert: String,
}

/// Sort by fuzzy score and truncate (stable: ties keep their original order,
/// same semantics as before).
pub(super) fn rank_reference_rows(query: &str, candidates: Vec<ReferenceRow>) -> Vec<ReferenceRow> {
    let mut scored: Vec<(Option<i64>, usize, ReferenceRow)> = candidates
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let key = match row.kind {
                ReferenceKind::Command => row.label.trim_start_matches('/'),
                ReferenceKind::File => row.label.as_str(),
            };
            (fuzzy_score(query, key), index, row)
        })
        .collect();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored
        .into_iter()
        .take(REFERENCE_ROW_CAP)
        .map(|(_, _, row)| row)
        .collect()
}

/// Command source hierarchy (same ordering as the original CommandScope:
/// Project < User < Skill; Builtin is owned natively by the CLI and stays out
/// of the client catalog).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CommandScope {
    Project,
    User,
    Skill,
}

impl CommandScope {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::User => "User",
            Self::Skill => "Skill",
        }
    }
}

/// Slash-command/skill entry visible to the composer (mirrors the original
/// SlashCommand shape).
/// `template = Some(_)` means a client-expanded prompt template; None means
/// pass-through to the CLI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComposerCommand {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) scope: CommandScope,
    pub(super) argument_hint: Option<String>,
    pub(super) template: Option<String>,
}

/// Merge ordering: ascending scope (Builtin→Project→User→Skill), then name
/// lexicographic order.
pub(super) fn merge_commands(mut commands: Vec<ComposerCommand>) -> Vec<ComposerCommand> {
    commands.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then_with(|| left.name.cmp(&right.name))
    });
    commands
}

/// Expand the command template's `$ARGUMENTS` / `$@` / `$1..$9` placeholders;
/// with no placeholders the arguments are appended wholesale at the end (same
/// semantics as the original expandCommandTemplate).
pub(super) fn expand_command_template(template: &str, args: &str) -> String {
    let positional: Vec<&str> = args.split_whitespace().filter(|s| !s.is_empty()).collect();
    let mut expanded = String::new();
    let mut consumed_args = false;
    let mut rest = template;
    while let Some(index) = rest.find('$') {
        expanded.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            expanded.push_str(args);
            consumed_args = true;
            rest = tail;
        } else if let Some(tail) = after.strip_prefix('@') {
            expanded.push_str(args);
            consumed_args = true;
            rest = tail;
        } else if after
            .chars()
            .next()
            .is_some_and(|ch| ('1'..='9').contains(&ch))
        {
            let digit = after.chars().next().unwrap_or('1') as usize - '0' as usize;
            if let Some(value) = positional.get(digit - 1) {
                expanded.push_str(value);
            }
            consumed_args = true;
            rest = &after[1..];
        } else {
            expanded.push('$');
            rest = after;
        }
    }
    expanded.push_str(rest);
    if !consumed_args && !args.is_empty() {
        expanded.push_str("\n\n");
        expanded.push_str(args);
    }
    expanded
}

/// Dialect encoding at submission time (this repo's mapping of the original
/// expandedComposerSubmission):
///
/// - Skill scope: Codex → `$name args`; Pi/Omp → `/skill:name args`;
///   other agents pass through `/name args` (parsed natively by the CLI);
/// - Project/User commands with a template: expand the template client-side;
/// - the rest (Builtin / no template) returns None and passes through as-is.
pub(super) fn expand_command_submission(
    agent: AgentId,
    prompt: &str,
    commands: &[ComposerCommand],
) -> Option<String> {
    let trimmed = prompt.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let invocation = &trimmed[1..];
    let whitespace = invocation.find(char::is_whitespace);
    let (name, args) = match whitespace {
        Some(index) => (&invocation[..index], invocation[index..].trim()),
        None => (invocation, ""),
    };
    let command = commands.iter().find(|item| item.name == name)?;
    if command.scope == CommandScope::Skill {
        let encoded = match agent {
            AgentId::Codex => format!("${invocation}"),
            AgentId::Pi | AgentId::Omp => format!("/skill:{invocation}"),
            _ => return None,
        };
        return Some(encoded);
    }
    command
        .template
        .as_ref()
        .map(|template| expand_command_template(template, args))
}

/// Byte offset → LSP Position (line and character are both UTF-16 code unit
/// counts), for the component completion menu's TextEdit range.
pub(super) fn byte_to_utf16_position(text: &str, byte_offset: usize) -> (u32, u32) {
    let end = byte_offset.min(text.len());
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    for (index, ch) in text.char_indices() {
        if index >= end {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    (line, character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_trigger_at_token_start() {
        let trigger = detect_reference_trigger("fix @ca", "fix @ca".len());
        assert_eq!(
            trigger,
            Some(ReferenceTrigger {
                kind: ReferenceKind::File,
                query: "ca".into(),
                start: 4,
                end: 7,
            })
        );
    }

    #[test]
    fn file_trigger_works_at_text_start_and_mid_text() {
        let trigger = detect_reference_trigger("@src", 4);
        assert_eq!(
            trigger.map(|t| (t.kind, t.start, t.query.clone())),
            Some((ReferenceKind::File, 0, "src".into()))
        );
        let trigger = detect_reference_trigger("see @a/b.rs here", 11);
        assert_eq!(trigger.map(|t| t.query), Some("a/b.rs".into()));
    }

    #[test]
    fn command_trigger_requires_line_start_slash() {
        let trigger = detect_reference_trigger("/rev", 4);
        assert_eq!(
            trigger,
            Some(ReferenceTrigger {
                kind: ReferenceKind::Command,
                query: "rev".into(),
                start: 0,
                end: 4,
            })
        );
        // A / mid-line (not at line start) does not trigger.
        assert!(detect_reference_trigger("run /rev", 8).is_none());
    }

    #[test]
    fn command_trigger_ends_at_whitespace() {
        assert!(detect_reference_trigger("/rev now", 8).is_none());
        // A / at the start of the second line triggers.
        let trigger = detect_reference_trigger("line\n/rev", 9);
        assert_eq!(trigger.map(|t| (t.start, t.query)), Some((5, "rev".into())));
    }

    #[test]
    fn plain_text_has_no_trigger() {
        assert!(detect_reference_trigger("plain prose", 12).is_none());
        assert!(detect_reference_trigger("", 0).is_none());
        // A bare @ followed by whitespace: the token is an empty @, the query is
        // empty but still triggers (listing everything).
        let trigger = detect_reference_trigger("@", 1);
        assert_eq!(
            trigger.map(|t| (t.kind, t.query)),
            Some((ReferenceKind::File, "".into()))
        );
    }

    #[test]
    fn unicode_cursor_boundaries_are_safe() {
        let text = "你好 @wor";
        // The cursor lands in the middle of a multibyte character → safely
        // returns None.
        assert!(detect_reference_trigger(text, 3).is_none());
        let trigger = detect_reference_trigger(text, text.len());
        assert_eq!(trigger.map(|t| t.query), Some("wor".into()));
    }

    #[test]
    fn fuzzy_prefers_contiguous_early_matches() {
        let contiguous =
            fuzzy_score("mod", "models.rs").unwrap_or_else(|| panic!("fuzzy must hit"));
        let late =
            fuzzy_score("mod", "src/README-mod.rs").unwrap_or_else(|| panic!("fuzzy must hit"));
        assert!(contiguous > late);
        assert!(fuzzy_score("xyz", "Cargo.toml").is_none());
        assert_eq!(
            fuzzy_score("", "anything").unwrap_or_else(|| panic!("test value")),
            0
        );
    }

    #[test]
    fn ranking_is_stable_and_capped() {
        let rows: Vec<ReferenceRow> = ["b_alpha.rs", "a_alpha.rs", "c_zeta.rs"]
            .iter()
            .map(|label| ReferenceRow {
                kind: ReferenceKind::File,
                label: label.to_string(),
                detail: None,
                insert: format!("@{label} "),
            })
            .collect();
        let ranked = rank_reference_rows("alpha", rows.clone());
        // Tie stability: keep the original order (b_alpha before a_alpha).
        assert_eq!(ranked[0].label, "b_alpha.rs");
        assert_eq!(ranked[1].label, "a_alpha.rs");
        assert_eq!(ranked[2].label, "c_zeta.rs");
        let many: Vec<ReferenceRow> = (0..100)
            .map(|i| ReferenceRow {
                kind: ReferenceKind::File,
                label: format!("f{i}.rs"),
                detail: None,
                insert: String::new(),
            })
            .collect();
        assert_eq!(rank_reference_rows("", many).len(), REFERENCE_ROW_CAP);
    }

    #[test]
    fn command_rows_rank_against_name_without_slash() {
        let rows = vec![
            ReferenceRow {
                kind: ReferenceKind::Command,
                label: "/review".into(),
                detail: None,
                insert: "/review ".into(),
            },
            ReferenceRow {
                kind: ReferenceKind::Command,
                label: "/fast".into(),
                detail: None,
                insert: "/fast ".into(),
            },
        ];
        let ranked = rank_reference_rows("rev", rows);
        assert_eq!(
            ranked.first().map(|r| r.label.clone()),
            Some("/review".into())
        );
    }

    fn skill(name: &str) -> ComposerCommand {
        ComposerCommand {
            name: name.into(),
            description: String::new(),
            scope: CommandScope::Skill,
            argument_hint: None,
            template: None,
        }
    }

    #[test]
    fn skill_submission_dialects_per_agent() {
        let commands = vec![skill("review")];
        assert_eq!(
            expand_command_submission(AgentId::Codex, "/review deep", &commands),
            Some("$review deep".into())
        );
        assert_eq!(
            expand_command_submission(AgentId::Pi, "/review deep", &commands),
            Some("/skill:review deep".into())
        );
        assert_eq!(
            expand_command_submission(AgentId::Omp, "/review", &commands),
            Some("/skill:review".into())
        );
        // Other agents pass through (None = no rewrite).
        assert_eq!(
            expand_command_submission(AgentId::ClaudeCode, "/review deep", &commands),
            None
        );
    }

    #[test]
    fn template_commands_expand_client_side() {
        let command = ComposerCommand {
            name: "fix".into(),
            description: String::new(),
            scope: CommandScope::Project,
            argument_hint: None,
            template: Some("Fix $ARGUMENTS in src".into()),
        };
        assert_eq!(
            expand_command_submission(
                AgentId::ClaudeCode,
                "/fix the parser",
                std::slice::from_ref(&command)
            ),
            Some("Fix the parser in src".into())
        );
        let positional = ComposerCommand {
            name: "mv".into(),
            description: String::new(),
            scope: CommandScope::User,
            argument_hint: None,
            template: Some("git mv $1 $2".into()),
        };
        assert_eq!(
            expand_command_submission(AgentId::Codex, "/mv a.rs b.rs", &[positional]),
            Some("git mv a.rs b.rs".into())
        );
    }

    #[test]
    fn builtin_and_unknown_commands_pass_through() {
        // Commands without a template (CLI-native slashes, like claude's /compact)
        // → no rewrite.
        let native = ComposerCommand {
            name: "compact".into(),
            description: String::new(),
            scope: CommandScope::User,
            argument_hint: None,
            template: None,
        };
        assert_eq!(
            expand_command_submission(AgentId::ClaudeCode, "/compact", &[native]),
            None
        );
        assert_eq!(
            expand_command_submission(AgentId::ClaudeCode, "/unknown", &[]),
            None
        );
        assert_eq!(
            expand_command_submission(AgentId::ClaudeCode, "no slash", &[]),
            None
        );
    }

    #[test]
    fn template_expansion_appends_unconsumed_args() {
        assert_eq!(
            expand_command_template("static body", "extra"),
            "static body\n\nextra"
        );
        assert_eq!(expand_command_template("raw $ sign", ""), "raw $ sign");
    }

    #[test]
    fn merge_commands_orders_by_scope_then_name() {
        let merged = merge_commands(vec![
            skill("zeta"),
            ComposerCommand {
                name: "alpha".into(),
                description: String::new(),
                scope: CommandScope::User,
                argument_hint: None,
                template: None,
            },
            ComposerCommand {
                name: "beta".into(),
                description: String::new(),
                scope: CommandScope::Project,
                argument_hint: None,
                template: None,
            },
        ]);
        let names: Vec<&str> = merged.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["beta", "alpha", "zeta"]);
    }

    #[test]
    fn utf16_position_counts_code_units_and_lines() {
        let text = "你好\nabc";
        assert_eq!(byte_to_utf16_position(text, 0), (0, 0));
        // '你' and '好' each occupy 1 UTF-16 code unit (inside the BMP).
        assert_eq!(byte_to_utf16_position(text, "你好".len()), (0, 2));
        assert_eq!(byte_to_utf16_position(text, "你好\n".len()), (1, 0));
        assert_eq!(byte_to_utf16_position(text, text.len()), (1, 3));
    }
}
