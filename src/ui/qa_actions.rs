use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::centered_rect;
use crate::app::App;
use crate::models::status::Status;

/// One row of the QA action menu.
struct Action {
    key: &'static str,
    name: &'static str,
    /// What the action does, phrased for the item's current column.
    effect: String,
    /// Whether the action applies to the item as it stands.
    available: bool,
    /// Why it does not apply (shown in place of the effect when blocked).
    blocked_reason: Option<String>,
}

/// Build the action list for one item, marking what actually applies.
///
/// The menu used to advertise every action unconditionally and only explain
/// itself after the keypress, through an error popup. Showing availability up
/// front makes the current column and the handoff labels visible where the
/// decision is made.
fn actions_for(app: &App, item_idx: usize) -> Vec<Action> {
    let item = app.items.get(item_idx);
    let status = item.map(|i| i.status.clone()).unwrap_or_default();
    let has_staging = app.item_has_label(item_idx, "Ready-for-Staging");
    let has_uat = app.item_has_label(item_idx, "Ready-for-UAT");
    let in_qa_column = matches!(status, Status::InQADev | Status::InQA | Status::InUAT);
    // Bulk verify picks from the In UAT column, so say how big it is. The
    // ticket in hand is offered even when it sits outside the sprint the
    // board shows: then it is named apart instead of counted into the sprint.
    let uat_batch = app.verify_uat_candidates(item_idx).len();
    let uat_sprint = app.board_sprint_filter.as_deref().unwrap_or("all sprints");
    let in_sprint = app
        .items_in_column(&Status::InUAT)
        .iter()
        .any(|(i, _)| *i == item_idx);
    let verify_effect = if in_sprint {
        format!(
            "pick from the {} In UAT ticket(s) in {} and pass them together",
            uat_batch, uat_sprint
        )
    } else {
        format!(
            "pick from this ticket and the {} In UAT ticket(s) in {}, pass them together",
            uat_batch.saturating_sub(1),
            uat_sprint
        )
    };

    let wrong_column = |expected: &str| Some(format!("needs {}", expected));

    vec![
        Action {
            key: "p",
            name: "Pass Dev",
            effect: "adds Ready-for-Staging + tags the lead".to_string(),
            available: status == Status::InQADev && !has_staging,
            blocked_reason: if status != Status::InQADev {
                wrong_column("In QA - Dev")
            } else {
                Some("already passed on Dev".to_string())
            },
        },
        Action {
            key: "P",
            name: "Pass STG",
            effect: "adds Ready-for-UAT + tags the lead, no move".to_string(),
            available: status == Status::InQA && !has_uat,
            blocked_reason: if status != Status::InQA {
                wrong_column("In QA")
            } else {
                Some("already passed on STG".to_string())
            },
        },
        Action {
            key: "U",
            name: "Pass UAT",
            effect: "this ticket to Tech Complete + comment, one Slack message".to_string(),
            available: status == Status::InUAT,
            blocked_reason: wrong_column("In UAT"),
        },
        Action {
            key: "V",
            name: "Bulk verify UAT",
            effect: verify_effect,
            available: status == Status::InUAT,
            blocked_reason: wrong_column("In UAT"),
        },
        Action {
            key: "I",
            name: "Move to In UAT",
            effect: "code is on UAT: pick In QA + Ready-for-UAT tickets to move into In UAT"
                .to_string(),
            available: status == Status::InQA && has_uat,
            blocked_reason: if status != Status::InQA {
                wrong_column("In QA")
            } else {
                Some("needs the Ready-for-UAT label (pass on STG first)".to_string())
            },
        },
        Action {
            key: "f",
            name: "Fail (new bug)",
            // In UAT the run is a regression pass, so a finding is often in a
            // different area: demoting THIS ticket would be wrong, and [b]
            // files a standalone bug without touching it.
            effect: if status == Status::InUAT {
                "files a bug and sends THIS ticket back; for a regression elsewhere use [b]"
                    .to_string()
            } else {
                "files a bug, parent back to In Progress".to_string()
            },
            available: in_qa_column,
            blocked_reason: wrong_column("a QA column"),
        },
        Action {
            key: "r",
            name: "Return (not fixed)",
            effect: "back to In Progress, clears BOTH handoff labels (full re-verify)".to_string(),
            available: status != Status::InProgress,
            blocked_reason: Some("already In Progress".to_string()),
        },
        Action {
            key: "x",
            name: "Clear handoff labels",
            effect: "makes a returned ticket testable again".to_string(),
            available: has_staging || has_uat,
            blocked_reason: Some("no handoff label on this item".to_string()),
        },
    ]
}

/// What an action does, or why it does not apply here.
fn action_detail(a: &Action) -> String {
    if a.available {
        a.effect.clone()
    } else {
        a.blocked_reason
            .clone()
            .unwrap_or_else(|| "not available".to_string())
    }
}

