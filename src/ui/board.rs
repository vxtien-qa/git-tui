use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::word_wrap;
use crate::app::App;
use crate::models::status::Status;

/// Adaptive column count: one column per ~40 cells of width, so wide
/// terminals show more of the 11-column pipeline (2..=6).
fn visible_cols(width: u16) -> usize {
    (width as usize / 40).clamp(2, 6)
}

/// Render the kanban board - 3 columns, full titles, text labels.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let columns = Status::all_columns();
    let vis = visible_cols(area.width);

    // Auto-correct: ensure selected column is always within visible range
    if app.board_selected_col >= app.board_col_offset + vis {
        app.board_col_offset = app.board_selected_col.saturating_sub(vis - 1);
    }
    if app.board_selected_col < app.board_col_offset {
        app.board_col_offset = app.board_selected_col;
    }
    // Clamp offset
    if app.board_col_offset + vis > columns.len() {
        app.board_col_offset = columns.len().saturating_sub(vis);
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),    // columns
            Constraint::Length(1), // footer
        ])
        .split(area);

    render_header(f, app, chunks[0]);
    render_columns(f, app, chunks[1]);
    render_footer(f, chunks[2]);
}

/// Number of item lines a card occupies (must mirror the render loop).
fn card_height(item: &crate::models::item::Item, card_w: usize) -> usize {
    // line 1 (meta) + wrapped title + detail line + separator
    1 + word_wrap(&item.title, card_w.saturating_sub(2)).len() + 1 + 1
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let columns = Status::all_columns();
    let visible_end = (app.board_col_offset + visible_cols(area.width)).min(columns.len());

    let header = Line::from(vec![
        Span::styled(" Board", theme::title()),
        Span::raw("  "),
        Span::styled(
            format!(
                "[s] {}",
                app.board_sprint_filter.as_deref().unwrap_or("All Sprints")
            ),
            theme::highlight(),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "Col {}-{} of {}",
                app.board_col_offset + 1,
                visible_end,
                columns.len()
            ),
            theme::dim(),
        ),
    ]);
    f.render_widget(Paragraph::new(header), area);
}

