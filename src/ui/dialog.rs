use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::centered_rect;
use crate::app::App;
use crate::models::status::Status;

/// Render the Move dialog (centered popup).
pub fn render_move(f: &mut Frame, app: &App, item_indices: &[usize]) {
    let popup_w = (f.area().width.saturating_sub(4)).min(42);
    let first_item = item_indices.first().and_then(|i| app.items.get(*i));
    let title = if item_indices.len() == 1 {
        if let Some(item) = first_item {
            format!(" Move #{} to... ", item.number.unwrap_or(0))
        } else {
            " Move to... ".to_string()
        }
    } else {
        format!(" Batch Move ({} items) ", item_indices.len())
    };

    let mut lines = vec![Line::from("")];
    for (i, status) in Status::all_columns().iter().enumerate() {
        let is_current = item_indices.len() == 1 && first_item.map(|i| &i.status) == Some(status);
        let is_cursor = i == app.move_cursor;
        let suffix = if is_current { "  (current)" } else { "" };
        let style = if is_cursor {
            theme::selected()
        } else if is_current {
            theme::dim()
        } else {
            theme::normal()
        };
        let pointer = if is_cursor { theme::icon_arrow() } else { " " };

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", pointer), theme::highlight()),
            Span::styled(format!("{}. ", status.move_key()), theme::highlight()),
            Span::styled(format!("{}{}", status.label(), suffix), style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(super::utils::footer_line(
        &[
            (theme::icon_up_down(), "Pick"),
            ("Enter", "Move"),
            ("Esc", "Cancel"),
        ],
        popup_w.saturating_sub(2) as usize,
    ));

    // Height follows the real content (rows + blank lines + footer) so adding
    // a column or a hint can never clip the bottom of the dialog.
    let wanted_h = lines.len() as u16 + 2;
    let area = centered_rect(
        popup_w,
        wanted_h.min(f.area().height.saturating_sub(2)),
        f.area(),
    );
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(title, theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render a confirmation dialog.
pub fn render_confirm(f: &mut Frame, title: &str, message: &str) {
    let popup_w = (f.area().width.saturating_sub(4)).min(50);
    let inner_w = popup_w.saturating_sub(4) as usize;

    let wrapped = wrap_message(message, inner_w);
    let max_h = f.area().height.saturating_sub(4);
    let height = (wrapped.len() as u16 + 4).clamp(6, max_h.max(6));
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);

    let mut lines = vec![Line::from("")];
    for line in &wrapped {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::normal(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [Enter/y] ", theme::highlight()),
        Span::styled("Confirm", theme::normal()),
        Span::raw("    "),
        Span::styled("[Esc/n] ", theme::dim()),
        Span::styled("Cancel", theme::dim()),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(format!(" {} ", title), theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render an error popup.
pub fn render_error(f: &mut Frame, message: &str) {
    let popup_w = (f.area().width.saturating_sub(4)).min(70);
    let inner_w = popup_w.saturating_sub(4) as usize; // border + padding

    let wrapped = wrap_message(message, inner_w);

    let max_h = f.area().height.saturating_sub(4);
    let height = (wrapped.len() as u16 + 4).min(max_h).max(6);
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);

    let mut lines = vec![Line::from("")];
    for line in &wrapped {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::error(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss",
        theme::dim(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::error())
        .title(Span::styled(" Error ", theme::error()));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render a success popup.
pub fn render_success(f: &mut Frame, message: &str) {
    let popup_w = (f.area().width.saturating_sub(4)).min(60);
    let inner_w = popup_w.saturating_sub(4) as usize;

    let wrapped = wrap_message(message, inner_w);
    let max_h = f.area().height.saturating_sub(4);
    let height = (wrapped.len() as u16 + 4).min(max_h).max(6);
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);

    let mut lines = vec![Line::from("")];
    for line in &wrapped {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::normal(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss",
        theme::dim(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::success())
        .title(Span::styled(
            format!(" {} Success ", theme::icon_ok()),
            theme::success(),
        ));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Word-wrap a multi-line message to fit inside a popup (shared by all popups).
fn wrap_message(message: &str, width: usize) -> Vec<String> {
    let message = super::utils::sanitize_glyphs(message);
    let mut wrapped: Vec<String> = Vec::new();
    for line in message.lines() {
        if line.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(super::utils::word_wrap(line, width.max(10)));
        }
    }
    wrapped
}
