mod app;
mod cache;
mod config;
mod export;
mod gh;
mod handlers;
mod loader;
mod models;
mod slack;
mod ui;

use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, Popup, Screen};

fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let mut app = App::new();
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    // Step 1: Show auth checking screen
    terminal.draw(|f| {
        let lines = vec![
            ratatui::text::Line::from(""),
            ratatui::text::Line::from(ratatui::text::Span::styled(
                "  Git TUI - Starting up...",
                ui::theme::title(),
            )),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from(ratatui::text::Span::styled(
                "  Checking gh CLI authentication...",
                ui::theme::normal(),
            )),
        ];
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(ui::theme::border())
            .title(ratatui::text::Span::styled(" Git TUI ", ui::theme::title()));
        f.render_widget(
            ratatui::widgets::Paragraph::new(lines).block(block),
            f.area(),
        );
    })?;

    // Step 2: Auth check (blocking but fast ~0.5s)
    let auth = gh::auth::check();
    app.last_auth_poll = Some(std::time::Instant::now()); // Prevent redundant re-check in main loop
    if !auth.is_ready() {
        app.auth_status_text = Some(handlers::screens::format_auth_status(&auth));
    } else {
        app.current_user = auth.username.unwrap_or_default();
        app.config.current_user = app.current_user.clone();

        // Only show Setup Wizard if config file does NOT exist on disk.
        let config_file_exists = crate::config::Config::path().exists();
        if !config_file_exists {
            app.screen = Screen::Setup;
        } else if !app.config.slack_setup_offered {
            app.screen = Screen::SlackSetup;
        } else {
            // Step 3: Load data with step-by-step progress and error handling
            let user = app.current_user.clone();

            // Helper: check if user pressed Ctrl+C / Esc / q during loading
            let check_interrupt = || -> bool {
                if event::poll(Duration::from_millis(10)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        return key.kind == KeyEventKind::Press
                            && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                                || (key.code == KeyCode::Char('c')
                                    && key.modifiers.contains(KeyModifiers::CONTROL)));
                    }
                }
                false
            };

            'load_loop: loop {
                // -- Step 3a: Loading items --
                draw_loading(
                    terminal,
                    &user,
                    &[("Authenticated", true), ("Loading items...", false)],
                )?;

                match loader::load_data_step_items(app) {
                    Ok(()) => {}
                    Err(e) => {
                        let msg = format!("{}", e);
                        let reset_info = gh::client::rate_limit_info();
                        draw_error_screen(terminal, &msg, &reset_info)?;
                        loop {
                            match event::read()? {
                                Event::Key(key) if key.kind == KeyEventKind::Press => {
                                    match key.code {
                                        KeyCode::Char('r') => continue 'load_loop,
                                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                                        _ => {}
                                    }
                                }
                                Event::Resize(_, _) => {
                                    draw_error_screen(terminal, &msg, &reset_info)?;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                if check_interrupt() {
                    return Ok(());
                }

                // -- Step 3b: Loading fields --
                draw_loading(
                    terminal,
                    &user,
                    &[
                        ("Authenticated", true),
                        (&format!("{} items loaded", app.items.len()), true),
                        ("Loading fields...", false),
                    ],
                )?;
                loader::load_data_step_fields(app);

                if check_interrupt() {
                    return Ok(());
                }

                // -- Step 3c: Loading project --
                draw_loading(
                    terminal,
                    &user,
                    &[
                        ("Authenticated", true),
                        (&format!("{} items loaded", app.items.len()), true),
                        (&format!("{} fields loaded", app.fields.len()), true),
                        ("Loading project config...", false),
                    ],
                )?;
                loader::load_data_step_project(app);

                // -- Step 3d: Finalize --
                app.search_results = (0..app.items.len()).collect();

                if let Some(sprint) = app.current_sprint_name() {
                    app.board_sprint_filter = Some(sprint);
                }
                app.last_refresh = Some(chrono::Utc::now());

                draw_loading(
                    terminal,
                    &user,
                    &[
                        ("Authenticated", true),
                        (&format!("{} items loaded", app.items.len()), true),
                        (&format!("{} fields loaded", app.fields.len()), true),
                        ("Project ready", true),
                    ],
                )?;

                app.screen = Screen::Menu;
                match app.status_drift_warning() {
                    Some(warn) => app.set_status(&format!("⚠ {}", warn)),
                    None => app.set_status(&format!("{} items loaded", app.items.len())),
                }
                break;
            }
        }
    }

    loop {
        // ── Auto-poll auth status when on Auth screen (every 10 seconds) ──
        if app.screen == Screen::Auth {
            let should_poll = match app.last_auth_poll {
                Some(last) => last.elapsed() >= Duration::from_secs(10),
                None => true,
            };
            if should_poll {
                let auth = gh::auth::check();
                // Set timer AFTER the blocking call so cooldown starts after completion.
                // If set before, and check() takes >10s, the next poll triggers immediately
                // creating an infinite freeze loop.
                app.last_auth_poll = Some(std::time::Instant::now());
                if auth.is_ready() {
                    app.current_user = auth.username.unwrap_or_default();
                    app.config.current_user = app.current_user.clone();

                    let config_file_exists = crate::config::Config::path().exists();
                    if !config_file_exists {
                        app.screen = Screen::Setup;
                    } else if !app.config.slack_setup_offered {
                        app.screen = Screen::SlackSetup;
                    } else {
                        app.screen = Screen::Menu;
                        app.set_status(&format!("Logged in as @{}", app.current_user));
                        loader::spawn_background_refresh(app, false);
                    }
                } else {
                    app.auth_status_text = Some(handlers::screens::format_auth_status(&auth));
                }
            }
        }

        // Check background receiver - drain ALL available events per frame
        if let Some(rx) = app.bg_rx.take() {
            let mut channel_disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(event) => match event {
                        crate::app::BackgroundEvent::RefreshSuccess {
                            items,
                            fields,
                            project_id,
                        } => {
                            // The option under each filter row's cursor, by
                            // value: the EPIC row is ordered by open work, so
                            // a sync can reorder it under the cursor.
                            let cursor_values: Vec<Option<String>> = (1..=App::SEARCH_FILTER_ROWS)
                                .map(|field| {
                                    handlers::search::get_filter_option_at(
                                        app,
                                        field,
                                        app.filter_cursor.get(field - 1).copied().unwrap_or(0),
                                    )
                                })
                                .collect();
                            let old_items = std::mem::replace(&mut app.items, items);
                            // Carry batch selection across the refresh -
                            // `selected` is #[serde(skip)], so without this a
                            // 3-minute auto-sync silently wipes an in-progress
                            // multi-select.
                            let selected_ids: Vec<&str> = old_items
                                .iter()
                                .filter(|i| i.selected && !i.id.is_empty())
                                .map(|i| i.id.as_str())
                                .collect();
                            if !selected_ids.is_empty() {
                                for item in &mut app.items {
                                    if selected_ids.contains(&item.id.as_str()) {
                                        item.selected = true;
                                    }
                                }
                            }
                            // Linked PRs load with a ticket's detail, not with
                            // the sync - carry them over or every auto-sync
                            // would blank the PR list of the ticket on screen.
                            let old_prs: std::collections::HashMap<
                                &str,
                                &Vec<crate::models::item::LinkedPr>,
                            > = old_items
                                .iter()
                                .filter(|o| !o.linked_prs.is_empty())
                                .map(|o| (o.id.as_str(), &o.linked_prs))
                                .collect();
                            if !old_prs.is_empty() {
                                for item in &mut app.items {
                                    if item.linked_prs.is_empty() {
                                        if let Some(prs) = old_prs.get(item.id.as_str()) {
                                            item.linked_prs = (*prs).clone();
                                        }
                                    }
                                }
                            }
                            for (i, value) in cursor_values.into_iter().enumerate() {
                                let Some(value) = value else {
                                    continue;
                                };
                                if let Some(pos) = app
                                    .search_filter_options(i + 1)
                                    .iter()
                                    .position(|(v, _)| *v == value)
                                {
                                    if let Some(c) = app.filter_cursor.get_mut(i) {
                                        *c = pos;
                                    }
                                }
                            }
                            handlers::helpers::remap_screen_indices(app, &old_items);
                            // Cache writes happen in the refresh worker thread -
                            // serializing 1000+ items inline here caused a
                            // visible hitch every auto-refresh.

                            app.fields = fields;

                            if let Some(pid) = project_id {
                                app.project_id = Some(pid);
                            }

                            // Re-apply search filters if any are active, otherwise reset.
                            // Cursor-preserving: an auto-sync must not yank the
                            // selection back to the top of the list.
                            if app.has_active_filters() {
                                app.reapply_filters_keep_cursor();
                            } else {
                                app.search_results = (0..app.items.len()).collect();
                            }

                            if app.board_sprint_filter.is_none() {
                                if let Some(sprint) = app.current_sprint_name() {
                                    app.board_sprint_filter = Some(sprint);
                                }
                            }

                            app.loading = false;
                            // A board column this build doesn't know about is
                            // otherwise invisible - items there just look like
                            // Backlog.
                            match app.status_drift_warning() {
                                Some(warn) => app.set_status(&format!("⚠ {}", warn)),
                                None => app.set_status(&format!(
                                    "Background sync complete: {} items",
                                    app.items.len()
                                )),
                            }
                            app.last_refresh = Some(chrono::Utc::now());
                            // Don't close bg_rx - timeline & created_at events still incoming
                        }
                        crate::app::BackgroundEvent::StatusHistoryFetched {
                            item_idx,
                            history,
                            comment_history,
                            label_history,
                        } => {
                            if let Some(item) = app.items.get_mut(item_idx) {
                                item.status_history = Some(history);
                                item.comment_history = Some(comment_history);
                                item.label_history = Some(label_history);
                            }
                        }
                        crate::app::BackgroundEvent::CreatedAtFetched {
                            item_idx,
                            created_at,
                        } => {
                            if let Some(item) = app.items.get_mut(item_idx) {
                                item.created_at = Some(created_at);
                            }
                        }
                        crate::app::BackgroundEvent::RefreshError(err) => {
                            app.loading = false;
                            // Show the tail of the error (the actual reason),
                            // not the head (which is just the command line).
                            let display = if err.len() > 60 {
                                let tail: String = err
                                    .chars()
                                    .rev()
                                    .take(57)
                                    .collect::<String>()
                                    .chars()
                                    .rev()
                                    .collect();
                                format!("...{}", tail)
                            } else {
                                err
                            };
                            app.set_status(&format!("Sync failed: {}", display));
                        }
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        channel_disconnected = true;
                        break;
                    }
                }
            }
            if !channel_disconnected {
                app.bg_rx = Some(rx);
            }
        }

        // Check action results (non-blocking write operations) - drain the
        // persistent queue so no result is ever dropped when several actions
        // run back-to-back.
        while let Ok(result) = app.action_rx.try_recv() {
            // StatusMessage is a side-note (e.g. a delayed Slack-notify result)
            // and must not clear the spinner of an action still in flight.
            // Neither do the per-ticket updates a batch sends while it runs.
            if !matches!(
                result,
                crate::app::ActionResult::StatusMessage(_)
                    | crate::app::ActionResult::ParentStatusChanged { .. }
                    | crate::app::ActionResult::RemoveLabelLocal { .. }
                    | crate::app::ActionResult::SprintChanged { .. }
            ) {
                app.loading = false;
            }
            // Result popups must not clobber an open Confirm popup (that would
            // silently discard the user's pending confirmation) - route the
            // message to the status bar instead.
            let confirm_open = app.popup.is_interactive();
            match result {
                crate::app::ActionResult::Success(msg) => {
                    if confirm_open {
                        app.set_status(&msg);
                    } else {
                        app.popup = Popup::Success(msg);
                    }
                }
                crate::app::ActionResult::Error(msg) => {
                    if confirm_open {
                        app.set_status(&format!("Error: {}", msg));
                    } else {
                        app.popup = Popup::Error(msg);
                    }
                }
                crate::app::ActionResult::TaskCreated(url) => {
                    // Clear the form only now that the task really exists -
                    // a failed create keeps the draft for retry.
                    app.task_title.clear();
                    app.task_description.clear();
                    app.task_outcome.clear();
                    app.task_field_idx = 0;
                    app.task_cursor = usize::MAX;
                    app.task_assignees.clear();
                    let msg = format!("Task created: {}", url);
                    if confirm_open {
                        app.set_status(&msg);
                    } else {
                        app.popup = Popup::Success(msg);
                    }
                }
                crate::app::ActionResult::CommentPosted => {
                    app.comment_text.clear();
                    app.comment_cursor = 0;
                    app.comment_draft_for = None;
                    app.comment_discard_armed = false;
                    if confirm_open {
                        app.set_status("✓ Comment posted!");
                    } else {
                        app.popup = Popup::Success("✓ Comment posted!".to_string());
                    }
                }
                crate::app::ActionResult::ParentStatusChanged { item_id, status } => {
                    if let Some(item) = app.items.iter_mut().find(|i| i.id == item_id) {
                        item.status = status;
                    }
                }
                crate::app::ActionResult::LinkedPrsFetched { item_id, result } => {
                    apply_linked_prs(app, &item_id, result);
                }
                crate::app::ActionResult::SprintChanged { item_id, sprint } => {
                    if let Some(item) = app.items.iter_mut().find(|i| i.id == item_id) {
                        item.sprint = Some(sprint);
                    }
                }
                crate::app::ActionResult::RemoveLabelLocal { item_id, label } => {
                    app.remove_label_local(&item_id, label);
                }
                crate::app::ActionResult::StatusMessage(msg) => {
                    app.set_status(&msg);
                }
                crate::app::ActionResult::SlackMembersFetched(members) => {
                    let count = members.len();
                    app.slack_members = members;
                    app.set_status(&format!("Loaded {} Slack members", count));
                }
                crate::app::ActionResult::SlackWebhookTested {
                    url,
                    bot_token,
                    error,
                } => match error {
                    None => handlers::screens::finish_slack_setup(app, Some(url), bot_token),
                    Some(e) => {
                        app.slack_setup_status = Some(format!(
                            "{} Webhook test failed: {}",
                            ui::theme::icon_fail(),
                            e
                        ));
                    }
                },
                crate::app::ActionResult::HistoryFetched(events) => {
                    app.history_events = events;
                }
                crate::app::ActionResult::ItemCreated(item, warnings) => {
                    // Bug form is cleared only on confirmed creation - a failed
                    // create keeps the draft for retry.
                    app.clear_bug_form();
                    // Without a project-item id the next refresh can't re-find
                    // this item (remap matches by id) - don't navigate into it.
                    let has_id = !item.id.is_empty();
                    let num = item
                        .number
                        .map(|n| format!("#{}", n))
                        .unwrap_or_else(|| "(draft)".to_string());
                    // Name what was filed: the form files bugs and enhancements.
                    let kind_label = item
                        .labels
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Ticket".to_string());
                    app.items.push(*item);
                    let new_idx = app.items.len() - 1;
                    // Respect active search filters instead of blindly showing
                    // the new item in filtered results.
                    if app.has_active_filters() {
                        app.reapply_filters_keep_cursor();
                    } else {
                        app.search_results.push(new_idx);
                    }
                    if confirm_open {
                        // Never clobber a pending confirmation or yank the
                        // user to another screen mid-decision.
                        app.set_status(&format!(
                            "{} {} created{}",
                            kind_label,
                            num,
                            if warnings.is_empty() {
                                ""
                            } else {
                                " (with warnings)"
                            }
                        ));
                    } else {
                        if warnings.is_empty() {
                            app.popup = Popup::Success("Ticket created successfully!".to_string());
                        } else {
                            let msg =
                                format!("Ticket created with warnings:\n{}", warnings.join("\n"));
                            app.popup = Popup::Error(msg);
                        }
                        if has_id {
                            app.goto(crate::app::Screen::ItemDetail(new_idx));
                        }
                    }
                }
                crate::app::ActionResult::NotificationsFetched(notifs) => {
                    app.notifications = notifs;
                    app.list_selected = 0;
                    app.list_scroll = 0;
                }
                crate::app::ActionResult::DeployChecked {
                    item_id,
                    label,
                    results,
                } => {
                    app.deploy_check_inflight.remove(&item_id);
                    let failures: Vec<String> = results
                        .iter()
                        .flat_map(|(_, states)| states.iter())
                        .filter_map(|(env, s)| match s {
                            crate::gh::issue::DeployState::Failed(e) => {
                                Some(format!("{}: {}", env, e))
                            }
                            _ => None,
                        })
                        .collect();
                    app.deploy_checks.extend(results);
                    let summary = app
                        .items
                        .iter()
                        .find(|i| i.id == item_id)
                        .map(|i| app.deploy_summary(i))
                        .unwrap_or_default();
                    if failures.is_empty() && summary.is_empty() {
                        app.set_status(&format!(
                            "Deploy check {}: no merged linked PR to check",
                            label
                        ));
                    } else if failures.is_empty() {
                        app.set_status(&format!("Deploy check {}: {}", label, summary));
                    } else if confirm_open {
                        app.set_status(&format!(
                            "Deploy check {} could not finish: {}",
                            label,
                            failures.join("; ")
                        ));
                    } else {
                        app.popup = Popup::Error(format!(
                            "Deploy check {} could not finish:\n{}",
                            label,
                            failures.join("\n")
                        ));
                    }
                }
                crate::app::ActionResult::ItemDetailFetched {
                    item_id,
                    body,
                    comments,
                    labels,
                    created_at,
                    linked_prs,
                } => {
                    app.detail_fetch_inflight.remove(&item_id);
                    if let Some(item) = app.items.iter_mut().find(|i| i.id == item_id) {
                        let comment_count = comments.len();
                        item.body = body;
                        item.comments = comments;
                        // Always replace - `gh issue view` labels are
                        // authoritative; the old empty-only guard meant Reload
                        // never picked up labels added/removed on GitHub.
                        item.labels = labels;
                        if created_at.is_some() {
                            item.created_at = created_at;
                        }
                        if let Some(result) = linked_prs {
                            apply_linked_prs(app, &item_id, result);
                        }
                        app.set_status(&format!(
                            "Loaded: {} comment{}",
                            comment_count,
                            if comment_count == 1 { "" } else { "s" }
                        ));
                    }
                }
            }
        }

        // Auto-refresh: if poll_interval has elapsed since the last refresh
        // (or last ATTEMPT - so a failed initial sync keeps retrying instead
        // of dying silently), trigger a background sync.
        if app.bg_rx.is_none() {
            let now = chrono::Utc::now();
            let anchor = match (app.last_refresh, app.last_refresh_attempt) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            let due = match anchor {
                Some(t) => {
                    now.signed_duration_since(t).num_seconds()
                        >= app.config.poll_interval_secs as i64
                }
                // No sync has ever run/been attempted: retry as soon as we're
                // past the startup screens.
                None => !matches!(
                    app.screen,
                    Screen::Auth | Screen::Setup | Screen::SlackSetup
                ),
            };
            if due {
                app.last_refresh_attempt = Some(now);
                loader::spawn_background_refresh(app, true);
            }
        }

        // Draw
        terminal.draw(|f| draw(f, app))?;

        // Handle input - drain every queued event before the next redraw.
        // Windows never delivers Event::Paste (crossterm's WinAPI input has
        // no bracketed paste), so a terminal-native paste replays as one key
        // event per character; redrawing after each one animates the paste
        // char-by-char and queues all other input behind it.
        if event::poll(Duration::from_millis(100))? {
            loop {
                let ev = event::read()?;
                handle_event(app, ev);
                if !app.running || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        if !app.running {
            break;
        }
    }

    Ok(())
}

/// Store a ticket's linked-PR lookup: the list on success, the error on
/// failure (keeping whatever was shown before).
fn apply_linked_prs(
    app: &mut App,
    item_id: &str,
    result: Result<Vec<crate::models::item::LinkedPr>, String>,
) {
    match result {
        Ok(prs) => {
            if let Some(item) = app.items.iter_mut().find(|i| i.id == item_id) {
                item.linked_prs = prs;
            }
            app.linked_prs_loaded.insert(item_id.to_string());
            app.linked_prs_failed.remove(item_id);
        }
        Err(e) => {
            app.linked_prs_failed.insert(item_id.to_string(), e);
        }
    }
}

/// Handle a single terminal event. Quit requests set `app.running = false`.
fn handle_event(app: &mut App, ev: Event) {
    // Terminal-native paste (bracketed) goes straight into the focused
    // text field instead of replaying as individual key events.
    if let Event::Paste(text) = &ev {
        handlers::screens::handle_paste(app, text);
        return;
    }
    // Mouse wheel scrolls wherever Up/Down already navigate/scroll.
    // Form screens are excluded so a stray wheel can't move focus.
    if let Event::Mouse(me) = &ev {
        let synth = match me.kind {
            event::MouseEventKind::ScrollUp => Some(KeyCode::Up),
            event::MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        if let Some(code) = synth {
            let scrollable = matches!(
                app.screen,
                Screen::Board
                    | Screen::MyTasks
                    | Screen::Search
                    | Screen::ItemDetail(_)
                    | Screen::Dashboard
                    | Screen::Settings
                    | Screen::Notifications
                    | Screen::Help
                    | Screen::BatchPicker(_)
                    | Screen::Menu
            );
            if scrollable && app.popup == Popup::None {
                dispatch_key(app, code, KeyModifiers::NONE);
            }
        }
        return;
    }
    if let Event::Key(key) = ev {
        // Windows: crossterm sends Press + Release for each key.
        // Only handle Press to prevent double-fire.
        if key.kind != KeyEventKind::Press {
            return;
        }

        // Global: Ctrl+C force quit
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            app.running = false;
            return;
        }

        // Handle popup first
        if app.popup != Popup::None {
            match &app.popup {
                Popup::Error(_) | Popup::Success(_) => {
                    app.popup = Popup::None;
                }
                Popup::ConfirmSprint {
                    choices,
                    selected,
                    on_confirm,
                    ..
                } => match key.code {
                    KeyCode::Left | KeyCode::Right => {
                        let n = choices.len().max(1);
                        let next = if key.code == KeyCode::Right {
                            (selected + 1) % n
                        } else {
                            (selected + n - 1) % n
                        };
                        if let Popup::ConfirmSprint { selected, .. } = &mut app.popup {
                            *selected = next;
                        }
                    }
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        let sprint = choices.get(*selected).and_then(|c| c.sprint.clone());
                        let action = on_confirm.clone().with_sprint(sprint);
                        app.popup = Popup::None;
                        handlers::actions::execute_confirm_action(app, &action);
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        app.popup = Popup::None;
                    }
                    _ => {}
                },
                Popup::SavePreset { .. } => {
                    handlers::search::handle_save_preset(app, key.code, key.modifiers)
                }
                Popup::Presets { .. } => handlers::search::handle_presets(app, key.code),
                Popup::CopyResults => match key.code {
                    KeyCode::Char('m') | KeyCode::Char('M') => {
                        handlers::search::copy_results(app, handlers::search::CopyFormat::Markdown)
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        handlers::search::copy_results(app, handlers::search::CopyFormat::Slack)
                    }
                    KeyCode::Esc => app.popup = Popup::None,
                    _ => {}
                },
                Popup::Confirm { on_confirm, .. } => match key.code {
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        let action = on_confirm.clone();
                        app.popup = Popup::None;
                        handlers::actions::execute_confirm_action(app, &action);
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        app.popup = Popup::None;
                    }
                    // Any other key is ignored so a stray keystroke
                    // neither confirms nor silently cancels.
                    _ => {}
                },
                Popup::None => {}
            }
            return;
        }

        // Screen-specific input
        dispatch_key(app, key.code, key.modifiers);
    }
}

