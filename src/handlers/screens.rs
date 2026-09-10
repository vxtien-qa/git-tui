use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::{App, ConfirmAction, Popup, Screen};
use crate::gh;
use crate::handlers::text_edit;
use crate::loader;
use crate::slack;
use crate::ui;
use crate::ui::theme;

/// Run a cursor-aware edit op on the focused bug-form text field (0-9).
/// Returns false when a non-text field is focused.
fn with_bug_text_field<F: FnOnce(&mut String, &mut usize)>(app: &mut App, f: F) -> bool {
    match app.bug_field_idx {
        0 => f(&mut app.bug_title, &mut app.bug_cursor),
        1 => f(&mut app.bug_description, &mut app.bug_cursor),
        2 => f(&mut app.bug_precondition, &mut app.bug_cursor),
        3 => f(&mut app.bug_test_data, &mut app.bug_cursor),
        4 => f(&mut app.bug_steps, &mut app.bug_cursor),
        5 => f(&mut app.bug_expected, &mut app.bug_cursor),
        6 => f(&mut app.bug_actual, &mut app.bug_cursor),
        7 => f(&mut app.bug_impact, &mut app.bug_cursor),
        8 => f(&mut app.bug_evidence, &mut app.bug_cursor),
        9 => f(&mut app.bug_security, &mut app.bug_cursor),
        _ => return false,
    }
    true
}

/// Run a cursor-aware edit op on the focused task-form text field (0-2).
fn with_task_text_field<F: FnOnce(&mut String, &mut usize)>(app: &mut App, f: F) -> bool {
    match app.task_field_idx {
        0 => f(&mut app.task_title, &mut app.task_cursor),
        1 => f(&mut app.task_description, &mut app.task_cursor),
        2 => f(&mut app.task_outcome, &mut app.task_cursor),
        _ => return false,
    }
    true
}

pub fn format_auth_status(auth: &gh::auth::AuthStatus) -> String {
    let mut lines = Vec::new();

    if !auth.gh_installed {
        // State A: gh CLI not installed
        lines.push(format!("{} GitHub CLI (gh) not found", theme::icon_fail()));
        lines.push(String::new());
        lines.push("GitHub CLI is required to connect to GitHub.".to_string());
        lines.push("Install it from: https://cli.github.com".to_string());
        lines.push(String::new());
        lines.push("  macOS:   brew install gh".to_string());
        lines.push("  Windows: winget install GitHub.cli".to_string());
        lines.push("  Linux:   sudo apt install gh  (or see link above)".to_string());
        lines.push(String::new());
        lines.push("After installing, press [Enter] to check again.".to_string());
    } else {
        lines.push(format!(
            "{} GitHub CLI installed ({})",
            theme::icon_ok(),
            auth.gh_version.as_deref().unwrap_or("?")
        ));

        if !auth.logged_in {
            // State B: gh installed, not logged in
            lines.push(format!("{} Not logged in to GitHub", theme::icon_fail()));
            lines.push(String::new());
            lines.push("Press [f] to login - a browser will open".to_string());
            lines.push("for you to authorize git-tui.".to_string());
        } else {
            // State C: logged in, check scopes
            lines.push(format!(
                "{} Logged in as @{}",
                theme::icon_ok(),
                auth.username.as_deref().unwrap_or("?")
            ));

            let missing = auth.missing_scopes();
            if !missing.is_empty() {
                lines.push(format!(
                    "{} Missing permissions: {}",
                    theme::icon_fail(),
                    missing.join(", ")
                ));
                lines.push(String::new());
                lines.push("Press [f] to fix - a browser will open briefly".to_string());
                lines.push("to authorize the extra permissions.".to_string());
            }
        }
    }

    lines.join("\n")
}

/// Get the appropriate fix command based on auth state.
fn auth_fix_command(auth: &gh::auth::AuthStatus) -> Option<String> {
    if !auth.gh_installed {
        None
    } else if !auth.logged_in {
        Some("gh auth login -w -s repo,project,read:org".to_string())
    } else {
        let missing = auth.missing_scopes();
        if missing.is_empty() {
            None
        } else {
            Some(format!("gh auth refresh -s {}", missing.join(",")))
        }
    }
}

pub fn handle_auth(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            app.auth_feedback = Some(format!(
                "{} Checking authentication...",
                theme::icon_spinner()
            ));
            let auth = gh::auth::check();
            if auth.is_ready() {
                app.current_user = auth.username.unwrap_or_default();
                app.config.current_user = app.current_user.clone();

                let config_file_exists = crate::config::Config::path().exists();
                if !config_file_exists {
                    app.screen = crate::app::Screen::Setup;
                } else if !app.config.slack_setup_offered {
                    app.screen = crate::app::Screen::SlackSetup;
                } else {
                    app.screen = crate::app::Screen::Menu;
                    app.loading = true;
                    app.set_status("Loading data...");
                    loader::spawn_background_refresh(app, false);
                }
            } else {
                app.auth_state = Some(auth.clone());
                app.auth_status_text = Some(format_auth_status(&auth));
                app.auth_feedback = Some(format!(
                    "{} Not ready yet - see status above",
                    theme::icon_fail()
                ));
            }
        }
        KeyCode::Char('f') => {
            // Auto-fix: spawn the right gh command based on current state
            let auth = app.auth_state.clone().unwrap_or_else(gh::auth::check);

            if !auth.gh_installed {
                app.auth_feedback = Some(format!(
                    "{} Install GitHub CLI first, then press [f]",
                    theme::icon_fail()
                ));
                return;
            }

            if let Some(cmd_str) = auth_fix_command(&auth) {
                app.auth_fix_in_progress = true;
                app.auth_feedback = Some(format!(
                    "{} Opening browser... complete the authorization there",
                    theme::icon_spinner()
                ));

                // Parse and spawn the command
                let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                if parts.len() >= 2 {
                    let program = parts[0].to_string();
                    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                    std::thread::spawn(move || {
                        let _ = std::process::Command::new(&program)
                            .args(&args)
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();
                    });
                }
            } else {
                app.auth_feedback = Some(format!(
                    "{} Already authenticated! Press [Enter] to continue.",
                    theme::icon_ok()
                ));
            }
        }
        KeyCode::Char('o') => {
            // Open gh CLI install page
            let _ = open::that("https://cli.github.com");
            app.auth_feedback = Some(format!(
                "{} Opened https://cli.github.com in browser",
                theme::icon_ok()
            ));
        }
        KeyCode::Char('c') => {
            // Context-aware copy: copy the right command
            let auth = app.auth_state.clone().unwrap_or_else(gh::auth::check);
            let cmd = auth_fix_command(&auth)
                .unwrap_or_else(|| "gh auth login -w -s repo,project,read:org".to_string());

            match crate::export::copy_to_clipboard(&cmd) {
                Ok(()) => {
                    app.auth_feedback = Some(format!("{} Copied: {}", theme::icon_ok(), cmd));
                }
                Err(e) => {
                    app.auth_feedback =
                        Some(format!("{} Clipboard error: {}", theme::icon_fail(), e));
                }
            }
        }
        KeyCode::Char('q') | KeyCode::Esc => {
            app.running = false;
        }
        _ => {}
    }
}

