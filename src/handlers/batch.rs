use crossterm::event::KeyCode;

use crate::app::{App, BatchKind, ConfirmAction, Popup};

/// Keys in the batch pick list (`Screen::BatchPicker`).
pub fn handle_batch_picker(app: &mut App, key: KeyCode, kind: BatchKind) {
    let len = app.batch_rows.len();
    match key {
        KeyCode::Up | KeyCode::Char('k') => {
            app.batch_cursor = app.batch_cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.batch_cursor + 1 < len {
                app.batch_cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            if let Some(row) = app.batch_rows.get_mut(app.batch_cursor) {
                row.1 = !row.1;
            }
        }
        // All on, or all off when every row is already on.
        KeyCode::Char('a') => {
            let all = app.batch_rows.iter().all(|(_, c)| *c);
            for row in &mut app.batch_rows {
                row.1 = !all;
            }
        }
        KeyCode::Enter => confirm_batch(app, kind),
        // Deploy-check every row at once, instead of opening each ticket.
        KeyCode::Char('d') => {
            let rows: Vec<usize> = app
                .batch_rows
                .iter()
                .filter_map(|(id, _)| app.items.iter().position(|i| &i.id == id))
                .collect();
            crate::loader::spawn_deploy_check_rows(app, &rows);
        }
        KeyCode::Char('?') => app.goto(crate::app::Screen::Help),
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

/// Ask before running the batch on the checked rows.
fn confirm_batch(app: &mut App, kind: BatchKind) {
    let indices = app.batch_checked_indices();
    if indices.is_empty() {
        app.set_status("Nothing checked - Space checks a ticket, a checks all");
        return;
    }
    let nums: Vec<String> = indices
        .iter()
        .filter_map(|i| app.items.get(*i))
        .map(|it| {
            it.number
                .map(|n| format!("#{}", n))
                .unwrap_or_else(|| "draft".to_string())
        })
        .collect();
    let preview = if nums.len() <= 10 {
        nums.join(", ")
    } else {
        format!("{}, +{} more", nums[..10].join(", "), nums.len() - 10)
    };
    let (message, on_confirm) = match kind {
        BatchKind::VerifyUat => (
            format!(
                "Regression clear on UAT.\nMove {} ticket(s) to Tech Complete and comment on each:\n{}\nOne 'Ready for release' summary goes to Slack.",
                indices.len(),
                preview
            ),
            ConfirmAction::PassUat {
                indices: indices.clone(),
                bulk: true,
            },
        ),
        BatchKind::MoveToUat => (
            format!(
                "Code is on UAT.\nMove {} ticket(s) to In UAT, remove Ready-for-UAT and comment on each:\n{}\nOne Slack message lists them.",
                indices.len(),
                preview
            ),
            ConfirmAction::MoveToUat {
                indices: indices.clone(),
            },
        ),
    };
    app.popup = Popup::Confirm {
        title: format!("{} - {} ticket(s)", kind.title(), indices.len()),
        message,
        on_confirm,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::item::Item;

    fn app_with_rows() -> App {
        let mut app = App::new();
        app.items = (0..3)
            .map(|i| Item {
                id: format!("id{}", i),
                number: Some(100 + i),
                ..Default::default()
            })
            .collect();
        app.batch_rows = vec![
            ("id0".into(), true),
            ("id1".into(), false),
            ("id2".into(), true),
        ];
        app
    }

    #[test]
    fn test_space_toggles_and_a_toggles_all() {
        let mut app = app_with_rows();
        handle_batch_picker(&mut app, KeyCode::Down, BatchKind::VerifyUat);
        handle_batch_picker(&mut app, KeyCode::Char(' '), BatchKind::VerifyUat);
        assert_eq!(app.batch_checked_indices(), vec![0, 1, 2]);

        // All on: `a` turns everything off, then on again.
        handle_batch_picker(&mut app, KeyCode::Char('a'), BatchKind::VerifyUat);
        assert!(app.batch_checked_indices().is_empty());
        handle_batch_picker(&mut app, KeyCode::Char('a'), BatchKind::VerifyUat);
        assert_eq!(app.batch_checked_indices(), vec![0, 1, 2]);
    }

    #[test]
    fn test_enter_with_nothing_checked_does_nothing() {
        let mut app = app_with_rows();
        for row in &mut app.batch_rows {
            row.1 = false;
        }
        handle_batch_picker(&mut app, KeyCode::Enter, BatchKind::VerifyUat);
        assert_eq!(app.popup, Popup::None);
    }

    #[test]
    fn test_enter_confirms_the_checked_rows_by_id() {
        let mut app = app_with_rows();
        // A refresh reordered the board: rows follow the ids, not positions.
        app.items.swap(0, 2);
        handle_batch_picker(&mut app, KeyCode::Enter, BatchKind::VerifyUat);
        match &app.popup {
            Popup::Confirm {
                on_confirm: ConfirmAction::PassUat { indices, bulk },
                ..
            } => {
                assert!(*bulk);
                assert_eq!(indices, &vec![2, 0]);
            }
            other => panic!("expected a Pass UAT confirm, got {:?}", other),
        }
    }
}
