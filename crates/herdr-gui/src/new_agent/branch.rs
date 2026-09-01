//! [INPUT]: The main-crate namespace and sibling-module public surface forwarded by the new_agent module root (super).
//! [OUTPUT]: Provides the New Agent picker's git branch snapshot reading. Startup-time
//! branch/worktree resolution (reuse/create) has moved up into
//! `shardlane_host::GitWorktreePreparation` (the M2 convergence); this file no longer owns
//! the cwd decision.
//! [POS]: The `crates/herdr-gui` new_agent submodule; only the read-only branch list
//! needed by the picker remains.
use super::*;

pub(super) fn git_branch_snapshot(project_path: &str) -> Result<BranchSnapshot, String> {
    let current = git_output(project_path, &["branch", "--show-current"]).unwrap_or_default();
    let branches = git_output(
        project_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    let mut names = branches
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Ok(BranchSnapshot {
            choices: vec![NewAgentBranchChoice {
                name: String::new(),
                label: "Current working tree".to_string(),
            }],
            selected_index: 0,
        });
    }
    let selected_index = names
        .iter()
        .position(|branch| branch == current.trim())
        .unwrap_or(0);
    let choices = names
        .into_iter()
        .map(|name| NewAgentBranchChoice {
            label: name.clone(),
            name,
        })
        .collect();
    Ok(BranchSnapshot {
        choices,
        selected_index,
    })
}

fn git_output(project_path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
