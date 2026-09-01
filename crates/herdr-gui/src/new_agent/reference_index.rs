//! Composer reference catalog: project file indexing and Agent skill/slash-command scanning.
//!
//! [INPUT]: std (fs/walk/process), reference.rs' command model, and
//! shardlane_history's AgentId; read-only external directories, zero writes, zero runtime.
//! [OUTPUT]: Provides ProjectFileIndex/build_file_index (git ls-files first,
//! bounded directory-walk fallback outside git) and CommandCatalog/scan_agent_commands
//! (claude/codex/pi three-way dialect catalogs, gracefully empty on missing directories).
//! [POS]: The read-only data layer of new_agent, built by surface.rs background tasks and
//! consumed by the reference provider; the same kind of catalog as history (a client-side
//! read-only projection).

use super::reference::{merge_commands, CommandScope, ComposerCommand};
use shardlane_history::AgentId;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Index entry cap (truncated beyond it; 20k entries is plenty for the
/// completion popup and keeps the scan bounded).
const FILE_INDEX_CAP: usize = 20_000;
/// Depth cap of the non-git fallback walk.
const WALK_MAX_DEPTH: usize = 10;
/// Directory names skipped by the non-git fallback walk (common cross-language
/// build artifacts / dependency trees).
const WALK_SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "__pycache__",
    ".cache",
    "vendor",
];
/// Description field truncation (one line in the popup's detail).
const DESCRIPTION_CAP: usize = 96;

/// Project file index: a list of relative paths (forward slashes), a one-shot
/// snapshot (rebuilt when a project opens/switches).
#[derive(Clone, Debug)]
pub(crate) struct ProjectFileIndex {
    pub(crate) project_path: String,
    pub(crate) files: Vec<String>,
}

/// Agent command catalog: a merged, sorted snapshot of skills + custom commands.
#[derive(Clone, Debug)]
pub(crate) struct CommandCatalog {
    pub(crate) agent: AgentId,
    pub(crate) commands: Vec<ComposerCommand>,
}

/// Build the project file index: git repos use `git ls-files -coz --exclude-standard`
/// (naturally respects gitignore and includes untracked files); non-git
/// directories fall back to a bounded walk.
pub(crate) fn build_file_index(project_root: &Path) -> ProjectFileIndex {
    let files = git_ls_files(project_root).unwrap_or_else(|| bounded_walk(project_root));
    ProjectFileIndex {
        project_path: project_root.to_string_lossy().to_string(),
        files,
    }
}

/// Scan the selected agent's skill/slash-command catalog (gracefully empty when
/// directories are missing).
pub(crate) fn scan_agent_commands(agent: AgentId, project_root: &Path) -> CommandCatalog {
    let commands = scan_agent_commands_in(agent, project_root, home_root());
    CommandCatalog { agent, commands }
}

fn home_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Internal entry for test injection of the home root.
fn scan_agent_commands_in(
    agent: AgentId,
    project_root: &Path,
    home: PathBuf,
) -> Vec<ComposerCommand> {
    let mut commands: Vec<ComposerCommand> = Vec::new();
    match agent {
        AgentId::ClaudeCode => {
            collect_skill_dirs(
                &[
                    project_root.join(".claude/skills"),
                    home.join(".claude/skills"),
                ],
                &mut commands,
            );
            collect_command_markdown(
                &[
                    (project_root.join(".claude/commands"), CommandScope::Project),
                    (home.join(".claude/commands"), CommandScope::User),
                ],
                &mut commands,
            );
        }
        AgentId::Codex => {
            collect_skill_dirs(
                &[
                    project_root.join(".codex/skills"),
                    home.join(".codex/skills"),
                ],
                &mut commands,
            );
            collect_command_markdown(
                &[
                    (project_root.join(".codex/prompts"), CommandScope::Project),
                    (home.join(".codex/prompts"), CommandScope::User),
                ],
                &mut commands,
            );
        }
        AgentId::Pi | AgentId::Omp => {
            collect_skill_dirs(
                &[
                    project_root.join(".pi/agent/skills"),
                    home.join(".pi/agent/skills"),
                ],
                &mut commands,
            );
        }
        _ => {}
    }
    // Same-name deduplication: the higher-priority scope (earlier after sorting)
    // is kept.
    merge_commands(commands).into_iter().fold(
        Vec::new(),
        |mut acc: Vec<ComposerCommand>, command| {
            if !acc.iter().any(|existing| existing.name == command.name) {
                acc.push(command);
            }
            acc
        },
    )
}

/// Collect `*/SKILL.md` skill commands from the exact list of skills directories.
/// Scope is always Skill (original semantics: Skill is a source category peer to
/// Project/User, and the dialect encoding keys off it); project-before-user
/// priority comes from the input order plus stable deduplication.
fn collect_skill_dirs(skills_dirs: &[PathBuf], commands: &mut Vec<ComposerCommand>) {
    for skills_dir in skills_dirs {
        let entries = match std::fs::read_dir(skills_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let Some((name, description)) = read_skill_markdown(&skill_md) else {
                continue;
            };
            commands.push(ComposerCommand {
                name,
                description,
                scope: CommandScope::Skill,
                argument_hint: None,
                template: None,
            });
        }
    }
}

