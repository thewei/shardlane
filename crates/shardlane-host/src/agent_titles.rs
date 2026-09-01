//! Deterministic task-title suggestion for Agent launch/transfer.
//!
//! [INPUT]: the user's raw task prompt (the text before any branch/attachment
//! suffix appended by the launch builder).
//! [OUTPUT]: `suggest_task_title` — returns a 2–5 word task title when
//! confidence is high; returns `None` when evidence is insufficient and the
//! caller falls back to the Provider display name.
//! [POS]: pure function, zero IO, zero model/network calls (plan §17: no
//! model/network naming request). Only deliberate deterministic patterns are
//! matched; no general semantic compression.

const MAX_TITLE_CHARS: usize = 30;
const MAX_TITLE_WORDS: usize = 5;

/// Courtesy prefixes stripped only when the remaining sentence stays meaningful.
const COURTESY_PREFIXES: &[&str] = &[
    "please ",
    "can you ",
    "could you ",
    "would you ",
    "help me ",
    "i need you to ",
    "i want you to ",
    "let's ",
    "let us ",
];

/// Weak/generic requests that must never become a task title.
const WEAK_REQUESTS: &[&str] = &[
    "continue",
    "go",
    "stop",
    "help",
    "again",
    "more",
    "next",
    "ok",
    "okay",
    "yes",
    "no",
    "do it",
    "fix it",
    "fix this",
    "test it",
    "work on this",
    "try again",
    "run",
    "start",
];

/// Clause separators at which a long request is cut to its leading task phrase.
const CLAUSE_SEPARATORS: &[&str] = &[" and then ", " then ", " and also ", ", ", "; ", " and "];

fn strip_task_prefix(line: &str) -> &str {
    let mut text = line.trim_start();
    loop {
        let stripped = text
            .strip_prefix("- ")
            .or_else(|| text.strip_prefix("* "))
            .or_else(|| text.strip_prefix("+ "))
            .or_else(|| text.strip_prefix("[ ] "))
            .or_else(|| text.strip_prefix("[x] "))
            .or_else(|| text.strip_prefix("[X] "));
        match stripped {
            Some(rest) => text = rest,
            None => return text,
        }
    }
}

fn strip_courtesy(text: &str) -> &str {
    let lowered = text.to_lowercase();
    for prefix in COURTESY_PREFIXES {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            if rest.trim().len() >= 4 {
                return &text[prefix.len()..];
            }
        }
    }
    text
}

fn truncate_clause(text: &str) -> &str {
    let lowered = text.to_lowercase();
    let mut cut = text.len();
    for separator in CLAUSE_SEPARATORS {
        if let Some(position) = lowered.find(separator) {
            cut = cut.min(position);
        }
    }
    // Cut at a sentence terminator if it appears before the clause cut.
    for terminator in ['.', ':', '!'] {
        if let Some(position) = text.find(terminator) {
            if position > 0 && position < cut {
                cut = position;
            }
        }
    }
    text[..cut].trim()
}

/// Mid-title stop words that stay lowercase unless they lead the title.
const TITLE_STOP_WORDS: &[&str] = &[
    "the", "a", "an", "of", "to", "for", "and", "in", "on", "with", "into",
];

