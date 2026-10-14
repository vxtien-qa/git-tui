# UAT cycle and board query (v3.3.0)

Date: 2026-10-14
Status: draft for review

## Why

Over the last month the QA ran the same board work by hand, through ad-hoc
`gh` scripts, again and again. Two groups of it belong in git-tui:

- **The UAT cycle** (sessions of Oct 5, 12 and 13): working out whether a
  ticket's code is on UAT by comparing its PRs against the `uat` branch,
  moving In QA tickets that carry `Ready-for-UAT` into In UAT, passing or
  failing each UAT ticket on its own, and sending a failed ticket to another
  sprint.
- **Board queries** (seven sessions between Sep 14 and Oct 12): lists such as
  "Sprint 29 tickets under the Dave epic still In QA", "In QA without
  Ready-for-UAT, assigned to me", then a copyable list with links. On Sep 30
  the QA could not copy the links out of the terminal at all.

## Scope

In:

1. Linked PRs in Item Detail.
2. Deploy check: is each PR's merge commit on `develop`, `staging`, `uat`,
   `production`.
3. Pass UAT (`U`) passes one ticket; a separate Bulk verify UAT flow (`V`)
   passes a checked list.
4. Move to In UAT (`I`): In QA + `Ready-for-UAT` to In UAT, label removed,
   comment on each, one Slack summary.
5. Fail and Return choose the ticket's sprint in the confirm popup.
6. Search filters by EPIC / root ticket and by assignee.
7. Saved search filters.
8. Copy search results as a Markdown or Slack list with links.
9. The bug form defaults Priority and Stack to `--`.
10. Every new key documented in Help, footers, the QA Actions menu and README,
    and every screen and popup checked before release.

Out (decided 2026-10-14): the other bugs found in the flow audit, a headless
CLI mode, a release-note export, editing assignees, TestRail.

## Part 1: UAT cycle

### Linked PRs

Fetched lazily with the item detail, not in the board sync: the board has
over 2,600 items and almost none are ever opened.

One GraphQL query per opened issue reads both link kinds:

- `closedByPullRequestsReferences(includeClosedPrs: true)`: PRs that close the
  issue ("Fixes #n").
