use crate::app::App;
use crate::cache::Cache;
use crate::models::root::{clean_title, resolve_root, Root};
use crate::models::status::Status;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};

// ─── Time helpers ───────────────────────────────────────────────────────────

/// Paid working hours in the configured timezone, as two shifts.
///
/// The 11:30-13:00 lunch break is deliberately NOT paid time: work logged in
/// that window counts as OT, which is the team's policy.
const SHIFTS: [((u32, u32), (u32, u32)); 2] = [((7, 30), (11, 30)), ((13, 0), (17, 0))];

/// The configured timezone. A bad offset falls back to UTC instead of
/// panicking in the middle of a report.
fn fixed_offset(offset_hours: i32) -> FixedOffset {
    FixedOffset::east_opt(offset_hours * 3600).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap())
}

/// Check if a timestamp is Overtime (OT) in the configured timezone.
/// OT = weekends, or weekdays outside the paid shifts (lunch break included).
fn is_ot(dt_utc: DateTime<Utc>, offset_hours: i32) -> bool {
    let tz_offset = fixed_offset(offset_hours);
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
    let tz_offset = fixed_offset(offset_hours);
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
    let tz_offset = fixed_offset(offset_hours);
    Utc::now()
        .with_timezone(&tz_offset)
        .format("%b %d, %Y")
        .to_string()
}

/// Check if a timestamp is Today in the configured timezone.
fn is_today(dt_utc: DateTime<Utc>, offset_hours: i32) -> bool {
    let tz_offset = fixed_offset(offset_hours);
    let now_local = Utc::now().with_timezone(&tz_offset);
    let dt_local = dt_utc.with_timezone(&tz_offset);

    dt_local.date_naive() == now_local.date_naive()
}

// ─── Shared data structures ─────────────────────────────────────────────────

use crate::models::item::{Item, ItemKind};

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
    /// Any other status change by the user, reported as the move it was.
    /// `from` is empty when the ticket had no status before (just created).
    WorkedOn { from: String, to: String },
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
            Self::Commented => "Reviewed & commented",
            Self::WorkedOn { .. } => "Moved",
        }
    }

    /// Full description.
    fn full_description(&self) -> String {
        match self {
            Self::WorkedOn { from, to } if from.is_empty() => format!("Moved to {}", to),
            Self::WorkedOn { from, to } => format!("Moved {} → {}", from, to),
            other => other.description().to_string(),
        }
    }

    /// Whether the verdict sent the ticket back to dev.
    fn is_fail(&self) -> bool {
        matches!(
            self,
            Self::FailedOnDev | Self::FailedOnStg | Self::FailedOnUat
        )
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
            Self::WorkedOn { .. } => 2,
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
            Self::WorkedOn { .. } => 9,
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
    /// QA work done on the previous workday that did NOT fail.
    tested: Vec<YesterdayAction<'a>>,
    /// Tickets QA rejected on the previous workday. Reported on their own:
    /// sorted in among the passes they read as just more work done, while
    /// they are the part of the standup the team has to act on.
    failed: Vec<YesterdayAction<'a>>,
    /// Bugs and QA-raised enhancements the user filed. Kept apart from
    /// `tested` because filing an issue is not a test result, and it used to
    /// appear twice: once as a generic "Worked on ticket (Ready for Dev)"
    /// line and again in its own section.
    issues_found: Vec<TaggedItem<'a>>,
    in_qa: Vec<TaggedItem<'a>>,
    blocked: Vec<TaggedItem<'a>>,
    /// Sprint items with NO timeline data available - the Yesterday section
    /// may be undercounted while these are still being fetched.
    missing_history: usize,
}

/// Per-EPIC rollup for a sprint: what the sprint's tickets sit under, and
/// how QA is doing inside each group.
struct EpicGroup<'a> {
    /// The top of the tickets' sub-issue chain.
    root: Root<'a>,
    tickets: usize,
    bugs: usize,
    in_qa: usize,
    in_uat: usize,
    tested: usize,
    passed: usize,
    failed: usize,
}

