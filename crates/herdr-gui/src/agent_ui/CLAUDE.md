# agent_ui/ — Shared Agent conversation presentation primitives

The presentation layer shared by the three Agent conversation surfaces: New
Agent / History Detail / Live Chat. This module owns presentation primitives
only; lifecycles belong to each consuming surface.

## Ownership

- `composer.rs` — the **single** Agent input card implementation
  (`AgentComposer`): card geometry, input slot, attachment tile strip,
  control-row left/right slots, Send/Busy/Disabled send button, footer row
  under the card. Extracted bottom-up from `new_agent/page.rs` on 2026-08-27;
  New Agent is the first consumer, Live Chat the second.
- `conversation.rs` / `activity.rs` / `markdown/` — shared conversation
  timeline / activity / streaming Markdown primitives (C3/C4).

## Hard boundaries

- No Herdr calls are initiated (no `agent.prompt`, no launch/termination
  orchestration); send callbacks are injected by the caller.
- No ownership of the Project/Branch/provider/semantic sources of truth; the
  components render only the slots and data the caller supplies.
- No second Composer: New Agent's and Chat's input cards must be configured
  through `AgentComposer`; a geometry/interaction change lands once and takes
  effect in both places, and both sides' tests run together.
- No general-purpose component bucket; only primitives shared by the Agent
  conversation surfaces may enter this module.