- `timelineItems(itemTypes: [CROSS_REFERENCED_EVENT, CONNECTED_EVENT])` whose
  source or subject is a `PullRequest`: PRs that mention or were manually
  linked to the issue. This is how bundle PRs ("All outstanding FE work for
  s29 (#3920, ...)") and deploy-branch PRs show up.

Per PR: repository, number, title, url, state, merged, merge commit oid, base
branch. Deduplicated by url, sorted by repository then number.

`Item::linked_prs` changes from `Vec<String>` (always empty today) to
`Vec<LinkedPr>`. Cached `items.json` stores it as `[]`, which still
deserializes. Copy as Markdown (`y` in Item Detail) keeps listing the PR URLs.

Item Detail renders one line per PR under `-- Linked PRs (n) --`:

```
marvelx-web#3793        merged -> develop   dev ✓  stg ✓  uat ?  prod ✗
be-compliance-api#2103  merged -> develop   dev ✓  stg ✓  uat ✓  prod ✗
be-compliance-api#2110  open -> develop
```

The environment columns appear once a deploy check has run.

### Deploy check (`d` in Item Detail)

For each merged PR, for each environment branch, on a worker thread:
`gh api repos/{repo}/compare/{branch}...{merge_sha}` and read `.status`.

| `status` | Shown | Meaning |
| --- | --- | --- |
| `identical`, `behind` | ✓ | the merge commit is in the branch |
| `ahead` | ✗ | the branch is an ancestor of the commit: not deployed |
| `diverged` | ? | not in the branch's history, but the code may have arrived by cherry-pick (the FE `deploy-production-sNN` flow does this) |
| 404 or any error | - | branch missing or check failed; the error is shown in the status bar |

Never print "not deployed" for `?`: a cherry-picked or bundled change is
common, and saying "not deployed" when the code is live would mislead.

Environment branches come from a new config field with defaults:

```json
"deploy_branches": { "Dev": "develop", "STG": "staging", "UAT": "uat", "Prod": "production" }
```

All five main code repositories have these four branches (checked
2026-10-14). Results are kept in memory for the session, keyed by
`repo#number`, so the Move to In UAT list can show them.

Open and closed-unmerged PRs are listed but not checked.

### Pass UAT (`U`): one ticket

Same shape as Pass Dev and Pass STG. Guard: the ticket is In UAT. Execution:

1. Re-read the live Status; abort and correct the board if it left In UAT.
2. Set Status to Tech Complete.
3. Comment `Verified on UAT. Moving to Tech Complete.`, cc the devs.
4. Slack: one "Verified on UAT" message (toggle `pass_uat`).

### Bulk verify UAT (`V`)

Opens a checklist screen listing the In UAT tickets in the active sprint
filter (the scope `U` uses today), every row checked, the triggering ticket
included even if the filter hides it.

Keys: arrows move, `Space` toggles a row, `a` toggles all, `Enter` confirms,
`Esc` cancels. Each row: checkbox, number, sprint, title.

`Enter` runs the existing batch: per ticket a live status check, Tech
Complete, a comment; then one Slack "Ready for release" summary (toggle
`pass_uat`). Nothing runs with zero rows checked.

### Move to In UAT (`I`)

Opens the same checklist screen listing In QA tickets carrying
`Ready-for-UAT` in the active sprint filter, plus the triggering ticket. Only
the triggering ticket is pre-checked, because each ticket's deploy is separate.
A row shows the UAT result of a deploy check run this session (✓ / ✗ / ?).

Per checked ticket:

1. Re-read the live Status; skip it if it is no longer In QA.
2. Set Status to In UAT.
3. Remove `Ready-for-UAT`.
4. Comment `Code is on UAT. Moving to In UAT for verification.`

Then one Slack message listing the moved tickets, behind a new toggle
`slack_notify.move_to_uat` (default on, shown in Settings → `n`). Failures
are listed per ticket, as the Pass UAT batch already does.

Guard: offered from In QA tickets with `Ready-for-UAT`. In the QA Actions
menu it is dimmed with a reason otherwise.

### Sprint choice on Fail and Return

The Return confirm popup, and the bug submit confirm popup when the form was
opened through Fail, gain one selector row, cycled with ←/→:

```
Sprint:  < Keep (Sprint 30) >   Today (Sprint 31)   Next (Sprint 32)
```

- Keep: the ticket's current sprint (or `No sprint`). The default.
- Today: the iteration whose dates cover today (`sprint_for_new_item`).
- Next: the iteration that starts after today's.

Choices that resolve to the same sprint collapse into one. Without iteration
dates only Keep is offered.

When the choice is not Keep, the worker writes the ticket's Sprint field after
the status move, and the comment gains `Moving to Sprint N.`. A failed sprint
write is reported as a warning, like the other field writes.

## Part 2: board query

### Root resolution

`resolve_root` and `is_epic` move out of `export.rs` into a shared module so
Search and the reports use one definition. No behaviour change for reports.

### EPIC / root filter

A new Search filter row. Options are the roots that still have at least one
ticket not in Tech Complete, Ready for Release or Done (about 30 today, out of
170 roots), ordered by that open-ticket count, then `No parent`. Labels read
`#2388 RBAC Phase 2`, using the reports' short-name rules.

Stored filter value: `{repo}#{number}` (numbers repeat across the project's
repositories), or `none` for No parent. Include and exclude work like the
other rows: excludes veto, any include must match.

### Assignee filter

A new Search filter row listing every login assigned on the board, sorted, plus
`(none)`. Matching like labels: an exclude vetoes if any assignee matches, an
include passes if any assignee matches.

### Saved filters

Search can't use letter keys (they type into the query), so:

- `Ctrl+S` asks for a name and saves the query and every filter row. Same name
  overwrites.
- `Ctrl+O` opens the saved list: arrows, `Enter` applies, `d` deletes after a
  confirm, `Esc` closes.

Stored in `config.json` as `search_presets` (default empty, so older configs
load unchanged). Sprint filter values equal to the current, previous or next
sprint are saved as `@current`, `@previous`, `@next` and resolved again on
load, so "In QA, this sprint" stays correct next sprint. A preset sprint or
EPIC that no longer exists is dropped on load with a status-bar note.

### Copy results (`Ctrl+Y`)

Opens `Copy N results as: [m] Markdown  [s] Slack`. Copies every current
result in screen order.

Markdown:

```
- [#4224 [FE] Copy changes in Activity Library](https://github.com/Kinatico/kup-project-management/issues/4224) - In QA, Sprint 31, @vxtien-qa
```

Slack (titles escaped the way the reports escape them; names from the Slack
user map, logins otherwise):

```
• <https://github.com/Kinatico/kup-project-management/issues/4224|#4224 [FE] Copy changes in Activity Library> - In QA, Sprint 31, @Bruce Vu
```

Clipboard failures are reported, not hidden.

## Bug form default

`bug_priority_idx` and `bug_stack_idx` default to 0 (`--`), with the comments
corrected. The existing path then waits for GitHub's priority automation
before the Slack message, as it already does when the QA leaves `--`.

## Keys and where they are documented

| Key | Where | Action |
| --- | --- | --- |
| `d` | Item Detail | Deploy check of linked PRs |
| `U` | Item Detail, QA Actions | Pass UAT, this ticket |
| `V` | Item Detail, QA Actions | Bulk verify UAT |
| `I` | Item Detail, QA Actions | Move to In UAT |
| ←/→ | Return and Fail confirm popups | Choose the sprint |
| `Space`, `a`, `Enter` | Checklist screen | Toggle row, toggle all, confirm |
| `Ctrl+S`, `Ctrl+O` | Search | Save filters, open saved filters |
| `Ctrl+Y` | Search | Copy results (then `m` or `s`) |

Every key above must appear in all of:

- the Help screen (`ui/help.rs`), in the section of the screen it belongs to;
- that screen's footer (`footer_line`), within the width rules;
- the QA Actions menu rows (`ui/qa_actions.rs`), for `U`, `V`, `I`, with the
  blocked reason when dimmed;
- README: Keyboard reference, the QA workflow table, Configuration
  (`deploy_branches`, `search_presets`, `slack_notify.move_to_uat`);
- CHANGELOG.

The QA Actions menu text for `U` changes from "moves all N In UAT tickets" to
the single-ticket wording.

## UI verification

1. The render test (`every_screen_renders_at_every_size`) gains the checklist
   screen (both kinds), the saved-filters list, the copy chooser, the sprint
   selector popups, Item Detail with linked PRs and deploy results, and Search
   with the two new rows, at all five sizes (40x8 to 200x60).
2. A text snapshot of every screen and popup at 80x24 and 120x40, read
   screen by screen for clipping, wrapping and footer fit.
3. The real app against the live board, read-only: open tickets with linked
   PRs, run `d`, use the new filters, save and load a filter, copy both formats.
4. GitHub writes (`U`, `V`, `I`, sprint move) are exercised only on tickets
   the QA names, after asking.

## Testing

Unit tests:

- compare `status` to ✓ / ✗ / ? / - mapping;
- linked PR parsing and deduplication from both GraphQL sources;
- sprint choices: keep, today, next, collapse, no iteration dates;
- checklist toggling, toggle all, empty selection does nothing;
- Move to In UAT guard;
- root options and ordering, EPIC filter, assignee filter;
- preset save and load, `@current` / `@previous` / `@next`, stale values
  dropped, old config without `search_presets`;
- both copy formats, including escaping;
- bug form defaults.

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
`cargo test --all-targets` pass, as the release workflow requires.

## Checkpoints

Work lands as one commit per step on `feat/v3.3.0-uat-cycle-board-query`,
each tagged locally `cp/NN-name` (tags are not pushed). Every checkpoint
builds, passes fmt, clippy and tests, and documents its own keys in Help and
the footers, so the QA can stop at any tag and test by hand. To drop a step:
`git revert <commit>`; to go back to a checkpoint: `git reset --hard cp/NN-name`.

| Tag | Step |
| --- | --- |
| `cp/00-spec` | This spec |
| `cp/01-bug-defaults` | Bug form defaults `--` |
| `cp/02-search-epic-assignee` | Shared root module, EPIC and assignee filters |
| `cp/03-search-copy` | Copy results |
| `cp/04-search-presets` | Saved filters |
| `cp/05-linked-prs` | Linked PRs in Item Detail |
| `cp/06-deploy-check` | Deploy check |
| `cp/07-pass-uat-split` | Pass UAT one ticket, Bulk verify UAT checklist |
| `cp/08-move-to-uat` | Move to In UAT |
| `cp/09-sprint-choice` | Sprint choice on Fail and Return |
| `cp/10-docs-ui` | README, CHANGELOG, UI verification fixes |

## Release

Version 3.3.0. CHANGELOG gets a `v3.3.0` section holding the four unreleased
commits on `fix/new-items-follow-the-current-sprint` and this work. The
existing `Unreleased` lines that already shipped in the v3.2.0 tag are left
where they are and called out in review. Merge to `master`, tag `v3.3.0`, push;
the release workflow builds and publishes. Push and tag only after the QA says
go.
