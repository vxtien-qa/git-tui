use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;

use super::theme;
use super::utils::trunc;
use crate::app::App;
use crate::models::status::Status;

/// Render Search & Filter screen - full-width, wrapped options.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let w = area.width as usize;

    // Build filter lines first to know exact height needed
    let filter_lines = build_filter_lines(app, w);
    let filter_height = (filter_lines.len() + 2).min(area.height as usize / 2) as u16; // +2 for border

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(filter_height),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    // ── Filter Panel ──
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(
            // Name the field Tab has landed on: with five collapsed filter
            // rows the pointer alone is easy to lose.
            format!(" Search & Filter  [Tab] {} ", focused_field_name(app)),
            theme::title(),
        ));
    f.render_widget(Paragraph::new(filter_lines).block(filter_block), chunks[0]);

    // ── Results ──
    let mut result_rows: Vec<Row> = Vec::new();

    if app.search_results.is_empty() && !app.search_query.is_empty() {
        result_rows.push(Row::new(vec![Cell::from(Span::styled(
            "  No results found. Press Esc to clear filters.",
            theme::dim(),
        ))]));
    }

    let col_w = w.saturating_sub(4); // inside border
    for (display_idx, &item_idx) in app.search_results.iter().enumerate().skip(app.list_scroll) {
        if let Some(item) = app.items.get(item_idx) {
            let is_sel = display_idx == app.list_selected;
            let style = if is_sel {
                theme::selected()
            } else {
                theme::normal()
            };

            let prefix = if is_sel {
                format!("{} ", theme::icon_arrow())
            } else {
                "  ".to_string()
            };

            let kind = item.kind();
            let type_label = Span::styled(
                format!("{}{}", prefix, kind.short_label()),
                if is_sel {
                    style
                } else {
                    theme::item_kind_style(kind)
                },
            );

            let num = format!("#{}", item.number.unwrap_or(0));
            let prio = item.priority.as_deref().unwrap_or("--");
            let status = item.status.short_label();
            let stack = item.stack.as_deref().unwrap_or("");

            // Limit title to what the Title column actually gets: the five
            // fixed columns plus one space of table spacing between each.
            // The old estimate was 9 columns short, so long titles were cut
            // off by the table with no ".." to show it had happened.
            const FIXED_COLS: usize = 8 + 7 + 10 + 4 + 6;
            const COL_SPACING: usize = 5;
            let title_w = col_w.saturating_sub(FIXED_COLS + COL_SPACING).max(10);
            let title = trunc(&item.title, title_w);

            result_rows.push(
                Row::new(vec![
                    Cell::from(type_label),
                    Cell::from(Span::styled(num, theme::dim())),
                    Cell::from(Span::styled(status, theme::dim())),
                    Cell::from(Span::styled(prio, theme::priority_style(prio))),
                    Cell::from(Span::styled(stack.to_string(), theme::dim())),
                    Cell::from(Span::styled(title, style)),
                ])
                .style(style),
            );
        }
    }

    let result_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(
            // Position, not just the count: on a scrolled list the highlight
            // alone does not say where in the results you are.
            format!(
                " Results{}",
                super::utils::position_title(app.list_selected, app.search_results.len())
            ),
            theme::dim(),
        ));

    if app.search_results.is_empty() {
        // Render simple paragraph for empty state to avoid table column issues
        let msg = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No results found. Press Esc to clear filters.",
                theme::dim(),
            )),
        ];
        f.render_widget(Paragraph::new(msg).block(result_block), chunks[1]);
    } else {
        let widths = [
            Constraint::Length(8),  // Type (Bug/Task + prefix)
            Constraint::Length(7),  // Number
            Constraint::Length(10), // Status
            Constraint::Length(4),  // Priority
            Constraint::Length(6),  // Stack
            Constraint::Min(20),    // Title
        ];
        let table = Table::new(result_rows, widths).block(result_block);
        f.render_widget(table, chunks[1]);
    }

    // ── Footer ──
    let footer = super::utils::footer_line(
        &[
            ("Tab", "Field"),
            (theme::icon_left_right(), "Option"),
            ("Space", "Toggle"),
            ("Enter", "Search/Open"),
            ("Ctrl+L", "Clear filters"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

/// Name of the field Tab focus currently sits on.
fn focused_field_name(app: &App) -> &'static str {
    match app.search_field_idx {
        0 => "Keyword",
        1 => "Status",
        2 => "Priority",
        3 => "Stack",
        4 => "Sprint",
        _ => "Label",
    }
}

/// Build all filter lines, wrapping options to fit within `max_w`.
fn build_filter_lines(app: &App, max_w: usize) -> Vec<Line<'static>> {
    let inner_w = max_w.saturating_sub(4); // inside border + padding
    let mut lines: Vec<Line> = Vec::new();

    // ── Keyword ──
    let kw_focused = app.search_field_idx == 0;
    let icon = if kw_focused { theme::icon_arrow() } else { " " };
    let cursor = if kw_focused { theme::icon_cursor() } else { "" };
    let hint = if kw_focused && app.search_query.is_empty() {
        "  type keyword + Enter"
    } else {
        ""
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {} Keyword: ", icon),
            if kw_focused {
                theme::highlight()
            } else {
                theme::dim()
            },
        ),
        Span::styled(
            format!("{}{}", app.search_query, cursor),
            if kw_focused {
                theme::selected()
            } else {
                theme::normal()
            },
        ),
        Span::styled(hint.to_string(), theme::dim()),
    ]));
    lines.push(Line::from(""));

    // ── Filter fields ──
    let filter_defs: Vec<(&str, &Vec<String>, usize, Vec<String>)> = vec![
        (
            "Status  ",
            &app.filter_status,
            0,
            Status::all_columns()
                .iter()
                .map(|s| s.label().to_string())
                .collect(),
        ),
        (
            "Priority",
            &app.filter_priority,
            1,
            app.available_priorities(),
        ),
        ("Stack   ", &app.filter_stack, 2, app.available_stacks()),
        (
            "Sprint  ",
            &app.filter_sprint,
            3,
            app.sprint_filter_options().into_iter().flatten().collect(),
        ),
        ("Label   ", &app.filter_label, 4, app.available_labels()),
    ];

    for (i, (label, selected, cursor_field, options)) in filter_defs.iter().enumerate() {
        let field_idx = i + 1;
        let is_focused = field_idx == app.search_field_idx;
        let icon = if is_focused { theme::icon_arrow() } else { " " };

        // Summary of active filters (when not focused)
        if !is_focused {
            if selected.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} {} ", icon, label), theme::dim()),
                    Span::styled("All", theme::dim()),
                ]));
            } else {
                let tags: Vec<String> = selected
                    .iter()
                    .map(|s| {
                        if let Some(rest) = s.strip_prefix('!') {
                            format!("NOT {}", rest)
                        } else {
                            s.clone()
                        }
                    })
                    .collect();
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} {} ", icon, label), theme::dim()),
                    Span::styled(tags.join(" · "), theme::highlight()),
                ]));
            }
            continue;
        }

        // Focused: show label
        lines.push(Line::from(vec![
            Span::styled(format!(" {} {} ", icon, label), theme::highlight()),
            Span::styled(
                format!("({} move, Space toggle)", theme::icon_left_right()),
                theme::dim(),
            ),
        ]));

        // Wrap options into lines that fit terminal width
        let cursor_pos = app.filter_cursor[*cursor_field];
        let pad_left = "    "; // 4 spaces indent
        let pad_left_w = pad_left.len();

        let mut current_spans: Vec<Span> = vec![Span::raw(pad_left.to_string())];
        let mut current_w = pad_left_w;

        for (opt_idx, opt) in options.iter().enumerate() {
            let is_include = selected.contains(opt);
            let not_val = format!("!{}", opt);
            let is_exclude = selected.contains(&not_val);
            let is_cur = cursor_pos == opt_idx;

            let chip = if is_include {
                format!(" {} {} ", theme::icon_bullet(), opt)
            } else if is_exclude {
                format!(" {} {} ", theme::icon_fail(), opt)
            } else {
                format!(" {} {} ", theme::icon_bullet_empty(), opt)
            };
            let chip_w = chip.chars().count();

            // Wrap to next line if needed
            if current_w + chip_w > inner_w && current_w > pad_left_w {
                lines.push(Line::from(current_spans));
                current_spans = vec![Span::raw(pad_left.to_string())];
                current_w = pad_left_w;
            }

            let style = if is_cur {
                theme::selected()
            } else if is_exclude {
                theme::warning()
            } else if is_include {
                theme::highlight()
            } else {
                theme::normal()
            };

            current_spans.push(Span::styled(chip, style));
            current_w += chip_w;
        }

        if current_w > pad_left_w {
            lines.push(Line::from(current_spans));
        }

        lines.push(Line::from("")); // spacer after focused field
    }

    lines
}
