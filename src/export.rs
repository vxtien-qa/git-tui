use crate::app::App;
use crate::cache::Cache;
use crate::models::status::Status;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveTime, Utc};

// ─── Time helpers ───────────────────────────────────────────────────────────

/// Paid working hours in the configured timezone, as two shifts.
///
/// The 11:30-13:00 lunch break is deliberately NOT paid time: work logged in
/// that window counts as OT, which is the team's policy.
const SHIFTS: [((u32, u32), (u32, u32)); 2] = [((7, 30), (11, 30)), ((13, 0), (17, 0))];

/// Check if a timestamp is Overtime (OT) in the configured timezone.
/// OT = weekends, or weekdays outside the paid shifts (lunch break included).
fn is_ot(dt_utc: DateTime<Utc>, offset_hours: i32) -> bool {
    let tz_offset = FixedOffset::east_opt(offset_hours * 3600)
        .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    let dt_local = dt_utc.with_timezone(&tz_offset);

    if dt_local.weekday() == chrono::Weekday::Sat || dt_local.weekday() == chrono::Weekday::Sun {
        return true;
    }

    let time = dt_local.time();
    let in_shift = SHIFTS.iter().any(|((sh, sm), (eh, em))| {
        let start = NaiveTime::from_hms_opt(*sh, *sm, 0).unwrap();
        let end = NaiveTime::from_hms_opt(*eh, *em, 0).unwrap();
        time >= start && time <= end
    });

    !in_shift
}

/// Check if a timestamp falls on the "previous workday" in the configured timezone.
/// On Monday: includes Friday, Saturday, Sunday (so Friday's work shows up).
/// On other weekdays: includes only yesterday.
fn is_previous_workday(dt_utc: DateTime<Utc>, offset_hours: i32) -> bool {
    let tz_offset = FixedOffset::east_opt(offset_hours * 3600).unwrap();
    let now_local = Utc::now().with_timezone(&tz_offset);
    let dt_local = dt_utc.with_timezone(&tz_offset);

    let today = now_local.date_naive();
    let event_date = dt_local.date_naive();

    match now_local.weekday() {
        // Monday: "yesterday" means Friday, Saturday, or Sunday
        chrono::Weekday::Mon => {
            let friday = today - Duration::days(3);
            event_date >= friday && event_date < today
        }
        // Other weekdays: just yesterday
        _ => {
            let yesterday = today - Duration::days(1);
            event_date == yesterday
        }
    }
}

/// Report header date in the CONFIGURED timezone - the rest of the report
/// classifies events by this offset, so the header must agree with it (the
/// machine-local clock can differ, e.g. a laptop on travel).
fn report_date(offset_hours: i32) -> String {
    let tz_offset = FixedOffset::east_opt(offset_hours * 3600)
        .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
    Utc::now()
        .with_timezone(&tz_offset)
        .format("%b %d, %Y")
        .to_string()
}

/// Check if a timestamp is Today in the configured timezone.
fn is_today(dt_utc: DateTime<Utc>, offset_hours: i32) -> bool {
    let tz_offset = FixedOffset::east_opt(offset_hours * 3600).unwrap();
    let now_local = Utc::now().with_timezone(&tz_offset);
    let dt_local = dt_utc.with_timezone(&tz_offset);

    dt_local.date_naive() == now_local.date_naive()
}

// ─── Shared data structures ─────────────────────────────────────────────────

use crate::models::item::Item;

/// An item tagged with whether it was done during OT.
struct TaggedItem<'a> {
    item: &'a Item,
    is_ot: bool,
}

/// Describes what QA action was performed on a ticket yesterday.
#[derive(Debug, Clone, PartialEq)]
enum ActionKind {
    /// InQA Dev → In Progress: ticket failed QA on Dev
    FailedOnDev,
    /// InQA → In Progress: ticket failed QA on STG
    FailedOnStg,
    /// InUAT → In Progress: ticket failed on UAT
    FailedOnUat,
    /// InQA Dev + label Ready-for-Staging added: ticket passed on Dev
    PassedDev,
    /// InQA + label Ready-for-UAT added: ticket passed on STG
    PassedStg,
    /// → Tech Complete: ticket verified, ready for release
    VerifiedForRelease,
    /// → Done: testing complete
    TestComplete,
    /// → Blocked: ticket is blocked
    Blocked,
    /// → Backlog: ticket moved back to backlog
    MovedToBacklog,
    /// Comment only, no status change by user
    Commented,
    /// Other status change by user (catch-all)
    WorkedOn(String),
}

impl ActionKind {
    /// Human-readable description for the report.
    fn description(&self) -> &str {
        match self {
            Self::FailedOnDev => "Failed on Dev",
            Self::FailedOnStg => "Failed on STG",
            Self::FailedOnUat => "Failed on UAT",
            Self::PassedDev => "Passed on Dev, waiting for STG deploy",
            Self::PassedStg => "Passed on STG, waiting for UAT deploy",
            Self::VerifiedForRelease => "Verified, ready for release",
            Self::TestComplete => "Testing complete",
            Self::Blocked => "Blocked, needs further action",
            Self::MovedToBacklog => "Moved to Backlog, will handle later",
            Self::Commented => "Reviewed & commented, needs confirmation",
            Self::WorkedOn(_) => "Worked on ticket",
        }
    }

    /// Full description.
    fn full_description(&self) -> String {
        match self {
            Self::WorkedOn(status) if !status.is_empty() => {
                format!("Worked on ticket ({})", status)
            }
            other => other.description().to_string(),
        }
    }

    /// Significance level for chronological overwriting.
    /// Higher number = more significant (major status change > minor > comment).
    fn significance(&self) -> u8 {
        match self {
            Self::TestComplete
            | Self::VerifiedForRelease
            | Self::PassedDev
            | Self::PassedStg
            | Self::FailedOnStg
            | Self::FailedOnUat
            | Self::FailedOnDev
            | Self::Blocked
            | Self::MovedToBacklog => 3,
            Self::WorkedOn(_) => 2,
            Self::Commented => 1,
        }
    }

    /// Priority for dedup/sorting: lower = more important (shown first in report).
    fn priority(&self) -> u8 {
        match self {
            Self::FailedOnDev => 0,
            Self::FailedOnStg => 1,
            Self::FailedOnUat => 2,
            Self::PassedDev => 3,
            Self::PassedStg => 4,
            Self::VerifiedForRelease => 5,
            Self::TestComplete => 6,
            Self::Blocked => 7,
            Self::MovedToBacklog => 8,
            Self::WorkedOn(_) => 9,
            Self::Commented => 10,
        }
    }
}

/// A yesterday action with context.
struct YesterdayAction<'a> {
    item: &'a Item,
    kind: ActionKind,
    is_ot: bool,
}

/// Collected data for a Daily Standup report.
struct DailyStandupData<'a> {
    sprint: String,
    user: String,
    date: String,
    yesterday: Vec<YesterdayAction<'a>>,
    in_qa: Vec<TaggedItem<'a>>,
    blocked: Vec<TaggedItem<'a>>,
    bugs_found: Vec<TaggedItem<'a>>,
    /// Sprint items with NO timeline data available - the Yesterday section
    /// may be undercounted while these are still being fetched.
    missing_history: usize,
}

/// Collected data for a Sprint Report.
struct SprintReportData<'a> {
    sprint: String,
    user: String,
    date: String,
    /// Sprint items with NO timeline data available - QA metrics may be
    /// undercounted while these are still being fetched.
    missing_history: usize,
    total: usize,
    done_count: usize,
    done_pct: f64,
    // ─── Pipeline groups (add up to total) ───
    not_started_count: usize,
    in_dev_pipeline: usize,
    in_qa_pipeline: usize,
    in_uat_count: usize,
    tech_complete_count: usize,
    ready_release_count: usize,
    blocked_count: usize,
    // ─── QA Metrics ───
    tested_count: usize,
    passed_count: usize,
    failed_count: usize,
    retest_items: Vec<RetestInfo<'a>>,
    // ─── Current QA Queue ───
    tickets_to_test: usize,
    bugs_to_verify: Vec<&'a Item>,
    /// Items sitting in a QA column that QA already passed and that are
    /// waiting for a dev deploy. Counted separately: they are NOT work the QA
    /// still has to do, and reporting them as "to test" overstated the queue.
    waiting_deploy_count: usize,
    // ─── Bugs ───
    bugs_in_uat: Vec<&'a Item>,
    bugs_blocked: Vec<&'a Item>,
    bugs_for_dev: Vec<&'a Item>,
    bugs_waiting_deploy: Vec<&'a Item>,
}

/// Info about a ticket that was retested (failed then came back to QA).
struct RetestInfo<'a> {
    item: &'a Item,
    fail_count: usize,
    /// Whether the ticket's LATEST QA verdict is still a fail.
    currently_failed: bool,
}

