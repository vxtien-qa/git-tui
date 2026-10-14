use anyhow::Result;

use crate::app::App;
use crate::gh;
use crate::handlers::helpers::extract_repo_name;

/// Minimum remaining API quota before skipping background refresh.
const MIN_RATE_LIMIT_REMAINING: i64 = 500;
/// Maximum number of issues per batch timeline query - 20 keeps the argv
/// comfortably under the Windows 32K CreateProcess limit.
const TIMELINE_BATCH_SIZE: usize = 20;

/// Timeline cache key. MUST include the repo - the project spans multiple
/// repositories, and issue #42 exists in more than one of them; a number-only
/// key let their histories overwrite each other (silently corrupting the QA
/// metrics that are computed from cached timelines).
pub fn timeline_cache_key(repo: &str, number: u32) -> String {
    format!("timeline_{}_{}", extract_repo_name(repo), number)
}

/// Whether the background sync fetches `item`'s timeline.
///
/// Active tickets always. Backlog and Ready for Dev are lazy - the Dashboard
/// fetches them on open - except in the sprint the reports cover: that fetch
/// is skipped whenever a sync is in flight, and every sync drops the in-memory
/// histories, so a ticket filed straight into Ready for Dev (#4000, #4001 and
/// #4004 in Sprint 29) reported "no history" through every refresh.
pub fn sync_fetches_timeline(
    item: &crate::models::item::Item,
    report_sprint: Option<&str>,
) -> bool {
    use crate::models::status::Status;
    match item.status {
        Status::Backlog | Status::ReadyForDev => {
            report_sprint.is_some() && item.sprint.as_deref() == report_sprint
        }
        Status::InProgress
        | Status::Blocked
        | Status::InReview
        | Status::InQADev
        | Status::InQA
        | Status::InUAT
        | Status::TechComplete
        | Status::ReadyForRelease
        | Status::Done => true,
    }
}

/// Whether a detail fetch should be started for `item_id`.
///
/// Deliberately does NOT consult `bg_rx`: that channel belongs to the
/// background sync, whose timeline workers hold it open long after the item
/// list has landed. Gating on it meant that during a sync, opening a ticket
/// only parked one index for later and reported a load that never resolved.
///
/// The only gate is "already in flight", because `handle_detail` runs on every
/// keypress and would otherwise spawn a `gh` process per scroll press.
fn should_start_detail_fetch(app: &App, item_id: &str) -> bool {
    !app.detail_fetch_inflight.contains(item_id)
}

/// Load items only (step 1 of startup).
pub fn load_data_step_items(app: &mut App) -> Result<()> {
    let (cached, is_fresh) = app
        .cache
        .get_with_freshness::<Vec<crate::models::item::Item>>("items");
    if let Some(items) = cached {
        app.items = items;
        if !is_fresh {
            spawn_background_refresh(app, true);
        }
    } else {
        let items = gh::sync::full_sync(&app.config.owner, app.config.project_number)?;
        app.items = items;
        let _ = app.cache.set("items", &app.items);
    }
    Ok(())
}

/// Load fields only (step 2 of startup).
/// Fields rarely change, so we use a 30-minute TTL instead of the default 3 min.
pub fn load_data_step_fields(app: &mut App) {
    const FIELDS_TTL_SECS: u64 = 1800; // 30 minutes
    let (cached, is_fresh) = app
        .cache
        .get_with_custom_ttl::<Vec<crate::models::field::Field>>("fields", FIELDS_TTL_SECS);
    if let Some(fields) = cached {
        app.fields = fields;
        if !is_fresh {
            spawn_background_refresh(app, true);
        }
    } else {
        match gh::field::list(&app.config.owner, app.config.project_number) {
            Ok(fields) => {
                app.fields = fields;
                let _ = app.cache.set("fields", &app.fields);
            }
            Err(e) => {
                let msg = format!("{}", e);
                app.set_status(&format!(
                    "Fields: {}",
                    msg.chars().take(60).collect::<String>()
                ));
            }
        }
    }
}

/// Load project ID only (step 3 of startup).
/// Project ID almost never changes, so we use a 1-hour TTL.
pub fn load_data_step_project(app: &mut App) {
    const PROJECT_TTL_SECS: u64 = 3600; // 1 hour
    let (cached, is_fresh) = app
        .cache
        .get_with_custom_ttl::<String>("project_id", PROJECT_TTL_SECS);
    if let Some(pid) = cached {
        app.project_id = Some(pid);
        if !is_fresh {
            spawn_background_refresh(app, true);
        }
    } else if let Ok(id) = gh::project::get_id(&app.config.owner, app.config.project_number) {
        app.project_id = Some(id.clone());
        let _ = app.cache.set("project_id", &id);
    }
}

