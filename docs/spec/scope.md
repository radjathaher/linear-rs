# Linear TUI Scope

## In Scope (Current Release)
- Issue-first terminal UI with sidebar filters, status tabs, and detail pane.
- Team, project, and workflow state filters (project cycling via `p`/`Shift+p`, status tabs `1-4`).
- Read-only overlays for projects (`o`), cycles (`y`), and help (`?`).
- Command palette with completion for `team`, `state`, `project`, `status`, paging, and issue navigation commands.
- CLI automation trigger (`Ctrl+Enter`) that executes `linear issue view <key>` using the active profile and surfaces completion status in the UI.

## Deferred / Out of Scope (Future Work)
- Creating or editing projects/issues directly inside the TUI (modal forms, quick editors).
- Bulk actions, label/assignee pickers, or arbitrary CLI command execution.
- Persistent user preferences (custom keymap, saved column widths) and multi-session state sync.
- Rich sub-issue visualization and inline timelines/activity feeds beyond current summaries.
- Offline caching, speculative writes, or multi-account switching inside a single session.

## Reference Notes
- Design inspirations and flow research: see `docs/research/lazygit.md` and `docs/research/lazydocker.md`.
- Screen flow diagrams and interaction breakdown live in `docs/spec/screen-flows.md`.
