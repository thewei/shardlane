# Shardlane Remote Client Architecture

Status: **As-built semantic Conversation/History slice + shared Herdr TUI source landed; live/native acceptance in progress**
Baseline: 2026-08-22
Architecture source of truth: `client-product-architecture.md`
Implementation sequencing and acceptance gates live in the internal engineering archive (not published).

This document defines the approved remote/mobile direction for Shardlane. The
semantic Conversation/History Host and Remote API v2 slice is now present and
covered by golden fixtures; pairing, relay infrastructure, and native remote
TUI remain future work or explicitly gated below.

### 2026-08-29 protocol probe (amended 2026-08-30)

The installed Herdr 0.8.2/protocol-20 API has global focus fields and runtime
object focus/resize methods, but no client-scoped TUI isolation. Product accepts
that shared/global behavior. The Host therefore owns one bounded shared TUI
session for authenticated viewers; semantic Mobile navigation remains local,
while explicit TUI input/focus/resize may affect the Mac TUI. The evidence and
remaining lifecycle gates are recorded in `remote-tui-protocol-probe-2026-08-29.md`.

### 2026-08-30 semantic convergence checkpoint

Host/Core now owns the single Conversation projection used by both Live and
History surfaces. The v2 product API exposes bounded, opaque-ID resources:

```text
GET  /api/v2/projects/{project_id}/conversations
GET  /api/v2/agents/{agent_ref}/conversation
GET  /api/v2/conversations/{conversation_id}
GET  /api/v2/conversations/{conversation_id}/window
POST /api/v2/conversations/{conversation_id}/prompt
POST /api/v2/conversations/{conversation_id}/continue
GET  /api/v2/history/search
```

Shared Herdr TUI (explicit Terminal surface):

```text
POST   /api/v2/tui/session
GET    /api/v2/tui/session/{id}
POST   /api/v2/tui/session/{id}/input
POST   /api/v2/tui/session/{id}/resize
DELETE /api/v2/tui/session/{id}
GET    /api/v2/tui/session/{id}/stream   (authenticated WebSocket)
```

The TUI endpoint returns one opaque session per Host. Stream `output` frames
carry base64 PTY bytes for a real VT renderer; no screen or Chat code parses
ANSI, and reconnect attaches to the same session rather than spawning another
Herdr process.

`POST /api/v2/tui/session/{id}/input` accepts the legacy renderer payload
`{"data":"..."}` and the preferred typed events:

```json
{"kind":"text","text":"hello"}
{"kind":"paste","text":"..."}
{"kind":"key","code":"arrow_up","modifiers":{"ctrl":true,"alt":false,"shift":false,"meta":false}}
```

Live identity is derived only from Herdr's typed `AgentSessionInfo`; History
uses the read-only catalog's exact provider/native-session locator. Continue is
Host-owned and verifies `Continue/Fork -> readiness -> identity -> one prompt`.
The v1 routes remain compatibility-only while clients migrate. Mobile's
default Agent and History screens render one shared semantic
`ConversationSurface`; they never parse `agent.read`, provider files, sockets,
or ANSI output.

## 1. Goal

Shardlane must support a mobile client, initially React Native, that can connect to a Shardlane instance running on a Mac and operate the same Projects, Agents, Tasks, Tabs, Panes, Terminal sessions, and History without turning the mobile app into a second runtime or a remote-desktop clone.

The primary mobile use case is:

1. connect to a Mac running Shardlane;
2. choose a Shardlane Workspace and Project;
3. inspect active Agents and their state;
4. send semantic prompts to an Agent and continue development work;
5. open the capability-gated Host-owned shared Herdr TUI (global focus/resize
   effects are disclosed; there is no Pane-terminal fallback);
6. run or inspect Tasks;
7. reconnect after network/background interruption without disturbing the Mac desktop view.

Remote access must work on a local network and, later, over the public internet through NAT/CGNAT/firewalls without requiring Shardlane to expose the Herdr Unix socket or invent a second terminal runtime.

## 2. Architectural boundary

The target model is:

