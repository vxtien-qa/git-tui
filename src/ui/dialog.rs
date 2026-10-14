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

/// Render a confirm with a sprint selector (←/→) under the message.
pub fn render_confirm_sprint(
    f: &mut Frame,
    title: &str,
    message: &str,
    choices: &[crate::app::SprintChoice],
    selected: usize,
) {
    let popup_w = (f.area().width.saturating_sub(4)).min(64);
    let inner_w = popup_w.saturating_sub(4) as usize;

    let mut lines = vec![Line::from("")];
    for line in wrap_message(message, inner_w) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::normal(),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Sprint:", theme::dim())));
    // Chips wrap so every choice stays visible on a narrow terminal.
    let mut row: Vec<Span> = vec![Span::raw("  ")];
    let mut row_w = 2;
    for (i, choice) in choices.iter().enumerate() {
        let is_sel = i == selected;
        let chip = if is_sel {
            format!(
                "{} {} {}",
                theme::icon_left_arrow(),
                choice.label,
                theme::icon_right_arrow()
            )
        } else {
            format!("  {}  ", choice.label)
        };
        let w = chip.chars().count() + 1;
        if row_w + w > inner_w && row_w > 2 {
            lines.push(Line::from(std::mem::take(&mut row)));
            row = vec![Span::raw("  ")];
            row_w = 2;
        }
        row.push(Span::styled(
            chip,
            if is_sel {
                theme::selected()
            } else {
                theme::dim()
            },
        ));
        row.push(Span::raw(" "));
        row_w += w;
    }
    lines.push(Line::from(row));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [Enter/y] ", theme::highlight()),
        Span::styled("Confirm", theme::normal()),
        Span::raw("  "),
        Span::styled(
            format!(
                "[{}/{}] ",
                theme::icon_left_arrow(),
                theme::icon_right_arrow()
            ),
            theme::highlight(),
        ),
        Span::styled("Sprint", theme::normal()),
        Span::raw("  "),
        Span::styled("[Esc/n] ", theme::dim()),
        Span::styled("Cancel", theme::dim()),
    ]));

    let max_h = f.area().height.saturating_sub(2);
    let height = (lines.len() as u16 + 2).min(max_h);
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(format!(" {} ", title), theme::title()));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render a pick-one popup: a message, then one `[key] label` row per choice.
pub fn render_choice(f: &mut Frame, title: &str, message: &str, choices: &[(&str, &str)]) {
    let popup_w = (f.area().width.saturating_sub(4)).min(50);
    let inner_w = popup_w.saturating_sub(4) as usize;

    let mut lines = vec![Line::from("")];
    for line in wrap_message(message, inner_w) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::normal(),
        )));
    }
    lines.push(Line::from(""));
    for (key, label) in choices {
        lines.push(Line::from(vec![
            Span::styled(format!("  [{}] ", key), theme::highlight()),
            Span::styled(label.to_string(), theme::normal()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [Esc] ", theme::dim()),
        Span::styled("Cancel", theme::dim()),
    ]));

    let max_h = f.area().height.saturating_sub(2);
    let height = (lines.len() as u16 + 2).min(max_h);
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(format!(" {} ", title), theme::title()));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render a one-line text prompt.
pub fn render_text_prompt(f: &mut Frame, title: &str, message: &str, value: &str) {
    let popup_w = (f.area().width.saturating_sub(4)).min(56);
    let inner_w = popup_w.saturating_sub(4) as usize;

    let mut lines = vec![Line::from("")];
    for line in wrap_message(message, inner_w) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::normal(),
        )));
    }
    lines.push(Line::from(""));
    // Keep the end of a long value (where the typing happens) in view.
    let room = inner_w.saturating_sub(2);
    let shown: String = {
        let chars: Vec<char> = value.chars().collect();
        let skip = chars.len().saturating_sub(room);
        chars[skip..].iter().collect()
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{}{}", shown, theme::icon_cursor()),
            theme::selected(),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [Enter] ", theme::highlight()),
        Span::styled("Save", theme::normal()),
        Span::raw("    "),
        Span::styled("[Esc] ", theme::dim()),
        Span::styled("Cancel", theme::dim()),
    ]));

    let max_h = f.area().height.saturating_sub(2);
    let height = (lines.len() as u16 + 2).min(max_h);
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(format!(" {} ", title), theme::title()));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Render a list to pick one entry from, with Enter / d / Esc hints.
pub fn render_pick_list(f: &mut Frame, title: &str, entries: &[String], selected: usize) {
    let popup_w = (f.area().width.saturating_sub(4)).min(56);
    let inner_w = popup_w.saturating_sub(4) as usize;
    // Rows that fit between the border, the blank lines and the footer.
    let max_rows = (f.area().height.saturating_sub(8) as usize).max(1);
    let skip = selected.saturating_sub(max_rows.saturating_sub(1));

    let mut lines = vec![Line::from("")];
    for (i, entry) in entries.iter().enumerate().skip(skip).take(max_rows) {
        let is_sel = i == selected;
        let marker = if is_sel { theme::icon_arrow() } else { " " };
        lines.push(Line::from(Span::styled(
            format!(
                " {} {}",
                marker,
                super::utils::trunc(entry, inner_w.saturating_sub(3))
            ),
            if is_sel {
                theme::selected()
            } else {
                theme::normal()
            },
        )));
    }
    lines.push(Line::from(""));
    lines.push(super::utils::footer_line(
        &[("Enter", "Load"), ("d", "Delete"), ("Esc", "Close")],
        inner_w + 2,
    ));

    let max_h = f.area().height.saturating_sub(2);
    let height = (lines.len() as u16 + 2).min(max_h);
    let area = centered_rect(popup_w, height, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(
            format!(
                " {}{}",
                title,
                super::utils::position_title(selected, entries.len())
            ),
            theme::title(),
        ));
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
