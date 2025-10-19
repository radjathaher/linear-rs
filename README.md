# linear-rs

Rust workspace providing a CLI and TUI for Linear project management.

## Prerequisites
- Rust 1.76+
- Optional: Linear API overrides via `LINEAR_CLIENT_ID`, `LINEAR_CLIENT_SECRET`, `LINEAR_REDIRECT_URI`, or `LINEAR_SCOPES` if you need a custom OAuth app

## CLI Usage
```
cargo run -p linear -- auth login
cargo run -p linear -- issue list --team KEY --state-id STATE_ID
cargo run -p linear -- issue create --team KEY --title "New issue" --description "Details"
```
Key commands (see `docs/cli.md` for the full tree):
- `linear auth login` – OAuth login with browser/manual/API key options
- `linear issue list` – filter with team/state/assignee/label/contains flags plus pagination
- `linear issue update`, `linear issue close`, `linear issue comment`, `linear issue delete --yes`
- `linear project list|create|update|archive` – manage project metadata with sorting & filters
- `linear cycle list|update` – inspect iterations per team
- `linear label list|create|update --team-id TEAM`
- `linear team list`, `linear state list --team KEY`
- `linear tui` – launches the interactive interface without a separate binary

## TUI Usage
```
cargo run -p linear -- tui
```
Keys:
- `r` refresh, `q` quit
- `Tab` cycle focus between teams, states, issues
- `j/k` navigate within focused list
- `t`/`s` cycle team/state filters
- `:` open command palette (history with ↑/↓). Useful commands: `team <key>`, `state <name>`, `project <name|next|prev|clear>`, `status <todo|doing|done|all>`, `activity`, `sub-issues`, `detail <tab>`.
- `p` toggle the projects overlay (fetches latest projects)
- `y` toggle the cycles overlay (uses selected team when available)
- `?` open contextual help; `/` filter issues by title snippet
- `.` / `,` cycle detail tabs (Summary, Description, Activity, Sub-issues); tab choice is remembered per issue

Detail pane highlights:
- Activity tab merges comments and change history into a chronological timeline with local timestamps.
- Sub-issues tab renders a nested tree showing state, assignee, priority, and team for each child issue.

CLI issue detail output strips basic Markdown (via `pulldown-cmark`) and wraps descriptions to 80 characters for readability.

## Development
- `cargo fmt`, `cargo clippy --workspace`
- `cargo check` runs quickly across all crates
