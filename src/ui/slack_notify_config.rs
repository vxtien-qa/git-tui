use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;
use crate::config::SlackNotifyConfig;
use crate::slack::webhook;

/// Render the Slack Notification Config screen.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Slack notification settings",
        theme::title(),
    )));
    lines.push(Line::from(""));

    // Status
    if app.config.is_slack_configured() {
        if let Some(ref url) = app.config.slack_webhook_url {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} Connected: ", theme::icon_ok()),
                    theme::success(),
                ),
                Span::styled(webhook::mask_url(url), theme::dim()),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "  {} Slack not configured - set up webhook first in Settings [i]",
                theme::icon_fail()
            ),
            theme::warning(),
        )));
    }
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  Toggle which events send Slack notifications.",
        theme::dim(),
    )));
    lines.push(Line::from(Span::styled(
        "  Press Space to toggle, Esc to save & go back.",
        theme::dim(),
    )));
    lines.push(Line::from(""));

    // ─── Toggle list ───
    lines.push(Line::from(Span::styled(
        "  ── Event Toggles ───────────────────────────",
        theme::dim(),
    )));
    lines.push(Line::from(""));

    // The toggles outgrew an 80x24 terminal: remember where the cursor's
    // row lands so the panel can scroll to it.
    let first_toggle_line = lines.len();
    for (i, label) in SlackNotifyConfig::LABELS.iter().enumerate() {
        let enabled = app.config.slack_notify.get(i);
        let is_selected = i == app.slack_notify_cursor;

        let marker = if is_selected {
            format!(" {} ", theme::icon_arrow())
        } else {
            "   ".to_string()
        };
        let symbol = if enabled {
            Span::styled(format!("{} ", theme::icon_ok()), theme::success())
        } else {
            Span::styled(format!("{} ", theme::icon_fail()), theme::dim())
        };
        let toggle_label = if enabled { " ON " } else { " OFF" };

        let style = if is_selected {
            theme::selected()
        } else {
            theme::normal()
        };

        lines.push(Line::from(vec![
            Span::styled(
                marker,
                if is_selected {
                    theme::highlight()
                } else {
                    theme::dim()
                },
            ),
            symbol,
            Span::styled(format!("{:<18}", label), style),
            Span::styled(
                format!("[{}]", toggle_label),
                if enabled {
                    theme::success()
                } else {
                    theme::dim()
                },
            ),
        ]));
    }

    lines.push(Line::from(""));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" Notification Settings ", theme::title()));

    let inner_h = chunks[0].height.saturating_sub(2) as usize;
    let cursor_line = first_toggle_line + app.slack_notify_cursor;
    let scroll = if inner_h > 0 && cursor_line >= inner_h {
        (cursor_line + 1 - inner_h) as u16
    } else {
        0
    };
    f.render_widget(
        Paragraph::new(lines).block(block).scroll((scroll, 0)),
        chunks[0],
    );

    // Footer
    let footer = super::utils::footer_line(
        &[
            (theme::icon_up_down(), "Navigate"),
            ("Space", "Toggle"),
            ("Esc", "Save & Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
