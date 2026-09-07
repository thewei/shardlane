---
name: shardlane
description: "Work with Shardlane, the macOS workspace app for coding agents: enumerate its Workspaces (Herdr instances) and Projects (Herdr runtime workspaces), inspect live agents, and manage tabs/panes/agents through the herdr CLI with exact instance targeting. Use when the user mentions Shardlane, their Workspaces, Projects, or running agents, or asks you to inspect or manage another agent from outside a Herdr pane."
---

# Shardlane

Shardlane is the macOS client for Herdr. It shows one window per Workspace,
where each Workspace is one named Herdr instance (session) and each Project
inside it is one Herdr runtime workspace. Tab, Pane, and Agent concepts are
Herdr-owned end to end. This skill teaches the exact vocabulary mapping and
the two CLIs you combine: the read-only `shardlane` CLI for querying, and the
`herdr` CLI for management.

## Concept alignment (read this first)

| Shardlane concept | Runtime fact (Herdr) | How to address it |
|---|---|---|
| Workspace | one named Herdr instance (session) | session name = the `HERDR_SESSION` value; the default instance has `is_default: true` and MUST be addressed by leaving `HERDR_SESSION` unset |
| Project | a Herdr runtime workspace inside one instance (`w1`, `w2`, ...) | `herdr workspace ...` commands run against that instance |
| Tab | Herdr tab | `herdr tab ...` |
| Pane | Herdr pane | `herdr pane ...` |
| Agent | a Herdr-recognized coding agent occupying a pane | `herdr agent ...` |

The word "workspace" means different things in the two CLIs. This is the
single most important rule:

- `shardlane workspace list` lists **Workspaces** (= Herdr instances/sessions).
- `HERDR_SESSION=<name> herdr workspace list` lists **Projects** inside that
  one Workspace (= Herdr runtime workspaces).
- Never set `HERDR_SESSION=default`. For the default instance, unset the
  variable entirely; the literal string `default` resolves to a non-existent
  socket.

## Locate the CLIs

The `shardlane` CLI is the Shardlane app binary running in headless mode:

    command -v shardlane || ls /Applications/Shardlane.app/Contents/MacOS/shardlane

The `herdr` CLI manages the runtime:

    command -v herdr

All `shardlane` commands are read-only and print JSON on stdout. If the
binary is missing, Shardlane is not installed; say so and stop.

## Query with the shardlane CLI

    shardlane workspace list
    shardlane project list [--workspace <name|display name>]
    shardlane agent list [--workspace <name|display name>]
    shardlane version

- `workspace list` returns rows with `name`, `display_name` (Shardlane-owned
  rename override), `running`, `is_default`, and `herdr_session` — the exact
  value to put in `HERDR_SESSION` (`null` means "leave it unset").
- `project list` sweeps every running Workspace and returns rows with
  `herdr_session`, `herdr_workspace_id`, `name`, `cwd`, `focused`,
  `active_tab_id`, and tab/pane counts. Stopped instances are skipped.
- `agent list` returns live agents across Workspaces with `herdr_session`,
  `name`, `kind`, `status`, `title`, `project`, `tab`, `pane`, and `cwd`.
- `--workspace` matches the session name or its `display_name` override,
  exactly. An explicit match on a stopped instance is an error, not a skip.
- Instance-level failures do not abort a sweep; they are reported in the
  `errors` array of the same JSON document.

## Manage through herdr

All mutations — creating layout, splitting panes, starting/prompting/reading
agents, sending keys — go through the `herdr` CLI so Herdr stays the single
runtime authority. Address the instance first, then use normal herdr commands:

    HERDR_SESSION=<herdr_session> herdr workspace list
    HERDR_SESSION=<herdr_session> herdr tab list --workspace <herdr_workspace_id>
    HERDR_SESSION=<herdr_session> herdr pane split --pane <pane_id> --direction right --cwd <dir> --no-focus
    HERDR_SESSION=<herdr_session> herdr agent list
    HERDR_SESSION=<herdr_session> herdr agent prompt <name-or-pane-id> "..." --wait --timeout 120000
    HERDR_SESSION=<herdr_session> herdr agent read <name-or-pane-id> --source recent-unwrapped --lines 120

If the `herdr` skill is installed, follow it for command syntax and workflow
discipline (pane selection, agent lifecycle states, read sources). One
adaptation: the `herdr` skill's `HERDR_ENV=1` check is a gate for agents that
are already running INSIDE a Herdr pane and want to use focused-session
shortcuts like `--current`. When you manage Shardlane Workspaces from the
outside, skip that gate and address instances explicitly with
`HERDR_SESSION` instead; never rely on another client's focused pane.

## Safety rules

- Parse IDs from JSON responses. Never guess `w1`, `w1:t1`, or `w1:p1` style
  IDs, and never derive them from list order.
- Prefer `--no-focus` for background work. Do not steal the user's focused
  pane or switch their UI unless asked.
- Do not close Workspaces, tabs, panes, or sessions you did not create unless
  the user explicitly asked. `herdr session stop <name>` and
  `herdr session delete <name>` destroy a whole Shardlane Workspace and every
  process in it — treat them as explicitly-requested-only, and never run
  `herdr server stop` casually.
- Agent names must match `[a-z][a-z0-9_-]{0,31}` and stay unique among live
  agents. Agent commands accept a unique live agent name or the pane ID
  hosting it — never terminal IDs or bare provider labels.
- Error conventions: herdr CLI server errors print JSON on stderr with exit
  status 1; syntax errors exit with status 2.
