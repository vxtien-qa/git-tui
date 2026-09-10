use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils;
use crate::app::App;
use crate::models::status::Status;

/// Render Dashboard & Stats screen - responsive: one scrollable column, or
/// two columns side-by-side on wide terminals (overview | bugs & stats).
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let w = area.width as usize;
    let inner_w = w.saturating_sub(4); // border + padding
    let wide = inner_w >= 110;
    // Width each section actually has available for bars/separators.
    let col_w = if wide { inner_w / 2 - 2 } else { inner_w };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(1),    // content
            Constraint::Length(1), // footer
        ])
        .split(area);

    // ── Header ──
    let sprint_label = app.board_sprint_filter.as_deref().unwrap_or("All Sprints");
    let header = Line::from(vec![
        Span::styled(" QA Dashboard", theme::title()),
        Span::raw("   "),
        Span::styled(format!("[s] {}", sprint_label), theme::highlight()),
        Span::raw("   "),
        Span::styled(format!("@{}", app.current_user), theme::dim()),
    ]);
    f.render_widget(Paragraph::new(header), chunks[0]);

    // ── Build all content lines ──
    let items: Vec<_> = app
        .items
        .iter()
        .filter(|item| {
            if let Some(ref sprint) = app.board_sprint_filter {
                item.sprint.as_deref() == Some(sprint.as_str())
            } else {
                true
            }
        })
        .collect();

    let total = items.len();
    let done = items.iter().filter(|i| i.status == Status::Done).count();
    let in_qa = items.iter().filter(|i| i.status == Status::InQA).count();
    let in_qa_dev = items.iter().filter(|i| i.status == Status::InQADev).count();
    let in_uat = items.iter().filter(|i| i.status == Status::InUAT).count();
    let in_progress = items
        .iter()
        .filter(|i| i.status == Status::InProgress)
        .count();
    let blocked = items.iter().filter(|i| i.status == Status::Blocked).count();
    let ready_dev = items
        .iter()
        .filter(|i| i.status == Status::ReadyForDev)
        .count();
    let in_review = items
        .iter()
        .filter(|i| i.status == Status::InReview)
        .count();
    let tech_complete = items
        .iter()
        .filter(|i| i.status == Status::TechComplete)
        .count();
    let ready_release = items
        .iter()
        .filter(|i| i.status == Status::ReadyForRelease)
        .count();
    let done_pct = utils::percent(done, total);

    // Section rules span the panel: capping them at 55 columns left a ragged
    // half-width line on any wide terminal.
    let sep_w = col_w.saturating_sub(4).min(120);
    let mut lines: Vec<Line> = Vec::new();

    // ════════════════════════════════════════════════════════
    // SECTION 1: Sprint Overview
    // ════════════════════════════════════════════════════════
    lines.push(section_header("Sprint Overview", sep_w));
    lines.push(Line::from(""));

    // Compact stats row - responsive: stack vertically if narrow
    if col_w >= 55 {
        lines.push(Line::from(vec![
            Span::styled(format!("  Total: {}", total), theme::normal()),
            Span::raw("   "),
            Span::styled(format!("Done: {} ({}%)", done, done_pct), theme::success()),
            Span::raw("   "),
            Span::styled(
                format!("QA: {}", in_qa + in_qa_dev + in_uat),
                theme::highlight(),
            ),
            Span::raw("   "),
            Span::styled(format!("Progress: {}", in_progress), theme::normal()),
            Span::raw("   "),
            Span::styled(
                format!("Blocked: {}", blocked),
                if blocked > 0 {
                    theme::warning()
                } else {
                    theme::dim()
                },
            ),
        ]));
    } else {
        // Narrow: two lines
        lines.push(Line::from(vec![
            Span::styled(format!("  Total: {}", total), theme::normal()),
            Span::raw("  "),
            Span::styled(format!("Done: {} ({}%)", done, done_pct), theme::success()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  QA: {}", in_qa + in_qa_dev + in_uat),
                theme::highlight(),
            ),
            Span::raw("  "),
            Span::styled(format!("Progress: {}", in_progress), theme::normal()),
            Span::raw("  "),
            Span::styled(
                format!("Blocked: {}", blocked),
                if blocked > 0 {
                    theme::warning()
                } else {
                    theme::dim()
                },
            ),
        ]));
    }

    // Progress bar
    lines.push(Line::from(utils::progress_bar_spans(
        40usize.min(col_w.saturating_sub(10)),
        total,
        &[
            (done, theme::success()),
            (in_qa + in_qa_dev + in_uat, theme::highlight()),
            (in_progress, theme::normal()),
        ],
    )));
    lines.push(Line::from(""));
    lines.push(utils::progress_bar_legend());
    lines.push(Line::from(""));

    // ════════════════════════════════════════════════════════
    // SECTION 2: Status Breakdown - horizontal mini-bars
    // ════════════════════════════════════════════════════════
    lines.push(section_header("Status Breakdown", sep_w));

    let status_data = vec![
        (
            "Backlog",
            items.iter().filter(|i| i.status == Status::Backlog).count(),
        ),
        ("Ready for Dev", ready_dev),
        ("In Progress", in_progress),
        ("In Review", in_review),
        ("In QA - Dev", in_qa_dev),
        ("In QA", in_qa),
        ("In UAT", in_uat),
        ("Tech Complete", tech_complete),
        ("Ready for Release", ready_release),
        ("Done", done),
        ("Blocked", blocked),
    ];

    let label_w = if col_w >= 50 { 18 } else { 12 };
    // Bars grow with the available width instead of a hard 25-cell cap.
    let max_bar = if col_w >= 50 {
        (col_w.saturating_sub(label_w + 10)).clamp(20, 45)
    } else {
        12
    };
    for (label, count) in &status_data {
        if *count == 0 {
            continue;
        }
        let b_len = if total > 0 {
            (count * max_bar / total.max(1)).max(1)
        } else {
            0
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:>width$} ", label, width = label_w),
                theme::dim(),
            ),
            Span::styled(theme::icon_bar_full().repeat(b_len), theme::highlight()),
            Span::styled(theme::icon_bar_empty(), theme::dim()),
            Span::styled(format!(" {}", count), theme::normal()),
        ]));
    }
    lines.push(Line::from(""));

    // ════════════════════════════════════════════════════════
    // SECTION 3: Bug Tracker
    // ════════════════════════════════════════════════════════
    // In wide mode everything from here renders in the right-hand column.
    let split_at = lines.len();
    let bugs: Vec<_> = items
        .iter()
        .filter(|i| i.labels.iter().any(|l| l.to_lowercase().contains("bug")))
        .collect();
    let bug_open = bugs
        .iter()
        .filter(|i| {
            !matches!(
                i.status,
                Status::Done | Status::TechComplete | Status::ReadyForRelease
            )
        })
        .count();
    let bug_done = bugs
        .iter()
        .filter(|i| {
            matches!(
                i.status,
                Status::Done | Status::TechComplete | Status::ReadyForRelease
            )
        })
        .count();

    lines.push(section_header("Bug Tracker", sep_w));
    lines.push(Line::from(vec![
        Span::styled(format!("  Total: {}", bugs.len()), theme::normal()),
        Span::raw("   "),
        Span::styled(
            format!("Open: {}", bug_open),
            if bug_open > 0 {
                theme::warning()
            } else {
                theme::dim()
            },
        ),
        Span::raw("   "),
        Span::styled(format!("Fixed: {}", bug_done), theme::success()),
    ]));

    // Bug by priority
    let prios = ["P0", "P1", "P2", "P3", "P4"];
    let mut prio_spans: Vec<Span> = vec![Span::raw("  ")];
    for p in &prios {
        let cnt = bugs
            .iter()
            .filter(|i| i.priority.as_deref() == Some(p))
            .count();
        if cnt > 0 {
            prio_spans.push(Span::styled(
                format!("{}:{} ", p, cnt),
                theme::priority_style(p),
            ));
        }
    }
    if prio_spans.len() > 1 {
        lines.push(Line::from(prio_spans));
    }
    lines.push(Line::from(""));

    // ════════════════════════════════════════════════════════
    // SECTION 4: My Stats
    // ════════════════════════════════════════════════════════
    let my_items: Vec<_> = items
        .iter()
        .filter(|i| i.assignees.iter().any(|a| a == &app.current_user))
        .collect();
    let my_total = my_items.len();
    let my_in_qa = my_items
        .iter()
        .filter(|i| matches!(i.status, Status::InQA | Status::InQADev | Status::InUAT))
        .count();
    let my_done = my_items
        .iter()
        .filter(|i| i.status == Status::Done || i.status == Status::TechComplete)
        .count();
    let my_bugs = my_items
        .iter()
        .filter(|i| i.labels.iter().any(|l| l.to_lowercase().contains("bug")))
        .count();
    let my_blocked = my_items
        .iter()
        .filter(|i| i.status == Status::Blocked)
        .count();

    lines.push(section_header(
        &format!("My Stats (@{})", app.current_user),
        sep_w,
    ));

    if col_w >= 50 {
        lines.push(Line::from(vec![
            Span::styled(format!("  Assigned: {}", my_total), theme::normal()),
            Span::raw("   "),
            Span::styled(format!("In QA: {}", my_in_qa), theme::highlight()),
            Span::raw("   "),
            Span::styled(format!("Done: {}", my_done), theme::success()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  Bugs: {}", my_bugs),
                if my_bugs > 0 {
                    theme::warning()
                } else {
                    theme::dim()
                },
            ),
            Span::raw("   "),
            Span::styled(
                format!("Blocked: {}", my_blocked),
                if my_blocked > 0 {
                    theme::warning()
                } else {
                    theme::dim()
                },
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            format!("  Assigned: {}  In QA: {}", my_total, my_in_qa),
            theme::normal(),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "  Done: {}  Bugs: {}  Blocked: {}",
                my_done, my_bugs, my_blocked
            ),
            theme::dim(),
        )));
    }
    lines.push(Line::from(""));

    // ════════════════════════════════════════════════════════
    // SECTION 5: By Stack
    // ════════════════════════════════════════════════════════
    let stacks = app.available_stacks();
    if !stacks.is_empty() {
        lines.push(section_header("By Stack", sep_w));
        for stack in &stacks {
            let s_items: Vec<_> = items
                .iter()
                .filter(|i| i.stack.as_deref() == Some(stack.as_str()))
                .collect();
            let s_total = s_items.len();
            let s_done = s_items
                .iter()
                .filter(|i| {
                    i.status == Status::Done
                        || i.status == Status::TechComplete
                        || i.status == Status::ReadyForRelease
                })
                .count();
            let s_qa = s_items
                .iter()
                .filter(|i| matches!(i.status, Status::InQA | Status::InQADev | Status::InUAT))
                .count();

            let pct = utils::percent(s_done, s_total);
            let bar_max = 16usize.min(col_w.saturating_sub(30));
            let bar_len = utils::scale_to(s_done, s_total, bar_max);
            lines.push(Line::from(vec![
                Span::styled(format!("  {:>10} ", stack), theme::dim()),
                Span::styled(theme::icon_bar_full().repeat(bar_len), theme::success()),
                Span::styled(
                    theme::icon_bar_empty().repeat(bar_max.saturating_sub(bar_len)),
                    theme::dim(),
                ),
                Span::styled(
                    format!(" {}/{} ({}%)", s_done, s_total, pct),
                    theme::normal(),
                ),
                Span::raw("  "),
                Span::styled(format!("QA:{}", s_qa), theme::highlight()),
            ]));
        }
        lines.push(Line::from(""));
    }

    // (status_message is shown by the global status bar in main::draw)
    lines.push(Line::from(""));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(
            format!(" Dashboard - {} ", sprint_label),
            theme::title(),
        ));

    if wide {
        // Two columns: overview + breakdown | bugs + my stats + by stack.
        let right_lines = lines.split_off(split_at);
        let max_scroll = lines.len().max(right_lines.len()).saturating_sub(1);
        let scroll = app.dashboard_scroll.min(max_scroll);

        let content = block.inner(chunks[1]);
        f.render_widget(block, chunks[1]);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(content);

        let left: Vec<Line> = lines.into_iter().skip(scroll).collect();
        let right: Vec<Line> = right_lines.into_iter().skip(scroll).collect();
        f.render_widget(Paragraph::new(left), cols[0]);
        f.render_widget(Paragraph::new(right), cols[1]);
    } else {
        let max_scroll = lines.len().saturating_sub(1);
        let scroll = app.dashboard_scroll.min(max_scroll);
        let visible: Vec<Line> = lines.into_iter().skip(scroll).collect();
        f.render_widget(Paragraph::new(visible).block(block), chunks[1]);
    }

    // ── Footer ──
    let mut hints: Vec<(&str, &str)> = vec![
        (theme::icon_up_down(), "Scroll"),
        ("s", "Sprint"),
        ("d/D", "Standup MD/Txt"),
        ("e/E", "Sprint MD/Txt"),
    ];
    if app.config.is_slack_configured() {
        hints.push(("S", "Slack standup"));
        hints.push(("X", "Slack sprint"));
    }
    hints.push(("Esc", "Back"));
    let footer = utils::footer_line(&hints, area.width as usize);
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

fn section_header<'a>(title: &str, width: usize) -> Line<'a> {
    let sep = theme::icon_h_line().repeat(width.saturating_sub(title.len() + 6));
    Line::from(Span::styled(
        format!("  ── {} {}", title, sep),
        theme::title(),
    ))
}
