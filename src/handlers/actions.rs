use crate::app::{App, ConfirmAction, Popup};
use crate::gh;
use crate::models::status::Status;
use crate::slack;

/// Execute a confirmed action (after user presses Enter on Confirm popup).
pub fn execute_confirm_action(app: &mut App, action: &ConfirmAction) {
    // Pre-extract Slack config for background threads
    let slack_url = if app.config.is_slack_configured() {
        app.config.slack_webhook_url.clone()
    } else {
        None
    };
    let slack_user = app.current_user.clone();
    let slack_notify = app.config.slack_notify.clone();
    let slack_user_map = app.config.slack_user_map.clone();

    match action {
        ConfirmAction::PassDev(item_idx) => {
            handle_pass_label_action(
                app,
                *item_idx,
                PassLabelSpec {
                    action_name: "Pass Dev",
                    slack_label: "Passed Dev",
                    label: "Ready-for-Staging",
                    body: "Verified on Dev. Ready for STG deploy.",
                    notify_enabled: slack_notify.pass_dev,
                    expected_status: Status::InQADev,
                },
                slack_url,
                slack_user,
                slack_user_map.clone(),
            );
        }
        ConfirmAction::PassStg(item_idx) => {
            // Label-only handoff (mirrors PassDev): devs deploy to UAT and move the
            // ticket to In UAT themselves. No status move here.
            handle_pass_label_action(
                app,
                *item_idx,
                PassLabelSpec {
                    action_name: "Pass STG",
                    slack_label: "Passed STG",
                    label: "Ready-for-UAT",
                    body: "Verified on STG. Ready for UAT deploy.",
                    notify_enabled: slack_notify.pass_stg,
                    expected_status: Status::InQA,
                },
                slack_url,
                slack_user,
                slack_user_map.clone(),
            );
        }
        ConfirmAction::PassUat { indices } => {
            // Resolve the status option once, and fail before touching
            // anything if the project fields are not loaded.
            let Some(field) = app.status_field().cloned() else {
                app.set_status(
                    "Pass UAT failed: project fields not loaded yet - refresh and retry",
                );
                return;
            };
            let Some(pid) = app.project_id.clone() else {
                app.set_status(
                    "Pass UAT failed: project fields not loaded yet - refresh and retry",
                );
                return;
            };
            let Some(option) = field.find_option(Status::TechComplete.label()).cloned() else {
                app.popup = Popup::Error(format!(
                    "Pass UAT failed: status option '{}' not found on the project.",
                    Status::TechComplete.label()
                ));
                return;
            };

            let mut targets: Vec<UatTarget> = Vec::new();
            let mut drafts: Vec<String> = Vec::new();
            let mut slack_items: Vec<slack::notifier::ReleaseItem> = Vec::new();
            let mut slack_devs: Vec<String> = Vec::new();

            for &idx in indices {
                let Some(item) = app.items.get(idx) else {
                    continue;
                };
                // A draft item has no issue to move or comment on.
                let (Some(number), Some(repo)) = (item.number, item.repository.clone()) else {
                    drafts.push(item.title.clone());
                    continue;
                };
                let devs = item.dev_assignees(&app.current_user);
                let dev_mentions: Vec<String> = devs.iter().map(|d| format!("@{}", d)).collect();
                for d in devs {
                    if !slack_devs.contains(&d) {
                        slack_devs.push(d);
                    }
                }
                // Leads of every stack in the batch, deduped.
                for l in app.config.get_leads(item.stack.as_deref()) {
                    if !slack_devs.contains(&l) {
                        slack_devs.push(l);
                    }
                }
                slack_items.push(slack::notifier::ReleaseItem {
                    number: Some(number),
                    repo: Some(repo.clone()),
                    title: item.title.clone(),
                });
                targets.push(UatTarget {
                    item_id: item.id.clone(),
                    number,
                    repo,
                    comment: comment_with_cc(
                        "Regression verified on UAT. Moved to Tech Complete.",
                        &dev_mentions.join(" "),
                    ),
                });
            }

            if targets.is_empty() {
                app.popup = Popup::Error(
                    "Pass UAT: none of these items has a linked GitHub issue (draft items) - cannot move or comment."
                        .to_string(),
                );
                return;
            }

            // Optimistic local move; anything the revalidation aborts is put
            // back through ParentStatusChanged.
            for &idx in indices {
                if let Some(item) = app.items.get_mut(idx) {
                    if item.number.is_some() {
                        item.status = Status::TechComplete;
                    }
                }
            }

            let field_id = field.id.clone();
            let option_id = option.id.clone();
            let notify_uat = slack_notify.pass_uat;
            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status(&format!(
                "Pass UAT: verifying {} ticket(s)...",
                targets.len()
            ));

            std::thread::spawn(move || {
                let mut passed: Vec<usize> = Vec::new(); // index into targets
                let mut moved_on: Vec<String> = Vec::new(); // "#123 comment failed"
                let mut aborted: Vec<String> = Vec::new();
                let mut failed: Vec<String> = Vec::new();

                for (i, t) in targets.iter().enumerate() {
                    // Revalidate per ticket: a dev may have moved one of them
                    // while the regression run was going on.
                    if let Ok(Some(remote)) = gh::item::get_item_status(&t.item_id) {
                        let actual = Status::from_str(&remote);
                        if actual != Status::InUAT {
                            let _ = tx.send(crate::app::ActionResult::ParentStatusChanged {
                                item_id: t.item_id.clone(),
                                status: actual.clone(),
                            });
                            aborted.push(format!("#{} is now '{}'", t.number, actual.label()));
                            continue;
                        }
                    }
                    if let Err(e) = gh::item::edit_field(&t.item_id, &pid, &field_id, &option_id) {
                        failed.push(format!("#{}: {}", t.number, e));
                        continue;
                    }
                    if let Err(e) = gh::issue::comment(&t.repo, t.number, &t.comment) {
                        // Moved but not commented: report it separately, the
                        // status change already happened.
                        moved_on.push(format!("#{}: comment failed ({})", t.number, e));
                    }
                    passed.push(i);
                }

                // One aggregated release message for the whole run.
                let mut slack_note = String::new();
                if !passed.is_empty() && notify_uat {
                    if let Some(ref url) = slack_url {
                        let released: Vec<slack::notifier::ReleaseItem> = passed
                            .iter()
                            .filter_map(|i| slack_items.get(*i))
                            .map(|r| slack::notifier::ReleaseItem {
                                number: r.number,
                                repo: r.repo.clone(),
                                title: r.title.clone(),
                            })
                            .collect();
                        if slack::notifier::notify_release_ready(
                            url,
                            &slack_user,
                            &released,
                            &slack_devs,
                            &slack_user_map,
                        )
                        .is_err()
                        {
                            slack_note = ", ✗ Slack release summary failed".to_string();
                        } else {
                            slack_note = ", ✓ Slack release summary sent".to_string();
                        }
                    }
                }

                let mut notes: Vec<String> = Vec::new();
                if !drafts.is_empty() {
                    notes.push(format!(
                        "{} draft item(s) skipped (no linked issue)",
                        drafts.len()
                    ));
                }
                for a in &aborted {
                    notes.push(format!("aborted: {}", a));
                }
                for m in &moved_on {
                    notes.push(m.clone());
                }
                for fl in &failed {
                    notes.push(format!("failed: {}", fl));
                }

                let headline = format!(
                    "Pass UAT: ✓ {} ticket(s) moved to Tech Complete{}",
                    passed.len(),
                    slack_note
                );
                let result = if notes.is_empty() {
                    crate::app::ActionResult::Success(headline)
                } else if passed.is_empty() {
                    crate::app::ActionResult::Error(format!(
                        "Pass UAT: nothing was passed.\n{}",
                        notes.join("\n")
                    ))
                } else {
                    crate::app::ActionResult::Error(format!("{}\n{}", headline, notes.join("\n")))
                };
                let _ = tx.send(result);
            });
        }

        ConfirmAction::Return(item_idx) => {
            handle_return_action(
                app,
                *item_idx,
                slack_url,
                slack_user,
                slack_notify,
                slack_user_map.clone(),
            );
        }
        ConfirmAction::MoveItems { indices, target } => {
            // Resolve the field/option ONCE and fail loudly before touching
            // anything, instead of per item deep inside the loop.
            let Some(field) = app.status_field().cloned() else {
                app.set_status("Move failed: project fields not loaded yet - refresh and retry");
                return;
            };
            let Some(proj_id) = app.project_id.clone() else {
                app.set_status("Move failed: project fields not loaded yet - refresh and retry");
                return;
            };
            let Some(option) = field.find_option(target.label()).cloned() else {
                app.set_status(&format!(
                    "Move failed: status option '{}' not found on the project",
                    target.label()
                ));
                return;
            };

            // (item_id, "#123") for every item, plus the optimistic local move.
            let mut targets: Vec<(String, String)> = Vec::new();
            for &idx in indices {
                if let Some(item) = app.items.get_mut(idx) {
                    let num = item
                        .number
                        .map(|n| format!("#{}", n))
                        .unwrap_or_else(|| "draft".to_string());
                    targets.push((item.id.clone(), num));
                    item.status = target.clone();
                    item.selected = false;
                }
            }
            if targets.is_empty() {
                app.set_status("Move failed: no items to move");
                app.go_back();
                return;
            }

            let target_label = target.label();
            let move_notify = slack_notify.move_items;
            let field_id = field.id.clone();
            let option_id = option.id.clone();
            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status(&format!(
                "Moving {} item(s) to {}...",
                targets.len(),
                target_label
            ));

            // ONE worker for the whole batch: a thread + `gh` process per item
            // meant a 20-item move spawned 20 concurrent subprocesses, and each
            // failure reported itself separately.
            std::thread::spawn(move || {
                let mut moved: Vec<String> = Vec::new();
                let mut failures: Vec<String> = Vec::new();
                for (item_id, num) in &targets {
                    match gh::item::edit_field(item_id, &proj_id, &field_id, &option_id) {
                        Ok(()) => moved.push(num.clone()),
                        Err(e) => failures.push(format!("{}: {}", num, e)),
                    }
                }

                if !moved.is_empty() && move_notify {
                    if let Some(ref url) = slack_url {
                        // Name the tickets - "3 item(s) to In QA" told the team
                        // nothing about WHAT moved.
                        let move_title = format!(
                            "{} item(s) to {}: {}",
                            moved.len(),
                            target_label,
                            moved.join(" ")
                        );
                        if slack::notifier::notify_action(
                            url,
                            &slack_user,
                            "Moved",
                            &move_title,
                            None,
                            None,
                            &[],
                            &slack_user_map,
                            None,
                        )
                        .is_err()
                        {
                            let _ = tx.send(crate::app::ActionResult::StatusMessage(
                                "✗ Slack notify for move failed".to_string(),
                            ));
                        }
                    }
                }

                let result = if failures.is_empty() {
                    crate::app::ActionResult::Success(format!(
                        "✓ Moved {} item(s) to {}",
                        moved.len(),
                        target_label
                    ))
                } else {
                    crate::app::ActionResult::Error(format!(
                        "Moved {} of {} item(s) to {}.\nFailed:\n{}\nThe board will revert the failures on the next sync.",
                        moved.len(),
                        targets.len(),
                        target_label,
                        failures.join("\n")
                    ))
                };
                let _ = tx.send(result);
            });
            app.go_back();
        }
        ConfirmAction::SubmitTask(parent_idx) => {
            let parent_idx = *parent_idx;
            let (repo, parent_node_id, parent_sprint) = parent_idx
                .and_then(|idx| app.items.get(idx))
                .map(|item| {
                    (
                        item.repository.clone(),
                        item.content_node_id.clone(),
                        item.sprint.clone(),
                    )
                })
                .unwrap_or((None, None, None));

            let repo = repo.unwrap_or_else(|| app.config.bug_repo.clone()); // Assuming bug_repo is a reasonable default for tasks too

            let priorities = ["--", "P0", "P1", "P2", "P3", "P4"];
            let stacks = ["--", "FE", "BE", "App UI", "App BE"];
            let prio = priorities[app.task_priority_idx].to_string();
            let stack = stacks[app.task_stack_idx].to_string();

            // For now, Tasks don't natively capture the Sprint from UI, we'll assign it to None.
            // In a future update we can inherit the Sprint from the Parent Issue.
            let task_sprint: Option<String> = parent_sprint;

            // Build body matching GitHub bug.yml template format
            let mut body = String::new();

            // Text sections (only include if non-empty)
            let sections: Vec<(&str, &str)> = vec![
                ("Description", &app.task_description),
                ("Expected Outcome", &app.task_outcome),
            ];
            for (heading, content) in &sections {
                if !content.is_empty() {
                    body.push_str(&format!("### {}\n\n{}\n\n", heading, content));
                }
            }

            // Assignee logic
            let selected_assignee = app.task_assignees.clone();

            let owner = app.config.owner.clone();
            let project_number = app.config.project_number;

            let title = format!("[TASK] {}", app.task_title);

            // Clone for Slack notification
            let slack_url = slack_url.clone();
            let slack_user = slack_user.clone();
            let slack_notify = slack_notify.clone();

            if repo.is_empty() {
                app.popup = Popup::Error(
                    "No repository to create the task in: the parent item has no repo and no Bug Repo URL is configured.\nSet it via Settings → [s] Setup wizard."
                        .to_string(),
                );
                return;
            }

            let project_id_clone = app.project_id.clone();
            let priority_field_clone = app.priority_field().cloned();
            let stack_field_clone = app.stack_field().cloned();
            let sprint_field_clone = app.sprint_field().cloned();
            let type_field_clone = app.type_field().cloned();
            let status_field_clone = app.status_field().cloned();

            // Leave the form intact - it is cleared by the main loop only when
            // TaskCreated arrives, so a failed create keeps the draft.
            app.go_back();

            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status("Creating task...");

            std::thread::spawn(move || {
                let assignee_refs: Vec<&str> =
                    selected_assignee.iter().map(AsRef::as_ref).collect();
                // Field edits used to be fire-and-forget: a task could land on
                // the board with no Status, Sprint or Priority and report
                // nothing but success.
                let mut task_warnings: Vec<String> = Vec::new();
                match gh::issue::create(&repo, &title, &body, &["Task"], &assignee_refs) {
                    Ok(url) => {
                        let item_id = match gh::item::add_to_project(&owner, project_number, &url) {
                            Ok(id) => Some(id),
                            Err(e) => {
                                task_warnings.push(format!("Add to project: {}", e));
                                None
                            }
                        };

                        // Set Fields securely on the Project, rather than Markdown
                        if let (Some(iid), Some(pid)) = (&item_id, &project_id_clone) {
                            if prio != "--" {
                                if let Some(field) = &priority_field_clone {
                                    if let Some(opt) = field.find_option(&prio) {
                                        let _ = gh::item::edit_field(iid, pid, &field.id, &opt.id);
                                    }
                                }
                            }
                            if stack != "--" {
                                if let Some(field) = &stack_field_clone {
                                    if let Some(opt) = field.find_option(&stack) {
                                        let _ = gh::item::edit_field(iid, pid, &field.id, &opt.id);
                                    }
                                }
                            }
                            if let Some(sprint_val) = &task_sprint {
                                match &sprint_field_clone {
                                    Some(field) => match field.find_option(sprint_val) {
                                        Some(opt) => {
                                            if let Err(e) = gh::item::edit_iteration_field(
                                                iid, pid, &field.id, &opt.id,
                                            ) {
                                                task_warnings.push(format!("Sprint: {}", e));
                                            }
                                        }
                                        None => task_warnings.push(format!(
                                            "Sprint: '{}' not found on the project",
                                            sprint_val
                                        )),
                                    },
                                    None => {
                                        task_warnings.push("Sprint: field not found".to_string())
                                    }
                                }
                            }
                            if let Some(field) = &type_field_clone {
                                if let Some(opt) = field.find_option("Task") {
                                    let _ = gh::item::edit_field(iid, pid, &field.id, &opt.id);
                                }
                            }
                            // "Todo" is not a column on this board, so the
                            // lookup always missed and new tasks landed with no
                            // Status at all. Use the same entry column as a bug.
                            match &status_field_clone {
                                Some(field) => match field.find_option(Status::ReadyForDev.label())
                                {
                                    Some(opt) => {
                                        if let Err(e) =
                                            gh::item::edit_field(iid, pid, &field.id, &opt.id)
                                        {
                                            task_warnings.push(format!("Status: {}", e));
                                        }
                                    }
                                    None => task_warnings.push(format!(
                                        "Status: '{}' not found on the project",
                                        Status::ReadyForDev.label()
                                    )),
                                },
                                None => task_warnings.push("Status: field not found".to_string()),
                            }
                        }

                        // Both mutations need the ISSUE node id. They used to
                        // be handed the project item id, so neither the issue
                        // type nor the sub-issue link was ever applied (both
                        // results were discarded).
                        let new_number = url
                            .split('/')
                            .next_back()
                            .and_then(|s| s.parse::<u32>().ok());
                        let content_node = match new_number {
                            Some(n) => match gh::issue::node_id(&repo, n) {
                                Ok(id) => Some(id),
                                Err(e) => {
                                    task_warnings.push(format!("Issue node id: {}", e));
                                    None
                                }
                            },
                            None => None,
                        };

                        if let Some(node) = &content_node {
                            if let Err(e) = gh::issue::set_issue_type(&repo, node, "Task") {
                                task_warnings.push(format!("Issue type 'Task': {}", e));
                            }
                            if let Some(p_node) = &parent_node_id {
                                if let Err(e) = gh::issue::add_sub_issue(p_node, node) {
                                    task_warnings.push(format!("Sub-issue link: {}", e));
                                }
                            }
                        }

                        // Slack notification
                        if let Some(ref webhook_url) = slack_url {
                            if slack_notify.new_task {
                                let slack_assignees: Vec<String> = selected_assignee
                                    .iter()
                                    .filter(|a| *a != &slack_user)
                                    .cloned()
                                    .collect();
                                // The issue number is right there in the URL -
                                // link the task instead of a bare title.
                                let task_number = url
                                    .split('/')
                                    .next_back()
                                    .and_then(|s| s.parse::<u32>().ok());
                                if slack::notifier::notify_action(
                                    webhook_url,
                                    &slack_user,
                                    "Task Created",
                                    &title,
                                    task_number,
                                    Some(&repo),
                                    &slack_assignees,
                                    &slack_user_map,
                                    None,
                                )
                                .is_err()
                                {
                                    let _ = tx.send(crate::app::ActionResult::StatusMessage(
                                        "✗ Slack notify for task failed".to_string(),
                                    ));
                                }
                            }
                        }
                        if !task_warnings.is_empty() {
                            let _ = tx.send(crate::app::ActionResult::StatusMessage(format!(
                                "Task created with warnings: {}",
                                task_warnings.join("; ")
                            )));
                        }
                        let _ = tx.send(crate::app::ActionResult::TaskCreated(url));
                    }
                    Err(e) => {
                        let _ = tx.send(crate::app::ActionResult::Error(format!(
                            "Failed to create task: {}\nYour draft is kept - press t to reopen and retry.",
                            e
                        )));
                    }
                }
            });
        }
        ConfirmAction::SubmitBugReport(parent_idx, is_fail) => {
            let parent_idx = *parent_idx;
            let is_fail = *is_fail;
            let priorities = ["--", "P0", "P1", "P2", "P3", "P4"];
            let stacks = ["--", "FE", "BE", "App UI", "App BE"];
            let envs = crate::app::App::BUG_ENVS;
            let broken_opts = ["Completely", "Partially"];
            let visible_opts = ["Yes", "No"];
            let workaround_opts = ["Easy workaround", "Difficult workaround", "No Workaround"];
            let impact_prio_opts = ["Yes", "No"];

            let prio = priorities[app.bug_priority_idx].to_string();
            let stack = stacks[app.bug_stack_idx].to_string();
            let env = envs[app.bug_env_idx];

            // Build body matching GitHub bug.yml template format
            let mut body = String::new();

            // Text sections (only include if non-empty)
            let sections: Vec<(&str, &str)> = vec![
                ("Description", &app.bug_description),
                ("Pre-Conditions", &app.bug_precondition),
                ("Test Data", &app.bug_test_data),
                ("Steps to Reproduce", &app.bug_steps),
                ("Expected Result", &app.bug_expected),
                ("Actual Result", &app.bug_actual),
                ("Impact / Severity", &app.bug_impact),
                ("Evidence", &app.bug_evidence),
            ];
            for (heading, content) in &sections {
                if !content.is_empty() || *heading == "Evidence" {
                    body.push_str(&format!("### {}\n\n{}\n\n", heading, content));
                }
            }

            // Environment (checkbox format)
            body.push_str("### Environment\n\n");
            for e in &envs {
                let checked = if *e == env { "X" } else { " " };
                body.push_str(&format!("- [{}] {}\n", checked, e));
            }
            body.push('\n');

            // Linked Story (auto-fill from parent via GraphQL later)
            // No longer injecting markdown links.

            // Security
            if !app.bug_security.is_empty() {
                body.push_str(&format!(
                    "### Security Implications\n\n{}\n\n",
                    app.bug_security
                ));
            }

            // Priority Scoring section
            body.push_str("---\n## Priority Scoring\n\n");
            body.push_str(&format!(
                "### Is the feature broken?\n\n{}\n\n",
                broken_opts[app.bug_broken_idx]
            ));
            body.push_str(&format!(
                "### Is the bug visible to the user?\n\n{}\n\n",
                visible_opts[app.bug_visible_idx]
            ));
            body.push_str(&format!(
                "### Is there a workaround?\n\n{}\n\n",
                workaround_opts[app.bug_workaround_idx]
            ));
            body.push_str(&format!(
                "### Impact prioritised functionality?\n\n{}\n\n",
                impact_prio_opts[app.bug_impact_prio_idx]
            ));
            body.push_str("### Priority Scoring Confirmation\n\n- [X] I have reviewed all priority scoring metrics above and confirmed they are accurate.\n\n");

            // Assignee logic
            let selected_assignee = app.bug_assignees.clone();

            // Fail flow: the parent is moved to In Progress and commented only
            // AFTER the bug is successfully created - otherwise a failed create
            // would leave a moved parent with no bug. The comment goes to the
            // parent's own repository (not bug_repo) and links the new bug.
            // A draft parent can't be moved or commented on - the confirm
            // popup promised both, so say so instead of silently skipping.
            let mut parent_fail_warning: Option<String> = None;
            let parent_fail_info = if is_fail {
                match parent_idx.and_then(|idx| app.items.get(idx)) {
                    Some(item) => match (item.number, item.repository.clone()) {
                        (Some(number), Some(parent_repo)) => {
                            let devs = item.dev_assignees(&app.current_user);
                            let dev_mentions: Vec<String> =
                                devs.iter().map(|d| format!("@{}", d)).collect();
                            // Params for the remote status move, performed from the thread.
                            let move_params = match (app.status_field(), app.project_id.as_ref()) {
                                (Some(field), Some(pid)) => {
                                    field.find_option(Status::InProgress.label()).map(|opt| {
                                        (
                                            item.id.clone(),
                                            pid.clone(),
                                            field.id.clone(),
                                            opt.id.clone(),
                                        )
                                    })
                                }
                                _ => None,
                            };
                            let stale_labels = App::handoff_labels_on(item);
                            Some((
                                parent_repo,
                                number,
                                dev_mentions,
                                move_params,
                                item.id.clone(),
                                stale_labels,
                            ))
                        }
                        _ => {
                            parent_fail_warning = Some(
                                "✗ Parent not updated: it has no linked GitHub issue (draft item) - move and comment it manually"
                                    .to_string(),
                            );
                            None
                        }
                    },
                    None => None,
                }
            } else {
                None
            };

            let owner = app.config.owner.clone();
            let project_number = app.config.project_number;

            let kind = app.bug_kind;
            let title = format!("{} {}", kind.title_prefix(), app.bug_title);
            let notify_report = match kind {
                crate::app::ReportKind::Bug => slack_notify.bug_report,
                crate::app::ReportKind::Enhancement => slack_notify.enhancement,
            };

            // Clone for Slack notification (the toggle was already resolved
            // into `notify_report` above, per report kind).
            let slack_url = slack_url.clone();
            let slack_user = slack_user.clone();

            let repo = parent_idx
                .and_then(|idx| app.items.get(idx))
                .and_then(|item| item.repository.clone())
                .unwrap_or_else(|| app.config.bug_repo.clone());

            if repo.is_empty() {
                app.popup = Popup::Error(
                    "No repository to create the bug in: the parent item has no repo and no Bug Repo URL is configured.\nSet it via Settings → [s] Setup wizard."
                        .to_string(),
                );
                return;
            }

            let project_id_clone = app.project_id.clone();
            let priority_field_clone = app.priority_field().cloned();
            let stack_field_clone = app.stack_field().cloned();
            let sprint_field_clone = app.sprint_field().cloned();
            let type_field_clone = app.type_field().cloned();
            let status_field_clone = app.status_field().cloned();

            let parent_sprint = parent_idx
                .and_then(|idx| app.items.get(idx))
                .and_then(|item| item.sprint.clone())
                .or_else(|| app.board_sprint_filter.clone());

            let parent_node_id = parent_idx
                .and_then(|idx| app.items.get(idx))
                .and_then(|item| item.content_node_id.clone());

            // Leave the form intact - it is cleared by the main loop only when
            // ItemCreated arrives, so a failed create keeps the draft.
            app.go_back();

            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status("Creating bug report...");

            std::thread::spawn(move || {
                let assignee_refs: Vec<&str> =
                    selected_assignee.iter().map(AsRef::as_ref).collect();
                match gh::issue::create(&repo, &title, &body, kind.labels(), &assignee_refs) {
                    Ok(url) => {
                        let mut warnings: Vec<String> = Vec::new();
                        if let Some(w) = parent_fail_warning {
                            warnings.push(w);
                        }

                        let item_id = match gh::item::add_to_project(&owner, project_number, &url) {
                            Ok(id) => Some(id),
                            Err(e) => {
                                warnings.push(format!("✗ Add to project failed: {}", e));
                                None
                            }
                        };

                        // Set Fields securely on the Project
                        if let (Some(iid), Some(pid)) = (&item_id, &project_id_clone) {
                            // Priority
                            if prio != "--" {
                                match &priority_field_clone {
                                    Some(field) => match field.find_option(&prio) {
                                        Some(opt) => {
                                            if let Err(e) =
                                                gh::item::edit_field(iid, pid, &field.id, &opt.id)
                                            {
                                                warnings.push(format!("✗ Priority: {}", e));
                                            }
                                        }
                                        None => warnings.push(format!(
                                            "✗ Priority: option '{}' not found in field options",
                                            prio
                                        )),
                                    },
                                    None => {
                                        warnings.push("✗ Priority: field not found".to_string())
                                    }
                                }
                            }
                            // Stack
                            if stack != "--" {
                                match &stack_field_clone {
                                    Some(field) => match field.find_option(&stack) {
                                        Some(opt) => {
                                            if let Err(e) =
                                                gh::item::edit_field(iid, pid, &field.id, &opt.id)
                                            {
                                                warnings.push(format!("✗ Stack: {}", e));
                                            }
                                        }
                                        None => warnings
                                            .push(format!("✗ Stack: option '{}' not found", stack)),
                                    },
                                    None => warnings.push("✗ Stack: field not found".to_string()),
                                }
                            }
                            // Sprint
                            if let Some(sprint_val) = &parent_sprint {
                                match &sprint_field_clone {
                                    Some(field) => match field.find_option(sprint_val) {
                                        Some(opt) => {
                                            if let Err(e) = gh::item::edit_iteration_field(
                                                iid, pid, &field.id, &opt.id,
                                            ) {
                                                warnings.push(format!("✗ Sprint: {}", e));
                                            }
                                        }
                                        None => {
                                            let available: Vec<&str> = field
                                                .options
                                                .iter()
                                                .map(|o| o.name.as_str())
                                                .collect();
                                            warnings.push(format!(
                                                "✗ Sprint: '{}' not found in [{}]",
                                                sprint_val,
                                                available.join(", ")
                                            ));
                                        }
                                    },
                                    None => warnings.push("✗ Sprint: field not found".to_string()),
                                }
                            } else {
                                warnings.push(
                                    "✗ Sprint: no parent sprint or board filter set".to_string(),
                                );
                            }
                            // Project-level Type field, when the board has one.
                            // Accepts either spelling: some boards mirror the
                            // label, others mirror GitHub's issue type.
                            if let Some(field) = &type_field_clone {
                                if let Some(opt) = field
                                    .find_option(kind.label())
                                    .or_else(|| field.find_option(kind.issue_type()))
                                {
                                    if let Err(e) =
                                        gh::item::edit_field(iid, pid, &field.id, &opt.id)
                                    {
                                        let _ = e; // Type field is optional
                                    }
                                }
                            }
                            // Status → Ready for Dev
                            match &status_field_clone {
                                Some(field) => match field.find_option("Ready for Dev") {
                                    Some(opt) => {
                                        if let Err(e) =
                                            gh::item::edit_field(iid, pid, &field.id, &opt.id)
                                        {
                                            warnings.push(format!("✗ Status: {}", e));
                                        }
                                    }
                                    None => {
                                        let available: Vec<&str> =
                                            field.options.iter().map(|o| o.name.as_str()).collect();
                                        warnings.push(format!(
                                            "✗ Status: 'Ready for Dev' not found in [{}]",
                                            available.join(", ")
                                        ));
                                    }
                                },
                                None => warnings.push("✗ Status: field not found".to_string()),
                            }
                        } else if item_id.is_none() {
                            warnings
                                .push("✗ Skipped all field edits: no project item ID".to_string());
                        } else {
                            warnings.push("✗ Skipped all field edits: no project ID".to_string());
                        }

                        if let Some((
                            parent_repo,
                            parent_number,
                            dev_mentions,
                            move_params,
                            parent_item_id,
                            parent_stale_labels,
                        )) = parent_fail_info
                        {
                            // Bug exists - now move the parent back to In Progress.
                            if let Some((pitem_id, pid, fid, oid)) = move_params {
                                match gh::item::edit_field(&pitem_id, &pid, &fid, &oid) {
                                    Ok(_) => {
                                        let _ = tx.send(
                                            crate::app::ActionResult::ParentStatusChanged {
                                                item_id: pitem_id,
                                                status: Status::InProgress,
                                            },
                                        );
                                    }
                                    Err(e) => warnings.push(format!(
                                        "✗ Parent move to In Progress failed: {}",
                                        e
                                    )),
                                }
                            }
                            // The parent is going back for a fix, so its
                            // handoff labels no longer hold - clearing them is
                            // what lets it reappear as testable work later.
                            let (_, clear_failed) = strip_handoff_labels(
                                &tx,
                                &parent_item_id,
                                &parent_repo,
                                parent_number,
                                &parent_stale_labels,
                            );
                            if !clear_failed.is_empty() {
                                warnings.push(format!(
                                    "✗ Parent handoff label(s) not cleared: {} - remove them on GitHub or the next pass stays blocked",
                                    clear_failed.join(" + ")
                                ));
                            }

                            let comment = comment_with_cc(
                                &format!(
                                    "QA found a new bug on this ticket: {}\nMoving back to In Progress.",
                                    url
                                ),
                                &dev_mentions.join(" "),
                            );
                            if let Err(e) =
                                gh::issue::comment(&parent_repo, parent_number, &comment)
                            {
                                warnings.push(format!("✗ Parent comment failed: {}", e));
                            }
                        }

                        let new_item = crate::models::item::Item {
                            id: item_id.clone().unwrap_or_default(),
                            title: title.clone(),
                            status: crate::models::status::Status::ReadyForDev,
                            priority: if prio != "--" {
                                Some(prio.clone())
                            } else {
                                None
                            },
                            size: None,
                            stack: if stack != "--" {
                                Some(stack.clone())
                            } else {
                                None
                            },
                            sprint: parent_sprint.clone(),
                            assignees: selected_assignee.clone(),
                            labels: kind.labels().iter().map(|l| l.to_string()).collect(),
                            repository: Some(repo.clone()),
                            number: url
                                .split('/')
                                .next_back()
                                .and_then(|s| s.parse::<u32>().ok()),
                            content_type: crate::models::item::ContentType::Issue,
                            body: Some(body.clone()),
                            ..Default::default()
                        };

                        // The issue's own node ID, resolved once and reused by
                        // the native issue type and the sub-issue link.
                        let new_number = url
                            .split('/')
                            .next_back()
                            .and_then(|s| s.parse::<u32>().ok());
                        let content_node = match new_number {
                            Some(n) => match gh::issue::node_id(&repo, n) {
                                Ok(id) => Some(id),
                                Err(e) => {
                                    warnings.push(format!("✗ Issue node id: {}", e));
                                    None
                                }
                            },
                            None => None,
                        };

                        if let Some(node) = &content_node {
                            // GitHub's native Issue Type (Bug / Task).
                            if let Err(e) =
                                gh::issue::set_issue_type(&repo, node, kind.issue_type())
                            {
                                warnings.push(format!(
                                    "✗ Issue type '{}': {}",
                                    kind.issue_type(),
                                    e
                                ));
                            }
                            // Sub-issue link to the parent.
                            if let Some(p_node) = &parent_node_id {
                                if let Err(e) = gh::issue::add_sub_issue(p_node, node) {
                                    warnings.push(format!("✗ Sub-issue link: {}", e));
                                }
                            }
                        }

                        // Report the result BEFORE the Slack notification below -
                        // it waits ~10s for GitHub's priority automation, and the
                        // user shouldn't stare at "Creating bug report..." that long.
                        let _ = tx.send(crate::app::ActionResult::ItemCreated(
                            Box::new(new_item),
                            warnings,
                        ));

                        if let Some(ref webhook) = slack_url {
                            if notify_report {
                                // Only wait for GitHub's priority automation
                                // when the QA didn't set one - and poll instead
                                // of a blind 10-second sleep.
                                let mut final_prio = prio.clone();
                                if final_prio == "--" {
                                    if let Some(iid) = &item_id {
                                        for _ in 0..5 {
                                            std::thread::sleep(std::time::Duration::from_secs(2));
                                            if let Ok(Some(fetched_prio)) =
                                                gh::item::get_item_priority(iid)
                                            {
                                                final_prio = fetched_prio;
                                                break;
                                            }
                                        }
                                    }
                                }
                                // Skip unset values instead of rendering "(--, --)".
                                let mut meta: Vec<&str> = Vec::new();
                                if final_prio != "--" {
                                    meta.push(final_prio.as_str());
                                }
                                if stack != "--" {
                                    meta.push(stack.as_str());
                                }
                                let bug_slack_title = if meta.is_empty() {
                                    title.clone()
                                } else {
                                    format!("{} ({})", title, meta.join(", "))
                                };

                                let issue_number = url
                                    .split('/')
                                    .next_back()
                                    .and_then(|s| s.parse::<u32>().ok());
                                let slack_assignees: Vec<String> = selected_assignee
                                    .iter()
                                    .filter(|a| *a != &slack_user)
                                    .cloned()
                                    .collect();
                                if slack::notifier::notify_action(
                                    webhook,
                                    &slack_user,
                                    kind.slack_action(),
                                    &bug_slack_title,
                                    issue_number,
                                    Some(&repo),
                                    &slack_assignees,
                                    &slack_user_map,
                                    None,
                                )
                                .is_err()
                                {
                                    let _ = tx.send(crate::app::ActionResult::StatusMessage(
                                        "✗ Slack notify for bug report failed".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(crate::app::ActionResult::Error(format!(
                            "Failed to create bug: {}\nYour report draft is kept - press f/b to reopen and retry.",
                            e
                        )));
                    }
                }
            });
        }
        ConfirmAction::PostComment(item_idx) => {
            let item_idx = *item_idx;
            if let Some(item) = app.items.get(item_idx) {
                if let (Some(repo), Some(number)) = (item.repository.clone(), item.number) {
                    let text = app.comment_text.clone();

                    let item_title = item.title.clone();
                    let item_number = item.number;
                    let item_repo = item.repository.clone();
                    let slack_url = slack_url.clone();
                    let slack_user = slack_user.clone();
                    let slack_notify = slack_notify.clone();
                    let slack_user_map = slack_user_map.clone();

                    // Draft is cleared by the main loop on CommentPosted only,
                    // so a failed post keeps the text for retry.
                    app.go_back();

                    let tx = app.action_tx.clone();
                    app.loading = true;
                    app.set_status("Posting comment...");

                    std::thread::spawn(move || match gh::issue::comment(&repo, number, &text) {
                        Ok(()) => {
                            // Slack notification
                            if let Some(ref url) = slack_url {
                                if slack_notify.comment
                                    && slack::notifier::notify_action(
                                        url,
                                        &slack_user,
                                        "Comment",
                                        &item_title,
                                        item_number,
                                        item_repo.as_deref(),
                                        &[],
                                        &slack_user_map,
                                        Some(text.as_str()),
                                    )
                                    .is_err()
                                {
                                    let _ = tx.send(crate::app::ActionResult::StatusMessage(
                                        "✗ Slack notify for comment failed".to_string(),
                                    ));
                                }
                            }
                            let _ = tx.send(crate::app::ActionResult::CommentPosted);
                        }
                        Err(e) => {
                            let _ = tx.send(crate::app::ActionResult::Error(format!(
                                "Comment failed: {}\nYour draft is kept - press c to reopen and retry.",
                                e
                            )));
                        }
                    });
                } else {
                    app.popup = Popup::Error(
                        "This item has no linked GitHub issue (draft item) - comments can't be posted."
                            .to_string(),
                    );
                }
            }
        }
        ConfirmAction::ClearHandoffLabels(item_idx) => {
            let item_idx = *item_idx;
            let info = app.items.get(item_idx).map(|item| {
                (
                    item.id.clone(),
                    item.number,
                    item.repository.clone(),
                    App::handoff_labels_on(item),
                )
            });
            let Some((item_id, number_opt, repo_opt, stale_labels)) = info else {
                return;
            };
            let (Some(number), Some(repo)) = (number_opt, repo_opt) else {
                app.popup = Popup::Error(
                    "Clear handoff labels: this item has no linked GitHub issue (draft item)."
                        .to_string(),
                );
                return;
            };
            if stale_labels.is_empty() {
                app.popup = Popup::Error(
                    "Clear handoff labels: this item has no Ready-for-Staging or Ready-for-UAT label."
                        .to_string(),
                );
                return;
            }

            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status("Clearing handoff labels...");
            std::thread::spawn(move || {
                let (removed, failed) =
                    strip_handoff_labels(&tx, &item_id, &repo, number, &stale_labels);
                if failed.is_empty() {
                    let _ = tx.send(crate::app::ActionResult::Success(format!(
                        "Cleared {}. The ticket is testable again.",
                        removed.join(" + ")
                    )));
                } else {
                    let _ = tx.send(crate::app::ActionResult::Error(format!(
                        "Could not clear {}.\nRemove the label on GitHub, or the next pass stays blocked.",
                        failed.join(" + ")
                    )));
                }
            });
        }
        ConfirmAction::ClearCache => {
            app.cache.clear();
            for item in &mut app.items {
                item.body = None;
                item.comments.clear();
            }
            app.popup = Popup::Success(
                "Cache cleared!\n\nDisk cache and item details have been cleared.\nData will be re-fetched on next access."
                    .to_string(),
            );
        }
        ConfirmAction::SendStandupToSlack => {
            let report = crate::export::daily_standup_slack(app);
            let webhook_url = app.config.slack_webhook_url.clone().unwrap_or_default();
            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status("Sending daily standup to Slack...");
            std::thread::spawn(move || {
                match slack::notifier::notify_daily_standup(&webhook_url, &report) {
                    Ok(()) => {
                        let _ = tx.send(crate::app::ActionResult::Success(
                            "Daily standup sent to Slack!".to_string(),
                        ));
                    }
                    Err(e) => {
                        let _ = tx.send(crate::app::ActionResult::Error(format!(
                            "Slack send failed: {}",
                            e
                        )));
                    }
                }
            });
        }
        ConfirmAction::SendSprintReportToSlack => {
            let report = crate::export::sprint_report_slack(app);
            let webhook_url = app.config.slack_webhook_url.clone().unwrap_or_default();
            let tx = app.action_tx.clone();
            app.loading = true;
            app.set_status("Sending sprint report to Slack...");
            std::thread::spawn(move || {
                match slack::notifier::notify_sprint_report(&webhook_url, &report) {
                    Ok(()) => {
                        let _ = tx.send(crate::app::ActionResult::Success(
                            "Sprint report sent to Slack!".to_string(),
                        ));
                    }
                    Err(e) => {
                        let _ = tx.send(crate::app::ActionResult::Error(format!(
                            "Slack send failed: {}",
                            e
                        )));
                    }
                }
            });
        }
        ConfirmAction::MarkAllNotificationsRead => {
            for n in &mut app.notifications {
                n.unread = false; // Optimistic UI
            }
            let tx = app.action_tx.clone();
            app.set_status("Marking all notifications as read...");
            // The result used to be discarded while the UI claimed success, so
            // a failed call left the list looking read and nothing said otherwise.
            std::thread::spawn(move || match gh::notifications::mark_all_read() {
                Ok(()) => {
                    let _ = tx.send(crate::app::ActionResult::StatusMessage(
                        "All notifications marked as read".to_string(),
                    ));
                }
                Err(e) => {
                    let _ = tx.send(crate::app::ActionResult::Error(format!(
                        "Mark all as read failed: {}\nThe list will correct itself on the next fetch (r).",
                        e
                    )));
                }
            });
        }
        ConfirmAction::Quit => {
            app.running = false;
        }
        ConfirmAction::DeleteStackLead(stack) => {
            app.config.stack_leads.remove(stack);
            if app.stack_lead_cursor > 0 {
                app.stack_lead_cursor -= 1;
            }
            match app.config.save() {
                Ok(()) => app.set_status(&format!("Removed stack '{}'", stack)),
                Err(e) => app.popup = Popup::Error(format!("Failed to save config: {}", e)),
            }
        }
    }
}

fn handle_return_action(
    app: &mut App,
    item_idx: usize,
    slack_url: Option<String>,
    slack_user: String,
    slack_notify: crate::config::SlackNotifyConfig,
    slack_user_map: std::collections::HashMap<String, crate::config::SlackUserMapping>,
) {
    let info = app.items.get(item_idx).map(|item| {
        (
            item.number,
            item.repository.clone(),
            item.title.clone(),
            item.dev_assignees(&app.current_user),
            App::handoff_labels_on(item),
        )
    });
    let Some((number_opt, repo_opt, item_title, devs, stale_labels)) = info else {
        return;
    };
    let (Some(number), Some(repo)) = (number_opt, repo_opt) else {
        app.popup = Popup::Error(
            "Return: this item has no linked GitHub issue (draft item) - cannot move or comment."
                .to_string(),
        );
        return;
    };

    let dev_mentions: Vec<String> = devs.iter().map(|d| format!("@{}", d)).collect();
    // Wording works for bug tickets and feature tickets alike.
    let comment = comment_with_cc(
        "QA verification failed - issue not resolved. Returning to In Progress.",
        &dev_mentions.join(" "),
    );

    let slack_devs = devs;

    // Resolve the move params; the remote move runs in the thread after
    // revalidation (local state can be a poll interval stale).
    let move_params = match (app.status_field(), app.project_id.as_ref()) {
        (Some(field), Some(pid)) => field
            .find_option(Status::InProgress.label())
            .map(|opt| (pid.clone(), field.id.clone(), opt.id.clone())),
        _ => None,
    };
    let Some((pid, fid, oid)) = move_params else {
        app.set_status("Return failed: project fields not loaded yet - refresh and retry");
        return;
    };
    let item_id = match app.items.get(item_idx) {
        Some(item) => item.id.clone(),
        None => return,
    };
    // Optimistic local move (reverted via ParentStatusChanged on abort).
    if let Some(item) = app.items.get_mut(item_idx) {
        item.status = Status::InProgress;
    }

    let tx = app.action_tx.clone();
    app.loading = true;
    app.set_status("Returning to In Progress...");

    std::thread::spawn(move || {
        // Someone may already have returned it - don't double-comment.
        if let Ok(Some(remote)) = gh::item::get_item_status(&item_id) {
            let actual = Status::from_str(&remote);
            if actual == Status::InProgress {
                let _ = tx.send(crate::app::ActionResult::Error(
                    "Return aborted: the ticket is already In Progress on GitHub.\nNothing was changed."
                        .to_string(),
                ));
                return;
            }
        }
        if let Err(e) = gh::item::edit_field(&item_id, &pid, &fid, &oid) {
            let _ = tx.send(crate::app::ActionResult::Error(format!(
                "Return: ✗ Status move failed on GitHub: {}\nThe board will revert on the next sync.",
                e
            )));
            return;
        }

        // The ticket is going back for a fix, so it is no longer "passed":
        // clear the handoff labels. Leaving them behind is what made a
        // retested ticket vanish from My Tasks and blocked the next pass.
        let (cleared, clear_failed) =
            strip_handoff_labels(&tx, &item_id, &repo, number, &stale_labels);
        let label_note = handoff_note(&cleared, &clear_failed);

        match gh::issue::comment(&repo, number, &comment) {
            Ok(()) => {
                // Slack notification - a failed notify is reported, not swallowed
                let mut slack_note = "";
                if let Some(ref url) = slack_url {
                    if slack_notify.return_fail
                        && slack::notifier::notify_action(
                            url,
                            &slack_user,
                            "Returned",
                            &item_title,
                            Some(number),
                            Some(&repo),
                            &slack_devs,
                            &slack_user_map,
                            None,
                        )
                        .is_err()
                    {
                        slack_note = ", ✗ Slack notify failed";
                    }
                }
                let _ = tx.send(crate::app::ActionResult::Success(format!(
                    "Return: ✓ Comment posted (status → In Progress){}{}",
                    label_note, slack_note
                )));
            }
            Err(e) => {
                let _ = tx.send(crate::app::ActionResult::Error(format!(
                "Return: ✗ Comment failed: {}\nThe status move was still applied - comment manually or retry.",
                e
            )));
            }
        }
    });
}

/// One ticket in a Pass UAT batch, resolved before the worker starts.
struct UatTarget {
    item_id: String,
    number: u32,
    repo: String,
    comment: String,
}

/// Static description of a label-only QA handoff (Pass Dev / Pass STG).
struct PassLabelSpec {
    action_name: &'static str,
    slack_label: &'static str,
    label: &'static str,
    body: &'static str,
    notify_enabled: bool,
    /// Column the action is valid from - rechecked at execute time, since the
    /// board may have refreshed while the Confirm popup was open.
    expected_status: Status,
}

/// Shared implementation for the label-only QA handoffs: add label + comment
/// tagging leads, then Slack. No status move.
fn handle_pass_label_action(
    app: &mut App,
    item_idx: usize,
    spec: PassLabelSpec,
    slack_url: Option<String>,
    slack_user: String,
    slack_user_map: std::collections::HashMap<String, crate::config::SlackUserMapping>,
) {
    let info = app.items.get(item_idx).map(|item| {
        (
            item.number,
            item.repository.clone(),
            item.title.clone(),
            item.stack.clone(),
            item.dev_assignees(&app.current_user),
            item.id.clone(),
            item.status.clone(),
        )
    });
    let Some((number_opt, repo_opt, item_title, stack, devs, item_id, status)) = info else {
        return;
    };
    let (Some(number), Some(repo)) = (number_opt, repo_opt) else {
        app.popup = Popup::Error(format!(
            "{}: this item has no linked GitHub issue (draft item), so labels/comments can't be applied.",
            spec.action_name
        ));
        return;
    };

    // Execute-time guard: a background refresh while the Confirm was open may
    // have brought fresher state (moved column / label already added).
    if status != spec.expected_status {
        app.popup = Popup::Error(format!(
            "{} aborted: the item is now '{}' (expected '{}'). Nothing was changed.",
            spec.action_name,
            status.label(),
            spec.expected_status.label()
        ));
        return;
    }
    if app.item_has_label(item_idx, spec.label) {
        app.popup = Popup::Error(format!(
            "{} aborted: the item already has the {} label. Nothing was changed.",
            spec.action_name, spec.label
        ));
        return;
    }

    // Tag the stack lead (configurable via Settings). With no mapped lead the
    // comment tags the assignees instead: the old fallback tagged every lead
    // of every stack, so a ticket with no Stack pinged the whole team.
    let lead = app.config.leads_mention_string(stack.as_deref());
    let mentions = if lead.trim().is_empty() {
        devs.iter()
            .map(|d| format!("@{}", d))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        lead
    };
    let comment = comment_with_cc(spec.body, &mentions);

    // Slack tags the assignees plus the stack lead (empty for an unmapped stack).
    let mut slack_devs = devs;
    for l in app.config.get_leads(stack.as_deref()) {
        if !slack_devs.contains(&l) {
            slack_devs.push(l);
        }
    }

    // Optimistic: mirror the label locally so My Tasks exclusion and the
    // QA-action guards react now instead of after the next sync (which also
    // closes the window for accidental double-fires).
    app.add_label_local(item_idx, spec.label);

    let tx = app.action_tx.clone();
    app.loading = true;
    app.set_status(&format!("{}: sending...", spec.action_name));

    std::thread::spawn(move || {
        // Label first; if it fails, do nothing else - the optimistic local
        // label is reverted so the guard doesn't block a clean retry.
        if let Err(e) = gh::issue::add_label(&repo, number, spec.label) {
            let _ = tx.send(crate::app::ActionResult::RemoveLabelLocal {
                item_id,
                label: spec.label,
            });
            let _ = tx.send(crate::app::ActionResult::Error(format!(
                "{}: ✗ Label failed: {}\nNothing was applied - you can retry the action.",
                spec.action_name, e
            )));
            return;
        }

        if let Err(e) = gh::issue::comment(&repo, number, &comment) {
            // Label IS on GitHub now, so re-running the pass is blocked by the
            // guard on purpose - tell the user how to finish the handoff.
            let _ = tx.send(crate::app::ActionResult::Error(format!(
                "{}: ✓ Label added, ✗ Comment failed: {}\nPost the lead-tag comment manually with c.",
                spec.action_name, e
            )));
            return;
        }

        // Slack notification - a failed notify is reported, not swallowed
        let mut slack_note = "";
        if let Some(ref url) = slack_url {
            if spec.notify_enabled
                && slack::notifier::notify_action(
                    url,
                    &slack_user,
                    spec.slack_label,
                    &item_title,
                    Some(number),
                    Some(&repo),
                    &slack_devs,
                    &slack_user_map,
                    None,
                )
                .is_err()
            {
                slack_note = ", ✗ Slack notify failed";
            }
        }
        let _ = tx.send(crate::app::ActionResult::Success(format!(
            "{}: ✓ Label added, ✓ Comment posted{}",
            spec.action_name, slack_note
        )));
    });
}

/// Remove the QA handoff labels from an issue, from inside a worker thread.
///
/// Called whenever a ticket is sent back for a fix. Reports what happened as a
/// short suffix for the action's result message, and mirrors each successful
/// removal into local state so the "already passed" guard and the My Tasks
/// exclusion react immediately instead of after the next sync.
fn strip_handoff_labels(
    tx: &std::sync::mpsc::Sender<crate::app::ActionResult>,
    item_id: &str,
    repo: &str,
    number: u32,
    labels: &[&'static str],
) -> (Vec<&'static str>, Vec<String>) {
    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for label in labels {
        match gh::issue::remove_label(repo, number, label) {
            Ok(()) => {
                let _ = tx.send(crate::app::ActionResult::RemoveLabelLocal {
                    item_id: item_id.to_string(),
                    label,
                });
                removed.push(*label);
            }
            Err(e) => failed.push(format!("{} ({})", label, e)),
        }
    }
    (removed, failed)
}

/// Render the outcome of `strip_handoff_labels` as a result-message suffix.
fn handoff_note(removed: &[&'static str], failed: &[String]) -> String {
    let mut note = String::new();
    if !removed.is_empty() {
        note.push_str(&format!(", ✓ cleared {}", removed.join(" + ")));
    }
    if !failed.is_empty() {
        note.push_str(&format!(
            ", ✗ could not clear {} - remove it on GitHub or the next pass stays blocked",
            failed.join(" + ")
        ));
    }
    note
}

/// Build a comment body with an optional "cc" line - skipped entirely when
/// there is nobody to tag (avoids a dangling "cc " in the posted comment).
fn comment_with_cc(body: &str, mentions: &str) -> String {
    if mentions.trim().is_empty() {
        body.to_string()
    } else {
        format!("{}\ncc {}", body, mentions)
    }
}
