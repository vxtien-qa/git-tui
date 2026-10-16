# git-tui

A terminal UI for running QA on a GitHub Projects board. It turns the work a QA
engineer repeats dozens of times a day, moving a ticket through the test
pipeline, filing a bug against it, telling the developers, into a single
keystroke each, without leaving the keyboard or opening a browser tab.

[![CI](https://github.com/vxtien-qa/git-tui/actions/workflows/ci.yml/badge.svg)](https://github.com/vxtien-qa/git-tui/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange)](https://www.rust-lang.org)
![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey)

git-tui talks to GitHub through the [GitHub CLI](https://cli.github.com/), so it
holds no tokens of its own and inherits whatever access `gh` already has.

## Contents

- [What it does](#what-it-does)
- [Requirements](#requirements)
- [Install](#install)
- [First run](#first-run)
- [The QA workflow](#the-qa-workflow)
- [Screens](#screens)
- [Keyboard reference](#keyboard-reference)
- [Reports](#reports)
- [Slack](#slack)
- [Configuration](#configuration)
- [Development](#development)
- [Limitations](#limitations)

## What it does

- **Kanban board** over all 11 status columns of the project, with batch moves.
- **My Tasks**, the tickets waiting on you, grouped by status, plus the ones you
  already passed that are waiting for a deploy.
- **QA actions** bound to single keys: pass on Dev, pass on STG, pass on UAT
  one ticket or a picked batch, move tickets whose code reached UAT into
  In UAT, fail with a bug report, return as not fixed, optionally into another
  sprint.
- **Linked PRs and a deploy check** in the ticket view: which of `develop`,
  `staging`, `uat` and `production` already have each merged PR.
- **Report forms** for bugs, enhancements and tasks that fill in the project
  fields, link the ticket to its parent and notify the right people.
- **Search** across titles, numbers and assignees, with include and exclude
  filters on status, priority, stack, sprint, label, EPIC and assignee, saved
  filters, and a copy of the results as a linked list for GitHub, Notion or
  Slack.
- **Reports**: a daily standup and a sprint report with QA metrics, exported as
  Markdown, plain text, or posted to Slack.
- **Slack notifications** for every QA action, with per-action toggles.

## Requirements

- [GitHub CLI](https://cli.github.com/) 2.x, authenticated.
- A GitHub Projects (v2) board owned by an organisation.
- Scopes `repo`, `project` and `read:org`:

  ```bash
  gh auth login -s repo,project,read:org
  ```

Install `gh` with `brew install gh` on macOS, `sudo apt install gh` on Debian or
Ubuntu, or `winget install GitHub.cli` on Windows.

## Install

### From the latest release

macOS, Linux, or Windows with Git Bash:

```bash
curl -fsSL https://raw.githubusercontent.com/vxtien-qa/git-tui/master/install.sh | bash
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/vxtien-qa/git-tui/master/install.ps1 | iex
```

The installer detects your platform, downloads the matching binary and puts it
on your PATH. The binaries are not code signed, so Windows SmartScreen may warn
on first run; choose **More info**, then **Run anyway**.

### From source

The install script sets up Rust, and on Windows the Visual Studio C++ Build
Tools, before building:

```bash
git clone https://github.com/vxtien-qa/git-tui.git
cd git-tui
./install.sh --build          # PowerShell: .\install.ps1 -Build
```

On Windows use the PowerShell script. Git Bash cannot install the Build Tools.

### Manually

Download a binary from [Releases](https://github.com/vxtien-qa/git-tui/releases):

| Platform | Asset |
| --- | --- |
| Linux x64 | `git-tui-linux-x64` |
| macOS Apple silicon | `git-tui-darwin-arm64` |
| macOS Intel | `git-tui-darwin-x64` |
| Windows x64 | `git-tui-windows-x64.exe` |

```bash
chmod +x git-tui-*
sudo mv git-tui-* /usr/local/bin/git-tui
```

## First run

Run `git-tui`. On first launch it checks `gh`, then opens a setup wizard that
takes two URLs:

1. The GitHub Project URL, for example
   `https://github.com/orgs/acme/projects/9`.
2. The repository where bug reports are filed, for example
   `https://github.com/acme/project-board`.

Both are parsed for you. Settings are written to a config file you can also edit
by hand; see [Configuration](#configuration). The wizard is available later from
Settings with `s`.

## The QA workflow

git-tui models one specific pipeline. Tickets move left to right; QA owns three
of the columns.

```
Backlog -> Ready for Dev -> In Progress -> In Review
              -> In QA - Dev -> In QA -> In UAT
                    -> Tech Complete -> Ready for Release -> Done

Blocked sits beside the pipeline and any column can move into it.
```

Two of the three passes deliberately do not move the ticket. QA marks the
handoff with a label and the developers move the ticket once they have deployed,
so the board always reflects what is actually deployed where.

| Action | Key | Valid from | What it writes |
| --- | --- | --- | --- |
| Pass Dev | `p` | In QA - Dev | Adds `Ready-for-Staging`, comments tagging the stack lead |
| Pass STG | `P` | In QA | Adds `Ready-for-UAT`, comments tagging the stack lead |
| Move to In UAT | `I` | In QA with `Ready-for-UAT` | Pick list of those tickets in the sprint; each checked one moves to In UAT, loses the label, gets a comment; one Slack message |
| Pass UAT | `U` | In UAT | Moves the ticket to Tech Complete, comments, one Slack message |
| Bulk verify UAT | `V` | In UAT | Pick list of the sprint's In UAT tickets, all checked; passes the checked ones together, one release summary |
| Fail | `f` | any QA column | Opens the bug form, then moves the parent to In Progress and comments with a link to the new bug |
| Return | `r` | anywhere but In Progress | Moves to In Progress, comments, clears both handoff labels |
| Clear handoff | `x` | ticket with a `Ready-for-*` label | Removes the labels, so a returned ticket is testable again |

A UAT regression run covers everything deployed there, so `V` passes a picked
batch at once and sends a single aggregated message rather than one
notification per ticket. `U` is for a ticket verified on its own.

Fail and Return ask where the ticket goes next: keep its sprint, move it to the
sprint running today, or to the next one (arrow keys in the confirm). Keep is
the default. On Fail the choice is for the parent being sent back, not for the
new bug, which the form files into the sprint running today.

Before moving a ticket to In UAT, `d` in the ticket view checks its linked PRs
against the deploy branches, and `d` in the Move to In UAT list checks every
ticket in it. `✓` means the branch has the PR's merge commit, `✗` that it does
not, and `?` that the commit is not in the branch's history but the change may
have arrived by cherry-pick, so it is never read as "not deployed". `-` means
GitHub found no such branch, commit or repository, and `!` that the check
failed. The list's UAT column is `✓` only when every open or merged linked PR
is merged and on UAT; a key to the marks shows under the list once a check has
run. A ticket only shows the PRs GitHub links to it; one whose PR was never
linked shows "none linked on GitHub". If a later refresh of the PR list fails,
the last list stays, marked as such, and `d` checks that one.

The tickets waiting for a UAT deploy are listed in My Tasks under Waiting for
deploy; `I` on one of them opens Move to In UAT, with the cursor on that
ticket.

Each action is only offered from the column it belongs to. The QA actions menu
(`p`) lists them all and dims the ones that do not apply, with the reason.

## Screens

| Screen | Key | Purpose |
| --- | --- | --- |
| Board | `1` | All columns, batch select and move |
| My Tasks | `2` | Your queue by status, plus tickets waiting for a deploy |
| Search | `3` | Keyword search and multi dimensional filters |
| Settings | `4` | Config, Slack, stack leads, cache |
| Dashboard | `5` | Sprint stats and report export |
| Notifications | `6` | GitHub notifications |
| Help | `?` | Full key reference |

## Keyboard reference

`?` opens the complete list inside the app. The essentials:

### Global

| Key | Action |
| --- | --- |
| `1` to `6` | Jump to a screen |
| `Esc` | Back |
| `?` | Help |
| `q` | Quit |
| `Ctrl+C` | Force quit |

### Board and My Tasks

| Key | Action |
| --- | --- |
| Arrows | Move between columns and rows |
| `Enter` | Open the ticket |
| `Space` | Select for a batch move (board) |
| `p` | QA actions menu |
| `m` | Move, with arrows and `Enter` or the digit shortcuts |
| `s` | Cycle the sprint filter |
| `r` | Refresh |
| `I`, `x` | My Tasks, on a Waiting for deploy row: Move to In UAT, clear stale handoff labels |

### Item detail

| Key | Action |
| --- | --- |
| `p` | QA actions menu |
| `P`, `U` | Pass STG, Pass UAT |
| `V`, `I` | Bulk verify UAT, Move to In UAT |
| `d` | Deploy check of the linked PRs |
| `f` | Fail and file a bug |
| `b`, `e`, `t` | File a bug, an enhancement, or a task |
| `c` | Comment |
| `x` | Clear stale handoff labels |
| `h` | Status history |
| `o` | Jump to the parent issue |
| `y`, `w` | Copy as Markdown, open in a browser |
| `r` | Reload |

### Search

| Key | Action |
| --- | --- |
| `Tab` | Next filter row: Status, Priority, Stack, Sprint, Label, EPIC, Assignee |
| Left, Right | Move between a row's options |
| `Space` | Include, exclude, off |
| `Ctrl+Y` | Copy the results: `m` Markdown, `s` Slack |
| `Ctrl+S`, `Ctrl+O` | Save the filters under a name, open the saved ones (`Enter` loads, `d` deletes) |
| `Ctrl+L` | Clear the filters |

### Pick lists (Bulk verify UAT, Move to In UAT)

| Key | Action |
| --- | --- |
| `Space`, `a` | Check a ticket, check all or none |
| `d` | Deploy check every ticket in the list; the UAT column fills in |
| `Enter` | Confirm the checked tickets (asks first) |
| `Esc` | Back without doing anything |

### Confirm popups

| Key | Action |
| --- | --- |
| `Enter` / `y` | Confirm |
| `Esc` / `n` | Cancel |
| Left, Right | Pick the sprint in a Fail or Return confirm |

### Forms

| Key | Action |
| --- | --- |
| `Tab` | Next field |
| Up, Down | Move by line inside a field, or between fields at its edge |
| Left, Right | Move the cursor, or cycle a selector |
| `Ctrl+V` | Paste |
| `Ctrl+S` | Submit |
| `Esc` | Back, keeping the draft |

Bugs and enhancements share one form. `b` files a bug with the label `Bug` and
issue type `Bug`; `e` files an enhancement with the labels `Enhancement` and
`Task` and issue type `Task`.

## Reports

The Dashboard exports two reports, each in Markdown, plain text, or straight to
Slack.

**Daily standup** covers what you did on the previous working day, from the
ticket timeline rather than from memory. On a Monday it reaches back to Friday.
Work outside the paid shifts is tagged as overtime. A move you reverse yourself
within half an hour is treated as a misclick and left out. Every line is
tagged with the ticket's EPIC, or with the root ticket it hangs under.

**Sprint report** covers the pipeline, the open bugs by bucket, QA metrics,
and a rollup by EPIC or root ticket. Done means Tech Complete, Ready for
Release or Done. The metrics come from the latest QA verdict on each ticket
since the sprint's first day, including label-only passes, so
`passed + failed = tested` always holds and a carried-over ticket's verdict
from a previous sprint does not count. Tickets whose timeline has not been
fetched yet are reported as such rather than quietly dropped from the totals.

| Key | Report |
| --- | --- |
| `d`, `D` | Daily standup as Markdown, as text |
| `e`, `E` | Sprint report as Markdown, as text |
| `S`, `X` | Post the standup, the sprint report, to Slack |

## Slack

Slack is optional. Configure it in Settings:

| Key | Sets up |
| --- | --- |
| `i` | Incoming webhook URL and bot token |
| `n` | Per action notification toggles |
| `u` | GitHub username to Slack user mapping, so mentions resolve |
| `l` | Stack leads, who get tagged on a handoff |

Without a user mapping, notifications name people as plain text instead of real
mentions. A ticket with no Stack, or a Stack with no lead mapped, tags its
assignees rather than everyone.

## Configuration

| Platform | Config | Cache |
| --- | --- | --- |
| macOS, Linux | `~/.config/git-tui/config.json` | `~/.cache/git-tui/` |
| Windows | `%APPDATA%\git-tui\config.json` | `%LOCALAPPDATA%\git-tui\` |

```json
{
  "owner": "acme",
  "project_number": 9,
  "bug_repo": "acme/project-board",
  "poll_interval_secs": 180,
  "timezone_offset_hours": 7,
  "slack_enabled": true,
  "slack_webhook_url": "https://hooks.slack.com/services/...",
  "slack_bot_token": "xoxb-...",
  "slack_notify": { "pass_dev": true, "pass_uat": true, "move_to_uat": true, "bug_report": true },
  "slack_user_map": {
    "github-username": { "slack_id": "U0EXAMPLE1", "slack_display": "Alex Lee" }
  },
  "stack_leads": { "FE": ["fe-lead"], "BE": ["be-lead"] },
  "deploy_branches": [
    { "env": "Dev", "branch": "develop" },
    { "env": "STG", "branch": "staging" },
    { "env": "UAT", "branch": "uat" },
    { "env": "Prod", "branch": "production" }
  ],
  "search_presets": [
    { "name": "In QA, not passed STG, mine", "status": ["In QA"], "label": ["!Ready-for-UAT"], "assignee": ["vxtien-qa"], "sprint": ["@current"] }
  ]
}
```

| Field | Meaning | Default |
| --- | --- | --- |
| `owner` | Organisation that owns the project | required |
| `project_number` | Project number from its URL | required |
| `bug_repo` | Fallback repository for standalone reports | required |
| `poll_interval_secs` | Background refresh interval | `180` |
| `timezone_offset_hours` | Offset used for every date in reports | `7` |
| `slack_enabled` | Master switch for notifications | `false` |
| `slack_notify.*` | One toggle per action | all `true` |
| `slack_user_map` | GitHub username to Slack ID and display name | empty |
| `stack_leads` | Stack to lead usernames, tagged on a handoff | empty |
| `deploy_branches` | Environments and branches the deploy check compares with | Dev, STG, UAT, Prod = `develop`, `staging`, `uat`, `production` |
| `search_presets` | Saved Search filters; sprints as `@current`, `@previous`, `@next` | empty |

The webhook URL and bot token are stored in plain text, as `gh` stores its own
credentials. Both are masked in the UI.

Set `GIT_TUI_UNICODE=1` or `0` to force Unicode icons on or off; by default they
are detected from the terminal.

## Development

```bash
cargo test
cargo clippy --all-targets    # no warnings expected
cargo fmt --check
cargo build --release
```

Rendering is covered by a test that draws every screen at five terminal sizes,
from 40x8 up, which is how layout regressions get caught without a terminal. To
inspect a screen as text:

```bash
cargo test snapshot -- --ignored --nocapture
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the module layout and the
data flow, and [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

Built with [Ratatui](https://ratatui.rs/) and
[Crossterm](https://github.com/crossterm-rs/crossterm), talking to GitHub
entirely through the [GitHub CLI](https://cli.github.com/).

## Limitations

- Projects owned by an organisation only. A user owned project is not supported,
  because the sync queries `organization`.
- The status columns, the handoff label names and the report templates are
  compiled in. A board with different columns will report a mismatch rather than
  adapt; see the issue tracker if you need this configurable.
- Draft items on the board cannot be commented on or moved, so QA actions skip
  them and say so.

## License

[MIT](LICENSE)