/// Collect slash commands from a directory of `.md` command files (claude
/// commands / codex prompts).
fn collect_command_markdown(
    bases: &[(PathBuf, CommandScope)],
    commands: &mut Vec<ComposerCommand>,
) {
    for (dir, scope) in bases {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let (name, description, argument_hint) = read_command_markdown(&path)
                .unwrap_or_else(|| (stem.to_string(), String::new(), None));
            commands.push(ComposerCommand {
                name,
                description,
                scope: *scope,
                argument_hint,
                // claude/codex natively expand $ARGUMENTS in their own command
                // directories — the client passes through without pre-expanding.
                template: None,
            });
        }
    }
}

/// Parse SKILL.md: the frontmatter's name/description (name falls back to the
/// directory name).
fn read_skill_markdown(path: &Path) -> Option<(String, String)> {
    let body = std::fs::read_to_string(path).ok()?;
    let frontmatter = frontmatter_block(&body);
    let name = frontmatter
        .as_deref()
        .and_then(|block| frontmatter_value(block, "name"))
        .or_else(|| {
            path.parent()
                .and_then(|dir| dir.file_name())
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })?;
    if name.trim().is_empty() {
        return None;
    }
    let description = frontmatter
        .as_deref()
        .and_then(|block| frontmatter_value(block, "description"))
        .unwrap_or_default();
    Some((name.trim().to_string(), clamp_description(&description)))
}

/// Parse a command markdown file: the name comes from the file name (claude/codex
/// semantics), taking description/argument-hint.
fn read_command_markdown(path: &Path) -> Option<(String, String, Option<String>)> {
    let stem = path.file_stem()?.to_str()?.to_string();
    let body = std::fs::read_to_string(path).ok()?;
    let frontmatter = frontmatter_block(&body);
    let description = frontmatter
        .as_deref()
        .and_then(|block| frontmatter_value(block, "description"))
        .unwrap_or_default();
    let argument_hint = frontmatter
        .as_deref()
        .and_then(|block| frontmatter_value(block, "argument-hint"));
    Some((
        stem,
        clamp_description(&description),
        argument_hint.map(|hint| clamp_description(&hint)),
    ))
}