/// Collected data for a Sprint Report.
struct SprintReportData<'a> {
    sprint: String,
    user: String,
    date: String,
    /// Sprint items with NO timeline data available - QA metrics may be
    /// undercounted while these are still being fetched.
    missing_history: usize,
    /// First day of the sprint when the QA metrics are scoped to it.
    metrics_since: Option<NaiveDate>,
    total: usize,
    /// Completed tickets: Tech Complete, Ready for Release and Done.
    done_count: usize,
    done_pct: f64,
    /// The Done column alone, for the split in the Done line.
    done_column_count: usize,
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
    // ─── Per-EPIC rollup ───
    epics: Vec<EpicGroup<'a>>,
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
/// `since` narrows the verdicts to a window - the sprint report passes the
/// sprint's first day, so a carry-over passed on Dev weeks earlier and never
/// deployed is not counted as this sprint's testing.
///
/// Returns `None` when the ticket has no QA verdict (in the window): never
/// tested, or sitting in a QA column awaiting a first verdict.
fn latest_qa_verdict(
    tl: &ItemTimeline,
    since: Option<DateTime<Utc>>,
) -> Option<(QaVerdict, DateTime<Utc>)> {
    let in_window = |at: DateTime<Utc>| since.is_none_or(|s| at >= s);
    let mut best: Option<(QaVerdict, DateTime<Utc>)> = None;

    for e in &tl.status_events {
        if !in_window(e.created_at) {
            continue;
        }
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
        if !in_window(l.created_at) {
            continue;
        }
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
    // The QA's own Move to In UAT (`I`): code reached UAT, a handoff the QA
    // made, reported as the move it was. Only the user's own events get here.
    if curr_lower == "in uat" && prev_lower == "in qa" {
        return Some(moved(prev, curr));
    }
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
        return Some(moved(prev, curr));
    }

    // A verdict is a move OUT of a QA column. The QA also moves tickets Ready
    // for Dev -> Tech Complete (delivered under another ticket, say) and Tech
    // Complete -> Ready for Release (the release step); neither is "verified",
    // and the sprint report already refuses to count them as tested.
    let from_qa = prev_lower.contains("in qa") || prev_lower.contains("in uat");
    if from_qa && (curr_lower.contains("tech complete") || curr_lower.contains("ready for release"))
    {
        return Some(ActionKind::VerifiedForRelease);
    }
    if from_qa && curr_lower == "done" {
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

    // Everything else done by user - show as the move it was
    Some(moved(prev, curr))
}

fn moved(prev: &str, curr: &str) -> ActionKind {
    ActionKind::WorkedOn {
        from: prev.to_string(),
        to: curr.to_string(),
    }
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

    let sprint_filter_matches = |i: &Item| {
        app.board_sprint_filter.is_none()
            || i.sprint.as_deref() == app.board_sprint_filter.as_deref()
    };

    let mut missing_history = 0usize;

    // Every ticket the user touched counts, not only the sprint's: on day one
    // of a sprint, Friday's Tech Complete tickets still sit in the previous
    // sprint, and a sprint-scoped scan dropped them from Monday's standup.
    // Off-sprint tickets are read from memory only (the sync fetches a
    // timeline for every active ticket); the disk cache is for the sprint's
    // own, and only those count as missing.
    for item in &app.items {
        let in_sprint = sprint_filter_matches(item);
        if !in_sprint && item.status_history.is_none() {
            continue;
        }
        // Load timeline data: prefer in-memory, fallback to cache
        let tl = load_timeline_for_item(item, &timeline_cache);
        if tl.missing {
            if in_sprint {
                missing_history += 1;
            }
            continue;
        }
        let status_events = &tl.status_events;
        let comment_events = &tl.comment_events;

        // 1. Status changes and label passes made BY the current user
        // yesterday, judged together. Pass Dev / Pass STG are LABEL-ONLY
        // actions (LabeledEvent), and consulting them only when no status
        // event matched reported a ticket failed at 17:12 and passed at 17:16
        // as "Failed on STG". The most significant action wins; among equals,
        // the latest. OT sticks: any OT event on the ticket keeps the flag.
        let mut candidates: Vec<(DateTime<Utc>, ActionKind)> = Vec::new();
        let mut any_ot = false;
        for evt in status_events {
            // Only events explicitly by the current user. (An actor-less
            // fallback used to attribute a co-assigned DEV's move to the QA.)
            let actor_match = evt.actor.as_deref().map(|a| a == user).unwrap_or(false);
            if !actor_match || !is_previous_workday(evt.created_at, tz) {
                continue;
            }
            // None = definitively not a QA action (a move INTO a QA column).
            let Some(kind) =
                classify_status_change(evt.previous_status.as_deref(), evt.status.as_deref())
            else {
                continue;
            };
            any_ot |= is_ot(evt.created_at, tz);
            candidates.push((evt.created_at, kind));
        }
        for lev in &tl.label_events {
            let actor_match = lev.actor.as_deref().map(|a| a == user).unwrap_or(false);
            if !actor_match || !is_previous_workday(lev.created_at, tz) {
                continue;
            }
            let kind = if App::label_eq(&lev.label, "Ready-for-Staging") {
                ActionKind::PassedDev
            } else if App::label_eq(&lev.label, "Ready-for-UAT") {
                ActionKind::PassedStg
            } else {
                continue;
            };
            any_ot |= is_ot(lev.created_at, tz);
            candidates.push((lev.created_at, kind));
        }
        let mut best_action: Option<(ActionKind, bool)> = candidates
            .into_iter()
            .max_by_key(|(at, kind)| (kind.significance(), *at))
            .map(|(_, kind)| (kind, any_ot));

        // 2. If no status change by user, check for comments by user yesterday
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

    // 4. Issues found: bugs AND QA-raised enhancements created yesterday or
    // today by the user. Only enhancements carrying the report form's
    // `[ENHANCEMENT]` prefix count - the plain Enhancement label is also on
    // dev feature work, which QA did not raise. From any sprint, like the
    // actions above: a bug filed on a sprint's last day sits in that sprint
    // on the next one's first morning.
    let mut issues_found: Vec<TaggedItem<'_>> = Vec::new();
    for item in &app.items {
        let is_bug = item.labels.iter().any(|l| l.to_lowercase().contains("bug"));
        if !is_bug && !is_qa_enhancement_report(item) {
            continue;
        }
        let created_recently = item
            .created_at
            .map(|dt| is_previous_workday(dt, tz) || is_today(dt, tz))
            .unwrap_or(false);
        if !created_recently {
            continue;
        }
        let tl = load_timeline_for_item(item, &timeline_cache);
        if filed_by_user(item, &tl, &user) {
            let ot_flag = item.created_at.map(|dt| is_ot(dt, tz)).unwrap_or(false);
            issues_found.push(TaggedItem {
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

    // Filing an issue also lands in `yesterday` as a generic "Worked on
    // ticket" status change, so the same ticket was reported twice under two
    // different headings. The issue section wins: it says what actually
    // happened.
    let reported_ids: std::collections::HashSet<&str> =
        issues_found.iter().map(|t| t.item.id.as_str()).collect();
    let (failed, tested): (Vec<YesterdayAction<'_>>, Vec<YesterdayAction<'_>>) = yesterday
        .into_iter()
        .filter(|a| !reported_ids.contains(a.item.id.as_str()))
        .partition(|a| a.kind.is_fail());

    DailyStandupData {
        sprint,
        user,
        date,
        tested,
        failed,
        issues_found,
        in_qa,
        blocked,
        missing_history,
    }
}

/// Whether the current user filed this issue.
///
/// The board carries no author, so it is read from the timeline: the report
/// form (and a GitHub issue template) creates the issue, labels it and gives
/// it its first status as the author, within moments of creation. Being an
/// assignee is not filing - the QA assigns a bug to the developers, and a
/// developer may assign one to the QA - so that heuristic only remains for
/// tickets whose timeline has not been fetched yet.
fn filed_by_user(item: &Item, tl: &ItemTimeline, user: &str) -> bool {
    if tl.missing {
        return item.assignees.iter().any(|a| a == user);
    }
    let by_user = |actor: Option<&str>| actor == Some(user);
    let at_creation = |at: DateTime<Utc>| {
        item.created_at
            .map(|c| (at - c).num_minutes().abs() <= 5)
            .unwrap_or(true)
    };
    // The first status the ticket ever got (nothing before it).
    let first_status = tl.status_events.iter().find(|e| {
        e.previous_status
            .as_deref()
            .is_none_or(|p| p.trim().is_empty())
    });
    if first_status
        .map(|e| by_user(e.actor.as_deref()) && at_creation(e.created_at))
        .unwrap_or(false)
    {
        return true;
    }
    // Or the type label, added at creation.
    tl.label_events.iter().any(|l| {
        (App::label_eq(&l.label, "Bug") || App::label_eq(&l.label, "Enhancement"))
            && by_user(l.actor.as_deref())
            && at_creation(l.created_at)
    })
}

/// A move that the same person reverses within this many minutes is an undo,
/// not two actions.
///
/// #3844, Sprint 29: the QA moved it In QA - Dev -> In Progress at 11:11 and
/// straight back at 11:17 - a misclick - and the standup reported a failure
/// while the sprint report counted "failed 2×". A reversal by someone else
/// (a developer returning a one-line fix) is two real actions and stays.
const UNDO_WINDOW_MINUTES: i64 = 30;

/// Drop every move and the same actor's prompt reversal of it.
fn without_undone_moves(
    mut events: Vec<crate::gh::timeline::StatusChangedEvent>,
) -> Vec<crate::gh::timeline::StatusChangedEvent> {
    events.sort_by_key(|e| e.created_at);
    let mut undone = vec![false; events.len()];
    let mut i = 0;
    while i + 1 < events.len() {
        let (a, b) = (&events[i], &events[i + 1]);
        let same_actor = a.actor.is_some() && a.actor == b.actor;
        let reverses =
            b.status.is_some() && b.status == a.previous_status && b.previous_status == a.status;
        let quick = b.created_at - a.created_at <= Duration::minutes(UNDO_WINDOW_MINUTES);
        if same_actor && reverses && quick {
            undone[i] = true;
            undone[i + 1] = true;
            i += 2;
        } else {
            i += 1;
        }
    }
    events
        .into_iter()
        .zip(undone)
        .filter(|(_, u)| !u)
        .map(|(e, _)| e)
        .collect()
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
            status_events: without_undone_moves(history.clone()),
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
                status_events: without_undone_moves(td.status_changes),
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

    // QA metrics are THIS sprint's work. 12 of Sprint 29's 39 "tested" were
    // verdicts from before the sprint began - carry-overs passed on Dev in
    // August and never deployed. Without iteration dates (no sprint filter,
    // or a board that carries none) every verdict counts, as before.
    let metrics_since: Option<NaiveDate> = app
        .board_sprint_filter
        .as_deref()
        .and_then(|s| app.sprint_start_date(s));
    let since_utc: Option<DateTime<Utc>> = metrics_since.and_then(|d| {
        fixed_offset(tz)
            .from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
            .single()
            .map(|dt| dt.with_timezone(&Utc))
    });

    let items: Vec<_> = app
        .items
        .iter()
        .filter(|i| {
            app.board_sprint_filter.is_none()
                || i.sprint.as_deref() == app.board_sprint_filter.as_deref()
        })
        .collect();

    let total = items.len();
    // Tech Complete is done as far as the sprint goes - the release note
    // ships it - so it counts, with Ready for Release, alongside Done.
    let done_count = items.iter().filter(|i| i.status.is_completed()).count();
    let done_column_count = items.iter().filter(|i| i.status == Status::Done).count();
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
    let mut epic_groups: std::collections::HashMap<String, EpicGroup> =
        std::collections::HashMap::new();

    for item in &items {
        let tl = load_timeline_for_item(item, &timeline_cache);
        if tl.missing {
            missing_history += 1;
        }
        let has_history = !(tl.status_events.is_empty() && tl.label_events.is_empty());
        // The sprint's verdict drives the QA metrics; the all-time one says
        // where the ticket stands today, for the retest list.
        let (verdict, verdict_all) = if has_history {
            (
                latest_qa_verdict(&tl, since_utc),
                latest_qa_verdict(&tl, None),
            )
        } else {
            (None, None)
        };

        // ─── Per-EPIC rollup ───
        //
        // Counted for EVERY sprint ticket, including those with no timeline
        // yet: a group's ticket count is a board fact, not a QA metric.
        let root = resolve_root(item, &app.items);
        let group = epic_groups.entry(root.key()).or_insert_with(|| EpicGroup {
            root,
            tickets: 0,
            bugs: 0,
            in_qa: 0,
            in_uat: 0,
            tested: 0,
            passed: 0,
            failed: 0,
        });
        group.tickets += 1;
        if item.labels.iter().any(|l| l.to_lowercase().contains("bug")) {
            group.bugs += 1;
        }
        // Same definition as the Sprint Overview's "In QA" line, so the epic
        // figures add up to it; In UAT is its own regression stage there too.
        if matches!(item.status, Status::InQA | Status::InQADev) {
            group.in_qa += 1;
        }
        if item.status == Status::InUAT {
            group.in_uat += 1;
        }
        match verdict {
            Some((QaVerdict::Passed, _)) => {
                group.tested += 1;
                group.passed += 1;
            }
            Some((QaVerdict::Failed, _)) => {
                group.tested += 1;
                group.failed += 1;
            }
            None => {}
        }

        if !has_history {
            continue;
        }

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
                currently_failed: matches!(verdict_all, Some((QaVerdict::Failed, _))),
            });
        }
    }

    // Sort retested items by fail count descending
    retest_items.sort_by_key(|a| std::cmp::Reverse(a.fail_count));

    // Epics first, then the other roots (a sprint bucket task, a story whose
    // epic is not on the board), biggest first; tickets with no parent at all
    // last - that bucket is nobody's.
    let mut epics: Vec<EpicGroup> = epic_groups.into_values().collect();
    epics.sort_by_key(|g| (g.root.rank(), std::cmp::Reverse(g.tickets)));

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
        metrics_since,
        total,
        done_count,
        done_pct,
        done_column_count,
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
        epics,
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
    crate::slack::webhook::escape_mrkdwn(&clean_title(&item.title))
}

/// The EPIC an item belongs to, if the chain above it reaches one.
#[cfg(test)]
fn resolve_epic<'a>(item: &Item, items: &'a [Item]) -> Option<&'a Item> {
    match resolve_root(item, items) {
        Root::Epic(epic) => Some(epic),
        _ => None,
    }
}

/// Whether an enhancement was raised BY QA.
///
/// The tool's own form writes the `[ENHANCEMENT]` title prefix
/// (`ReportKind::title_prefix`), so that upper-case spelling marks a QA
/// finding. The match is case-sensitive on purpose: the board also carries
/// dev tickets titled `[Enhancement][BE] Refactor ...`, which are feature
/// work to test, not a QA finding to verify.
fn is_qa_enhancement_report(item: &Item) -> bool {
    clean_title(&item.title).starts_with(crate::app::ReportKind::Enhancement.title_prefix())
}

/// What QA does to a ticket.
///
/// A bug fix and a QA-raised enhancement come back from dev to be verified; a
/// task - and an enhancement that is really feature work carrying the
/// Enhancement label - gets tested. Saying "testing" for all of them read as
/// if every ticket were a fresh feature pass.
fn qa_verb(item: &Item) -> &'static str {
    if is_qa_enhancement_report(item) {
        return "Verifying";
    }
    match item.kind() {
        ItemKind::Bug => "Verifying",
        ItemKind::Enhancement | ItemKind::Task => "Testing",
    }
}