/// Route a key (real or synthesized from mouse wheel) to the active screen.
fn dispatch_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    match &app.screen {
        Screen::Auth => handlers::screens::handle_auth(app, code),
        Screen::Menu => handlers::screens::handle_menu(app, code),
        Screen::Board => handlers::board::handle_board(app, code),
        Screen::ItemDetail(idx) => {
            let idx = *idx;
            handlers::detail::handle_detail(app, code, idx);
        }
        Screen::MyTasks => handlers::board::handle_my_tasks(app, code),
        Screen::Search => handlers::search::handle_search(app, code, modifiers),
        Screen::BatchPicker(kind) => {
            let kind = *kind;
            handlers::batch::handle_batch_picker(app, code, kind);
        }
        Screen::QaActions(idx) => {
            let idx = *idx;
            handlers::detail::handle_qa_actions(app, code, idx);
        }
        Screen::MoveDialog(indices) => {
            let indices = indices.clone();
            handlers::detail::handle_move_dialog(app, code, &indices);
        }
        Screen::BugReport(parent, is_fail) => {
            let parent = *parent;
            let is_fail = *is_fail;
            handlers::screens::handle_bug_report(app, code, modifiers, parent, is_fail);
        }
        Screen::TaskForm(parent) => {
            let parent = *parent;
            handlers::screens::handle_task_form(app, code, modifiers, parent)
        }
        Screen::AssigneePicker(ctx) => {
            let ctx = ctx.clone();
            handlers::screens::handle_assignee_picker(app, code, modifiers, &ctx);
        }
        Screen::Settings => match code {
            KeyCode::Up => {
                app.settings_scroll = app.settings_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                app.settings_scroll += 1;
            }
            _ => handlers::screens::handle_settings(app, code),
        },
        Screen::Setup => handlers::screens::handle_setup(app, code, modifiers),
        Screen::SlackSetup => handlers::screens::handle_slack_setup(app, code, modifiers),
        Screen::SlackNotifyConfig => handlers::screens::handle_slack_notify_config(app, code),
        Screen::SlackUserMap => handlers::screens::handle_slack_user_map(app, code),
        Screen::StackLeadsEdit => handlers::screens::handle_stack_leads_edit(app, code),
        Screen::Dashboard => match code {
            KeyCode::Up => {
                app.dashboard_scroll = app.dashboard_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                app.dashboard_scroll += 1;
            }
            _ => handlers::screens::handle_dashboard(app, code),
        },
        Screen::Comment(idx) => {
            let idx = *idx;
            handlers::detail::handle_comment(app, code, modifiers, idx);
        }
        Screen::Notifications => handlers::screens::handle_notifications(app, code),
        Screen::History(idx) => {
            let idx = *idx;
            handlers::detail::handle_history(app, code, idx);
        }
        Screen::Help => match code {
            KeyCode::Up => {
                app.help_scroll = app.help_scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                app.help_scroll += 1;
            }
            KeyCode::Esc => {
                app.help_scroll = 0;
                app.go_back();
            }
            _ => {}
        },
    }
}

