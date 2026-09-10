use crossterm::event::KeyCode;

use crate::app::{App, Screen};
use crate::loader;
use crate::models::status::Status;

pub fn handle_board(app: &mut App, key: KeyCode) {
    let columns = Status::all_columns();
    match key {
        // Column offset is auto-corrected by the renderer from the real
        // visible-column count - no width assumptions here.
        KeyCode::Left => {
            if app.board_selected_col > 0 {
                app.board_selected_col -= 1;
                app.board_selected_row = 0;
            }
        }
        KeyCode::Right => {
            if app.board_selected_col < columns.len() - 1 {
                app.board_selected_col += 1;
                app.board_selected_row = 0;
            }
        }
        KeyCode::Up => {
            if app.board_selected_row > 0 {
                app.board_selected_row -= 1;
                let col = app.board_selected_col;
                while app.board_row_offset.len() <= col {
                    app.board_row_offset.push(0);
                }
                if app.board_selected_row < app.board_row_offset[col] {
                    app.board_row_offset[col] = app.board_selected_row;
                }
            }
        }
        KeyCode::Down => {
            let status = &columns[app.board_selected_col];
            let count = app.items_in_column(status).len();
            if app.board_selected_row + 1 < count {
                app.board_selected_row += 1;
                let col = app.board_selected_col;
                while app.board_row_offset.len() <= col {
                    app.board_row_offset.push(0);
                }
                // Height-aware: scroll when the selection walks past the cards
                // that actually fit (measured by the last render).
                let visible = app.board_visible_cards.max(1);
                if app.board_selected_row >= app.board_row_offset[col] + visible {
                    app.board_row_offset[col] = app.board_selected_row + 1 - visible;
                }
            }
        }
        KeyCode::Enter => {
            let status = &columns[app.board_selected_col];
            let items = app.items_in_column(status);
            if let Some(&(item_idx, _)) = items.get(app.board_selected_row) {
                app.detail_scroll = 0;
                app.goto(Screen::ItemDetail(item_idx));
                loader::fetch_item_detail(app, item_idx);
            }
        }
        KeyCode::Char(' ') => {
            let status = &columns[app.board_selected_col];
            let items = app.items_in_column(status);
            if let Some(&(item_idx, _)) = items.get(app.board_selected_row) {
                app.items[item_idx].selected = !app.items[item_idx].selected;
            }
        }
        KeyCode::Char('m') => {
            let selected = app.selected_items();
            if !selected.is_empty() {
                let indices: Vec<usize> = selected.iter().map(|(i, _)| *i).collect();
                app.goto(Screen::MoveDialog(indices));
            } else {
                let status = &columns[app.board_selected_col];
                let items = app.items_in_column(status);
                if let Some(&(item_idx, _)) = items.get(app.board_selected_row) {
                    app.goto(Screen::MoveDialog(vec![item_idx]));
                }
            }
        }
        KeyCode::Char('r') => {
            loader::try_refresh(app, false);
        }
        KeyCode::Char('s') => {
            app.cycle_sprint_filter();
            app.board_selected_row = 0;
            app.board_row_offset = vec![0; crate::models::status::Status::all_columns().len()];
        }
        KeyCode::Char('t') => {
            let status = &columns[app.board_selected_col];
            let items = app.items_in_column(status);
            if let Some(&(item_idx, _)) = items.get(app.board_selected_row) {
                app.goto(Screen::TaskForm(Some(item_idx)));
                return;
            }
            app.goto(Screen::TaskForm(None));
        }
        KeyCode::Char('p') => {
            // Same entry point as My Tasks, so every QA action (including
            // clearing handoff labels) is reachable from the board too.
            let status = &columns[app.board_selected_col];
            let items = app.items_in_column(status);
            if let Some(&(item_idx, _)) = items.get(app.board_selected_row) {
                app.goto(Screen::QaActions(item_idx));
            }
        }
        KeyCode::Char('f') => {
            let status = &columns[app.board_selected_col];
            let items = app.items_in_column(status);
            if let Some(&(item_idx, _)) = items.get(app.board_selected_row) {
                crate::handlers::detail::start_fail_flow(app, item_idx);
            }
        }
        // Standalone (no parent), as `b` has always been from the board.
        // `e` mirrors it exactly, only the label and title prefix differ.
        KeyCode::Char('b') => {
            app.open_report_form(None, false, crate::app::ReportKind::Bug);
        }
        KeyCode::Char('e') => {
            app.open_report_form(None, false, crate::app::ReportKind::Enhancement);
        }
        KeyCode::Char('?') => app.goto(Screen::Help),
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

pub fn handle_my_tasks(app: &mut App, key: KeyCode) {
    // Clamp list_selected if items were removed (e.g., after QA action changed status)
    let task_count = app.my_tasks_grouped().len();
    if task_count > 0 && app.list_selected >= task_count {
        app.list_selected = task_count - 1;
    }

    let grouped_items = app.my_tasks_grouped();
    match key {
        KeyCode::Up => {
            if app.list_selected > 0 {
                app.list_selected -= 1;
                let line_pos = compute_my_task_line_with_header(app, app.list_selected);
                if line_pos < app.list_scroll {
                    app.list_scroll = line_pos;
                }
            }
        }
        KeyCode::Down => {
            if app.list_selected + 1 < grouped_items.len() {
                app.list_selected += 1;
                let line_pos = compute_my_task_line(app, app.list_selected);
                // Height-aware: use the viewport measured by the last render
                // instead of a hardcoded 15-line assumption.
                let view = app.list_view_height.max(4);
                if line_pos >= app.list_scroll + view {
                    app.list_scroll = line_pos.saturating_sub(view / 2);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(&(item_idx, _)) = grouped_items.get(app.list_selected) {
                app.detail_scroll = 0;
                app.goto(Screen::ItemDetail(item_idx));
                loader::fetch_item_detail(app, item_idx);
            }
        }
        KeyCode::Char('p') => {
            if let Some(&(item_idx, _)) = grouped_items.get(app.list_selected) {
                app.goto(Screen::QaActions(item_idx));
            }
        }
        KeyCode::Char('f') => {
            if let Some(&(item_idx, _)) = grouped_items.get(app.list_selected) {
                crate::handlers::detail::start_fail_flow(app, item_idx);
            }
        }
        KeyCode::Char('m') => {
            if let Some(&(item_idx, _)) = grouped_items.get(app.list_selected) {
                app.goto(Screen::MoveDialog(vec![item_idx]));
            }
        }
        KeyCode::Char('r') => {
            loader::try_refresh(app, false);
        }
        KeyCode::Char('s') => {
            app.cycle_sprint_filter();
            app.list_selected = 0;
            app.list_scroll = 0;
        }
        KeyCode::Char('?') => app.goto(Screen::Help),
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

/// Compute the actual line position of item at `target_idx` in My Tasks view,
/// accounting for group headers and spacers.
pub fn compute_my_task_line(app: &App, target_idx: usize) -> usize {
    compute_my_task_line_inner(app, target_idx).0
}

/// Like compute_my_task_line but returns the group header line if the item
/// is the first in its status group, so scrolling up doesn't clip the header.
pub fn compute_my_task_line_with_header(app: &App, target_idx: usize) -> usize {
    let (item_line, header_line) = compute_my_task_line_inner(app, target_idx);
    header_line.unwrap_or(item_line)
}

/// Returns (item_line, Option<header_line>).
/// header_line is Some if the item is the first in its status group.
fn compute_my_task_line_inner(app: &App, target_idx: usize) -> (usize, Option<usize>) {
    let groups = app.my_tasks_by_status();

    let mut line = 0;
    let mut global_idx = 0;

    for (_status, items_in_status) in &groups {
        if items_in_status.is_empty() {
            continue;
        }

        let header_line = line;
        line += 1; // group header

        for (i, (_, item)) in items_in_status.iter().enumerate() {
            if global_idx == target_idx {
                let hdr = if i == 0 { Some(header_line) } else { None };
                return (line, hdr);
            }

            line += 1; // main item line

            let has_stack = item.stack.as_deref().is_some_and(|s| !s.is_empty());
            let has_details =
                !item.labels.is_empty() || has_stack || item.has_prs() || item.sprint.is_some();

            if has_details {
                line += 1; // detail line
            }

            global_idx += 1;
        }

        line += 1; // spacer
    }

    (line, None)
}