pub const MENU_ITEM_COUNT: usize = 8;

pub fn activate_menu_item(app: &mut App, idx: usize) {
    match idx {
        0 => app.goto(Screen::Board),
        1 => app.goto(Screen::MyTasks),
        2 => {
            if app.filter_status.is_empty() {
                app.filter_status = vec![
                    "!Done".to_string(),
                    "!Tech Complete".to_string(),
                    "!Ready for Release".to_string(),
                ];
            }
            app.apply_filters();
            app.goto(Screen::Search);
        }
        3 => app.goto(Screen::Settings),
        4 => {
            app.goto(Screen::Dashboard);
            loader::trigger_timeline_fetch(app);
        }
        5 => {
            app.list_selected = 0;
            app.list_scroll = 0;
            app.notifications.clear();
            app.loading = true;
            let tx = app.action_tx.clone();
            std::thread::spawn(move || match gh::notifications::fetch() {
                Ok(notifs) => {
                    let _ = tx.send(crate::app::ActionResult::NotificationsFetched(notifs));
                }
                // An Err must still send something - otherwise the spinner
                // stays on forever and the failure is invisible.
                Err(e) => {
                    let _ = tx.send(crate::app::ActionResult::Error(format!(
                        "Failed to fetch notifications: {}",
                        e
                    )));
                }
            });
            app.goto(Screen::Notifications);
        }
        6 => app.goto(Screen::Help),
        7 => app.running = false,
        _ => {}
    }
}

/// Route a terminal-native (bracketed) paste into the focused text field.
/// Without bracketed paste, a native paste replays as key events - every
/// Enter/Tab inside the pasted text fires form actions instead of inserting.
pub fn handle_paste(app: &mut App, text: &str) {
    let clean = text.replace("\r\n", "\n").replace('\r', "\n");
    let one_line = clean.replace('\n', " ");
    match app.screen.clone() {
        Screen::Comment(_) => {
            app.comment_discard_armed = false;
            text_edit::insert_str(&mut app.comment_text, &mut app.comment_cursor, &clean);
        }
        Screen::BugReport(_, _) => {
            with_bug_text_field(app, |s, cur| text_edit::insert_str(s, cur, &clean));
        }
        Screen::TaskForm(_) => {
            with_task_text_field(app, |s, cur| text_edit::insert_str(s, cur, &clean));
        }
        Screen::Setup => match app.setup_field_idx {
            0 => app.setup_project_url.push_str(one_line.trim()),
            1 => app.setup_bug_repo_url.push_str(one_line.trim()),
            _ => {}
        },
        Screen::SlackSetup => match app.slack_setup_field {
            0 => app.slack_webhook_input.push_str(one_line.trim()),
            1 => app.slack_bot_token_input.push_str(one_line.trim()),
            _ => {}
        },
        Screen::Search => {
            if app.search_field_idx == 0 {
                app.search_query.push_str(one_line.trim_end());
                app.apply_filters();
            }
        }
        Screen::AssigneePicker(_) => {
            app.assignee_search.push_str(one_line.trim());
            app.assignee_cursor = 0;
        }
        Screen::StackLeadsEdit if app.stack_lead_editing => {
            app.stack_lead_input.push_str(one_line.trim());
        }
        _ => {}
    }
}

pub fn handle_menu(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up | KeyCode::Char('k') => {
            if app.menu_selected > 0 {
                app.menu_selected -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.menu_selected < MENU_ITEM_COUNT - 1 {
                app.menu_selected += 1;
            }
        }
        KeyCode::Enter => activate_menu_item(app, app.menu_selected),
        KeyCode::Char('1') => {
            app.menu_selected = 0;
            activate_menu_item(app, 0);
        }
        KeyCode::Char('2') => {
            app.menu_selected = 1;
            activate_menu_item(app, 1);
        }
        KeyCode::Char('3') => {
            app.menu_selected = 2;
            activate_menu_item(app, 2);
        }
        KeyCode::Char('4') => {
            app.menu_selected = 3;
            activate_menu_item(app, 3);
        }
        KeyCode::Char('5') => {
            app.menu_selected = 4;
            activate_menu_item(app, 4);
        }
        KeyCode::Char('6') => {
            app.menu_selected = 5;
            activate_menu_item(app, 5);
        }
        KeyCode::Char('?') => {
            app.menu_selected = 6;
            activate_menu_item(app, 6);
        }
        KeyCode::Char('q') => {
            app.menu_selected = 7;
            activate_menu_item(app, 7);
        }
        KeyCode::Char('r') => {
            // Refresh from the menu too - without this, a failed initial sync
            // left the user with no obvious way to retry.
            loader::try_refresh(app, false);
        }
        _ => {}
    }
}