/// Render the location bar: `Menu > Board > #412 > QA actions`.
///
/// Truncates from the LEFT, because the tail (where you actually are) is the
/// part that must never be cut off.
fn render_breadcrumb(f: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    const INDICATOR_RESERVE: usize = 18; // loading / last-sync text on the right
    let sep = format!(" {} ", ui::theme::icon_right_arrow());
    let trail = app.breadcrumb();
    let avail = (area.width as usize).saturating_sub(INDICATOR_RESERVE + 2);

    let mut shown: Vec<String> = trail.clone();
    let mut text = shown.join(&sep);
    let mut clipped = false;
    while ui::utils::display_width(&text) > avail && shown.len() > 1 {
        shown.remove(0);
        clipped = true;
        text = shown.join(&sep);
    }
    if clipped {
        text = format!("...{}{}", sep, text);
    }
    // Still too wide (a very narrow terminal): keep the tail readable.
    let text = ui::utils::trunc_left(&text, avail);

    let mut spans = vec![Span::raw(" ")];
    let last = shown.len().saturating_sub(1);
    // Rebuild as spans so the current screen stands out from the trail.
    if clipped || ui::utils::display_width(&shown.join(&sep)) > avail {
        spans.push(Span::styled(text, ui::theme::dim()));
    } else {
        for (i, part) in shown.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(sep.clone(), ui::theme::border()));
            }
            if i == last {
                spans.push(Span::styled(part.clone(), ui::theme::title()));
            } else {
                spans.push(Span::styled(part.clone(), ui::theme::dim()));
            }
        }
    }
    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let full_area = f.area();

    // ── Minimum size guard ──
    let min_w: u16 = 60;
    let min_h: u16 = 10;
    if full_area.width < min_w || full_area.height < min_h {
        let msg = format!(
            "Terminal too small ({}x{})\nPlease resize to at least {}x{}",
            full_area.width, full_area.height, min_w, min_h
        );
        let lines: Vec<ratatui::text::Line> = msg
            .lines()
            .map(|l| {
                ratatui::text::Line::from(ratatui::text::Span::styled(
                    l.to_string(),
                    ui::theme::warning(),
                ))
            })
            .collect();
        f.render_widget(
            ratatui::widgets::Paragraph::new(lines)
                .alignment(ratatui::layout::Alignment::Center)
                .block(
                    ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .border_style(ui::theme::warning())
                        .title(ratatui::text::Span::styled(
                            " Resize ",
                            ui::theme::warning(),
                        )),
                ),
            full_area,
        );
        return;
    }

    // ── Location bar ──
    // One line at the top, on every screen but the sign-in wall: the trail of
    // where the user is (Board > #412 > QA actions). The right-hand side of
    // this line is left free for the loading / last-sync indicator.
    let body_area = if matches!(app.screen, Screen::Auth) {
        full_area
    } else {
        render_breadcrumb(f, app, full_area);
        ratatui::layout::Rect {
            x: full_area.x,
            y: full_area.y + 1,
            width: full_area.width,
            height: full_area.height.saturating_sub(1),
        }
    };

    match &app.screen {
        Screen::Auth => ui::auth::render(f, app, full_area),
        _ => {
            let show_sidebar = body_area.width >= 80;
            let sidebar_w: u16 = if body_area.width >= 100 {
                28
            } else if body_area.width >= 80 {
                22
            } else {
                0
            };

            if show_sidebar {
                let panels = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Horizontal)
                    .constraints([
                        ratatui::layout::Constraint::Length(sidebar_w),
                        ratatui::layout::Constraint::Min(1),
                    ])
                    .split(body_area);

                let sidebar_area = panels[0];
                let content_area = panels[1];

                ui::menu::render_sidebar(f, app, sidebar_area);
                render_content(f, app, content_area, true);
            } else {
                render_content(f, app, body_area, false);
            }
        }
    }

    // ── Global status bar ──
    // status_message used to be rendered only on Menu/Dashboard, which made
    // several flows silently swallow their feedback (double-Esc warning,
    // cooldowns, async errors while a Confirm was open). Shown on every
    // screen for a few seconds, overlaying the bottom line.
    const STATUS_BAR_SECS: u64 = 6;
    if let Some(at) = app.status_message_at {
        if at.elapsed() > Duration::from_secs(STATUS_BAR_SECS) {
            app.status_message = None;
            app.status_message_at = None;
        }
    }
    if let Some(ref msg) = app.status_message {
        let area = f.area();
        let bar = ratatui::layout::Rect {
            x: 0,
            y: area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        };
        let text = ui::utils::trunc(
            &ui::utils::sanitize_glyphs(msg),
            area.width.saturating_sub(3) as usize,
        );
        f.render_widget(ratatui::widgets::Clear, bar);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {}", text),
                ui::theme::highlight(),
            ))),
            bar,
        );
    }

    // Render popup overlay
    match &app.popup {
        Popup::Error(msg) => ui::dialog::render_error(f, msg),
        Popup::Success(msg) => ui::dialog::render_success(f, msg),
        Popup::Confirm { title, message, .. } => ui::dialog::render_confirm(f, title, message),
        Popup::ConfirmSprint {
            title,
            message,
            choices,
            selected,
            ..
        } => ui::dialog::render_confirm_sprint(f, title, message, choices, *selected),
        Popup::SavePreset { name } => ui::dialog::render_text_prompt(
            f,
            "Save filters",
            "Name these filters (same name replaces):",
            name,
        ),
        Popup::Presets { selected } => {
            let names: Vec<String> = app
                .config
                .search_presets
                .iter()
                .map(|p| p.name.clone())
                .collect();
            ui::dialog::render_pick_list(f, "Saved filters", &names, *selected)
        }
        Popup::CopyResults => ui::dialog::render_choice(
            f,
            "Copy results",
            &format!("Copy {} result(s) as:", app.search_results.len()),
            &[
                ("m", "Markdown (GitHub, Notion)"),
                ("s", "Slack (linked titles)"),
            ],
        ),
        Popup::None => {}
    }

    // Loading indicator in status bar
    if app.loading {
        let loading_line = Line::from(Span::styled(
            " Loading...",
            ui::theme::warning().add_modifier(ratatui::style::Modifier::BOLD),
        ));
        let area = f.area();
        let loading_area = ratatui::layout::Rect {
            x: area.width.saturating_sub(16),
            y: 0,
            width: 15,
            height: 1,
        };
        f.render_widget(Paragraph::new(loading_line), loading_area);
    } else if let Some(last) = app.last_refresh {
        // Global Last Fetched indicator
        let elapsed = chrono::Utc::now().signed_duration_since(last).num_minutes();
        let icon = ui::theme::icon_spinner();
        let text = if elapsed == 0 {
            format!(" {} just now ", icon)
        } else {
            format!(" {} {}m ago ", icon, elapsed)
        };
        let line = ratatui::text::Line::from(ratatui::text::Span::styled(text, ui::theme::dim()));
        let width = line.width() as u16;
        let area = f.area();
        let render_area = ratatui::layout::Rect {
            x: area.width.saturating_sub(width),
            y: 0,
            width,
            height: 1,
        };
        f.render_widget(ratatui::widgets::Paragraph::new(line), render_area);
    }
}