/// The outcome of the most recent QA verdict on a ticket.
#[derive(Debug, Clone, Copy, PartialEq)]
enum QaVerdict {
    Passed,
    Failed,
}

/// Keep the later of two verdicts.
fn keep_latest(
    best: &mut Option<(QaVerdict, DateTime<Utc>)>,
    verdict: QaVerdict,
    at: DateTime<Utc>,
) {
    if best.map(|(_, t)| at > t).unwrap_or(true) {
        *best = Some((verdict, at));
    }
}

/// Derive a ticket's most recent QA verdict from its timeline.
///
/// Reads BOTH signals, which is the whole point: since v3.2.0 passing on Dev
/// and on STG only adds a label, so a status-only reading recorded every
/// failure the moment it happened but no pass until the ticket reached Tech
/// Complete. That biased the pass rate downward by construction and hid two
/// thirds of the QA work done in a sprint.
///
/// Returns `None` when the ticket has no QA verdict yet (never tested, or
/// currently sitting in a QA column awaiting a first verdict).
fn latest_qa_verdict(tl: &ItemTimeline) -> Option<(QaVerdict, DateTime<Utc>)> {
    let mut best: Option<(QaVerdict, DateTime<Utc>)> = None;

    for e in &tl.status_events {
        let prev = e.previous_status.as_deref().unwrap_or("").to_lowercase();
        let curr = e.status.as_deref().unwrap_or("").to_lowercase();
        // Only transitions OUT of a QA column are QA verdicts. Without this a
        // product owner parking a ticket in Backlog looked like a QA failure.
        if !(prev.contains("in qa") || prev.contains("in uat")) {
            continue;
        }
        if curr.contains("in progress") {
            keep_latest(&mut best, QaVerdict::Failed, e.created_at);
        } else if curr.contains("tech complete")
            || curr.contains("ready for release")
            || curr == "done"
        {
            keep_latest(&mut best, QaVerdict::Passed, e.created_at);
        }
    }

    for l in &tl.label_events {
        if App::label_eq(&l.label, "Ready-for-Staging") || App::label_eq(&l.label, "Ready-for-UAT")
        {
            keep_latest(&mut best, QaVerdict::Passed, l.created_at);
        }
    }

    best
}

// ─── Action detection helpers ───────────────────────────────────────────────

/// Classify a status transition as a QA action.
/// Returns None ONLY for transitions that are definitively NOT done by QA.
fn classify_status_change(previous: Option<&str>, current: Option<&str>) -> Option<ActionKind> {
    let prev = previous.unwrap_or("");
    let curr = current.unwrap_or("");

    let prev_lower = prev.to_lowercase();
    let curr_lower = curr.to_lowercase();

    // ── Definitively NOT QA actions ──
    // Dev puts ticket into QA queue, or deploys to UAT and moves it there
    if curr_lower == "in qa - dev" || curr_lower == "in qa" || curr_lower == "in uat" {
        return None;
    }
    // Dev submits PR for review
    if curr_lower == "in review" {
        return None;
    }

    // ── Specific QA actions ──
    // QA fails ticket → moves back to In Progress
    if curr_lower.contains("in progress") {
        if prev_lower.contains("in qa") && prev_lower.contains("dev") {
            return Some(ActionKind::FailedOnDev);
        }
        if prev_lower.contains("in qa") {
            return Some(ActionKind::FailedOnStg);
        }
        if prev_lower.contains("in uat") {
            return Some(ActionKind::FailedOnUat);
        }
        // In Progress from other columns - could be user, show as generic
        return Some(ActionKind::WorkedOn(curr.to_string()));
    }

    // QA verifies → Tech Complete or Ready for Release
    if curr_lower.contains("tech complete") || curr_lower.contains("ready for release") {
        return Some(ActionKind::VerifiedForRelease);
    }

    // QA completes testing → Done
    if curr_lower == "done" {
        return Some(ActionKind::TestComplete);
    }

    // QA blocks ticket
    if curr_lower.contains("blocked") {
        return Some(ActionKind::Blocked);
    }

    // QA moves to backlog
    if curr_lower == "backlog" {
        return Some(ActionKind::MovedToBacklog);
    }

    // Everything else done by user - show as generic
    Some(ActionKind::WorkedOn(curr.to_string()))
}

// ─── Data collection ────────────────────────────────────────────────────────

/// Collect all data needed for a Daily Standup report.
fn collect_daily_standup(app: &App) -> DailyStandupData<'_> {
    let tz = app.config.timezone_offset_hours;
    let sprint = app
        .board_sprint_filter
        .as_deref()
        .unwrap_or("All Sprints")
        .to_string();
    let user = app.current_user.clone();
    let date = report_date(tz);

    // Timeline cache for loading status_history when not in memory
    let timeline_cache = Cache::new(1800);

    let my_items: Vec<_> = app
        .items
        .iter()
        .filter(|i| {
            i.assignees.iter().any(|a| a == &user)
                && (app.board_sprint_filter.is_none()
                    || i.sprint.as_deref() == app.board_sprint_filter.as_deref())
        })
        .collect();

    // ── Yesterday: detect specific QA actions by current user ──
    // Scan ALL items in the sprint (not just assigned to user),
    // because QA changes status on tickets they may not be assigned to.
    let mut yesterday = Vec::new();
    // Track items already added (by id) to avoid duplicates
    let mut seen_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();

    let sprint_items: Vec<_> = app
        .items
        .iter()
        .filter(|i| {
            app.board_sprint_filter.is_none()
                || i.sprint.as_deref() == app.board_sprint_filter.as_deref()
        })
        .collect();

    let mut missing_history = 0usize;

    for item in &sprint_items {
        // Load timeline data: prefer in-memory, fallback to cache
        let tl = load_timeline_for_item(item, &timeline_cache);
        if tl.missing {
            missing_history += 1;
        }
        let status_events = &tl.status_events;
        let comment_events = &tl.comment_events;

        // 1. Check status changes made BY current user yesterday
        let mut best_action: Option<(ActionKind, bool)> = None;

        for evt in status_events {
            // Only events explicitly by the current user. (An actor-less
            // fallback used to attribute a co-assigned DEV's move to the QA.)
            let actor_match = evt.actor.as_deref().map(|a| a == user).unwrap_or(false);
            if !actor_match {
                continue;
            }
            if !is_previous_workday(evt.created_at, tz) {
                continue;
            }

            let ot_flag = is_ot(evt.created_at, tz);
            let kind =
                classify_status_change(evt.previous_status.as_deref(), evt.status.as_deref());

            // Skip non-QA actions (dev moving ticket to InQA, etc.)
            let kind = match kind {
                Some(k) => k,
                None => continue,
            };

            // Keep the most recent significant action (highest significance number wins)
            match &best_action {
                Some((existing, prev_ot)) if kind.significance() >= existing.significance() => {
                    // OT sticks: any OT event on the ticket keeps the flag.
                    let ot = ot_flag || *prev_ot;
                    best_action = Some((kind, ot));
                }
                None => {
                    best_action = Some((kind, ot_flag));
                }
                _ => {
                    // Keep OT flag if any event is OT
                    if ot_flag {
                        if let Some((_, ref mut existing_ot)) = best_action {
                            *existing_ot = true;
                        }
                    }
                }
            }
        }

        // 2. Label-based passes: Pass Dev / Pass STG are LABEL-ONLY actions.
        // Detected from LabeledEvent timeline data - label added yesterday by
        // the current user. (The old inference from current labels both missed
        // clean passes and re-reported week-old ones.)
        if best_action.is_none() {
            for lev in &tl.label_events {
                let actor_match = lev.actor.as_deref().map(|a| a == user).unwrap_or(false);
                if !actor_match || !is_previous_workday(lev.created_at, tz) {
                    continue;
                }
                let kind = if App::label_eq(&lev.label, "Ready-for-Staging") {
                    Some(ActionKind::PassedDev)
                } else if App::label_eq(&lev.label, "Ready-for-UAT") {
                    Some(ActionKind::PassedStg)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    best_action = Some((kind, is_ot(lev.created_at, tz)));
                    break;
                }
            }
        }

        // 3. If no status change by user, check for comments by user yesterday
        if best_action.is_none() {
            for cev in comment_events {
                let author_match = cev.author.as_deref().map(|a| a == user).unwrap_or(false);
                if !author_match {
                    continue;
                }
                if is_previous_workday(cev.created_at, tz) {
                    let ot_flag = is_ot(cev.created_at, tz);
                    best_action = Some((ActionKind::Commented, ot_flag));
                    break;
                }
            }
        }

        if let Some((kind, ot_flag)) = best_action {
            if seen_ids.insert(&item.id) {
                yesterday.push(YesterdayAction {
                    item,
                    kind,
                    is_ot: ot_flag,
                });
            }
        }
    }

    // 4. Bug Found: bugs created yesterday or today by the user
    let mut bugs_found: Vec<TaggedItem<'_>> = Vec::new();
    for item in &app.items {
        if app.board_sprint_filter.is_some()
            && item.sprint.as_deref() != app.board_sprint_filter.as_deref()
        {
            continue;
        }
        let is_bug = item.labels.iter().any(|l| l.to_lowercase().contains("bug"));
        if !is_bug {
            continue;
        }
        let created_recently = item
            .created_at
            .map(|dt| is_previous_workday(dt, tz) || is_today(dt, tz))
            .unwrap_or(false);
        if !created_recently {
            continue;
        }
        // Heuristic: user is in assignees → likely the reporter
        let user_involved = item.assignees.iter().any(|a| a == &user);
        if user_involved {
            let ot_flag = item.created_at.map(|dt| is_ot(dt, tz)).unwrap_or(false);
            bugs_found.push(TaggedItem {
                item,
                is_ot: ot_flag,
            });
        }
    }

    // Sort yesterday actions by priority (most important first)
    yesterday.sort_by_key(|a| a.kind.priority());

    // Doing today: all InQA/InQADev/InUAT items assigned to user, sorted by priority.
    // Exclude items with Ready-for-* handoff labels (synced with My Tasks filter).
    let mut in_qa = Vec::new();
    for item in &my_items {
        if matches!(item.status, Status::InQA | Status::InQADev | Status::InUAT)
            && !App::item_excluded_from_my_tasks(item)
        {
            in_qa.push(TaggedItem { item, is_ot: false });
        }
    }
    in_qa.sort_by_key(|t| t.item.priority_sort_key());

    // Blockers: all blocked items (no time filter)
    let mut blocked = Vec::new();
    for item in &my_items {
        if item.status == Status::Blocked {
            // Use load_timeline_for_item for OT check (handles disk cache fallback)
            let blocker_events = load_timeline_for_item(item, &timeline_cache).status_events;
            let mut ot_flag = false;
            for evt in &blocker_events {
                if let Some(status) = &evt.status {
                    if status.to_lowercase().contains("blocked")
                        && (is_previous_workday(evt.created_at, tz) || is_today(evt.created_at, tz))
                        && is_ot(evt.created_at, tz)
                    {
                        ot_flag = true;
                    }
                }
            }
            blocked.push(TaggedItem {
                item,
                is_ot: ot_flag,
            });
        }
    }

    // (Bugs Reported section removed - merged into Bug Found above)

    DailyStandupData {
        sprint,
        user,
        date,
        yesterday,
        in_qa,
        blocked,
        bugs_found,
        missing_history,
    }
}

