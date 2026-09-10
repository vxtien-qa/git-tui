use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// Render the Stack Leads editor screen.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let mut stacks: Vec<String> = app.config.stack_leads.keys().cloned().collect();
    stacks.sort();

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Stack Leads Configuration", theme::title())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑↓ ", theme::dim()),
            Span::styled("navigate  ", theme::normal()),
            Span::styled("Enter ", theme::dim()),
            Span::styled("edit  ", theme::normal()),
            Span::styled("a ", theme::dim()),
            Span::styled("add stack  ", theme::normal()),
            Span::styled("d ", theme::dim()),
            Span::styled("delete  ", theme::normal()),
            Span::styled("Esc ", theme::dim()),
            Span::styled("save & back", theme::normal()),
        ]),
        Line::from(""),
    ];

    if stacks.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No stack leads configured. Press [a] to add.",
            theme::dim(),
        )));
    } else {
        for (i, stack) in stacks.iter().enumerate() {
            let is_selected = i == app.stack_lead_cursor;
            let leads = app
                .config
                .stack_leads
                .get(stack)
                .map(|v| {
                    v.iter()
                        .map(|l| format!("@{}", l))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();

            if is_selected && app.stack_lead_editing {
                // Editing mode: show input
                lines.push(Line::from(vec![
                    Span::styled("  ▸ ", theme::highlight()),
                    Span::styled(format!("{:<12} ", stack), theme::highlight()),
                    Span::styled(&app.stack_lead_input, theme::normal()),
                    Span::styled("▏", theme::highlight()),
                ]));
            } else if is_selected {
                // Selected row
                lines.push(Line::from(vec![
                    Span::styled("  ▸ ", theme::highlight()),
                    Span::styled(format!("{:<12} ", stack), theme::highlight()),
                    Span::styled(leads, theme::normal()),
                ]));
            } else {
                // Normal row
                lines.push(Line::from(vec![
                    Span::styled("    ", theme::dim()),
                    Span::styled(format!("{:<12} ", stack), theme::dim()),
                    Span::styled(leads, theme::normal()),
                ]));
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Format: comma-separated usernames (e.g. user1, user2)",
        theme::dim(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" Stack Leads ", theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), area);
}