/// Root prefix for a ticket line: `[Dave project] ` for an epic, `[Issues
/// sprint 29] ` for a plain root task, `[no parent] ` when there is no chain.
fn epic_tag(item: &Item, items: &[Item]) -> String {
    match resolve_root(item, items).short_name() {
        Some(name) => format!("[{}] ", name),
        None => "[no parent] ".to_string(),
    }
}

/// [`epic_tag`] for a webhook message: root names come from GitHub titles
/// like ticket titles do, so `&`, `<` and `>` are escaped the same way.
fn epic_tag_slack(item: &Item, items: &[Item]) -> String {
    crate::slack::webhook::escape_mrkdwn(&epic_tag(item, items))
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

    // Yesterday - what was tested and passed
    r.push_str("### Yesterday - Tested & Verified\n");
    if data.tested.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for a in &data.tested {
            r.push_str(&format!(
                "- {}{} {} ({}, {}) → {}{}\n",
                epic_tag(a.item, &app.items),
                issue_link_md(a.item),
                clean_title(&a.item.title),
                a.item.status.label(),
                a.item.priority.as_deref().unwrap_or("--"),
                a.kind.full_description(),
                if a.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Yesterday - what was sent back to dev
    r.push_str("\n### Yesterday - Failed\n");
    if data.failed.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for f in &data.failed {
            r.push_str(&format!(
                "- {}{} {} ({}, {}) → {}{}\n",
                epic_tag(f.item, &app.items),
                issue_link_md(f.item),
                clean_title(&f.item.title),
                f.item.status.label(),
                f.item.priority.as_deref().unwrap_or("--"),
                f.kind.full_description(),
                if f.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Yesterday - what was filed
    r.push_str("\n### Issues found - yesterday and today\n");
    if data.issues_found.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for t in &data.issues_found {
            r.push_str(&format!(
                "- {}{} {} ({}, {}){}\n",
                epic_tag(t.item, &app.items),
                issue_link_md(t.item),
                clean_title(&t.item.title),
                t.item.status.label(),
                t.item.priority.as_deref().unwrap_or("--"),
                if t.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Doing today
    r.push_str("\n### Doing today\n");
    if data.in_qa.is_empty() {
        r.push_str("- (none)\n");
    } else {
        for t in &data.in_qa {
            r.push_str(&format!(
                "- {} {}{} {} ({}, {})\n",
                qa_verb(t.item),
                epic_tag(t.item, &app.items),
                issue_link_md(t.item),
                clean_title(&t.item.title),
                t.item.status.label(),
                t.item.priority.as_deref().unwrap_or("--"),
            ));
        }
    }

    // Blockers
    if !data.blocked.is_empty() {
        r.push_str("\n### Blockers\n");
        for t in &data.blocked {
            r.push_str(&format!(
                "- {}{} {} (Blocked){}\n",
                epic_tag(t.item, &app.items),
                issue_link_md(t.item),
                clean_title(&t.item.title),
                if t.is_ot { " (OT)" } else { "" },
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

    // Yesterday - what was tested and passed
    r.push_str("Yesterday - Tested & Verified:\n");
    if data.tested.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for a in &data.tested {
            r.push_str(&format!(
                "• {}{} {} ({}, {}) → {}{}\n",
                epic_tag(a.item, &app.items),
                issue_num_text(a.item),
                clean_title(&a.item.title),
                a.item.status.label(),
                a.item.priority.as_deref().unwrap_or("--"),
                a.kind.full_description(),
                if a.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Yesterday - what was sent back to dev
    r.push_str("\nYesterday - Failed:\n");
    if data.failed.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for f in &data.failed {
            r.push_str(&format!(
                "• {}{} {} ({}, {}) → {}{}\n",
                epic_tag(f.item, &app.items),
                issue_num_text(f.item),
                clean_title(&f.item.title),
                f.item.status.label(),
                f.item.priority.as_deref().unwrap_or("--"),
                f.kind.full_description(),
                if f.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Yesterday - what was filed
    r.push_str("\nIssues found - yesterday and today:\n");
    if data.issues_found.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.issues_found {
            r.push_str(&format!(
                "• {}{} {} ({}, {}){}\n",
                epic_tag(t.item, &app.items),
                issue_num_text(t.item),
                clean_title(&t.item.title),
                t.item.status.label(),
                t.item.priority.as_deref().unwrap_or("--"),
                if t.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Doing today
    r.push_str("\nDoing today:\n");
    if data.in_qa.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.in_qa {
            r.push_str(&format!(
                "• {} {}{} {} ({}, {})\n",
                qa_verb(t.item),
                epic_tag(t.item, &app.items),
                issue_num_text(t.item),
                clean_title(&t.item.title),
                t.item.status.label(),
                t.item.priority.as_deref().unwrap_or("--"),
            ));
        }
    }

    // Blockers
    if !data.blocked.is_empty() {
        r.push_str("\nBlockers:\n");
        for t in &data.blocked {
            r.push_str(&format!(
                "• {}{} {} (Blocked){}\n",
                epic_tag(t.item, &app.items),
                issue_num_text(t.item),
                clean_title(&t.item.title),
                if t.is_ot { " (OT)" } else { "" },
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

    // Yesterday - what was tested and passed
    r.push_str("*Yesterday - Tested & Verified*\n");
    if data.tested.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for a in &data.tested {
            r.push_str(&format!(
                "• {}{} {} ({}, {}) → {}{}\n",
                epic_tag_slack(a.item, &app.items),
                issue_link_slack(a.item, &app.config.owner),
                title_slack(a.item),
                a.item.status.label(),
                a.item.priority.as_deref().unwrap_or("--"),
                a.kind.full_description(),
                if a.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Yesterday - what was sent back to dev
    r.push_str("\n*Yesterday - Failed*\n");
    if data.failed.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for f in &data.failed {
            r.push_str(&format!(
                "• {}{} {} ({}, {}) → {}{}\n",
                epic_tag_slack(f.item, &app.items),
                issue_link_slack(f.item, &app.config.owner),
                title_slack(f.item),
                f.item.status.label(),
                f.item.priority.as_deref().unwrap_or("--"),
                f.kind.full_description(),
                if f.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Yesterday - what was filed
    r.push_str("\n*Issues found - yesterday and today*\n");
    if data.issues_found.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.issues_found {
            r.push_str(&format!(
                "• {}{} {} ({}, {}){}\n",
                epic_tag_slack(t.item, &app.items),
                issue_link_slack(t.item, &app.config.owner),
                title_slack(t.item),
                t.item.status.label(),
                t.item.priority.as_deref().unwrap_or("--"),
                if t.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    // Doing today
    r.push_str("\n*Doing today*\n");
    if data.in_qa.is_empty() {
        r.push_str("• (None)\n");
    } else {
        for t in &data.in_qa {
            r.push_str(&format!(
                "• {} {}{} {} ({}, {})\n",
                qa_verb(t.item),
                epic_tag_slack(t.item, &app.items),
                issue_link_slack(t.item, &app.config.owner),
                title_slack(t.item),
                t.item.status.label(),
                t.item.priority.as_deref().unwrap_or("--"),
            ));
        }
    }

    // Blockers
    if !data.blocked.is_empty() {
        r.push_str("\n*Blockers*\n");
        for t in &data.blocked {
            r.push_str(&format!(
                "• {}{} {} (Blocked){}\n",
                epic_tag_slack(t.item, &app.items),
                issue_link_slack(t.item, &app.config.owner),
                title_slack(t.item),
                if t.is_ot { " (OT)" } else { "" },
            ));
        }
    }

    r
}

// ─── Sprint Report formatters ───────────────────────────────────────────────

/// One line of the "By EPIC" section. `name` carries the format-specific epic
/// link.
fn epic_group_line(g: &EpicGroup, name: &str) -> String {
    let mut s = format!("{} - {} tickets", name, g.tickets);
    if g.bugs > 0 {
        s.push_str(&format!(" ({} bugs)", g.bugs));
    }
    if g.in_qa > 0 {
        s.push_str(&format!(" | in QA: {}", g.in_qa));
    }
    if g.in_uat > 0 {
        s.push_str(&format!(" | in UAT: {}", g.in_uat));
    }
    if g.tested > 0 {
        if g.failed > 0 {
            s.push_str(&format!(
                " | tested: {} ({} passed, {} failed)",
                g.tested, g.passed, g.failed
            ));
        } else {
            s.push_str(&format!(" | tested: {} (all passed)", g.tested));
        }
    }
    s
}

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

/// "Done: 14 (13%) = 6 Tech Complete + 8 Done". Tech Complete and Ready for
/// Release are done as far as the sprint goes - the release note ships them -
/// and the split stays visible for whoever prepares that release.
fn done_summary(d: &SprintReportData) -> String {
    done_line(
        d.done_count,
        d.done_pct,
        d.tech_complete_count,
        d.ready_release_count,
        d.done_column_count,
    )
}

/// [`done_summary`] from its counts.
fn done_line(
    done_count: usize,
    done_pct: f64,
    tech_complete: usize,
    ready_release: usize,
    done_column: usize,
) -> String {
    let head = format!("Done: {} ({:.0}%)", done_count, done_pct);
    let mut parts = Vec::new();
    if tech_complete > 0 {
        parts.push(format!("{} Tech Complete", tech_complete));
    }
    if ready_release > 0 {
        parts.push(format!("{} Ready for Release", ready_release));
    }
    if done_column > 0 {
        parts.push(format!("{} Done", done_column));
    }
    // The split says what "done" is made of, so it shows whenever any of
    // it is not the Done column: `Done: 6 (13%)` read as six tickets in Done
    // when they were all Tech Complete.
    if parts.len() > 1 || (parts.len() == 1 && done_column == 0) {
        format!("{} = {}", head, parts.join(" + "))
    } else {
        head
    }
}

/// "39 tested since Sep 14" - the window the QA metrics cover, when scoped.
fn tested_summary(d: &SprintReportData) -> String {
    match d.metrics_since {
        Some(day) => format!("{} tested since {}", d.tested_count, day.format("%b %d")),
        None => format!("{} tested", d.tested_count),
    }
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
    r.push_str(&format!("- {}\n", done_summary(&data)));
    if data.blocked_count > 0 {
        r.push_str(&format!("- Blocked: {}\n", data.blocked_count));
    }

    // ── By EPIC ──
    if !data.epics.is_empty() {
        r.push_str("\n### By EPIC / root ticket\n");
        for g in &data.epics {
            let name = g.root.group_name(&issue_link_md);
            r.push_str(&format!("- {}\n", epic_group_line(g, &name)));
        }
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
                "- Backlog / Blocked: {}{}\n",
                data.bugs_blocked.len(),
                prio_summary(&data.bugs_blocked)
            ));
        }
    }

    // ── QA Results ──
    if data.tested_count > 0 {
        let pass_pct = data.passed_count as f64 * 100.0 / data.tested_count as f64;
        r.push_str(&format!("\n### QA Results ({})\n", tested_summary(&data)));
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
                "- {}{} {} ({}, {}, failed {}×)\n",
                epic_tag(rt.item, &app.items),
                num,
                clean_title(&rt.item.title),
                prio,
                rt.item.status.label(),
                rt.fail_count
            ));
        }
    }

    // Bugs waiting for dev
    if !data.bugs_for_dev.is_empty() {
        r.push_str("\n### Bugs Waiting for Dev\n");
        r.push_str("| # | Epic | Title | Priority | Status | Assignee |\n");
        r.push_str("|---|---|---|---|---|---|\n");
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
                "| {} | {} | {} | {} | {} | {} |\n",
                num,
                epic_tag(bug, &app.items).trim(),
                clean_title(&bug.title),
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
    r.push_str(&format!("• {}\n", done_summary(&data)));
    if data.blocked_count > 0 {
        r.push_str(&format!("• Blocked: {}\n", data.blocked_count));
    }

    // ── Bugs ──
    // ── By EPIC ──
    if !data.epics.is_empty() {
        r.push_str("\nBy EPIC / root ticket:\n");
        for g in &data.epics {
            let name = g.root.group_name(&issue_num_text);
            r.push_str(&format!("• {}\n", epic_group_line(g, &name)));
        }
    }

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
                "• Backlog / Blocked: {}{}\n",
                data.bugs_blocked.len(),
                prio_summary(&data.bugs_blocked)
            ));
        }
    }

    // ── QA Results ──
    if data.tested_count > 0 {
        let pass_pct = data.passed_count as f64 * 100.0 / data.tested_count as f64;
        r.push_str(&format!("\nQA Results ({}):\n", tested_summary(&data)));
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
                "• {}{} {} ({}, {}, failed {}×)\n",
                epic_tag(rt.item, &app.items),
                num,
                clean_title(&rt.item.title),
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
                "• {}{} {} | {} | {} | {}\n",
                epic_tag(bug, &app.items),
                num,
                clean_title(&bug.title),
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
    r.push_str(&format!("• {}\n", done_summary(&data)));
    if data.blocked_count > 0 {
        r.push_str(&format!("• Blocked: {}\n", data.blocked_count));
    }

    // ── Bugs ──
    // ── By EPIC ──
    if !data.epics.is_empty() {
        r.push_str("\n*By EPIC / root ticket*\n");
        for g in &data.epics {
            let name = g.root.group_name_with(
                &|e: &Item| issue_link_slack(e, &app.config.owner),
                &crate::slack::webhook::escape_mrkdwn,
            );
            r.push_str(&format!("• {}\n", epic_group_line(g, &name)));
        }
    }

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
                "• Backlog / Blocked: {}{}\n",
                data.bugs_blocked.len(),
                prio_summary(&data.bugs_blocked)
            ));
        }
    }

    // ── QA Results ──
    if data.tested_count > 0 {
        let pass_pct = data.passed_count as f64 * 100.0 / data.tested_count as f64;
        r.push_str(&format!("\n*QA Results* ({})\n", tested_summary(&data)));
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
                "• {}{} {} ({}, {}, failed {}×){}\n",
                epic_tag_slack(rt.item, &app.items),
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
                "• {}{} {} | {} | {} | {}\n",
                epic_tag_slack(bug, &app.items),
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
    r.push_str(&format!(
        "## {} - {}\n\n",
        num,
        escape_md_link_text(&clean_title(&item.title))
    ));

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
        for pr in &item.linked_prs {
            r.push_str(&format!(
                "- [{}]({}) {} -> {}: {}\n",
                pr.short_ref(),
                pr.url,
                pr.state.to_lowercase(),
                pr.base_ref,
                clean_title(&pr.title)
            ));
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

// ─── Ticket lists ───────────────────────────────────────────────────────────

/// The issue's web URL, when it has a number and a repository.
fn issue_url(item: &Item) -> Option<String> {
    let (num, repo) = (item.number?, item.repository.as_deref()?);
    Some(if repo.starts_with("https://") {
        format!("{}/issues/{}", repo, num)
    } else {
        format!("https://github.com/{}/issues/{}", repo, num)
    })
}

/// The facts after a ticket in a copied list: `In QA, Sprint 31, @a @b`.
fn list_meta(item: &Item, name: &dyn Fn(&str) -> String) -> String {
    let mut parts = vec![item.status.label().to_string()];
    if let Some(sprint) = item.sprint.as_deref() {
        parts.push(sprint.to_string());
    }
    if !item.assignees.is_empty() {
        let who: Vec<String> = item.assignees.iter().map(|a| name(a)).collect();
        parts.push(who.join(" "));
    }
    parts.join(", ")
}

/// `#4224 Title` for a list line.
fn list_label(item: &Item) -> String {
    let title = clean_title(&item.title);
    match item.number {
        Some(n) => format!("#{} {}", n, title),
        None => title,
    }
}

/// Markdown text: a title like `[FE] Copy changes` would otherwise close the
/// link early, and GitHub drops `<Activity>` as an unknown HTML tag (checked
/// through its markdown API, 2026-10-14). Backslash escapes keep both.
fn escape_md_link_text(s: &str) -> String {
    let plain = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('[', "\\[")
            .replace(']', "\\]")
            .replace('<', "\\<")
            .replace('>', "\\>")
            .replace('`', "\\`")
    };
    // Inside a `code span` Markdown takes every character literally, so an
    // escape there would show its backslash; everything else is escaped.
    // Spans are matched the way CommonMark does it, not guessed from how many
    // backticks there are: a run of N backticks opens a span only when a later
    // run of exactly N closes it. A guess that disagreed with the renderer
    // left `](url)` unescaped outside any span, and a title could repoint its
    // own link. An unmatched run is escaped like any other text.
    let chars: Vec<char> = s.chars().collect();
    let run_at = |i: usize| chars[i..].iter().take_while(|&&c| c == '`').count();
    let mut out = String::new();
    let mut text_start = 0;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            i += 1;
            continue;
        }
        let open = run_at(i);
        let mut j = i + open;
        let mut close = None;
        while j < chars.len() {
            if chars[j] == '`' {
                let run = run_at(j);
                if run == open {
                    close = Some(j + run);
                    break;
                }
                j += run;
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                out.push_str(&plain(&chars[text_start..i].iter().collect::<String>()));
                out.extend(&chars[i..end]);
                i = end;
                text_start = end;
            }
            None => i += open,
        }
    }
    out.push_str(&plain(&chars[text_start..].iter().collect::<String>()));
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Tickets as a Markdown list, one linked line each, in the given order.
///
/// For GitHub and Notion. People are GitHub logins, which GitHub turns into
/// mentions.
pub fn ticket_list_md(items: &[Item], indices: &[usize]) -> String {
    let login = |a: &str| format!("@{}", a);
    indices
        .iter()
        .filter_map(|&i| items.get(i))
        .map(|item| {
            let label = list_label(item);
            let head = match issue_url(item) {
                Some(url) => format!("[{}]({})", escape_md_link_text(&label), url),
                None => escape_md_link_text(&label),
            };
            format!("- {} - {}\n", head, list_meta(item, &login))
        })
        .collect()
}

/// Tickets as a list for Slack: (HTML with linked titles, plain-text fallback).
///
/// People are Slack display names from the user map where one exists, like
/// the reports' copy formats, because a pasted `@login` pings nobody.
pub fn ticket_list_rich(
    items: &[Item],
    indices: &[usize],
    user_map: &std::collections::HashMap<String, crate::config::SlackUserMapping>,
) -> (String, String) {
    let name = |a: &str| resolve_display_name(a, user_map);
    let mut html = String::from("<ul>");
    let mut plain = String::new();
    for item in indices.iter().filter_map(|&i| items.get(i)) {
        let label = list_label(item);
        let meta = list_meta(item, &name);
        match issue_url(item) {
            Some(url) => {
                html.push_str(&format!(
                    "<li><a href=\"{}\">{}</a> - {}</li>",
                    escape_html(&url),
                    escape_html(&label),
                    escape_html(&meta)
                ));
                plain.push_str(&format!("• {} - {} - {}\n", label, meta, url));
            }
            None => {
                html.push_str(&format!(
                    "<li>{} - {}</li>",
                    escape_html(&label),
                    escape_html(&meta)
                ));
                plain.push_str(&format!("• {} - {}\n", label, meta));
            }
        }
    }
    html.push_str("</ul>");
    (html, plain)
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

/// Copy rich text (HTML) with a plain-text alternative.
///
/// Slack's message box keeps the links of pasted HTML but shows `<url|text>`
/// markup literally - that syntax only works through the API - so a list
/// meant for Slack goes on the clipboard as HTML. Apps that only take plain
/// text get `plain`.
pub fn copy_html_to_clipboard(html: &str, plain: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;
    clipboard
        .set_html(html, Some(plain))
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

    #[test]
    fn test_markdown_link_text_leaves_code_spans_alone() {
        // Escapes inside `...` show their backslash on GitHub.
        assert_eq!(
            escape_md_link_text("Crash in `<Select>` dropdown [x] a<b>"),
            "Crash in `<Select>` dropdown \\[x\\] a\\<b\\>"
        );
        // An unmatched backtick is plain text, escaped with the rest.
        assert_eq!(escape_md_link_text("it's `odd <x>"), "it's \\`odd \\<x\\>");
        // Three backticks never closed, then one: no span at all, so the
        // `](url)` after them must stay escaped (it used to repoint the link).
        assert_eq!(
            escape_md_link_text("a ``` ](https://evil.example) <b> `"),
            "a \\`\\`\\` \\](https://evil.example) \\<b\\> \\`"
        );
        // Runs of different lengths: `` closes ``, a lone ` inside is code.
        assert_eq!(
            escape_md_link_text("x ``a ` b`` [y]"),
            "x ``a ` b`` \\[y\\]"
        );
    }
    use crate::models::root::{epic_short_name, is_epic};
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
    fn test_classify_move_into_uat() {
        // Only the user's own events are classified. In QA -> In UAT is the
        // QA's Move to In UAT (`I`): reported as the move, not as a comment.
        assert_eq!(
            classify_status_change(Some("In QA"), Some("In UAT")),
            Some(ActionKind::WorkedOn {
                from: "In QA".into(),
                to: "In UAT".into()
            })
        );
        // Any other way into In UAT is still a dev's deploy step.
        assert_eq!(
            classify_status_change(Some("In QA - Dev"), Some("In UAT")),
            None
        );
        assert_eq!(
            classify_status_change(Some("In Progress"), Some("In UAT")),
            None
        );
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
        assert!(matches!(kind, Some(ActionKind::WorkedOn { .. })));

        // Ready for Dev = generic WorkedOn
        let kind = classify_status_change(Some("Backlog"), Some("Ready for Dev"));
        assert!(matches!(kind, Some(ActionKind::WorkedOn { .. })));
    }

    #[test]
    fn test_action_priority_order() {
        assert!(ActionKind::FailedOnDev.priority() < ActionKind::Commented.priority());
        assert!(ActionKind::VerifiedForRelease.priority() < ActionKind::Commented.priority());
        assert!(ActionKind::Blocked.priority() < ActionKind::Commented.priority());
    }

    #[test]
    fn test_classify_ready_for_release() {
        // Tech Complete -> Ready for Release is the release step after QA,
        // not a verdict: reported as the move it was.
        let kind = classify_status_change(Some("Tech Complete"), Some("Ready for Release"));
        assert!(matches!(kind, Some(ActionKind::WorkedOn { .. })));

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
            latest_qa_verdict(&t, None).map(|(v, _)| v),
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
            latest_qa_verdict(&t, None).map(|(v, _)| v),
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
            latest_qa_verdict(&t, None).map(|(v, _)| v),
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
        assert_eq!(latest_qa_verdict(&t, None), None);
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
            latest_qa_verdict(&t, None).map(|(v, _)| v),
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

    fn list_items() -> Vec<Item> {
        vec![
            Item {
                id: "a".into(),
                number: Some(4224),
                repository: Some("Kinatico/kup-project-management".into()),
                title: "[FE] Copy changes\nin <Activity> Library".into(),
                status: Status::InQA,
                sprint: Some("Sprint 31".into()),
                assignees: vec!["vxtien-qa".into(), "dev-a".into()],
                ..Default::default()
            },
            Item {
                id: "b".into(),
                title: "Draft idea".into(),
                status: Status::Backlog,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn test_ticket_list_md_links_each_ticket_in_order() {
        let items = list_items();
        let md = ticket_list_md(&items, &[1, 0]);
        let lines: Vec<&str> = md.lines().collect();
        assert_eq!(lines[0], "- Draft idea - Backlog");
        // Brackets in the title are escaped so the link text doesn't end early.
        assert_eq!(
            lines[1],
            "- [#4224 \\[FE\\] Copy changes in \\<Activity\\> Library](https://github.com/Kinatico/kup-project-management/issues/4224) - In QA, Sprint 31, @vxtien-qa @dev-a"
        );
    }

    #[test]
    fn test_ticket_list_rich_links_and_escapes_for_slack() {
        let items = list_items();
        let mut map = std::collections::HashMap::new();
        map.insert(
            "vxtien-qa".to_string(),
            crate::config::SlackUserMapping {
                slack_id: "U1".into(),
                slack_display: "Bruce Vu".into(),
            },
        );
        let (html, plain) = ticket_list_rich(&items, &[0, 1], &map);
        assert!(html.starts_with(
            "<ul><li><a href=\"https://github.com/Kinatico/kup-project-management/issues/4224\">"
        ));
        assert!(html.contains("#4224 [FE] Copy changes in &lt;Activity&gt; Library</a> - In QA, Sprint 31, @Bruce Vu @dev-a</li>"));
        assert!(html.ends_with("<li>Draft idea - Backlog</li></ul>"));
        assert_eq!(
            plain,
            "• #4224 [FE] Copy changes in <Activity> Library - In QA, Sprint 31, @Bruce Vu @dev-a - https://github.com/Kinatico/kup-project-management/issues/4224\n• Draft idea - Backlog\n"
        );
    }

    #[test]
    fn test_clean_title_puts_a_wrapped_title_back_on_one_line() {
        assert_eq!(
            clean_title("[BUG] Title:\nUnauthorised APIs are called "),
            "[BUG] Title: Unauthorised APIs are called"
        );
    }

    #[test]
    fn test_is_epic_accepts_either_the_label_or_the_title_prefix() {
        // The board uses both conventions, and some epics carry only one.
        let labelled = Item {
            title: "Dave project".into(),
            labels: vec!["Epic".into()],
            ..Default::default()
        };
        let prefixed = Item {
            title: "[Epic] Dave project".into(),
            ..Default::default()
        };
        let story = Item {
            title: "US 13: Segments".into(),
            labels: vec!["Story".into()],
            ..Default::default()
        };
        assert!(is_epic(&labelled));
        assert!(is_epic(&prefixed));
        assert!(!is_epic(&story));
    }

    #[test]
    fn test_epic_short_name_drops_the_prefix() {
        let epic = Item {
            title: "[Epic] Dave project".into(),
            ..Default::default()
        };
        assert_eq!(epic_short_name(&epic), "Dave project");
    }

    #[test]
    fn test_qa_verb_separates_testing_from_verifying() {
        let task = Item {
            title: "Support per-template repetition config".into(),
            labels: vec!["Task".into()],
            ..Default::default()
        };
        let unlabelled = Item {
            title: "no label at all".into(),
            ..Default::default()
        };
        let bug = Item {
            title: "[BUG] Stripe form breaks".into(),
            labels: vec!["Bug".into()],
            ..Default::default()
        };
        // Feature work that merely carries the Enhancement label.
        let dev_enhancement = Item {
            title: "[Enhancement][BE] Refactor email handling".into(),
            labels: vec!["Enhancement".into()],
            ..Default::default()
        };
        // Raised by QA through the report form.
        let qa_enhancement = Item {
            title: "[ENHANCEMENT] Improve the empty state".into(),
            labels: vec!["Enhancement".into()],
            ..Default::default()
        };

        assert_eq!(qa_verb(&task), "Testing");
        assert_eq!(qa_verb(&unlabelled), "Testing");
        assert_eq!(qa_verb(&bug), "Verifying");
        assert_eq!(
            qa_verb(&dev_enhancement),
            "Testing",
            "the Enhancement label alone is feature work"
        );
        assert_eq!(qa_verb(&qa_enhancement), "Verifying");
    }

    #[test]
    fn test_resolve_epic_walks_up_through_a_story() {
        // ticket -> story -> epic. Sync fetches ONE parent level, so the epic
        // is only reachable by re-reading the story from the board.
        let items = vec![
            Item {
                number: Some(1),
                title: "[Epic] Emails".into(),
                repository: Some("org/web".into()),
                ..Default::default()
            },
            Item {
                number: Some(2),
                title: "Revive invitation emails".into(),
                labels: vec!["Story".into()],
                parent_number: Some(1),
                repository: Some("org/web".into()),
                ..Default::default()
            },
            Item {
                number: Some(3),
                title: "ticket".into(),
                parent_number: Some(2),
                repository: Some("org/web".into()),
                ..Default::default()
            },
        ];

        let epic = resolve_epic(&items[2], &items).expect("epic found through the story");
        assert_eq!(epic.number, Some(1));
        assert_eq!(epic_tag(&items[2], &items), "[Emails] ");
    }

    #[test]
    fn test_resolve_epic_ignores_the_same_number_in_another_repo() {
        // The project spans repositories and issue numbers repeat across them,
        // so a number-only match would file a ticket under a foreign epic.
        let items = vec![
            Item {
                number: Some(1),
                title: "[Epic] Epic in the API repo".into(),
                repository: Some("org/api".into()),
                ..Default::default()
            },
            Item {
                number: Some(9),
                title: "ticket".into(),
                parent_number: Some(1),
                parent_title: Some("Story in the web repo".into()),
                repository: Some("org/web".into()),
                ..Default::default()
            },
        ];

        assert!(resolve_epic(&items[1], &items).is_none());
        assert_eq!(epic_tag(&items[1], &items), "[Story in the web repo] ");
    }

    #[test]
    fn test_a_failed_ticket_is_reported_apart_from_the_passes() {
        use crate::app::App;
        let yesterday = Utc::now() - chrono::Duration::days(1);
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![
            Item {
                id: "sent-back".into(),
                number: Some(1),
                title: "failed on STG".into(),
                status: Status::InProgress,
                assignees: vec!["qa".into()],
                status_history: Some(vec![status_evt("In QA", "In Progress", yesterday)]),
                ..Default::default()
            },
            Item {
                id: "passed".into(),
                number: Some(2),
                title: "verified".into(),
                status: Status::TechComplete,
                assignees: vec!["qa".into()],
                status_history: Some(vec![status_evt("In QA", "Tech Complete", yesterday)]),
                ..Default::default()
            },
        ];

        let data = collect_daily_standup(&app);
        assert_eq!(data.failed.len(), 1, "the rejection gets its own section");
        assert_eq!(data.failed[0].item.id, "sent-back");
        assert_eq!(data.tested.len(), 1, "and is not counted among the passes");
        assert_eq!(data.tested[0].item.id, "passed");
    }

    #[test]
    fn test_issues_found_takes_qa_enhancements_but_not_dev_ones() {
        use crate::app::App;
        let yesterday = Utc::now() - chrono::Duration::days(1);
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![
            Item {
                id: "qa-enh".into(),
                number: Some(1),
                title: "[ENHANCEMENT] Improve the empty state".into(),
                status: Status::ReadyForDev,
                labels: vec!["Enhancement".into()],
                assignees: vec!["qa".into()],
                created_at: Some(yesterday),
                ..Default::default()
            },
            Item {
                id: "dev-enh".into(),
                number: Some(2),
                title: "[Enhancement][BE] Refactor email handling".into(),
                status: Status::ReadyForDev,
                labels: vec!["Enhancement".into()],
                assignees: vec!["qa".into()],
                created_at: Some(yesterday),
                ..Default::default()
            },
        ];

        let data = collect_daily_standup(&app);
        let found: Vec<&str> = data
            .issues_found
            .iter()
            .map(|t| t.item.id.as_str())
            .collect();
        assert_eq!(
            found,
            ["qa-enh"],
            "only the enhancement QA raised counts as an issue found"
        );
    }

    /// Yesterday in the configured timezone (UTC+7 by default) at a fixed
    /// local time, so two events built from it fall on the same workday.
    fn yesterday_at(hour: u32, minute: u32) -> DateTime<Utc> {
        let tz = FixedOffset::east_opt(7 * 3600).unwrap();
        let day = (Utc::now().with_timezone(&tz) - chrono::Duration::days(1)).date_naive();
        tz.from_local_datetime(&day.and_hms_opt(hour, minute, 0).unwrap())
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn test_standup_takes_the_latest_verdict_across_status_and_label_events() {
        // #1154, Sprint 29: failed on STG at 17:12, moved back into QA at 17:15,
        // passed STG (label) at 17:16. The standup said "Failed on STG" because
        // label passes were only consulted when no status event matched.
        use crate::app::App;
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "retested".into(),
            number: Some(1154),
            title: "address field".into(),
            status: Status::InQA,
            status_history: Some(vec![
                status_evt("In QA", "In Progress", yesterday_at(17, 12)),
                status_evt("In Progress", "In QA", yesterday_at(17, 15)),
            ]),
            label_history: Some(vec![label_evt("Ready-for-UAT", yesterday_at(17, 16))]),
            ..Default::default()
        }];

        let data = collect_daily_standup(&app);
        assert!(data.failed.is_empty(), "the later pass supersedes the fail");
        assert_eq!(data.tested.len(), 1);
        assert_eq!(data.tested[0].kind, ActionKind::PassedStg);
    }

    #[test]
    fn test_standup_reports_the_later_of_two_passes_on_one_day() {
        // Passed Dev in the morning, STG deployed, passed STG in the afternoon:
        // the day's result is the STG pass, not the first label found.
        use crate::app::App;
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "twice".into(),
            number: Some(2),
            title: "fast lane".into(),
            status: Status::InQA,
            status_history: Some(vec![]),
            label_history: Some(vec![
                label_evt("Ready-for-Staging", yesterday_at(9, 0)),
                label_evt("Ready-for-UAT", yesterday_at(15, 0)),
            ]),
            ..Default::default()
        }];

        let data = collect_daily_standup(&app);
        assert_eq!(data.tested.len(), 1);
        assert_eq!(data.tested[0].kind, ActionKind::PassedStg);
    }

    #[test]
    fn test_tech_complete_is_only_verified_when_it_comes_out_of_a_qa_column() {
        // #3874/#3879: moved Ready for Dev -> Tech Complete by the QA, never in
        // a QA column. The standup called that "Verified, ready for release"
        // while the sprint report (correctly) did not count it as tested.
        let kind = classify_status_change(Some("Ready for Dev"), Some("Tech Complete"))
            .expect("a move by the QA is still worth reporting");
        assert!(!matches!(kind, ActionKind::VerifiedForRelease));
        assert_eq!(
            kind.full_description(),
            "Moved Ready for Dev → Tech Complete"
        );

        // Same for Done, and for the release-management step after QA.
        assert!(!matches!(
            classify_status_change(Some("In Progress"), Some("Done")),
            Some(ActionKind::TestComplete)
        ));
        assert!(!matches!(
            classify_status_change(Some("Tech Complete"), Some("Ready for Release")),
            Some(ActionKind::VerifiedForRelease)
        ));

        // The real verdicts are untouched.
        assert_eq!(
            classify_status_change(Some("In UAT"), Some("Tech Complete")),
            Some(ActionKind::VerifiedForRelease)
        );
        assert_eq!(
            classify_status_change(Some("In QA"), Some("Done")),
            Some(ActionKind::TestComplete)
        );
    }

    #[test]
    fn test_a_bug_the_qa_filed_and_assigned_to_a_dev_is_an_issue_found() {
        // #3989: filed by the QA through the form, assigned to the developers
        // only. The assignee heuristic missed it and the ticket surfaced as
        // "Worked on ticket (Ready for Dev)" instead.
        use crate::app::App;
        let created = yesterday_at(11, 15);
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "bug".into(),
            number: Some(3989),
            title: "[BUG] wrong billing flow".into(),
            status: Status::ReadyForDev,
            labels: vec!["Bug".into()],
            assignees: vec!["dev1".into(), "dev2".into()],
            created_at: Some(created),
            status_history: Some(vec![status_evt("", "Ready for Dev", created)]),
            label_history: Some(vec![label_evt("Bug", created)]),
            ..Default::default()
        }];

        let data = collect_daily_standup(&app);
        assert_eq!(data.issues_found.len(), 1, "the QA filed it");
        assert!(data.tested.is_empty(), "and it is not 'work on a ticket'");
    }

    #[test]
    fn test_a_bug_a_dev_filed_and_assigned_to_the_qa_is_not_an_issue_found() {
        use crate::app::App;
        let created = yesterday_at(11, 15);
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        let mut dev_evt = status_evt("", "Ready for Dev", created);
        dev_evt.actor = Some("dev1".into());
        let mut dev_label = label_evt("Bug", created);
        dev_label.actor = Some("dev1".into());
        app.items = vec![Item {
            id: "dev-bug".into(),
            number: Some(7),
            title: "[BUG] found by a dev".into(),
            status: Status::ReadyForDev,
            labels: vec!["Bug".into()],
            assignees: vec!["qa".into()],
            created_at: Some(created),
            status_history: Some(vec![dev_evt]),
            label_history: Some(vec![dev_label]),
            ..Default::default()
        }];

        let data = collect_daily_standup(&app);
        assert!(data.issues_found.is_empty(), "being assigned is not filing");
    }

    #[test]
    fn test_epic_in_qa_matches_the_sprint_overview() {
        // By EPIC said "in QA: 52" while the overview said "In QA: 50" - the
        // epic rollup counted In UAT as QA, the overview lists it separately.
        use crate::app::App;
        let mut app = App::new();
        app.items = vec![
            Item {
                number: Some(1),
                title: "[Epic] Dave project".into(),
                repository: Some("org/web".into()),
                ..Default::default()
            },
            Item {
                id: "a".into(),
                number: Some(2),
                title: "in qa".into(),
                status: Status::InQA,
                parent_number: Some(1),
                repository: Some("org/web".into()),
                ..Default::default()
            },
            Item {
                id: "b".into(),
                number: Some(3),
                title: "in uat".into(),
                status: Status::InUAT,
                parent_number: Some(1),
                repository: Some("org/web".into()),
                ..Default::default()
            },
        ];

        let data = collect_sprint_report(&app);
        let dave = data
            .epics
            .iter()
            .find(|g| matches!(g.root, Root::Epic(_)))
            .expect("epic group");
        assert_eq!(dave.in_qa, 1, "In UAT is not In QA");
        assert_eq!(dave.in_uat, 1);
        let in_qa_sum: usize = data.epics.iter().map(|g| g.in_qa).sum();
        assert_eq!(in_qa_sum, data.in_qa_pipeline);
        assert!(epic_group_line(dave, "Dave").contains("in UAT: 1"));
    }

    #[test]
    fn test_verdicts_before_the_sprint_started_do_not_count_as_this_sprints_testing() {
        // 12 of Sprint 29's 39 "tested" verdicts were dated before the sprint
        // began (carry-overs passed on Dev weeks earlier and never deployed).
        let t = tl(
            vec![],
            vec![label_evt("Ready-for-Staging", make_utc(2026, 8, 28, 8, 15))],
        );
        let sprint_start = make_utc(2026, 9, 13, 17, 0); // Sep 14 00:00 UTC+7
        assert_eq!(latest_qa_verdict(&t, Some(sprint_start)), None);
        assert_eq!(
            latest_qa_verdict(&t, None).map(|(v, _)| v),
            Some(QaVerdict::Passed),
            "without a window the verdict still stands"
        );

        // A fail before the window and a pass inside it: only the pass counts.
        let t = tl(
            vec![status_evt(
                "In QA",
                "In Progress",
                make_utc(2026, 9, 9, 8, 0),
            )],
            vec![label_evt("Ready-for-UAT", make_utc(2026, 9, 15, 8, 0))],
        );
        assert_eq!(
            latest_qa_verdict(&t, Some(sprint_start)).map(|(v, _)| v),
            Some(QaVerdict::Passed)
        );
    }

    #[test]
    fn test_sprint_report_scopes_qa_results_to_the_sprint_window() {
        use crate::app::App;
        use crate::models::field::{Field, FieldOption, FieldType};
        let mut app = App::new();
        app.fields = vec![Field {
            id: "f".into(),
            name: "Sprint".into(),
            field_type: FieldType::Iteration,
            options: vec![FieldOption {
                id: "o".into(),
                name: "Sprint 29".into(),
                start_date: NaiveDate::from_ymd_opt(2026, 9, 14),
                duration_days: Some(14),
            }],
        }];
        app.board_sprint_filter = Some("Sprint 29".into());
        app.items = vec![
            Item {
                id: "old-pass".into(),
                number: Some(1),
                title: "passed on Dev in August, still waiting for deploy".into(),
                status: Status::InQADev,
                sprint: Some("Sprint 29".into()),
                labels: vec!["Ready-for-Staging".into()],
                status_history: Some(vec![]),
                label_history: Some(vec![label_evt(
                    "Ready-for-Staging",
                    make_utc(2026, 8, 28, 8, 15),
                )]),
                ..Default::default()
            },
            Item {
                id: "old-fail".into(),
                number: Some(2),
                title: "failed before the sprint, still not fixed".into(),
                status: Status::InReview,
                sprint: Some("Sprint 29".into()),
                status_history: Some(vec![status_evt(
                    "In QA",
                    "In Progress",
                    make_utc(2026, 9, 9, 8, 13),
                )]),
                ..Default::default()
            },
            Item {
                id: "new-pass".into(),
                number: Some(3),
                title: "passed STG this sprint".into(),
                status: Status::InQA,
                sprint: Some("Sprint 29".into()),
                status_history: Some(vec![]),
                label_history: Some(vec![label_evt(
                    "Ready-for-UAT",
                    make_utc(2026, 9, 18, 4, 52),
                )]),
                ..Default::default()
            },
        ];

        let data = collect_sprint_report(&app);
        assert_eq!(data.tested_count, 1, "only this sprint's verdict");
        assert_eq!(data.passed_count, 1);
        assert_eq!(data.failed_count, 0);
        assert_eq!(data.metrics_since, NaiveDate::from_ymd_opt(2026, 9, 14));
        // The retest list is about the ticket's CURRENT state, so a pre-sprint
        // fail that was never retested still shows.
        assert_eq!(data.retest_items.len(), 1);
        assert!(data.retest_items[0].currently_failed);
        let epic_tested: usize = data.epics.iter().map(|g| g.tested).sum();
        assert_eq!(epic_tested, data.tested_count);
    }

    #[test]
    fn test_standup_yesterday_covers_work_outside_the_sprint_filter() {
        // Day one of a sprint: Friday's Tech Complete tickets still sit in the
        // previous sprint, so a sprint-scoped scan lost them from Monday's standup.
        use crate::app::App;
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = Some("Sprint 29".into());
        app.items = vec![
            Item {
                id: "prev-sprint".into(),
                number: Some(1),
                title: "verified on Friday".into(),
                status: Status::TechComplete,
                sprint: Some("Sprint 28".into()),
                status_history: Some(vec![status_evt(
                    "In UAT",
                    "Tech Complete",
                    yesterday_at(9, 30),
                )]),
                ..Default::default()
            },
            Item {
                id: "no-timeline-elsewhere".into(),
                number: Some(2),
                title: "backlog item in another sprint, never fetched".into(),
                status: Status::Backlog,
                sprint: Some("Sprint 30".into()),
                ..Default::default()
            },
            Item {
                id: "no-timeline-here".into(),
                number: Some(3),
                title: "sprint item, timeline not fetched yet".into(),
                status: Status::InQA,
                sprint: Some("Sprint 29".into()),
                repository: Some("org/web".into()),
                ..Default::default()
            },
        ];

        let data = collect_daily_standup(&app);
        assert_eq!(data.tested.len(), 1);
        assert_eq!(data.tested[0].item.id, "prev-sprint");
        assert_eq!(
            data.missing_history, 1,
            "only the sprint's own tickets count as missing"
        );
    }

    #[test]
    fn test_report_wording_does_not_invent_intent() {
        assert_eq!(ActionKind::Commented.description(), "Reviewed & commented");
        let moved = classify_status_change(Some("Backlog"), Some("Ready for Dev")).unwrap();
        assert_eq!(moved.full_description(), "Moved Backlog → Ready for Dev");
        let moved = classify_status_change(None, Some("Ready for Dev")).unwrap();
        assert_eq!(moved.full_description(), "Moved to Ready for Dev");
    }

    #[test]
    fn test_date_helpers_survive_a_bad_timezone_offset() {
        // is_ot and report_date guard the offset; these two used to unwrap.
        let now = Utc::now();
        let _ = is_today(now, 99);
        let _ = is_previous_workday(now, 99);
    }

    fn status_evt_by(
        actor: &str,
        prev: &str,
        curr: &str,
        at: DateTime<Utc>,
    ) -> crate::gh::timeline::StatusChangedEvent {
        let mut e = status_evt(prev, curr, at);
        e.actor = Some(actor.to_string());
        e
    }

    #[test]
    fn test_a_move_the_same_person_reverts_within_minutes_is_an_undo() {
        // #3844, Sprint 29: failed on STG for real earlier (here: a week
        // before, so it sits outside every "yesterday" window); dev moved it
        // to In QA - Dev; yesterday 11:11 QA moved it to In Progress and at
        // 11:17 moved it straight back - a misclick, per the QA. The standup
        // said "Failed on Dev" and the sprint report said "failed 2×".
        use crate::app::App;
        let earlier = |h, m| yesterday_at(h, m) - chrono::Duration::days(7);
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "3844".into(),
            number: Some(3844),
            title: "reorder flow".into(),
            status: Status::InQADev,
            status_history: Some(vec![
                status_evt_by("qa", "In QA", "In Progress", earlier(16, 41)),
                status_evt_by("dev", "In Progress", "In QA - Dev", earlier(19, 49)),
                status_evt_by("qa", "In QA - Dev", "In Progress", yesterday_at(11, 11)),
                status_evt_by("qa", "In Progress", "In QA - Dev", yesterday_at(11, 17)),
            ]),
            ..Default::default()
        }];

        let daily = collect_daily_standup(&app);
        assert!(daily.failed.is_empty(), "the undone fail is not a fail");
        assert!(daily.tested.is_empty(), "nor is it any other action");

        let sprint = collect_sprint_report(&app);
        assert_eq!(sprint.failed_count, 1, "the earlier fail still stands");
        assert_eq!(sprint.retest_items.len(), 1);
        assert_eq!(sprint.retest_items[0].fail_count, 1, "not 2");
        assert!(sprint.retest_items[0].currently_failed);
    }

    #[test]
    fn test_a_reversal_by_someone_else_is_not_an_undo() {
        // QA fails at 11:11, the dev puts it straight back into QA at 11:17
        // (say the fix was a one-liner): two real actions.
        use crate::app::App;
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "quick-fix".into(),
            number: Some(1),
            title: "quick fix".into(),
            status: Status::InQA,
            status_history: Some(vec![
                status_evt_by("qa", "In QA", "In Progress", yesterday_at(11, 11)),
                status_evt_by("dev", "In Progress", "In QA", yesterday_at(11, 17)),
            ]),
            ..Default::default()
        }];
        let daily = collect_daily_standup(&app);
        assert_eq!(daily.failed.len(), 1);
        assert_eq!(daily.failed[0].kind, ActionKind::FailedOnStg);
    }

    #[test]
    fn test_a_reversal_after_the_undo_window_is_not_an_undo() {
        use crate::app::App;
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "slow".into(),
            number: Some(1),
            title: "changed my mind two hours later".into(),
            status: Status::InQA,
            status_history: Some(vec![
                status_evt_by("qa", "In QA", "In Progress", yesterday_at(9, 0)),
                status_evt_by("qa", "In Progress", "In QA", yesterday_at(11, 0)),
            ]),
            ..Default::default()
        }];
        let daily = collect_daily_standup(&app);
        assert_eq!(daily.failed.len(), 1, "two hours apart is two decisions");
    }

    #[test]
    fn test_done_counts_tech_complete_and_ready_for_release() {
        // Tech Complete is done as far as the sprint goes (the release note
        // ships it); the overview used to count the Done column alone.
        use crate::app::App;
        let mut app = App::new();
        let mk = |id: &str, status: Status| Item {
            id: id.into(),
            title: id.into(),
            status,
            ..Default::default()
        };
        app.items = vec![
            mk("tc", Status::TechComplete),
            mk("rfr", Status::ReadyForRelease),
            mk("done", Status::Done),
            mk("qa", Status::InQA),
        ];
        let data = collect_sprint_report(&app);
        assert_eq!(data.done_count, 3);
        assert_eq!(data.done_pct, 75.0);
        let text = sprint_report_text(&app);
        assert!(
            text.contains("• Done: 3 (75%) = 1 Tech Complete + 1 Ready for Release + 1 Done"),
            "{text}"
        );
        assert!(
            !text.contains("• Tech Complete: 1"),
            "folded into Done: {text}"
        );
    }

    #[test]
    fn test_a_ticket_under_a_non_epic_root_is_tagged_with_that_root() {
        // 24 of Sprint 29's 54 "no epic" tickets hang off #3916 "Issues sprint
        // 29", a plain task. The root of the chain is what traces them.
        use crate::app::App;
        let mut app = App::new();
        app.items = vec![
            Item {
                id: "bucket".into(),
                number: Some(3916),
                title: "Issues sprint 29".into(),
                labels: vec!["Task".into()],
                repository: Some("org/web".into()),
                ..Default::default()
            },
            Item {
                id: "bug".into(),
                number: Some(3957),
                title: "[BUG] expired invoices".into(),
                labels: vec!["Bug".into()],
                parent_number: Some(3916),
                repository: Some("org/web".into()),
                ..Default::default()
            },
        ];
        assert_eq!(epic_tag(&app.items[1], &app.items), "[Issues sprint 29] ");

        let data = collect_sprint_report(&app);
        let text = sprint_report_text(&app);
        assert!(
            text.contains("• #3916 Issues sprint 29 - 1 tickets (1 bugs)"),
            "{text}"
        );
        // The bucket task itself has no parent.
        assert!(text.contains("• No parent - 1 tickets"), "{text}");
        assert_eq!(data.epics.len(), 2);
    }

    #[test]
    fn test_a_parent_that_is_not_on_the_board_is_shown_by_its_synced_title() {
        // Sync stores the parent's title even when the parent is not a
        // project item, so the chain can still be named where it leaves the board.
        let items = vec![Item {
            number: Some(9),
            title: "ticket".into(),
            parent_number: Some(2491),
            parent_title: Some("[Epic] Billing overhaul".into()),
            repository: Some("org/web".into()),
            ..Default::default()
        }];
        assert_eq!(epic_tag(&items[0], &items), "[Billing overhaul] ");

        let orphan = Item {
            number: Some(10),
            title: "no chain at all".into(),
            ..Default::default()
        };
        assert_eq!(epic_tag(&orphan, &items), "[no parent] ");
    }

    #[test]
    fn test_a_filed_bug_is_reported_once_not_twice() {
        // Filing a bug also shows up as a status change to Ready for Dev, so
        // the ticket used to appear both as yesterday's "work" and as a bug.
        use crate::app::App;
        let yesterday = Utc::now() - chrono::Duration::days(1);
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![Item {
            id: "bug".into(),
            number: Some(1),
            title: "filed by the QA".into(),
            status: Status::ReadyForDev,
            labels: vec!["Bug".into()],
            assignees: vec!["qa".into()],
            created_at: Some(yesterday),
            status_history: Some(vec![status_evt("", "Ready for Dev", yesterday)]),
            ..Default::default()
        }];

        let data = collect_daily_standup(&app);
        assert_eq!(data.issues_found.len(), 1, "belongs in Issues found");
        assert!(
            data.tested.is_empty(),
            "filing a bug is not a test result, got {:?}",
            data.tested
                .iter()
                .map(|a| a.kind.full_description())
                .collect::<Vec<_>>()
        );

        // Monday morning: the bug was filed on the last day of the sprint
        // that ended, and the board now shows the next one.
        app.items[0].sprint = Some("Sprint 29".into());
        app.board_sprint_filter = Some("Sprint 30".into());
        let data = collect_daily_standup(&app);
        assert_eq!(data.issues_found.len(), 1, "any sprint");
        assert!(data.tested.is_empty());
    }

    #[test]
    fn test_done_split_shows_whenever_done_is_not_the_done_column() {
        assert_eq!(
            done_line(6, 13.0, 6, 0, 0),
            "Done: 6 (13%) = 6 Tech Complete"
        );
        assert_eq!(done_line(6, 13.0, 0, 0, 6), "Done: 6 (13%)");
        assert_eq!(
            done_line(14, 13.0, 6, 0, 8),
            "Done: 14 (13%) = 6 Tech Complete + 8 Done"
        );
    }

    #[test]
    fn test_slack_root_names_are_escaped() {
        let items = vec![
            Item {
                id: "e".into(),
                number: Some(1),
                title: "[Epic] R&D <beta> tools".into(),
                labels: vec!["Epic".into()],
                ..Default::default()
            },
            Item {
                id: "t".into(),
                number: Some(2),
                parent_number: Some(1),
                ..Default::default()
            },
        ];
        assert_eq!(
            epic_tag_slack(&items[1], &items),
            "[R&amp;D &lt;beta&gt; tools] "
        );
    }
}