/// Render the QA Actions popup.
pub fn render(f: &mut Frame, app: &App, item_idx: usize) {
    let number = app
        .items
        .get(item_idx)
        .and_then(|i| i.number)
        .map(|n| format!("#{}", n))
        .unwrap_or_else(|| "item".to_string());
    let status_label = app
        .items
        .get(item_idx)
        .map(|i| i.status.label())
        .unwrap_or("");

    let actions = actions_for(app, item_idx);

    let popup_w = (f.area().width.saturating_sub(4)).min(64);
    let inner_w = popup_w.saturating_sub(4) as usize;
    let status = app
        .items
        .get(item_idx)
        .map(|i| i.status.clone())
        .unwrap_or_default();

    // Four layouts, roomiest first, so every action and the way out stay on
    // screen down to the 60x10 minimum:
    // - Full: name, then what it does, wrapped;
    // - Compact: one line per action;
    // - Tight: the same without the spacer rows, the column moving into the
    //   title;
    // - Grid: two actions per row, names only.
    let rows_avail = f.area().height.saturating_sub(4) as usize;
    let n = actions.len();
    let full_rows = actions
        .iter()
        .map(|a| 1 + super::utils::word_wrap(&action_detail(a), inner_w.saturating_sub(8)).len())
        .sum::<usize>()
        + 6;
    let layout = if full_rows <= rows_avail {
        Layout::Full
    } else if n + 6 <= rows_avail {
        Layout::Compact
    } else if n + 2 <= rows_avail {
        Layout::Tight
    } else {
        Layout::Grid
    };
    let roomy = matches!(layout, Layout::Full | Layout::Compact);

    let mut lines: Vec<Line> = Vec::new();
    if roomy {
        lines.push(Line::from(vec![
            Span::styled("  Currently in  ", theme::dim()),
            Span::styled(status_label.to_string(), theme::status_style(&status)),
        ]));
        lines.push(Line::from(""));
    }

    let styles = |a: &Action| {
        if a.available {
            (theme::highlight(), theme::normal())
        } else {
            (theme::dim(), theme::dim())
        }
    };
    if layout == Layout::Grid {
        let col_w = inner_w.saturating_sub(2) / 2;
        for pair in actions.chunks(2) {
            let mut spans = vec![Span::raw("  ")];
            for a in pair {
                let (key_style, name_style) = styles(a);
                let name_w = col_w.saturating_sub(5);
                spans.push(Span::styled(format!("[{}] ", a.key), key_style));
                spans.push(Span::styled(
                    format!(
                        "{:<w$}",
                        super::utils::trunc(a.name, name_w),
                        w = name_w + 1
                    ),
                    name_style,
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    for a in actions.iter().filter(|_| layout != Layout::Grid) {
        let (key_style, name_style) = styles(a);
        let detail = action_detail(a);
        let detail_style = if a.available {
            theme::dim()
        } else {
            theme::warning()
        };
        if layout != Layout::Full {
            let room = inner_w.saturating_sub(6 + 22);
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", a.key), key_style),
                Span::styled(format!("{:<22}", a.name), name_style),
                Span::styled(super::utils::trunc(&detail, room), detail_style),
            ]));
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("  [{}] ", a.key), key_style),
            Span::styled(format!("{:<22}", a.name), name_style),
        ]));
        // Indent-wrapped so a long reason never spills out of the popup.
        for (i, chunk) in super::utils::word_wrap(&detail, inner_w.saturating_sub(8))
            .into_iter()
            .enumerate()
        {
            let prefix = if i == 0 { "      " } else { "        " };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, chunk),
                detail_style,
            )));
        }
    }

    // The other ticket keys; the grid keeps them only when a row is left.
    if roomy {
        lines.push(Line::from(""));
    }
    if layout != Layout::Grid || n.div_ceil(2) + 2 <= rows_avail {
        // The row is 59 columns; a narrow popup shortens its last label.
        let enhancement = if inner_w + 2 >= 59 {
            "Enhancement"
        } else {
            "Enh."
        };
        lines.push(Line::from(vec![
            Span::styled("  [c] ", theme::highlight()),
            Span::styled("Comment", theme::normal()),
            Span::styled("  [m] ", theme::highlight()),
            Span::styled("Move", theme::normal()),
            Span::styled("  [t] ", theme::highlight()),
            Span::styled("Task", theme::normal()),
            Span::styled("  [b] ", theme::highlight()),
            Span::styled("Bug", theme::normal()),
            Span::styled("  [e] ", theme::highlight()),
            Span::styled(enhancement, theme::normal()),
        ]));
    }
    if roomy {
        lines.push(Line::from(""));
    }
    lines.push(super::utils::footer_line(&[("Esc", "Cancel")], inner_w));

    // Height follows the content, clamped to the terminal, so nothing clips.
    let wanted_h = lines.len() as u16 + 2;
    let area = centered_rect(
        popup_w,
        wanted_h.min(f.area().height.saturating_sub(2)),
        f.area(),
    );
    f.render_widget(Clear, area);

    let title = if roomy {
        format!(" QA Action - {} ", number)
    } else {
        format!(" QA Action - {} ({}) ", number, status_label)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(title, theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// How much of the QA menu fits (see `render`).
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Full,
    Compact,
    Tight,
    Grid,
}