```text
                         ┌────────────────── macOS GPUI Client
                         │
Herdr Runtime ← Shardlane Host/Core
                         │
                         └────────────────── Remote API / Protocol
                                                │
                                  ┌─────────────┴─────────────┐
                                  │                           │
                              Direct Transport            Relay Transport
                                  │                           │
                             Mobile Client                Mobile Client
```

Ownership remains unchanged:

- **Herdr** is the only runtime authority for runtime workspaces, Tabs, Panes, layouts, terminal sessions, Agents, scrollback, PTYs, and process lifecycle.
- **Shardlane Host/Core** owns Shardlane Workspace configuration, Project projection, Task semantic definitions, History projection/indexing, remote-device authorization, transport-independent application services, and remote-client DTO/event projection.
- **macOS GPUI** and **React Native mobile** are clients of the same Shardlane application-service boundary. Neither owns runtime state.

A remote client must never call the Herdr Unix socket directly.

## 3. Host, not GUI remote control

Remote access is an application API, not GUI automation.

The mobile app must not:

- send synthetic clicks to GPUI;
- depend on desktop Sidebar state;
- depend on a desktop Dialog being open;
- call `ShardlaneApp` presentation methods as a service layer;
- treat the currently focused Herdr Pane as an implicit target;
- mirror a desktop split layout as the primary mobile Terminal UI.

Instead, both desktop and mobile issue explicit intents to a shared Host/Core service layer:

```text
macOS GPUI ─┐
            ├─> Shardlane Application Services ─> Herdr / Shardlane-owned stores
Remote API ─┘
```

This requires extracting orchestration that is currently coupled to `ShardlaneApp`, especially New Agent, Task lifecycle, Workspace membership operations, and History continuation.

## 4. Client-local navigation

Every client owns its own navigation state.

Example:

```text
Mac client:    Workspace A / Project A / Tab 1 / Pane 1
Mobile client: Workspace A / Project B / Agent 5 / Pane 8
```

Selecting Project B on mobile must not move the Mac UI to Project B.

Remote commands therefore use explicit opaque identifiers:

- `workspace_id`
- `project_id`
- `tab_id`
- `pane_id`
- `agent_id`
- `task_id`
- `conversation_id`

The remote protocol must avoid implicit operations such as `current_pane`, `focused_tab`, or `active_project` except for client-local convenience state.

A future explicit action such as **Show on Mac** may intentionally route through the existing desktop focus/navigation path.

## 5. Product API, not Herdr passthrough

Do not expose a generic endpoint such as:

```text
POST /api/herdr/call
{ "method": "pane.send_text", ... }
```

That would make the mobile contract a network-exposed copy of the Herdr ABI.

The remote contract must describe Shardlane product semantics and map internally to Herdr APIs.

Examples:

```text
Create Agent
Prompt Agent
Read Agent output
Interrupt Agent
List Project Tabs
Observe Pane
Send Pane text
Start Task
Restart Task
Continue Conversation
```

Herdr protocol changes must remain behind the Host/Core integration boundary.

## 6. Agent-first mobile control

Agent semantics are the primary mobile control surface.

The installed Herdr 0.8.2 protocol schema was verified on 2026-08-22 to expose at least:

- `agent.start`
- `agent.prompt`
- `agent.read`
- `agent.wait`
- `agent.send_keys`
- `pane.send_text`
- `pane.send_keys`
- `pane.read`

Shardlane should add typed wrappers for the relevant Agent APIs before implementing mobile remote control.

Preferred command path:

```text
Mobile Prompt Composer
    ↓
Shardlane AgentService.prompt(...)
    ↓
Herdr agent.prompt
```

Terminal key simulation is a secondary escape hatch, not the normal way to talk to a coding Agent.

## 7. Terminal remote model

Remote TUI uses one Host-owned shared session because the installed Herdr
protocol has global focus/resize rather than client-local isolation. `herdr_tui`
is advertised only with the Host lifecycle implementation; Mobile has no normal
Pane-terminal product path and must not use a per-Pane terminal as a fallback.
The remaining acceptance is attach/stream/input/resize/reconnect/close plus a
real VT renderer on Android.