pub fn handle_settings(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('s') => {
            app.setup_project_url = format!(
                "https://github.com/orgs/{}/projects/{}",
                app.config.owner, app.config.project_number
            );
            app.setup_bug_repo_url = format!("https://github.com/{}", app.config.bug_repo);
            app.setup_field_idx = 0;
            app.goto(Screen::Setup);
        }
        KeyCode::Char('r') => {
            // Navigate to Auth screen instead of exiting TUI
            app.auth_status_text = None;
            app.last_auth_poll = None;
            app.goto(Screen::Auth);
        }
        KeyCode::Char('c') => {
            app.popup = Popup::Confirm {
                title: "Clear cache".to_string(),
                message: "Clear the disk cache and all loaded item details?".to_string(),
                on_confirm: ConfirmAction::ClearCache,
            };
        }
        KeyCode::Char('i') => {
            // Pre-fill existing webhook URL if any
            if let Some(ref url) = app.config.slack_webhook_url {
                app.slack_webhook_input = url.clone();
            }
            // Pre-fill existing bot token if any
            if let Some(ref token) = app.config.slack_bot_token {
                app.slack_bot_token_input = token.clone();
            }
            app.slack_setup_field = 0;
            app.slack_setup_status = None;
            app.goto(Screen::SlackSetup);
        }
        KeyCode::Char('n') => {
            app.slack_notify_cursor = 0;
            app.goto(Screen::SlackNotifyConfig);
        }
        KeyCode::Char('u') => {
            if app.config.is_slack_bot_configured() {
                // Fetch Slack members in a worker thread - the users.list
                // crawl (15s timeout per page) used to freeze the UI.
                if app.slack_members.is_empty() {
                    if let Some(token) = app.config.slack_bot_token.clone() {
                        app.set_status("Fetching Slack members...");
                        app.loading = true;
                        let tx = app.action_tx.clone();
                        std::thread::spawn(move || {
                            match crate::slack::users::fetch_users(&token) {
                                Ok(members) => {
                                    let _ = tx.send(crate::app::ActionResult::SlackMembersFetched(
                                        members,
                                    ));
                                }
                                Err(e) => {
                                    let _ = tx.send(crate::app::ActionResult::Error(format!(
                                        "Failed to fetch Slack members: {}",
                                        e
                                    )));
                                }
                            }
                        });
                    }
                }
                app.slack_map_cursor = 0;
                app.slack_map_picking = false;
                app.slack_map_search.clear();
                app.goto(Screen::SlackUserMap);
            } else {
                app.set_status("Configure Slack Bot Token first (Settings → [i])");
            }
        }
        KeyCode::Char('?') => app.goto(Screen::Help),
        KeyCode::Char('l') => {
            app.stack_lead_cursor = 0;
            app.stack_lead_editing = false;
            app.stack_lead_input.clear();
            app.goto(Screen::StackLeadsEdit);
        }
        KeyCode::Esc => {
            if app.previous_screens.is_empty() {
                if app.config.is_configured() && app.items.is_empty() {
                    app.screen = Screen::Menu;
                    app.loading = true;
                    app.set_status("Loading data...");
                    loader::spawn_background_refresh(app, false);
                } else {
                    app.screen = Screen::Menu;
                }
            } else {
                app.go_back();
            }
        }
        _ => {}
    }
}

pub fn handle_setup(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    match key {
        KeyCode::Tab | KeyCode::Down => {
            app.setup_field_idx = (app.setup_field_idx + 1) % 2;
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.setup_field_idx = if app.setup_field_idx == 0 { 1 } else { 0 };
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+V: paste from clipboard (clear field first to avoid double-paste)
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(text) = clipboard.get_text() {
                    let clean = text.trim().to_string();
                    match app.setup_field_idx {
                        0 => {
                            app.setup_project_url.clear();
                            app.setup_project_url.push_str(&clean);
                        }
                        1 => {
                            app.setup_bug_repo_url.clear();
                            app.setup_bug_repo_url.push_str(&clean);
                        }
                        _ => {}
                    }
                }
            }
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+U: clear current field
            match app.setup_field_idx {
                0 => app.setup_project_url.clear(),
                1 => app.setup_bug_repo_url.clear(),
                _ => {}
            }
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            let (owner, number) = ui::setup::parse_project_url(&app.setup_project_url);
            let bug_repo = ui::setup::parse_repo_url(&app.setup_bug_repo_url);

            let owner = match owner {
                Some(o) => o,
                None => {
                    app.popup = Popup::Error(
                        "Invalid Project URL.\nExample: https://github.com/orgs/MyOrg/projects/9"
                            .to_string(),
                    );
                    return;
                }
            };
            let project_num = match number {
                Some(n) if n > 0 => n,
                _ => {
                    app.popup =
                        Popup::Error("Could not parse project number from URL.".to_string());
                    return;
                }
            };

            // Switching owner/project must not reuse the old project's cache:
            // a fresh-looking cached project_id would route field mutations to
            // the WRONG project for up to an hour.
            let project_changed =
                app.config.owner != owner || app.config.project_number != project_num;
            app.config.owner = owner;
            app.config.project_number = project_num;
            if let Some(repo) = bug_repo {
                app.config.bug_repo = repo;
            }
            if project_changed {
                app.cache.clear();
                app.items.clear();
                app.fields.clear();
                app.project_id = None;
                app.search_results.clear();
                app.board_sprint_filter = None;
            }

            match app.config.save() {
                Ok(()) => {
                    app.popup = Popup::Success(format!("{} Config saved!", theme::icon_ok()));
                    app.previous_screens.clear();
                    // Route to Slack setup if not yet offered
                    if !app.config.slack_setup_offered {
                        app.screen = Screen::SlackSetup;
                    } else {
                        app.screen = Screen::Menu;
                        app.loading = true;
                        app.set_status("Config saved - loading data...");
                        loader::spawn_background_refresh(app, false);
                    }
                }
                Err(e) => {
                    app.popup = Popup::Error(format!("Failed to save config: {}", e));
                }
            }
        }
        KeyCode::Esc => {
            if app.previous_screens.is_empty() {
                // First run: a stray Esc must not silently kill the app.
                app.popup = Popup::Confirm {
                    title: "Quit".to_string(),
                    message: "Setup is not finished - quit git-tui?".to_string(),
                    on_confirm: ConfirmAction::Quit,
                };
            } else {
                app.go_back();
            }
        }
        KeyCode::Char(c) => match app.setup_field_idx {
            0 => app.setup_project_url.push(c),
            1 => app.setup_bug_repo_url.push(c),
            _ => {}
        },
        KeyCode::Backspace => match app.setup_field_idx {
            0 => {
                app.setup_project_url.pop();
            }
            1 => {
                app.setup_bug_repo_url.pop();
            }
            _ => {}
        },
        _ => {}
    }
}

