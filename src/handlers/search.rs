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
        // Letter keys type into the keyword box, so saving, opening and
        // copying take Ctrl.
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.popup = crate::app::Popup::SavePreset {
                name: String::new(),
            };
        }
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            if app.config.search_presets.is_empty() {
                app.set_status("No saved filters yet - Ctrl+S saves the current ones");
            } else {
                app.popup = crate::app::Popup::Presets { selected: 0 };
            }
        }
        KeyCode::Char('y') if modifiers.contains(KeyModifiers::CONTROL) => {
            if app.search_results.is_empty() {
                app.set_status("No results to copy");
            } else {
                app.popup = crate::app::Popup::CopyResults;
            }
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
                    // In place: a value the board no longer offers is listed
                    // in this order, so moving it would move the cursor too.
                    if let Some(slot) = filter.iter_mut().find(|x| **x == opt) {
                        *slot = not_opt;
                    }
                } else if is_exclude {
                    filter.retain(|x| x != &not_opt);
                } else {
                    filter.push(opt);
                }
                app.apply_filters();
                // Switching off a value the board no longer offers removes it
                // from the row: keep the cursor on an option.
                let len = get_filter_options_len(app, app.search_field_idx);
                if len > 0 && app.filter_cursor[ci] >= len {
                    app.filter_cursor[ci] = len - 1;
                }
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

/// Copy format for the Search results.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CopyFormat {
    /// For GitHub and Notion.
    Markdown,
    /// Rich text with linked titles, for pasting into Slack.
    Slack,
}

/// Copy every current Search result, in screen order, and say how it went.
pub fn copy_results(app: &mut App, format: CopyFormat) {
    let n = app.search_results.len();
    let result = match format {
        CopyFormat::Markdown => crate::export::copy_to_clipboard(&crate::export::ticket_list_md(
            &app.items,
            &app.search_results,
        )),
        CopyFormat::Slack => {
            let (html, plain) = crate::export::ticket_list_rich(
                &app.items,
                &app.search_results,
                &app.config.slack_user_map,
            );
            crate::export::copy_html_to_clipboard(&html, &plain)
        }
    };
    app.popup = match result {
        Ok(()) => crate::app::Popup::Success(format!(
            "Copied {} result(s) as {}",
            n,
            match format {
                CopyFormat::Markdown => "Markdown",
                CopyFormat::Slack => "a Slack list (links kept when pasted)",
            }
        )),
        Err(e) => crate::app::Popup::Error(e),
    };
}

/// Keys while naming the current filters (`Popup::SavePreset`).
pub fn handle_save_preset(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    let crate::app::Popup::SavePreset { name } = &mut app.popup else {
        return;
    };
    match key {
        KeyCode::Esc => app.popup = crate::app::Popup::None,
        KeyCode::Backspace => {
            name.pop();
        }
        KeyCode::Enter => {
            let name = name.trim().to_string();
            if name.is_empty() {
                return;
            }
            let replaced = app.config.search_presets.iter().any(|p| p.name == name);
            app.upsert_search_preset(&name);
            app.popup = match app.config.save() {
                Ok(()) => crate::app::Popup::Success(format!(
                    "{} filter '{}'. Ctrl+O opens the saved filters.",
                    if replaced { "Updated" } else { "Saved" },
                    name
                )),
                Err(e) => crate::app::Popup::Error(format!("Failed to save config: {}", e)),
            };
        }
        KeyCode::Char(c)
            if !modifiers.contains(KeyModifiers::CONTROL) && name.chars().count() < 40 =>
        {
            name.push(c);
        }
        _ => {}
    }
}

/// Keys in the saved-filters list (`Popup::Presets`).
pub fn handle_presets(app: &mut App, key: KeyCode) {
    let crate::app::Popup::Presets { selected } = app.popup else {
        return;
    };
    let count = app.config.search_presets.len();
    if count == 0 {
        app.popup = crate::app::Popup::None;
        return;
    }
    let selected = selected.min(count - 1);
    match key {
        KeyCode::Esc => app.popup = crate::app::Popup::None,
        KeyCode::Up => {
            app.popup = crate::app::Popup::Presets {
                selected: selected.saturating_sub(1),
            }
        }
        KeyCode::Down => {
            app.popup = crate::app::Popup::Presets {
                selected: (selected + 1).min(count - 1),
            }
        }
        KeyCode::Enter => {
            let preset = app.config.search_presets[selected].clone();
            let dropped = app.apply_search_preset(&preset);
            app.popup = crate::app::Popup::None;
            if dropped.is_empty() {
                app.set_status(&format!(
                    "Loaded '{}': {} result(s)",
                    preset.name,
                    app.search_results.len()
                ));
            } else {
                app.set_status(&format!(
                    "Loaded '{}' without {} (no such sprint now)",
                    preset.name,
                    dropped.join(", ")
                ));
            }
        }
        KeyCode::Char('d') => {
            let name = app.config.search_presets[selected].name.clone();
            app.popup = crate::app::Popup::Confirm {
                title: "Delete saved filter".to_string(),
                message: format!("Delete the saved filter '{}'?", name),
                on_confirm: crate::app::ConfirmAction::DeleteSearchPreset(name),
            };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_excluding_a_value_keeps_its_place_under_the_cursor() {
        let mut app = App::new();
        // Two epics with no open work any more, from a saved filter: listed
        // only because they are selected, in the filter's order.
        *app.search_filter_mut(6) = vec!["acme/b#10".to_string(), "acme/b#20".to_string()];
        app.search_field_idx = 6;
        let at = |app: &App| get_filter_option_at(app, 6, app.filter_cursor[5]);
        app.filter_cursor[5] = app
            .search_filter_options(6)
            .iter()
            .position(|(v, _)| v == "acme/b#10")
            .expect("listed");

        handle_search(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.search_filter(6), &vec!["!acme/b#10", "acme/b#20"]);
        assert_eq!(
            at(&app).as_deref(),
            Some("acme/b#10"),
            "cursor stays on #10"
        );

        // The next Space switches #10 off, not #20.
        handle_search(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.search_filter(6), &vec!["acme/b#20"]);
    }
}
