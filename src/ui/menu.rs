use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;
use crate::models::status::Status;

/// Menu item labels (must match indices in handle_menu / activate_menu_item).
const MENU_LABELS: &[&str] = &[
    "Board View",
    "My Tasks",
    "Search & Filter",
    "Settings",
    "Dashboard",
    "Notifications",
];

const MENU_KEYS: &[&str] = &["1", "2", "3", "4", "5", "6"];

/// Map a Screen variant to its menu index (None if not a top-level menu item).
fn screen_to_menu_idx(screen: &crate::app::Screen) -> Option<usize> {
    use crate::app::Screen;
    match screen {
        Screen::Board => Some(0),
        Screen::MyTasks => Some(1),
        Screen::Search => Some(2),
        Screen::Settings => Some(3),
        Screen::Dashboard => Some(4),
        Screen::Notifications => Some(5),
        Screen::Help => Some(6),
        _ => None,
    }
}

/// Which top-level section the user is in, even from a sub-screen.
///
/// Walks the navigation stack back to the nearest menu destination, so a
/// ticket opened from the Board keeps Board highlighted. Previously any
/// sub-screen (item detail, forms, history) highlighted nothing at all and
/// the sidebar stopped telling the user where they were.
fn active_menu_idx(app: &App) -> Option<usize> {
    if app.screen == crate::app::Screen::Menu {
        return Some(app.menu_selected);
    }
    if let Some(i) = screen_to_menu_idx(&app.screen) {
        return Some(i);
    }
    app.previous_screens
        .iter()
        .rev()
        .find_map(screen_to_menu_idx)
}

/// How a sidebar row should be drawn.
enum RowState {
    /// The menu itself has focus and this is the cursor row.
    Cursor,
    /// The user is inside this section (possibly several screens deep).
    Active,
    Idle,
}

fn menu_row<'a>(key: &str, label: &str, inner_w: usize, state: RowState) -> Line<'a> {
    match state {
        RowState::Cursor => Line::from(Span::styled(
            pad_line(
                &format!(" {} [{}] {}", theme::icon_arrow(), key, label),
                inner_w,
            ),
            theme::selected(),
        )),
        RowState::Active => Line::from(Span::styled(
            pad_line(
                &format!(" {} [{}] {}", theme::icon_arrow(), key, label),
                inner_w,
            ),
            theme::title(),
        )),
        RowState::Idle => Line::from(vec![
            Span::styled(format!("   [{}] ", key), theme::highlight()),
            Span::styled(label.to_string(), theme::normal()),
        ]),
    }
}

/// Pad a line's text content with trailing spaces to fill the given width.
fn pad_line(text: &str, width: usize) -> String {
    let text_w = unicode_width::UnicodeWidthStr::width(text);
    if text_w >= width {
        text.to_string()
    } else {
        format!("{}{}", text, " ".repeat(width - text_w))
    }
}

/// Render the menu sidebar in the given area.
/// Highlights the selected item (when on Menu screen) or the active screen.
pub fn render_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let my_count = app.my_tasks_grouped().len();

    // Inner width = sidebar width minus 2 border columns
    let inner_w = (area.width as usize).saturating_sub(2);

    // Determine which item to highlight
    let is_menu_focused = app.screen == crate::app::Screen::Menu;
    let active_idx = active_menu_idx(app);

    let row_state = |i: usize| {
        if active_idx == Some(i) {
            if is_menu_focused {
                RowState::Cursor
            } else {
                RowState::Active
            }
        } else {
            RowState::Idle
        }
    };

    let mut menu_lines: Vec<Line> = Vec::new();
    for (i, label) in MENU_LABELS.iter().enumerate() {
        let display = if i == 1 {
            format!("{} ({})", label, my_count)
        } else {
            label.to_string()
        };
        menu_lines.push(menu_row(MENU_KEYS[i], &display, inner_w, row_state(i)));
    }

    // Blank line separator before Help/Quit
    menu_lines.push(Line::from(""));
    menu_lines.push(menu_row("?", "Help", inner_w, row_state(6)));
    menu_lines.push(menu_row(
        "q",
        "Quit",
        inner_w,
        if is_menu_focused && app.menu_selected == 7 {
            RowState::Cursor
        } else {
            RowState::Idle
        },
    ));

    // Version footer - push to bottom with spacer lines
    let used_lines = menu_lines.len();
    let inner_h = (area.height as usize).saturating_sub(2); // minus top/bottom borders
    if inner_h > used_lines + 1 {
        for _ in 0..(inner_h - used_lines - 1) {
            menu_lines.push(Line::from(""));
        }
    }
    let version = env!("CARGO_PKG_VERSION");
    let ver_text = format!("v{}", version);
    let ver_pad = " ".repeat(inner_w.saturating_sub(ver_text.len()) / 2);
    menu_lines.push(Line::from(Span::styled(
        format!("{}{}", ver_pad, ver_text),
        theme::dim(),
    )));

    let menu_block = Block::default()
        .borders(Borders::ALL)
        .border_style(if is_menu_focused {
            theme::title()
        } else {
            theme::border()
        })
        .title(Span::styled(" Menu ", theme::title()));

    f.render_widget(Paragraph::new(menu_lines).block(menu_block), area);
}

/// Section rule: `-- Title -------------------`.
fn section<'a>(title: &str, width: usize) -> Line<'a> {
    // -8: "  " indent + 2 rule chars + 2 spaces around the title + 2 right margin
    let rule_w = width.saturating_sub(title.chars().count() + 8);
    Line::from(vec![
        Span::styled(
            format!("  {} ", theme::icon_h_line().repeat(2)),
            theme::border(),
        ),
        Span::styled(title.to_string(), theme::title()),
        Span::styled(
            format!(" {}", theme::icon_h_line().repeat(rule_w.min(80))),
            theme::border(),
        ),
    ])
}

