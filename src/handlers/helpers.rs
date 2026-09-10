use crate::app::{ConfirmAction, Popup, Screen};
use crate::models::item::Item;

/// Find the new index for an old item index by matching item IDs.
pub fn find_new_idx(old_items: &[Item], new_items: &[Item], old_idx: usize) -> Option<usize> {
    let old_id = old_items.get(old_idx).map(|i| i.id.as_str())?;
    new_items.iter().position(|i| i.id == old_id)
}

/// Remap a single screen's item indices after items vec is replaced.
pub fn remap_screen(screen: &Screen, old_items: &[Item], new_items: &[Item]) -> Option<Screen> {
    match screen {
        Screen::ItemDetail(idx) => find_new_idx(old_items, new_items, *idx).map(Screen::ItemDetail),
        Screen::QaActions(idx) => find_new_idx(old_items, new_items, *idx).map(Screen::QaActions),
        Screen::History(idx) => find_new_idx(old_items, new_items, *idx).map(Screen::History),
        Screen::Comment(idx) => find_new_idx(old_items, new_items, *idx).map(Screen::Comment),
        Screen::BugReport(Some(idx), is_fail) => Some(
            find_new_idx(old_items, new_items, *idx)
                .map(|n| Screen::BugReport(Some(n), *is_fail))
                .unwrap_or(Screen::BugReport(None, *is_fail)),
        ),
        Screen::TaskForm(Some(idx)) => Some(
            find_new_idx(old_items, new_items, *idx)
                .map(|n| Screen::TaskForm(Some(n)))
                .unwrap_or(Screen::TaskForm(None)),
        ),
        Screen::MoveDialog(indices) => {
            let remapped: Vec<usize> = indices
                .iter()
                .filter_map(|i| find_new_idx(old_items, new_items, *i))
                .collect();
            if remapped.is_empty() {
                None
            } else {
                Some(Screen::MoveDialog(remapped))
            }
        }
        _ => Some(screen.clone()), // No index to remap
    }
}

/// Remap item indices in the current screen (and history) after items vec is replaced.
/// Looks up the old item's ID in the new items vec. If not found, pops back.
pub fn remap_screen_indices(app: &mut crate::app::App, old_items: &[Item]) {
    // Remap current screen
    if let Some(new_screen) = remap_screen(&app.screen, old_items, &app.items) {
        app.screen = new_screen;
    } else {
        app.screen = app.previous_screens.pop().unwrap_or(Screen::Menu);
        app.set_status("Item removed during refresh, returning...");
    }

    // Remap previous_screens history
    for screen in &mut app.previous_screens {
        *screen = remap_screen(screen, old_items, &app.items).unwrap_or(Screen::Menu);
    }

    // Remap indices held by an open Confirm popup. Without this, a background
    // refresh that reorders `items` while the popup is open would make Enter
    // execute the action against whatever item now sits at the stale index.
    if let Popup::Confirm { on_confirm, .. } = &app.popup {
        match remap_confirm_action(on_confirm, old_items, &app.items) {
            Some(remapped) => {
                if let Popup::Confirm { on_confirm, .. } = &mut app.popup {
                    *on_confirm = remapped;
                }
            }
            None => {
                app.popup = Popup::None;
                app.set_status("Item changed during refresh - action cancelled, please retry");
            }
        }
    }
}

