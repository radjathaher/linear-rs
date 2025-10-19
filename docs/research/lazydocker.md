# Lazydocker Screen Model

- **Runtime stack**: `gocui` views + `boxlayout` auto-layout (see `pkg/gui/layout.go`, `pkg/gui/arrangement.go`)
- **Primary regions**: side-column of list panels, central `main` panel, bottom status/info strip, transient popups
- **State drivers**: `Gui.State.ViewStack` (focus), `Gui.State.ScreenMode`, `Gui.State.Filter`, per-panel `ContextState`

## Layout + Responsiveness

- `getWindowDimensions` builds a `boxlayout.Box` tree: side column + main panel + optional bottom info strip.
- Side column orientation flips at ≤84 cols & >45 rows (“portrait” mode); otherwise stacked vertically with optional accordion (`ExpandFocusedSidePanel` flag).
- Screen modes cycle (`SCREEN_NORMAL`, `SCREEN_HALF`, `SCREEN_FULL`) via `+` / `_` and rebalance side vs main weights (`getMidSectionWeights`).
- Minimum terminal 10×9; below that only the `limit` overlay renders.
- Bottom strip is suppressed unless `ShowBottomLine` or active filter; components sized by content width.

## View Inventory

| View | Region | Purpose |
| --- | --- | --- |
| `project` | Side list | Project meta/logs; single-row list. Hidden? never. |
| `services` | Side list | Compose services (if compose project). |
| `containers` | Side list | Docker containers with filters/config-aware sorting. |
| `images` | Side list | Docker images. |
| `volumes` | Side list | Docker volumes. |
| `networks` | Side list | Docker networks. |
| `main` | Center | Tabbed detail view driven by selected side item. |
| `options` | Bottom | Keybinding hints (global/panel/popup aware). |
| `information` | Bottom | Version + donor CTA (mouse-click opens link). |
| `appStatus` | Bottom | Async status ticker (spinning loader). |
| `filterPrefix` + `filter` | Bottom | Inline filter prompt + editable buffer. |
| `menu` | Popup | Contextual command menu (`Menu()` helper). |
| `confirmation` | Popup | Modal confirmation/error/prompt. |
| `limit` | Overlay | Fullscreen warning when space too small. |

## Side Panels & Main Tabs

- Each side list is a `SideListPanel` (`pkg/gui/panels/side_list_panel.go`) with shared behaviors:
  - Filterable (`/`) unless `DisableFilter`.
  - `ContextState` registers main-panel tabs per item + cache key for re-render control.
  - Up/down (`j/k`/arrows/mouse), click focus, `Enter` focuses `main`, `[`/`]` cycle tabs.
- **Projects** (`project_panel.go`):
  - Tabs: compose `logs`, `docker-compose config`, `credits` (non-compose: `credits` only).
  - Single list entry (filter disabled).
- **Services** (`services_panel.go`):
  - Tabs: `logs`, `stats`, container `env`, container `config`, `top`.
  - Hidden when not in compose project.
  - Menus for removing/pause/stop/resume map to docker-compose commands.
- **Containers** (`containers_panel.go`):
  - Tabs: `logs`, `stats`, `env`, `config`, `top`.
  - Filters exclude stopped containers (toggle `e`) or compose-managed ones based on config.
  - Removal menu handles force/volumes; top-level bulk commands exist.
- **Images** (`images_panel.go`):
  - Tab: `config` (metadata + history).
  - Removal/prune menus.
- **Volumes** (`volumes_panel.go`):
  - Tab: `config` (labels, usage stats).
  - Prune/remove/bulk custom commands.
- **Networks** (`networks_panel.go`):
  - Tab: `config` (flags, attached containers).
  - Prune/remove/bulk commands.
- **Menu panel** (`menu_panel.go`):
  - Reused for contextual menus & keybinding cheat-sheets (`x`/`?`).
  - `OnClick` executes `MenuItem.OnPress`, closes popup first.

