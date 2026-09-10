use crossterm::event::KeyCode;

use crate::app::{App, ConfirmAction, Popup, ReportKind, Screen};
use crate::loader;
use crate::models::status::Status;

/// Which QA pass action a guard is checked for.
enum PassKind {
    Dev,
    Stg,
    Uat,
    Return,
    Fail,
}

/// Guard: QA pass actions only make sense from specific columns, and passing
/// twice would duplicate the comment + Slack ping. Returns Err with an
/// explanation to show the user when the action doesn't apply.
fn qa_pass_allowed(app: &App, item_idx: usize, kind: PassKind) -> Result<(), String> {
    let Some(item) = app.items.get(item_idx) else {
        return Err("Item not found".to_string());
    };
    let status_label = item.status.label();
    match kind {
        PassKind::Dev => {
            if item.status != Status::InQADev {
                Err(format!(
                    "Pass Dev applies to items in 'In QA - Dev' - this one is '{}'.",
                    status_label
                ))
            } else if app.item_has_label(item_idx, "Ready-for-Staging") {
                Err("Already passed on Dev: this item has the Ready-for-Staging label.".to_string())
            } else {
                Ok(())
            }
        }
        PassKind::Stg => {
            if item.status != Status::InQA {
                Err(format!(
                    "Pass STG applies to items in 'In QA' - this one is '{}'.",
                    status_label
                ))
            } else if app.item_has_label(item_idx, "Ready-for-UAT") {
                Err("Already passed on STG: this item has the Ready-for-UAT label.".to_string())
            } else {
                Ok(())
            }
        }
        PassKind::Uat => {
            if item.status != Status::InUAT {
                Err(format!(
                    "Pass UAT applies to items in 'In UAT' - this one is '{}'.",
                    status_label
                ))
            } else {
                Ok(())
            }
        }
        PassKind::Return => {
            if item.status == Status::InProgress {
                Err("This item is already In Progress.".to_string())
            } else {
                Ok(())
            }
        }
        PassKind::Fail => {
            // Fail files a bug AND drags the parent back to In Progress, so it
            // only makes sense from a QA column. Filing a bug against any other
            // ticket is what [b] is for.
            if !matches!(item.status, Status::InQADev | Status::InQA | Status::InUAT) {
                Err(format!(
                    "Fail applies to items in a QA column (In QA - Dev, In QA, In UAT). This one is '{}'.\nUse [b] to file a bug without moving the ticket.",
                    status_label
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Guarded confirm popup for a QA pass action.
fn guarded_confirm(
    app: &mut App,
    item_idx: usize,
    kind: PassKind,
    title: &str,
    message: &str,
    on_confirm: ConfirmAction,
) {
    match qa_pass_allowed(app, item_idx, kind) {
        Ok(()) => {
            let num = app
                .items
                .get(item_idx)
                .and_then(|i| i.number)
                .map(|n| format!("#{}", n))
                .unwrap_or_default();
            app.popup = Popup::Confirm {
                title: format!("{} - {}", title, num),
                message: message.to_string(),
                on_confirm,
            };
        }
        Err(msg) => {
            app.popup = Popup::Error(msg);
        }
    }
}

/// The regression batch: every ticket sitting In UAT in the active sprint.
///
/// A UAT run verifies everything deployed there at once, so passing it one
/// ticket at a time meant a keypress and a Slack ping per ticket for a single
/// regression result. The triggering ticket is always included, even when the
/// sprint filter would otherwise hide it.
fn uat_batch(app: &App, item_idx: usize) -> Vec<usize> {
    let mut indices: Vec<usize> = app
        .items_in_column(&Status::InUAT)
        .into_iter()
        .map(|(i, _)| i)
        .collect();
    if !indices.contains(&item_idx) {
        indices.push(item_idx);
    }
    // Stable, readable order for the confirm list and the Slack summary.
    indices.sort_by_key(|i| {
        app.items
            .get(*i)
            .and_then(|it| it.number)
            .unwrap_or(u32::MAX)
    });
    indices
}

/// Open the Fail (new bug) form for an item, guarded by column.
///
/// Public because the Board and My Tasks screens have their own `f` binding;
/// they used to jump straight into the form and so skipped the guard, meaning
/// `f` on a Done ticket would drag it back to In Progress.
pub fn start_fail_flow(app: &mut App, item_idx: usize) {
    match qa_pass_allowed(app, item_idx, PassKind::Fail) {
        // A QA failure always files a Bug, never an Enhancement.
        Ok(()) => app.open_report_form(Some(item_idx), true, ReportKind::Bug),
        Err(msg) => app.popup = Popup::Error(msg),
    }
}

/// Shared helper: show confirmation popup for QA actions (PassDev, PassStg, PassUat, Return).
fn confirm_qa_action(app: &mut App, key: KeyCode, item_idx: usize) -> bool {
    match key {
        // On ItemDetail, `p` opens the QA Actions menu instead (see handle_detail);
        // here it fires from the QA Actions popup.
        KeyCode::Char('p') => {
            guarded_confirm(
                app,
                item_idx,
                PassKind::Dev,
                "Pass Dev",
                "Add Ready-for-Staging label + comment tagging the lead?",
                ConfirmAction::PassDev(item_idx),
            );
            true
        }
        KeyCode::Char('P') => {
            guarded_confirm(
                app,
                item_idx,
                PassKind::Stg,
                "Pass STG",
                "Add Ready-for-UAT label + comment tagging the lead?\nNo status move - devs move it to In UAT after the deploy.",
                ConfirmAction::PassStg(item_idx),
            );
            true
        }
        KeyCode::Char('U') => {
            match qa_pass_allowed(app, item_idx, PassKind::Uat) {
                Ok(()) => {
                    let indices = uat_batch(app, item_idx);
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
                    app.popup = Popup::Confirm {
                        title: format!("Pass UAT - {} ticket(s)", indices.len()),
                        message: format!(
                            "Regression clear on UAT.\nMove {} ticket(s) to Tech Complete and comment on each:\n{}\nOne 'Ready for release' summary goes to Slack.",
                            indices.len(), preview
                        ),
                        on_confirm: ConfirmAction::PassUat { indices },
                    };
                }
                Err(msg) => app.popup = Popup::Error(msg),
            }
            true
        }
        // Return lives only in the QA Actions menu - on ItemDetail `r` is
        // reserved for reload so refresh muscle-memory can't demote a ticket.
        KeyCode::Char('r') if !matches!(app.screen, Screen::ItemDetail(_)) => {
            guarded_confirm(
                app,
                item_idx,
                PassKind::Return,
                "Return",
                "Move back to In Progress + comment tagging the devs?\nBoth Ready-for-* labels are cleared, so the ticket needs a full re-verify.",
                ConfirmAction::Return(item_idx),
            );
            true
        }
        KeyCode::Char('f') => {
            start_fail_flow(app, item_idx);
            true
        }
        // Escape hatch for the one case the app can't see: a dev moved the
        // ticket back outside the TUI, so its Ready-for-* label is stale and
        // both My Tasks and the pass guard still treat it as handed over.
        KeyCode::Char('x') => {
            let labels = app
                .items
                .get(item_idx)
                .map(App::handoff_labels_on)
                .unwrap_or_default();
            if labels.is_empty() {
                app.popup =
                    Popup::Error("No handoff label on this item: nothing to clear.".to_string());
            } else {
                let num = app
                    .items
                    .get(item_idx)
                    .and_then(|i| i.number)
                    .map(|n| format!("#{}", n))
                    .unwrap_or_default();
                app.popup = Popup::Confirm {
                    title: format!("Clear handoff labels - {}", num),
                    message: format!(
                        "Remove {} on GitHub?\nThe ticket then counts as testable again.",
                        labels.join(" + ")
                    ),
                    on_confirm: ConfirmAction::ClearHandoffLabels(item_idx),
                };
            }
            true
        }
        KeyCode::Char('t') => {
            app.goto(Screen::TaskForm(Some(item_idx)));
            true
        }
        KeyCode::Char('b') => {
            app.open_report_form(Some(item_idx), false, ReportKind::Bug);
            true
        }
        KeyCode::Char('e') => {
            app.open_report_form(Some(item_idx), false, ReportKind::Enhancement);
            true
        }
        KeyCode::Char('c') => {
            // Keep an unsent draft for the SAME item (failed-post retry);
            // only clear when switching to a different item.
            let item_id = app.items.get(item_idx).map(|i| i.id.clone());
            if app.comment_text.is_empty() || app.comment_draft_for != item_id {
                app.comment_text.clear();
                app.comment_cursor = 0;
            }
            app.comment_draft_for = item_id;
            app.comment_discard_armed = false;
            app.goto(Screen::Comment(item_idx));
            true
        }
        KeyCode::Char('m') => {
            app.goto(Screen::MoveDialog(vec![item_idx]));
            true
        }
        _ => false,
    }
}

pub fn handle_detail(app: &mut App, key: KeyCode, item_idx: usize) {
    // Fetch detail data on first visit
    loader::fetch_item_detail(app, item_idx);

    // `p` opens the QA Actions menu (same as My Tasks) - pass/return execute
    // from there, so a single mistyped letter can't fire a write action.
    if key == KeyCode::Char('p') {
        app.goto(Screen::QaActions(item_idx));
        return;
    }

    // Try shared QA actions first (P/U direct shortcuts stay, but guarded)
    if confirm_qa_action(app, key, item_idx) {
        return;
    }

    match key {
        KeyCode::Up => {
            app.detail_scroll = app.detail_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            app.detail_scroll += 1;
        }
        // `r` = reload everywhere in the app; Return moved into QA Actions (p).
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.detail_scroll = 0;
            loader::force_fetch_item_detail(app, item_idx);
        }
        KeyCode::Char('h') => {
            app.history_events.clear();
            app.goto(Screen::History(item_idx));
            // Auto-fetch history (UI-2: no need to press 'r' manually)
            spawn_history_fetch(app, item_idx);
        }
        // Jump to the sub-issues parent when it's on the board
        KeyCode::Char('o') => {
            let target = app
                .items
                .get(item_idx)
                .and_then(|item| item.parent_number.map(|pn| (pn, item.repository.clone())));
            match target {
                Some((pn, repo)) => {
                    if let Some(pidx) = app
                        .items
                        .iter()
                        .position(|i| i.number == Some(pn) && i.repository == repo)
                    {
                        app.detail_scroll = 0;
                        app.goto(Screen::ItemDetail(pidx));
                        loader::fetch_item_detail(app, pidx);
                    } else {
                        app.set_status(&format!("Parent #{} is not on this board/sprint", pn));
                    }
                }
                None => app.set_status("This item has no parent"),
            }
        }
        KeyCode::Char('y') => {
            if let Some(item) = app.items.get(item_idx) {
                let md = crate::export::item_to_markdown(item, &app.config.owner);
                let num = item.number.map(|n| format!("#{}", n)).unwrap_or_default();
                // Report the real clipboard outcome instead of claiming success.
                let result = arboard::Clipboard::new().and_then(|mut c| c.set_text(md));
                match result {
                    Ok(()) => {
                        app.popup = Popup::Success(format!("Copied {} detail as Markdown!", num));
                    }
                    Err(e) => {
                        app.popup = Popup::Error(format!("Clipboard error: {}", e));
                    }
                }
            }
        }
        KeyCode::Char('w') => {
            if let Some(item) = app.items.get(item_idx) {
                if let Some(number) = item.number {
                    let repo = item.repository.as_deref().map(|r| {
                        if r.contains('/') {
                            r.to_string()
                        } else {
                            format!("{}/{}", app.config.owner, r)
                        }
                    });
                    if let Some(repo) = repo {
                        let url = format!("https://github.com/{}/issues/{}", repo, number);
                        let _ = open::that(&url);
                    }
                }
            }
        }
        KeyCode::Char('?') => app.goto(Screen::Help),
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

pub fn handle_qa_actions(app: &mut App, key: KeyCode, item_idx: usize) {
    // Shared QA actions (p/P/U/r/f/t/b/c/m - `r` = Return here, guarded)
    if confirm_qa_action(app, key, item_idx) {
        return;
    }

    if key == KeyCode::Esc {
        app.go_back();
    }
}

pub fn handle_move_dialog(app: &mut App, key: KeyCode, item_indices: &[usize]) {
    let columns = Status::all_columns();
    // Keys are defined by Status::move_key() so the handler can never drift
    // from what the dialog renders. Arrow keys drive the same list, so the
    // dialog behaves like every other list in the app instead of being a
    // memorise-the-digit puzzle.
    let target = match key {
        KeyCode::Up => {
            app.move_cursor = app.move_cursor.saturating_sub(1);
            return;
        }
        KeyCode::Down => {
            if app.move_cursor + 1 < columns.len() {
                app.move_cursor += 1;
            }
            return;
        }
        KeyCode::Home => {
            app.move_cursor = 0;
            return;
        }
        KeyCode::End => {
            app.move_cursor = columns.len().saturating_sub(1);
            return;
        }
        KeyCode::Enter => columns.get(app.move_cursor).cloned(),
        KeyCode::Char(c) => columns.iter().find(|s| s.move_key() == c).cloned(),
        KeyCode::Esc => {
            app.go_back();
            return;
        }
        _ => None,
    };

    if let Some(target_status) = target {
        let count = item_indices.len();
        let status_label = target_status.label();
        // List the affected issue numbers so a batch move is never a surprise.
        let nums: Vec<String> = item_indices
            .iter()
            .filter_map(|i| app.items.get(*i))
            .map(|item| {
                item.number
                    .map(|n| format!("#{}", n))
                    .unwrap_or_else(|| "draft".to_string())
            })
            .collect();
        let preview = if nums.len() <= 6 {
            nums.join(", ")
        } else {
            format!("{}, … +{} more", nums[..6].join(", "), nums.len() - 6)
        };
        app.popup = Popup::Confirm {
            title: format!("Move {} item(s)", count),
            message: format!("Move to {}?\n{}", status_label, preview),
            on_confirm: ConfirmAction::MoveItems {
                indices: item_indices.to_vec(),
                target: target_status,
            },
        };
    }
}

pub fn handle_history(app: &mut App, key: KeyCode, item_idx: usize) {
    match key {
        KeyCode::Char('r') => {
            app.history_events.clear();
            spawn_history_fetch(app, item_idx);
        }
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

/// Shared: spawn a background thread to fetch timeline history for an item.
fn spawn_history_fetch(app: &mut App, item_idx: usize) {
    if let Some(item) = app.items.get(item_idx) {
        if let (Some(repo), Some(number)) = (item.repository.clone(), item.number) {
            app.history_events.push("Loading history...".to_string());
            app.loading = true;
            let tx = app.action_tx.clone();
            std::thread::spawn(move || {
                let endpoint = format!("repos/{}/issues/{}/timeline", repo, number);
                let mut events_list = Vec::new();
                match crate::gh::client::api(&endpoint, None, &[]) {
                    Ok(output) => {
                        if let Ok(events) = serde_json::from_str::<Vec<serde_json::Value>>(&output)
                        {
                            for event in events {
                                let event_type = event
                                    .get("event")
                                    .and_then(|e| e.as_str())
                                    .unwrap_or("unknown");
                                let actor = event
                                    .get("actor")
                                    .and_then(|a| a.get("login"))
                                    .and_then(|l| l.as_str())
                                    .unwrap_or("system");
                                let created = event
                                    .get("created_at")
                                    .and_then(|c| c.as_str())
                                    .unwrap_or("");
                                let date = if created.len() >= 10 {
                                    &created[..10]
                                } else {
                                    created
                                };

                                let action = match event_type {
                                    "commented" | "commented_on" => {
                                        let body_preview = event
                                            .get("body")
                                            .and_then(|b| b.as_str())
                                            .map(|b| b.chars().take(60).collect::<String>())
                                            .unwrap_or_default();
                                        format!("commented: \"{}\"", body_preview)
                                    }
                                    "moved_columns_in_project" => {
                                        "moved in project board".to_string()
                                    }
                                    "labeled" => {
                                        let label = event
                                            .get("label")
                                            .and_then(|l| l.get("name"))
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("?");
                                        format!("added label: {}", label)
                                    }
                                    "assigned" => {
                                        let assignee = event
                                            .get("assignee")
                                            .and_then(|a| a.get("login"))
                                            .and_then(|l| l.as_str())
                                            .unwrap_or("?");
                                        format!("assigned to @{}", assignee)
                                    }
                                    "closed" => "closed issue".to_string(),
                                    "reopened" => "reopened issue".to_string(),
                                    _ => event_type.to_string(),
                                };

                                events_list.push(format!("{}|{}|{}", date, actor, action));
                            }
                        }
                        if events_list.is_empty() {
                            events_list.push("No timeline events found".to_string());
                        }
                    }
                    Err(e) => {
                        events_list.push(format!("Error fetching: {}", e));
                    }
                }
                let _ = tx.send(crate::app::ActionResult::HistoryFetched(events_list));
            });
        }
    }
}

pub fn handle_comment(
    app: &mut App,
    key: KeyCode,
    modifiers: crossterm::event::KeyModifiers,
    item_idx: usize,
) {
    match key {
        KeyCode::Char('s') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            if app.comment_text.trim().is_empty() {
                app.popup = Popup::Error("Comment cannot be empty".to_string());
                return;
            }
            let num = app
                .items
                .get(item_idx)
                .and_then(|i| i.number)
                .map(|n| format!("#{}", n))
                .unwrap_or_default();
            app.popup = Popup::Confirm {
                title: format!("Post Comment - {}", num),
                message: "Post this comment?".to_string(),
                on_confirm: ConfirmAction::PostComment(item_idx),
            };
        }
        KeyCode::Enter => {
            app.comment_discard_armed = false;
            crate::handlers::text_edit::insert_char(
                &mut app.comment_text,
                &mut app.comment_cursor,
                '\n',
            );
        }
        KeyCode::Char('v') if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            app.comment_discard_armed = false;
            if let Ok(text) = crate::export::read_from_clipboard() {
                crate::handlers::text_edit::insert_str(
                    &mut app.comment_text,
                    &mut app.comment_cursor,
                    &text.replace("\r\n", "\n"),
                );
            }
        }
        // Plain chars only - an unhandled Ctrl+<key> must not type a letter.
        KeyCode::Char(c) if !modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
            app.comment_discard_armed = false;
            crate::handlers::text_edit::insert_char(
                &mut app.comment_text,
                &mut app.comment_cursor,
                c,
            );
        }
        KeyCode::Backspace => {
            app.comment_discard_armed = false;
            crate::handlers::text_edit::backspace(&mut app.comment_text, &mut app.comment_cursor);
        }
        KeyCode::Delete => {
            app.comment_discard_armed = false;
            crate::handlers::text_edit::delete(&mut app.comment_text, &mut app.comment_cursor);
        }
        KeyCode::Left => {
            crate::handlers::text_edit::move_left(&app.comment_text, &mut app.comment_cursor);
        }
        KeyCode::Right => {
            crate::handlers::text_edit::move_right(&app.comment_text, &mut app.comment_cursor);
        }
        KeyCode::Home => {
            crate::handlers::text_edit::move_home(&app.comment_text, &mut app.comment_cursor);
        }
        KeyCode::End => {
            crate::handlers::text_edit::move_end(&app.comment_text, &mut app.comment_cursor);
        }
        KeyCode::Esc => {
            // One accidental Esc must not wipe a long comment: require a
            // second Esc to discard non-empty text.
            if app.comment_text.trim().is_empty() || app.comment_discard_armed {
                app.comment_text.clear();
                app.comment_discard_armed = false;
                app.go_back();
            } else {
                app.comment_discard_armed = true;
                app.set_status("Unsaved comment - press Esc again to discard, Ctrl+S to post");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::item::Item;

    fn app_with_item(status: Status, labels: Vec<String>) -> App {
        let mut app = App::new();
        app.items = vec![Item {
            id: "1".into(),
            title: "test item".into(),
            number: Some(42),
            status,
            labels,
            ..Default::default()
        }];
        app
    }

    #[test]
    fn test_pass_dev_guard() {
        let app = app_with_item(Status::InQADev, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Dev).is_ok());

        // Wrong column
        let app = app_with_item(Status::InQA, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Dev).is_err());

        // Already passed (label present)
        let app = app_with_item(Status::InQADev, vec!["Ready-for-Staging".into()]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Dev).is_err());
    }

    #[test]
    fn test_pass_stg_guard() {
        let app = app_with_item(Status::InQA, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Stg).is_ok());

        // Space-spelled label must also count as already-passed
        let app = app_with_item(Status::InQA, vec!["Ready for UAT".into()]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Stg).is_err());

        let app = app_with_item(Status::InUAT, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Stg).is_err());
    }

    #[test]
    fn test_pass_uat_guard() {
        let app = app_with_item(Status::InUAT, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Uat).is_ok());

        let app = app_with_item(Status::Backlog, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Uat).is_err());
    }

    #[test]
    fn test_return_guard() {
        let app = app_with_item(Status::InProgress, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Return).is_err());

        let app = app_with_item(Status::InQA, vec![]);
        assert!(qa_pass_allowed(&app, 0, PassKind::Return).is_ok());
    }

    fn uat_item(id: &str, number: u32, sprint: &str, status: Status) -> Item {
        Item {
            id: id.into(),
            number: Some(number),
            sprint: Some(sprint.into()),
            repository: Some("Org/repo".into()),
            status,
            ..Default::default()
        }
    }

    #[test]
    fn test_uat_batch_covers_the_column_in_the_active_sprint() {
        let mut app = App::new();
        app.items = vec![
            uat_item("a", 401, "Sprint 28", Status::InUAT),
            uat_item("b", 402, "Sprint 28", Status::InQA), // not In UAT
            uat_item("c", 403, "Sprint 28", Status::InUAT),
            uat_item("d", 404, "Sprint 27", Status::InUAT), // other sprint
        ];
        app.board_sprint_filter = Some("Sprint 28".to_string());

        // Triggered from a ticket inside the filter: the whole column, ordered.
        let batch = uat_batch(&app, 0);
        assert_eq!(
            batch,
            vec![0, 2],
            "only In UAT tickets of the active sprint"
        );

        // Triggered from a ticket the sprint filter hides: it still has to be
        // included, otherwise the action would silently skip the ticket the
        // user actually picked.
        let batch = uat_batch(&app, 3);
        assert_eq!(batch, vec![0, 2, 3]);
    }

    #[test]
    fn test_uat_batch_orders_by_issue_number() {
        let mut app = App::new();
        app.items = vec![
            uat_item("a", 409, "Sprint 28", Status::InUAT),
            uat_item("b", 401, "Sprint 28", Status::InUAT),
        ];
        app.board_sprint_filter = Some("Sprint 28".to_string());
        assert_eq!(uat_batch(&app, 0), vec![1, 0]);
    }

    #[test]
    fn test_fail_is_only_allowed_from_a_qa_column() {
        let mut app = App::new();
        app.items = vec![
            uat_item("a", 401, "Sprint 28", Status::Done),
            uat_item("b", 402, "Sprint 28", Status::InQA),
        ];
        assert!(qa_pass_allowed(&app, 0, PassKind::Fail).is_err());
        assert!(qa_pass_allowed(&app, 1, PassKind::Fail).is_ok());
    }
}
