# Architecture

git-tui is a single-threaded Ratatui application with background worker threads
for every network call. This document covers the module layout, how data flows,
and the invariants that are easy to break by accident.

## Module layout

```
src/
  main.rs          Event loop, draw dispatch, key routing, render tests
  app.rs           App state, Screen and Popup enums, filters, sprint logic
  loader.rs        Startup load, background refresh, detail and timeline fetch
  config.rs        Persistent settings
  cache.rs         TTL disk cache
  export.rs        Daily standup and sprint report, three output formats each

  models/          Domain types, no IO
    item.rs        Item, Comment, ItemKind
    status.rs      The 11 board columns
    field.rs       Project field and option definitions, iteration windows
    project.rs     Project metadata

  gh/              Everything that talks to GitHub, always through the gh CLI
    client.rs      Process wrapper, rate limit cache
    sync.rs        Paginated GraphQL fetch of all project items
    field.rs       GraphQL fetch of project fields, including iterations
    timeline.rs    Batched GraphQL timeline fetch
    issue.rs       Issue create, comment, label, node id, issue type, sub-issue
    item.rs        Project item field edits, live status and priority reads
    auth.rs        gh installation, login and scope checks
    project.rs     Project id lookup
    notifications.rs

  handlers/        Key events to state changes
    screens.rs     Menu, setup, settings, forms, Slack screens
    board.rs       Board and My Tasks navigation
    detail.rs      Item detail, QA action guards, move dialog, history
    search.rs      Search field and filters
    actions.rs     Executes confirmed actions: every GitHub write lives here
    helpers.rs     Index remapping after a refresh, repo splitting

  slack/           Webhook send, notification formatting, workspace user fetch
  ui/              Renderers, one per screen, plus theme and shared widgets
```

`ui/` only draws. It never mutates state and never performs IO, so any screen
can be rendered in a test.

## Data flow

### Startup

Three steps behind a loading screen: items, then field definitions, then the
project id. Each is served from the disk cache when fresh. Cached data older
than its soft TTL is still shown immediately and a background refresh is
triggered, so the UI is never blocked on the network.

### Background refresh

`loader::spawn_background_refresh` fetches all items, all fields and the project
id, then spawns two child workers: one for ticket timelines in batches, one for
missing created-at timestamps. Results arrive on a `BackgroundEvent` channel
stored in `App::bg_rx`.

That channel stays open until both child workers finish, which on a large board
is much longer than the item fetch itself. Nothing else may queue behind it. The
item detail fetch used to, and reported a load that never resolved.

### Writes and detail fetches

Everything else uses the persistent `ActionResult` channel, `App::action_rx`,
created once at startup. Every GitHub write happens on a worker thread and
reports back through it: success, failure, or a partial result with warnings.

The rule: **long-lived, restartable polling uses `bg_rx`; anything the user is
waiting for uses `action_tx`.**

### Confirm before execute

```
key press -> guard -> Popup::Confirm { on_confirm: ConfirmAction }
          -> Enter -> handlers::actions::execute_confirm_action
          -> worker thread -> gh CLI + optional Slack
          -> ActionResult -> popup or status bar
```

Guards live in `handlers::detail::qa_pass_allowed` and run twice: once when the
popup opens, and again at execute time, because a refresh may have landed in
between.

## Invariants

### Screens reference items by index

`Screen::ItemDetail(usize)` and friends index into `App::items`, which a refresh
replaces wholesale. `handlers::helpers::remap_screen_indices` rewrites the
current screen, the history stack and any open confirm popup by matching item
IDs. Anything that survives across a refresh must be keyed by ID, not index.
Detail fetches and label updates already are.

### Optimistic updates must be revertible

Status moves apply locally before the remote write so the board feels instant.
If the write fails, or a pre-write revalidation finds the ticket has moved, the
worker sends `ParentStatusChanged` with the real status to put it back.

### Handoff labels are state

Passing on Dev or STG only adds a label; the developers move the ticket. That
makes `Ready-for-Staging` and `Ready-for-UAT` real workflow state, and every
path that sends a ticket back has to clear them, or the ticket stays hidden from
My Tasks and the next pass is refused. See `App::HANDOFF_LABELS`.

### Failures are reported

A swallowed error behind a success message has caused several bugs here. If a
GitHub call can fail, its result reaches the user, as a popup for single actions
or as a warning list for multi-step ones.

## QA metrics

`export::latest_qa_verdict` derives one verdict per ticket from its timeline,
reading both signals:

- a status event out of a QA column into In Progress is a fail;
- a status event out of a QA column into Tech Complete, Ready for Release or
  Done is a pass;
- a `Ready-for-Staging` or `Ready-for-UAT` label event is a pass.

The latest event wins, so `passed + failed = tested` holds by construction. A
status-only reading recorded failures immediately but no pass until a ticket
reached Tech Complete, which biased the pass rate downwards.

Transitions that are not QA verdicts, a product owner parking a ticket in
Backlog, a developer putting one into QA, are ignored rather than counted as
failures.

## Caching

| Data | Key | Soft TTL |
| --- | --- | --- |
| Items | `items` | `poll_interval_secs`, default 3 minutes |
| Fields | `fields` | 30 minutes |
| Project id | `project_id` | 1 hour |
| Timeline | `timeline_{repo}_{number}` | 30 minutes |

Anything older than a 48 hour hard TTL is discarded rather than shown. Timeline
keys include the repository because one project can span repositories where the
same issue number exists in more than one.

## Adding to the app

**A screen.** Add a `Screen` variant, a renderer in `ui/`, a handler, arms in
`main::dispatch_key` and `main::render_content`, a label in `Screen::label`, and
an entry in the render test's `every_screen()`. If it holds an item index, remap
it in `handlers::helpers`.

**A QA action.** Add a `ConfirmAction` variant, a guard arm in
`qa_pass_allowed`, a trigger in `handlers::detail`, execution in
`handlers::actions`, a row in `ui::qa_actions`, and remapping in
`handlers::helpers`.

**An item field.** Add it to `models::item::Item`, request it in
`gh::sync::ITEMS_QUERY`, parse it in `node_to_item`, then display it.

**A filter dimension.** Add the vector to `App`, matching in
`App::apply_filters`, the option lookups in `handlers::search`, and a section in
`ui::search`.
