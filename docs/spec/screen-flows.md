# Screen Flows

## Philosophy
- Issue-first workspace; teams/projects/status act as optional filters.
- Detail pane always visible, shows sub-issue tree and actions.
- Activity tab merges comments and change history into a timeline grouped by day.
- Sub-issues tab renders a nested tree for quick parent/child navigation.
- Project creation/edit launched on demand (modal) without leaving issues context.
- Automation agent runs Linear CLI commands from active issue/project (`Ctrl+Enter`).

## Main Interface
```
┌────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Filter Bar: Team Selector | Project Selector | Status Tabs (Todo/Doing/Done/All) | Saved Filters    │
├──────────── Sidebar 24 cols ─────────────┬──────────────────────────── Issues Workspace ────────────┤
│ Team List (filterable)                   │ Issues List (team+project+status filtered, sorted)       │
│ Project List (per team)                  │ ├─ Inline sort row: priority | assignee | updated         │
│ Status Overview                          │ └─ Preview summary for highlighted issue                 │
│ Saved Filters                             │ Details Pane (tabs: Summary | Description | Activity timeline | Sub-issues tree) │
├────────────────────────────── Bottom Strip ─────────────────────────────────────────────────────────┤
│ Active Filters | Mode Indicator | Status Spinner | Keymap Legend / Palette Prompt                   │
└────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Detail Tabs
- **Summary** – Identifier, state, assignee, labels, team, timestamps, and external URL for the selected issue.
- **Description** – Markdown body wrapped to the viewport; displays "(no description)" when the field is empty.
- **Activity** – Combined stream of newest comments and change history, grouped by calendar day with tree branches and local timestamps.
- **Sub-issues** – Recursive ASCII tree of child issues showing state, assignee, priority, and team; palette `sub-issues` jumps straight here and the active tab is remembered per issue.

### States & Overlays
- Command palette replaces keymap column with prompt, suggestions, history.
- Projects overlay: read-only list scoped to current team, toggled with `o`.
- Issue quick editor flyout: planned enhancement for inline issue edits.
- Help overlay: grouped keymap reference.

## Interaction Model
- Focus order: Issues list (default) → Filter bar selectors → Sidebar.
- Team selector: `t`/`Shift+t` cycle teams, `/` filter via palette.
- Project filter: `p` next, `Shift+p` previous, `Ctrl+p` clear, `o` toggles the project overlay.
- Status tabs: `1` Todo, `2` Doing, `3` Done, `4` All, `Ctrl+[` / `Ctrl+]` cycle tabs.
- Issues list: `j/k` move, `Enter` or palette `view` commands open details, `a` assign, `s` change state, `l` labels, `.` more actions.
- Detail pane: `.` next tab, `,` previous tab; palette `detail <tab>` plus shorthands `activity` / `sub-issues` jump directly when data is loaded.
- CLI automation: `Ctrl+Enter` triggers the Linear CLI helper stub for the focused issue.
- Global: `:` command palette, `?` keymap overlay, `R` refresh, `c` clear filters, `y` cycles overlay.

## Common Flows
### Inspect & Update Issue
```
[Filters] -> [Issues List] -> (select issue) -> [Detail Tabs]
                               |--(s)--> Update Status
                               |--(a)--> Assign User
                               |--(Ctrl+Enter)--> CLI Automation stub (issue command)
                               '--(x)--> Expand Sub-issues -> Inline actions
```
1. Use filters (team/project/status) if needed.
2. Navigate issues with `j/k`, open detail (`o`).
3. Update status (`s`) or assign (`a`), adjust sub-issues (`x` to expand, `.` actions).
4. Trigger CLI automation (`Ctrl+Enter`) for scripted tasks (e.g., create sub-issue).

### Cycle Project Filter
```
[Main Screen] --p--> Next Project Filter --> [Issue Reload]
    -> Shift+p previous project -> Ctrl+p clear -> o opens overlay snapshot
```
1. Use `p` / `Shift+p` to iterate through projects scoped to the active team.
2. Press `Ctrl+p` to clear the project constraint and return to "All" issues.
3. `o` opens the read-only project overlay for a broader view without changing the filter.

### Review Cycles
```
[Main Screen] -> Apply Team Filter -> press y -> [Cycles Overlay]
    -> Inspect Timeline -> Esc -> Return to Issue List (filters intact)
```
1. Apply team filter, open cycles overlay (`y`).
2. Inspect timeline, close (`Esc`), continue issue triage.

### Review Activity Timeline
```
[Issue Detail] --activity--> Grouped Activity Timeline
    -> Scroll history/comments -> return with other tabs
```
1. Use palette `activity` (or `detail activity`) after the issue detail loads.
2. Activity tab stitches change history and comments, grouped by calendar day with `├─/└─` connectors.
3. Switching away remembers the last tab per issue, so returning to the issue keeps the activity view.

### Inspect Sub-issue Tree
```
[Issue Detail] --sub-issues--> Nested Tree View
    -> Review children inline -> jump back with Summary/Description
```
1. Use palette `sub-issues` (or `detail sub-issues`) to jump directly into the tree.
2. Tree rows include identifier, title, state, assignee, and priority; children render recursively beneath parents.
3. Tab persistence keeps the sub-issue tree active the next time the issue is selected.

## Structure & Files
- `tui/app.rs`: application state, filter cache, palette/overlay orchestration.
- `tui/runner.rs`: terminal lifecycle + event loop.
- `tui/view/`:
  - `mod.rs`: root render + layout orchestration.
  - `filter_bar.rs`, `sidebar.rs`, `workspace.rs`, `bottom.rs`, `palette.rs`, `overlays.rs`, `util.rs`.
- `tui/mod.rs`: module wiring, re-export of `run`.
- Future: `tui/commands.rs` & `tui/resources/` for command parsing + data access.