## Main Panel Behavior

- Backed by `Gui.Views.Main`; wraps, autoscroll toggled per task.
- `ContextState` selects tab list & maintains cache key (`item` + tab key + optional state fingerprints).
- `HandleSelect` on a side panel:
  - Focus and scroll the list entry.
  - Resolve selected item; `renderContext` obtains current tab’s `tasks.TaskFunc`.
  - `Gui.QueueTask` submits the task to `TaskManager`; task writes into `main` view (often async).
- Tabs clickable; `onMainTabClick` sets `ContextState` index and re-renders.
- Main scrolling: `j/k` (global), `PgUp/PgDn`, `Ctrl` combos; `Enter` escapes to/from main.

## Bottom Strip Flows

- `renderPanelOptions` populates `options`: global defaults unless popup overrides.
- `information` shows version + “Donate” link; environment-aware underscore hiding.
- `filter` logic:
  - `/` opens filter view (`handleOpenFilter`); keystrokes update `Gui.State.Filter.needle` live.
  - `Enter` commits (stays filtered, returns focus); `Esc` clears needle & reset list origins.
  - Filter context stored per panel to reset when switching focus or leaving search.
- `appStatus` updated by `WithWaitingStatus`; displays loader until async completes.

## Popups, Focus & View Stack

- `Gui.State.ViewStack` tracks focus history; popups pushed but stripped on new pushes.
- `switchFocus`:
  - Updates stack, toggles cursor, refreshes options, clears filter when leaving panel.
  - Auto-hides menu unless still in stack.
- `returnFocus` pops to previous non-popup view.
- `Menu` and `Confirmation` views share `resizePopupPanel` to fit content on resize.
- `Limit` view (`autoPositioned`) becomes visible when dimensions mapping excludes other views.

## Navigation & Input Flows

- Side panel cycling: `Tab`/`Shift-Tab`, `h/l`, arrow left/right.
- Direct focus shortcuts: numeric keys `[1-6]` map to side panels.
- Screen mode cycling: `+` → next (normal→half→full), `_` → previous; behavior flips when main focused.
- Options menu: `x` or `?` enumerates effective keybindings for current (and parent) view.
- Bulk command menus: `b` global; context-specific menus bound inside panel keymaps.
- Mouse:
  - Scroll wheel bound to list navigation.
  - Clicking `information` triggers donation link open via OS command.
- Filtering/resets: `Ctrl+C`? (not explicitly; rely on `Esc`).

## Data & Refresh Flow

- `setPanels` instantiates all `SideListPanel`s before keybindings.
- Background goroutines (`goEvery`) run at fixed intervals:
  - `reRenderMain` throttle loop keeps async tasks flushed.
  - `updateContainerDetails`, `checkForContextChange`, `renderContainersAndServices`.
  - `monitorContainerStats` spawns Docker stats monitors per running container.
- `listenForEvents` attaches to Docker event stream; on error displays message via `ErrorChan`.
- Panel data reloaders (projects, containers, images, volumes, networks) update list models then trigger `RerenderList` (which re-applies filters/sort and resets cursor clamp).

## Configuration Touchpoints

- `Gui.Config.UserConfig` influences visuals & data:
  - `Gui.SidePanelWidth`, `ScreenMode`, `ExpandFocusedSidePanel`, `ShowBottomLine`, `WrapMainPanel`, `ShowAllContainers`, `LegacySortContainers`, etc.
  - Custom/bulk command templates map into menu/bulk flows per resource type.
- Borders & theme colors set when creating views (`FrameRunes`, `SelBgColor`, `TitlePrefix` digits match numeric shortcuts).

## Failure/Limit States

- When terminal too small: only `limit` view shown with `NotEnoughSpace` message.
- Missing data states surface via `NoItemsMessage` per panel; `HandleSelect` renders message into main if list empty.
- Errors bubble through `createErrorPanel` (confirmation popup tinted red), preserving password secrecy by avoiding prompt echo.