/// Timeline events for one item, plus whether any data was available at all.
struct ItemTimeline {
    status_events: Vec<crate::gh::timeline::StatusChangedEvent>,
    comment_events: Vec<crate::gh::timeline::CommentEvent>,
    label_events: Vec<crate::gh::timeline::LabelEvent>,
    /// True when nothing was found in memory OR cache - the caller cannot
    /// distinguish "ticket has no events" from "events not fetched yet", so
    /// reports must surface this instead of silently undercounting.
    missing: bool,
}

/// Load timeline events for an item: prefer in-memory, fallback to disk cache.
fn load_timeline_for_item(item: &Item, timeline_cache: &Cache) -> ItemTimeline {
    if let Some(ref history) = item.status_history {
        return ItemTimeline {
            status_events: history.clone(),
            comment_events: item.comment_history.clone().unwrap_or_default(),
            label_events: item.label_history.clone().unwrap_or_default(),
            missing: false,
        };
    }
    if let (Some(repo), Some(number)) = (item.repository.as_deref(), item.number) {
        let cache_key = crate::loader::timeline_cache_key(repo, number);
        if let (Some(td), _) =
            timeline_cache.get_with_freshness::<crate::gh::timeline::TimelineData>(&cache_key)
        {
            return ItemTimeline {
                status_events: td.status_changes,
                comment_events: td.comments,
                label_events: td.labels,
                missing: false,
            };
        }
    }
    ItemTimeline {
        status_events: Vec::new(),
        comment_events: Vec::new(),
        label_events: Vec::new(),
        missing: true,
    }
}

/// Collect all data needed for a Sprint Report.
fn collect_sprint_report(app: &App) -> SprintReportData<'_> {
    let tz = app.config.timezone_offset_hours;
    let sprint = app
        .board_sprint_filter
        .as_deref()
        .unwrap_or("All Sprints")
        .to_string();
    let user = app.current_user.clone();
    let date = report_date(tz);

    let timeline_cache = Cache::new(1800);

    let items: Vec<_> = app
        .items
        .iter()
        .filter(|i| {
            app.board_sprint_filter.is_none()
                || i.sprint.as_deref() == app.board_sprint_filter.as_deref()
        })
        .collect();

    let total = items.len();
    let done_count = items
        .iter()
        .filter(|i| matches!(i.status, Status::Done))
        .count();
    let done_pct = if total > 0 {
        done_count as f64 * 100.0 / total as f64
    } else {
        0.0
    };

    // ─── QA Metrics from timeline data ───
    //
    // Every ticket with a QA verdict counts as tested, and the verdict is the
    // LATEST one (pass or fail), so `passed + failed == tested` holds by
    // construction. Deriving "failed" from the current column instead used to
    // brand any ticket that merely left QA (parked in Backlog, blocked on
    // infra) a QA failure, while the retest list below counted real events -
    // two different definitions of "failed" in one report.
    let mut tested_count = 0usize;
    let mut passed_count = 0usize;
    let mut failed_count = 0usize;
    let mut retest_items: Vec<RetestInfo> = Vec::new();
    let mut missing_history = 0usize;

    for item in &items {
        let tl = load_timeline_for_item(item, &timeline_cache);
        if tl.missing {
            missing_history += 1;
        }
        if tl.status_events.is_empty() && tl.label_events.is_empty() {
            continue;
        }

        let verdict = latest_qa_verdict(&tl);
        match verdict {
            Some((QaVerdict::Passed, _)) => {
                tested_count += 1;
                passed_count += 1;
            }
            Some((QaVerdict::Failed, _)) => {
                tested_count += 1;
                failed_count += 1;
            }
            // In a QA column awaiting its first verdict: pending, not tested.
            None => {}
        }

        // Count historical failures for retest tracking:
        // InQA/InQADev/InUAT -> InProgress
        let fail_count = tl
            .status_events
            .iter()
            .filter(|e| {
                let prev_is_qa = e
                    .previous_status
                    .as_deref()
                    .map(|s| {
                        let low = s.to_lowercase();
                        low.contains("in qa") || low.contains("in uat")
                    })
                    .unwrap_or(false);
                let curr_is_progress = e
                    .status
                    .as_deref()
                    .map(|s| s.to_lowercase().contains("in progress"))
                    .unwrap_or(false);
                prev_is_qa && curr_is_progress
            })
            .count();

        // Track retested items: only if the ticket actually failed QA at least
        // once, meaning it was sent back and retested. Note: counting QA
        // entries is NOT reliable, because the normal flow
        // (In QA - Dev -> pass -> In QA) already produces two entries.
        if fail_count > 0 {
            retest_items.push(RetestInfo {
                item,
                fail_count,
                currently_failed: matches!(verdict, Some((QaVerdict::Failed, _))),
            });
        }
    }

    // Sort retested items by fail count descending
    retest_items.sort_by_key(|a| std::cmp::Reverse(a.fail_count));

    // ─── Pipeline-grouped counts (add up to total) ───
    let not_started_count = items
        .iter()
        .filter(|i| matches!(i.status, Status::Backlog | Status::ReadyForDev))
        .count();
    let in_dev_pipeline = items
        .iter()
        .filter(|i| matches!(i.status, Status::InProgress | Status::InReview))
        .count();
    let in_qa_pipeline = items
        .iter()
        .filter(|i| matches!(i.status, Status::InQA | Status::InQADev))
        .count();
    let in_uat_count = items
        .iter()
        .filter(|i| matches!(i.status, Status::InUAT))
        .count();
    let tech_complete_count = items
        .iter()
        .filter(|i| matches!(i.status, Status::TechComplete))
        .count();
    let ready_release_count = items
        .iter()
        .filter(|i| matches!(i.status, Status::ReadyForRelease))
        .count();
    let blocked_count = items
        .iter()
        .filter(|i| matches!(i.status, Status::Blocked))
        .count();

    // ─── Bugs ───
    // All items with bug label in sprint
    let all_bugs: Vec<_> = items
        .iter()
        .filter(|i| i.labels.iter().any(|l| l.to_lowercase().contains("bug")))
        .copied()
        .collect();

    // Bugs stuck: not resolved, not in QA, not being actively worked on by dev
    // (only Backlog + Blocked)
    let bugs_blocked: Vec<_> = all_bugs
        .iter()
        .filter(|i| matches!(i.status, Status::Backlog | Status::Blocked))
        .copied()
        .collect();

    // Bugs waiting for dev fix (ReadyForDev / InProgress)
    let bugs_for_dev: Vec<_> = all_bugs
        .iter()
        .filter(|i| {
            matches!(
                i.status,
                Status::ReadyForDev | Status::InProgress | Status::InReview
            )
        })
        .copied()
        .collect();

    // ─── Current QA Queue ───
    //
    // Split into work QA still has to do vs work already passed that is
    // waiting for a dev deploy. Both sit in a QA column, and reporting them
    // together as "to test" overstated the remaining queue - while My Tasks,
    // using the same exclusion helper, showed the smaller number.
    let in_qa_items: Vec<_> = items
        .iter()
        .filter(|i| matches!(i.status, Status::InQA | Status::InQADev))
        .copied()
        .collect();
    let (waiting_deploy, to_test): (Vec<&Item>, Vec<&Item>) = in_qa_items
        .iter()
        .copied()
        .partition(|i| App::item_excluded_from_my_tasks(i));
    let is_bug = |i: &&Item| i.labels.iter().any(|l| l.to_lowercase().contains("bug"));
    let bugs_to_verify: Vec<&Item> = to_test.iter().copied().filter(is_bug).collect();
    let tickets_to_test = to_test.len() - bugs_to_verify.len();
    let bugs_waiting_deploy: Vec<&Item> = waiting_deploy.iter().copied().filter(is_bug).collect();
    let waiting_deploy_count = waiting_deploy.len();

    // Bugs awaiting UAT verification - previously in NO bucket, so the
    // "Bugs (N open)" total silently undercounted them.
    let bugs_in_uat: Vec<_> = all_bugs
        .iter()
        .filter(|i| i.status == Status::InUAT)
        .copied()
        .collect();

    SprintReportData {
        sprint,
        user,
        date,
        missing_history,
        total,
        done_count,
        done_pct,
        not_started_count,
        in_dev_pipeline,
        in_qa_pipeline,
        in_uat_count,
        tech_complete_count,
        ready_release_count,
        blocked_count,
        tested_count,
        passed_count,
        failed_count,
        retest_items,
        tickets_to_test,
        bugs_to_verify,
        waiting_deploy_count,
        bugs_in_uat,
        bugs_blocked,
        bugs_for_dev,
        bugs_waiting_deploy,
    }
}

