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
- `conversation.rs` / `activity.rs` / `conversation_view.rs` / `markdown/` —
  shared conversation timeline primitives under the ChatGPT process contract
  (2026-09-06): text never folds (narration is the visible progress report),
  thinking consolidates into one collapsible block per turn ("Thought for Xs"
  from timestamps; Chat streams its tail live and auto-collapses when the
  turn starts talking), tools render as compact chip rows with only a running
  spinner or failure mark (runs of 2+ consecutive calls collapse into one
  expandable ToolGroup, possibly spanning messages); each settled turn closes
  with a response footer. `conversation.rs` owns
  the pure row projection; `activity.rs` owns tool rows / footers;
  `conversation_view.rs` owns User / thinking block / Working styling;
  `markdown/` is the one answer-body engine (C3/C4).

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
