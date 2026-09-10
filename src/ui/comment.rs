use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::word_wrap;
use crate::app::App;

/// Render Comment screen.
pub fn render(f: &mut Frame, app: &App, item_idx: usize, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),    // text area
            Constraint::Length(1), // footer
        ])
        .split(area);

    let item = app.items.get(item_idx);
    let title = if let Some(item) = item {
        format!(" [c] Comment on #{} ", item.number.unwrap_or(0))
    } else {
        " [c] Comment ".to_string()
    };

    let header = Line::from(Span::styled(title, theme::title()));
    f.render_widget(Paragraph::new(header), chunks[0]);

    // Text area: word-wrapped so long lines are never typed blind, and
    // scrolled so the cursor line always stays visible. The cursor glyph is
    // inserted into the text at the cursor position (Left/Right/Home/End move it).
    let inner_w = chunks[1].width.saturating_sub(4) as usize; // borders + padding
    let view_h = chunks[1].height.saturating_sub(2) as usize; // borders

    let text = &app.comment_text;
    let mut display_lines: Vec<Line> = Vec::new();
    let mut cursor_line_idx = 0usize;
    if text.is_empty() {
        display_lines.push(Line::from(Span::styled(
            format!("  Type your comment here...{}", theme::icon_cursor()),
            theme::dim(),
        )));
    } else {
        let glyph = theme::icon_cursor();
        let display =
            crate::handlers::text_edit::with_cursor_glyph(text, app.comment_cursor, glyph);
        // split('\n') (not .lines()) so a trailing newline yields the empty
        // line the user just created with Enter.
        for raw in display.split('\n') {
            let wrapped = if raw.is_empty() {
                vec![String::new()]
            } else {
                word_wrap(raw, inner_w.max(10))
            };
            for w in wrapped {
                if w.contains(glyph) {
                    cursor_line_idx = display_lines.len();
                }
                display_lines.push(Line::from(Span::styled(
                    format!("  {}", w),
                    theme::normal(),
                )));
            }
        }
    }

    // Keep the cursor line in view.
    let view = view_h.max(1);
    let scroll = if cursor_line_idx >= view {
        (cursor_line_idx + 1 - view) as u16
    } else {
        0
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border());

    let paragraph = Paragraph::new(display_lines)
        .block(block)
        .scroll((scroll, 0));
    f.render_widget(paragraph, chunks[1]);

    // Footer with character count (chars, not bytes - matters for Vietnamese).
    // The counter is reserved first so the hints shrink around it.
    let char_count = app.comment_text.chars().count();
    let counter = format!("{} chars ", char_count);
    let hint_w = (area.width as usize).saturating_sub(counter.chars().count());
    let mut footer = super::utils::footer_line(
        &[
            ("Ctrl+S", "Submit"),
            ("Ctrl+V", "Paste"),
            ("Enter", "New line"),
            (theme::icon_left_right(), "Move"),
            ("Esc", "Back (twice discards)"),
        ],
        hint_w,
    );
    footer.spans.push(Span::raw("  "));
    footer.spans.push(Span::styled(
        counter,
        if char_count > 0 {
            theme::normal()
        } else {
            theme::dim()
        },
    ));
    f.render_widget(Paragraph::new(footer), chunks[2]);
}
