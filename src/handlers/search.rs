use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::App;

/// Tab stops: the keyword box plus one per filter row.
const FIELDS: usize = App::SEARCH_FILTER_ROWS + 1;

pub fn handle_search(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    match key {
        // Ctrl+L clears all filters (Esc only goes back - the two used to be
        // fused, which destroyed carefully-built filters on exit).
        KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
            clear_all_filters(app);
            app.set_status("Filters cleared");
        }
        KeyCode::Tab => {
            app.search_field_idx = (app.search_field_idx + 1) % FIELDS;
        }
        KeyCode::BackTab => {
            app.search_field_idx = if app.search_field_idx == 0 {
                FIELDS - 1
            } else {
                app.search_field_idx - 1
            };
        }
        KeyCode::Right | KeyCode::Left => {
            if app.search_field_idx == 0 {
                return;
            }
            let forward = key == KeyCode::Right;
            let options_len = get_filter_options_len(app, app.search_field_idx);
            if options_len == 0 {
                return;
            }
            let ci = app.search_field_idx - 1;
            let total = options_len;
            if forward {
                app.filter_cursor[ci] = (app.filter_cursor[ci] + 1) % total;
            } else {
                app.filter_cursor[ci] = if app.filter_cursor[ci] == 0 {
                    total - 1
                } else {
                    app.filter_cursor[ci] - 1
                };
            }
        }
        KeyCode::Char(' ') => {
            if app.search_field_idx == 0 {
                app.search_query.push(' ');
                app.apply_filters();
                return;
            }
            let ci = app.search_field_idx - 1;
            let cursor_pos = app.filter_cursor[ci];
            if let Some(opt) = get_filter_option_at(app, app.search_field_idx, cursor_pos) {
                let not_opt = format!("!{}", opt);
                let filter = app.search_filter_mut(app.search_field_idx);

                let is_include = filter.contains(&opt);
                let is_exclude = filter.contains(&not_opt);

                if is_include {
                    filter.retain(|x| x != &opt);
                    filter.push(not_opt);
                } else if is_exclude {
                    filter.retain(|x| x != &not_opt);
                } else {
                    filter.push(opt);
                }
                app.apply_filters();
            }
        }
        KeyCode::Char(c) => {
            // Note: no `?`-to-Help here on purpose - it must be typable in queries.
            if app.search_field_idx == 0 {
                app.search_query.push(c);
                app.apply_filters();
            }
        }
        KeyCode::Backspace => {
            if app.search_field_idx == 0 {
                app.search_query.pop();
                app.apply_filters();
            }
        }
        KeyCode::Up => {
            if app.list_selected > 0 {
                app.list_selected -= 1;
                if app.list_selected < app.list_scroll {
                    app.list_scroll = app.list_selected;
                }
            }
        }
        KeyCode::Down => {
            if app.list_selected + 1 < app.search_results.len() {
                app.list_selected += 1;
                if app.list_selected > app.list_scroll + 5 {
                    app.list_scroll = app.list_selected.saturating_sub(3);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(&item_idx) = app.search_results.get(app.list_selected) {
                app.detail_scroll = 0;
                app.goto(crate::app::Screen::ItemDetail(item_idx));
                crate::loader::fetch_item_detail(app, item_idx);
            }
        }
        KeyCode::Esc => {
            // Filters are kept - coming back to Search resumes where you left
            // off. Ctrl+L clears them explicitly.
            app.go_back();
        }
        _ => {}
    }
}

/// Reset the search query and every filter field.
fn clear_all_filters(app: &mut App) {
    app.search_query.clear();
    for field in 1..FIELDS {
        app.search_filter_mut(field).clear();
    }
    app.filter_cursor = vec![0; App::SEARCH_FILTER_ROWS];
    app.search_field_idx = 0;
    app.apply_filters();
}

/// Get the number of options for a filter field.
pub fn get_filter_options_len(app: &App, field_idx: usize) -> usize {
    app.search_filter_options(field_idx).len()
}

/// Get the filter value of the option at the cursor.
pub fn get_filter_option_at(app: &App, field_idx: usize, cursor_pos: usize) -> Option<String> {
    app.search_filter_options(field_idx)
        .into_iter()
        .nth(cursor_pos)
        .map(|(value, _)| value)
}