/// Remap the item indices inside a pending ConfirmAction after items are replaced.
/// Returns None when the target item(s) no longer exist (the action must be cancelled).
fn remap_confirm_action(
    action: &ConfirmAction,
    old_items: &[Item],
    new_items: &[Item],
) -> Option<ConfirmAction> {
    let remap = |idx: usize| find_new_idx(old_items, new_items, idx);
    match action {
        ConfirmAction::PassDev(idx) => remap(*idx).map(ConfirmAction::PassDev),
        ConfirmAction::PassStg(idx) => remap(*idx).map(ConfirmAction::PassStg),
        ConfirmAction::PassUat { indices } => {
            let remapped: Vec<usize> = indices.iter().filter_map(|i| remap(*i)).collect();
            if remapped.is_empty() {
                None
            } else {
                Some(ConfirmAction::PassUat { indices: remapped })
            }
        }
        ConfirmAction::Return(idx) => remap(*idx).map(ConfirmAction::Return),
        ConfirmAction::PostComment(idx) => remap(*idx).map(ConfirmAction::PostComment),
        ConfirmAction::ClearHandoffLabels(idx) => {
            remap(*idx).map(ConfirmAction::ClearHandoffLabels)
        }
        ConfirmAction::MoveItems { indices, target } => {
            let remapped: Vec<usize> = indices.iter().filter_map(|i| remap(*i)).collect();
            if remapped.is_empty() {
                None
            } else {
                Some(ConfirmAction::MoveItems {
                    indices: remapped,
                    target: target.clone(),
                })
            }
        }
        // A fail-flow bug report needs its parent; cancel if the parent vanished.
        ConfirmAction::SubmitBugReport(Some(idx), is_fail) => {
            remap(*idx).map(|n| ConfirmAction::SubmitBugReport(Some(n), *is_fail))
        }
        ConfirmAction::SubmitBugReport(None, is_fail) => {
            Some(ConfirmAction::SubmitBugReport(None, *is_fail))
        }
        ConfirmAction::SubmitTask(Some(idx)) => {
            remap(*idx).map(|n| ConfirmAction::SubmitTask(Some(n)))
        }
        ConfirmAction::SubmitTask(None) => Some(ConfirmAction::SubmitTask(None)),
        // No item indices involved - safe across refreshes.
        ConfirmAction::ClearCache
        | ConfirmAction::SendStandupToSlack
        | ConfirmAction::SendSprintReportToSlack
        | ConfirmAction::MarkAllNotificationsRead
        | ConfirmAction::DeleteStackLead(_)
        | ConfirmAction::Quit => Some(action.clone()),
    }
}

pub fn extract_repo_name(repo: &str) -> String {
    if repo.contains('/') {
        repo.split('/').next_back().unwrap_or(repo).to_string()
    } else {
        repo.to_string()
    }
}

/// Split an item's `nameWithOwner` into `(owner, repo)`.
///
/// A project can contain issues from repositories outside the project's own
/// owner. Rebuilding the pair as `config.owner` + repo name (what the timeline
/// and created_at fetches used to do) silently queried the wrong repository -
/// those items just came back with no history, which the reports then counted
/// as "no QA events". `default_owner` only fills in for a bare repo name.
pub fn split_repo(repo: &str, default_owner: &str) -> (String, String) {
    match repo.split_once('/') {
        Some((owner, name)) if !owner.is_empty() && !name.is_empty() => {
            (owner.to_string(), name.to_string())
        }
        _ => (default_owner.to_string(), repo.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn test_remap_confirm_action_follows_item() {
        let old = vec![item("a"), item("b")];
        let new = vec![item("b"), item("a")]; // reordered by refresh

        match remap_confirm_action(&ConfirmAction::PassUat { indices: vec![0] }, &old, &new) {
            Some(ConfirmAction::PassUat { indices }) => {
                assert_eq!(indices, vec![1], "must follow item 'a'")
            }
            other => panic!("unexpected remap result: {:?}", other),
        }
    }

    #[test]
    fn test_remap_confirm_action_cancels_when_item_gone() {
        let old = vec![item("a"), item("b")];
        let new = vec![item("b")];
        assert!(
            remap_confirm_action(&ConfirmAction::PassUat { indices: vec![0] }, &old, &new)
                .is_none()
        );
        assert!(remap_confirm_action(
            &ConfirmAction::MoveItems {
                indices: vec![0],
                target: crate::models::status::Status::InUAT,
            },
            &old,
            &new
        )
        .is_none());
    }

    #[test]
    fn test_split_repo() {
        assert_eq!(
            split_repo("OtherOrg/service-api", "acme"),
            ("OtherOrg".to_string(), "service-api".to_string())
        );
        // Bare repo name → fall back to the configured owner.
        assert_eq!(
            split_repo("service-api", "acme"),
            ("acme".to_string(), "service-api".to_string())
        );
    }

    #[test]
    fn test_remap_confirm_action_passthrough_without_indices() {
        let old = vec![item("a")];
        let new: Vec<Item> = vec![];
        assert_eq!(
            remap_confirm_action(&ConfirmAction::ClearCache, &old, &new),
            Some(ConfirmAction::ClearCache)
        );
    }
}
