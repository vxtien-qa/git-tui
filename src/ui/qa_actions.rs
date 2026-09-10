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
    // Pass UAT covers the whole regression batch, so say how big it is.
    let uat_batch = app.items_in_column(&Status::InUAT).len().max(1);

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
            effect: if uat_batch > 1 {
                format!(
                    "regression clear: moves all {} In UAT tickets to Tech Complete, one Slack release summary",
                    uat_batch
                )
            } else {
                "regression clear: moves to Tech Complete + Slack release summary".to_string()
            },
            available: status == Status::InUAT,
            blocked_reason: wrong_column("In UAT"),
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

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  Currently in  ", theme::dim()),
        Span::styled(
            status_label.to_string(),
            theme::status_style(
                &app.items
                    .get(item_idx)
                    .map(|i| i.status.clone())
                    .unwrap_or_default(),
            ),
        ),
    ]));
    lines.push(Line::from(""));

    for a in &actions {
        let (key_style, name_style) = if a.available {
            (theme::highlight(), theme::normal())
        } else {
            (theme::dim(), theme::dim())
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  [{}] ", a.key), key_style),
            Span::styled(format!("{:<22}", a.name), name_style),
        ]));

        let detail = if a.available {
            a.effect.clone()
        } else {
            a.blocked_reason
                .clone()
                .unwrap_or_else(|| "not available".to_string())
        };
        // Indent-wrapped so a long reason never spills out of the popup.
        for (i, chunk) in super::utils::word_wrap(&detail, inner_w.saturating_sub(8))
            .into_iter()
            .enumerate()
        {
            let prefix = if i == 0 { "      " } else { "        " };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, chunk),
                if a.available {
                    theme::dim()
                } else {
                    theme::warning()
                },
            )));
        }
    }

    lines.push(Line::from(""));
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
        Span::styled("Enhancement", theme::normal()),
    ]));
    lines.push(Line::from(""));
    lines.push(super::utils::footer_line(&[("Esc", "Cancel")], inner_w));

    // Height follows the content, clamped to the terminal, so nothing clips.
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
        .title(Span::styled(
            format!(" QA Action - {} ", number),
            theme::title(),
        ));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