/// Render the active screen content in the given area.
/// No outer wrapper block here - every subpage draws its own frame, and the
/// old double border cost 4 columns/rows and nested two competing frames.
fn render_content(
    f: &mut ratatui::Frame,
    app: &mut App,
    content_area: ratatui::layout::Rect,
    sidebar_visible: bool,
) {
    let inner_area = content_area;

    match &app.screen {
        // Below the sidebar breakpoint the menu list has nowhere else to go,
        // and the Menu screen without it left the user navigating blind by
        // number keys.
        Screen::Menu => ui::menu::render(f, app, inner_area, !sidebar_visible),
        Screen::Board => ui::board::render(f, app, inner_area),
        Screen::ItemDetail(idx) => ui::detail::render(f, app, *idx, inner_area),
        Screen::MyTasks => ui::my_tasks::render(f, app, inner_area),
        Screen::Search => ui::search::render(f, app, inner_area),
        Screen::BatchPicker(kind) => ui::batch_picker::render(f, app, *kind, inner_area),
        Screen::QaActions(idx) => {
            ui::detail::render(f, app, *idx, inner_area);
            ui::qa_actions::render(f, app, *idx);
        }
        Screen::MoveDialog(indices) => {
            let indices = indices.clone();
            ui::board::render(f, app, inner_area);
            ui::dialog::render_move(f, app, &indices);
        }
        Screen::BugReport(parent, _) => ui::bug_report::render(f, app, *parent, inner_area),
        Screen::TaskForm(_) => ui::task_form::render(f, app, inner_area),
        Screen::AssigneePicker(context) => {
            // Render the underlying form first
            match context {
                crate::app::AssigneeContext::Bug(parent, _) => {
                    ui::bug_report::render(f, app, *parent, inner_area);
                }
                crate::app::AssigneeContext::Task => {
                    ui::task_form::render(f, app, inner_area);
                }
            }
            // Then overlay the picker
            ui::assignee_picker::render(f, app, context, inner_area);
        }
        Screen::Settings => ui::settings::render(f, app, inner_area),
        Screen::Setup => ui::setup::render(f, app, inner_area),
        Screen::SlackSetup => ui::slack_setup::render(f, app, inner_area),
        Screen::SlackNotifyConfig => ui::slack_notify_config::render(f, app, inner_area),
        Screen::SlackUserMap => ui::slack_user_map::render(f, app, inner_area),
        Screen::StackLeadsEdit => ui::stack_leads::render(f, app, inner_area),
        Screen::Dashboard => ui::dashboard::render(f, app, inner_area),
        Screen::Comment(idx) => ui::comment::render(f, app, *idx, inner_area),
        Screen::Notifications => ui::notifications::render(f, app, inner_area),
        Screen::History(idx) => ui::history::render(f, app, *idx, inner_area),
        Screen::Help => ui::help::render(f, app, inner_area),
        Screen::Auth => {} // handled in draw()
    }
}

