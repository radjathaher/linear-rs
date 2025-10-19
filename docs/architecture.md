# linear-rs Architecture & Specification

## Goals
- Provide a Rust-based terminal experience for Linear that includes both a traditional CLI and an interactive Ratatui TUI.
- Centralize API, authentication, and domain logic in a shared crate so both front-ends stay lightweight and consistent.
- Support multiple authentication strategies (browser-based OAuth 2.0 with PKCE, manual copy/paste fallback, personal API keys, and client-credential automation) while acknowledging the lack of a published device-code flow.
- Offer ergonomic, typed wrappers around Linear's GraphQL API to cover common issue, project, and workflow operations and expose escape hatches for custom queries.

## Workspace Layout
```
linear-rs/
├── Cargo.toml           # workspace manifest
├── docs/                # design docs, specs
├── crates/
│   ├── linear-core/     # shared library crate (auth, GraphQL, domain)
│   └── linear/          # CLI + TUI binary crate (clap-based, Ratatui subcommand)
```

### `linear-core`
- **Auth module** – Implements OAuth 2.0 Authorization Code flow with PKCE, including local loopback server capture and copy-paste fallback paths; manages refresh token rotation (enabled by default for apps created after 2025-10-01), and exposes personal API key and client-credential login helpers. Embedded a public client identifier and loopback redirect search across ports 9000-9999 on October 19, 2025 so zero-config browser logins just work.citeturn1search0
- `AuthManager` orchestrates flow selection, credential persistence, and refresh handling so front-ends only invoke high-level helpers (`authenticate_browser`, `authenticate_browser_auto_port`, `authenticate_manual`, `authenticate_api_key`, `authenticate_client_credentials`).
- **Token store** – Persists encrypted credentials in `$XDG_CONFIG_HOME/linear-rs/credentials.json` (or platform-specific directories). Prefer `keyring` for secure storage when available; fall back to filesystem with `chmod 600`.
- **GraphQL client** – Async wrapper around `reqwest` + `graphql_client` (or `cynic`) with request middleware for headers, retries, rate limiting, and structured error handling per GraphQL spec.citeturn2search0
- **Domain layer** – Strongly-typed service objects (e.g., `IssuesService`, `ProjectsService`) that expose ergonomic operations and hide pagination/connection details. Supports actor-scoped mutations via `actor=user/app` flags.citeturn1search6
- **Domain layer** – Strongly-typed service objects (e.g., `IssuesService`, `ProjectsService`) that expose ergonomic operations and hide pagination/connection details. Supports actor-scoped mutations via `actor=user/app` flags.citeturn1search6 `IssuesService` now wraps GraphQL issue queries with filter helpers so front-ends can request lists by team, state, assignee, labels, or title substring matches without crafting raw filters.
- **Configuration** – Loads workspace defaults (team filters, default view presets, UI preferences) and user profiles to enable multi-workspace switching.
- **Event pipeline** – Optional module for webhook ingestion or polling diffs to keep local caches in sync.

### `linear`
- Depends on `clap` derive for command tree (`linear auth login`, `linear auth logout`, `linear issue list`, `linear issue view <issue-key>`, `linear issue create`, `linear project list`, `linear cycle list`, `linear sync`, `linear tui`).
- `linear auth login` launches the browser automatically using embedded OAuth defaults, falls back to copy/paste when necessary, and still exposes `--manual` and `--api-key` switches for edge cases. Environment variables (`LINEAR_CLIENT_ID`, `LINEAR_REDIRECT_URI`, `LINEAR_CLIENT_SECRET`, `LINEAR_SCOPES`) optionally override the baked-in client if needed.
- `linear user me` surfaces the authenticated account via the GraphQL `viewer` query; `linear issue list/view` consume the shared GraphQL services for recent issues and detailed inspection, including friendly filters (`--team`, `--state`, `--assignee-id`, `--label-id`) that resolve human inputs to IDs.
- Issue detail output strips Markdown via `pulldown-cmark` and wraps long descriptions for terminal readability, keeping metadata aligned.
- `linear user me` surfaces the authenticated account via the GraphQL `viewer` query; `linear issue list/view` consume the shared GraphQL services for recent issues and detailed inspection, including friendly filters (`--team`, `--state`, `--assignee-id`, `--label-id`) that resolve human inputs to IDs. Metadata helpers (`linear team list`, `linear state list --team ...`) expose cached lookups for TUI/CLI reuse.
- Uses `linear-core` services; formatting handled with `owo-colors` or `colored`; supports JSON/YAML output for scripting.
- Implements interactive selection helpers (e.g., `fzf`-style search using `skimmer` when terminal supports raw mode).
- `tui` subcommand (migrated from standalone crate October 19, 2025) launches the Ratatui experience described below without requiring a separate binary.

### TUI module (within `linear`)
- Built on `ratatui` with `crossterm` backend; optional `ratatui-tree`/`ratatui-logger` widgets for navigation and diagnostics.
- Screen layout:
  - Left column: team and view filters.
  - Center: issues/projects list with infinite scroll (paginated via connection cursors).
  - Right panel: detail view with markdown rendering (using `tui-markdown` or custom viewer).
