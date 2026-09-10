# Security policy

## Supported versions

The latest release is the only supported version.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/vxtien-qa/git-tui/security/advisories/new)
instead, or contact the maintainer directly.

Include what you found, how to reproduce it, and what an attacker could do with
it. You can expect an acknowledgement within a few days.

## What git-tui stores

- **No GitHub credentials.** Every GitHub call is made through the
  [GitHub CLI](https://cli.github.com/), which holds its own token. git-tui
  never reads, stores or transmits it.
- **Slack credentials in plain text.** The incoming webhook URL and the bot
  token are stored in the config file, unencrypted, with permissions inherited
  from your home directory. They are masked in the interface. Treat the config
  file as a secret: anyone who can read it can post to your Slack workspace.
- **Cached ticket data.** Titles, bodies, comments and timelines are cached
  unencrypted under the cache directory so the app can start instantly. Clear
  it from Settings with `c`.

Both paths are listed in the [README](README.md#configuration).