// ── Loading Progress ──────────────────────────────────────────

/// Draw a loading progress screen with step-by-step checklist.
fn draw_loading(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    _user: &str,
    steps: &[(&str, bool)],
) -> Result<()> {
    terminal.draw(|f| {
        let mut lines = vec![
            ratatui::text::Line::from(""),
            ratatui::text::Line::from(ratatui::text::Span::styled(
                "  Git TUI -- Starting up...",
                ui::theme::title(),
            )),
            ratatui::text::Line::from(""),
        ];

        for (label, done) in steps {
            if label.is_empty() {
                lines.push(ratatui::text::Line::from(""));
                continue;
            }
            let marker = if *done { "[ok] " } else { "[..] " };
            let style = if *done {
                ui::theme::success()
            } else {
                ui::theme::normal()
            };
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("  {} {}", marker, label),
                style,
            )));
        }

        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(ui::theme::border())
            .title(ratatui::text::Span::styled(" Git TUI ", ui::theme::title()));
        f.render_widget(
            ratatui::widgets::Paragraph::new(lines).block(block),
            f.area(),
        );
    })?;
    Ok(())
}

/// Draw a blocking error screen with retry/quit options.
fn draw_error_screen(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    error_msg: &str,
    reset_info: &str,
) -> Result<()> {
    terminal.draw(|f| {
        let area = f.area();
        let max_w = (area.width as usize).saturating_sub(6);

        let mut lines = vec![
            ratatui::text::Line::from(""),
            ratatui::text::Line::from(ratatui::text::Span::styled(
                format!(
                    "  {} ERROR - Failed to load data",
                    ui::theme::icon_warning()
                ),
                ui::theme::warning(),
            )),
            ratatui::text::Line::from(""),
        ];

        for wrapped in ui::utils::word_wrap(error_msg, max_w.max(20)) {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("  {}", wrapped),
                ui::theme::normal(),
            )));
        }

        lines.push(ratatui::text::Line::from(""));

        for wrapped in ui::utils::word_wrap(reset_info, max_w.max(20)) {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("  {}", wrapped),
                ui::theme::highlight(),
            )));
        }

        lines.push(ratatui::text::Line::from(""));
        lines.push(ratatui::text::Line::from(""));
        lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
            "  [r] Retry     [q] Quit",
            ui::theme::dim(),
        )));

        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(ui::theme::warning())
            .title(ratatui::text::Span::styled(" Error ", ui::theme::warning()));
        f.render_widget(ratatui::widgets::Paragraph::new(lines).block(block), area);
    })?;
    Ok(())
}

