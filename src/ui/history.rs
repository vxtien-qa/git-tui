use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// Render Status History screen.
pub fn render(f: &mut Frame, app: &App, item_idx: usize, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),    // timeline
            Constraint::Length(1), // footer
        ])
        .split(area);

    let item = app.items.get(item_idx);
    let title = if let Some(item) = item {
        format!(" [H] History - #{} ", item.number.unwrap_or(0))
    } else {
        " [H] History ".to_string()
    };

    let header = Line::from(Span::styled(title, theme::title()));
    f.render_widget(Paragraph::new(header), chunks[0]);

    // Timeline events
    let mut lines = Vec::new();
    if app.history_events.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No history events. Press 'r' to fetch.",
            theme::dim(),
        )));
    } else {
        for event in &app.history_events {
            // Parse event string "date|actor|action"
            let parts: Vec<&str> = event.splitn(3, '|').collect();
            if parts.len() == 3 {
                let date = parts[0];
                let actor = parts[1];
                let action = parts[2];

                let icon = if action.contains("Pass") || action.contains("Tech Complete") {
                    "[+]"
                } else if action.contains("Fail") || action.contains("In Progress") {
                    "[-]"
                } else if action.contains("comment") {
                    "[c]"
                } else if action.contains("created") {
                    "[*]"
                } else {
                    "[~]"
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", date), theme::dim()),
                    Span::styled(format!("{} ", icon), theme::normal()),
                    Span::styled(format!("@{} ", actor), theme::highlight()),
                    Span::styled(action.to_string(), theme::normal()),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  {}", event),
                    theme::normal(),
                )));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border());

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, chunks[1]);

    // Footer
    let footer = super::utils::footer_line(
        &[
            (theme::icon_up_down(), "Scroll"),
            ("r", "Refresh"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[2]);
}
