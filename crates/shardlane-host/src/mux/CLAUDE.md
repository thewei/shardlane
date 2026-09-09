# mux/
> L2 | Parent: ../../CLAUDE.md

Backend-neutral Multiplexer API — the macOS shell and the Remote API consume
runtime state only through the traits in this module; backend-specific
knowledge (socket shapes, CLI spellings, protocol gates, child spawns, wire
parsing) lives exclusively inside each adapter file, upper layers never branch
on backend identity, and capability differences flow only through
MuxCapabilities and the Option accessors.

Members:
- mod.rs — the neutral contract: Multiplexer / MultiplexerConnection /
  MultiplexerStream / MuxAgentRuntime traits, MuxCapabilities,
  InstanceRef/InstanceTarget, the HerdrError <-> MuxError mapping, and domain
  parameter types; every capability field must have a named degradation
  consumer (docs/multiplexer-api.md §7).
- registry.rs — the MuxRegistry assembly point: the only place that names
  backends; with_builtins() registers the four builtin adapters (herdr, tmux,
  uuyc, luvus); registration performs no I/O.
- kit.rs — the adapter contract suite (facet/capability coherence, snapshot
  invariants, structural round trips, event contract, registry routing); every
  new backend must pass the kit.
- herdr.rs — Herdr adapter #1: wraps the crate::herdr HerdrClient /
  HerdrTuiSession with all capabilities on; the sole provider of the
  as_herdr() escape hatch.
- tmux.rs — the tmux adapter: CLI enumeration plus the tmux attach child
  stream; agents/server_admin degraded, events_push=false.
- uuyc.rs — the uuyc-cli lterm adapter: session enumeration and an attach
  stream over its internal tmux daemon.
- luvus.rs — the Luvus adapter (UHP 1.0, newline-delimited JSON-RPC over a
  per-session Unix socket): session enumeration via luvus session list --json,
  full workspace/tab/pane/agent projection mapping, the events.subscribe
  stream with client-side pane filtering, and the luvus TUI attach child
  stream (LUVUS_SOCKET_PATH points at the per-session socket); send_keys is
  encoded to PTY bytes client-side; display-name overrides live under
  ~/.config/shardlane/luvus-instances/. Wire shapes verified live against
  luvus 0.13.4.

[PROTOCOL]: Update this header on change, then check CLAUDE.md.