// ─── Daily Standup formatters ───────────────────────────────────────────────

/// Reports used to run silently against half-fetched timeline data (fresh
/// install, cleared cache) - numbers just came out low with no indication.
fn push_missing_history_warning(r: &mut String, missing: usize, bullet: &str) {
    if missing > 0 {
        r.push_str(&format!(
            "{}⚠ {} ticket(s) have no history data yet - numbers may be undercounted. Re-open the Dashboard in a minute and regenerate.\n\n",
            bullet, missing
        ));
    }
}

/// Build a GitHub issue link for markdown: `[#123](https://github.com/owner/repo/issues/123)`
fn issue_link_md(item: &Item) -> String {
    match (item.number, item.repository.as_deref()) {
        (Some(num), Some(repo)) => {
            let repo_path = if repo.starts_with("https://") {
                repo.to_string()
            } else {
                format!("https://github.com/{}", repo)
            };
            format!("[#{}]({}/issues/{})", num, repo_path, num)
        }
        (Some(num), None) => format!("#{}", num),
        _ => String::new(),
    }
}

/// Plain text ticket number: `#123`
fn issue_num_text(item: &Item) -> String {
    item.number.map(|n| format!("#{}", n)).unwrap_or_default()
}

/// Slack-formatted ticket link: `<https://github.com/org/repo/issues/123|#123>`
fn issue_link_slack(item: &Item, owner: &str) -> String {
    match (item.number, item.repository.as_deref()) {
        (Some(num), Some(repo)) => {
            let repo_path = if repo.contains('/') {
                repo.to_string()
            } else {
                format!("{}/{}", owner, repo)
            };
            format!("<https://github.com/{}/issues/{}|#{}>", repo_path, num, num)
        }
        (Some(num), None) => format!("#{}", num),
        _ => String::new(),
    }
}

/// Escaped title for webhook-sent Slack reports - titles are user-supplied
/// and must not be interpretable as Slack directives (see webhook::escape_mrkdwn).
fn title_slack(item: &Item) -> String {
    crate::slack::webhook::escape_mrkdwn(&item.title)
}

/// Resolve a GitHub username to a human-readable display name for copy-paste formats.
/// Returns `@slack_display` if mapped (e.g. "@Alex Lee"), else `@github_username`.
/// Use this for md/text reports that are copied and pasted into Slack.
/// (Unlike <@UID> which only works when sent via API/webhook.)
fn resolve_display_name(
    github_username: &str,
    user_map: &std::collections::HashMap<String, crate::config::SlackUserMapping>,
) -> String {
    if let Some(mapping) = user_map.get(github_username) {
        format!("@{}", mapping.slack_display)
    } else {
        format!("@{}", github_username)
    }
}

/// Generate a QA Daily Standup report in Markdown.
pub fn daily_standup_md(app: &App) -> String {
    let data = collect_daily_standup(app);
    let mut r = String::new();

    r.push_str(&format!("## QA Daily Standup - {}\n", data.date));
    r.push_str(&format!(
        "**QA:** @{} | **Sprint:** {}\n\n",
        data.user, data.sprint
    ));
    push_missing_history_warning(&mut r, data.missing_history, "- ");

    // Yesterday
    r.push_str("### Yesterday\n");
    if data.yesterday.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for a in &data.yesterday {
            let num = issue_link_md(a.item);
            let prio = a.item.priority.as_deref().unwrap_or("--");
            let ot = if a.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "- {} {} ({}, {}) → {}{}\n",
                num,
                a.item.title,
                a.item.status.label(),
                prio,
                a.kind.full_description(),
                ot
            ));
        }
    }

    // Doing today
    r.push_str("\n### Doing today\n");
    if data.in_qa.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for t in &data.in_qa {
            let num = issue_link_md(t.item);
            let prio = t.item.priority.as_deref().unwrap_or("--");
            r.push_str(&format!(
                "- Testing {} {} ({}, {})\n",
                num,
                t.item.title,
                t.item.status.label(),
                prio
            ));
        }
    }

    // Blockers
    if !data.blocked.is_empty() {
        r.push_str("\n### Blockers\n");
        for t in &data.blocked {
            let num = issue_link_md(t.item);
            let ot = if t.is_ot { " (OT)" } else { "" };
            r.push_str(&format!("- {} {} (Blocked){}\n", num, t.item.title, ot));
        }
    }

    // Bug Found
    r.push_str("\n### Bug Found\n");
    if data.bugs_found.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for t in &data.bugs_found {
            let num = issue_link_md(t.item);
            let prio = t.item.priority.as_deref().unwrap_or("--");
            let ot = if t.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "- {} {} ({}, {}){}\n",
                num,
                t.item.title,
                t.item.status.label(),
                prio,
                ot
            ));
        }
    }

    r
}

/// Generate a QA Daily Standup report in Plain Text (Slack/Teams friendly).
pub fn daily_standup_text(app: &App) -> String {
    let data = collect_daily_standup(app);
    let mut r = String::new();

    r.push_str(&format!("QA Daily Standup - {}\n", data.date));
    r.push_str(&format!("QA: @{} | Sprint: {}\n\n", data.user, data.sprint));
    push_missing_history_warning(&mut r, data.missing_history, "");

    // Yesterday
    r.push_str("Yesterday:\n");
    if data.yesterday.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for a in &data.yesterday {
            let num = issue_num_text(a.item);
            let prio = a.item.priority.as_deref().unwrap_or("--");
            let ot = if a.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "• {} {} ({}, {}) → {}{}\n",
                num,
                a.item.title,
                a.item.status.label(),
                prio,
                a.kind.full_description(),
                ot
            ));
        }
    }

    // Doing today
    r.push_str("\nDoing today:\n");
    if data.in_qa.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.in_qa {
            let num = t.item.number.map(|n| format!("#{}", n)).unwrap_or_default();
            let prio = t.item.priority.as_deref().unwrap_or("--");
            r.push_str(&format!(
                "• Testing {} {} ({}, {})\n",
                num,
                t.item.title,
                t.item.status.label(),
                prio
            ));
        }
    }

    // Blockers
    if !data.blocked.is_empty() {
        r.push_str("\nBlockers:\n");
        for t in &data.blocked {
            let num = issue_num_text(t.item);
            let ot = if t.is_ot { " (OT)" } else { "" };
            r.push_str(&format!("• {} {} (Blocked){}\n", num, t.item.title, ot));
        }
    }

    // Bug Found
    r.push_str("\nBug Found:\n");
    if data.bugs_found.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.bugs_found {
            let num = issue_num_text(t.item);
            let prio = t.item.priority.as_deref().unwrap_or("--");
            let ot = if t.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "• {} {} ({}, {}){}\n",
                num,
                t.item.title,
                t.item.status.label(),
                prio,
                ot
            ));
        }
    }

    r
}