#[cfg(test)]
mod render_tests_support {
    use super::*;
    use crate::models::item::Item;
    use crate::models::status::Status;

    /// A populated app so the renderers exercise real rows, not empty states.
    pub fn app_with_items() -> App {
        let mut app = App::new();
        app.current_user = "qa-user".to_string();
        app.items = (0..8)
            .map(|i| Item {
                id: format!("PVTI_{}", i),
                title: format!(
                    "Ticket {} with a deliberately long title that has to be truncated somewhere",
                    i
                ),
                number: Some(400 + i),
                repository: Some("Org/repo-name".to_string()),
                status: Status::all_columns()[i as usize % Status::all_columns().len()].clone(),
                priority: Some("P2".to_string()),
                stack: Some("BE".to_string()),
                sprint: Some("Sprint 28".to_string()),
                assignees: vec!["qa-user".to_string(), "dev-one".to_string()],
                labels: if i % 3 == 0 {
                    vec!["Enhancement".to_string()]
                } else {
                    vec!["Bug".to_string(), "Ready-for-UAT".to_string()]
                },
                ..Default::default()
            })
            .collect();
        app.search_results = (0..app.items.len()).collect();
        app.notifications = Vec::new();
        app
    }
}

#[cfg(test)]
mod render_tests {
    use super::render_tests_support::*;
    use super::*;
    use crate::app::{AssigneeContext, ConfirmAction};
    use ratatui::backend::TestBackend;

    fn every_screen() -> Vec<Screen> {
        vec![
            Screen::Auth,
            Screen::Menu,
            Screen::Board,
            Screen::MyTasks,
            Screen::Search,
            Screen::Dashboard,
            Screen::Notifications,
            Screen::Settings,
            Screen::Setup,
            Screen::SlackSetup,
            Screen::SlackNotifyConfig,
            Screen::SlackUserMap,
            Screen::StackLeadsEdit,
            Screen::Help,
            Screen::ItemDetail(2),
            Screen::QaActions(2),
            Screen::MoveDialog(vec![1, 2, 3]),
            Screen::History(2),
            Screen::Comment(2),
            Screen::BugReport(Some(2), true),
            Screen::BugReport(None, false),
            Screen::TaskForm(Some(2)),
            Screen::AssigneePicker(AssigneeContext::Bug(Some(2), true)),
            Screen::BatchPicker(crate::app::BatchKind::VerifyUat),
            Screen::BatchPicker(crate::app::BatchKind::MoveToUat),
        ]
    }

    /// Render every screen at every supported size.
    ///
    /// A TUI's responsive failures are panics: a `width - 20` on a narrow
    /// terminal, or indexing a list that scrolled past its end. This walks the
    /// whole screen set at the smallest supported size and up, which is the
    /// only cheap way to keep that from regressing.
    #[test]
    fn every_screen_renders_at_every_size() {
        // 60x10 is the documented minimum; 40x8 is below it and must still
        // render the resize notice rather than panic.
        let sizes = [(40u16, 8u16), (60, 10), (80, 24), (100, 30), (200, 60)];
        for (w, h) in sizes {
            for screen in every_screen() {
                let mut app = app_with_items();
                app.screen = screen.clone();
                let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
                terminal
                    .draw(|f| draw(f, &mut app))
                    .unwrap_or_else(|e| panic!("{:?} at {}x{} failed: {}", screen, w, h, e));
            }
        }
    }

    /// The report form renders both kinds, at every size.
    #[test]
    fn enhancement_form_renders_at_every_size() {
        for (w, h) in [(40u16, 8u16), (60, 10), (80, 24), (200, 60)] {
            for kind in [
                crate::app::ReportKind::Bug,
                crate::app::ReportKind::Enhancement,
            ] {
                let mut app = app_with_items();
                app.open_report_form(Some(2), false, kind);
                let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
                terminal
                    .draw(|f| draw(f, &mut app))
                    .unwrap_or_else(|e| panic!("{:?} at {}x{} failed: {}", kind, w, h, e));
            }
        }
    }

