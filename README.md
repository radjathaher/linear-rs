# linear-rs

Rust workspace providing a CLI and TUI for Linear project management.

## Prerequisites
- Rust 1.76+
- Linear API credentials: set `LINEAR_CLIENT_ID`, `LINEAR_CLIENT_SECRET` (optional for PKCE apps), and `LINEAR_REDIRECT_URI`

## CLI Usage
```
cargo run -p linear-cli -- auth login --browser
cargo run -p linear-cli -- issue list --team KEY --state-id STATE_ID
```
Key commands:
- `linear auth login` – OAuth login with browser/manual/API key options
- `linear issue list` – filter with `--team`, `--state`, `--assignee-id`, `--label-id`
- `linear team list`, `linear state list --team KEY`

## TUI Usage
```
cargo run -p linear-tui
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