/// Generate a QA Daily Standup report in Slack formatting.
pub fn daily_standup_slack(app: &App) -> String {
    let data = collect_daily_standup(app);
    let mut r = String::new();

    r.push_str(&format!("*QA Daily Standup - {}*\n", data.date));
    // Webhook posts under the app identity - say whose standup this is.
    r.push_str(&format!(
        "*QA:* {}  |  *Sprint:* {}\n\n",
        resolve_display_name(&data.user, &app.config.slack_user_map),
        data.sprint
    ));
    push_missing_history_warning(&mut r, data.missing_history, "");

    // Yesterday
    r.push_str("*Yesterday*\n");
    if data.yesterday.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for a in &data.yesterday {
            let num = issue_link_slack(a.item, &app.config.owner);
            let prio = a.item.priority.as_deref().unwrap_or("--");
            let ot = if a.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "• {} {} ({}, {}) → {}{}\n",
                num,
                title_slack(a.item),
                a.item.status.label(),
                prio,
                a.kind.full_description(),
                ot
            ));
        }
    }

    // Doing today
    r.push_str("\n*Doing today*\n");
    if data.in_qa.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.in_qa {
            let num = issue_link_slack(t.item, &app.config.owner);
            let prio = t.item.priority.as_deref().unwrap_or("--");
            r.push_str(&format!(
                "• Testing {} {} ({}, {})\n",
                num,
                title_slack(t.item),
                t.item.status.label(),
                prio
            ));
        }
    }

    // Blockers
    if !data.blocked.is_empty() {
        r.push_str("\n*Blockers*\n");
        for t in &data.blocked {
            let num = issue_link_slack(t.item, &app.config.owner);
            let ot = if t.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "• {} {} (Blocked){}\n",
                num,
                title_slack(t.item),
                ot
            ));
        }
    }

    // Bug Found
    r.push_str("\n*Bugs found*\n");
    if data.bugs_found.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.bugs_found {
            let num = issue_link_slack(t.item, &app.config.owner);
            let prio = t.item.priority.as_deref().unwrap_or("--");
            let ot = if t.is_ot { " (OT)" } else { "" };
            r.push_str(&format!(
                "• {} {} ({}, {}){}\n",
                num,
                title_slack(t.item),
                t.item.status.label(),
                prio,
                ot
            ));
        }
    }

    r
}

// ─── Sprint Report formatters ───────────────────────────────────────────────

/// QA queue summary: "10 (4 to test + 2 bugs to verify + 4 waiting for deploy)".
///
/// The parts add up to the In QA column count, so the pipeline still balances
/// while the number QA actually has to act on is stated separately.
fn in_qa_summary(d: &SprintReportData) -> String {
    let mut parts = vec![
        format!("{} to test", d.tickets_to_test),
        format!("{} bugs to verify", d.bugs_to_verify.len()),
    ];
    if d.waiting_deploy_count > 0 {
        parts.push(format!("{} waiting for deploy", d.waiting_deploy_count));
    }
    format!("{} ({})", d.in_qa_pipeline, parts.join(" + "))
}

/// Open bugs across every bucket the report prints (they must agree).
fn total_open_bugs(d: &SprintReportData) -> usize {
    d.bugs_to_verify.len()
        + d.bugs_waiting_deploy.len()
        + d.bugs_in_uat.len()
        + d.bugs_for_dev.len()
        + d.bugs_blocked.len()
}

