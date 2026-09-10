use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

const PRIORITIES: [&str; 6] = ["--", "P0", "P1", "P2", "P3", "P4"];
const STACKS: [&str; 5] = ["--", "FE", "BE", "App UI", "App BE"];
const ENVS: [&str; 5] = crate::app::App::BUG_ENVS;
const BROKEN: [&str; 2] = ["Completely", "Partially"];
const VISIBLE: [&str; 2] = ["Yes", "No"];
// Must match the strings written into the GitHub issue body
// (handlers/actions.rs `workaround_opts`) - the priority automation reads them.
const WORKAROUND: [&str; 3] = ["Easy workaround", "Difficult workaround", "No Workaround"];
const IMPACT_PRIO: [&str; 2] = ["Yes", "No"];

// Field indices:
// 0=Title, 1=Description, 2=Pre-Cond, 3=Test Data,
// 4=Steps, 5=Expected, 6=Actual, 7=Impact, 8=Evidence, 9=Security,
// 10=Priority, 11=Stack, 12=Env,
// 13=Is broken?, 14=Visible?, 15=Workaround?, 16=Impact prioritised?,
// 17=Assignee

/// Number of focusable fields in this form (indices 0..=17).
const BUG_FIELD_COUNT: usize = 18;

/// Render Bug Report form.
pub fn render(f: &mut Frame, app: &App, parent_idx: Option<usize>, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let parent_info = parent_idx.and_then(|idx| app.items.get(idx));
    // The form is taller than the screen, so the title carries the field
    // position: the focus pointer alone does not say how far in you are.
    let title = if let Some(parent) = parent_info {
        format!(
            " {} (child of #{})  field {}/{} ",
            app.bug_kind.form_title(),
            parent.number.unwrap_or(0),
            app.bug_field_idx + 1,
            BUG_FIELD_COUNT
        )
    } else {
        format!(
            " {}  field {}/{} ",
            app.bug_kind.form_title(),
            app.bug_field_idx + 1,
            BUG_FIELD_COUNT
        )
    };

    let fi = app.bug_field_idx;
    let mut lines = vec![Line::from("")];

    // Parent info
    if let Some(parent) = parent_info {
        lines.push(Line::from(vec![
            Span::styled("  Parent:    ", theme::dim()),
            Span::styled(
                format!("#{} {}", parent.number.unwrap_or(0), parent.title),
                theme::normal(),
            ),
        ]));
    }
    lines.push(Line::from(""));

    // ── Text fields ──
    let text_fields: Vec<(usize, &str, &str)> = vec![
        (0, "Title *", &app.bug_title),
        (1, "Descript.", &app.bug_description),
        (2, "Pre-Cond", &app.bug_precondition),
        (3, "Test Data", &app.bug_test_data),
        (4, "Steps", &app.bug_steps),
        (5, "Expected", &app.bug_expected),
        (6, "Actual", &app.bug_actual),
        (7, "Impact", &app.bug_impact),
        (8, "Evidence", &app.bug_evidence),
        (9, "Security", &app.bug_security),
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
            app.bug_cursor,
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
    // Priority (idx=10)
    if fi == 10 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Priority",
        &PRIORITIES,
        app.bug_priority_idx,
        fi == 10,
        radio_w,
    );
    // Stack (idx=11)
    if fi == 11 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Stack",
        &STACKS,
        app.bug_stack_idx,
        fi == 11,
        radio_w,
    );
    // Env (idx=12)
    if fi == 12 {
        focus_start_line = lines.len();
    }
    render_radio_line(&mut lines, "Env", &ENVS, app.bug_env_idx, fi == 12, radio_w);

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ── Priority Scoring ──",
        theme::dim(),
    )));

    // Scoring dropdowns (idx=13-16)
    if fi == 13 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Broken?",
        &BROKEN,
        app.bug_broken_idx,
        fi == 13,
        radio_w,
    );
    if fi == 14 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Visible?",
        &VISIBLE,
        app.bug_visible_idx,
        fi == 14,
        radio_w,
    );
    if fi == 15 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Workaround?",
        &WORKAROUND,
        app.bug_workaround_idx,
        fi == 15,
        radio_w,
    );
    if fi == 16 {
        focus_start_line = lines.len();
    }
    render_radio_line(
        &mut lines,
        "Impact?",
        &IMPACT_PRIO,
        app.bug_impact_prio_idx,
        fi == 16,
        radio_w,
    );

    lines.push(Line::from(""));

    // Assignee picker (idx=17)
    let assignee_display = if app.bug_assignees.is_empty() {
        "--".to_string()
    } else {
        app.bug_assignees.join(", ")
    };
    let prefix = if fi == 17 { theme::icon_arrow() } else { " " };
    let style = if fi == 17 {
        theme::selected()
    } else {
        theme::normal()
    };
    if fi == 17 {
        focus_start_line = lines.len();
    }
    lines.push(Line::from(vec![
        Span::styled(format!(" {} {:<10}", prefix, "Assignee:"), theme::dim()),
        Span::styled(format!("[{}]", assignee_display), style),
        Span::styled(" (Enter to search & assign)", theme::dim()),
    ]));

    // Sprint (auto)
    let sprint = app.board_sprint_filter.as_deref().unwrap_or("--");
    lines.push(Line::from(vec![
        Span::styled("   Sprint:     ", theme::dim()),
        Span::styled(format!("[{}]", sprint), theme::normal()),
    ]));

    lines.push(Line::from(""));
    let footnote = format!(
        "* Auto-add to project board + label(s) {} + issue type '{}' after submit",
        app.bug_kind
            .labels()
            .iter()
            .map(|l| format!("'{}'", l))
            .collect::<Vec<_>>()
            .join(", "),
        app.bug_kind.issue_type()
    );
    for line in super::utils::word_wrap(&footnote, area.width.saturating_sub(6) as usize) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme::dim(),
        )));
    }

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