    /// The batch pick list with rows, scrolled, at every size.
    #[test]
    fn batch_picker_renders_at_every_size() {
        for (w, h) in [(40u16, 8u16), (60, 10), (80, 24), (100, 30), (200, 60)] {
            let mut app = app_with_items();
            app.batch_rows = app
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| (it.id.clone(), i % 2 == 0))
                .chain(std::iter::once(("gone".to_string(), true)))
                .collect();
            app.batch_cursor = 7;
            for kind in [
                crate::app::BatchKind::VerifyUat,
                crate::app::BatchKind::MoveToUat,
            ] {
                app.screen = Screen::BatchPicker(kind);
                let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
                terminal
                    .draw(|f| draw(f, &mut app))
                    .unwrap_or_else(|e| panic!("pick list at {}x{} failed: {}", w, h, e));
            }
        }
    }

    /// Item Detail with linked PRs and deploy results, at every size.
    #[test]
    fn detail_with_deploy_results_renders_at_every_size() {
        use crate::gh::issue::DeployState;
        for (w, h) in [(40u16, 8u16), (60, 10), (80, 24), (100, 30), (200, 60)] {
            let mut app = app_with_items();
            app.items[2].linked_prs = (0..3)
                .map(|n| crate::models::item::LinkedPr {
                    repo: "Org/a-repository-with-a-long-name".into(),
                    number: 2100 + n,
                    title:
                        "A pull request title long enough to need truncating on a narrow terminal"
                            .into(),
                    url: format!("https://github.com/Org/repo/pull/{}", 2100 + n),
                    state: if n == 2 {
                        "OPEN".into()
                    } else {
                        "MERGED".into()
                    },
                    merge_commit: Some("abc".into()),
                    base_ref: "develop".into(),
                })
                .collect();
            let id = app.items[2].id.clone();
            app.linked_prs_loaded.insert(id.clone());
            app.deploy_checks.insert(
                "https://github.com/Org/repo/pull/2100".into(),
                vec![
                    ("Dev".into(), DeployState::Contained),
                    ("STG".into(), DeployState::Unknown),
                    ("UAT".into(), DeployState::Missing),
                    ("Prod".into(), DeployState::Failed("boom".into())),
                ],
            );
            app.deploy_check_inflight.insert(id);
            app.screen = Screen::ItemDetail(2);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
            terminal
                .draw(|f| draw(f, &mut app))
                .unwrap_or_else(|e| panic!("detail at {}x{} failed: {}", w, h, e));
        }
    }

    /// Popups sit on top of every screen, so they get the same treatment.
    #[test]
    fn every_popup_renders_at_every_size() {
        let popups = vec![
            Popup::Error(
                "A long error message that has to wrap several times to prove the popup grows and never clips its dismiss hint.".to_string(),
            ),
            Popup::Success("Done".to_string()),
            Popup::CopyResults,
            Popup::ConfirmSprint {
                title: "Return - #4218".to_string(),
                message: "Move back to In Progress + comment tagging the devs?\nBoth Ready-for-* labels are cleared, so the ticket needs a full re-verify.".to_string(),
                choices: vec![
                    crate::app::SprintChoice {
                        label: "Keep (Sprint 30)".to_string(),
                        sprint: None,
                    },
                    crate::app::SprintChoice {
                        label: "Today (Sprint 31)".to_string(),
                        sprint: Some("Sprint 31".to_string()),
                    },
                    crate::app::SprintChoice {
                        label: "Next (Sprint 32)".to_string(),
                        sprint: Some("Sprint 32".to_string()),
                    },
                ],
                selected: 1,
                on_confirm: ConfirmAction::Quit,
            },
            Popup::SavePreset {
                name: "A filter name long enough to scroll inside the prompt box".to_string(),
            },
            Popup::Presets { selected: 5 },
            Popup::Confirm {
                title: "Move 3 item(s)".to_string(),
                message: "Move to In UAT?\n#401, #402, #403".to_string(),
                on_confirm: ConfirmAction::Quit,
            },
        ];
        for (w, h) in [(40u16, 8u16), (60, 10), (80, 24), (100, 30), (200, 60)] {
            for popup in &popups {
                let mut app = app_with_items();
                app.config.search_presets = (0..8)
                    .map(|i| crate::config::SearchPreset {
                        name: format!("Saved filter number {} with a long name", i),
                        ..Default::default()
                    })
                    .collect();
                app.screen = Screen::Board;
                app.popup = popup.clone();
                let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
                terminal
                    .draw(|f| draw(f, &mut app))
                    .unwrap_or_else(|e| panic!("popup at {}x{} failed: {}", w, h, e));
            }
        }
    }

    /// The location bar must always end at the current screen, at any width.
    #[test]
    fn breadcrumb_keeps_the_current_screen_visible() {
        let mut app = app_with_items();
        app.screen = Screen::Board;
        app.goto(Screen::ItemDetail(2));
        app.goto(Screen::QaActions(2));

        let trail = app.breadcrumb();
        assert_eq!(trail.last().map(String::as_str), Some("QA actions"));
        assert!(trail.contains(&"Board".to_string()));

        for w in [60u16, 80, 120] {
            let mut terminal = Terminal::new(TestBackend::new(w, 24)).expect("test terminal");
            terminal.draw(|f| draw(f, &mut app)).expect("draw");
            let buffer = terminal.backend().buffer().clone();
            let first_line: String = (0..w)
                .map(|x| buffer.cell((x, 0)).map(|c| c.symbol()).unwrap_or(" "))
                .collect();
            assert!(
                first_line.contains("QA actions"),
                "breadcrumb lost the current screen at width {}: {:?}",
                w,
                first_line
            );
        }
    }
}

#[cfg(test)]
mod snapshot_tool {
    use super::render_tests_support::*;
    use super::*;
    use ratatui::backend::TestBackend;