/// Build a priority summary string from a list of items, e.g. " (1×P0, 2×P1, 3×P4)".
fn prio_summary(items: &[&Item]) -> String {
    let mut parts = Vec::new();
    for prio in &["P0", "P1", "P2", "P3", "P4"] {
        let count = items
            .iter()
            .filter(|i| i.priority.as_deref() == Some(prio))
            .count();
        if count > 0 {
            parts.push(format!("{}×{}", count, prio));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// Generate a Sprint Report in Markdown.
pub fn sprint_report_md(app: &App) -> String {
    let data = collect_sprint_report(app);
    let mut r = String::new();

    r.push_str(&format!("## QA Sprint Report - {}\n", data.sprint));
    r.push_str(&format!(
        "**QA:** @{} | **Date:** {}\n\n",
        data.user, data.date
    ));
    push_missing_history_warning(&mut r, data.missing_history, "- ");

    // ── Sprint Overview (pipeline groups that add up to total) ──
    r.push_str(&format!("### Sprint Overview ({} tickets)\n", data.total));
    if data.not_started_count > 0 {
        r.push_str(&format!("- Not started: {}\n", data.not_started_count));
    }
    if data.in_dev_pipeline > 0 {
        r.push_str(&format!("- In Dev: {}\n", data.in_dev_pipeline));
    }
    r.push_str(&format!("- In QA: {}\n", in_qa_summary(&data)));
    if data.in_uat_count > 0 {
        r.push_str(&format!("- In UAT: {}\n", data.in_uat_count));
    }
    if data.tech_complete_count > 0 {
        r.push_str(&format!("- Tech Complete: {}\n", data.tech_complete_count));
    }
    if data.ready_release_count > 0 {
        r.push_str(&format!(
            "- Ready for Release: {}\n",
            data.ready_release_count
        ));
    }
    r.push_str(&format!(
        "- Done: {} ({:.0}%)\n",
        data.done_count, data.done_pct
    ));
    if data.blocked_count > 0 {
        r.push_str(&format!("- Blocked: {}\n", data.blocked_count));
    }

    // ── Bugs ──
    let total_bugs = total_open_bugs(&data);
    if total_bugs > 0 {
        r.push_str(&format!("\n### Bugs ({} open)\n", total_bugs));
        if !data.bugs_to_verify.is_empty() {
            r.push_str(&format!(
                "- In QA: {}{}\n",
                data.bugs_to_verify.len(),
                prio_summary(&data.bugs_to_verify)
            ));
        }
        if !data.bugs_waiting_deploy.is_empty() {
            r.push_str(&format!(
                "- Passed, waiting for deploy: {}{}\n",
                data.bugs_waiting_deploy.len(),
                prio_summary(&data.bugs_waiting_deploy)
            ));
        }
        if !data.bugs_in_uat.is_empty() {
            r.push_str(&format!(
                "- In UAT: {}{}\n",
                data.bugs_in_uat.len(),
                prio_summary(&data.bugs_in_uat)
            ));
        }
        if !data.bugs_for_dev.is_empty() {
            r.push_str(&format!(
                "- Waiting for dev fix: {}{}\n",
                data.bugs_for_dev.len(),
                prio_summary(&data.bugs_for_dev)
            ));
        }
        if !data.bugs_blocked.is_empty() {
            r.push_str(&format!(
                "- Blocked: {}{}\n",
                data.bugs_blocked.len(),
                prio_summary(&data.bugs_blocked)
            ));
        }
    }

    // ── QA Results ──
    if data.tested_count > 0 {
        let pass_pct = data.passed_count as f64 * 100.0 / data.tested_count as f64;
        r.push_str(&format!(
            "\n### QA Results ({} tested)\n",
            data.tested_count
        ));
        r.push_str(&format!(
            "- Passed: {} ({:.0}%)\n",
            data.passed_count, pass_pct
        ));
        if data.failed_count > 0 {
            let fail_pct = data.failed_count as f64 * 100.0 / data.tested_count as f64;
            r.push_str(&format!(
                "- Failed: {} ({:.0}%)\n",
                data.failed_count, fail_pct
            ));
        }
    }

    // Failed / Retested tickets (only currently-failed: exclude passed and still-in-QA)
    let failed_retest: Vec<_> = data
        .retest_items
        .iter()
        // Latest verdict, not current column: a ticket parked in Backlog after
        // a QA fail belongs here, and one that passed on retest does not.
        .filter(|rt| rt.currently_failed)
        .collect();
    if !failed_retest.is_empty() {
        r.push_str("\n### Failed / Retested Tickets\n");
        for rt in &failed_retest {
            let num = issue_link_md(rt.item);
            let prio = rt.item.priority.as_deref().unwrap_or("--");
            r.push_str(&format!(
                "- {} {} ({}, {}, failed {}×)\n",
                num,
                rt.item.title,
                prio,
                rt.item.status.label(),
                rt.fail_count
            ));
        }
    }

    // Bugs waiting for dev
    if !data.bugs_for_dev.is_empty() {
        r.push_str("\n### Bugs Waiting for Dev\n");
        r.push_str("| # | Title | Priority | Status | Assignee |\n");
        r.push_str("|---|---|---|---|---|\n");
        for bug in &data.bugs_for_dev {
            let num = bug.number.map(|n| format!("#{}", n)).unwrap_or_default();
            let prio = bug.priority.as_deref().unwrap_or("--");
            let assignee = bug
                .assignees
                .iter()
                .map(|a| resolve_display_name(a, &app.config.slack_user_map))
                .collect::<Vec<_>>()
                .join(" ");
            r.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                num,
                bug.title,
                prio,
                bug.status.label(),
                assignee
            ));
        }
    }

    r
}

/// Generate a Sprint Report in Plain Text.
pub fn sprint_report_text(app: &App) -> String {
    let data = collect_sprint_report(app);
    let mut r = String::new();

    r.push_str(&format!("QA Sprint Report - {}\n", data.sprint));
    r.push_str(&format!("QA: @{} | Date: {}\n\n", data.user, data.date));
    push_missing_history_warning(&mut r, data.missing_history, "");

    // ── Sprint Overview ──
    r.push_str(&format!("Sprint Overview ({} tickets):\n", data.total));
    if data.not_started_count > 0 {
        r.push_str(&format!("• Not started: {}\n", data.not_started_count));
    }
    if data.in_dev_pipeline > 0 {
        r.push_str(&format!("• In Dev: {}\n", data.in_dev_pipeline));
    }
    r.push_str(&format!("• In QA: {}\n", in_qa_summary(&data)));
    if data.in_uat_count > 0 {
        r.push_str(&format!("• In UAT: {}\n", data.in_uat_count));
    }
    if data.tech_complete_count > 0 {
        r.push_str(&format!("• Tech Complete: {}\n", data.tech_complete_count));
    }
    if data.ready_release_count > 0 {
        r.push_str(&format!(
            "• Ready for Release: {}\n",
            data.ready_release_count
        ));
    }
    r.push_str(&format!(
        "• Done: {} ({:.0}%)\n",
        data.done_count, data.done_pct
    ));
    if data.blocked_count > 0 {
        r.push_str(&format!("• Blocked: {}\n", data.blocked_count));
    }

    // ── Bugs ──
    let total_bugs = total_open_bugs(&data);
    if total_bugs > 0 {
        r.push_str(&format!("\nBugs ({} open):\n", total_bugs));
        if !data.bugs_to_verify.is_empty() {
            r.push_str(&format!(
                "• In QA: {}{}\n",
                data.bugs_to_verify.len(),
                prio_summary(&data.bugs_to_verify)
            ));
        }
        if !data.bugs_waiting_deploy.is_empty() {
            r.push_str(&format!(
                "• Passed, waiting for deploy: {}{}\n",
                data.bugs_waiting_deploy.len(),
                prio_summary(&data.bugs_waiting_deploy)
            ));
        }
        if !data.bugs_in_uat.is_empty() {
            r.push_str(&format!(
                "• In UAT: {}{}\n",
                data.bugs_in_uat.len(),
                prio_summary(&data.bugs_in_uat)
            ));
        }
        if !data.bugs_for_dev.is_empty() {
            r.push_str(&format!(
                "• Waiting for dev fix: {}{}\n",
                data.bugs_for_dev.len(),
                prio_summary(&data.bugs_for_dev)
            ));
        }
        if !data.bugs_blocked.is_empty() {
            r.push_str(&format!(
                "• Blocked: {}{}\n",
                data.bugs_blocked.len(),
                prio_summary(&data.bugs_blocked)
            ));
        }
    }

    // ── QA Results ──
    if data.tested_count > 0 {
        let pass_pct = data.passed_count as f64 * 100.0 / data.tested_count as f64;
        r.push_str(&format!("\nQA Results ({} tested):\n", data.tested_count));
        r.push_str(&format!(
            "• Passed: {} ({:.0}%)\n",
            data.passed_count, pass_pct
        ));
        if data.failed_count > 0 {
            let fail_pct = data.failed_count as f64 * 100.0 / data.tested_count as f64;
            r.push_str(&format!(
                "• Failed: {} ({:.0}%)\n",
                data.failed_count, fail_pct
            ));
        }
    }

    // Failed / Retested tickets (only currently-failed)
    let failed_retest: Vec<_> = data
        .retest_items
        .iter()
        // Latest verdict, not current column: a ticket parked in Backlog after
        // a QA fail belongs here, and one that passed on retest does not.
        .filter(|rt| rt.currently_failed)
        .collect();
    if !failed_retest.is_empty() {
        r.push_str("\nFailed / Retested Tickets:\n");
        for rt in &failed_retest {
            let num = issue_num_text(rt.item);
            let prio = rt.item.priority.as_deref().unwrap_or("--");
            r.push_str(&format!(
                "• {} {} ({}, {}, failed {}×)\n",
                num,
                rt.item.title,
                prio,
                rt.item.status.label(),
                rt.fail_count
            ));
        }
    }

    // Bugs waiting for dev
    if !data.bugs_for_dev.is_empty() {
        r.push_str("\nBugs Waiting for Dev:\n");
        for bug in &data.bugs_for_dev {
            let num = bug.number.map(|n| format!("#{}", n)).unwrap_or_default();
            let prio = bug.priority.as_deref().unwrap_or("--");
            let assignee = bug
                .assignees
                .iter()
                .map(|a| resolve_display_name(a, &app.config.slack_user_map))
                .collect::<Vec<_>>()
                .join(" ");
            r.push_str(&format!(
                "• {} {} | {} | {} | {}\n",
                num,
                bug.title,
                prio,
                bug.status.label(),
                assignee
            ));
        }
    }

    r
}

/// Generate a Sprint Report in Slack formatting.
pub fn sprint_report_slack(app: &App) -> String {
    let data = collect_sprint_report(app);
    let mut r = String::new();

    r.push_str(&format!("*QA Sprint Report - {}*\n", data.sprint));
    r.push_str(&format!("*Date:* {}\n\n", data.date));
    push_missing_history_warning(&mut r, data.missing_history, "");

    // ── Sprint Overview ──
    r.push_str(&format!("*Sprint Overview* ({} tickets)\n", data.total));
    if data.not_started_count > 0 {
        r.push_str(&format!("• Not started: {}\n", data.not_started_count));
    }
    if data.in_dev_pipeline > 0 {
        r.push_str(&format!("• In Dev: {}\n", data.in_dev_pipeline));
    }
    r.push_str(&format!("• In QA: {}\n", in_qa_summary(&data)));
    if data.in_uat_count > 0 {
        r.push_str(&format!("• In UAT: {}\n", data.in_uat_count));
    }
    if data.tech_complete_count > 0 {
        r.push_str(&format!("• Tech Complete: {}\n", data.tech_complete_count));
    }
    if data.ready_release_count > 0 {
        r.push_str(&format!(
            "• Ready for Release: {}\n",
            data.ready_release_count
        ));
    }
    r.push_str(&format!(
        "• Done: {} ({:.0}%)\n",
        data.done_count, data.done_pct
    ));
    if data.blocked_count > 0 {
        r.push_str(&format!("• Blocked: {}\n", data.blocked_count));
    }

    // ── Bugs ──
    let total_bugs = total_open_bugs(&data);
    if total_bugs > 0 {
        r.push_str(&format!("\n*Bugs* ({} open)\n", total_bugs));
        if !data.bugs_to_verify.is_empty() {
            r.push_str(&format!(
                "• In QA: {}{}\n",
                data.bugs_to_verify.len(),
                prio_summary(&data.bugs_to_verify)
            ));
        }
        if !data.bugs_waiting_deploy.is_empty() {
            r.push_str(&format!(
                "• Passed, waiting for deploy: {}{}\n",
                data.bugs_waiting_deploy.len(),
                prio_summary(&data.bugs_waiting_deploy)
            ));
        }
        if !data.bugs_in_uat.is_empty() {
            r.push_str(&format!(
                "• In UAT: {}{}\n",
                data.bugs_in_uat.len(),
                prio_summary(&data.bugs_in_uat)
            ));
        }
        if !data.bugs_for_dev.is_empty() {
            r.push_str(&format!(
                "• Waiting for dev fix: {}{}\n",
                data.bugs_for_dev.len(),
                prio_summary(&data.bugs_for_dev)
            ));
        }
        if !data.bugs_blocked.is_empty() {
            r.push_str(&format!(
                "• Blocked: {}{}\n",
                data.bugs_blocked.len(),
                prio_summary(&data.bugs_blocked)
            ));
        }
    }

    // ── QA Results ──
    if data.tested_count > 0 {
        let pass_pct = data.passed_count as f64 * 100.0 / data.tested_count as f64;
        r.push_str(&format!("\n*QA Results* ({} tested)\n", data.tested_count));
        r.push_str(&format!(
            "• Passed: {} ({:.0}%)\n",
            data.passed_count, pass_pct
        ));
        if data.failed_count > 0 {
            let fail_pct = data.failed_count as f64 * 100.0 / data.tested_count as f64;
            r.push_str(&format!(
                "• Failed: {} ({:.0}%)\n",
                data.failed_count, fail_pct
            ));
        }
    }

    // Failed / Retested tickets (only currently-failed)
    let failed_retest: Vec<_> = data
        .retest_items
        .iter()
        // Latest verdict, not current column: a ticket parked in Backlog after
        // a QA fail belongs here, and one that passed on retest does not.
        .filter(|rt| rt.currently_failed)
        .collect();
    if !failed_retest.is_empty() {
        r.push_str("\n*Failed / Retested Tickets*\n");
        for rt in &failed_retest {
            let num = issue_link_slack(rt.item, &app.config.owner);
            let prio = rt.item.priority.as_deref().unwrap_or("--");
            let assignee_mentions: Vec<String> = rt
                .item
                .assignees
                .iter()
                .map(|a| crate::slack::notifier::resolve_mention(a, &app.config.slack_user_map))
                .collect();
            let assignee_str = if assignee_mentions.is_empty() {
                String::new()
            } else {
                format!(" → {}", assignee_mentions.join(" "))
            };
            r.push_str(&format!(
                "• {} {} ({}, {}, failed {}×){}\n",
                num,
                title_slack(rt.item),
                prio,
                rt.item.status.label(),
                rt.fail_count,
                assignee_str
            ));
        }
    }

    // Bugs waiting for dev
    if !data.bugs_for_dev.is_empty() {
        r.push_str("\n⏳ *Bugs Waiting for Dev:*\n");
        for bug in &data.bugs_for_dev {
            let num = issue_link_slack(bug, &app.config.owner);
            let prio = bug.priority.as_deref().unwrap_or("--");
            let assignee_mentions: Vec<String> = bug
                .assignees
                .iter()
                .map(|a| crate::slack::notifier::resolve_mention(a, &app.config.slack_user_map))
                .collect();
            let assignee_str = if assignee_mentions.is_empty() {
                String::new()
            } else {
                assignee_mentions.join(" ")
            };
            r.push_str(&format!(
                "• {} {} | {} | {} | {}\n",
                num,
                title_slack(bug),
                prio,
                bug.status.label(),
                assignee_str
            ));
        }
    }

    r
}

// ─── Item to Markdown ───────────────────────────────────────────────────────

/// Format a single item as a full Markdown document for clipboard copy.
pub fn item_to_markdown(item: &Item, owner: &str) -> String {
    let mut r = String::new();

    // Title
    let num = item.number.map(|n| format!("#{}", n)).unwrap_or_default();
    r.push_str(&format!("## {} - {}\n\n", num, item.title));

    // Fields table
    r.push_str("| Field | Value |\n|-------|-------|\n");
    r.push_str(&format!("| Status | {} |\n", item.status.label()));
    r.push_str(&format!(
        "| Priority | {} |\n",
        item.priority.as_deref().unwrap_or("--")
    ));
    r.push_str(&format!(
        "| Stack | {} |\n",
        item.stack.as_deref().unwrap_or("--")
    ));
    r.push_str(&format!(
        "| Size | {} |\n",
        item.size.as_deref().unwrap_or("--")
    ));
    r.push_str(&format!(
        "| Sprint | {} |\n",
        item.sprint.as_deref().unwrap_or("--")
    ));
    let assignees = if item.assignees.is_empty() {
        "--".to_string()
    } else {
        item.assignees
            .iter()
            .map(|a| format!("@{}", a))
            .collect::<Vec<_>>()
            .join(", ")
    };
    r.push_str(&format!("| Assignees | {} |\n", assignees));
    let labels = if item.labels.is_empty() {
        "--".to_string()
    } else {
        item.labels.join(", ")
    };
    r.push_str(&format!("| Labels | {} |\n", labels));
    r.push_str(&format!(
        "| Repo | {} |\n",
        item.repository.as_deref().unwrap_or("--")
    ));
    if let Some(dt) = item.created_at {
        r.push_str(&format!("| Created | {} |\n", dt.format("%b %d, %Y")));
    }

    // GitHub link
    if let (Some(num_val), Some(repo)) = (item.number, item.repository.as_deref()) {
        let repo_path = if repo.contains('/') {
            repo.to_string()
        } else {
            format!("{}/{}", owner, repo)
        };
        r.push_str(&format!(
            "\n**Link:** https://github.com/{}/issues/{}\n",
            repo_path, num_val
        ));
    }

    // Linked PRs
    if !item.linked_prs.is_empty() {
        r.push_str(&format!("\n### Linked PRs ({})\n", item.linked_prs.len()));
        for pr_url in &item.linked_prs {
            r.push_str(&format!("- {}\n", pr_url));
        }
    }

    // Body
    if let Some(ref body) = item.body {
        if !body.trim().is_empty() {
            r.push_str("\n### Description\n\n");
            r.push_str(body.trim());
            r.push('\n');
        }
    }

    // Comments
    if !item.comments.is_empty() {
        r.push_str(&format!("\n### Comments ({})\n", item.comments.len()));
        for comment in &item.comments {
            r.push_str(&format!(
                "\n**@{}** · {}\n",
                comment.author,
                comment.created_at.format("%b %d, %H:%M")
            ));
            for line in comment.body.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    r.push_str(&format!("> {}\n", trimmed));
                }
            }
        }
    }

    r
}

