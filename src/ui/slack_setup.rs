use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;
use crate::slack::webhook;

/// Render the Slack Setup screen.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  GIT-TUI   Slack setup",
        theme::title(),
    )));
    lines.push(Line::from(""));

    // ─── Field 1: Webhook URL ───
    let cursor = theme::icon_cursor();
    let is_webhook_active = app.slack_setup_field == 0;

    lines.push(Line::from(Span::styled(
        "  ── Webhook URL (for sending messages) ──────",
        if is_webhook_active {
            theme::highlight()
        } else {
            theme::dim()
        },
    )));
    lines.push(Line::from(""));

    let w_prefix = if is_webhook_active {
        theme::icon_arrow()
    } else {
        " "
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", w_prefix),
            if is_webhook_active {
                theme::highlight()
            } else {
                theme::dim()
            },
        ),
        Span::styled(
            // Always masked - the webhook URL is a bearer-equivalent secret,
            // and re-entering this screen pre-fills the stored value. The
            // masked tail still updates as you type/paste.
            if app.slack_webhook_input.is_empty() {
                if is_webhook_active {
                    format!("[{}]", cursor)
                } else {
                    "[empty]".to_string()
                }
            } else if is_webhook_active {
                format!(
                    "[{}{}]",
                    webhook::mask_url(&app.slack_webhook_input),
                    cursor
                )
            } else {
                format!("[{}]", webhook::mask_url(&app.slack_webhook_input))
            },
            if is_webhook_active {
                theme::selected()
            } else {
                theme::normal()
            },
        ),
    ]));

    // Webhook validation
    let url = app.slack_webhook_input.trim();
    if !url.is_empty() {
        if webhook::validate_url(url) {
            lines.push(Line::from(Span::styled(
                format!("    {} URL format valid", theme::icon_ok()),
                theme::success(),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!(
                    "    {} Must start with https://hooks.slack.com/services/",
                    theme::icon_fail()
                ),
                theme::warning(),
            )));
        }
    }
    lines.push(Line::from(""));

    // ─── Field 2: Bot Token ───
    let is_bot_active = app.slack_setup_field == 1;

    lines.push(Line::from(Span::styled(
        "  ── Bot Token (for user mapping, optional) ──",
        if is_bot_active {
            theme::highlight()
        } else {
            theme::dim()
        },
    )));
    lines.push(Line::from(Span::styled(
        "  Scope: users:read → lets git-tui fetch Slack members",
        theme::dim(),
    )));
    lines.push(Line::from(""));

    let b_prefix = if is_bot_active {
        theme::icon_arrow()
    } else {
        " "
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", b_prefix),
            if is_bot_active {
                theme::highlight()
            } else {
                theme::dim()
            },
        ),
        Span::styled(
            // Always masked (secret) - the tail updates as you type/paste.
            {
                let masked = if app.slack_bot_token_input.is_empty() {
                    String::new()
                } else {
                    let chars: Vec<char> = app.slack_bot_token_input.chars().collect();
                    let tail: String = chars[chars.len().saturating_sub(4)..].iter().collect();
                    format!("xoxb-...{}", tail)
                };
                if is_bot_active {
                    format!("[{}{}]", masked, cursor)
                } else if masked.is_empty() {
                    "[empty - optional]".to_string()
                } else {
                    format!("[{}]", masked)
                }
            },
            if is_bot_active {
                theme::selected()
            } else {
                theme::normal()
            },
        ),
    ]));

    // Bot token validation
    if !app.slack_bot_token_input.is_empty() {
        if crate::slack::users::validate_bot_token(&app.slack_bot_token_input) {
            lines.push(Line::from(Span::styled(
                format!("    {} Token format valid (xoxb-...)", theme::icon_ok()),
                theme::success(),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("    {} Must start with xoxb-", theme::icon_fail()),
                theme::warning(),
            )));
        }
    }
    lines.push(Line::from(""));

    // ─── Status / test result ───
    if let Some(ref status) = app.slack_setup_status {
        let style = if status.contains(theme::icon_ok()) {
            theme::success()
        } else if status.contains(theme::icon_fail()) {
            theme::warning()
        } else {
            theme::highlight()
        };
        lines.push(Line::from(Span::styled(format!("  {}", status), style)));
        lines.push(Line::from(""));
    }

    // Security note
    lines.push(Line::from(Span::styled(
        format!(
            "  {} Secrets are stored locally only (~/.config/git-tui/)",
            theme::icon_warning()
        ),
        theme::dim(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" Slack Setup ", theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Footer
    let footer = super::utils::footer_line(
        &[
            ("Tab", "Switch field"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+U", "Clear"),
            ("Ctrl+O", "Open Slack apps"),
            ("Ctrl+S", "Save"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