/// Title-case a word only when it is purely lowercase alphabetic, so
/// identifiers, paths, and acronyms survive untouched. Mid-title stop words
/// stay lowercase for natural reading.
fn title_case_word(word: &str, is_first: bool) -> String {
    if !word.chars().all(|character| character.is_ascii_lowercase()) {
        return word.to_string();
    }
    if !is_first && TITLE_STOP_WORDS.contains(&word) {
        return word.to_string();
    }
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn is_weak(text: &str) -> bool {
    let lowered = text.trim().to_lowercase();
    let trimmed = lowered.trim_end_matches(['.', '!', '?']);
    if trimmed.is_empty() {
        return true;
    }
    // Slash commands and skill invocations are not task statements.
    if trimmed.starts_with('/') || trimmed.starts_with('$') {
        return true;
    }
    WEAK_REQUESTS.contains(&trimmed)
}

/// Suggest a short task-oriented title from the original user prompt.
/// Returns `None` whenever confidence is not high; the caller keeps the
/// provider display name instead of forcing a mediocre label.
pub fn suggest_task_title(prompt: &str) -> Option<String> {
    let first_line = prompt.lines().find(|line| !line.trim().is_empty())?;
    let text = strip_courtesy(strip_task_prefix(first_line));
    if is_weak(text) {
        return None;
    }
    let clause = truncate_clause(text);
    if is_weak(clause) || clause.len() < 4 {
        return None;
    }
    let mut words: Vec<&str> = clause.split_whitespace().collect();
    words.truncate(MAX_TITLE_WORDS);
    // Drop trailing filler words that add no task meaning (after truncation, so
    // a cut-off tail cannot strand a filler at the end).
    while matches!(
        words.last().copied().map(str::to_lowercase).as_deref(),
        Some(
            "the"
                | "a"
                | "an"
                | "this"
                | "that"
                | "it"
                | "please"
                | "now"
                | "for"
                | "with"
                | "to"
                | "of"
                | "in"
                | "on"
                | "during"
                | "into"
        )
    ) {
        words.pop();
    }
    if words.len() == 1 {
        // CJK-style task statements carry no spaces; a bounded single block is
        // still a valid high-confidence title. Single ASCII words are not.
        let only = words[0];
        let single_word_ok =
            !only.is_ascii() && (4..=MAX_TITLE_CHARS).contains(&only.chars().count());
        if !single_word_ok {
            return None;
        }
    }
    let titled: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(index, word)| title_case_word(word, index == 0))
        .collect();
    let mut title = titled.join(" ");
    if title.chars().count() > MAX_TITLE_CHARS {
        // Trim whole words only; a mid-word cut would corrupt identifiers.
        while title.chars().count() > MAX_TITLE_CHARS && title.contains(' ') {
            let cut = title.rfind(' ')?;
            title = title[..cut].to_string();
        }
        if title.chars().count() > MAX_TITLE_CHARS || title.split(' ').count() < 2 {
            return None;
        }
    }
    let starts_alphabetic = title.chars().next().is_some_and(char::is_alphabetic);
    if !starts_alphabetic {
        return None;
    }
    Some(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_task_statements_become_short_titles() {
        assert_eq!(
            suggest_task_title("Fix terminal scroll regression and add coverage"),
            Some("Fix Terminal Scroll Regression".to_string())
        );
        assert_eq!(
            suggest_task_title("Audit our Agent provider integration coverage"),
            Some("Audit Our Agent Provider".to_string())
        );
        assert_eq!(
            suggest_task_title("Refactor shardlane-host API boundaries"),
            Some("Refactor shardlane-host API".to_string())
        );
        assert_eq!(
            suggest_task_title("- please add dark mode to the settings page"),
            Some("Add Dark Mode".to_string())
        );
    }

    #[test]
    fn weak_and_generic_requests_stay_untitled() {
        assert_eq!(suggest_task_title("Continue"), None);
        assert_eq!(suggest_task_title("Please help"), None);
        assert_eq!(suggest_task_title("/plan"), None);
        assert_eq!(suggest_task_title("work on this"), None);
        assert_eq!(suggest_task_title(""), None);
        assert_eq!(suggest_task_title("ok"), None);
        assert_eq!(suggest_task_title("Run"), None);
    }

    #[test]
    fn identifiers_acronyms_and_cjk_survive_without_case_corruption() {
        assert_eq!(
            suggest_task_title("fix the GPU pipeline for macOS rendering"),
            Some("Fix the GPU Pipeline".to_string())
        );
        assert_eq!(
            suggest_task_title("修复终端滚动回退并补充测试覆盖"),
            Some("修复终端滚动回退并补充测试覆盖".to_string())
        );
        assert_eq!(
            suggest_task_title("update README, then ping the team"),
            Some("Update README".to_string())
        );
    }

    #[test]
    fn courtesy_prefix_is_stripped_only_when_content_remains() {
        assert_eq!(
            suggest_task_title("Please audit the login flow"),
            Some("Audit the Login Flow".to_string())
        );
        assert_eq!(suggest_task_title("Please help"), None);
    }

    #[test]
    fn very_long_requests_trim_whole_words_only() {
        let title = suggest_task_title("Investigate intermittent packet loss during long uploads")
            .unwrap_or_else(|| panic!("high-confidence request"));
        assert!(title.chars().count() <= MAX_TITLE_CHARS);
        assert!(title.contains(' '));
    }
}
