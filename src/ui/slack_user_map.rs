use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// Render the GitHub → Slack User Mapping screen.
#[allow(clippy::needless_range_loop)]
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  GitHub → Slack User Mapping",
        theme::title(),
    )));
    lines.push(Line::from(""));

    // Collect unique GitHub usernames from all items
    let github_users = collect_github_users(app);

    if github_users.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No GitHub users found in current project items.",
            theme::dim(),
        )));
    } else if app.slack_map_picking {
        // ─── Slack member picker mode ───
        render_picker(app, &github_users, &mut lines);
    } else {
        // ─── Main mapping view ───
        render_mapping_list(app, &github_users, &mut lines);
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" User Mapping ", theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Footer
    let footer = if app.slack_map_picking {
        super::utils::footer_line(
            &[
                ("Type", "Filter"),
                (theme::icon_up_down(), "Navigate"),
                ("Enter", "Select"),
                ("Esc", "Cancel"),
            ],
            area.width as usize,
        )
    } else {
        super::utils::footer_line(
            &[
                (theme::icon_up_down(), "Navigate"),
                ("Enter", "Pick Slack user"),
                ("x", "Clear"),
                ("Esc", "Save & Back"),
            ],
            area.width as usize,
        )
    };
    f.render_widget(Paragraph::new(footer), chunks[1]);
}

/// Render the main mapping list (GitHub user → Slack member).
fn render_mapping_list(app: &App, github_users: &[String], lines: &mut Vec<Line>) {
    // Header
    lines.push(Line::from(vec![
        Span::styled("    GitHub User          ", theme::dim()),
        Span::styled("    Slack Member", theme::dim()),
    ]));
    lines.push(Line::from(Span::styled(
        "  ─────────────────────  ─────────────────────",
        theme::dim(),
    )));

    for (i, gh_user) in github_users.iter().enumerate() {
        let is_selected = i == app.slack_map_cursor;
        let prefix = if is_selected {
            format!(" {} ", theme::icon_arrow())
        } else {
            "   ".to_string()
        };

        let gh_display = format!("{:<22}", gh_user);

        let slack_display = if let Some(mapping) = app.config.slack_user_map.get(gh_user) {
            format!(
                "→  {} ({})",
                mapping.slack_display,
                &mapping.slack_id[..mapping.slack_id.len().min(8)]
            )
        } else {
            "→  [not mapped]".to_string()
        };

        let style = if is_selected {
            theme::selected()
        } else if app.config.slack_user_map.contains_key(gh_user) {
            theme::success()
        } else {
            theme::normal()
        };

        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                if is_selected {
                    theme::highlight()
                } else {
                    theme::normal()
                },
            ),
            Span::styled(gh_display, style),
            Span::styled(
                slack_display,
                if app.config.slack_user_map.contains_key(gh_user) {
                    theme::success()
                } else {
                    theme::dim()
                },
            ),
        ]));
    }

    lines.push(Line::from(""));

    // Stats
    let mapped = github_users
        .iter()
        .filter(|u| app.config.slack_user_map.contains_key(*u))
        .count();
    lines.push(Line::from(Span::styled(
        format!("  Mapped: {}/{} users", mapped, github_users.len()),
        if mapped == github_users.len() {
            theme::success()
        } else {
            theme::dim()
        },
    )));

    if app.slack_members.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "  {} No Slack members loaded. Configure Bot Token in Slack Setup first.",
                theme::icon_warning()
            ),
            theme::warning(),
        )));
    }
}

/// Render the Slack member picker (when user presses Enter on a GitHub user).
fn render_picker(app: &App, github_users: &[String], lines: &mut Vec<Line>) {
    let gh_user = github_users
        .get(app.slack_map_cursor)
        .cloned()
        .unwrap_or_default();

    lines.push(Line::from(Span::styled(
        format!("  Pick Slack member for: {}", gh_user),
        theme::highlight(),
    )));
    lines.push(Line::from(""));

    // Search field
    let cursor = theme::icon_cursor();
    lines.push(Line::from(vec![
        Span::styled("  Filter: ", theme::dim()),
        Span::styled(
            format!("{}{}", app.slack_map_search, cursor),
            theme::selected(),
        ),
    ]));
    lines.push(Line::from(""));

    // Filter members
    let filtered: Vec<&crate::slack::users::SlackUser> = app
        .slack_members
        .iter()
        .filter(|m| {
            if app.slack_map_search.is_empty() {
                return true;
            }
            let q = app.slack_map_search.to_lowercase();
            m.real_name.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q)
                || m.display_name.to_lowercase().contains(&q)
        })
        .collect();

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No matching members found.",
            theme::dim(),
        )));
    } else {
        // Show up to 15 visible items with scroll
        let max_items = 15;
        let start = if app.slack_map_member_cursor >= max_items {
            app.slack_map_member_cursor - max_items + 1
        } else {
            0
        };
        let end = (start + max_items).min(filtered.len());

        for (display_i, &m) in filtered[start..end].iter().enumerate() {
            let actual_i = start + display_i;
            let is_selected = actual_i == app.slack_map_member_cursor;

            let prefix = if is_selected {
                format!("  {} ", theme::icon_arrow())
            } else {
                "    ".to_string()
            };

            let display = if m.display_name.is_empty() || m.display_name == m.real_name {
                format!("{:<20} ({})", m.real_name, m.name)
            } else {
                format!("{:<20} ({})", m.real_name, m.display_name)
            };

            let id_hint = format!("  {}", &m.id[..m.id.len().min(11)]);

            let style = if is_selected {
                theme::selected()
            } else {
                theme::normal()
            };

            lines.push(Line::from(vec![
                Span::styled(
                    prefix,
                    if is_selected {
                        theme::highlight()
                    } else {
                        theme::normal()
                    },
                ),
                Span::styled(display, style),
                Span::styled(id_hint, theme::dim()),
            ]));
        }

        if filtered.len() > max_items {
            lines.push(Line::from(Span::styled(
                format!("    ... {} total members", filtered.len()),
                theme::dim(),
            )));
        }
    }
}

/// Collect unique GitHub usernames from project items (assignees).
pub fn collect_github_users(app: &App) -> Vec<String> {
    let mut users: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Add all assignees from items
    for item in &app.items {
        for assignee in &item.assignees {
            if seen.insert(assignee.clone()) {
                users.push(assignee.clone());
            }
        }
    }

    // Also add current_user if not already there
    if !app.current_user.is_empty() && seen.insert(app.current_user.clone()) {
        users.push(app.current_user.clone());
    }

    // Sort alphabetically
    users.sort_by_key(|a| a.to_lowercase());
    users
}