/// One `label   value` row with a fixed label column.
fn info_row<'a>(label: &str, value: String, value_style: ratatui::style::Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {:<9}", label), theme::dim()),
        Span::styled(value, value_style),
    ])
}

/// Render the welcome/info panel (right side when Menu is focused).
///
/// Left-aligned and information-first: the panel used to spend a third of its
/// height on a centred banner and ASCII art, which pushed the numbers a QA
/// actually opens the app for below the fold on short terminals.
pub fn render(f: &mut Frame, app: &App, area: Rect, with_menu_list: bool) {
    let sprint = app.board_sprint_filter.as_deref().unwrap_or("All Sprints");

    // Narrow terminal: no sidebar, so the menu list is rendered here on top of
    // the info panel. MENU_ROWS = 6 items + spacer + Help + Quit + borders.
    const MENU_ROWS: u16 = 12;
    let area = if with_menu_list && area.height > MENU_ROWS + 4 {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(MENU_ROWS), Constraint::Min(1)])
            .split(area);
        render_sidebar(f, app, split[0]);
        split[1]
    } else if with_menu_list {
        // Too short for both: the menu is what this screen is for.
        render_sidebar(f, app, area);
        return;
    } else {
        area
    };

    let inner_w = (area.width as usize).saturating_sub(2);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // content
            Constraint::Length(1), // footer
        ])
        .split(area);

    let mut lines: Vec<Line> = Vec::new();

    // ── Wordmark (the version lives in the sidebar footer) ──
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  GIT-TUI", theme::title())));
    lines.push(Line::from(Span::styled(
        "  QA workspace for GitHub Projects",
        theme::dim(),
    )));
    lines.push(Line::from(""));

    // ── Context ──
    lines.push(info_row(
        "Project",
        format!("{} / #{}", app.config.owner, app.config.project_number),
        theme::normal(),
    ));
    lines.push(info_row("Sprint", sprint.to_string(), theme::highlight()));
    lines.push(info_row(
        "QA",
        format!("@{}", app.current_user),
        theme::normal(),
    ));
    lines.push(Line::from(""));

    // ── Your queue: what this user has to act on ──
    lines.push(section("Your queue", inner_w));
    lines.push(Line::from(""));
    let groups = app.my_tasks_by_status();
    let waiting = app.waiting_for_deploy().len();
    if groups.is_empty() && waiting == 0 {
        lines.push(Line::from(Span::styled(
            "    Nothing assigned to you in this sprint.",
            theme::dim(),
        )));
    } else {
        for (status, items) in &groups {
            lines.push(Line::from(vec![
                Span::styled(format!("    {:<16}", status.label()), theme::dim()),
                Span::styled(format!("{}", items.len()), theme::status_style(status)),
            ]));
        }
        if waiting > 0 {
            lines.push(Line::from(vec![
                Span::styled("    Waiting for deploy", theme::dim()),
                Span::styled(format!("  {}", waiting), theme::dim()),
            ]));
        }
    }
    lines.push(Line::from(""));

    // ── Sprint progress ──
    let filtered_items: Vec<_> = app
        .items
        .iter()
        .filter(|item| {
            if let Some(ref s) = app.board_sprint_filter {
                item.sprint.as_deref() == Some(s.as_str())
            } else {
                true
            }
        })
        .collect();
    let filtered_total = filtered_items.len();
    let count_of = |f: &dyn Fn(&crate::models::item::Item) -> bool| -> usize {
        filtered_items.iter().filter(|i| f(i)).count()
    };
    let done_count = count_of(&|i| i.status == Status::Done);
    let in_qa_count =
        count_of(&|i| matches!(i.status, Status::InQA | Status::InQADev | Status::InUAT));
    let in_progress_count = count_of(&|i| i.status == Status::InProgress);
    let blocked_count = count_of(&|i| i.status == Status::Blocked);
    let done_pct = super::utils::percent(done_count, filtered_total);

    lines.push(section("Sprint progress", inner_w));
    lines.push(Line::from(""));
    let bar_w = inner_w.saturating_sub(12).clamp(10, 40);
    lines.push(Line::from(super::utils::progress_bar_spans(
        bar_w,
        filtered_total,
        &[
            (done_count, theme::success()),
            (in_qa_count, theme::highlight()),
            (in_progress_count, theme::normal()),
        ],
    )));
    lines.push(Line::from(""));
    lines.push(super::utils::progress_bar_legend());
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "  {}",
            super::utils::fit_parts(
                &[
                    format!("Total {}", filtered_total),
                    format!("Done {} ({}%)", done_count, done_pct),
                    format!("In QA {}", in_qa_count),
                    format!("In Progress {}", in_progress_count),
                    format!("Blocked {}", blocked_count),
                ],
                "   ",
                inner_w.saturating_sub(4),
            )
        ),
        theme::normal(),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" GIT TUI ", theme::title()));
    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Hint bar (status_message is shown by the global status bar in main::draw,
    // which overlays this line while a message is fresh)
    let footer = super::utils::footer_line(
        &[
            (theme::icon_up_down(), "Navigate"),
            ("Enter", "Open"),
            ("1-6", "Shortcut"),
            ("r", "Refresh"),
            ("?", "Help"),
            ("q", "Quit"),
        ],
        chunks[1].width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
