use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// One "key → description" row.
fn key_line(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<14}", key), theme::highlight()),
        Span::styled(desc.to_string(), theme::normal()),
    ])
}

/// Section title row.
fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {}", title), theme::title()))
}

/// Render help as a full content panel with scroll support.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // content
            Constraint::Length(1), // footer
        ])
        .split(area);

    let arrows_lr = format!("{}/{}", theme::icon_left_arrow(), theme::icon_right_arrow());

    let lines = vec![
        Line::from(""),
        section("Navigation"),
        key_line("Esc", "Back / Close"),
        key_line("Enter", "Open / Select"),
        key_line(theme::icon_up_down(), "Navigate up / down"),
        key_line(&arrows_lr, "Scroll columns / cycle options"),
        Line::from(""),
        section("Confirm popups"),
        key_line("Enter / y", "Confirm the action"),
        key_line("Esc / n", "Cancel (other keys are ignored)"),
        Line::from(""),
        section("Menu"),
        key_line("1-6", "Quick jump to screen"),
        key_line("j / k", "Navigate"),
        key_line("r", "Refresh data"),
        key_line("q", "Quit application"),
        Line::from(""),
        section("Board"),
        key_line(&arrows_lr, "Scroll columns"),
        key_line("Space", "Select for batch"),
        key_line("m", "Move item(s)"),
        key_line("s", "Cycle sprint filter"),
        key_line("t", "Create task (parent = selected)"),
        key_line("b", "Create bug report (standalone)"),
        key_line("e", "Create enhancement request (standalone)"),
        key_line("f", "Fail selected item (bug report)"),
        key_line("p", "Open QA Actions menu"),
        key_line("r", "Refresh data"),
        Line::from(""),
        section("Move dialog"),
        key_line(theme::icon_up_down(), "Pick target column"),
        key_line("Enter", "Move to the highlighted column"),
        key_line("1-0 / u", "Jump straight to a column (u = In UAT)"),
        key_line("Esc", "Cancel"),
        Line::from(""),
        section("My Tasks"),
        key_line("p", "Open QA Actions menu"),
        key_line("f", "Fail (create bug report)"),
        key_line("x", "Clear Ready-for-* handoff labels"),
        key_line("m", "Move item"),
        key_line("s", "Cycle sprint filter"),
        key_line("r", "Refresh data"),
        Line::from(""),
        section("Item Detail"),
        key_line("p", "Open QA Actions menu"),
        key_line("P (Shift+p)", "Pass STG (add Ready-for-UAT label, no move)"),
        key_line(
            "U (Shift+u)",
            "Pass UAT: regression clear for EVERY In UAT ticket",
        ),
        key_line("f", "Fail (create bug)"),
        key_line("c", "Comment"),
        key_line("x", "Clear Ready-for-* handoff labels"),
        key_line("t", "Create task (child of this item)"),
        key_line("b", "Create bug report"),
        key_line(
            "e",
            "Enhancement request (labels Enhancement + Task, issue type Task)",
        ),
        key_line("h", "View history"),
        key_line("m", "Move item"),
        key_line("o", "Open parent issue (sub-issues)"),
        key_line("y", "Copy detail as Markdown"),
        key_line("w", "Open in browser"),
        key_line("r / R", "Reload detail"),
        Line::from(""),
        section("QA Actions menu (p)"),
        key_line("p", "Pass Dev (add Ready-for-Staging label)"),
        key_line("P", "Pass STG (add Ready-for-UAT label, no move)"),
        key_line("U", "Pass UAT: passes the whole In UAT batch (regression)"),
        key_line("r", "Return (not fixed, back to In Progress)"),
        key_line("f", "Fail (create bug, only from a QA column)"),
        key_line("x", "Clear Ready-for-* handoff labels"),
        key_line("b / e", "File a bug / an enhancement (parent untouched)"),
        Line::from(vec![
            Span::styled("  Note: ", theme::dim()),
            Span::styled(
                "the menu dims actions that do not apply to the current column",
                theme::dim(),
            ),
        ]),
        Line::from(""),
        section("Search & Filter"),
        key_line("Tab / S-Tab", "Switch filter field"),
        key_line(&arrows_lr, "Navigate options"),
        key_line("Space", "Toggle filter (include → exclude → off)"),
        key_line("Ctrl+L", "Clear all filters"),
        key_line("Esc", "Back (filters kept)"),
        Line::from(""),
        section("Bug / Enhancement / Task forms"),
        key_line("Tab / S-Tab", "Next / previous field"),
        key_line(
            theme::icon_up_down(),
            "Line up / down (text) - at field edge: move field",
        ),
        key_line(&arrows_lr, "Move cursor (text) / cycle option (selectors)"),
        key_line("Home / End", "Start / end of line"),
        key_line("Del", "Delete char under cursor"),
        key_line("Enter", "New line - on Assignees: open picker"),
        key_line("Ctrl+V", "Paste from clipboard"),
        key_line("Ctrl+S", "Submit"),
        key_line("Esc", "Back (draft is kept)"),
        Line::from(""),
        section("Comment"),
        key_line("Ctrl+S", "Post comment"),
        key_line("Ctrl+V", "Paste from clipboard"),
        key_line(&arrows_lr, "Move cursor"),
        key_line("Home / End", "Start / end of line"),
        key_line("Enter", "New line"),
        key_line("Esc", "Back (press twice to discard text)"),
        Line::from(""),
        section("Dashboard"),
        key_line("s", "Cycle sprint filter"),
        key_line("d / D", "Copy standup report (MD / Text)"),
        key_line("e / E", "Copy sprint report + QA metrics (MD / Text)"),
        key_line("S", "Send standup to Slack (asks first)"),
        key_line("X", "Send sprint report to Slack (asks first)"),
        Line::from(""),
        section("Settings"),
        key_line("s", "Edit config (Setup wizard)"),
        key_line("r", "Re-authenticate"),
        key_line("c", "Clear cache (asks first)"),
        key_line("i", "Slack setup (webhook + bot token)"),
        key_line("n", "Slack notification toggles"),
        key_line("u", "Map GitHub → Slack users"),
        key_line("l", "Edit stack leads"),
        Line::from(""),
        section("Notifications"),
        key_line("Enter", "Open (in app; browser if not on board)"),
        key_line("r", "Refresh notifications"),
        key_line("m", "Mark selected as read"),
        key_line("a", "Mark all as read (asks first)"),
        Line::from(""),
        section("Stack Leads"),
        key_line("Enter", "Edit leads of selected stack"),
        key_line("a", "Add stack"),
        key_line("d", "Delete stack (asks first)"),
        key_line("Esc", "Save & back"),
        Line::from(""),
        section("General"),
        key_line("?", "Open this help"),
        key_line("Mouse wheel", "Scroll lists / board / detail"),
        key_line("Ctrl+C", "Force quit"),
        Line::from(vec![
            Span::styled("  Env: ", theme::dim()),
            Span::styled(
                "GIT_TUI_UNICODE=1|0 forces Unicode icons on/off",
                theme::dim(),
            ),
        ]),
        Line::from(""),
    ];

    // Apply scroll
    let visible: Vec<Line> = lines.into_iter().skip(app.help_scroll).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::title())
        .title(Span::styled(" Keyboard Shortcuts ", theme::title()));

    f.render_widget(Paragraph::new(visible).block(block), chunks[0]);

    // Footer
    let footer = super::utils::footer_line(
        &[(theme::icon_up_down(), "Scroll"), ("Esc", "Back")],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}
