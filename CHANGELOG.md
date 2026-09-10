# Changelog

All notable changes to git-tui. Versions follow [semantic versioning](https://semver.org/).

## Unreleased

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
