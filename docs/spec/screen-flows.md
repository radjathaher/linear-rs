# Screen Flows

## Philosophy
- Issue-first workspace; teams/projects/status act as optional filters.
- Detail pane always visible, shows sub-issue tree and actions.
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
│ Saved Filters                             │ Details Pane (tabs: Summary | Description | Activity | Sub-issues) │
├────────────────────────────── Bottom Strip ─────────────────────────────────────────────────────────┤
│ Active Filters | Mode Indicator | Status Spinner | Keymap Legend / Palette Prompt                   │
└────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

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
- Detail pane: `Tab` cycles Summary/Description/Activity/Sub-issues; `x` expand sub-issue tree (planned refinement).
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

## Structure & Files
- `tui/app.rs`: application state, filter cache, palette/overlay orchestration.
- `tui/runner.rs`: terminal lifecycle + event loop.
- `tui/view/`:
  - `mod.rs`: root render + layout orchestration.
  - `filter_bar.rs`, `sidebar.rs`, `workspace.rs`, `bottom.rs`, `palette.rs`, `overlays.rs`, `util.rs`.
- `tui/mod.rs`: module wiring, re-export of `run`.
- Future: `tui/commands.rs` & `tui/resources/` for command parsing + data access.