/// Spawn a background thread to refresh data if one isn't already running.
/// Returns true if a refresh thread was actually started - callers that set
/// `loading` must check this, otherwise a skipped spawn leaks the spinner.
pub fn spawn_background_refresh(app: &mut App, quiet: bool) -> bool {
    if app.bg_rx.is_some() {
        return false;
    }

    // Check rate limit first (anti-spam). CACHED-only lookup - the blocking
    // variant spawns a gh process + HTTPS round trip and this runs on the UI
    // thread every auto-refresh; the worker below refreshes the cache instead.
    if let Some(remaining) = gh::client::rate_limit_remaining_cached() {
        if remaining < MIN_RATE_LIMIT_REMAINING {
            app.set_status(&format!(
                "Rate limit low ({}), skipped background refresh",
                remaining
            ));
            // Nothing will ever clear the spinner if we leave it on.
            app.loading = false;
            return false;
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    app.bg_rx = Some(rx);
    if !quiet {
        app.loading = true;
    }

    let owner = app.config.owner.clone();
    let project_number = app.config.project_number;
    let cache_ttl = app.config.poll_interval_secs;
    // The sprint the reports cover - its parked tickets get a timeline too.
    let report_sprint = app
        .board_sprint_filter
        .clone()
        .or_else(|| app.current_sprint_name());

    std::thread::spawn(move || {
        // Refresh the rate-limit cache off the UI thread so the cached-only
        // check above has fresh data next time.
        let _ = gh::client::rate_limit_remaining();

        let items_res = gh::sync::full_sync(&owner, project_number);
        let fields_res = gh::field::list(&owner, project_number);
        let pid_res = gh::project::get_id(&owner, project_number);

        match (items_res, fields_res) {
            (Ok(items), Ok(fields)) => {
                let pid = pid_res.ok();

                // Persist to disk cache HERE (worker thread) - doing it on the
                // UI thread caused a serialize+write hitch every refresh.
                let disk_cache = crate::cache::Cache::new(cache_ttl);
                let _ = disk_cache.set("items", &items);
                let _ = disk_cache.set("fields", &fields);
                if let Some(ref pid) = pid {
                    let _ = disk_cache.set("project_id", pid);
                }

                let _ = tx.send(crate::app::BackgroundEvent::RefreshSuccess {
                    items: items.clone(),
                    fields,
                    project_id: pid,
                });

                // Batch + Cache: Batch GraphQL timeline queries
                let timeline_cache = crate::cache::Cache::new(1800);
                let tx_timeline = tx.clone();
                let owner_timeline = owner.clone();
                let items_for_created = items.clone();

                std::thread::spawn(move || {
                    let mut batch_entries = Vec::new();
                    for (i, item) in items.iter().enumerate() {
                        if !sync_fetches_timeline(item, report_sprint.as_deref()) {
                            continue;
                        }
                        if let (Some(repo), Some(number)) = (item.repository.clone(), item.number) {
                            let cache_key = timeline_cache_key(&repo, number);
                            let (cached_td, is_fresh) = timeline_cache
                                .get_with_freshness::<crate::gh::timeline::TimelineData>(
                                &cache_key,
                            );
                            if let Some(td) = cached_td {
                                let _ = tx_timeline.send(
                                    crate::app::BackgroundEvent::StatusHistoryFetched {
                                        item_idx: i,
                                        history: td.status_changes,
                                        comment_history: td.comments,
                                        label_history: td.labels,
                                    },
                                );
                                if is_fresh {
                                    continue;
                                }
                            }

                            // Owner comes from the item's own nameWithOwner -
                            // a repo from another org would otherwise be queried
                            // under the project owner and return no timeline.
                            let (repo_owner, repo_name) =
                                crate::handlers::helpers::split_repo(&repo, &owner_timeline);

                            batch_entries.push(crate::gh::timeline::BatchTimelineEntry {
                                item_idx: i,
                                owner: repo_owner,
                                repo_name,
                                issue_number: number,
                            });
                        }
                    }

                    for chunk in batch_entries.chunks(TIMELINE_BATCH_SIZE) {
                        match crate::gh::timeline::fetch_status_changes_batch(chunk) {
                            Ok(results) => {
                                for (item_idx, timeline_data) in results {
                                    if let Some(item) = items.get(item_idx) {
                                        if let (Some(repo), Some(number)) =
                                            (item.repository.as_deref(), item.number)
                                        {
                                            let cache_key = timeline_cache_key(repo, number);
                                            let _ = timeline_cache.set(&cache_key, &timeline_data);
                                        }
                                    }
                                    let _ = tx_timeline.send(
                                        crate::app::BackgroundEvent::StatusHistoryFetched {
                                            item_idx,
                                            history: timeline_data.status_changes,
                                            comment_history: timeline_data.comments,
                                            label_history: timeline_data.labels,
                                        },
                                    );
                                }
                            }
                            Err(_) => {
                                // Batch failed - skip silently, stale cache data still shown
                            }
                        }
                    }
                });

                // Persist created_at: Skip items that already have it
                let tx_created = tx.clone();
                let owner_created = owner.clone();
                std::thread::spawn(move || {
                    for (i, item) in items_for_created.iter().enumerate() {
                        if item.created_at.is_some() {
                            continue;
                        }
                        let is_bug = item.labels.iter().any(|l| l.to_lowercase().contains("bug"));
                        if !is_bug {
                            continue;
                        }
                        if let (Some(repo), Some(number)) = (item.repository.clone(), item.number) {
                            let (repo_owner, repo_name) =
                                crate::handlers::helpers::split_repo(&repo, &owner_created);
                            let full_repo = format!("{}/{}", repo_owner, repo_name);
                            if let Ok(json) = crate::gh::issue::view(&full_repo, number) {
                                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json)
                                {
                                    if let Some(created) = parsed["createdAt"].as_str() {
                                        if let Ok(dt) =
                                            chrono::DateTime::parse_from_rfc3339(created)
                                        {
                                            let _ = tx_created.send(
                                                crate::app::BackgroundEvent::CreatedAtFetched {
                                                    item_idx: i,
                                                    created_at: dt.with_timezone(&chrono::Utc),
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }
            (Err(e), _) | (_, Err(e)) => {
                let _ = tx.send(crate::app::BackgroundEvent::RefreshError(format!("{}", e)));
            }
        }
    });
    true
}

/// Trigger a background timeline fetch for items missing timeline data in memory.
/// This is a lightweight alternative to spawn_background_refresh - it only fetches
/// timeline (no full item sync), and only for items that don't already have it.
/// Called when navigating to Dashboard so reports always have fresh data.
pub fn trigger_timeline_fetch(app: &mut App) {
    // If a background refresh is already running (which includes timeline fetch), skip
    if app.bg_rx.is_some() {
        return;
    }

    // Collect items that need timeline data
    let active_statuses = [
        crate::models::status::Status::InQA,
        crate::models::status::Status::InQADev,
        crate::models::status::Status::InUAT,
        crate::models::status::Status::InProgress,
        crate::models::status::Status::InReview,
        crate::models::status::Status::ReadyForRelease,
        crate::models::status::Status::Blocked,
        crate::models::status::Status::TechComplete,
        crate::models::status::Status::Done,
        // Report path also needs parked items: a ticket that failed QA and
        // was then moved to Backlog would otherwise silently drop out of the
        // failed/tested metrics.
        crate::models::status::Status::Backlog,
        crate::models::status::Status::ReadyForDev,
    ];

    let items_needing_timeline: Vec<_> = app
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            active_statuses.contains(&item.status)
                && item.status_history.is_none()
                && item.repository.is_some()
                && item.number.is_some()
        })
        .map(|(i, item)| (i, item.repository.clone().unwrap(), item.number.unwrap()))
        .collect();

    if items_needing_timeline.is_empty() {
        return; // All items already have timeline data
    }

    let owner = app.config.owner.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.bg_rx = Some(rx);
    app.set_status(&format!(
        "Fetching timeline for {} items...",
        items_needing_timeline.len()
    ));

    std::thread::spawn(move || {
        let timeline_cache = crate::cache::Cache::new(1800);
        let mut batch_entries = Vec::new();

        for (item_idx, repo, number) in &items_needing_timeline {
            // Check disk cache first; a STALE hit is still shown immediately
            // but also queued for refetch (previously stale-but-under-hard-TTL
            // entries were served forever through this path).
            let cache_key = timeline_cache_key(repo, *number);
            let mut have_fresh = false;
            if let (Some(td), is_fresh) =
                timeline_cache.get_with_freshness::<crate::gh::timeline::TimelineData>(&cache_key)
            {
                let _ = tx.send(crate::app::BackgroundEvent::StatusHistoryFetched {
                    item_idx: *item_idx,
                    history: td.status_changes,
                    comment_history: td.comments,
                    label_history: td.labels,
                });
                have_fresh = is_fresh;
            }
            if have_fresh {
                continue;
            }

            let (repo_owner, repo_name) = crate::handlers::helpers::split_repo(repo, &owner);
            batch_entries.push(crate::gh::timeline::BatchTimelineEntry {
                item_idx: *item_idx,
                owner: repo_owner,
                repo_name,
                issue_number: *number,
            });
        }

        // Batch fetch from API for the rest
        for chunk in batch_entries.chunks(TIMELINE_BATCH_SIZE) {
            match crate::gh::timeline::fetch_status_changes_batch(chunk) {
                Ok(results) => {
                    for (item_idx, timeline_data) in results {
                        if let Some((_, repo, number)) = items_needing_timeline
                            .iter()
                            .find(|(i, _, _)| *i == item_idx)
                        {
                            let cache_key = timeline_cache_key(repo, *number);
                            let _ = timeline_cache.set(&cache_key, &timeline_data);
                        }
                        let _ = tx.send(crate::app::BackgroundEvent::StatusHistoryFetched {
                            item_idx,
                            history: timeline_data.status_changes,
                            comment_history: timeline_data.comments,
                            label_history: timeline_data.labels,
                        });
                    }
                }
                Err(_) => {
                    // Batch failed - silent, stale cache still shown
                }
            }
        }

        // Signal done (send a dummy that won't match any item - bg_rx will be cleared)
        // We abuse RefreshSuccess with empty data to signal completion so bg_rx gets cleared.
        // Actually just let the channel drop naturally - main loop will clear bg_rx when sender drops.
    });
}

/// Debounced refresh: only allows refresh if 30+ seconds since last refresh.
/// Returns true if refresh was performed, false if debounced.
pub fn try_refresh(app: &mut App, quiet: bool) -> bool {
    const DEBOUNCE_SECS: i64 = 30;

    if app.bg_rx.is_some() {
        app.set_status("Already refreshing in background...");
        return false;
    }

    if let Some(last) = app.last_refresh {
        let elapsed = chrono::Utc::now().signed_duration_since(last).num_seconds();
        if elapsed < DEBOUNCE_SECS {
            app.set_status(&format!(
                "Cooldown: wait {}s before refresh",
                DEBOUNCE_SECS - elapsed
            ));
            return false;
        }
    }

    // Spawn first - it can decline (rate limit); only claim success if it ran.
    if spawn_background_refresh(app, quiet) {
        if !quiet {
            app.loading = true;
        }
        app.set_status("Refresh started in background...");
        true
    } else {
        false
    }
}

/// Fetch full issue details (body + comments) asynchronously via background thread.
/// Uses `bg_rx` channel - will defer if bg refresh is already running.
pub fn fetch_item_detail(app: &mut App, item_idx: usize) {
    let (has_body, number, repo, item_id) = {
        let item = match app.items.get(item_idx) {
            Some(i) => i,
            None => return,
        };
        let has_body = item.body.is_some();
        let number = item.number;
        let repo = item.repository.clone();
        (has_body, number, repo, item.id.clone())
    };

    if has_body {
        return;
    }

    let (number, repo) = match resolve_detail_params(app, item_idx, number, repo) {
        Some(pair) => pair,
        None => return,
    };

    if !should_start_detail_fetch(app, &item_id) {
        return;
    }

    // Goes through the persistent action channel, NOT `bg_rx`.
    //
    // `bg_rx` belongs to the background sync, whose timeline and created_at
    // workers keep it open long after the item list has arrived. Sharing the
    // slot meant that during a sync (every poll interval, for as long as the
    // timeline fetch takes) opening a ticket only parked one index in
    // `pending_detail_fetch` and reported "Loading item details..." that never
    // resolved, while moving to another ticket dropped the previous one.
    app.detail_fetch_inflight.insert(item_id.clone());
    app.loading = true;
    app.set_status(&format!("Fetching #{} from {}...", number, repo));
    let is_issue = is_issue(app, item_idx);
    spawn_detail_fetch_action(
        app.action_tx.clone(),
        item_id,
        number,
        repo,
        is_issue,
        false,
    );
}

/// Check where a ticket's merged PRs are deployed (`d` in Item Detail).
///
/// For each merged PR and each `config.deploy_branches` entry, GitHub's
/// compare API says whether the branch contains the PR's merge commit. Runs
/// on worker threads, up to four PRs at a time; the result lands as
/// `DeployChecked`.
pub fn spawn_deploy_check(app: &mut App, item_idx: usize) {
    let Some(item) = app.items.get(item_idx) else {
        return;
    };
    let label = item
        .number
        .map(|n| format!("#{}", n))
        .unwrap_or_else(|| "this ticket".to_string());
    match item.content_type {
        crate::models::item::ContentType::Issue => {}
        crate::models::item::ContentType::PullRequest => {
            app.set_status(&format!(
                "{} is a pull request card - open the ticket it belongs to and press d there",
                label
            ));
            return;
        }
        crate::models::item::ContentType::DraftIssue => {
            app.set_status("A draft card has no linked PRs - nothing to check");
            return;
        }
    }
    if let Some(e) = app.linked_prs_failed.get(&item.id) {
        app.set_status(&format!(
            "Linked PRs could not be loaded ({}) - press r to reload",
            e
        ));
        return;
    }
    if !app.linked_prs_loaded.contains(&item.id) {
        app.set_status("Linked PRs are still loading - try again in a moment");
        return;
    }
    if item.linked_prs.is_empty() {
        app.set_status(&format!(
            "{} has no PRs linked on GitHub - nothing to check",
            label
        ));
        return;
    }
    let prs: Vec<crate::models::item::LinkedPr> = item
        .linked_prs
        .iter()
        .filter(|p| p.is_merged() && p.merge_commit.is_some())
        .cloned()
        .collect();
    if prs.is_empty() {
        app.set_status(&format!(
            "{}: no linked PR is merged yet - nothing can be deployed",
            label
        ));
        return;
    }
    if !app.deploy_check_inflight.insert(item.id.clone()) {
        app.set_status("Deploy check already running");
        return;
    }
    let item_id = item.id.clone();
    let branches = app.config.deploy_branches.clone();
    app.set_status(&format!(
        "Checking {} merged PR(s) against {}...",
        prs.len(),
        branches
            .iter()
            .map(|b| b.branch.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let tx = app.action_tx.clone();
    std::thread::spawn(move || {
        // A root ticket can be mentioned by dozens of PRs: check at most four
        // at a time rather than start a `gh` process per PR at once.
        let mut results = Vec::new();
        for chunk in prs.chunks(4) {
            let workers: Vec<_> = chunk
                .iter()
                .cloned()
                .map(|pr| {
                    let branches = branches.clone();
                    std::thread::spawn(move || {
                        let sha = pr.merge_commit.clone().unwrap_or_default();
                        let states = branches
                            .iter()
                            .map(|b| {
                                (
                                    b.env.clone(),
                                    gh::issue::deploy_state(&pr.repo, &b.branch, &sha),
                                )
                            })
                            .collect();
                        (pr.url, states)
                    })
                })
                .collect();
            results.extend(workers.into_iter().filter_map(|w| w.join().ok()));
        }
        let _ = tx.send(crate::app::ActionResult::DeployChecked {
            item_id,
            label,
            results,
        });
    });
}

/// Deploy-check every ticket in `item_indices` (`d` in the pick list).
///
/// Tickets whose linked PRs were not read yet get them read first; each
/// ticket then lands as its own `DeployChecked`, four tickets at a time, so
/// the pick list fills in its UAT column as results arrive.
pub fn spawn_deploy_check_rows(app: &mut App, item_indices: &[usize]) {
    struct Row {
        item_id: String,
        label: String,
        repo: String,
        number: u32,
        known: Option<Vec<crate::models::item::LinkedPr>>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for &idx in item_indices {
        let Some(item) = app.items.get(idx) else {
            continue;
        };
        let (Some(number), Some(repo)) = (item.number, item.repository.clone()) else {
            continue;
        };
        // A row whose lookup failed earlier gets a fresh one.
        let known = (app.linked_prs_loaded.contains(&item.id)
            && !app.linked_prs_failed.contains_key(&item.id))
        .then(|| item.linked_prs.clone());
        if item.content_type != crate::models::item::ContentType::Issue
            || app.deploy_check_inflight.contains(&item.id)
        {
            continue;
        }
        rows.push(Row {
            item_id: item.id.clone(),
            label: format!("#{}", number),
            repo,
            number,
            known,
        });
    }
    if rows.is_empty() {
        app.set_status("Nothing to check: no issue in the list, or checks already running");
        return;
    }
    for r in &rows {
        app.deploy_check_inflight.insert(r.item_id.clone());
    }
    let branches = app.config.deploy_branches.clone();
    let total = rows.len();
    app.set_status(&format!("Deploy check: {} ticket(s)...", total));

    let tx = app.action_tx.clone();
    std::thread::spawn(move || {
        let mut failed = 0usize;
        for chunk in rows.chunks(4) {
            let workers: Vec<_> = chunk
                .iter()
                .map(|r| {
                    let tx = tx.clone();
                    let branches = branches.clone();
                    let (item_id, label, repo, number, known) = (
                        r.item_id.clone(),
                        r.label.clone(),
                        r.repo.clone(),
                        r.number,
                        r.known.clone(),
                    );
                    std::thread::spawn(move || {
                        let prs = match known {
                            Some(prs) => prs,
                            None => {
                                let result =
                                    gh::issue::linked_prs(&repo, number).map_err(|e| e.to_string());
                                let ok = result.is_ok();
                                let prs = result.clone().unwrap_or_default();
                                let _ = tx.send(crate::app::ActionResult::LinkedPrsFetched {
                                    item_id: item_id.clone(),
                                    result,
                                });
                                if !ok {
                                    // Recorded as failed by LinkedPrsFetched; the
                                    // row shows ! and the summary counts it.
                                    let _ = tx.send(crate::app::ActionResult::DeployChecked {
                                        item_id,
                                        label,
                                        results: Vec::new(),
                                    });
                                    return false;
                                }
                                prs
                            }
                        };
                        let results = prs
                            .iter()
                            .filter(|p| p.is_merged())
                            .filter_map(|p| p.merge_commit.as_ref().map(|sha| (p, sha)))
                            .map(|(p, sha)| {
                                let states = branches
                                    .iter()
                                    .map(|b| {
                                        (
                                            b.env.clone(),
                                            gh::issue::deploy_state(&p.repo, &b.branch, sha),
                                        )
                                    })
                                    .collect();
                                (p.url.clone(), states)
                            })
                            .collect();
                        let _ = tx.send(crate::app::ActionResult::DeployChecked {
                            item_id,
                            label,
                            results,
                        });
                        true
                    })
                })
                .collect();
            for w in workers {
                if !w.join().unwrap_or(false) {
                    failed += 1;
                }
            }
        }
        let _ = tx.send(crate::app::ActionResult::StatusMessage(if failed == 0 {
            format!("Deploy check finished for {} ticket(s)", total)
        } else {
            format!(
                "Deploy check finished for {} ticket(s); {} could not read linked PRs (marked !) - open one and press r to retry",
                total, failed
            )
        }));
    });
}

/// Force-fetch item details using `action_rx` - never blocked by bg refresh.
/// Used by Shift+R reload.
pub fn force_fetch_item_detail(app: &mut App, item_idx: usize) {
    let (number, repo, item_id) = {
        let item = match app.items.get(item_idx) {
            Some(i) => i,
            None => return,
        };
        let number = item.number;
        let repo = item.repository.clone();
        (number, repo, item.id.clone())
    };

    let (number, repo) = match resolve_detail_params(app, item_idx, number, repo) {
        Some(pair) => pair,
        None => return,
    };

    let tx = app.action_tx.clone();
    app.detail_fetch_inflight.insert(item_id.clone());
    app.loading = true;
    app.set_status(&format!("Reloading #{} from {}...", number, repo));

    // Explicit reload also re-reads the live Status, which is why it is worth
    // the extra call here but not on every ticket the user opens.
    let is_issue = is_issue(app, item_idx);
    spawn_detail_fetch_action(tx, item_id, number, repo, is_issue, true);
}

/// Resolve number/repo into usable values, or set placeholder body and return None.
fn resolve_detail_params(
    app: &mut App,
    item_idx: usize,
    number: Option<u32>,
    repo: Option<String>,
) -> Option<(u32, String)> {
    let number = match number {
        Some(n) => n,
        None => {
            if let Some(item) = app.items.get_mut(item_idx) {
                item.body = Some("[Draft item - no issue linked]".to_string());
            }
            return None;
        }
    };

    let repo = match repo {
        Some(r) => {
            if r.contains('/') {
                r
            } else {
                format!("{}/{}", app.config.owner, r)
            }
        }
        None => {
            if let Some(item) = app.items.get_mut(item_idx) {
                item.body = Some("[No repository]".to_string());
            }
            return None;
        }
    };

    Some((number, repo))
}

/// Whether the card is an issue - the only kind with linked PRs to read.
fn is_issue(app: &App, item_idx: usize) -> bool {
    app.items
        .get(item_idx)
        .is_some_and(|i| i.content_type == crate::models::item::ContentType::Issue)
}

/// Spawn detail fetch thread - sends result via ActionResult (for action_rx).
/// This is the user-initiated Reload path, so it also refetches the live
/// project Status (`gh issue view` can't return it - it's a project field,
/// and without this the reload appeared to "not work" for status changes).
fn spawn_detail_fetch_action(
    tx: std::sync::mpsc::Sender<crate::app::ActionResult>,
    item_id: String,
    number: u32,
    repo: String,
    is_issue: bool,
    refresh_status: bool,
) {
    std::thread::spawn(move || {
        // The linked-PR lookup runs beside the issue fetch: it is a second
        // round trip of about the same length, and in series it doubled the
        // time to open a ticket.
        let prs = is_issue.then(|| {
            let repo = repo.clone();
            std::thread::spawn(move || {
                gh::issue::linked_prs(&repo, number).map_err(|e| e.to_string())
            })
        });
        let (body, comments, labels, created_at) = do_fetch_detail(number, &repo);
        let linked_prs = prs.map(|worker| {
            worker
                .join()
                .unwrap_or_else(|_| Err("linked-PR lookup crashed".to_string()))
        });

        if refresh_status {
            if let Ok(Some(remote)) = gh::item::get_item_status(&item_id) {
                let _ = tx.send(crate::app::ActionResult::ParentStatusChanged {
                    item_id: item_id.clone(),
                    status: crate::models::status::Status::from_str(&remote),
                });
            }
        }

        let _ = tx.send(crate::app::ActionResult::ItemDetailFetched {
            item_id,
            body,
            comments,
            labels,
            created_at,
            linked_prs,
        });
    });
}

/// Shared logic: fetch issue/PR details from GitHub API.
fn do_fetch_detail(
    number: u32,
    repo: &str,
) -> (
    Option<String>,
    Vec<crate::models::item::Comment>,
    Vec<String>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    #[allow(unused_assignments)]
    let mut body: Option<String> = None;
    let mut comments = Vec::new();
    let mut labels = Vec::new();
    let mut created_at = None;
    let mut _diag = String::new();

    let result = gh::issue::view(repo, number);
    let json_str = match result {
        Ok(json) => {
            _diag = format!("issue view OK ({} bytes)", json.len());
            Some(json)
        }
        Err(e) => {
            _diag = format!("issue view failed: {}, trying pr...", e);
            match crate::gh::client::exec(&[
                "pr",
                "view",
                &number.to_string(),
                "--repo",
                repo,
                "--json",
                "title,body,comments,labels,createdAt,number",
            ]) {
                Ok(json) => {
                    _diag = format!("pr view OK ({} bytes)", json.len());
                    Some(json)
                }
                Err(e2) => {
                    _diag = format!("Both failed: {}", e2);
                    None
                }
            }
        }
    };

    if let Some(json) = json_str {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
            body = Some(parsed["body"].as_str().unwrap_or("[No body]").to_string());
            if let Some(comment_arr) = parsed["comments"].as_array() {
                for c in comment_arr {
                    let author = c["author"]["login"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                    let c_body = c["body"].as_str().unwrap_or("").to_string();
                    let c_created_at = c["createdAt"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now);

                    let media = extract_media_from_body(&c_body);

                    comments.push(crate::models::item::Comment {
                        author,
                        body: c_body,
                        created_at: c_created_at,
                        media,
                    });
                }
            }
            if let Some(label_arr) = parsed["labels"].as_array() {
                labels = label_arr
                    .iter()
                    .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                    .collect();
            }
            if let Some(created) = parsed["createdAt"].as_str() {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(created) {
                    created_at = Some(dt.with_timezone(&chrono::Utc));
                }
            }
        } else {
            body = Some("[Failed to parse response]".to_string());
        }
    } else {
        let rate_info = gh::client::rate_limit_info();
        body = Some(format!(
            "[{} Could not load ticket details]\n[{}]\n[Press Shift+R to retry]",
            crate::ui::theme::icon_warning(),
            rate_info
        ));
    }

    (body, comments, labels, created_at)
}

/// Extract [img] and [vid] media references from a comment body.
pub fn extract_media_from_body(body: &str) -> Vec<crate::models::item::MediaItem> {
    let mut media = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.contains("<img") && trimmed.contains("src=") {
            if let Some(start) = trimmed.find("src=\"") {
                let url_start = start + 5;
                if let Some(end) = trimmed[url_start..].find('"') {
                    media.push(crate::models::item::MediaItem {
                        media_type: crate::models::item::MediaType::Image,
                        url: trimmed[url_start..url_start + end].to_string(),
                        alt: Some("screenshot".to_string()),
                        width: None,
                        height: None,
                    });
                }
            }
        } else if trimmed.contains("user-attachments/assets") {
            media.push(crate::models::item::MediaItem {
                media_type: crate::models::item::MediaType::Video,
                url: trimmed.to_string(),
                alt: Some("video".to_string()),
                width: None,
                height: None,
            });
        }
    }
    media
}

#[cfg(test)]
mod tests {

    fn app_with_issue(content_type: crate::models::item::ContentType) -> App {
        let mut app = App::new();
        app.items = vec![crate::models::item::Item {
            id: "PVTI_1".into(),
            number: Some(4019),
            repository: Some("Org/board".into()),
            content_type,
            ..Default::default()
        }];
        app
    }

    #[test]
    fn test_pick_list_deploy_check_skips_what_it_cannot_check() {
        use crate::models::item::{ContentType, Item};
        let mut app = App::new();
        app.items = vec![
            // A draft: no issue to read.
            Item {
                id: "draft".into(),
                ..Default::default()
            },
            // A pull request card.
            Item {
                id: "pr".into(),
                number: Some(7),
                repository: Some("Org/web".into()),
                content_type: ContentType::PullRequest,
                ..Default::default()
            },
        ];
        spawn_deploy_check_rows(&mut app, &[0, 1]);
        assert!(app
            .status_message
            .as_deref()
            .unwrap_or("")
            .starts_with("Nothing to check"));
        assert!(app.deploy_check_inflight.is_empty());
    }

    #[test]
    fn test_deploy_check_explains_why_it_cannot_run() {
        use crate::models::item::ContentType;
        // A pull request card has no linked PRs to read.
        let mut app = app_with_issue(ContentType::PullRequest);
        spawn_deploy_check(&mut app, 0);
        assert!(app
            .status_message
            .as_deref()
            .unwrap_or("")
            .contains("pull request card"));
        assert!(app.deploy_check_inflight.is_empty());

        // A failed lookup says so and points at reload, instead of "still
        // loading" forever.
        let mut app = app_with_issue(ContentType::Issue);
        app.linked_prs_failed
            .insert("PVTI_1".into(), "HTTP 502".into());
        spawn_deploy_check(&mut app, 0);
        let msg = app.status_message.clone().unwrap_or_default();
        assert!(
            msg.contains("could not be loaded") && msg.contains("HTTP 502"),
            "{}",
            msg
        );

        // Looked up, nothing linked.
        let mut app = app_with_issue(ContentType::Issue);
        app.linked_prs_loaded.insert("PVTI_1".into());
        spawn_deploy_check(&mut app, 0);
        assert!(app
            .status_message
            .as_deref()
            .unwrap_or("")
            .contains("no PRs linked"));
        assert!(app.deploy_check_inflight.is_empty());
    }

    use super::*;

    #[test]
    fn test_detail_fetch_is_not_gated_on_the_sync_channel() {
        let mut app = App::new();
        // A background sync in flight must not block opening a ticket: its
        // channel stays open for as long as the timeline fetch takes.
        let (_tx, rx) = std::sync::mpsc::channel();
        app.bg_rx = Some(rx);
        assert!(should_start_detail_fetch(&app, "PVTI_1"));

        // But the same ticket is never fetched twice at once.
        app.detail_fetch_inflight.insert("PVTI_1".to_string());
        assert!(!should_start_detail_fetch(&app, "PVTI_1"));
        // A different ticket still can be.
        assert!(should_start_detail_fetch(&app, "PVTI_2"));
    }

    #[test]
    fn test_sync_fetches_timelines_for_the_report_sprints_parked_tickets() {
        use crate::models::item::Item;
        use crate::models::status::Status;

        // #4000, #4001 and #4004 sat in Ready for Dev with no timeline through
        // every refresh: the sync only fetched active statuses, and the
        // Dashboard's own fetch is skipped whenever a sync is in flight.
        let mut item = Item {
            status: Status::ReadyForDev,
            sprint: Some("Sprint 29".into()),
            ..Default::default()
        };
        assert!(sync_fetches_timeline(&item, Some("Sprint 29")));
        item.status = Status::Backlog;
        assert!(sync_fetches_timeline(&item, Some("Sprint 29")));

        // Another sprint's parked tickets are not the report's problem.
        assert!(!sync_fetches_timeline(&item, Some("Sprint 30")));
        // No sprint to report on: parked tickets stay lazy, as before.
        assert!(!sync_fetches_timeline(&item, None));
    }

    #[test]
    fn test_sync_fetches_active_timelines_whatever_the_sprint() {
        use crate::models::item::Item;
        use crate::models::status::Status;

        let item = Item {
            status: Status::InQA,
            sprint: Some("Sprint 28".into()),
            ..Default::default()
        };
        assert!(sync_fetches_timeline(&item, Some("Sprint 29")));
        assert!(sync_fetches_timeline(&item, None));
    }

    #[test]
    fn test_timeline_cache_key_includes_the_repo() {
        // #42 exists in more than one repository of the same project.
        assert_ne!(
            timeline_cache_key("Org/web", 42),
            timeline_cache_key("Org/api", 42)
        );
    }
}
