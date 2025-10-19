# linear-rs

Rust workspace providing a CLI and TUI for Linear project management.

## Prerequisites
- Rust 1.76+
- Linear API credentials: set `LINEAR_CLIENT_ID`, `LINEAR_CLIENT_SECRET` (optional for PKCE apps), and `LINEAR_REDIRECT_URI`

## CLI Usage
```
cargo run -p linear -- auth login --browser
cargo run -p linear -- issue list --team KEY --state-id STATE_ID
cargo run -p linear -- issue create --team KEY --title "New issue" --description "Details"
```
Key commands:
- `linear auth login` – OAuth login with browser/manual/API key options
- `linear issue list` – filter with `--team`, `--state`, `--assignee-id`, `--label-id`, `--contains`
- `linear issue create` – create an issue with optional `--description`, `--state`, `--assignee-id`, `--label-id`, `--priority`
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
- `:` open command palette (history with ↑/↓, commands: `team <key>`, `state <name>`, `clear`, `reload`)

CLI issue detail output strips basic Markdown (via `pulldown-cmark`) and wraps descriptions to 80 characters for readability.

## Development
- `cargo fmt`, `cargo clippy --workspace`
- `cargo check` runs quickly across all crates
