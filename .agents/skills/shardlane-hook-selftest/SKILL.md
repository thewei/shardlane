---
name: shardlane-hook-selftest
description: Self-test Shardlane's agent-hook, journal, and Chat pipeline against a live agent session - fire hooks manually, verify journal writes and Herdr session identity, read the operation log, and match failure signatures. Use when the Chat entry button or binding misbehaves, hook events don't arrive, or after changing hook templates, installers, or the capability registry.
---

# Shardlane Hook Pipeline Self-Test

Verify the hook chain end to end with a real agent session instead of guessing:
CLI process -> hook script -> app IPC socket -> Host ingest -> journal -> Chat binding.

## Surfaces (all user-local)

| Surface | Path / command |
|---|---|
| App IPC socket | `~/.shardlane/run/agent-status.sock` (exists while the app runs) |
| Hook journal | `~/.shardlane/hook-journal/<agent>/<session>.jsonl` (8 MiB rotate, 64 files/agent, 7-day freshness) |
| Operation log | `~/.shardlane/logs/ui.log` (`SHARDLANE_UI_LOG_PATH` overrides; 512 KiB rotate) |
| Installed hook scripts | `~/.shardlane/hooks/shardlane-<agent>-hook.sh` |
| Templates / installer | `crates/shardlane-host/src/agent_hooks/registry.rs` (`SHELL_HOOK_TEMPLATE`, `CURRENT_HOOK_VERSION`) |
| Per-CLI hook configs | e.g. `~/.gemini/config/hooks.json` (group `shardlane-hook`), `~/.claude/settings.json` |
| Herdr session identity | `HERDR_SESSION=<name> herdr agent list` -> `agent_session` |

## The entry-gate chain (button hidden -> walk the gates in order)

`focused_chat_agent()` requires ALL of:

1. Herdr lists an Agent for the focused pane **with** `agent_session` (typed
   identity). Absent when no hook reported the session for that pane/process.
2. `agent_session.agent` resolves through the registry aliases
   (`resolve_agent_alias`).
3. The capability tier is live (`HookJournal` or `AppendLog`).
4. The provider is exposed (not Hidden).
5. `config.json -> providers.enabled` contains the slug (Settings -> Providers).

The Chat-toggle failure branch logs `chat entry blocked: ...` to the operation
log naming the exact gate - read it before reasoning from code.

## Self-test recipes

### 1. Live-state sweep (read-only)

```sh
shardlane agent list                                 # sweeps every workspace
HERDR_SESSION=<herdr_session> herdr agent list       # one instance + agent_session
herdr pane process-info --pane <pane_id>             # foreground argv (recover ids)
ps eww <pid> | tr ' ' '\n' | rg '^HERDR_'            # env the hook will inherit
```

Instances matter: unset `HERDR_SESSION` addresses the default instance only;
an agent running in another instance is invisible to a bare `herdr agent list`.

### 2. Fire the hook manually (end-to-end, no agent restart)

The hook chain is `agy -> hook.sh -> python`. Payload session ids may be
absent, so the script recovers the id from **ancestor** process args (walk at
least two levels - the direct parent is the hook script itself). Reproduce with
a fake grandparent whose argv carries the id:

```sh
export HERDR_ENV=1 HERDR_PANE_ID=<pane_id> HERDR_SOCKET_PATH=<herdr.sock>
bash -c 'echo "{\"hook_event_name\":\"PreInvocation\"}" | \
  bash ~/.shardlane/hooks/shardlane-<agent>-hook.sh <agent> PreInvocation; sleep 2' \
  fake-agent --conversation <real-session-uuid> >/dev/null 2>&1
```

Expect: `ui.log` -> `hook journal: ... outcome=ok`; the journal file appears;
`herdr agent list` shows `agent_session.value == <uuid>`.

### 3. Replay Herdr's official hook (fallback path)

Its script gates on `HERDR_ENV=1` and exits silently when the payload lacks
`conversationId` - replay with a payload that carries it:

```sh
export HERDR_ENV=1 HERDR_PANE_ID=<pane_id> HERDR_SOCKET_PATH=<herdr.sock>
echo '{"hook_event_name":"PreInvocation","conversationId":"<uuid>"}' | \
  bash ~/.gemini/config/hooks/herdr-agent-state.sh session
```