    /// Dump screens as text so the layout can be eyeballed without a terminal:
    /// Print `screen` as text, framed so trailing spaces stay visible.
    fn print_screen(app: &mut App, label: &str, w: u16, h: u16) {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        terminal.draw(|f| draw(f, app)).expect("draw");
        let buffer = terminal.backend().buffer().clone();
        println!("\n=== {} @ {}x{} ===", label, w, h);
        for y in 0..h {
            let line: String = (0..w)
                .map(|x| buffer.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
                .collect();
            println!("|{}|", line.trim_end());
        }
    }

    /// Scenarios for `snapshot_live`, by name: each sets up the app state a
    /// screen is reviewed in.
    fn live_scenario(app: &mut App, name: &str) -> bool {
        match name {
            "search" => {
                app.screen = Screen::Search;
                app.apply_filters();
            }
            "search-epic" => {
                app.screen = Screen::Search;
                app.search_field_idx = 6;
                if let Some((value, _)) = app.search_filter_options(6).into_iter().nth(3) {
                    app.filter_epic = vec![value];
                }
                app.filter_cursor[5] = 3;
                app.apply_filters();
            }
            "search-assignee" => {
                app.screen = Screen::Search;
                app.search_field_idx = 7;
                app.filter_status = vec!["In QA".to_string()];
                app.filter_label = vec!["!Ready-for-UAT".to_string()];
                app.filter_assignee = vec![app.config.current_user.clone()];
                app.apply_filters();
            }
            "search-presets" => {
                app.screen = Screen::Search;
                app.config.search_presets = vec![
                    crate::config::SearchPreset {
                        name: "In QA - not passed STG - mine".into(),
                        ..Default::default()
                    },
                    crate::config::SearchPreset {
                        name: "Dave epic, this sprint".into(),
                        ..Default::default()
                    },
                ];
                app.apply_filters();
                app.popup = Popup::Presets { selected: 1 };
            }
            "search-save" => {
                app.screen = Screen::Search;
                app.apply_filters();
                app.popup = Popup::SavePreset {
                    name: "In QA mine".into(),
                };
            }
            // `detail-4019`: a ticket with its linked PRs, read live from
            // GitHub (read-only).
            n if n.starts_with("detail-") => {
                let number: u32 = n["detail-".len()..].parse().expect("detail-<number>");
                let Some(idx) = app.items.iter().position(|i| {
                    i.number == Some(number)
                        && i.repository.as_deref() == Some(&app.config.bug_repo)
                }) else {
                    return false;
                };
                let repo = app.items[idx].repository.clone().unwrap_or_default();
                app.items[idx].linked_prs =
                    crate::gh::issue::linked_prs(&repo, number).expect("linked PRs");
                let id = app.items[idx].id.clone();
                app.linked_prs_loaded.insert(id);
                // Run the deploy check inline, as the worker would.
                if std::env::var("SNAP_DEPLOY").is_ok() {
                    let branches = app.config.deploy_branches.clone();
                    for pr in app.items[idx].linked_prs.clone() {
                        let Some(sha) = pr.merge_commit.clone() else {
                            continue;
                        };
                        let states = branches
                            .iter()
                            .map(|b| {
                                (
                                    b.env.clone(),
                                    crate::gh::issue::deploy_state(&pr.repo, &b.branch, &sha),
                                )
                            })
                            .collect();
                        app.deploy_checks.insert(pr.url.clone(), states);
                    }
                }
                app.screen = Screen::ItemDetail(idx);
            }
            // Move to In UAT opened with I from a labelled In QA ticket.
            "move-to-uat" => {
                app.board_sprint_filter = None;
                let has_label = |i: &crate::models::item::Item| {
                    i.labels.iter().any(|l| App::label_eq(l, "Ready-for-UAT"))
                };
                // Nothing qualifies right now: label a few In QA tickets, in
                // memory only.
                if !app
                    .items
                    .iter()
                    .any(|i| i.status == crate::models::status::Status::InQA && has_label(i))
                {
                    for item in app
                        .items
                        .iter_mut()
                        .filter(|i| i.status == crate::models::status::Status::InQA)
                        .take(4)
                    {
                        item.labels.push("Ready-for-UAT".into());
                    }
                }
                let Some(idx) = app
                    .items
                    .iter()
                    .position(|i| i.status == crate::models::status::Status::InQA && has_label(i))
                else {
                    return false;
                };
                app.screen = Screen::ItemDetail(idx);
                handlers::detail::handle_detail(app, KeyCode::Char('I'), idx);
            }
            // Bulk verify UAT opened with V from the first In UAT ticket.
            "verify-uat" | "qa-menu-uat" => {
                app.board_sprint_filter = None;
                // The board may have nothing In UAT right now: put a few
                // real tickets there, in memory only, to review the layout.
                if !app
                    .items
                    .iter()
                    .any(|i| i.status == crate::models::status::Status::InUAT)
                {
                    for item in app
                        .items
                        .iter_mut()
                        .filter(|i| i.status == crate::models::status::Status::TechComplete)
                        .take(6)
                    {
                        item.status = crate::models::status::Status::InUAT;
                    }
                }
                let Some(idx) = app
                    .items
                    .iter()
                    .position(|i| i.status == crate::models::status::Status::InUAT)
                else {
                    return false;
                };
                app.screen = Screen::ItemDetail(idx);
                if name == "verify-uat" {
                    handlers::detail::handle_detail(app, KeyCode::Char('V'), idx);
                    handlers::batch::handle_batch_picker(
                        app,
                        KeyCode::Down,
                        crate::app::BatchKind::VerifyUat,
                    );
                    handlers::batch::handle_batch_picker(
                        app,
                        KeyCode::Char(' '),
                        crate::app::BatchKind::VerifyUat,
                    );
                } else {
                    app.screen = Screen::QaActions(idx);
                }
            }
            // Return's sprint picker over a real ticket.
            "return-sprint" => {
                let Some(idx) = app
                    .items
                    .iter()
                    .position(|i| i.status == crate::models::status::Status::InQA)
                else {
                    return false;
                };
                app.screen = Screen::QaActions(idx);
                handlers::detail::handle_qa_actions(app, KeyCode::Char('r'), idx);
                if let Popup::ConfirmSprint { selected, .. } = &mut app.popup {
                    *selected = 1;
                }
            }
            // Help, scrolled to `help-<line>`.
            n if n.starts_with("help") => {
                app.screen = Screen::Help;
                app.help_scroll = n
                    .strip_prefix("help-")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            "slack-notify" => {
                app.screen = Screen::SlackNotifyConfig;
                // The last toggle, which used to sit below an 80x24 panel.
                app.slack_notify_cursor = crate::config::SlackNotifyConfig::LABELS.len() - 1;
            }
            "search-copy" => {
                app.screen = Screen::Search;
                app.filter_status = vec!["In QA".to_string()];
                app.apply_filters();
                app.popup = Popup::CopyResults;
            }
            _ => return false,
        }
        true
    }

    /// Render screens over the real board from the local cache, for reviewing
    /// a layout against real titles, epics and assignees. Read-only: only the
    /// `detail-<n>` scenarios call GitHub, and only to read.
    ///
    /// `SNAP=search,search-epic cargo test snapshot_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn snapshot_live() {
        let names = std::env::var("SNAP").unwrap_or_else(|_| "search".to_string());
        let sizes: Vec<(u16, u16)> = std::env::var("SNAP_SIZES")
            .ok()
            .map(|v| {
                v.split(',')
                    .filter_map(|s| {
                        let (w, h) = s.split_once('x')?;
                        Some((w.parse().ok()?, h.parse().ok()?))
                    })
                    .collect()
            })
            .unwrap_or_else(|| vec![(120, 40), (80, 24)]);
        for name in names.split(',') {
            for (w, h) in &sizes {
                let mut app = App::new();
                let (items, _) = app
                    .cache
                    .get_with_freshness::<Vec<crate::models::item::Item>>("items");
                let (fields, _) = app
                    .cache
                    .get_with_freshness::<Vec<crate::models::field::Field>>("fields");
                app.items = items.expect("no cached items - run git-tui once first");
                app.fields = fields.unwrap_or_default();
                app.current_user = app.config.current_user.clone();
                app.board_sprint_filter = app.current_sprint_name();
                assert!(live_scenario(&mut app, name), "unknown scenario {}", name);
                print_screen(&mut app, name, *w, *h);
            }
        }
    }

    /// `cargo test snapshot -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn snapshot() {
        let targets = [
            (Screen::Menu, 100u16, 26u16),
            (Screen::Menu, 62, 20),
            (Screen::Board, 100, 26),
            (Screen::MyTasks, 100, 26),
            (Screen::MyTasks, 62, 18),
            (Screen::Search, 100, 26),
            (Screen::ItemDetail(2), 100, 26),
            (Screen::QaActions(2), 100, 26),
            (Screen::QaActions(7), 96, 28),
            (Screen::BugReport(Some(2), false), 100, 34),
            (Screen::MoveDialog(vec![1, 2]), 80, 22),
            (Screen::Dashboard, 120, 30),
        ];
        for (screen, w, h) in targets {
            let mut app = app_with_items();
            // The report form files both kinds; snapshot the Enhancement one
            // so its title and label are visible here too.
            if matches!(screen, Screen::BugReport(_, false)) {
                app.bug_kind = crate::app::ReportKind::Enhancement;
            }
            app.screen = screen.clone();
            let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
            terminal.draw(|f| draw(f, &mut app)).expect("draw");
            let buffer = terminal.backend().buffer().clone();
            println!("\n=== {:?} @ {}x{} ===", screen, w, h);
            for y in 0..h {
                let line: String = (0..w)
                    .map(|x| buffer.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "))
                    .collect();
                println!("|{}|", line.trim_end());
            }
        }
    }
}
