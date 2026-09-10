use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::{App, AssigneeContext};

pub fn render(f: &mut Frame, app: &App, context: &AssigneeContext, area: Rect) {
    let popup_area = crate::ui::utils::centered_rect(60, 20, area);
    f.render_widget(Clear, popup_area);

    let (title, active_assignees) = match context {
        AssigneeContext::Bug(_, _) => ("Assignees (Bug Report)", &app.bug_assignees),
        AssigneeContext::Task => ("Assignees (Task Form)", &app.task_assignees),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(format!(" {} ", title), theme::title()));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Search box
            Constraint::Min(1),    // List
            Constraint::Length(1), // Footer
        ])
        .split(block.inner(popup_area));

    f.render_widget(block, popup_area);

    // Search Box
    let search_style = theme::selected();
    let search_line = Line::from(vec![
        Span::styled(" Search: ", theme::dim()),
        Span::styled(format!("{}_", app.assignee_search), search_style),
    ]);
    let search_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(theme::border());
    f.render_widget(Paragraph::new(search_line).block(search_block), chunks[0]);

    // List of Users
    let mut users: Vec<String> = app.config.slack_user_map.keys().cloned().collect();
    users.sort();

    // Filter by search
    let query = app.assignee_search.to_lowercase();
    let filtered: Vec<&String> = users
        .iter()
        .filter(|u| u.to_lowercase().contains(&query))
        .collect();

    let mut list_lines = Vec::new();
    let limit = chunks[1].height.saturating_sub(2) as usize;
    let start_idx = app.assignee_cursor.saturating_sub(limit / 2);

    for (i, user) in filtered.iter().enumerate() {
        if i < start_idx {
            continue;
        }
        if list_lines.len() >= chunks[1].height as usize {
            break;
        }

        let is_selected = app.assignee_cursor == i;
        let is_assigned = active_assignees.contains(*user);

        let checkbox = if is_assigned {
            theme::icon_checked().to_string()
        } else {
            theme::icon_unchecked().to_string()
        };

        let prefix = if is_selected {
            theme::icon_arrow()
        } else {
            " "
        };
        let style = if is_selected {
            theme::selected()
        } else {
            theme::normal()
        };

        list_lines.push(Line::from(vec![
            Span::styled(
                format!(" {} {} ", prefix, checkbox),
                if is_assigned {
                    theme::success()
                } else {
                    theme::dim()
                },
            ),
            Span::styled(user.to_string(), style),
        ]));
    }

    if filtered.is_empty() {
        list_lines.push(Line::from(Span::styled(
            "  No users found.",
            theme::warning(),
        )));
        list_lines.push(Line::from(Span::styled(
            "  This list comes from the Slack user map -",
            theme::dim(),
        )));
        list_lines.push(Line::from(Span::styled(
            "  map users in Settings → [u] first.",
            theme::dim(),
        )));
    }

    f.render_widget(Paragraph::new(list_lines), chunks[1]);

    // Footer (sized to the popup, not the screen)
    let footer = crate::ui::utils::footer_line(
        &[
            (theme::icon_up_down(), "Nav"),
            ("Space", "Toggle"),
            ("Enter/Esc", "Done"),
        ],
        chunks[2].width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[2]);
}
