use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;
use crate::slack::webhook;

/// Render Settings screen.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Configuration", theme::title())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Owner:         ", theme::dim()),
            Span::styled(&app.config.owner, theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("  Project:       ", theme::dim()),
            Span::styled(format!("#{}", app.config.project_number), theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("  User:          ", theme::dim()),
            Span::styled(format!("@{}", app.current_user), theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("  Bug repo:      ", theme::dim()),
            Span::styled(&app.config.bug_repo, theme::normal()),
        ]),
        Line::from(vec![
            Span::styled("  Config file:   ", theme::dim()),
            Span::styled(
                crate::config::Config::path().display().to_string(),
                theme::dim(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("  gh CLI Status", theme::title())),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {} ", theme::icon_ok()), theme::success()),
            Span::styled(
                format!("Authenticated as @{}", app.current_user),
                theme::normal(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("  API Rate Limit", theme::title())),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Quota:         ", theme::dim()),
            Span::styled(crate::gh::client::rate_limit_info(), theme::normal()),
        ]),
        Line::from(""),
    ];

    // ─── Board columns ───
    // Surfaces a Status field that no longer matches this build: unknown
    // columns are silently counted as Backlog everywhere else.
    lines.push(Line::from(Span::styled("  Board Columns", theme::title())));
    lines.push(Line::from(""));
    match app.status_drift_warning() {
        Some(warn) => {
            for line in crate::ui::utils::word_wrap(&warn, area.width.saturating_sub(8) as usize) {
                lines.push(Line::from(vec![
                    Span::styled("  ", theme::dim()),
                    Span::styled(line, theme::warning()),
                ]));
            }
        }
        None => lines.push(Line::from(vec![
            Span::styled(format!("  {} ", theme::icon_ok()), theme::success()),
            Span::styled(
                format!(
                    "{} status columns match the board",
                    crate::models::status::Status::all_columns().len()
                ),
                theme::normal(),
            ),
        ])),
    }
    lines.push(Line::from(""));

    // ─── Slack Integration section ───
    lines.push(Line::from(Span::styled(
        "  Slack Integration",
        theme::title(),
    )));
    lines.push(Line::from(""));
    if app.config.is_slack_configured() {
        lines.push(Line::from(vec![
            Span::styled("  Status:        ", theme::dim()),
            Span::styled(format!("{} Connected", theme::icon_ok()), theme::success()),
        ]));
        if let Some(ref url) = app.config.slack_webhook_url {
            lines.push(Line::from(vec![
                Span::styled("  Webhook:       ", theme::dim()),
                Span::styled(webhook::mask_url(url), theme::dim()),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Status:        ", theme::dim()),
            Span::styled(
                format!("{} Not configured", theme::icon_fail()),
                theme::dim(),
            ),
        ]));
    }
    lines.push(Line::from(""));

    // ─── Actions section ───
    lines.push(Line::from(Span::styled("  Actions", theme::title())));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [r] ", theme::highlight()),
        Span::styled("Refresh auth / re-login", theme::normal()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [c] ", theme::highlight()),
        Span::styled("Clear cache", theme::normal()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [s] ", theme::highlight()),
        Span::styled("Edit config (Setup Wizard)", theme::normal()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [i] ", theme::highlight()),
        Span::styled(
            if app.config.is_slack_configured() {
                "Slack setup (Webhook + Bot Token)"
            } else {
                "Setup Slack (Webhook + Bot Token)"
            },
            theme::normal(),
        ),
    ]));
    if app.config.is_slack_configured() {
        lines.push(Line::from(vec![
            Span::styled("  [n] ", theme::highlight()),
            Span::styled("Configure notifications", theme::normal()),
        ]));
        if app.config.is_slack_bot_configured() {
            let mapped = app.config.slack_user_map.len();
            lines.push(Line::from(vec![
                Span::styled("  [u] ", theme::highlight()),
                Span::styled(
                    format!("Map GitHub → Slack users ({})", mapped),
                    theme::normal(),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  [u] ", theme::dim()),
                Span::styled("Map GitHub → Slack users ", theme::dim()),
                Span::styled("(needs Bot Token in [i])", theme::warning()),
            ]));
        }
    }
    lines.push(Line::from(vec![
        Span::styled("  [l] ", theme::highlight()),
        Span::styled(
            format!("Edit Stack Leads ({} stacks)", app.config.stack_leads.len()),
            theme::normal(),
        ),
    ]));
    lines.push(Line::from(""));

    // ─── Stack Leads section ───
    if !app.config.stack_leads.is_empty() {
        lines.push(Line::from(Span::styled("  Stack Leads", theme::title())));
        lines.push(Line::from(""));
        let mut stacks: Vec<_> = app.config.stack_leads.keys().cloned().collect();
        stacks.sort();
        for stack in &stacks {
            if let Some(leads) = app.config.stack_leads.get(stack) {
                let leads_str = leads
                    .iter()
                    .map(|l| format!("@{}", l))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<12} ", stack), theme::dim()),
                    Span::styled(leads_str, theme::normal()),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

    // Apply scroll (capped so content is never fully scrolled past)
    let max_scroll = lines.len().saturating_sub(1);
    let scroll = app.settings_scroll.min(max_scroll);
    let visible_lines: Vec<Line> = lines.into_iter().skip(scroll).collect();

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Length(1), // footer
        ])
        .split(area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" Settings ", theme::title()));

    f.render_widget(Paragraph::new(visible_lines).block(block), chunks[0]);

    // Footer
    let footer = super::utils::footer_line(
        &[
            ("s", "Setup"),
            ("r", "Re-auth"),
            ("c", "Clear cache"),
            ("i", "Slack"),
            ("n", "Notify"),
            ("u", "User map"),
            ("l", "Leads"),
            ("?", "Help"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
