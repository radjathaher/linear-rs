# linear-rs Architecture

## Goals
- Ship a fast, purely-terminal Linear client in Rust with both scripted (CLI) and interactive (TUI) entry points.
- Centralize authentication, GraphQL transport, and Linear domain modelling in a shared crate so front-ends stay thin.
- Provide broad CRUD coverage for day-to-day resources (issues, projects, cycles, labels, teams, workflow states) while keeping the escape hatch for custom GraphQL calls.

## Workspace Layout
```
linear-rs/
├── Cargo.toml           # workspace manifest
├── docs/                # architecture, CLI reference
├── crates/
│   ├── linear-core/     # shared library crate (auth + GraphQL + services)
│   └── linear/          # CLI + TUI binary crate
```

### `linear-core`

| Area | Responsibility |
| --- | --- |
| **Auth** | Consolidates OAuth2 PKCE, manual copy/paste fallback, and personal API key flows through `AuthManager`. Credentials are kept in a pluggable `CredentialStore` (filesystem-backed by default). |
| **GraphQL client** | Thin async client built on `reqwest`, targeting `https://api.linear.app/graphql`. It assembles raw queries/mutations and materialises strongly-typed structs (`IssueDetail`, `ProjectDetail`, `CycleSummary`, etc). Issue detail hydration also fetches recent comments, change history, and the nested sub-issue tree in one round trip. Error handling normalises HTTP failures, GraphQL errors, and deserialization issues into `GraphqlError`. |
| **Services** | Domain helpers wrap the raw client and add conveniences: |
| &nbsp; | • `IssueService` – list/filter issues, resolve team/state names, create/update/archive/delete issues, add comments, and surface richer detail payloads (history + sub-issues). |
| &nbsp; | • `ProjectService` – list projects with filter/sort, create/update/archive. |
| &nbsp; | • `CycleService` – list cycles for selected teams and update cycle metadata. |
| &nbsp; | • `LabelService` – list/create/update issue labels for a team. |
| **Data types** | GraphQL responses are mapped onto serde structs with camelCase field support and optional metadata (assignees, workflow state, teams, target dates, etc). All list responses preserve pagination info (`end_cursor`, `has_next_page`). |

### `linear`

| Component | Notes |
| --- | --- |
| **CLI** | Built with `clap` derive. Subcommands mirror the shared services (`issue`, `project`, `cycle`, `label`, `team`, `state`, `auth`, `user`). Every nested command has `--help`, JSON output toggles, and consistent pagination/filter/sort flags (see `docs/cli.md`). CLI flows are intentionally synchronous and surface friendly error messages. |
| **Output helpers** | When not in JSON mode, the CLI prints fixed-width tables and multi-line detail blocks with Markdown stripped via `pulldown-cmark`, matching terminal width where possible. |
| **TUI** | Ratatui-based dashboard showing issues, teams, and states. Enhancements in this iteration include: persistent keymap pane, `p` overlay for the latest projects, `y` overlay for cycles scoped to the selected team, command palette history, help overlays, an activity timeline (comments + history), and a nested sub-issue tree with palette shortcuts. Detail tab selection is remembered per issue so returning to an issue restores the previously viewed tab. |
| **Command dispatch** | `main.rs` translates parsed Clap args into service calls, performing any necessary ID resolution (e.g. translating team keys/state names to IDs before hitting GraphQL). |

## Request Flow
1. CLI/TUI loads credentials via `AuthManager`, ensuring a fresh `AuthSession`.
2. Front-end constructs a `LinearGraphqlClient` from the session.
3. Domain service prepares filters/order-by payloads and invokes the typed GraphQL method.
4. GraphQL client executes the HTTP POST, validates status, deserialises into envelopes, and bubbles GraphQL errors.
5. Service converts results into ergonomically shaped structs for rendering back to the user.

## Current Coverage vs Future Work

**Implemented**
- Issues: list, view, create, update (state/labels/priority/project), archive/restore, delete, comment.
- Projects: list (filter + sort), create, update, archive/restore.
- Cycles: list (per team, sorted), update.
- Labels: list/create/update per team.
- Metadata: teams, workflow states, authenticated viewer.
- TUI overlays for projects/cycles plus command palette & keymap window.

**Deferred / Out-of-scope**
- Documents, attachments, project roadmaps, milestones, objectives.
- Integration management (webhooks excluded by request, plus GitHub/Slack automations).
- Admin-level user management or settings mutations.
- Live sync/webhook ingestion (plan to revisit once webhook surface is re-enabled).

The codebase is structured so new resources can be added by extending `linear-core` with GraphQL queries/mutations, exposing them via a dedicated service module, and threading the new commands through the Clap/TUI layers.