### 4. Read the operation log

`tail ~/.shardlane/logs/ui.log` - key lines:

- `hook journal: agent=... kind=... session=... outcome=ok|err(...)` - write result
- `hook report rejected: ... (identity/capability/anchor gate)` - normalized but dropped
- `chat lookup: ... journal=absent (connecting)` - binding retry every 2 s while unbound
- `chat entry blocked: ...` - the failing entry gate
- `hook install/refresh/uninstall <agent> -> <outcome>`
- `hook enrich: agent=antigravity session=... source_turns=N appended=M` - the
  Antigravity body backfill ran on a SessionStart/TurnComplete event (source
  turns = agy db turns seen, appended = records actually written after dedupe)

## Failure signatures (real incidents, 2026-09-19)

| Signature | Cause | Fix |
|---|---|---|
| Connecting forever, journal empty, no ui.log hook lines | hook script never ran or died early (python syntax error, missing `HERDR_ENV=1`, app not running) | fix script; `python3 -m py_compile` the template's python block (unit test exists); restart the agent |
| Chat open -> `No such file or directory (os error 2)` | catalog virtual path (`<db>#<id>`) fed to the tail transport | HookJournal agents bind the journal source, never the catalog path |
| Same journal record twice | dual-channel ingest (socket + OSC both arrived, both ingested) | ingest only in the IPC server thread |
| `agent_session.value` holds an old conversation id | hook events for the new invocation never fired | restart the agent; note PreInvocation fires at the **start of each turn** (first prompt), Stop at turn end - a freshly opened, never-prompted agy TUI fires nothing, so no button until the first prompt |
| Chat opens (no error) but the view is one blank pane | journal holds lifecycle events only - agy hook payloads carry **no text**, so 0 messages decode; the empty conversation is rendered "blank" | Antigravity enrichment backfills assistant replies from agy's own `conversations/<uuid>.db` on the next hook event; verify via `hook enrich ... appended=N` and `assistant_message` lines in the journal. User prompts are NOT recoverable (absent from both the db plaintext and history.jsonl) |
| `hook report rejected ... anchor gate` | session id empty/absent and the ancestor walk found nothing | verify walk depth (the direct parent is hook.sh; the agent sits higher) |
| reinstall reports Installed but behavior is stale | template edited without bumping `CURRENT_HOOK_VERSION` **and every embedded marker** | bump all markers together; the python-compile regression test covers syntax only |

## Change protocol

1. Template edits: bump `CURRENT_HOOK_VERSION` **and all four embedded
   `SHARDLANE_HOOK_VERSION=` markers**; keep the python-compile regression
   test (`shell_hook_template_python_block_compiles`) green.
2. Per-CLI payload keys are undocumented - verify against Herdr's own hook
   (`~/.gemini/config/hooks/herdr-agent-state.sh`) or recover ids from process
   args instead of trusting payload shapes.
3. Reinstall via Settings -> Agent Hooks (writes the managed group, preserves
   foreign groups), then restart the agent process.
4. Log new diagnostics through `shardlane_host::op_log` (the operation log),
   not `println!` or `lag_log`.

## Eval prompts

1. `agy runs in a pane but the Chat button never appears.` - expected: sweep
   `shardlane agent list` plus per-instance `herdr agent list` for
   `agent_session`, then walk the five gates in order; read `ui.log` for
   `chat entry blocked` instead of reasoning from code.
2. `I reinstalled the hook but the journal is still empty.` - expected: replay
   the installed script with a fake agent grandparent and the pane env;
   distinguish script failure (no ui.log line at all) from ingest rejection
   (a rejected line).
3. `Chat opens with os error 2 for agy.` - expected: recognize the catalog
   virtual-path signature; HookJournal agents bind the journal, never the
   catalog's `<db>#<id>` path.
4. `Chat opens but shows a blank pane for an agy conversation that has real
   replies.` - expected: check the journal for lifecycle-only records
   (`session_start`/`turn_complete`, no `assistant_message`), conclude the
   payload-carries-no-text signature, then trigger a turn in agy and confirm
   the `hook enrich` line appended records; cross-check counts against
   `SELECT count(*) FROM steps WHERE step_type=15` in the conversation db.