The installed Herdr 0.8.2 CLI still exposes the historical
`terminal session observe` and `terminal session control --takeover` commands,
but the current Remote adapter does not expose them as a generic Pane API. It
owns one shared TUI bridge/PTY, and every authenticated viewer attaches to that
same session. This avoids a second takeover controller per Pane while preserving
Herdr's global focus/resize semantics. The implementation must still be
operator-tested against the live runtime before claiming the lifecycle gate
complete.

## 8. Remote protocol shape

The Host/Core interface is transport-independent. Network adapters expose it using versioned DTOs and events.

Recommended direct adapter:

- HTTPS/JSON for bounded query and command operations;
- WebSocket for events and streaming updates;
- one bootstrap snapshot after connect/reconnect;
- explicit protocol and capability negotiation.

Example logical endpoints:

```text
GET  /api/v2/hello
GET  /api/v2/bootstrap
GET  /api/v2/projects/{project_id}/conversations
GET  /api/v2/agents/{agent_ref}/conversation
GET  /api/v2/conversations/{conversation_id}
GET  /api/v2/conversations/{conversation_id}/window
POST /api/v2/conversations/{conversation_id}/prompt
POST /api/v2/conversations/{conversation_id}/continue
GET  /api/v2/history/search
GET  /api/v1/events   # WebSocket upgrade; carries v2 semantic invalidation events
```

The v2 URL set above is the current semantic contract; the older v1 routes
remain compatibility-only while clients migrate.

### 8.1 Bootstrap contract

`bootstrap` should provide the bounded initial projection needed to render mobile UI without replaying a large event log:

- Host identity and version;
- Remote API version/capabilities;
- authorized-device/session metadata;
- Shardlane Workspaces;
- Project summaries;
- active Agent summaries;
- Task summaries;
- connection/runtime health.

After WebSocket loss, V1 reconnect behavior is:

1. reconnect transport;
2. authenticate;
3. fetch bootstrap;
4. replace disposable client projection;
5. resubscribe to events.

Do not build durable event replay before a real need exists.

### 8.2 Event model

Events are state-change hints, not a second source of truth.

Initial event families:

- `workspace.updated`
- `project.updated`
- `tab.updated`
- `pane.updated`
- `agent.updated`
- `agent.output_available`
- `task.updated`
- `history.updated`
- `host.health_changed`

Events should contain stable IDs and enough change metadata to avoid unrelated full refreshes. A client may always recover by refetching the bounded authoritative projection.

## 9. Connection/transport options

The Shardlane protocol must not be coupled to one network transport. `Transport` is a replaceable connection layer below the application contract.

### 9.1 Local LAN HTTPS/WSS

**Use:** normal same-network connection.

Flow:

```text
Mobile ──HTTPS/WSS──> Mac Shardlane Host
```

Discovery can use Bonjour/mDNS; authorization still uses Shardlane pairing/device credentials.

Advantages:

- lowest conceptual overhead;
- no SSH dependency;
- low latency;
- excellent local developer/user experience.

Limits:

- LAN only;
- multicast discovery does not cross the public internet;
- iOS local-network privacy/Bonjour declarations are required when this path is implemented.

**Decision:** approved as the preferred local transport.

### 9.2 SSH port forwarding

**Use:** developer fallback and advanced/manual remote access.

Flow:

```text
Mobile ──SSH tunnel──> Mac localhost:Shardlane-API
```

Advantages:

- mature encryption and host-key model;
- Host API can remain loopback-only;
- useful during development and for machines already reachable by SSH.

Limits:

- raw SSH still needs the Mac to be reachable somehow;
- home NAT/CGNAT frequently prevents inbound SSH without router/VPN setup;
- user/SSH key/host-key UX is too infrastructure-oriented for the default mobile product;
- embedding and maintaining SSH clients on iOS/Android increases native complexity.

**Decision:** keep, but demote from primary product transport to developer/advanced fallback.

### 9.3 Tailscale/WireGuard overlay

**Use:** preferred early public-internet remote path for development, dogfood, and private beta.