pub fn handle_dashboard(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('s') => {
            app.cycle_sprint_filter();
        }
        KeyCode::Char('d') => {
            let report = crate::export::daily_standup_md(app);
            match crate::export::copy_to_clipboard(&report) {
                Ok(()) => app.set_status("[+] Daily standup (Markdown) copied!"),
                Err(e) => app.popup = Popup::Error(format!("Clipboard error: {}", e)),
            }
        }
        KeyCode::Char('D') => {
            let report = crate::export::daily_standup_text(app);
            match crate::export::copy_to_clipboard(&report) {
                Ok(()) => app.set_status("[+] Daily standup (Text) copied!"),
                Err(e) => app.popup = Popup::Error(format!("Clipboard error: {}", e)),
            }
        }
        KeyCode::Char('S') => {
            // Send Daily Standup to Slack - confirmed first: it posts to the
            // whole team channel and `S` is one shift-slip away from `s`.
            if app.config.is_slack_configured() && app.config.slack_notify.daily_standup {
                app.popup = Popup::Confirm {
                    title: "Send to Slack".to_string(),
                    message: "Send the daily standup to the team Slack channel?".to_string(),
                    on_confirm: ConfirmAction::SendStandupToSlack,
                };
            } else if !app.config.is_slack_configured() {
                app.set_status("Slack not configured - go to Settings [i]");
            } else {
                app.set_status("Slack daily standup notification is disabled");
            }
        }
        KeyCode::Char('e') => {
            let report = crate::export::sprint_report_md(app);
            match crate::export::copy_to_clipboard(&report) {
                Ok(()) => app.set_status("[+] Sprint report (Markdown) copied!"),
                Err(e) => app.popup = Popup::Error(format!("Clipboard error: {}", e)),
            }
        }
        KeyCode::Char('E') => {
            let report = crate::export::sprint_report_text(app);
            match crate::export::copy_to_clipboard(&report) {
                Ok(()) => app.set_status("[+] Sprint report (Text) copied!"),
                Err(e) => app.popup = Popup::Error(format!("Clipboard error: {}", e)),
            }
        }
        KeyCode::Char('X') => {
            // Send Sprint Report to Slack - confirmed first (posts to the team).
            if app.config.is_slack_configured() && app.config.slack_notify.sprint_report {
                app.popup = Popup::Confirm {
                    title: "Send to Slack".to_string(),
                    message: "Send the sprint report to the team Slack channel?".to_string(),
                    on_confirm: ConfirmAction::SendSprintReportToSlack,
                };
            } else if !app.config.is_slack_configured() {
                app.set_status("Slack not configured - go to Settings [i]");
            } else {
                app.set_status("Slack sprint report notification is disabled");
            }
        }
        KeyCode::Char('?') => app.goto(Screen::Help),
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

pub fn handle_bug_report(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    parent_idx: Option<usize>,
    is_fail: bool,
) {
    // Total fields: 0-9 text, 10-12 selectors, 13-16 scoring, 17 assignee = 18
    const TOTAL_FIELDS: usize = 18;

    match key {
        KeyCode::Tab => {
            app.bug_field_idx = (app.bug_field_idx + 1) % TOTAL_FIELDS;
            app.bug_cursor = usize::MAX; // end of the newly focused field
        }
        KeyCode::BackTab => {
            app.bug_field_idx = if app.bug_field_idx == 0 {
                TOTAL_FIELDS - 1
            } else {
                app.bug_field_idx - 1
            };
            app.bug_cursor = usize::MAX;
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            if app.bug_title.is_empty() {
                app.popup = Popup::Error("Title is required".to_string());
                return;
            }

            let priorities = ["--", "P0", "P1", "P2", "P3", "P4"];
            let prio = priorities[app.bug_priority_idx];
            // The Fail flow does more than create a bug - say so up front.
            let kind = app.bug_kind;
            let headline = format!(
                "Create {} \"{} {}\" ({})?",
                kind.label().to_lowercase(),
                kind.title_prefix(),
                app.bug_title,
                prio
            );
            let message = if is_fail && parent_idx.is_some() {
                format!(
                    "{}\nAlso: parent moves to In Progress, gets a comment linking this bug, and devs are tagged.",
                    headline
                )
            } else {
                headline
            };
            app.popup = Popup::Confirm {
                title: format!("Submit {}", kind.form_title()),
                message,
                on_confirm: ConfirmAction::SubmitBugReport(parent_idx, is_fail),
            };
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Ok(text) = crate::export::read_from_clipboard() {
                let clean = text.replace("\r\n", "\n");
                with_bug_text_field(app, |s, cur| text_edit::insert_str(s, cur, &clean));
            }
        }
        // Plain chars only - an unhandled Ctrl+<key> must not type a letter.
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            with_bug_text_field(app, |s, cur| text_edit::insert_char(s, cur, c));
        }
        KeyCode::Enter => {
            if app.bug_field_idx == 17 {
                app.assignee_search.clear();
                app.assignee_cursor = 0;
                app.goto(crate::app::Screen::AssigneePicker(
                    crate::app::AssigneeContext::Bug(parent_idx, is_fail),
                ));
            } else {
                with_bug_text_field(app, |s, cur| text_edit::insert_char(s, cur, '\n'));
            }
        }
        KeyCode::Backspace => {
            with_bug_text_field(app, text_edit::backspace);
        }
        KeyCode::Delete => {
            with_bug_text_field(app, text_edit::delete);
        }
        KeyCode::Home => {
            with_bug_text_field(app, |s, cur| text_edit::move_home(s, cur));
        }
        KeyCode::End => {
            with_bug_text_field(app, |s, cur| text_edit::move_end(s, cur));
        }
        KeyCode::Right => match app.bug_field_idx {
            // Text fields: move the cursor; selectors: cycle the option.
            0..=9 => {
                with_bug_text_field(app, |s, cur| text_edit::move_right(s, cur));
            }
            10 => app.bug_priority_idx = (app.bug_priority_idx + 1) % 6,
            11 => app.bug_stack_idx = (app.bug_stack_idx + 1) % 5,
            12 => app.bug_env_idx = (app.bug_env_idx + 1) % crate::app::App::BUG_ENVS.len(),
            13 => app.bug_broken_idx = (app.bug_broken_idx + 1) % 2,
            14 => app.bug_visible_idx = (app.bug_visible_idx + 1) % 2,
            15 => app.bug_workaround_idx = (app.bug_workaround_idx + 1) % 3,
            16 => app.bug_impact_prio_idx = (app.bug_impact_prio_idx + 1) % 2,
            _ => {}
        },
        KeyCode::Left => match app.bug_field_idx {
            0..=9 => {
                with_bug_text_field(app, |s, cur| text_edit::move_left(s, cur));
            }
            10 => {
                app.bug_priority_idx = if app.bug_priority_idx == 0 {
                    5
                } else {
                    app.bug_priority_idx - 1
                }
            }
            11 => {
                app.bug_stack_idx = if app.bug_stack_idx == 0 {
                    4
                } else {
                    app.bug_stack_idx - 1
                }
            }
            12 => {
                app.bug_env_idx = if app.bug_env_idx == 0 {
                    crate::app::App::BUG_ENVS.len() - 1
                } else {
                    app.bug_env_idx - 1
                }
            }
            13 => {
                app.bug_broken_idx = if app.bug_broken_idx == 0 {
                    1
                } else {
                    app.bug_broken_idx - 1
                }
            }
            14 => {
                app.bug_visible_idx = if app.bug_visible_idx == 0 {
                    1
                } else {
                    app.bug_visible_idx - 1
                }
            }
            15 => {
                app.bug_workaround_idx = if app.bug_workaround_idx == 0 {
                    2
                } else {
                    app.bug_workaround_idx - 1
                }
            }
            16 => {
                app.bug_impact_prio_idx = if app.bug_impact_prio_idx == 0 {
                    1
                } else {
                    app.bug_impact_prio_idx - 1
                }
            }
            _ => {}
        },
        // Up/Down: move by line inside a multiline text field; at the field's
        // first/last line (or on a selector row) move focus instead. Entering
        // a field from above lands on its first line, from below on its last.
        KeyCode::Up => {
            let mut moved = false;
            if matches!(app.bug_field_idx, 0..=9) {
                with_bug_text_field(app, |s, cur| moved = text_edit::move_up(s, cur));
            }
            if !moved {
                app.bug_field_idx = if app.bug_field_idx == 0 {
                    TOTAL_FIELDS - 1
                } else {
                    app.bug_field_idx - 1
                };
                app.bug_cursor = usize::MAX;
            }
        }
        KeyCode::Down => {
            let mut moved = false;
            if matches!(app.bug_field_idx, 0..=9) {
                with_bug_text_field(app, |s, cur| moved = text_edit::move_down(s, cur));
            }
            if !moved {
                app.bug_field_idx = (app.bug_field_idx + 1) % TOTAL_FIELDS;
                app.bug_cursor = 0;
            }
        }
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

pub fn handle_task_form(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    parent: Option<usize>,
) {
    const TOTAL_FIELDS: usize = 6;

    match key {
        KeyCode::Tab => {
            app.task_field_idx = (app.task_field_idx + 1) % TOTAL_FIELDS;
            app.task_cursor = usize::MAX; // end of the newly focused field
        }
        KeyCode::BackTab => {
            app.task_field_idx = if app.task_field_idx == 0 {
                TOTAL_FIELDS - 1
            } else {
                app.task_field_idx - 1
            };
            app.task_cursor = usize::MAX;
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            if app.task_title.is_empty() {
                app.popup = Popup::Error("Title is required".to_string());
                return;
            }

            let priorities = ["--", "P0", "P1", "P2", "P3", "P4"];
            let prio = priorities[app.task_priority_idx];
            app.popup = Popup::Confirm {
                title: "Submit Task".to_string(),
                message: format!("Create task \"[TASK] {}\" ({})?", app.task_title, prio),
                on_confirm: ConfirmAction::SubmitTask(parent),
            };
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Ok(text) = crate::export::read_from_clipboard() {
                let clean = text.replace("\r\n", "\n");
                with_task_text_field(app, |s, cur| text_edit::insert_str(s, cur, &clean));
            }
        }
        // Plain chars only - an unhandled Ctrl+<key> must not type a letter.
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            with_task_text_field(app, |s, cur| text_edit::insert_char(s, cur, c));
        }
        KeyCode::Enter => {
            if app.task_field_idx == 5 {
                app.assignee_search.clear();
                app.assignee_cursor = 0;
                app.goto(crate::app::Screen::AssigneePicker(
                    crate::app::AssigneeContext::Task,
                ));
            } else {
                with_task_text_field(app, |s, cur| text_edit::insert_char(s, cur, '\n'));
            }
        }
        KeyCode::Backspace => {
            with_task_text_field(app, text_edit::backspace);
        }
        KeyCode::Delete => {
            with_task_text_field(app, text_edit::delete);
        }
        KeyCode::Home => {
            with_task_text_field(app, |s, cur| text_edit::move_home(s, cur));
        }
        KeyCode::End => {
            with_task_text_field(app, |s, cur| text_edit::move_end(s, cur));
        }
        KeyCode::Right => match app.task_field_idx {
            0..=2 => {
                with_task_text_field(app, |s, cur| text_edit::move_right(s, cur));
            }
            3 => app.task_priority_idx = (app.task_priority_idx + 1) % 6,
            4 => app.task_stack_idx = (app.task_stack_idx + 1) % 5,
            _ => {}
        },
        KeyCode::Left => match app.task_field_idx {
            0..=2 => {
                with_task_text_field(app, |s, cur| text_edit::move_left(s, cur));
            }
            3 => {
                app.task_priority_idx = if app.task_priority_idx == 0 {
                    5
                } else {
                    app.task_priority_idx - 1
                }
            }
            4 => {
                app.task_stack_idx = if app.task_stack_idx == 0 {
                    4
                } else {
                    app.task_stack_idx - 1
                }
            }
            _ => {}
        },
        // Same line/field Up-Down navigation as the Bug Report form.
        KeyCode::Up => {
            let mut moved = false;
            if matches!(app.task_field_idx, 0..=2) {
                with_task_text_field(app, |s, cur| moved = text_edit::move_up(s, cur));
            }
            if !moved {
                app.task_field_idx = if app.task_field_idx == 0 {
                    TOTAL_FIELDS - 1
                } else {
                    app.task_field_idx - 1
                };
                app.task_cursor = usize::MAX;
            }
        }
        KeyCode::Down => {
            let mut moved = false;
            if matches!(app.task_field_idx, 0..=2) {
                with_task_text_field(app, |s, cur| moved = text_edit::move_down(s, cur));
            }
            if !moved {
                app.task_field_idx = (app.task_field_idx + 1) % TOTAL_FIELDS;
                app.task_cursor = 0;
            }
        }
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

pub fn handle_notifications(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up => {
            if app.list_selected > 0 {
                app.list_selected -= 1;
                if app.list_selected < app.list_scroll {
                    app.list_scroll = app.list_selected;
                }
            }
        }
        KeyCode::Down => {
            if app.list_selected + 1 < app.notifications.len() {
                app.list_selected += 1;
                if app.list_selected > app.list_scroll + 5 {
                    app.list_scroll = app.list_selected.saturating_sub(3);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(notif) = app.notifications.get(app.list_selected) {
                if let Some(url) = &notif.subject.url {
                    let parts: Vec<&str> = url.split('/').collect();
                    if let Some(last) = parts.last() {
                        if let Ok(number) = last.parse::<u32>() {
                            if let Some(item_idx) =
                                app.items.iter().position(|i| i.number == Some(number))
                            {
                                app.detail_scroll = 0;
                                app.goto(Screen::ItemDetail(item_idx));
                                loader::fetch_item_detail(app, item_idx);
                            } else {
                                // Fallback: construct GitHub URL and open in browser
                                let html_url = format!(
                                    "https://github.com/{}/issues/{}",
                                    notif.repository.full_name, number
                                );
                                let _ = open::that(&html_url);
                                app.status_message = Some(format!("Opened #{} in browser", number));
                            }
                        }
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            app.loading = true;
            let tx = app.action_tx.clone();
            std::thread::spawn(move || match gh::notifications::fetch() {
                Ok(notifs) => {
                    let _ = tx.send(crate::app::ActionResult::NotificationsFetched(notifs));
                }
                Err(e) => {
                    let _ = tx.send(crate::app::ActionResult::Error(format!(
                        "Failed to fetch notifications: {}",
                        e
                    )));
                }
            });
        }
        KeyCode::Char('m') => {
            if let Some(notif) = app.notifications.get_mut(app.list_selected) {
                notif.unread = false; // Optimistic UI
                let id = notif.id.clone();
                std::thread::spawn(move || {
                    let _ = gh::notifications::mark_read(&id);
                });
            }
        }
        KeyCode::Char('a') => {
            let unread = app.notifications.iter().filter(|n| n.unread).count();
            if unread == 0 {
                app.set_status("No unread notifications");
            } else {
                app.popup = Popup::Confirm {
                    title: "Mark all read".to_string(),
                    message: format!("Mark all {} unread notification(s) as read?", unread),
                    on_confirm: ConfirmAction::MarkAllNotificationsRead,
                };
            }
        }
        KeyCode::Esc => app.go_back(),
        _ => {}
    }
}

/// Handle input on the Slack Setup screen.
pub fn handle_slack_setup(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    match key {
        KeyCode::Tab | KeyCode::BackTab => {
            // Toggle between fields: 0=webhook, 1=bot_token
            app.slack_setup_field = 1 - app.slack_setup_field;
            app.slack_setup_status = None;
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(text) = clipboard.get_text() {
                    let trimmed = text.trim().to_string();
                    if app.slack_setup_field == 0 {
                        app.slack_webhook_input = trimmed;
                    } else {
                        app.slack_bot_token_input = trimmed;
                    }
                    app.slack_setup_status = None;
                }
            }
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            if app.slack_setup_field == 0 {
                app.slack_webhook_input.clear();
            } else {
                app.slack_bot_token_input.clear();
            }
            app.slack_setup_status = None;
        }
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
            let _ = open::that("https://api.slack.com/apps");
            app.slack_setup_status = Some(format!(
                "{} Opened Slack Apps page in browser",
                theme::icon_ok()
            ));
        }
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            let url = app.slack_webhook_input.trim().to_string();

            // Validate webhook URL
            if !url.is_empty() && !slack::webhook::validate_url(&url) {
                app.slack_setup_status = Some(format!(
                    "{} Invalid URL - must start with https://hooks.slack.com/services/",
                    theme::icon_fail()
                ));
                return;
            }

            // Validate bot token if provided
            let bot_token = app.slack_bot_token_input.trim().to_string();
            if !bot_token.is_empty() && !crate::slack::users::validate_bot_token(&bot_token) {
                app.slack_setup_status = Some(format!(
                    "{} Invalid Bot Token - must start with xoxb-",
                    theme::icon_fail()
                ));
                return;
            }

            let bot = if bot_token.is_empty() {
                None
            } else {
                Some(bot_token)
            };

            // Test the webhook in a thread - the HTTP call has a 10s timeout
            // and must not freeze the UI. finish_slack_setup runs when the
            // SlackWebhookTested result arrives.
            if !url.is_empty() {
                app.slack_setup_status =
                    Some(format!("{} Testing webhook...", theme::icon_spinner()));
                let tx = app.action_tx.clone();
                app.loading = true;
                std::thread::spawn(move || {
                    let error = slack::webhook::send_test(&url).err().map(|e| e.to_string());
                    let _ = tx.send(crate::app::ActionResult::SlackWebhookTested {
                        url,
                        bot_token: bot,
                        error,
                    });
                });
            } else {
                finish_slack_setup(app, None, bot);
            }
        }
        KeyCode::Esc => {
            app.config.slack_setup_offered = true;
            let _ = app.config.save();
            if app.previous_screens.is_empty() {
                app.screen = Screen::Menu;
                app.loading = true;
                app.set_status("Loading data...");
                loader::spawn_background_refresh(app, false);
            } else {
                app.go_back();
            }
        }
        KeyCode::Char(c) => {
            if app.slack_setup_field == 0 {
                app.slack_webhook_input.push(c);
            } else {
                app.slack_bot_token_input.push(c);
            }
            app.slack_setup_status = None;
        }
        KeyCode::Backspace => {
            if app.slack_setup_field == 0 {
                app.slack_webhook_input.pop();
            } else {
                app.slack_bot_token_input.pop();
            }
            app.slack_setup_status = None;
        }
        _ => {}
    }
}

/// Handle input on the Slack Notification Config screen.
pub fn handle_slack_notify_config(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up | KeyCode::Char('k') => {
            if app.slack_notify_cursor > 0 {
                app.slack_notify_cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.slack_notify_cursor < crate::config::SlackNotifyConfig::LABELS.len() - 1 {
                app.slack_notify_cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            app.config.slack_notify.toggle(app.slack_notify_cursor);
        }
        KeyCode::Esc => {
            // Auto-save on exit
            let _ = app.config.save();
            app.set_status("Notification settings saved");
            app.go_back();
        }
        _ => {}
    }
}

/// Handle input on the Stack Leads editor screen.
pub fn handle_stack_leads_edit(app: &mut App, key: KeyCode) {
    let mut stacks: Vec<String> = app.config.stack_leads.keys().cloned().collect();
    stacks.sort();
    let stack_count = stacks.len();

    if app.stack_lead_editing {
        // ─── Editing mode ───
        match key {
            KeyCode::Enter => {
                // Save the edited leads
                if let Some(stack_name) = stacks.get(app.stack_lead_cursor) {
                    let leads: Vec<String> = app
                        .stack_lead_input
                        .split(',')
                        .map(|s| s.trim().trim_start_matches('@').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if leads.is_empty() {
                        app.config.stack_leads.remove(stack_name);
                        app.set_status(&format!("Removed stack '{}'", stack_name));
                    } else {
                        app.config.stack_leads.insert(stack_name.clone(), leads);
                        app.set_status(&format!("Updated leads for '{}'", stack_name));
                    }
                }
                app.stack_lead_editing = false;
                app.stack_lead_input.clear();
            }
            KeyCode::Esc => {
                app.stack_lead_editing = false;
                app.stack_lead_input.clear();
            }
            KeyCode::Char(c) => {
                app.stack_lead_input.push(c);
            }
            KeyCode::Backspace => {
                app.stack_lead_input.pop();
            }
            _ => {}
        }
    } else {
        // ─── Navigation mode ───
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if app.stack_lead_cursor > 0 {
                    app.stack_lead_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if stack_count > 0 && app.stack_lead_cursor + 1 < stack_count {
                    app.stack_lead_cursor += 1;
                }
            }
            KeyCode::Enter => {
                // Start editing the selected stack's leads
                if let Some(stack_name) = stacks.get(app.stack_lead_cursor) {
                    if let Some(leads) = app.config.stack_leads.get(stack_name) {
                        app.stack_lead_input = leads.join(", ");
                    }
                    app.stack_lead_editing = true;
                }
            }
            KeyCode::Char('a') => {
                // Add a new stack - use a simple prompt via popup
                let existing = ["FE", "BE", "App UI", "App BE"];
                let next_stack = existing
                    .iter()
                    .find(|s| !app.config.stack_leads.contains_key(**s))
                    .map(|s| s.to_string());
                if let Some(stack_name) = next_stack {
                    app.config
                        .stack_leads
                        .insert(stack_name.clone(), Vec::new());
                    // Move cursor to the new stack
                    let mut new_stacks: Vec<String> =
                        app.config.stack_leads.keys().cloned().collect();
                    new_stacks.sort();
                    app.stack_lead_cursor = new_stacks
                        .iter()
                        .position(|s| s == &stack_name)
                        .unwrap_or(0);
                    app.stack_lead_editing = true;
                    app.stack_lead_input.clear();
                    app.set_status(&format!(
                        "Added stack '{}' - enter lead usernames",
                        stack_name
                    ));
                } else {
                    app.set_status("All default stacks already configured");
                }
            }
            KeyCode::Char('d') => {
                // Delete the selected stack (confirmed - removes its lead config)
                if let Some(stack_name) = stacks.get(app.stack_lead_cursor) {
                    let leads = app
                        .config
                        .stack_leads
                        .get(stack_name)
                        .map(|l| l.join(", "))
                        .unwrap_or_default();
                    app.popup = Popup::Confirm {
                        title: format!("Delete stack '{}'", stack_name),
                        message: if leads.is_empty() {
                            format!("Remove stack '{}' from the leads config?", stack_name)
                        } else {
                            format!("Remove stack '{}' and its leads ({})?", stack_name, leads)
                        },
                        on_confirm: ConfirmAction::DeleteStackLead(stack_name.clone()),
                    };
                }
            }
            KeyCode::Esc => {
                // Auto-save on exit
                let _ = app.config.save();
                app.set_status("Stack leads saved");
                app.go_back();
            }
            _ => {}
        }
    }
}

/// Apply and persist Slack setup once the (async) webhook test passed.
/// Also used directly when only a bot token is being saved (no HTTP test).
pub fn finish_slack_setup(app: &mut App, url: Option<String>, bot_token: Option<String>) {
    if let Some(u) = url {
        app.config.slack_webhook_url = Some(u);
        app.config.slack_enabled = true;
    }
    if let Some(t) = bot_token {
        app.config.slack_bot_token = Some(t);
    }
    app.config.slack_setup_offered = true;
    match app.config.save() {
        Ok(()) => {
            let mut msg = Vec::new();
            if app.config.slack_webhook_url.is_some() {
                msg.push("Webhook connected");
            }
            if app.config.slack_bot_token.is_some() {
                msg.push("Bot Token saved");
            }
            if msg.is_empty() {
                msg.push("Slack setup saved");
            }
            app.popup = Popup::Success(format!("{} {}", theme::icon_ok(), msg.join(" + ")));
            if app.previous_screens.is_empty() {
                app.screen = Screen::Menu;
                app.loading = true;
                app.set_status("Slack configured - loading data...");
                loader::spawn_background_refresh(app, false);
            } else if matches!(app.screen, Screen::SlackSetup) {
                app.go_back();
            }
        }
        Err(e) => {
            app.slack_setup_status = Some(format!("{} Save failed: {}", theme::icon_fail(), e));
        }
    }
}

/// Handle input on the Slack User Mapping screen.
pub fn handle_slack_user_map(app: &mut App, key: KeyCode) {
    let github_users = crate::ui::slack_user_map::collect_github_users(app);
    if github_users.is_empty() {
        if key == KeyCode::Esc {
            app.go_back();
        }
        return;
    }

    if app.slack_map_picking {
        // ─── Picker mode ───
        handle_user_map_picker(app, key, &github_users);
    } else {
        // ─── Main list mode ───
        match key {
            KeyCode::Up | KeyCode::Char('k') => {
                if app.slack_map_cursor > 0 {
                    app.slack_map_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.slack_map_cursor + 1 < github_users.len() {
                    app.slack_map_cursor += 1;
                }
            }
            KeyCode::Enter => {
                if !app.slack_members.is_empty() {
                    app.slack_map_picking = true;
                    app.slack_map_search.clear();
                    app.slack_map_member_cursor = 0;
                } else {
                    app.set_status("No Slack members loaded. Configure Bot Token first.");
                }
            }
            KeyCode::Char('x') => {
                if let Some(gh_user) = github_users.get(app.slack_map_cursor) {
                    if app.config.slack_user_map.remove(gh_user).is_some() {
                        app.set_status(&format!("Cleared mapping for {}", gh_user));
                    }
                }
            }
            KeyCode::Esc => {
                let _ = app.config.save();
                app.set_status("User mapping saved");
                app.go_back();
            }
            _ => {}
        }
    }
}

/// Handle input in the Slack member picker sub-mode.
fn handle_user_map_picker(app: &mut App, key: KeyCode, github_users: &[String]) {
    let filtered_len = app
        .slack_members
        .iter()
        .filter(|m| {
            if app.slack_map_search.is_empty() {
                return true;
            }
            let q = app.slack_map_search.to_lowercase();
            m.real_name.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q)
                || m.display_name.to_lowercase().contains(&q)
        })
        .count();

    match key {
        KeyCode::Up => {
            if app.slack_map_member_cursor > 0 {
                app.slack_map_member_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.slack_map_member_cursor + 1 < filtered_len {
                app.slack_map_member_cursor += 1;
            }
        }
        KeyCode::Enter => {
            let filtered: Vec<&crate::slack::users::SlackUser> = app
                .slack_members
                .iter()
                .filter(|m| {
                    if app.slack_map_search.is_empty() {
                        return true;
                    }
                    let q = app.slack_map_search.to_lowercase();
                    m.real_name.to_lowercase().contains(&q)
                        || m.name.to_lowercase().contains(&q)
                        || m.display_name.to_lowercase().contains(&q)
                })
                .collect();

            if let Some(selected) = filtered.get(app.slack_map_member_cursor) {
                if let Some(gh_user) = github_users.get(app.slack_map_cursor) {
                    let display = if selected.real_name.is_empty() {
                        selected.name.clone()
                    } else {
                        selected.real_name.clone()
                    };
                    app.config.slack_user_map.insert(
                        gh_user.clone(),
                        crate::config::SlackUserMapping {
                            slack_id: selected.id.clone(),
                            slack_display: display.clone(),
                        },
                    );
                    app.set_status(&format!("{} → {} ✓", gh_user, display));
                }
            }
            app.slack_map_picking = false;
        }
        KeyCode::Esc => {
            app.slack_map_picking = false;
        }
        KeyCode::Char(c) => {
            app.slack_map_search.push(c);
            app.slack_map_member_cursor = 0;
        }
        KeyCode::Backspace => {
            app.slack_map_search.pop();
            app.slack_map_member_cursor = 0;
        }
        _ => {}
    }
}

pub fn handle_assignee_picker(
    app: &mut crate::app::App,
    key: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
    context: &crate::app::AssigneeContext,
) {
    let mut users: Vec<String> = app.config.slack_user_map.keys().cloned().collect();
    users.sort();

    let query = app.assignee_search.to_lowercase();
    let filtered_users: Vec<&String> = users
        .iter()
        .filter(|u| u.to_lowercase().contains(&query))
        .collect();

    match key {
        // No j/k navigation here: this is a type-to-filter widget, and usernames
        // containing 'j' or 'k' would be untypable. Arrows/PageUp/PageDown navigate.
        crossterm::event::KeyCode::Up => {
            app.assignee_cursor = app.assignee_cursor.saturating_sub(1);
        }
        crossterm::event::KeyCode::Down => {
            if app.assignee_cursor + 1 < filtered_users.len() {
                app.assignee_cursor += 1;
            }
        }
        crossterm::event::KeyCode::PageUp => {
            app.assignee_cursor = app.assignee_cursor.saturating_sub(10);
        }
        crossterm::event::KeyCode::PageDown => {
            let limit = filtered_users.len().saturating_sub(1);
            app.assignee_cursor = (app.assignee_cursor + 10).min(limit);
        }
        crossterm::event::KeyCode::Char(' ') => {
            if let Some(selected_user) = filtered_users.get(app.assignee_cursor) {
                let user_str = selected_user.to_string();
                let assignees = match context {
                    crate::app::AssigneeContext::Bug(_, _) => &mut app.bug_assignees,
                    crate::app::AssigneeContext::Task => &mut app.task_assignees,
                };
                if let Some(pos) = assignees.iter().position(|x| x == &user_str) {
                    assignees.remove(pos);
                } else {
                    assignees.push(user_str);
                }
            }
        }
        crossterm::event::KeyCode::Enter | crossterm::event::KeyCode::Esc => {
            app.go_back();
        }
        crossterm::event::KeyCode::Backspace => {
            app.assignee_search.pop();
            app.assignee_cursor = 0;
        }
        crossterm::event::KeyCode::Char('u')
            if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            app.assignee_search.clear();
            app.assignee_cursor = 0;
        }
        crossterm::event::KeyCode::Char(c) => {
            app.assignee_search.push(c);
            app.assignee_cursor = 0;
        }
        _ => {}
    }
}
