# CLI Reference

## Command Tree & Flags

```
linear
├─ auth
│  ├─ login [--api-key <key>] [--manual]
│  └─ logout [--profile <name=default>]
├─ user
│  └─ me [--profile <name>] [--json]
├─ issue
│  ├─ list [--profile <name>] [--limit <n>] [--after <cursor>]
│  │         [--team-key <key> | --team-id <id> | --team <name>]
│  │         [--state-id <id> | --state <name>] [--assignee-id <id>]
│  │         [--label-id <id>]... [--contains <text>] [--json]
│  ├─ view <KEY> [--profile <name>] [--json]
│  ├─ create --title <text> (--team <name>|--team-id <id>)
│  │         [--profile <name>] [--description <md>] [--assignee-id <id>]
│  │         [--state-id <id>|--state <name>] [--label-id <id>]...
│  │         [--priority 0-4] [--json]
│  ├─ update <KEY> [--profile <name>] [--title <text>] [--description <md>]
│  │         [--assignee-id <id>] [--state-id <id>|--state <name>]
│  │         [--label-id <id>]... [--clear-labels] [--priority 0-4]
│  │         [--project-id <id>] [--json]
│  ├─ close <KEY> [--profile <name>] [--restore] [--json]
│  ├─ delete <KEY> [--profile <name>] --yes
│  └─ comment <KEY> --body <md> [--profile <name>] [--json]
├─ project
│  ├─ list [--profile <name>] [--limit <n>] [--after <cursor>]
│  │         [--state <value>] [--status <value>] [--team-id <id>]
│  │         [--sort updated|created|target[:asc|:desc]] [--json]
│  ├─ create [--profile <name>] --name <text>
│  │         [--description <text>] [--state <value>]
│  │         [--start-date <YYYY-MM-DD>] [--target-date <YYYY-MM-DD>]
│  │         [--lead-id <id>] [--team-id <id>]... [--json]
│  ├─ update --id <id> [--profile <name>]
│  │         [--name <text>] [--description <text>] [--state <value>]
│  │         [--start-date <YYYY-MM-DD>] [--target-date <YYYY-MM-DD>]
│  │         [--team-id <id>]... [--lead-id <id>] [--json]
│  └─ archive --id <id> [--profile <name>] [--restore] [--json]
├─ cycle
│  ├─ list [--profile <name>] [--team-id <id>] [--state <value>]
│  │         [--sort start|end[:asc|:desc]] [--limit <n>] [--after <cursor>] [--json]
│  └─ update --id <id> [--profile <name>] [--name <text>]
│            [--start-date <YYYY-MM-DD>] [--end-date <YYYY-MM-DD>]
│            [--state <value>] [--json]
├─ label
│  ├─ list --team-id <id> [--profile <name>] [--json]
│  ├─ create --team-id <id> --name <text>
│  │         [--profile <name>] [--description <text>] [--color <#hex>] [--json]
│  └─ update --id <id> [--profile <name>] [--name <text>]
│            [--description <text>] [--color <#hex>] [--json]
├─ team
│  └─ list [--profile <name>] [--json]
├─ state
│  └─ list --team <name|id> [--profile <name>] [--json]
└─ tui [--profile <name>]
```

## Requests & Responses

| Command | GraphQL operation | Response |
| --- | --- | --- |
| `issue list` | `issues(first, filter, after)` | Paginated issue summaries + `pageInfo` |
| `issue view` | `issue(id)` | Full issue detail (state, assignee, labels, team, timestamps) |
| `issue create` | `issueCreate(input)` | Created issue detail or user errors |
| `issue update` | `issueUpdate(id, input)` | Updated issue detail |
| `issue close` | `issueArchive(id, archive)` | Archived/restored issue detail |
| `issue delete` | `issueDelete(id)` | Boolean success |
| `issue comment` | `commentCreate(input)` | Comment body, author, timestamps |
| `project list` | `projects(first, filter, orderBy, after)` | Project summaries + pagination |
| `project create` | `projectCreate(input)` | Project detail (teams, lead, dates) |
| `project update` | `projectUpdate(id, input)` | Updated project detail |
| `project archive` | `projectArchive(id, archive)` | Project detail showing new state |
| `cycle list` | `cycles(first, filter, orderBy, after)` | Cycle summaries for team/org |
| `cycle update` | `cycleUpdate(id, input)` | Cycle summary including state/date span |
| `label list` | `issueLabels(filter)` | All labels for a team |
| `label create` | `issueLabelCreate(input)` | New label (id, name, color) |
| `label update` | `issueLabelUpdate(id, input)` | Updated label |
| `team list` | `teams` | Team id/key/name collection |
| `state list` | `team.states` | Workflow states per team |
| `user me` | `viewer` | Authenticated user metadata |

All list commands honour pagination via `--limit` and `--after`. Sorting is exposed for issues (updated desc default), projects (`updated|created|target` × `asc|desc`), and cycles (`start|end` × `asc|desc`). Filtering flags map directly onto GraphQL filter objects (e.g. `--team-id` translates to `team.id` equality filters).

## TUI Keymap

The TUI mirrors CLI capabilities for day-to-day triage:

```
Navigation  j/k or arrows move selection        Refresh     r reload issues
Focus       tab cycles issues→teams→states      Filters     / contains filter
Paging      ] next page  [ previous             Teams       t cycle team filter
States      s cycle state filter                Jump        view next/prev/first/last/<key>
Palette     : command mode                      Help        ? toggle overlay / Esc to close
Projects    p fetch + overlay of recent projects
Cycles      y fetch + overlay of cycles for selected team
Misc        c clear filters   q/Esc quit
```

Projects and cycles overlays can be opened with `p` and `y`. Each overlay fetches the latest ten items and can be dismissed with the same key or `Esc`.

## Unimplemented Resources

The CLI/TUI now cover issues, projects, cycles, labels, teams, and workflow states. Remaining GraphQL resources that are not yet exposed include:

- Documents, comments on projects, and document collections
- Objectives, roadmaps, and milestones
- Integrations (GitHub, Slack, etc.) and automation recipes
- Any write APIs for user management or admin settings

Agent- and webhook-specific APIs remain intentionally out-of-scope.