// ─── Clipboard ──────────────────────────────────────────────────────────────

/// Copy text to system clipboard using arboard.
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Failed to copy to clipboard: {}", e))
}

/// Read text from system clipboard using arboard.
pub fn read_from_clipboard() -> Result<String, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
    clipboard
        .get_text()
        .map_err(|e| format!("Failed to read from clipboard: {}", e))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    // Helper to easily create Utc date times
    fn make_utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        let naive = NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap();
        Utc.from_utc_datetime(&naive)
    }

    #[test]
    fn test_is_ot_vn_weekend() {
        // 2026-02-21 is a Saturday
        let dt = make_utc(2026, 2, 21, 12, 0);
        assert!(is_ot(dt, 7));
    }

    #[test]
    fn test_lunch_hour_is_ot() {
        // 12:00 VN on a Monday: the lunch break is unpaid time, so working
        // through it counts as OT.
        let dt = make_utc(2026, 2, 23, 5, 0);
        assert!(is_ot(dt, 7));
        // Shift boundaries themselves are paid.
        assert!(!is_ot(make_utc(2026, 2, 23, 4, 30), 7)); // 11:30
        assert!(!is_ot(make_utc(2026, 2, 23, 6, 0), 7)); // 13:00
    }

    #[test]
    fn test_is_ot_vn_weekday_in_hours() {
        // 2026-02-23 is Monday.
        // VN time: 10:00 AM. UTC time: 03:00 AM
        let dt = make_utc(2026, 2, 23, 3, 0);
        assert!(!is_ot(dt, 7));

        // VN Time: 15:00 PM. UTC time: 08:00 AM
        let dt = make_utc(2026, 2, 23, 8, 0);
        assert!(!is_ot(dt, 7));
    }

    #[test]
    fn test_is_ot_vn_weekday_out_hours() {
        // VN time: 06:00 AM (OT). UTC time: 23:00 PM previous day
        let dt = make_utc(2026, 2, 22, 23, 0);
        assert!(is_ot(dt, 7));

        // VN time: 18:00 PM (Evening OT). UTC time: 11:00 AM
        let dt = make_utc(2026, 2, 23, 11, 0);
        assert!(is_ot(dt, 7));
    }

    #[test]
    fn test_classify_status_change_failed_dev() {
        let kind = classify_status_change(Some("In QA - Dev"), Some("In Progress"));
        assert_eq!(kind, Some(ActionKind::FailedOnDev));
    }

    #[test]
    fn test_classify_status_change_failed_stg() {
        let kind = classify_status_change(Some("In QA"), Some("In Progress"));
        assert_eq!(kind, Some(ActionKind::FailedOnStg));
    }

    #[test]
    fn test_classify_status_change_failed_uat() {
        let kind = classify_status_change(Some("In UAT"), Some("In Progress"));
        assert_eq!(kind, Some(ActionKind::FailedOnUat));
    }

    #[test]
    fn test_classify_move_into_uat_is_not_qa_action() {
        // Devs deploy to UAT and move the ticket there - not a QA action
        assert_eq!(classify_status_change(Some("In QA"), Some("In UAT")), None);
    }

    #[test]
    fn test_classify_status_change_verified() {
        let kind = classify_status_change(Some("In QA"), Some("Tech Complete"));
        assert_eq!(kind, Some(ActionKind::VerifiedForRelease));

        // Pass UAT → Tech Complete also counts as verified
        let kind = classify_status_change(Some("In UAT"), Some("Tech Complete"));
        assert_eq!(kind, Some(ActionKind::VerifiedForRelease));
    }

    #[test]
    fn test_classify_status_change_done() {
        let kind = classify_status_change(Some("In QA"), Some("Done"));
        assert_eq!(kind, Some(ActionKind::TestComplete));
    }

    #[test]
    fn test_classify_status_change_blocked() {
        let kind = classify_status_change(Some("In QA"), Some("Blocked"));
        assert_eq!(kind, Some(ActionKind::Blocked));
    }

    #[test]
    fn test_classify_status_change_backlog() {
        let kind = classify_status_change(Some("In QA"), Some("Backlog"));
        assert_eq!(kind, Some(ActionKind::MovedToBacklog));
    }

    #[test]
    fn test_classify_non_qa_actions_return_none() {
        // Dev moving to In QA = not a QA action
        assert_eq!(
            classify_status_change(Some("In Progress"), Some("In QA - Dev")),
            None
        );
        assert_eq!(
            classify_status_change(Some("In Review"), Some("In QA")),
            None
        );
        // Dev submitting PR
        assert_eq!(
            classify_status_change(Some("In Progress"), Some("In Review")),
            None
        );
    }

    #[test]
    fn test_classify_generic_actions_return_worked_on() {
        // In Progress from non-QA columns = generic WorkedOn
        let kind = classify_status_change(Some("Backlog"), Some("In Progress"));
        assert!(matches!(kind, Some(ActionKind::WorkedOn(_))));

        // Ready for Dev = generic WorkedOn
        let kind = classify_status_change(Some("Backlog"), Some("Ready for Dev"));
        assert!(matches!(kind, Some(ActionKind::WorkedOn(_))));
    }

    #[test]
    fn test_action_priority_order() {
        assert!(ActionKind::FailedOnDev.priority() < ActionKind::Commented.priority());
        assert!(ActionKind::VerifiedForRelease.priority() < ActionKind::Commented.priority());
        assert!(ActionKind::Blocked.priority() < ActionKind::Commented.priority());
    }

    #[test]
    fn test_classify_ready_for_release() {
        // QA moves ticket to Ready for Release = VerifiedForRelease
        let kind = classify_status_change(Some("Tech Complete"), Some("Ready for Release"));
        assert_eq!(kind, Some(ActionKind::VerifiedForRelease));

        let kind = classify_status_change(Some("In QA"), Some("Ready for Release"));
        assert_eq!(kind, Some(ActionKind::VerifiedForRelease));
    }

    #[test]
    fn test_is_previous_workday_monday() {
        // 2026-02-23 is Monday. "Previous workday" should include Fri/Sat/Sun.
        // We can't test is_previous_workday directly because it uses Utc::now(),
        // but we can test classify_status_change still works for Ready for Release.
        // This test documents the intended behavior.

        // Friday 2026-02-20 in VN timezone (UTC: 02-19 17:00 = VN 02-20 00:00)
        let friday_vn = make_utc(2026, 2, 19, 17, 0);
        // Saturday
        let saturday_vn = make_utc(2026, 2, 20, 17, 0);
        // Sunday
        let sunday_vn = make_utc(2026, 2, 22, 5, 0);

        // All three should be recognized as OT (weekend or outside work hours)
        assert!(is_ot(friday_vn, 7)); // Fri midnight = OT (outside work hours)
        assert!(is_ot(saturday_vn, 7)); // Saturday = OT
        assert!(is_ot(sunday_vn, 7)); // Sunday = OT
    }

    fn tl(
        status: Vec<crate::gh::timeline::StatusChangedEvent>,
        labels: Vec<crate::gh::timeline::LabelEvent>,
    ) -> ItemTimeline {
        ItemTimeline {
            status_events: status,
            comment_events: Vec::new(),
            label_events: labels,
            missing: false,
        }
    }

    fn status_evt(
        prev: &str,
        curr: &str,
        at: DateTime<Utc>,
    ) -> crate::gh::timeline::StatusChangedEvent {
        crate::gh::timeline::StatusChangedEvent {
            created_at: at,
            previous_status: Some(prev.to_string()),
            status: Some(curr.to_string()),
            project: None,
            actor: Some("qa".to_string()),
        }
    }

    fn label_evt(label: &str, at: DateTime<Utc>) -> crate::gh::timeline::LabelEvent {
        crate::gh::timeline::LabelEvent {
            created_at: at,
            label: label.to_string(),
            actor: Some("qa".to_string()),
        }
    }

    #[test]
    fn test_in_qa_queue_splits_waiting_for_deploy() {
        use crate::app::App;
        let mut app = App::new();
        app.items = vec![
            Item {
                id: "a".into(),
                title: "still to test".into(),
                status: Status::InQA,
                ..Default::default()
            },
            Item {
                id: "b".into(),
                title: "passed STG, waiting for deploy".into(),
                status: Status::InQA,
                labels: vec!["Ready-for-UAT".into()],
                ..Default::default()
            },
            Item {
                id: "c".into(),
                title: "bug to verify".into(),
                status: Status::InQADev,
                labels: vec!["Bug".into()],
                ..Default::default()
            },
        ];

        let data = collect_sprint_report(&app);
        assert_eq!(data.in_qa_pipeline, 3);
        assert_eq!(
            data.tickets_to_test, 1,
            "waiting-for-deploy is not work to do"
        );
        assert_eq!(data.bugs_to_verify.len(), 1);
        assert_eq!(data.waiting_deploy_count, 1);
        // The parts must still add up to the column count.
        assert_eq!(
            data.tickets_to_test + data.bugs_to_verify.len() + data.waiting_deploy_count,
            data.in_qa_pipeline
        );
        assert!(in_qa_summary(&data).contains("1 waiting for deploy"));
    }

    #[test]
    fn test_verdict_label_pass_counts_as_tested() {
        // Pass STG only adds a label. Under the old status-only reading this
        // ticket counted as "not tested yet" while it sat In QA.
        let t = tl(
            vec![],
            vec![label_evt("Ready-for-UAT", make_utc(2026, 3, 2, 3, 0))],
        );
        assert_eq!(
            latest_qa_verdict(&t).map(|(v, _)| v),
            Some(QaVerdict::Passed)
        );
    }

    #[test]
    fn test_verdict_takes_the_latest_signal() {
        // Failed on Dev, fixed, then passed on Dev: the pass is the verdict.
        let t = tl(
            vec![status_evt(
                "In QA - Dev",
                "In Progress",
                make_utc(2026, 3, 2, 3, 0),
            )],
            vec![label_evt("Ready-for-Staging", make_utc(2026, 3, 4, 3, 0))],
        );
        assert_eq!(
            latest_qa_verdict(&t).map(|(v, _)| v),
            Some(QaVerdict::Passed)
        );

        // And the other way round: passed on Dev, then failed on STG.
        let t = tl(
            vec![status_evt(
                "In QA",
                "In Progress",
                make_utc(2026, 3, 5, 3, 0),
            )],
            vec![label_evt("Ready-for-Staging", make_utc(2026, 3, 4, 3, 0))],
        );
        assert_eq!(
            latest_qa_verdict(&t).map(|(v, _)| v),
            Some(QaVerdict::Failed)
        );
    }

    #[test]
    fn test_verdict_ignores_non_qa_transitions() {
        // Parked in Backlog by the PO, and a dev putting it into QA: neither is
        // a QA verdict. "failed = tested - passed" used to count the first one.
        let t = tl(
            vec![
                status_evt("In QA", "Backlog", make_utc(2026, 3, 2, 3, 0)),
                status_evt("In Progress", "In QA", make_utc(2026, 3, 3, 3, 0)),
            ],
            vec![],
        );
        assert_eq!(latest_qa_verdict(&t), None);
    }

    #[test]
    fn test_verdict_pass_on_move_forward() {
        // Pass UAT (and the pre-v3.2.0 flow) moves the ticket forward.
        let t = tl(
            vec![status_evt(
                "In UAT",
                "Tech Complete",
                make_utc(2026, 3, 2, 3, 0),
            )],
            vec![],
        );
        assert_eq!(
            latest_qa_verdict(&t).map(|(v, _)| v),
            Some(QaVerdict::Passed)
        );
    }

    #[test]
    fn test_doing_today_uses_my_tasks_excluded_labels() {
        // Daily report "Doing today" and My Tasks share App::item_excluded_from_my_tasks.
        // Verify the label/status pairs behave as documented.
        use crate::app::App;
        assert!(App::label_excludes_in_status(
            "Ready-for-Staging",
            &Status::InQADev
        ));
        assert!(!App::label_excludes_in_status(
            "Ready-for-Staging",
            &Status::InQA
        ));
        assert!(App::label_excludes_in_status(
            "Ready-for-UAT",
            &Status::InQA
        ));
        assert!(!App::label_excludes_in_status(
            "Ready-for-UAT",
            &Status::InUAT
        ));
        // Ready-for-Release excludes in any status (space spelling too)
        assert!(App::label_excludes_in_status(
            "Ready for Release",
            &Status::Blocked
        ));
    }
}
