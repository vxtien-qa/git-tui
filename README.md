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
- **QA actions** bound to single keys: pass on Dev, pass on STG, clear UAT
  regression, fail with a bug report, return as not fixed.
- **Report forms** for bugs, enhancements and tasks that fill in the project
  fields, link the ticket to its parent and notify the right people.
- **Search** across titles, numbers and assignees, with include and exclude
  filters on status, priority, stack, sprint and label.
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
| Pass UAT | `U` | In UAT | Moves **every** In UAT ticket to Tech Complete, comments on each, posts one release summary |
| Fail | `f` | any QA column | Opens the bug form, then moves the parent to In Progress and comments with a link to the new bug |
| Return | `r` | anywhere but In Progress | Moves to In Progress, comments, clears both handoff labels |
| Clear handoff | `x` | ticket with a `Ready-for-*` label | Removes the labels, so a returned ticket is testable again |

UAT is a regression pass over everything deployed there, not a per ticket test,
which is why `U` clears the whole column at once and sends a single aggregated
message rather than one notification per ticket.

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

### Item detail

| Key | Action |
| --- | --- |
| `p` | QA actions menu |
| `P`, `U` | Pass STG, Pass UAT |
| `f` | Fail and file a bug |
| `b`, `e`, `t` | File a bug, an enhancement, or a task |
| `c` | Comment |
| `x` | Clear stale handoff labels |
| `h` | Status history |
| `o` | Jump to the parent issue |
| `y`, `w` | Copy as Markdown, open in a browser |
| `r` | Reload |

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
Work outside the paid shifts is tagged as overtime.

**Sprint report** covers the pipeline, the open bugs by bucket, and QA metrics.
The metrics come from the latest QA verdict on each ticket, including
label-only passes, so `passed + failed = tested` always holds. Tickets whose
timeline has not been fetched yet are reported as such rather than quietly
dropped from the totals.

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
  "slack_notify": { "pass_dev": true, "pass_uat": true, "bug_report": true },
  "slack_user_map": {
    "github-username": { "slack_id": "U0EXAMPLE1", "slack_display": "Alex Lee" }
  },
  "stack_leads": { "FE": ["fe-lead"], "BE": ["be-lead"] }
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