fn render_columns(f: &mut Frame, app: &mut App, area: Rect) {
    let columns = Status::all_columns();
    let visible_end = (app.board_col_offset + visible_cols(area.width)).min(columns.len());
    let visible_cols = &columns[app.board_col_offset..visible_end];
    let num_visible = visible_cols.len();

    if num_visible == 0 {
        return;
    }

    let constraints: Vec<Constraint> = (0..num_visible)
        .map(|_| Constraint::Ratio(1, num_visible as u32))
        .collect();

    let col_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Deferred: written into app after the loop (item borrows end there).
    let mut selected_visible_cards: Option<usize> = None;

    for (col_idx, status) in visible_cols.iter().enumerate() {
        let actual_col = app.board_col_offset + col_idx;
        let items = app.items_in_column(status);
        let count = items.len();
        let is_selected_col = actual_col == app.board_selected_col;

        let title = format!(" {} ({}) ", status.label(), count);
        let border_style = if is_selected_col {
            theme::title()
        } else {
            theme::border()
        };

        // Column titles carry the status color (e.g. Blocked = yellow) so the
        // pipeline reads at a glance; the selected column keeps the accent.
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                &title,
                if is_selected_col {
                    theme::title()
                } else {
                    theme::status_style(status)
                },
            ));

        let inner_h = col_areas[col_idx].height.saturating_sub(2) as usize;
        let card_w = col_areas[col_idx].width.saturating_sub(3) as usize;

        let mut card_lines: Vec<Line> = Vec::new();
        let scroll_offset = app.board_row_offset.get(actual_col).copied().unwrap_or(0);

        // Scroll indicator top
        if scroll_offset > 0 {
            card_lines.push(Line::from(Span::styled(
                format!(" -- {} more above --", scroll_offset),
                theme::dim(),
            )));
        }

        // Empty column state
        if items.is_empty() {
            card_lines.push(Line::from(""));
            card_lines.push(Line::from(Span::styled("  - empty -", theme::dim())));
        }

        // Count cards that fit COMPLETELY, so the scroll math and the
        // "more below" indicator agree with what's actually on screen.
        let mut cards_fully_rendered = 0usize;

        for (item_idx_global, (_item_idx, item)) in items.iter().enumerate() {
            if item_idx_global < scroll_offset {
                continue;
            }
            // Stop before starting a card that can't fit completely.
            if card_lines.len() + card_height(item, card_w) > inner_h.saturating_sub(1) {
                break;
            }

            let is_selected = is_selected_col && item_idx_global == app.board_selected_row;

            let prefix = if item.selected {
                theme::icon_checked()
            } else if is_selected {
                theme::icon_arrow()
            } else {
                " "
            };

            let card_style = if is_selected {
                theme::selected()
            } else {
                theme::normal()
            };

            // Text label: [Bug] / [Enh] / [Task]
            let kind = item.kind();
            let type_label = format!("[{}]", kind.short_label());
            let type_style = theme::item_kind_style(kind);

            // Number + Priority
            let num = item.number.map(|n| format!("#{}", n)).unwrap_or_default();
            let prio = item.priority.as_deref().unwrap_or("--");

            // Line 1: prefix + number + priority + type
            card_lines.push(Line::from(vec![
                Span::styled(format!("{} ", prefix), card_style),
                Span::styled(format!("{} ", num), theme::dim()),
                Span::styled(format!("{} ", prio), theme::priority_style(prio)),
                Span::styled(type_label.to_string(), type_style),
            ]));

            // Line 2+: Full title (word wrap)
            let title_lines = word_wrap(&item.title, card_w.saturating_sub(2));
            for tl in &title_lines {
                card_lines.push(Line::from(Span::styled(format!("  {}", tl), card_style)));
            }

            // Line 3: assignee + stack + sprint + PR
            {
                let assignee = item.assignees.first().map(|s| s.as_str()).unwrap_or("");
                let stack = item.stack.as_deref().unwrap_or("");
                let pr = if item.has_prs() {
                    format!(" {}", item.pr_display())
                } else {
                    String::new()
                };
                // UI-12: Show sprint when viewing "All Sprints"
                let sprint = if app.board_sprint_filter.is_none() {
                    item.sprint
                        .as_deref()
                        .map(|s| format!(" [{}]", s))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                card_lines.push(Line::from(Span::styled(
                    format!("  @{} {}{}{}", assignee, stack, sprint, pr),
                    theme::dim(),
                )));
            }

            // Separator
            card_lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    theme::icon_h_line().repeat(card_w.saturating_sub(3).min(30))
                ),
                theme::border(),
            )));

            cards_fully_rendered += 1;
        }

        // Remember how many cards the selected column can show - the input
        // handler uses this for height-aware row scrolling.
        if is_selected_col {
            selected_visible_cards = Some(cards_fully_rendered.max(1));
        }

        let remaining = count.saturating_sub(scroll_offset + cards_fully_rendered);
        if remaining > 0 {
            let msg = Line::from(Span::styled(
                format!(" -- {} more below --", remaining),
                theme::dim(),
            ));
            if card_lines.len() >= inner_h && inner_h > 0 {
                card_lines.truncate(inner_h - 1);
                card_lines.push(msg);
            } else {
                card_lines.push(msg);
            }
        }

        let paragraph = Paragraph::new(card_lines).block(block);
        f.render_widget(paragraph, col_areas[col_idx]);
    }

    if let Some(v) = selected_visible_cards {
        app.board_visible_cards = v;
    }
}

fn render_footer(f: &mut Frame, area: Rect) {
    // One prioritised list: the helper fits what it can at any width and
    // always keeps Esc, instead of guessing breakpoints per screen.
    let footer = super::utils::footer_line(
        &[
            (theme::icon_left_right(), "Column"),
            (theme::icon_up_down(), "Row"),
            ("Enter", "Open"),
            ("Space", "Select"),
            ("p", "QA Actions"),
            ("m", "Move"),
            ("s", "Sprint"),
            ("t", "Task"),
            ("b", "Bug"),
            ("e", "Enhancement"),
            ("f", "Fail"),
            ("r", "Refresh"),
            ("?", "Help"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), area);
}
