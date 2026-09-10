# Contributing

Thanks for taking an interest. Bug reports, feature requests and pull requests
are all welcome.

## Before you start

git-tui is built around one specific QA pipeline: the status columns, the
handoff label names and the report templates are compiled in. If your change
depends on a different board layout, open an issue first so we can talk about
making that part configurable rather than swapping one hardcoded workflow for
another.

## Development setup

```bash
git clone https://github.com/vxtien-qa/git-tui.git
cd git-tui
cargo build
cargo test
```

You need Rust 1.88 or newer and the [GitHub CLI](https://cli.github.com/)
authenticated, since every GitHub call goes through `gh`.

## Before opening a pull request

```bash
cargo fmt
cargo clippy --all-targets    # must be free of warnings
cargo test
```

All three are enforced by CI on Linux, macOS and Windows.

## What a good change looks like

- **Tests for logic.** Anything that decides what gets written to GitHub, or
  what a report says, belongs in a unit test. See `src/export.rs` for how the
  QA metrics are tested.
- **Rendering stays covered.** `src/main.rs` has a test that draws every screen
  at five terminal sizes. Add new screens to `every_screen()` so layout
  regressions are caught without a terminal.
- **No silent failures.** If a GitHub write can fail, report it. Several bugs in
  this codebase came from `let _ = result` swallowing an error while the UI
  claimed success.
- **Comments explain why.** The code is read more often than it is written, and
  most of the tricky parts exist because of a specific past failure. Say what
  that failure was.
- **No emoji in the interface**, and no em-dashes anywhere. Icons go through
  `src/ui/theme.rs`, which has ASCII fallbacks for terminals without Unicode.
- **Footers and layouts stay responsive.** The minimum supported terminal is
  60x10. Use `ui::utils::footer_line` for key hints so they fit any width.

## Project layout

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Reporting a bug

Open an issue with the version (`git-tui` shows it in the sidebar), your
platform and terminal, and what you expected to happen. If the app printed a
message, include it verbatim.

## Security

Do not open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md).
