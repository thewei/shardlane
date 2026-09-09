//! Shardlane headless CLI: the read-only control-surface entry point for
//! `shardlane <command>`.
//!
//! [INPUT]: depends on shardlane_host::mux (the MuxRegistry /
//! InstanceRef / InstanceListing projection contracts) and serde_json
//! [OUTPUT]: exposes try_dispatch (intercepts argv before anything else in
//! main(): workspace/project/agent queries, version, help; returns Some(exit)
//! when handled so the GUI never starts, None to continue GUI startup)
//! [POS]: the outward read-only control surface of crates/herdr-gui; queries
//! go through the neutral mux seam and never touch socket shapes; management
//! stays explicitly with the herdr CLI (taught by the bundled shardlane
//! skill, see skill/shardlane/SKILL.md)
//! [PROTOCOL]: Update this header on change, then check CLAUDE.md.

use std::io::Write as _;
use std::sync::Arc;

use serde_json::json;
use shardlane_host::mux::{
    InstanceListing, InstanceRef, MultiplexerConnection, MuxError, MuxRegistry,
};

// ============================================================
// Parse model: None = not a CLI invocation (fall through to GUI startup,
// keeping the macOS launch path unchanged).
// ============================================================

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CliParse {
    None,
    Help,
    Version,
    Command(CliCommand),
    /// Recognized as a CLI call but with bad usage (exit 2, matching the
    /// herdr CLI's usage-error convention).
    Usage(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CliCommand {
    Workspaces,
    Projects { workspace: Option<String> },
    Agents { workspace: Option<String> },
}

pub(crate) const USAGE: &str = "\
shardlane — headless control surface (read-only; JSON on stdout)

USAGE:
    shardlane workspace list
    shardlane project list [--workspace <name|display name>]
    shardlane agent list  [--workspace <name|display name>]
    shardlane version

NOTES:
    A Shardlane Workspace is one named Herdr instance. `herdr_session` is the
    exact value for the HERDR_SESSION env var; null means leave it unset
    (the default instance). Management belongs to the herdr CLI:

        HERDR_SESSION=<herdr_session> herdr <command> ...

    Server errors print JSON on stderr with exit status 1; usage errors exit 2.\n";

// ============================================================
// Parsing: only the recognized command surface is handled; any other first
// argument falls through to the GUI.
// ============================================================

pub(crate) fn parse_args(args: &[String]) -> CliParse {
    let Some(first) = args.first() else {
        return CliParse::None;
    };
    match first.as_str() {
        "-h" | "--help" | "help" => return CliParse::Help,
        "--version" | "version" => return CliParse::Version,
        "workspace" | "project" | "agent" => {}
        _ => return CliParse::None,
    }
    if args.get(1).map(String::as_str) != Some("list") {
        return CliParse::Usage(format!(
            "unknown or missing subcommand after '{first}' (expected 'list')\n\n{USAGE}"
        ));
    }
    let mut workspace: Option<String> = None;
    let mut index = 2;
    while index < args.len() {
        let arg = args[index].as_str();
        if let Some(value) = arg.strip_prefix("--workspace=") {
            workspace = Some(value.to_string());
        } else if arg == "--workspace" {
            index += 1;
            match args.get(index) {
                Some(value) => workspace = Some(value.clone()),
                None => {
                    return CliParse::Usage(format!("--workspace requires a value\n\n{USAGE}"));
                }
            }
        } else {
            return CliParse::Usage(format!("unexpected argument '{arg}'\n\n{USAGE}"));
        }
        index += 1;
    }
    match first.as_str() {
        "workspace" if workspace.is_some() => CliParse::Usage(format!(
            "'workspace list' takes no --workspace filter\n\n{USAGE}"
        )),
        "workspace" => CliParse::Command(CliCommand::Workspaces),
        "project" => CliParse::Command(CliCommand::Projects { workspace }),
        _ => CliParse::Command(CliCommand::Agents { workspace }),
    }
}

/// Called at the very top of main(): Some(exit_code) = handled headless;
/// None = continue GUI startup.
pub(crate) fn try_dispatch(args: &[String]) -> Option<i32> {
    match parse_args(args) {
        CliParse::None => None,
        CliParse::Help => {
            print!("{USAGE}");
            Some(0)
        }
        CliParse::Version => {
            emit(&json!({ "name": "shardlane", "version": env!("CARGO_PKG_VERSION") }));
            Some(0)
        }
        CliParse::Usage(message) => {
            eprint!("{message}");
            Some(2)
        }
        CliParse::Command(command) => Some(run_command(command)),
    }
}

fn run_command(command: CliCommand) -> i32 {
    let registry = MuxRegistry::with_builtins();
    match command {
        CliCommand::Workspaces => run_workspace_list(&registry),
        CliCommand::Projects { workspace } => run_project_list(&registry, workspace.as_deref()),
        CliCommand::Agents { workspace } => run_agent_list(&registry, workspace.as_deref()),
    }
}

// ============================================================
// Instance addressing: Workspace = backend instance; herdr_session is the
// exact HERDR_SESSION value.
// ============================================================

/// The exact `HERDR_SESSION` value: the default instance must leave it
/// unset; other backends (tmux etc.) ignore this env var and also return
/// None.
fn herdr_session_env(listing: &InstanceListing) -> Option<String> {
    if listing.backend == "herdr" && !listing.is_default {
        Some(listing.name.clone())
    } else {
        None
    }
}

/// Per-backend enumeration: an unavailable backend (CLI missing etc.) never
/// aborts the overall query, it is recorded in errors — for agents, an
/// "empty list" and "not enumerable" must stay distinguishable.
fn enumerated_instances(
    registry: &MuxRegistry,
    errors: &mut Vec<serde_json::Value>,
) -> Vec<InstanceListing> {
    let mut all = Vec::new();
    for backend in registry.backends() {
        match backend.list_instances() {
            Some(listings) => all.extend(listings),
            None => errors.push(json!({
                "backend": backend.id(),
                "error": "backend enumeration unavailable (CLI missing?)",
            })),
        }
    }
    all
}

/// `--workspace` matches by session name or display_name exactly; without
/// a filter only running instances are kept. Explicitly selecting a stopped
/// instance is an error rather than a skip (actionable contracts win).
fn filter_listings(
    listings: Vec<InstanceListing>,
    workspace: Option<&str>,
) -> Result<Vec<InstanceListing>, String> {
    match workspace {
        None => Ok(listings.into_iter().filter(|l| l.running).collect()),
        Some(want) => {
            // Real environments can collide a display_name with another
            // instance's name: the name always wins.
            let by_name: Vec<InstanceListing> = listings
                .iter()
                .filter(|l| l.name == want)
                .cloned()
                .collect();
            let matched = if by_name.is_empty() {
                listings
                    .into_iter()
                    .filter(|l| l.display_name.as_deref() == Some(want))
                    .collect()
            } else {
                by_name
            };
            match matched.len() {
                0 => Err(format!("workspace not found: {want}")),
                1 if !matched[0].running => {
                    Err(format!("workspace '{want}' exists but is not running"))
                }
                _ => Ok(matched),
            }
        }
    }
}

/// Side-effect-free connect: never starts an instance; the query surface
/// never starts a service on the user's behalf.
fn connect_instance(
    registry: &MuxRegistry,
    listing: &InstanceListing,
) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
    let reference = if listing.is_default {
        InstanceRef::default_instance(&listing.backend)
    } else {
        InstanceRef::named(&listing.backend, &listing.name)
    };
    registry.connect_instance(&reference)
}

// ============================================================
// The three query commands
// ============================================================

fn run_workspace_list(registry: &MuxRegistry) -> i32 {
    let mut errors = Vec::new();
    let listings = enumerated_instances(registry, &mut errors);
    let workspaces: Vec<serde_json::Value> = listings.iter().map(instance_row).collect();
    emit(&json!({ "workspaces": workspaces, "errors": errors }));
    0
}

fn instance_row(listing: &InstanceListing) -> serde_json::Value {
    json!({
        "backend": listing.backend,
        "name": listing.name,
        "display_name": listing.display_name,
        "running": listing.running,
        "is_default": listing.is_default,
        "herdr_session": herdr_session_env(listing),
    })
}

fn run_project_list(registry: &MuxRegistry, workspace: Option<&str>) -> i32 {
    let mut projects = Vec::new();
    let mut errors = Vec::new();
    let all = enumerated_instances(registry, &mut errors);
    if let Ok(listings) = filter_listings(all, workspace) {
        for listing in &listings {
            let Ok(connection) = connect_instance(registry, listing) else {
                continue;
            };
            match connection.visible_state() {
                Ok(state) => {
                    for ws in &state.workspaces {
                        projects.push(json!({
                            "backend": listing.backend,
                            "herdr_session": herdr_session_env(listing),
                            "herdr_workspace_id": ws.workspace_id,
                            "name": ws.label,
                            "cwd": ws.cwd,
                            "focused": ws.focused,
                            "active_tab_id": ws.active_tab_id,
                            "tabs": ws.tab_count,
                            "panes": ws.pane_count,
                        }));
                    }
                }
                Err(error) => errors.push(instance_error(listing, &error.to_string())),
            }
        }
    }
    emit(&json!({ "projects": projects, "errors": errors }));
    0
}

fn run_agent_list(registry: &MuxRegistry, workspace: Option<&str>) -> i32 {
    let mut agents = Vec::new();
    let mut errors = Vec::new();
    let all = enumerated_instances(registry, &mut errors);
    if let Ok(listings) = filter_listings(all, workspace) {
        for listing in &listings {
            let Ok(connection) = connect_instance(registry, listing) else {
                continue;
            };
            match connection.agents() {
                Ok(rows) => {
                    for agent in rows {
                        agents.push(json!({
                            "backend": listing.backend,
                            "herdr_session": herdr_session_env(listing),
                            "name": agent.name,
                            "kind": agent.agent,
                            "status": agent.agent_status,
                            "title": agent.title,
                            "project": agent.workspace_id,
                            "tab": agent.tab_id,
                            "pane": agent.pane_id,
                            "cwd": agent.cwd,
                        }));
                    }
                }
                Err(error) => errors.push(instance_error(listing, &error.to_string())),
            }
        }
    }
    emit(&json!({ "agents": agents, "errors": errors }));
    0
}

fn instance_error(listing: &InstanceListing, message: &str) -> serde_json::Value {
    json!({
        "backend": listing.backend,
        "herdr_session": herdr_session_env(listing),
        "error": message,
    })
}

// ============================================================
// Output contract: data on stdout as JSON; runtime errors on stderr as JSON
// with exit status 1.
// ============================================================

fn emit(value: &serde_json::Value) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = serde_json::to_writer_pretty(&mut handle, value);
    let _ = handle.write_all(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn listing(backend: &str, name: &str, running: bool, is_default: bool) -> InstanceListing {
        InstanceListing {
            backend: backend.to_string(),
            name: name.to_string(),
            display_name: None,
            running,
            is_default,
        }
    }

    #[test]
    fn empty_and_unknown_arguments_fall_through_to_gui() {
        assert_eq!(parse_args(&args(&[])), CliParse::None);
        assert_eq!(parse_args(&args(&["-psn_0_12345"])), CliParse::None);
    }

    #[test]
    fn help_and_version_aliases_parse() {
        assert_eq!(parse_args(&args(&["--help"])), CliParse::Help);
        assert_eq!(parse_args(&args(&["help"])), CliParse::Help);
        assert_eq!(parse_args(&args(&["version"])), CliParse::Version);
        assert_eq!(parse_args(&args(&["--version"])), CliParse::Version);
    }

    #[test]
    fn query_commands_parse_with_workspace_flag_forms() {
        assert_eq!(
            parse_args(&args(&["workspace", "list"])),
            CliParse::Command(CliCommand::Workspaces)
        );
        assert_eq!(
            parse_args(&args(&["project", "list", "--workspace", "lab"])),
            CliParse::Command(CliCommand::Projects {
                workspace: Some("lab".to_string()),
            })
        );
        assert_eq!(
            parse_args(&args(&["agent", "list", "--workspace=api"])),
            CliParse::Command(CliCommand::Agents {
                workspace: Some("api".to_string()),
            })
        );
    }

    #[test]
    fn known_group_with_bad_usage_is_exit_two_error() {
        assert!(matches!(
            parse_args(&args(&["workspace"])),
            CliParse::Usage(_)
        ));
        assert!(matches!(
            parse_args(&args(&["workspace", "create"])),
            CliParse::Usage(_)
        ));
        assert!(matches!(
            parse_args(&args(&["workspace", "list", "--workspace", "x"])),
            CliParse::Usage(_)
        ));
        assert!(matches!(
            parse_args(&args(&["project", "list", "--workspace"])),
            CliParse::Usage(_)
        ));
    }

    #[test]
    fn herdr_session_env_follows_instance_identity() {
        // The default instance must leave HERDR_SESSION unset; a named herdr
        // instance carries the session name; other backends ignore the var.
        assert_eq!(
            herdr_session_env(&listing("herdr", "default", true, true)),
            None
        );
        assert_eq!(
            herdr_session_env(&listing("herdr", "lab", true, false)),
            Some("lab".to_string())
        );
        assert_eq!(
            herdr_session_env(&listing("tmux", "main", true, false)),
            None
        );
    }

    #[test]
    fn sweep_without_filter_keeps_only_running_instances() {
        let listings = vec![
            listing("herdr", "default", true, true),
            listing("herdr", "stopped", false, false),
        ];
        let kept = filter_listings(listings, None).ok().unwrap_or_default();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "default");
    }

    #[test]
    fn filter_matches_name_and_display_name_exactly() {
        let mut named = listing("herdr", "lab", true, false);
        named.display_name = Some("The Lab".to_string());
        let listings = vec![named, listing("herdr", "api", true, false)];

        let by_name = filter_listings(listings.clone(), Some("lab"))
            .ok()
            .unwrap_or_default();
        let by_display = filter_listings(listings.clone(), Some("The Lab"))
            .ok()
            .unwrap_or_default();
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_display.len(), 1);
        assert!(filter_listings(listings, Some("the lab")).is_err());
    }

    #[test]
    fn explicitly_selected_stopped_instance_is_an_error() {
        let listings = vec![listing("herdr", "stopped", false, false)];
        let message = filter_listings(listings, Some("stopped"))
            .err()
            .unwrap_or_default();
        assert!(message.contains("not running"), "unexpected: {message}");
    }

    #[test]
    fn name_match_wins_over_display_name_collision() {
        // Real-world case: a default instance's display_name equals another
        // named instance's name.
        let mut default_row = listing("herdr", "default", true, true);
        default_row.display_name = Some("MyWork".to_string());
        let listings = vec![default_row, listing("herdr", "MyWork", true, false)];

        let matched = filter_listings(listings, Some("MyWork"))
            .ok()
            .unwrap_or_default();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "MyWork");
        assert!(!matched[0].is_default);
    }
}
