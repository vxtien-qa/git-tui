//! Shared form widgets for the Bug Report and Task forms (previously
//! copy-pasted between the two files).

use ratatui::text::{Line, Span};

use super::theme;
use crate::handlers::text_edit;

/// Render one multiline text field: `label [value]`, word-wrapped, with the
/// cursor glyph rendered at `cursor` (byte offset) when focused.
///
/// Returns the rendered-line offset of the cursor within this field (0 when
/// not focused) so callers can anchor their scroll on the cursor, not just
/// on the top of the field.
pub fn render_text_field(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    focused: bool,
    cursor: usize,
    max_text_width: usize,
) -> usize {
    let style = if focused {
        theme::selected()
    } else {
        theme::normal()
    };
    let prefix = if focused { theme::icon_arrow() } else { " " };
    let label_str = format!(" {} {:<10} [", prefix, label);
    let pad_str = format!("   {:<10}  ", "");

    // The cursor is part of the display text, so it wraps naturally and stays
    // visible even mid-paragraph.
    let display_value = if focused {
        text_edit::with_cursor_glyph(value, cursor, theme::icon_cursor())
    } else {
        value.to_string()
    };

    let mut rendered_lines: Vec<String> = Vec::new();
    if display_value.is_empty() && !focused {
        rendered_lines.push(String::new());
    } else {
        for raw_line in display_value.split('\n') {
            let wrapped = crate::ui::utils::word_wrap(raw_line, max_text_width);
            rendered_lines.extend(wrapped);
        }
    }
    if rendered_lines.is_empty() {
        rendered_lines.push(String::new());
    }

    // The glyph is inserted exactly once, so the first line containing it is
    // the cursor's rendered line.
    let cursor_line = if focused {
        let glyph = theme::icon_cursor();
        rendered_lines
            .iter()
            .position(|l| l.contains(glyph))
            .unwrap_or(0)
    } else {
        0
    };

    let total = rendered_lines.len();
    for (i, line_str) in rendered_lines.into_iter().enumerate() {
        if i == 0 {
            if total == 1 {
                lines.push(Line::from(vec![
                    Span::styled(label_str.clone(), theme::dim()),
                    Span::styled(format!("{}]", line_str), style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(label_str.clone(), theme::dim()),
                    Span::styled(line_str, style),
                ]));
            }
        } else if i == total - 1 {
            lines.push(Line::from(vec![
                Span::styled(pad_str.clone(), theme::dim()),
                Span::styled(format!("{}]", line_str), style),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(pad_str.clone(), theme::dim()),
                Span::styled(line_str, style),
            ]));
        }
    }

    cursor_line
}

/// Render a radio/selector line.
/// Render one radio row, wrapping the options across lines when they do not
/// fit `max_w`.
///
/// The row used to be one unbounded string, so a long option set (the
/// workaround question) was simply cut off by the panel edge with no ".." to
/// show that an option was hidden.
pub fn render_radio_line(
    lines: &mut Vec<Line<'_>>,
    label: &str,
    options: &[&str],
    selected: usize,
    focused: bool,
    max_w: usize,
) {
    let prefix = if focused { theme::icon_arrow() } else { " " };
    let label_str = format!(" {} {:<12}", prefix, label);
    let indent = " ".repeat(label_str.chars().count());
    let style = if focused {
        theme::selected()
    } else {
        theme::normal()
    };

    let chips: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            if i == selected {
                format!("({}){}", theme::icon_bullet(), opt)
            } else {
                format!("( ){}", opt)
            }
        })
        .collect();

    let width = crate::ui::utils::display_width;
    let avail = max_w.saturating_sub(width(&label_str)).max(12);

    let mut row = String::new();
    let mut first = true;
    let flush = |row: &mut String, first: &mut bool, lines: &mut Vec<Line<'_>>| {
        if row.is_empty() {
            return;
        }
        let head = if *first {
            label_str.clone()
        } else {
            indent.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(head, theme::dim()),
            Span::styled(std::mem::take(row), style),
        ]));
        *first = false;
    };

    for chip in chips {
        let sep = if row.is_empty() { 0 } else { 2 };
        if !row.is_empty() && width(&row) + sep + width(&chip) > avail {
            flush(&mut row, &mut first, lines);
        }
        if !row.is_empty() {
            row.push_str("  ");
        }
        row.push_str(&chip);
    }
    flush(&mut row, &mut first, lines);
}
