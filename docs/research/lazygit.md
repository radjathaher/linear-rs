# Lazygit Screen Model

## Architecture
- Context tree instantiates every screen context (side, main, popup, extras, display) and wires views/windows (`pkg/gui/context/setup.go:5`, `pkg/gui/context/context.go:85`)
- Context stack enforces focus rules: side contexts replace all others, main contexts swap other mains, popups auto-dismiss when replaced (`pkg/gui/context.go:58`, `pkg/gui/context.go:82`, `pkg/gui/context.go:109`)
- Window helper tracks window→view mapping, lifts active view, and defines canonical side window order (`pkg/gui/controllers/helpers/window_helper.go:30`, `pkg/gui/controllers/helpers/window_helper.go:85`, `pkg/gui/controllers/helpers/window_helper.go:137`)

## Layout Sections
- Layout pass computes boxlayout dimensions per window via helper combining screen size, repo state, and config (`pkg/gui/layout.go:11`, `pkg/gui/controllers/helpers/window_arrangement_helper.go:76`)
- Root layout splits terminal into side section + main section + optional bottom info band (`pkg/gui/controllers/helpers/window_arrangement_helper.go:139`)
- Side column adapts between full-height, accordion, and squashed stacks for `status/files/branches/commits/stash` (`pkg/gui/controllers/helpers/window_arrangement_helper.go:422`)
- Main section chooses `main`/`secondary` pairing and attaches extras/command log panel when open (`pkg/gui/controllers/helpers/window_arrangement_helper.go:172`, `pkg/gui/controllers/helpers/window_arrangement_helper.go:186`)
- Bottom band assembles search prompt, options, app status, information with fixed/flexible spacers (`pkg/gui/controllers/helpers/window_arrangement_helper.go:268`)
- Limit overlay covers entire screen when terminal smaller than thresholds (`pkg/gui/layout.go:131`)

## View Layers
- Ordered view mapping sets z-index: base panels, main/secondary family, extras, bottom line, popups, then limit overlay (`pkg/gui/views.go:25`)
- View creation configures wrapping, editors, default visibility for main panes, staging, prompts, popups (`pkg/gui/views.go:79`, `pkg/gui/views.go:122`)

## Screen Modes & Splits
- Screen modes cycle Normal→Half→Full; affected contexts rerender on change (`pkg/gui/controllers/screen_mode_actions.go:6`, `pkg/gui/controllers/screen_mode_actions.go:14`)
- Weight algorithm reallocates space depending on focused window and screen mode (`pkg/gui/controllers/helpers/window_arrangement_helper.go:238`)
- Split main panel toggles horizontal vs vertical diff layout per config and terminal bounds (`pkg/gui/controllers/helpers/window_arrangement_helper.go:372`)
- Repo state tracks `ScreenMode`, `SplitMainPanel`, search prompt, window map (`pkg/gui/types/common.go:374`, `pkg/gui/types/common.go:401`)

## Navigation Flow
- Default side context is files unless filtering mode active (`pkg/gui/context_config.go:12`)
- Side window controller reuses universal next/prev block bindings to rotate contexts via window helper (`pkg/gui/controllers/side_window_controller.go:17`, `pkg/gui/controllers/side_window_controller.go:35`)
- Layout ensures transient contexts only visible in their active window mapping (`pkg/gui/layout.go:137`, `pkg/gui/layout.go:277`)
- Flatten order defines baseline stacking so popups overlay mains and display contexts stay lowest (`pkg/gui/context/context.go:129`)

## Context Catalog
- Side contexts: status, files, branches, commits, stash, remotes, tags, remote branches, reflog, submodules, worktrees, sub commits (`pkg/gui/context/setup.go:19`, `pkg/gui/context/setup.go:37`)
- Main contexts: normal/staging/custom patch/merge conflicts with paired secondary slots (`pkg/gui/context/setup.go:42`, `pkg/gui/context/setup.go:83`)
- Extras context holds command log in separate window (`pkg/gui/context/setup.go:108`)
- Popups: menu, confirmation, prompt, commit message/description, search, suggestions; persistent vs temporary dictated by `ContextKind` (`pkg/gui/context/setup.go:30`, `pkg/gui/context/setup.go:86`, `pkg/gui/types/context.go:24`)
- Display contexts drive bottom-line views, search prefix, limit overlay (`pkg/gui/context/setup.go:126`)

## Runtime Highlights
- Initial layout sets command log header, current view, and triggers repo-specific view setup (`pkg/gui/layout.go:13`, `pkg/gui/layout.go:150`)
- Buffer managers backfill main/secondary lines on resize to avoid truncated diffs (`pkg/gui/layout.go:30`)
- Window view map persisted in repo state allows reusing views across windows (e.g., commit files) (`pkg/gui/controllers/helpers/window_helper.go:50`, `pkg/gui/types/common.go:374`)
- `transientContexts` classification gates contexts that reuse windows (menus, prompts) so they surface only when active (`pkg/gui/layout.go:137`, `pkg/gui/layout.go:277`)

## Layout Sketch
```
+---------------+-------------------------------+-------+
| Side Stack    | Main / Secondary (split-aware) | Extras|
+---------------+-------------------------------+-------+
| Options | AppStatus | Information | Search     |
```