Current Tailscale behavior is a strong fit for Shardlane's topology: devices try direct UDP peer-to-peer connectivity through NAT traversal; if direct connectivity fails they can fall back to peer relay and then DERP relay, while traffic remains WireGuard-encrypted. Stable tailnet addresses also avoid DDNS/router configuration.

Advantages:

- works across most NAT/CGNAT/firewall situations without opening a public Shardlane port;
- usually reaches a direct peer-to-peer path;
- automatic relay fallback;
- stable private addressing;
- Shardlane still runs its normal HTTP/WebSocket application API above the overlay network.

Limits:

- both devices must install/join Tailscale and satisfy tailnet policy;
- unsuitable as the permanent default consumer onboarding experience;
- third-party account/product dependency if made mandatory.

**Decision:** preferred Phase-1 public remote transport for internal/private beta; never make Shardlane domain semantics depend on Tailscale.

References:

- https://tailscale.com/docs/reference/connection-types
- https://tailscale.com/docs/reference/derp-servers
- https://tailscale.com/docs/how-to/connect-to-devices

### 9.4 Direct public HTTPS/WSS with port forwarding/DDNS

**Use:** optional expert configuration only.

Advantages:

- no overlay provider;
- simple protocol once public reachability exists.

Limits:

- router port forwarding;
- CGNAT can make it impossible;
- dynamic IP/DDNS handling;
- increases the Internet-facing attack surface;
- certificate lifecycle and firewall support become user problems.

**Decision:** do not use as the default public-remote route.

### 9.5 Generic outbound tunnels such as Cloudflare Tunnel

Cloudflare Tunnel demonstrates the useful topology: a Host establishes an outbound-only encrypted tunnel, requiring neither a public IP nor inbound ports.

Advantages:

- works behind NAT/CGNAT;
- no inbound firewall configuration;
- operationally simple for web/API traffic.

Limits for Shardlane core product:

- external account/vendor dependency;
- public/private access policy must be configured correctly;
- product identity, pairing, relay authorization, and mobile lifecycle still need Shardlane semantics;
- using a generic tunnel as the permanent core would push product control into third-party infrastructure.

**Decision:** acceptable as a development/advanced integration, not the canonical Shardlane remote architecture.

Reference: https://developers.cloudflare.com/tunnel/

### 9.6 WebRTC DataChannel + ICE/STUN/TURN

WebRTC can carry arbitrary data peer-to-peer. ICE gathers connectivity candidates through STUN/TURN, but the peers still need a separate signaling service, and difficult networks require TURN relay.

Advantages:

- mature NAT traversal model;
- direct peer-to-peer when possible;
- relay fallback through TURN;
- good fit for interactive data.

Limits:

- signaling service is still required;
- TURN capacity/operations are still required for hard networks;
- React Native/native mobile integration is materially more complex than normal HTTPS/WebSocket;
- SDP/ICE lifecycle adds complexity that Shardlane's command/query/event protocol does not currently need.

**Decision:** not V1. Reconsider only if direct-path latency or peer-to-peer transport becomes a measured bottleneck.

References:

- https://webrtc.org/getting-started/peer-connections
- https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Connectivity

### 9.7 QUIC / HTTP/3

QUIC provides multiplexed secure streams and client connection migration, including continuity across some NAT rebinding/network changes.

Advantages:

- low-latency multiplexed streams;
- avoids TCP head-of-line behavior between independent streams;
- useful mobile network migration semantics.

Limits:

- QUIC alone does not solve peer discovery, NAT traversal, CGNAT reachability, authentication, or relay;
- UDP may be blocked and still needs a fallback strategy;
- adds implementation complexity before Shardlane has measured HTTP/WebSocket limits.

**Decision:** not a connectivity solution by itself. Keep it as a future transport optimization for direct or relay paths after profiling.

Reference: https://www.rfc-editor.org/rfc/rfc9000.html

### 9.8 Shardlane-owned outbound Relay

**Use:** long-term default public-internet product connection.

Target topology:

```text
Mac Shardlane Host ──outbound encrypted session──┐
                                                 ├─ Shardlane Relay
Mobile Shardlane Client ──outbound session───────┘
```

