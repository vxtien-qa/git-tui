# Changelog

All notable changes to git-tui. Versions follow [semantic versioning](https://semver.org/).

## v3.3.0 - UAT cycle and board query

### Added

- Item Detail lists the ticket's linked pull requests: the ones that close it, and the ones that mention it or were linked by hand, which is where bundle PRs and deploy-branch PRs show up. Each shows its state and base branch. They load with the detail, not the board sync.
- `d` in Item Detail runs a deploy check: every merged linked PR is compared with `develop`, `staging`, `uat` and `production` through GitHub's compare API, and shows `✓` (in the branch), `✗` (not in it), `?` (not in its history, so it may have arrived by cherry-pick), `-` (no such branch, commit or repository) or `!` (the check failed). A `?` is never reported as "not deployed". The branches are configurable as `deploy_branches`. A ticket whose PR lookup failed says so, with `r` to retry; when a later refresh fails, the last list stays on screen, marked, and `d` checks it. Lookups that fail are reported once per reason, with the PRs and branches each one hit, not one raw `gh` command per PR and branch. When a ticket has more links than GitHub returns at once, the detail says the oldest are not listed, and the UAT verdict is never `✓` on such a list.
- `V` (Bulk verify UAT) opens a pick list of the In UAT tickets in the sprint filter, all checked. `Space` unchecks one, `a` checks all or none, `Enter` passes the checked ones together: a comment on each and one Slack "Ready for release" summary, which cc's only the people of the tickets that went through. The confirm says when no Slack message will go out (toggle off, Slack not set up).
- `I` (Move to In UAT) is for tickets whose code reached UAT: a pick list of the sprint's In QA tickets carrying `Ready-for-UAT`, the ticket in hand checked and under the cursor. `d` in the list reads every row's PR list fresh and deploy-checks it, and its UAT column is `✓` only when every open or merged linked PR is merged and on UAT; a row with only open PRs shows `?`, one with no PR `-`. A key to the marks shows under the list once a check has run. Each checked ticket moves to In UAT, loses the label and gets a comment; one Slack message lists them, behind a new `Move to In UAT` toggle. A ticket that lost its label or left In QA since the list opened is skipped and named.
- My Tasks rows under Waiting for deploy can be selected, and `I` on one opens Move to In UAT; `x` clears a stale handoff label there, as Help already said. The footer offers the keys of the selected row, and the header counts the waiting tickets, which the list position includes.
- Item Detail's footer names the QA action that fits the ticket's column (`U` and `V` on In UAT, `I` on In QA with Ready-for-UAT, `P` on In QA).
- Return and Fail can move the ticket to another sprint. Their confirm has a Sprint row cycled with the arrow keys: Keep, Today (the sprint running today) or Next. Keep is the default. Another pick writes the Sprint field and says so in the comment. The row names the ticket it moves: on Fail that is the parent, not the bug being filed.
- Search filters by EPIC (the root of the ticket's sub-issue chain, offered for roots that still have open work) and by assignee, with `(none)` for unassigned tickets. Sync now reads each parent's repository, so a code-repo ticket under an epic in the board repo finds that epic instead of an unrelated issue with the same number in its own repo. The focused row shows where the cursor is among its options (`3/31`), since a long row scrolls.
- `Ctrl+S` in Search saves the keyword and every filter under a name, and `Ctrl+O` lists the saved ones: `Enter` loads, `d` deletes. Sprints are saved as `@current`, `@previous` and `@next`, so a saved filter follows the calendar. They live in `config.json` as `search_presets`.
- `Ctrl+Y` in Search copies every result as a linked list: `m` for Markdown (GitHub, Notion), `s` for rich text for Slack (pasted `<url|text>` markup shows literally in Slack, pasted HTML keeps its links).

### Changed

- `U` (Pass UAT) passes the ticket it is pressed on, like Pass Dev and Pass STG: Tech Complete, the comment `Verified on UAT. Moving to Tech Complete.` and one Slack message. Passing the whole In UAT column at once moved to `V`.
- The QA Actions menu shows one line per action when the terminal is too short for two; shorter still it drops its spacer rows, and at the 60x10 minimum it lists the actions two to a row. Every action and `Esc` stay on screen.
- Help wraps long descriptions under their column instead of cutting them off at the panel edge; at 80x24 a third of its lines were clipped.
- The Slack toggle for Pass UAT reads `Pass UAT (U, V)`, since it covers Bulk verify UAT too. The toggles are listed in the order a ticket goes through QA, so `Move to In UAT` sits between Pass STG and Pass UAT.
- Every report line names the root of the ticket's sub-issue chain: `[Dave project]` for an epic, `[Issues sprint 29]` for the plain task the QA files bugs under. Only a ticket with no parent at all is tagged `[no parent]`; half of Sprint 29 used to read `[no epic]`, which said nothing about where those tickets belonged. The root is found by walking the sub-issue chain: sync fetches only one parent level, so a ticket hanging off a story reaches its epic through the board, however deep the chain, and a parent that is not on the board is named by its synced title. Off-board parents with the same number in two repositories stay two groups. An epic is recognised by the `Epic` label or an `[Epic]` title prefix, because the board uses both.
- Sprint report gains a `By EPIC / root ticket` rollup - tickets, bugs, how many are in QA and in UAT, and how many were tested, passed and failed per root, epics first and tickets with no parent last under `No parent`. `in QA` uses the Sprint Overview's definition (In QA and In QA - Dev) and In UAT is listed on its own, so the per-root counts add up to the sprint totals.
- Daily standup reads as three separate things that happened yesterday: `Tested & Verified`, `Failed` and `Issues found`. A ticket QA filed used to appear twice, once as a generic "Worked on ticket (Ready for Dev)" line and again under the bug heading, and a rejection sat in among the passes where the team could miss it.
- `Issues found` covers bugs and QA-raised enhancements, replacing the bug-only section.
- Reports say what QA is doing to a ticket: `Testing` for a task, `Verifying` for a bug or an enhancement QA raised. Only the report form's upper-case `[ENHANCEMENT]` title prefix counts as a QA finding - the plain `Enhancement` label is also on dev feature work, which is tested, not verified.
- New bug reports, enhancements and tasks are filed into the sprint running today, read from the Sprint field's iteration dates. They used to inherit the parent ticket's sprint (falling back to the board's sprint filter), so a bug raised while QAing a carried-over ticket landed in the sprint that ticket came from - and on day one of a sprint, before any item sits in it, everything went to the previous one. The form's `Sprint (auto)` line now shows the same value that gets written.
- QA Results count only verdicts given since the sprint's first day, read from the Sprint field's iteration `startDate`, and the heading says so: `QA Results (27 tested since Sep 14)`. A carried-over ticket passed on Dev in a previous sprint and never deployed is not this sprint's testing; in Sprint 29 that was 12 of 39 "tested". The Failed / Retested list still reads the whole history, because it answers which tickets are failed right now. Without iteration dates, or under the `All Sprints` filter, every verdict counts as before.
- Done means Tech Complete, Ready for Release or Done, everywhere: the sprint report, the dashboard's Sprint Overview, My Stats, By Stack and Bug Tracker, and the menu's sprint progress. Tech Complete ships in the release note, and the dashboard alone used three different definitions. The sprint report's Done line shows the split: `Done: 14 (13%) = 6 Tech Complete + 8 Done`. The dashboard's Status Breakdown keeps one row per column, so its Done row is the Done column alone.
- A move the same person reverses within 30 minutes is an undo and is dropped from every report. A misclick to In Progress and straight back used to read as a failure in the standup and as `failed 2×` in the sprint report. A reversal by someone else, or a slower one, stays as two real actions.
- Standup lines say what the move was. `Verified, ready for release` and `Testing complete` are only printed when the ticket left a QA column; any other move, such as Ready for Dev straight to Tech Complete, or the release step Tech Complete to Ready for Release, prints as `Moved Ready for Dev → Tech Complete`. `Worked on ticket` and `needs confirmation` are gone, since the report cannot know either.
- The standup lists every ticket the QA touched yesterday, not only the current sprint's. On the first day of a sprint, Friday's tickets still sit in the previous one and were dropped from Monday's report.
- The bugs section labels its last bucket `Backlog / Blocked`, which is what it has always counted.

### Fixed

- Confirm, error and success popups grow to fit their text. A message of two lines or more pushed the `[Enter/y] Confirm  [Esc/n] Cancel` row, or `Press any key to dismiss`, off the bottom of the popup, so every UAT confirm hid its keys. On a short terminal the spacer rows, then the end of the message, give way; the keys never do. The text also keeps a margin from the right border.
- Item Detail puts `Created` on its own row when the Labels value is too long for its column; `Ready-for-UATCreated` ran together.
- A progress note such as `Pass UAT: verifying 1 ticket(s)...` clears when the result pops up, instead of staying under it for six seconds.
- A Return whose status write fails puts the ticket back in its column on screen, so it can be retried at once; it stayed In Progress and the guard refused a retry until the next sync. Write failures in the UAT flows and the sprint moves give gh's reason, not the command line.
- The bug, enhancement and task forms keep `Ctrl+S:Submit` in their footer when the sidebar takes width; it was the first hint to be dropped. The bug form's Parent line ends in `..` instead of running into the border.
- Help stops scrolling once its last line is at the bottom; `↓` ran on into blank pages, each one costing an `↑` to come back.
- My Tasks pulls the cursor row back on screen after a resize or a refresh, and stops scrolling when the last row reaches the bottom instead of leaving the last page half empty.
- The menu's Your queue counts line up under each other.
- The bug and enhancement form leaves Priority and Stack at `--`. The defaults were written for selectors without `--` in front and became P1 and FE, which also skipped the wait for GitHub's priority automation.
- Search scrolls its filter panel to keep the option under the cursor visible; on an 80x24 terminal a long filter row put it below the panel's edge.
- The Slack notification toggles scroll to the cursor; at 80x24 the last rows were below the panel.
- Background sync fetches the timeline of the report sprint's Backlog and Ready for Dev tickets too. Only the Dashboard fetched those, and it silently skipped whenever a sync was in flight, so a ticket filed straight into Ready for Dev counted as "no history data yet" in every report no matter how often you refreshed.
- Standup picks the most significant QA action of the day and, among equals, the latest, across status moves and label passes together. A label pass was only consulted when no status move existed, so a ticket failed at 17:12 and passed on STG at 17:16 reported as `Failed on STG`, and two passes in one day reported the earlier one.
- `Issues found` reads who filed the ticket from the timeline - the author gives it its first status and its type label within moments of creation - instead of guessing from the assignees. A bug the QA filed and assigned to two developers was missing; a bug a developer filed and assigned to the QA was claimed. The assignee guess remains only for tickets whose timeline is not fetched yet.
- A bad `timezone_offset_hours` falls back to UTC instead of panicking in the middle of a report.
- Report lines stay on one line. Titles carrying an embedded newline or a trailing space (a body pasted into the title field) broke the line in half, so a ticket's status and priority ended up on their own line in Slack.

## v3.2.0 - public release

Shipped in the v3.2.0 tag together with the UAT flow below; listed as Unreleased until 3.3.0. The `U` behaviour described here changed in 3.3.0.

### Added

- Enhancement requests: `e` files an `[ENHANCEMENT]` issue labelled `Enhancement` and `Task`, with GitHub's native issue type set to `Task`, through the same form and submit path as a bug report. It has its own Slack toggle.
- `x` clears stale `Ready-for-*` handoff labels, for tickets a developer moved back outside the app.
- `U` (Pass UAT) now clears the whole In UAT regression batch in one action: every ticket gets its own comment, and the team gets a single aggregated "Ready for release" message in Slack.
- A location bar on every screen showing where you are, for example `Board > #412 > QA actions`.
- Warning when the board's Status columns no longer match the ones this build knows, instead of silently counting unknown columns as Backlog.
- Position indicators: `n/total` on lists, `field n/m` on the long forms.
- Move dialog accepts arrow keys and Enter alongside the existing digit shortcuts.

### Changed

- Project fields are read over GraphQL. `gh project field-list` never returned the iteration configuration, so the Sprint field had no options at all and every "inherit the parent's Sprint" write silently found nothing to set.
- The current sprint is taken from the iteration's real `startDate` and `duration`. The hardcoded date anchor is now only a fallback.
- QA metrics are derived from the latest QA verdict per ticket, label-only passes included, so `passed + failed = tested` holds by construction. Previously a pass only counted once a ticket reached Tech Complete, which biased the pass rate downwards.
- "Failed" now means QA failed it. It is read from QA events rather than the current column, so a ticket parked in Backlog is no longer counted as a QA failure.
- The In QA figure is split into `to test`, `bugs to verify` and `waiting for deploy`, so reports and My Tasks agree.
- `Fail` is only offered from a QA column, on every screen, because it moves the parent ticket.
- New tasks are created in Ready for Dev. The hardcoded "Todo" column does not exist on the board, so tasks were created with no status at all.
- A ticket with no Stack, or an unmapped one, tags its assignees rather than every lead of every stack. `stack_leads` ships empty instead of carrying one team's GitHub handles.
- Footers fit any terminal width, always keep `Esc`, and point at `?` for the full key list instead of clipping off the right edge.
- The welcome panel shows the QA queue and sprint progress in place of decorative art.
- Every screen renders at 60x10 and up, verified by a render test that walks all screens at five terminal sizes.

### Fixed

- Retest loop: `Return` and `Fail` now remove the `Ready-for-Staging` and `Ready-for-UAT` labels they added. A ticket coming back for retest kept them, which hid it from My Tasks and made the next pass refuse with "already passed".
- GitHub's native issue type and sub-issue links are actually applied. Both mutations were handed the project item ID (`PVTI_...`) instead of the issue node ID (`I_kw...`), and both errors were discarded.
- Item detail loads while a background sync is running. The automatic fetch shared its channel with the sync, whose timeline workers hold it open long after the item list arrives, so opening a ticket reported a load that never resolved and moving to another ticket dropped the previous one.
- Ticket type reads the same on the board, My Tasks and Search: `[Bug]`, `[Enh]` or `[Task]`. An enhancement used to render as a task.
- Timeline and created-at data is fetched with each item's real repository owner, so tickets from another organisation keep their history.
- Label filters treat an exclusion as a veto, and compare label names the way the rest of the app does.
- A background sync no longer resets the list cursor.
- Radio rows in the report and task forms wrap instead of losing options past the panel edge.
- Mark all notifications as read reports failures instead of always claiming success.
- Batch moves run in one worker with one aggregated result, rather than a process and a popup per ticket.
- `Config::save` no longer unlinks the config before renaming the replacement into place.

## v3.2.0 - UAT flow

### UAT Environment Support
- **New "In UAT" board column** - the board, dashboard, reports, and search now recognize the new In UAT status (previously items in this column showed up under Backlog).
- **Pass STG reworked** - `P` now adds the **Ready-for-UAT** label + comment (like Pass Dev), instead of moving to Tech Complete. Devs deploy to UAT and move the ticket to In UAT.
- **New Pass UAT action** - `U` in Item Detail / QA Actions moves an In UAT ticket to Tech Complete + comments + tags devs. New Slack toggle: **Pass UAT**.
- **My Tasks includes In UAT** - tickets waiting for your UAT verification appear in My Tasks; tickets labeled Ready-for-UAT (waiting for dev deploy) are hidden.
- **Bug form UAT environment** - Environment options are now Dev/STG/UAT/Demo/Prod, and the default follows the parent ticket's status (In QA - Dev → Dev, In QA → STG, In UAT → UAT).
- **Reports understand UAT** - daily standup detects "Passed on STG, waiting for UAT deploy" and "Failed on UAT"; sprint report adds an In UAT pipeline line, and items in UAT are no longer miscounted as failed.

### Safer QA actions
- **Guarded passes** - Pass Dev/STG/UAT only fire from the matching column and never twice on the same ticket (no more duplicate comments/Slack pings).
- **`p` opens the QA Actions menu** on Item Detail (same as My Tasks); Return moved into that menu so `r` is reload everywhere - refresh muscle-memory can't demote a ticket anymore.
- **Confirm popups** now only cancel on `Esc`/`n` (any other key is ignored) and confirm on `Enter`/`y`.
- **Confirmations added** for Slack sends from Dashboard (`S`/`X`), mark-all-read (`a`), clear cache (`c`), delete stack (`d`).
- **Drafts survive failures** - bug/task/comment forms are cleared only after GitHub confirms creation; a failed submit keeps your text for retry. Esc in the comment editor asks twice before discarding.
- **Fail flow reordered** - the parent ticket is moved to In Progress and commented (with a link to the new bug, on the parent's own repo) only after the bug is created.
- **Honest results** - status moves report real GitHub failures instead of always claiming " Status moved"; multiple actions queue their results instead of overwriting each other.

### UX & UI
- **Cursor editing in text fields** - Left/Right/Home/End/Delete move a real cursor inside comments and bug/task form fields (multibyte-safe); no more backspacing a whole paragraph to fix one typo.
- **Mouse wheel scrolling** on board, lists, detail, dashboard, settings and help.
- **Pre-write revalidation** - Pass UAT and Return check the live GitHub status right before writing; if a dev already moved the ticket, the action aborts cleanly and the board corrects itself.
- **Dashboard uses wide terminals** - two-column layout at ≥110 columns, bars scale with width.
- **"Waiting for deploy" section** in My Tasks - passed tickets (Ready-for-Staging/UAT) stay visible while devs deploy, and reappear as testable work once moved onward.
- **Move dialog** supports `u` = In UAT (batch moves too) and never clips on short terminals; popups (QA Actions, confirm, success) word-wrap and size to content.
- **Board** shows up to 6 columns on wide terminals, colors column titles by status (Blocked = yellow), marks bugs in red consistently, and shows an "- empty -" placeholder.
- **Comment editor** word-wraps and follows the cursor - no more typing blind on long comments.
- **Native paste support** (bracketed paste) - pasting multi-line text into forms inserts it instead of firing key actions.
- **Search** - `?` is typable in queries; `Esc` goes back keeping filters; `Ctrl+L` clears them.
- **Unicode detection** - icons render properly in VS Code/Warp/Alacritty/WezTerm on Windows; override with `GIT_TUI_UNICODE=1|0`.
- **Menu `r`** refreshes, and a failed initial sync now retries automatically.

### Bug Fixes
- **Label matching normalized** - "Ready for Release" (space-separated, as on the repo) now correctly excludes items from My Tasks; label comparison ignores case and hyphen/space differences.
- **Stale-index race fixed** - a background refresh while a confirm popup is open can no longer make the action hit the wrong ticket.
- **Slack "Task Created" icon** - new-task notifications now show the  icon instead of the generic ; Slack messages now say who performed the action.

## v3.1.1 - Slack Lead Tagging Fix

### Bug Fixes
- **Slack Lead Tagging Fix** - When a ticket has no Stack (empty), Slack notifications now only tag the assignees instead of tagging all tech leads from every stack. Affects both Pass Dev and Pass STG actions.

## v3.1.0 - Stack Leads, Date-based Sprint & Settings Scroll

### New Features
- **Stack Leads Management** - New screen to manage QA leads per stack (FE/BE/App). Leads are stored in config and auto-tagged in Slack notifications. Edit via Settings → `[l]`.
- **Date-based Sprint Calculation** - Current sprint is now calculated from real-time date (anchor: Sprint 12 = Jan 19, 2026, 14-day cycles) instead of heuristic-based "most active sprint".
- **Settings Screen Scroll** - Settings screen is now scrollable when content exceeds terminal height.

### Improvements
- **History Fetch Refactoring** - Extracted `spawn_history_fetch()` to eliminate duplicated timeline-fetching code in detail/history handlers.
- **Sprint Report Enhancements** - Improved Done/TechComplete/ReadyForRelease status separation in reports.
- **Code Quality** - Cleaned up and reorganized `detail.rs`, `actions.rs`, `screens.rs` for better maintainability.

## v3.0.3 - Slack Comment Content & Bug Report Fixes

### New Features
- **Slack Comment Content** - Slack notifications now include the latest comment text as a blockquote when a QA member posts a comment on a ticket.

### Bug Fixes
- **Sprint Assignment on Bug Creation** - Bugs created with `b` now correctly inherit the parent ticket's Sprint.
- **Bug Report Field Error Reporting** - Field edits no longer silently fail. A popup now shows exactly which fields failed and why.
- **Error Popup Overflow** - Long error messages now word-wrap and dynamically size the dialog height.
- **TUI Display Corruption** - Removed `eprintln!` calls from background threads.

## v3.0.2 - Monday Report Fix & Sprint Mentions

### Bug Fixes
- **Monday Daily Report** - "Yesterday" section now correctly shows Friday's work on Monday. Events with missing actor data in cache were previously skipped, causing an empty Yesterday section.
- **Actor Fallback** - Timeline events on tickets assigned to the current user no longer require an explicit `actor` field; old cache entries are attributed to the assignee automatically.

### Improvements
- **Sprint Report Assignee Mentions** - "Bugs Waiting for Dev" now shows `@DisplayName` (Slack display name) in Markdown/Text copy formats for easier pasting into Slack.

## v3.0.1 - Missing Code Hotfix

### Bug Fixes
- **Missing Code in Release:** Included all Slack integration and task UI code that was accidentally omitted from the `v3.0.0` release build.

## v3.0.0 - Native Tracking, Auto-Assignee Slack Pickers, and Field Clearances

### Major Features
- **Native GraphQL Tracking** - Tasks and Bug Reports now fully embed themselves securely into GitHub's native Issue track layout via global identifiers instead of simple Markdown injections.
- **Standalone Tasks Shortcut** - Create a standalone Task using `T` alongside the newly isolated `b` workflow to spawn Parent-bound Bug tickets.
- **Slack Form Auto-Assignees** - Replacing numeric Assignee arrays with a comprehensive Slack User lookup matrix featuring searchable TUI overlays. Checkbox toggles bind natively across Tasks & Bugs.

### UI Improvements
- **Text Area Auto-wrap** - Replaced strict TUI text blocks with natural bounds detection, enabling natural visual line breaks inside text fields without overriding raw input data. Vertical form scrolling anchors cursor focus securely on-screen.
- **Dropdown Clears** - Option toggles (e.g., Priority, Stack) now support empty states (`--`) allowing metadata fields to clear successfully using `gh api graphql` column removals.

### Bug Fixes
- **Native Mappings Integration Fix** - Type & Sprint are updated asynchronously natively capturing `eq_ignore_ascii_case()` Iterations and Repository-level IssueTypes bindings (e.g: Type `Bug` or `Task`); safely bypassing the Project Board columns trap.

## v2.5.0 - Slack Notifications & Auto-Mentions

### New Features
- **Slack Auto-Notifications** - Automatic Slack notifications for all QA actions: Pass Dev, Pass STG, Return, Bug Report, Comment, and Move Items. Messages include emoji indicators () with ticket numbers and user mentions.
- **Per-Action Notification Toggles** - New config screen (Settings → `[n]`) lets you toggle each notification type ON/OFF individually. Auto-saves on exit.
- **Send Reports to Slack** - Dashboard hotkeys `S` (Daily Standup → Slack) and `X` (Sprint Report → Slack) send reports directly to your configured Slack channel.

### Bug Fixes
- **Shift+R reload fixed** - Force reload (`R`) in Item Detail now always works, even when a background refresh is running. Previously, Shift+R would get stuck on "Loading..." if the background data sync was in progress.

## v2.4.0 - Copy Ticket as Markdown

### New Features
- **Copy ticket as Markdown** - Press `y` in Item Detail to copy the full ticket (fields table, description, comments) as formatted Markdown to clipboard. Shows a success popup notification.

## v2.3.3 - UI Polish & Agent Knowledge Base

### UI Improvements
- **Full-width sidebar highlight** - Selected menu item indicator now spans the entire sidebar width.
- **Version display** - App version shown at the bottom of the sidebar.

### Developer Experience
- **Added `.agent/` knowledge base** - Codebase architecture skill, feature/debug/release workflows.

## v2.3.2 - Unicode Icon Fallback

### Bug Fixes
- **Fixed 50+ broken Unicode icons/symbols** that rendered as garbled text on Windows terminals without Unicode support.
- Added 17 icon helper functions to `theme.rs` with ASCII fallbacks via `supports_unicode()`.
- Replaced all hardcoded Unicode in 15 files with centralized theme helpers.
- Menu banner now has a full ASCII box-drawing alternative for non-Unicode terminals.

## v2.3.1 - Windows Double-Fire Fix

### Bug Fixes
- **Windows Double-Fire Fix (Loading & Error screens):** Extended `KeyEventKind::Press` filter to the loading interrupt checker and error screen retry/quit loop - Esc, `q`, and `r` keys no longer fire twice on Windows during startup loading and error recovery.

## v2.3.0 - First-Time User Flow Overhaul

### New Features
- **Auth Screen Redesign:** New contextual UX with color-coded status (/), Quick Fix section, and `[c]` key to **copy login command to clipboard**.
- **Setup Wizard UX:** Added **Ctrl+V paste** support for URL fields, **↑↓ arrow key** navigation between fields.
- **Auto-Load After Setup:** Saving config now **immediately loads project data**.
- **Generic Defaults:** Removed hardcoded organization values from default config.

### Bug Fixes
- **Windows Double-Fire Fix:** Added `KeyEventKind::Press` filter to main event loop.
- **Board Indicator Fix:** Column selection indicator now adjusts to terminal width.
- **Auth Scope Instructions:** All auth instructions now include required scopes.

## v2.2.5 - Windows Auth Redesign

### Bug Fixes
- **Windows Auth Redesign:** Completely removed the "exit TUI to run `gh auth login`" pattern which was fundamentally broken on Windows due to console mode issues. The app now **stays in the TUI**, shows step-by-step instructions to run `gh auth login` in another terminal, and **auto-polls auth status every 10 seconds** to detect when authentication completes.

## v2.2.1 - UI Polish & Sprint Report Redesign

### UI Improvements
- **Menu panel:** Centered progress bar and legend for a balanced, polished layout
- **Dashboard:** Removed cat mascot to maximize vertical space for useful content

### UX Improvements
- **Auto-load task details:** Navigating to a task now auto-fetches comments and body - no manual reload needed

### Sprint Report Redesign
- **Pipeline-grouped overview:** Sprint Overview now shows Not Started → In Dev → In QA → Done → Blocked (numbers add up to total)
- **Consolidated bugs section:** All open bugs (In QA + Waiting for Dev + Blocked) in one clear section with priority breakdown
- **Simplified QA Results:** Just Passed / Failed with consistent percentages - no more confusing mixed denominators
- **Added date** to report header (like daily standup)
- Removed redundant "By Status" breakdown and emoji icons

## v2.2.0 - Windows Auth Fix & Build Compatibility

### Bug Fixes
- **Windows Auth Fix:** `gh auth login` no longer hangs on Windows - now uses explicit `--hostname`, `-p https`, `--web` flags to skip interactive prompts

### Improvements
- **Build Compatibility:** Replaced `LazyLock` (nightly) with `OnceLock` (stable) for rate-limit cache - ensures MSRV 1.78.0 compatibility across all platforms
- **Private Repo Install:** Install scripts (`install.sh`, `install.ps1`) now use `gh` CLI for downloading release assets - supports private repositories
- **MSRV:** Explicitly set minimum supported Rust version to `1.78.0`

## v2.1.1 - Bug Fixes & UX Polish

### Bug Fixes
- **Comment Routing:** Fixed a critical bug where Return, Pass Dev, and Pass STG actions incorrectly posted comments to the global bug repository instead of the item's actual repository.
- **QA Action Navigation:** Fixed an issue where the Fail action (opening the bug report form) broke the screen history stack, ensuring the `Esc` key works correctly again.

### UX & Concurrency
- **Optimistic UI Updates:** Status moves (e.g., drag-and-drop or using shortcut keys) are now instantaneous. The UI updates immediately while the API request processes in the background.
- **Non-blocking API Calls:** Fetching history timeline (`h` or `r` in History) and fetching notifications no longer freeze the UI thread. They now run asynchronously.

### Tech Debt & Cleanups
- Extracted and deduplicated core logic (Return action, Sprint filtering).
- Removed legacy unused Models, Enum variants, and rendering functions for a leaner codebase.

## v2.1.0 - Advanced Sprint Reports

### Sprint Report Refinements
- **Tickets to test** - non-bug items currently in QA queue (InQA / InQA Dev)
- **Bugs to verify** - bug-labeled items currently in QA queue, with priority breakdown
- **Bugs open** - unresolved bugs not yet in QA (no overlap with Bugs to verify)
- **Bugs Waiting for Dev** - detail table of bugs in ReadyForDev / InProgress / InReview
- **Fixed pass/fail overlap** - Passed and Failed are now mutually exclusive by current status (`passed + failed = tested`)
- **Failed/Retested list** - only shows currently-failing items (excludes passed-on-retest and in-QA items)
- **Retested metric** - shows total retests with currently-failing breakdown
- Removed Risk sections (Not Started / Not Yet in QA) for cleaner reports

### Bug Fixes
- Fixed dashboard `bug_open` count inconsistency with sprint report (now both exclude TechComplete/ReadyForRelease)

## v2.0.0 - QA Metrics, Incremental Sync, Setup Wizard

### New Features
- **Sprint Report QA Metrics** - tested/passed/failed rates, retest tracking
- **Failed/Retested Tickets** section with fail count per ticket
- **Incremental Sync** - reduced API usage from 1300+ to ~few points/refresh
- **Batch GraphQL timeline queries** - fetches 30 issues in 1 API call
- **OT tagging** - Bug Found and Blockers tag overtime work
- **Setup Wizard** - paste GitHub Project URL and Bug Repo URL for auto-config

### Bug Fixes
- Fixed daily standup "Yesterday" section sometimes empty
- Fixed retest detection false positives (normal Dev→STG flow)
- Fixed error screen breaking on terminal resize
- Fixed UI layout issues with logo/cat ASCII overlap

### Improvements
- Tiered cache with per-data-type TTLs
- Background refresh optimizations
- My Tasks sprint filtering with `s` key
- Pass Dev/STG confirmation popups
- Excluded "Ready-for-Staging"/"Ready-for-Release" from My Tasks
- Search filter 3-state toggle UI

---
