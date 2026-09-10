use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;
use crate::gh::notifications::GitHubNotification;

/// Render Notification Hub.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(area);

    let unread_count = app.notifications.iter().filter(|n| n.unread).count();
    let header = Line::from(vec![Span::styled(
        format!(" [i] Notifications ({} unread)", unread_count),
        theme::title(),
    )]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    // Notification list
    let mut lines = Vec::new();
    if app.notifications.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No notifications. Press 'r' to refresh.",
            theme::dim(),
        )));
    } else {
        let visible: Vec<_> = app.notifications.iter().skip(app.list_scroll).collect();

        for (i, notif) in visible.iter().enumerate() {
            let global_i = i + app.list_scroll;
            let is_selected = global_i == app.list_selected;
            let prefix = if is_selected {
                theme::icon_arrow()
            } else {
                " "
            };

            let badge = notification_badge(notif);
            let unread_marker = if notif.unread {
                theme::icon_bullet()
            } else {
                " "
            };

            let style = if is_selected {
                theme::selected()
            } else if notif.unread {
                theme::normal()
            } else {
                theme::dim()
            };

            // Truncate title to fit available width - display-width aware, so
            // emoji/CJK in issue titles can't misalign the time column.
            let avail_w = area.width as usize;
            let title_w = avail_w.saturating_sub(30).max(10);
            let title = super::utils::trunc(&notif.subject.title, title_w);
            let pad =
                (title_w + 2).saturating_sub(unicode_width::UnicodeWidthStr::width(title.as_str()));

            lines.push(Line::from(vec![
                Span::styled(format!(" {} {} ", prefix, unread_marker), style),
                badge,
                Span::styled(format!("{}{}", title, " ".repeat(pad)), style),
                Span::styled(
                    format!(" {} ", format_time(&notif.updated_at)),
                    theme::dim(),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("        "),
                Span::styled(
                    format!("{} · {}", notif.repository.full_name, notif.reason),
                    theme::dim(),
                ),
            ]));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(
            super::utils::position_title(app.list_selected, app.notifications.len()),
            theme::dim(),
        ));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, chunks[1]);

    // Footer
    let footer = super::utils::footer_line(
        &[
            (theme::icon_up_down(), "Nav"),
            ("Enter", "Open"),
            ("r", "Refresh"),
            ("m", "Mark read"),
            ("a", "Mark all"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

fn notification_badge(notif: &GitHubNotification) -> Span<'static> {
    match notif.reason.as_str() {
        "assign" => Span::styled("[Assign]   ", theme::success()),
        "mention" => Span::styled("[@Mention] ", theme::warning()),
        "review_requested" => Span::styled("[Review]   ", theme::highlight()),
        "state_change" => Span::styled("[State]    ", theme::normal()),
        "comment" => Span::styled(
            "[Comment]  ",
            ratatui::style::Style::default().fg(theme::ACCENT),
        ),
        "subscribed" => Span::styled("[Sub'd]    ", theme::dim()),
        _ => Span::styled("[Other]    ", theme::dim()),
    }
}

fn format_time(time_str: &str) -> String {
    // Simple: just show date part (MM-DD from "2026-02-23T...")
    let chars: Vec<char> = time_str.chars().collect();
    if chars.len() >= 10 {
        chars[5..10].iter().collect()
    } else {
        time_str.to_string()
    }
}