- Bottom command palette for quick actions (`:` to open, `?` for help).
- Command palette doubles as a command entry surface with history (↑/↓), supporting commands like `team <key>`, `state <name>`, `contains <text|clear>`, and `clear`; overlay echoes live input in the footer and shows recent history.
- Slash (`/`) provides a shortcut into the palette pre-populated with `contains` to quickly refine the title filter.
- `?` toggles an inline help overlay summarizing navigation and command shortcuts; `Esc` also closes it. The palette accepts `help` to open the same overlay from command mode.
- `view <issue-key>` (and helpers `view next/prev/first/last`) jump focus to a loaded issue and fetch its detail pane without switching context; palette suggestions surface matching issue identifiers and numeric indices as you type.
- Footer banner surfaces the active team/state/title filters so users always see which constraints drive the current issue list.
- Issue selection persists across refreshes when the chosen issue still exists in the new result set, keeping the detail panel stable.
- `page next/prev/<number>` (also `[`, `]` shortcuts) paginate through results and reuse cached pages for instant revisits; `page refresh` invalidates the cache for the current page. `c` clears all active filters instantly, while active `contains` filters highlight matching tokens inside the issue list for quick scanning.
- Async runtime (Tokio) plus `tokio::sync::mpsc` channel to integrate network calls without blocking the draw loop.
- Shares state management primitives (e.g., `AppContext`) with CLI to ensure consistent caching and authorization behavior.
- Initial scaffold fetches recent issues for the default profile using `IssueService`, renders them in a selectable list with keyboard navigation (`j/k`, arrows), shows team/state sidebars (`tab` to change focus, `t` cycle teams, `s` cycle states), supports a `:` command palette with history (↑/↓), refresh (`r`), and quit (`q`).

## Authentication Strategy
1. **Browser-based Authorization Code + PKCE (default)**  
   - CLI/TUI now attempts ports `9000-9999` on `127.0.0.1`, updating the redirect URI dynamically before opening the system browser (respecting `$BROWSER`).  
   - On redirect, the local listener exchanges `code` + PKCE verifier for tokens via `https://api.linear.app/oauth/token`.citeturn1search0  
   - If the browser cannot be launched (e.g., headless SSH) or every port is unavailable, the CLI automatically falls back to the manual copy/paste flow.  
   - Tokens stored alongside metadata; refresh tokens automatically rotated and refreshed when nearing expiration.

2. **Manual / remote flow**  
   - Exposed via `linear auth login --manual` or invoked automatically when browser launch/port binding fails.  
   - Prints the authorization URL (with PKCE challenge) and prompts the user to paste either the returned code or the full redirect URL.

3. **Personal API keys**  
   - Shortcut for individual use: prompt for API key, store with same credential pipeline, and apply `Authorization: <API_KEY>` header per request for backwards compatibility.citeturn2search0

4. **Client credentials**  
   - Service account flow remains available in `AuthManager` but is not exposed via CLI in the zero-friction UX (deferred until post-MVP automation work).citeturn1search0

> **Note:** Linear does not advertise an OAuth device authorization grant today; we rely on PKCE + browser hand-off or manual code entry for headless environments. Document this limitation prominently.citeturn1search0

## GraphQL Wrappers
- Default endpoint `https://api.linear.app/graphql` with JSON payloads.citeturn2search0
- Core client implements:
  - Request batching (optional) to send multiple operations per HTTP call.
  - Automatic pagination helpers returning iterators/streams over connection edges.
  - Strong typing via generated structs; manual `serde_json::Value` escape hatch for custom queries.
  - Error normalization (GraphQL `errors` array, HTTP status, rate-limit backoff).
  - Structured logging/tracing using `tracing` crate.
- Provide high-level operations:
  - `viewer`, `teams`, `cycles`, `projects`, `issues` (list/search by filters).
  - Mutations for issue create/update, comment create, project status updates.
  - Attachments support so users can add external links (e.g., GitHub PRs).
- Include CLI-friendly transforms (table rows) and TUI state models (list items, detail view models).

## Developer Experience
- `cargo fmt`, `cargo clippy --all-targets`, and `cargo test --all` wired via workspace default.
- Integration tests for GraphQL services use mocked HTTP responses (via `wiremock` or `httpmock`) to avoid hitting live API in CI.
- Add `README.md` quickstart plus `docs/auth.md` (detailed setup) and `docs/cli-commands.md`.
- Provide example configuration file under `examples/linear.toml`.

## Next Steps
1. Scaffold workspace: create `Cargo.toml` workspace and stub crates. ~~Wire `cargo xtask codegen`.~~ Removed October 19, 2025 as codegen crate/xtask runner no longer needed.
2. Implement shared auth module in `linear-core`:
   - Browser PKCE flow with loopback server.
   - Manual/no-browser fallback (print URL, paste code).
   - Investigate feasibility of device-code style flow; document fallback to PKCE if Linear continues to lack native support.
   - Token refresh + persistence (config/credential store).
   - Auto-detect preferred flow based on environment (`$DISPLAY`, TTY).
3. Stand up GraphQL client wrappers and schema codegen; cover `viewer` + issue list queries with mocked integration tests.
4. ~~Build initial CLI commands (`linear` crate): `auth login`, `issue list`, `issue view <id>`, `issue create`.~~ Done October 19, 2025 (`linear issue create` added with team/state resolution helpers).
5. Prototype `linear-tui` core loop: app skeleton, issue list panel, detail panel, key bindings (`j/k`, `/`, `q`).
6. Expand testing + docs: CLI help audit, README quickstart, developer docs, and ensure integration test coverage.
7. Push changes to `github.com/radjathaher/linear-rs` after each atomic milestone.

## Open Questions
- What rate limiting/backoff strategy best aligns with Linear’s API quotas?
- Do we need offline caching or sync primitives for working without connectivity?
