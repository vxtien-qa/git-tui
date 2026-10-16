use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::{footer_line, trunc};
use crate::app::{App, BatchKind};

/// Render the batch pick list: one checkable row per ticket.
pub fn render(f: &mut Frame, app: &App, kind: BatchKind, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let checked = app.batch_rows.iter().filter(|(_, c)| *c).count();
    let inner_w = chunks[0].width.saturating_sub(2) as usize;
    let inner_h = chunks[0].height.saturating_sub(2) as usize;
    let intro = match kind {
        BatchKind::VerifyUat => {
            "Checked tickets move to Tech Complete, each gets a comment, and one Slack summary goes out. d deploy-checks every row."
        }
        BatchKind::MoveToUat => {
            "Check the tickets whose code is on UAT: each moves to In UAT, loses Ready-for-UAT and gets a comment; one Slack message lists them. d deploy-checks every row."
        }
    };
    let intro_lines = super::utils::word_wrap(intro, inner_w.saturating_sub(2));

    // What the deploy check (d) said about UAT, per row, once it ran.
    let uat_mark = |item: Option<&crate::models::item::Item>| -> String {
        let lookup_failed = item.is_some_and(|i| app.linked_prs_failed.contains_key(&i.id));
        // A result on screen wins over a later failed refresh of the PRs.
        match item.and_then(|i| app.uat_deploy_state(i)) {
            Some(state) => super::utils::deploy_mark(&state).0,
            None if lookup_failed => "!".to_string(),
            None if item.is_some_and(|i| app.deploy_check_inflight.contains(&i.id)) => {
                theme::icon_spinner().to_string()
            }
            None => " ".to_string(),
        }
    };
    let marks: Vec<String> = app
        .batch_rows
        .iter()
        .map(|(id, _)| uat_mark(app.items.iter().find(|i| &i.id == id)))
        .collect();

    // The marks mean nothing without a key: show one once any is on screen.
    let has_result = marks
        .iter()
        .any(|m| !m.trim().is_empty() && m != theme::icon_spinner());
    let legend: Vec<String> = if has_result {
        super::utils::word_wrap(
            &format!(
                "UAT: {} on UAT · {} not on UAT · ? not in its history (maybe cherry-picked) · - not found · ! check failed",
                theme::icon_ok(),
                theme::icon_fail()
            ),
            inner_w.saturating_sub(2),
        )
    } else {
        Vec::new()
    };

    // Rows come first: the intro, then the legend, give way on a short
    // terminal so at least a few tickets stay in view.
    let rows_wanted = app.batch_rows.len().clamp(1, 4);
    let show_legend = !legend.is_empty() && inner_h >= legend.len() + 1 + rows_wanted;
    let legend_h = if show_legend { legend.len() + 1 } else { 0 };
    let show_intro = inner_h >= intro_lines.len() + 1 + rows_wanted + legend_h;

    let mut lines: Vec<Line> = Vec::new();
    if show_intro {
        for part in &intro_lines {
            lines.push(Line::from(Span::styled(format!(" {}", part), theme::dim())));
        }
        lines.push(Line::from(""));
    }
    let header_rows = lines.len();

    if app.batch_rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  - nothing to pick -",
            theme::dim(),
        )));
    }

    // Keep the cursor row in view: rows below the intro, above the legend.
    let view_h = inner_h.saturating_sub(header_rows + legend_h).max(1);
    let skip = app.batch_cursor.saturating_sub(view_h.saturating_sub(1));

    for (pos, (id, is_checked)) in app.batch_rows.iter().enumerate().skip(skip).take(view_h) {
        let item = app.items.iter().find(|i| &i.id == id);
        let is_cur = pos == app.batch_cursor;
        let mark = if *is_checked {
            theme::icon_checked()
        } else {
            theme::icon_unchecked()
        };
        let num = item
            .and_then(|i| i.number)
            .map(|n| format!("#{}", n))
            .unwrap_or_else(|| "draft".to_string());
        let sprint = item.and_then(|i| i.sprint.as_deref()).unwrap_or("--");
        let title = item
            .map(|i| i.title.as_str())
            .unwrap_or("(no longer on the board)");
        let fixed = format!(
            " {} {} {:<7} {:<10} UAT {:<2} ",
            if is_cur { theme::icon_arrow() } else { " " },
            mark,
            num,
            trunc(sprint, 10),
            marks.get(pos).map(String::as_str).unwrap_or(" ")
        );
        let title_w = inner_w.saturating_sub(fixed.chars().count()).max(8);
        let style = if is_cur {
            theme::selected()
        } else if *is_checked {
            theme::normal()
        } else {
            theme::dim()
        };
        lines.push(Line::from(vec![
            Span::styled(fixed, style),
            Span::styled(trunc(title, title_w), style),
        ]));
    }

    if show_legend {
        // Pinned to the bottom of the panel, under however many rows fit.
        let used = lines.len();
        let pad = inner_h.saturating_sub(used + legend.len());
        lines.extend((0..pad).map(|_| Line::from("")));
        for part in &legend {
            lines.push(Line::from(Span::styled(format!(" {}", part), theme::dim())));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(
            format!(
                " {} - {}/{} checked ",
                kind.title(),
                checked,
                app.batch_rows.len()
            ),
            theme::title(),
        ));
    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Short labels so `d` survives an 80-column terminal; ↑↓ goes first.
    let footer = footer_line(
        &[
            ("Space", "Check"),
            ("Enter", "Confirm"),
            ("d", "Deploy"),
            ("a", "All/none"),
            (theme::icon_up_down(), "Move"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
