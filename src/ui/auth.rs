use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// Render the auth/startup screen with contextual instructions.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![
        Line::from(Span::styled(
            "  GIT-TUI   Authentication   step 1 of 3",
            theme::title(),
        )),
        Line::from(""),
    ];

    // ── Status section ──
    if let Some(ref status_text) = app.auth_status_text {
        for line in status_text.lines() {
            let style = if line.starts_with(theme::icon_ok()) || line.contains(theme::icon_ok()) {
                theme::success()
            } else if line.starts_with(theme::icon_fail()) || line.contains(theme::icon_fail()) {
                theme::warning()
            } else if line.starts_with("  macOS:")
                || line.starts_with("  Windows:")
                || line.starts_with("  Linux:")
            {
                theme::highlight()
            } else {
                theme::normal()
            };
            lines.push(Line::from(Span::styled(format!("  {}", line), style)));
        }
    } else {
        lines.push(Line::from(Span::styled("  Checking...", theme::dim())));
    }

    lines.push(Line::from(""));

    // ── Feedback line ──
    if let Some(ref feedback) = app.auth_feedback {
        let style = if feedback.starts_with(theme::icon_ok()) || feedback.contains(theme::icon_ok())
        {
            theme::success()
        } else if feedback.starts_with(theme::icon_fail()) || feedback.contains(theme::icon_fail())
        {
            theme::warning()
        } else {
            theme::highlight()
        };
        lines.push(Line::from(Span::styled(format!("  {}", feedback), style)));
    } else {
        lines.push(Line::from(Span::styled(
            "  Auto-checks every 10 seconds",
            theme::dim(),
        )));
    }

    lines.push(Line::from(""));

    // ── Actions ──
    lines.push(Line::from(Span::styled(
        "  ── Actions ──────────────────────────────────",
        theme::dim(),
    )));
    lines.push(Line::from(""));

    // Show context-aware actions based on auth state
    let gh_installed = app.auth_state.as_ref().is_none_or(|a| a.gh_installed);

    if !gh_installed {
        // gh not installed - show install actions
        lines.push(Line::from(vec![
            Span::styled("  [o] ", theme::highlight()),
            Span::styled(
                "Open download page (https://cli.github.com)",
                theme::normal(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [c] ", theme::highlight()),
            Span::styled("Copy install command", theme::normal()),
        ]));
    } else {
        // gh installed - show fix + check actions
        lines.push(Line::from(vec![
            Span::styled("  [f] ", theme::highlight()),
            Span::styled("Fix automatically (opens browser)", theme::normal()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [c] ", theme::highlight()),
            Span::styled("Copy command to clipboard", theme::normal()),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  [Enter] ", theme::highlight()),
        Span::styled("Check now   ", theme::dim()),
        Span::styled("[q/Esc] ", theme::highlight()),
        Span::styled("Quit", theme::dim()),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" Git TUI ", theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