Neither endpoint requires an inbound public port. The relay resolves device discovery/reachability and carries traffic only for explicitly paired/authorized endpoints.

Long-term connection policy should be:

```text
Local direct if available
    ↓
Internet direct/optimized peer path if available
    ↓
Shardlane Relay fallback
```

V1 Relay does not need to implement direct P2P upgrade. A relay-first version is operationally simpler; direct path negotiation can be added only when latency/cost measurements justify it.

Security goal: the relay should not become a runtime/state authority. Prefer end-to-end session encryption between paired Host and Client so the relay only authenticates routing metadata and forwards ciphertext.

**Decision:** canonical long-term public-remote direction, but intentionally out of the first implementation milestone.

### 9.9 Host-served web client (same-origin PWA)

**Use:** zero-install onboarding and the approved web validation surface for the mobile client.

The mobile client is one Expo/React Native tree that also compiles to a static web bundle via react-native-web. The Host serves that bundle (`dist/`) from the same TLS listener that serves `/api/v2` (with v1 compatibility routes retained during migration):

```text
phone ── HTTPS (LAN TLS or Tailscale cert) ──▶ Host ─┬─ static dist/ (PWA: manifest, icons)
                                                     └─ /api/v2 (same-origin API)
```

Properties:

- same-origin removes CORS entirely — no allowlist to configure or retire at the transport layer;
- PWA install requires a secure context: acceptance runs over Tailscale HTTPS (or authenticated LAN TLS), never a bare LAN IP;
- the web build shares the mobile interaction contract unchanged (phone-width shell on desktop browsers); no responsive fork, no second product;
- the bundle is a packaging-time build input — the Host gains no web runtime ownership and remains the single server authority;
- service workers / offline caching are explicitly out of scope for the validation track.

Limits:

- requires the R8/R9 TLS listener to exist (dev loop before that uses the loopback API with a dev-only CORS allowlist);
- web credential persistence is weaker than native SecureStore (session-memory only), so pairing (R7) must treat web as a second-class credential surface until proven otherwise.

**Decision:** approved target topology for the web client; sequenced as W4 in the mobile web plan, landing inside the R8 window.

## 10. Recommended transport sequence

| Stage | Connection | Role |
| --- | --- | --- |
| Host foundation | loopback HTTP/WSS | API development and automated tests |
| Local MVP | LAN HTTPS/WSS + pairing | same-network product path |
| Public private beta | Tailscale overlay + HTTPS/WSS | preferred early remote path |
| Advanced fallback | SSH port forwarding | developer/manual recovery path |
| Product public remote | Shardlane outbound Relay | default remote user experience |
| Optimization only | WebRTC/QUIC/direct-upgrade | add after evidence |

This sequence lets remote functionality ship without waiting for cloud infrastructure while preserving the final architecture.

## 11. Pairing and device identity

Transport encryption is not sufficient as Shardlane authorization.

Every remote path must authenticate a Shardlane device/session identity.

Initial pairing model:

1. user enables Remote Access on Mac;
2. Mac creates a short-lived one-time pairing session;
3. Shardlane shows a QR code or equivalent transfer containing only bootstrap metadata required for secure pairing;
4. mobile verifies the Host identity and establishes a durable device credential;
5. mobile stores its credential in iOS Keychain / Android Keystore;
6. Mac stores only the minimum durable authorization record required to recognize/revoke the device;
7. Settings exposes device name, created/last-seen metadata, and Revoke.

The exact cryptographic handshake is implementation work. Do not encode long-lived bearer secrets directly into reusable QR codes.

### 11.1 Authorization model

V1 may use one trusted-owner permission level, but the protocol must keep authorization centralized so later scopes can be added without changing Herdr.

Potential later scopes:

- view status;
- prompt Agents;
- raw Terminal input;
- run Tasks;
- modify Tasks;
- read History;
- manage devices.

Raw Terminal input is more powerful than Agent prompting and should remain a distinct permission boundary even if V1 grants both to the owner.

## 12. Listener policy

Do not default to `0.0.0.0` with unauthenticated HTTP.

Host listener modes should be explicit:

