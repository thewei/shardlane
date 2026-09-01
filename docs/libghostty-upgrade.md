# libghostty Upgrade Guide (Shardlane Perspective)

> Conclusion first: **we can upgrade, but upgrading our library does not unlock the kitty clipboard**. That door is on the Herdr server side.
> This document = upgrade decision framework + operational runbook. Pin metadata lives in `vendor/ghostty-vt/PIN.md`.

## 1. What we vendored

`vendor/ghostty-vt/lib/libghostty-vt.a`: a pure VT library extracted from the ghostty
upstream (ghostling/libghostty-rs extraction line), statically linked into the shardlane
binary. 162 `ghostty_*` C-ABI symbols; the 21 `extern "C"` declarations in `ghostty.rs`
treat the **binary as the ABI authority** (signatures derived by disassembly + pinned by
behavioral regression tests). The current snapshot sits between Ghostty 1.3.1 and 1.4
(it embeds the `kitty_clipboard_protocol` parsing module).

## 2. Why "upgrading to the 1.4 libghostty" does not mean "the kitty clipboard works in a pane"

Ghostty 1.4's kitty clipboard protocol implementation lives in the **terminal emulator
data plane**:

```
app inside the pane (Claude Code) → PTY → [ghostty embedded in the herdr server: emulator owner]
                                            ↓ consumes OSC 52 / kitty clipboard query
                                            ↓ (today: dropped; verified not forwarded, never reaches a clipboard)
Shardlane ← rendered byte stream forwarded by the herdr controller → our vendored library (mirror parser)
```

The clipboard protocol is a bidirectional handshake: the app sends a query, and the
**emulator** must (a) write the reply to the PTY and (b) read/write the host clipboard.
Only the PTY owner (Herdr) can do either. Our library sits downstream of forwarded
bytes and never sees these sequences (isolated measurement on 2026-08-25: zero
forwarding for both OSC 52 terminators, BEL and ST; the kitty APC family likewise).

**Unlock path**: Herdr upgrades its embedded ghostty and wires up the host clipboard.
Downstream only consumes the outcome: after every Herdr upgrade, rerun the isolated
probe (script mode: start a herdr server on a temporary socket → `workspace.create` →
inject `ESC]52;c;<b64>` in both BEL and ST forms plus the kitty `ESC P + c` query into a
pane → observe the controller byte stream + `pbpaste` + the PTY reply). Once unlocked,
scope a client-side effort (the kitty_clipboard parsing already present in our library
becomes useful at that point).

## 3. When upgrading our own library is worthwhile

- Upstream fixed bugs that affect render correctness/parsing (wide chars, graphemes,
  long-tail SGR, etc.);
- A new extraction ABI appears that we want to sample (e.g. faster whole-frame export,
  incremental frames);
- The Herdr forwarding surface unlocks new sequences (bytes first, parsing second).

## 4. Upgrade runbook

See `vendor/ghostty-vt/PIN.md` §Upgrade Procedure (fetch and build the source → record
the pin → `nm` symbol diff + embedded layout JSON comparison + disassembly review of
externs → key-encoder behavioral regression + perf microbenchmarks → full gates +
smoke + lag log).
