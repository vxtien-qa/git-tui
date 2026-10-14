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
    let intro = match kind {
        BatchKind::VerifyUat => {
            "Checked tickets move to Tech Complete, each gets a comment, and one Slack summary goes out."
        }
        BatchKind::MoveToUat => {
            "Check the tickets whose code is on UAT: each moves to In UAT, loses Ready-for-UAT and gets a comment; one Slack message lists them. UAT = the d deploy check, if run."
        }
    };

    let mut lines: Vec<Line> = Vec::new();
    for part in super::utils::word_wrap(intro, inner_w.saturating_sub(2)) {
        lines.push(Line::from(Span::styled(format!(" {}", part), theme::dim())));
    }
    lines.push(Line::from(""));
    let header_rows = lines.len();

    if app.batch_rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  - nothing to pick -",
            theme::dim(),
        )));
    }

    // Keep the cursor row in view: rows below the intro, inside the border.
    let view_h = (chunks[0].height.saturating_sub(2) as usize)
        .saturating_sub(header_rows)
        .max(1);
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
            " {} {} {:<7} {:<10} ",
            if is_cur { theme::icon_arrow() } else { " " },
            mark,
            num,
            trunc(sprint, 10)
        );
        // Move to In UAT: what the deploy check said about UAT, if it ran.
        let uat = if kind == BatchKind::MoveToUat {
            let mark = match item.and_then(|i| app.uat_deploy_state(i)) {
                Some(crate::gh::issue::DeployState::Contained) => theme::icon_ok().to_string(),
                Some(crate::gh::issue::DeployState::Missing) => theme::icon_fail().to_string(),
                Some(_) => "?".to_string(),
                None => "-".to_string(),
            };
            format!("UAT {:<2} ", mark)
        } else {
            String::new()
        };
        let fixed = format!("{}{}", fixed, uat);
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

    let footer = footer_line(
        &[
            ("Space", "Check"),
            ("a", "All/none"),
            ("Enter", "Confirm"),
            (theme::icon_up_down(), "Move"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
