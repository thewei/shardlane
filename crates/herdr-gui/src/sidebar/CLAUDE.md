# sidebar/
> L2 | Parent: ../CLAUDE.md

The single canonical Sidebar implementation, mechanically split into a directory family; all submodules share the module-root namespace via `use super::*`.

Members:
- `shell.rs`: the main assembly layer — section order (Agents, Projects), section collapse state, footer instance switcher chip, and the visible-projects loop that consumes the projection.
- `rows.rs`: row primitives — `sidebar_row` (single-line), `sidebar_card_row` (two-line Agents cards), the `RowLead` models (including the service-badged `IconService` terminal lead), drag ghosts, and group headers; all rows integrate `RovingList` so keyboard navigation never depends on row height.
- `tree_rows.rs`: project and tab rows. Project rows carry a git identity trailing (branch + +/- counts) read from the per-project snapshot map refreshed in `shell_navigation.rs`; tab leads use the square-terminal glyph with a success service badge joined from observed services by `tab_id`.
- `service_rows.rs`: the Agents section's cards — live agents (title over project · model · context meta, insight meta only from the live subscribed session) and history backfill rows plus the "view more" trailing row.
- `pane_rows.rs`: per-Pane rows under an expanded Project.
- `projection.rs`: pure helpers deriving sidebar row inputs from the runtime projection.
- `section_layout.rs`: section stacking constants/helpers.

Rules: keep one Sidebar path; services live in the right panel (2026-09-03), so no Services section may reappear here; project git info and service badges are read-only projections of existing collectors, never new collectors per row.
