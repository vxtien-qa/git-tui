use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::trunc;
use crate::app::App;
use crate::models::status::Status;

/// Render My Tasks - sorted by priority, full titles, text labels.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let w = area.width as usize;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(area);

    // Tell the input handler how tall the list viewport really is.
    app.list_view_height = chunks[1].height.saturating_sub(2) as usize;

    // Header
    let displayed_items = app.my_tasks_grouped();
    let displayed_total = displayed_items.len();
    let in_qa = displayed_items
        .iter()
        .filter(|(_, i)| matches!(i.status, Status::InQA | Status::InQADev | Status::InUAT))
        .count();
    let sprint_label = app.board_sprint_filter.as_deref().unwrap_or("All Sprints");
    let header_w = (w).saturating_sub(13); // leave room for the title
                                           // Least important fact drops first, so what remains stays readable.
    let header_info = super::utils::fit_parts(
        &[
            format!("[s] {}", sprint_label),
            format!("{} total", displayed_total),
            format!("{} in QA", in_qa),
            format!("@{}", app.current_user),
        ],
        "  |  ",
        header_w,
    );
    let header_info_trunc = header_info;
    let header = Line::from(vec![
        Span::styled(" My Tasks", theme::title()),
        Span::styled(format!("   {}", header_info_trunc), theme::dim()),
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    let col_w = w.saturating_sub(4); // inside border
    let title_w = col_w.saturating_sub(26); // prefix+prio+type+num

    let mut lines: Vec<Line> = Vec::new();
    let mut global_idx = 0;

    let groups = app.my_tasks_by_status();

    // UI-10: Empty state
    if displayed_total == 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No tasks assigned to you in this sprint.",
            theme::dim(),
        )));
        lines.push(Line::from(Span::styled(
            "  Press [s] to switch sprint, or [Esc] to go back.",
            theme::dim(),
        )));
    }

    for (status, items_in_status) in &groups {
        // Group header: a titled rule, matching the Dashboard sections
        // (the old " --Label--" read like debug output).
        let label = format!("{} ({})", status.label(), items_in_status.len());
        let sep_w = col_w.saturating_sub(label.chars().count() + 5);
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", theme::icon_h_line()), theme::border()),
            Span::styled(label, theme::title()),
            Span::styled(
                format!(" {}", theme::icon_h_line().repeat(sep_w.min(60))),
                theme::border(),
            ),
        ]));

        for (_, item) in items_in_status {
            let is_selected = global_idx == app.list_selected;
            let style = if is_selected {
                theme::selected()
            } else {
                theme::normal()
            };
            // Themed pointer like every other list (a bare ">" also skipped
            // the non-Unicode fallback).
            let prefix = if is_selected {
                theme::icon_arrow()
            } else {
                " "
            };

            let prio = item.priority.as_deref().unwrap_or("--");
            let num = item.number.map(|n| format!("#{}", n)).unwrap_or_default();

            // Fixed-width type column; Task carries no tag to keep it quiet.
            let kind = item.kind();
            let type_label = match kind {
                crate::models::item::ItemKind::Task => " ".repeat(6),
                other => format!("{:<6}", format!("[{}]", other.short_label())),
            };

            // Full title
            let title = trunc(&item.title, title_w);

            // Main line
            lines.push(Line::from(vec![
                Span::styled(format!(" {} ", prefix), style),
                Span::styled(format!("{:<3} ", prio), theme::priority_style(prio)),
                Span::styled(type_label.to_string(), theme::item_kind_style(kind)),
                Span::styled(format!("{:<6} ", num), theme::dim()),
                Span::styled(title, style),
            ]));

            // Detail line: labels, stack, PR, sprint
            let mut parts: Vec<String> = Vec::new();

            // Show all labels prominently
            for label in &item.labels {
                parts.push(format!("[{}]", label));
            }

            if let Some(stack) = item.stack.as_deref() {
                if !stack.is_empty() {
                    parts.push(format!("({})", stack));
                }
            }
            if item.has_prs() {
                parts.push(item.pr_display());
            }
            if let Some(ref sprint) = item.sprint {
                parts.push(sprint.clone());
            }

            if !parts.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("              "),
                    Span::styled(parts.join("  "), theme::highlight()),
                ]));
            }

            global_idx += 1;
        }

        lines.push(Line::from("")); // spacer
    }

    // "Waiting for deploy" - passed tickets QA would otherwise lose track of
    // (labelled Ready-for-Staging/UAT, still sitting in the passed-from column).
    let waiting = app.waiting_for_deploy();
    if !waiting.is_empty() {
        let label = format!("Waiting for deploy ({})", waiting.len());
        let sep_w = col_w.saturating_sub(label.chars().count() + 5);
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", theme::icon_h_line()), theme::border()),
            Span::styled(label, theme::dim()),
            Span::styled(
                format!(" {}", theme::icon_h_line().repeat(sep_w.min(60))),
                theme::border(),
            ),
        ]));
        for (_, item) in &waiting {
            let num = item.number.map(|n| format!("#{}", n)).unwrap_or_default();
            let target = if item.status == Status::InQADev {
                "STG deploy pending"
            } else {
                "UAT deploy pending"
            };
            lines.push(Line::from(vec![
                Span::styled(format!("   {:<6} ", num), theme::dim()),
                Span::styled(trunc(&item.title, title_w), theme::dim()),
                Span::styled(
                    format!("  {} {}", theme::icon_right_arrow(), target),
                    theme::dim(),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    let visible: Vec<Line> = lines.into_iter().skip(app.list_scroll).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(
            super::utils::position_title(app.list_selected, displayed_total),
            theme::dim(),
        ));
    f.render_widget(Paragraph::new(visible).block(block), chunks[1]);

    // Footer: fits any width, and never drops the way back.
    let footer = super::utils::footer_line(
        &[
            (theme::icon_up_down(), "Nav"),
            ("Enter", "Detail"),
            ("p", "QA Actions"),
            ("f", "Fail"),
            ("m", "Move"),
            ("s", "Sprint"),
            ("r", "Refresh"),
            ("?", "Help"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[2]);
}