- `off` — no remote service;
- `loopback` — API reachable only from the Mac; suitable for tests/SSH tunnel;
- `local_network` — authenticated TLS listener on approved local interfaces;
- `overlay` — authenticated listener on an explicitly selected overlay interface/address;
- `relay` — no inbound listener required; outbound Relay session only.

The UI should explain which mode is active and should not silently widen exposure when network interfaces change.

## 13. Mobile information architecture

Mobile is Agent-first, not desktop-first.

Recommended V1 surfaces:

1. **Hosts / Connection** — choose or reconnect to a paired Mac.
2. **Agents** — default operational home: Working / Waiting / Attention / Recent.
3. **Workspace picker** — compact switcher or bottom sheet.
4. **Project picker** — searchable bottom sheet.
5. **Agent detail** — status/output + persistent Prompt Composer.
6. **Project detail** — Agents / Tasks / Tabs / History entry points.
7. **Terminal/TUI** — capability-gated Host-owned shared session; when `herdr_tui=true`, no Pane picker or terminal fallback is exposed, and global focus/resize effects are disclosed.
8. **Tasks** — status and Start/Stop/Restart; editing can remain desktop-only initially.

Do not reproduce the macOS Sidebar, split-layout editor, drag interactions, or desktop Settings hierarchy on mobile.

## 14. Background and reconnect semantics

Direct-only V1 must assume the mobile OS may suspend networking.

Rules:

- Agent/process work continues on the Mac when mobile disconnects;
- the Host does not depend on a mobile socket to keep work alive;
- returning to foreground reconnects and bootstraps current state;
- stale client state is disposable;
- V1 does not promise remote push while the mobile process is suspended.

Reliable background notifications are a later Relay + APNs/FCM capability, not a reason to add cloud dependencies to the first Host API.

## 15. Data minimization

Remote DTOs should be purpose-built.

Do not automatically send:

- arbitrary environment variables;
- Herdr socket paths;
- full local configuration files;
- host filesystem contents;
- raw process environments;
- complete History transcripts in bootstrap;
- credentials/secrets contained in logs.

Project filesystem paths are host facts but should not be used as public protocol identifiers. Mobile receives an opaque `project_id` plus only the path/display metadata required by the feature.

History remains paginated/windowed and read-only, following the existing page-addressable cache architecture.

## 16. Source-of-truth rules

Remote support must preserve all current architecture invariants:

- Herdr remains the sole runtime authority.
- Shardlane Workspace remains client-owned configuration above Projects.
- Project remains a projection of a Herdr runtime Workspace, not a duplicated runtime record.
- Task semantic definitions remain Shardlane-owned until Herdr exposes protocol-native Tasks; Task execution remains Herdr-owned.
- History external sources remain read-only.
- client projections are disposable and rebuilt after reconnect.
- transport/relay servers never become owners of Project/Agent/Task runtime state.

## 17. Explicit non-goals for the first remote release

Do not implement in the first remote/mobile release:

- cloud account sync;
- permanent cloud persistence of terminal/history data;
- remote desktop/screen streaming;
- mobile multi-Pane split layout;
- mobile Workspace administration parity;
- cloud execution of Agents;
- a second PTY/runtime;
- generic Herdr RPC passthrough;
- custom NAT traversal before measuring the need;
- always-on background mobile socket guarantees.

## 18. Decision summary

Approved direction:

1. build a **Shardlane Host/Core** boundary before building mobile UI;
2. expose a **Shardlane product API**, never the raw Herdr protocol;
3. make **Agent semantic APIs** the primary mobile control path;
4. make client navigation independent and explicit-ID based;
5. use **LAN HTTPS/WSS** for local connection;
6. use **Tailscale overlay** as the preferred early public-remote path for dogfood/private beta;
7. retain **SSH port forwarding** as a developer/advanced fallback;
8. build a **Shardlane-owned outbound Relay** as the long-term default public-internet path;
9. treat WebRTC/QUIC as optional later transport optimizations, not prerequisites;
10. keep all connection-specific logic below a transport boundary so changing the network path never rewrites Workspace/Project/Agent/Task semantics.
