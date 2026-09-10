use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

const PRIORITIES: [&str; 6] = ["--", "P0", "P1", "P2", "P3", "P4"];
const STACKS: [&str; 5] = ["--", "FE", "BE", "App UI", "App BE"];

// Field indices:
// 0=Title, 1=Description, 2=Outcome, 3=Priority, 4=Stack, 5=Assignee

/// Number of focusable fields in this form.
const TASK_FIELD_COUNT: usize = 6;

/// Render Task form.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let title = format!(
        " Create Task  field {}/{} ",
        app.task_field_idx + 1,
        TASK_FIELD_COUNT
    );
    let fi = app.task_field_idx;
    let mut lines = vec![Line::from("")];

    // ── Text fields ──
    let text_fields: Vec<(usize, &str, &str)> = vec![
        (0, "Title *", &app.task_title),
        (1, "Descript.", &app.task_description),
        (2, "Outcome", &app.task_outcome),
    ];

    let max_text_width = area.width.saturating_sub(18) as usize;
    // Radio rows wrap inside the panel instead of running under its border.
    let radio_w = area.width.saturating_sub(4) as usize;
    let mut focus_start_line = 0;

    for (idx, label, value) in &text_fields {
        let is_focused = fi == *idx;
        let field_start = lines.len();
        let cursor_line = super::form_widgets::render_text_field(
            &mut lines,
            label,
            value,
            is_focused,
            app.task_cursor,
            max_text_width,
        );
        if is_focused {
            // Anchor the scroll on the cursor's rendered line, not the top
            // of the field - a tall field (e.g. after a long paste) keeps
            // the cursor in view.
            focus_start_line = field_start + cursor_line;
        }
    }

    lines.push(Line::from(""));

    // ── Selectors ──
    if fi == 3 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Priority",
        &PRIORITIES,
        app.task_priority_idx,
        fi == 3,
        radio_w,
    );
    if fi == 4 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Stack",
        &STACKS,
        app.task_stack_idx,
        fi == 4,
        radio_w,
    );

    let assignee_display = if app.task_assignees.is_empty() {
        "--".to_string()
    } else {
        app.task_assignees.join(", ")
    };

    let prefix = if fi == 5 { theme::icon_arrow() } else { " " };
    let style = if fi == 5 {
        theme::selected()
    } else {
        theme::normal()
    };
    if fi == 5 {
        focus_start_line = lines.len();
    }
    lines.push(Line::from(vec![
        Span::styled(format!(" {} {:<10}", prefix, "Assignee"), theme::dim()),
        Span::styled(format!("[{}]", assignee_display), style),
        Span::styled(" (Enter to search & assign)", theme::dim()),
    ]));

    lines.push(Line::from(""));

    // Sprint (auto)
    let sprint = app.board_sprint_filter.as_deref().unwrap_or("--");
    lines.push(Line::from(vec![
        Span::styled("   Sprint:    ", theme::dim()),
        Span::styled(format!("[{}]", sprint), theme::normal()),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  * Auto-add to project board + label 'Task' after submit",
        theme::dim(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(title, theme::title()));

    let view_height = chunks[0].height as usize;
    let mut scroll: u16 = 0;
    if focus_start_line >= view_height.saturating_sub(5) {
        scroll = focus_start_line.saturating_sub(view_height / 2) as u16;
    }
    if lines.len() > view_height && scroll as usize > lines.len() - view_height {
        scroll = (lines.len() - view_height) as u16;
    }

    let paragraph = Paragraph::new(lines).block(block).scroll((scroll, 0));
    f.render_widget(paragraph, chunks[0]);

    // Footer
    let footer = super::utils::footer_line(
        &[
            ("Tab", "Next field"),
            (theme::icon_up_down(), "Line/Field"),
            (theme::icon_left_right(), "Select"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+S", "Submit"),
            ("Esc", "Back (draft kept)"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}

use super::form_widgets::render_radio_line;