/// Take the frontmatter block wrapped in `---` at the head of a markdown file
/// (excluding the fence lines).
fn frontmatter_block(body: &str) -> Option<String> {
    let rest = body.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

/// Extract single-line `key: value` entries from a frontmatter block (enough
/// coverage for name/description/argument-hint; no YAML dependency pulled in).
fn frontmatter_value(block: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    block
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .starts_with(&prefix)
                .then(|| trimmed[prefix.len()..].trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn clamp_description(value: &str) -> String {
    let collapsed: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= DESCRIPTION_CAP {
        return collapsed;
    }
    let mut clamped: String = collapsed.chars().take(DESCRIPTION_CAP).collect();
    clamped.push('…');
    clamped
}

/// `git ls-files -coz --exclude-standard`: tracked + untracked (respects
/// gitignore).
fn git_ls_files(project_root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .current_dir(project_root)
        .args(["ls-files", "-coz", "--exclude-standard"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    files.truncate(FILE_INDEX_CAP);
    (!files.is_empty()).then_some(files)
}

/// Bounded fallback walk for non-git directories (skips common artifact
/// directories; capped by both depth and count).
fn bounded_walk(project_root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(project_root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if files.len() >= FILE_INDEX_CAP {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if files.len() >= FILE_INDEX_CAP {
                break;
            }
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if WALK_SKIP_DIRS.contains(&name.as_ref()) || depth + 1 > WALK_MAX_DEPTH {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if file_type.is_file() {
                if let Ok(relative) = path.strip_prefix(project_root) {
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    if !relative.is_empty() {
                        files.push(relative);
                    }
                }
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_walk_finds_files_and_skips_product_dirs() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let root = tmp.path();
        for path in ["src/main.rs", "src/lib.rs", "README.md"] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap_or(root))
                .unwrap_or_else(|error| panic!("mkdir: {error}"));
            std::fs::write(&file, "x").unwrap_or_else(|error| panic!("write: {error}"));
        }
        std::fs::create_dir_all(root.join("node_modules/pkg"))
            .unwrap_or_else(|error| panic!("mkdir: {error}"));
        std::fs::write(root.join("node_modules/pkg/index.js"), "x")
            .unwrap_or_else(|error| panic!("write: {error}"));
        let index = build_file_index(root);
        assert!(index.files.contains(&"src/main.rs".to_string()));
        assert!(index.files.contains(&"README.md".to_string()));
        assert!(
            !index.files.iter().any(|f| f.starts_with("node_modules")),
            "product dirs must be skipped: {:?}",
            index.files
        );
    }

    #[test]
    fn git_index_includes_untracked_and_respects_gitignore() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let root = tmp.path();
        let initialized = Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        assert!(initialized, "git init required for this test");
        std::fs::write(root.join("tracked.rs"), "x")
            .unwrap_or_else(|error| panic!("write: {error}"));
        std::fs::write(root.join("untracked.rs"), "x")
            .unwrap_or_else(|error| panic!("write: {error}"));
        std::fs::write(root.join(".gitignore"), "ignored.rs\n")
            .unwrap_or_else(|error| panic!("write: {error}"));
        std::fs::write(root.join("ignored.rs"), "x")
            .unwrap_or_else(|error| panic!("write: {error}"));
        let files = git_ls_files(root).unwrap_or_default();
        assert!(files.contains(&"tracked.rs".to_string()));
        assert!(files.contains(&"untracked.rs".to_string()));
        assert!(!files.contains(&"ignored.rs".to_string()));
    }

    #[test]
    fn skill_scan_reads_frontmatter_and_falls_back_to_dir_name() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        let project_skills = project.join(".claude/skills/review-pro");
        std::fs::create_dir_all(&project_skills).unwrap_or_else(|error| panic!("mkdir: {error}"));
        std::fs::write(
            project_skills.join("SKILL.md"),
            "---\nname: review\ndescription: Deep review the changes\n---\nbody",
        )
        .unwrap_or_else(|error| panic!("write: {error}"));
        let user_skills = home.join(".claude/skills/unnamed-skill");
        std::fs::create_dir_all(&user_skills).unwrap_or_else(|error| panic!("mkdir: {error}"));
        std::fs::write(user_skills.join("SKILL.md"), "no frontmatter")
            .unwrap_or_else(|error| panic!("write: {error}"));

        let commands = scan_agent_commands_in(AgentId::ClaudeCode, &project, home);
        let names: Vec<(&str, CommandScope)> = commands
            .iter()
            .map(|c| (c.name.as_str(), c.scope))
            .collect();
        assert!(names.contains(&("review", CommandScope::Skill)));
        // A skill without frontmatter falls back to the directory name.
        assert!(names.contains(&("unnamed-skill", CommandScope::Skill)));
        let review = commands
            .iter()
            .find(|c| c.name == "review")
            .unwrap_or_else(|| panic!("review command missing: {commands:?}"));
        assert_eq!(review.description, "Deep review the changes");
    }

    #[test]
    fn codex_and_pi_dialects_scan_their_own_dirs() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        let codex_prompt = home.join(".codex/prompts");
        std::fs::create_dir_all(&codex_prompt).unwrap_or_else(|error| panic!("mkdir: {error}"));
        std::fs::write(
            codex_prompt.join("ask.md"),
            "---\ndescription: Ask a question\n---\n",
        )
        .unwrap_or_else(|error| panic!("write: {error}"));
        let pi_skill = home.join(".pi/agent/skills/grill");
        std::fs::create_dir_all(&pi_skill).unwrap_or_else(|error| panic!("mkdir: {error}"));
        std::fs::write(
            pi_skill.join("SKILL.md"),
            "---\nname: grill\ndescription: Grill me\n---\n",
        )
        .unwrap_or_else(|error| panic!("write: {error}"));

        let codex = scan_agent_commands_in(AgentId::Codex, &project, home.clone());
        assert!(codex
            .iter()
            .any(|c| c.name == "ask" && c.scope == CommandScope::User));

        let pi = scan_agent_commands_in(AgentId::Pi, &project, home);
        assert!(pi
            .iter()
            .any(|c| c.name == "grill" && c.scope == CommandScope::Skill));
    }

    #[test]
    fn unsupported_agents_get_empty_catalog() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let catalog = scan_agent_commands(AgentId::Gemini, tmp.path());
        assert!(catalog.commands.is_empty());
    }

    #[test]
    fn duplicate_names_keep_highest_priority_scope() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let project = tmp.path().join("proj");
        let home = tmp.path().join("home");
        for base in [
            project.join(".claude/skills/review"),
            home.join(".claude/skills/review"),
        ] {
            std::fs::create_dir_all(&base).unwrap_or_else(|error| panic!("mkdir: {error}"));
            std::fs::write(base.join("SKILL.md"), "---\nname: review\n---\n")
                .unwrap_or_else(|error| panic!("write: {error}"));
        }
        let commands = scan_agent_commands_in(AgentId::ClaudeCode, &project, home);
        let reviews: Vec<&ComposerCommand> =
            commands.iter().filter(|c| c.name == "review").collect();
        assert_eq!(reviews.len(), 1, "duplicate names dedupe: {commands:?}");
        assert_eq!(reviews[0].scope, CommandScope::Skill);
    }
}
