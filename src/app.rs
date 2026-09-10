use crate::cache::Cache;
use crate::config::Config;
use crate::models::field::Field;
use crate::models::item::Item;
use crate::models::status::Status;
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum AssigneeContext {
    Bug(Option<usize>, bool),
    Task,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Auth,
    Menu,
    Board,
    ItemDetail(usize), // index into items
    MyTasks,
    Search,
    BugReport(Option<usize>, bool), // parent item index (None = standalone), is_fail
    TaskForm(Option<usize>),        // parent item index (None = standalone)
    AssigneePicker(AssigneeContext),
    QaActions(usize),       // item index
    MoveDialog(Vec<usize>), // item indices (batch)
    Settings,
    Setup,
    SlackSetup,
    SlackNotifyConfig,
    SlackUserMap,
    Dashboard,
    Comment(usize), // item index
    Notifications,
    History(usize), // item index
    Help,
    StackLeadsEdit,
}

impl Screen {
    /// Short human label for the breadcrumb bar.
    pub fn label(&self, items: &[Item]) -> String {
        let num = |idx: &usize| {
            items
                .get(*idx)
                .and_then(|i| i.number)
                .map(|n| format!("#{}", n))
                .unwrap_or_else(|| "item".to_string())
        };
        match self {
            Screen::Auth => "Sign in".to_string(),
            Screen::Menu => "Menu".to_string(),
            Screen::Board => "Board".to_string(),
            Screen::MyTasks => "My Tasks".to_string(),
            Screen::Search => "Search".to_string(),
            Screen::Dashboard => "Dashboard".to_string(),
            Screen::Notifications => "Notifications".to_string(),
            Screen::Settings => "Settings".to_string(),
            Screen::Setup => "Setup".to_string(),
            Screen::SlackSetup => "Slack setup".to_string(),
            Screen::SlackNotifyConfig => "Slack notifications".to_string(),
            Screen::SlackUserMap => "Slack user map".to_string(),
            Screen::StackLeadsEdit => "Stack leads".to_string(),
            Screen::Help => "Help".to_string(),
            Screen::ItemDetail(idx) => num(idx),
            Screen::QaActions(_) => "QA actions".to_string(),
            Screen::MoveDialog(indices) => match indices.len() {
                0 | 1 => "Move".to_string(),
                n => format!("Move {} items", n),
            },
            Screen::History(_) => "History".to_string(),
            Screen::Comment(_) => "Comment".to_string(),
            Screen::BugReport(_, true) => "Fail: new bug".to_string(),
            Screen::BugReport(_, false) => "New bug".to_string(),
            Screen::TaskForm(_) => "New task".to_string(),
            Screen::AssigneePicker(_) => "Assignees".to_string(),
        }
    }
}

/// What the report form is filing.
///
/// Bug and Enhancement share one form, one draft and one submit path: they
/// differ only in the title prefix and the label, so duplicating eighteen
/// fields and the whole submit flow would just be two things to keep in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportKind {
    #[default]
    Bug,
    Enhancement,
}

impl ReportKind {
    /// Primary GitHub label, also used for display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Bug => "Bug",
            Self::Enhancement => "Enhancement",
        }
    }

    /// Every label applied to the new issue.
    ///
    /// An enhancement is also a piece of work to schedule, so it carries
    /// `Task` alongside `Enhancement`.
    pub fn labels(&self) -> &'static [&'static str] {
        match self {
            Self::Bug => &["Bug"],
            Self::Enhancement => &["Enhancement", "Task"],
        }
    }

    /// GitHub's native repository-level Issue Type.
    ///
    /// The repo offers Bug / Feature / Task; an enhancement is filed as Task.
    pub fn issue_type(&self) -> &'static str {
        match self {
            Self::Bug => "Bug",
            Self::Enhancement => "Task",
        }
    }

    /// Title prefix on the new issue.
    pub fn title_prefix(&self) -> &'static str {
        match self {
            Self::Bug => "[BUG]",
            Self::Enhancement => "[ENHANCEMENT]",
        }
    }

    /// Heading for the form screen.
    pub fn form_title(&self) -> &'static str {
        match self {
            Self::Bug => "Bug Report",
            Self::Enhancement => "Enhancement Request",
        }
    }

    /// Action name used in the Slack notification.
    pub fn slack_action(&self) -> &'static str {
        match self {
            Self::Bug => "Bug Report",
            Self::Enhancement => "Enhancement",
        }
    }
}

/// Popup overlay on top of current screen.
#[derive(Debug, Clone, PartialEq)]
pub enum Popup {
    None,
    Confirm {
        title: String,
        message: String,
        on_confirm: ConfirmAction,
    },
    Error(String),
    Success(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    PassDev(usize),
    PassStg(usize),
    /// UAT is a regression pass over everything currently deployed there, so
    /// the action covers the whole In UAT column, not just the ticket it was
    /// triggered from.
    PassUat {
        indices: Vec<usize>,
    },
    Return(usize),
    MoveItems {
        indices: Vec<usize>,
        target: Status,
    },
    SubmitBugReport(Option<usize>, bool),
    SubmitTask(Option<usize>),
    PostComment(usize),
    /// Strip stale Ready-for-* handoff labels so a returned ticket becomes
    /// testable again (escape hatch for tickets a dev moved back outside the TUI).
    ClearHandoffLabels(usize),
    ClearCache,
    SendStandupToSlack,
    SendSprintReportToSlack,
    MarkAllNotificationsRead,
    DeleteStackLead(String),
    Quit,
}

/// Event sent from background refresh thread
#[derive(Debug)]
pub enum BackgroundEvent {
    RefreshSuccess {
        items: Vec<Item>,
        fields: Vec<Field>,
        project_id: Option<String>,
    },
    RefreshError(String),
    StatusHistoryFetched {
        item_idx: usize,
        history: Vec<crate::gh::timeline::StatusChangedEvent>,
        comment_history: Vec<crate::gh::timeline::CommentEvent>,
        label_history: Vec<crate::gh::timeline::LabelEvent>,
    },
    /// B2: Batch created_at fetch for bug items
    CreatedAtFetched {
        item_idx: usize,
        created_at: DateTime<Utc>,
    },
}

/// Result from a background write operation (comment, label, move, etc.)
#[derive(Debug)]
pub enum ActionResult {
    Success(String),
    Error(String),
    ItemCreated(Box<crate::models::item::Item>, Vec<String>),
    /// Task issue created - main loop clears the task form on receipt.
    TaskCreated(String),
    /// Comment posted - main loop clears the comment draft on receipt.
    CommentPosted,
    /// A remote status change performed from a background thread succeeded;
    /// identified by item ID (indices may have been remapped meanwhile).
    ParentStatusChanged {
        item_id: String,
        status: Status,
    },
    /// Low-priority note for the status bar (no popup).
    StatusMessage(String),
    /// Revert an optimistic local label after the remote add failed -
    /// unblocks the "already passed" guard so the action can be retried.
    RemoveLabelLocal {
        item_id: String,
        label: &'static str,
    },
    /// Async Slack webhook test finished (SlackSetup screen).
    SlackWebhookTested {
        url: String,
        bot_token: Option<String>,
        error: Option<String>,
    },
    /// Async Slack members fetch finished (Settings → user map).
    SlackMembersFetched(Vec<crate::slack::users::SlackUser>),
    HistoryFetched(Vec<String>),
    NotificationsFetched(Vec<crate::gh::notifications::GitHubNotification>),
    /// Resolved by item ID - see BackgroundEvent::ItemDetailFetched.
    ItemDetailFetched {
        item_id: String,
        body: Option<String>,
        comments: Vec<crate::models::item::Comment>,
        labels: Vec<String>,
        created_at: Option<chrono::DateTime<chrono::Utc>>,
    },
}

/// The entire app state.
pub struct App {
    pub screen: Screen,
    pub previous_screens: Vec<Screen>,
    pub popup: Popup,
    pub config: Config,
    pub cache: Cache,

    pub bg_rx: Option<std::sync::mpsc::Receiver<BackgroundEvent>>,
    /// Persistent result channel for background write operations. Threads
    /// clone `action_tx`; results queue up instead of overwriting each other.
    pub action_tx: std::sync::mpsc::Sender<ActionResult>,
    pub action_rx: std::sync::mpsc::Receiver<ActionResult>,

    // Data
    pub items: Vec<Item>,
    pub fields: Vec<Field>,
    pub project_id: Option<String>,
    pub current_user: String,
    pub loading: bool,
    pub status_message: Option<String>,
    /// When the current status message was set - the global status bar shows
    /// it for a few seconds on every screen, then clears it.
    pub status_message_at: Option<std::time::Instant>,

    // Board state
    pub board_col_offset: usize,      // horizontal scroll
    pub board_row_offset: Vec<usize>, // vertical scroll per column
    pub board_selected_col: usize,
    pub board_selected_row: usize,
    pub board_sprint_filter: Option<String>,
    /// Cards that fit in the selected column, measured by the last render -
    /// used by the input handler for height-aware row scrolling.
    pub board_visible_cards: usize,

    // List state (My Tasks, Search)
    pub list_selected: usize,
    pub list_scroll: usize,
    /// Content rows of the My Tasks list viewport, measured by the last render.
    pub list_view_height: usize,

    // Search state
    pub search_query: String,
    pub search_results: Vec<usize>, // indices into items
    pub search_field_idx: usize,    // which filter field is focused

    // Filter state: Vec of selected values. "value" = include, "!value" = exclude
    pub filter_status: Vec<String>,
    pub filter_priority: Vec<String>,
    pub filter_stack: Vec<String>,
    pub filter_sprint: Vec<String>,
    pub filter_label: Vec<String>,
    pub filter_cursor: Vec<usize>, // cursor index per filter field (for Space toggle)

    // Detail state
    pub detail_scroll: usize,
    pub help_scroll: usize,
    pub dashboard_scroll: usize,
    pub settings_scroll: usize,

    // Bug report form state
    pub bug_field_idx: usize, // 0=title, 1=desc, 2=precond, 3=steps, 4=expected, 5=actual, 6=priority, 7=stack, 8=env
    pub bug_title: String,
    pub bug_description: String,
    pub bug_precondition: String,
    pub bug_test_data: String,
    pub bug_steps: String,
    pub bug_expected: String,
    pub bug_actual: String,
    pub bug_impact: String,
    pub bug_evidence: String,
    pub bug_security: String,
    pub bug_priority_idx: usize,    // 0=P0..4=P4
    pub bug_stack_idx: usize,       // 0=FE, 1=BE, 2=App UI, 3=App BE
    pub bug_env_idx: usize,         // index into App::BUG_ENVS
    pub bug_broken_idx: usize,      // 0=Completely, 1=Partially
    pub bug_visible_idx: usize,     // 0=Yes, 1=No
    pub bug_workaround_idx: usize,  // 0=Easy, 1=Difficult, 2=No Workaround
    pub bug_impact_prio_idx: usize, // 0=Yes, 1=No
    pub bug_assignees: Vec<String>,
    /// Whether the report form is filing a Bug or an Enhancement.
    pub bug_kind: ReportKind,

    // Task form state
    pub task_title: String,
    pub task_description: String,
    pub task_outcome: String,
    pub task_priority_idx: usize,
    pub task_stack_idx: usize,
    pub task_assignees: Vec<String>,
    pub task_field_idx: usize,

    // Assignee Picker state
    pub assignee_search: String,
    pub assignee_cursor: usize,

    // Comment state
    pub comment_text: String,
    /// Byte-offset cursor into comment_text (clamped/boundary-snapped on use).
    pub comment_cursor: usize,
    /// Which item the current comment draft belongs to (by item id) - lets a
    /// failed post be retried with `c` without wiping the draft.
    pub comment_draft_for: Option<String>,
    /// Esc pressed once with unsaved comment text - next Esc discards.
    pub comment_discard_armed: bool,
    /// Cursor into the focused bug-form text field (usize::MAX = end).
    pub bug_cursor: usize,
    /// Cursor into the focused task-form text field (usize::MAX = end).
    pub task_cursor: usize,

    // Notifications
    pub notifications: Vec<crate::gh::notifications::GitHubNotification>,

    // History state
    pub history_events: Vec<String>,

    // Stack Leads editor state
    pub stack_lead_cursor: usize,
    pub stack_lead_editing: bool,
    pub stack_lead_input: String,

    // Menu state
    pub menu_selected: usize,

    // Auth screen state
    pub auth_status_text: Option<String>,
    pub auth_feedback: Option<String>,
    pub auth_state: Option<crate::gh::auth::AuthStatus>,
    pub auth_fix_in_progress: bool,

    // Setup wizard state
    pub setup_field_idx: usize, // 0=project_url, 1=bug_repo_url
    pub setup_project_url: String,
    pub setup_bug_repo_url: String,

    // Slack setup state
    pub slack_webhook_input: String,
    pub slack_bot_token_input: String,
    pub slack_setup_field: usize, // 0=webhook, 1=bot_token
    pub slack_setup_status: Option<String>,
    pub slack_notify_cursor: usize,

    // Slack user mapping state
    pub slack_members: Vec<crate::slack::users::SlackUser>,
    pub slack_map_cursor: usize,        // which GitHub user is selected
    pub slack_map_picking: bool,        // true = showing Slack member picker
    pub slack_map_search: String,       // search/filter text in picker
    pub slack_map_member_cursor: usize, // which Slack member is highlighted

    // Timer: last time we polled auth status (for auto-refresh on Auth screen)
    /// Cursor row in the Move dialog (arrow-key navigation).
    pub move_cursor: usize,
    pub last_auth_poll: Option<std::time::Instant>,

    // Debounce: last time data was refreshed
    pub last_refresh: Option<DateTime<Utc>>,
    /// Last time an auto-refresh was ATTEMPTED (set even when the sync later
    /// fails) - keeps the retry loop going after a failed initial sync without
    /// spamming the API every frame.
    pub last_refresh_attempt: Option<DateTime<Utc>>,

    // Running flag
    pub running: bool,

    // Pending detail fetch (retried when bg_rx becomes available)
    /// Item IDs with a detail fetch in flight.
    ///
    /// Replaces the old single deferred index: the automatic fetch used to
    /// queue behind a background sync, and only one ticket could be queued at
    /// a time. Keyed by ID, so a refresh reordering `items` cannot make the
    /// result land on the wrong ticket.
    pub detail_fetch_inflight: std::collections::HashSet<String>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let (action_tx, action_rx) = std::sync::mpsc::channel();
        Self {
            screen: Screen::Auth,
            previous_screens: Vec::new(),
            popup: Popup::None,
            cache: Cache::new(config.poll_interval_secs),
            config,
            bg_rx: None,
            action_tx,
            action_rx,
            items: Vec::new(),
            fields: Vec::new(),
            project_id: None,
            current_user: String::new(),
            loading: false,
            status_message: None,
            status_message_at: None,
            board_col_offset: 0,
            board_row_offset: vec![0; Status::all_columns().len()],
            board_selected_col: 0,
            board_selected_row: 0,
            board_sprint_filter: None,
            board_visible_cards: 3,
            list_selected: 0,
            list_scroll: 0,
            list_view_height: 15,
            search_query: String::new(),
            search_results: Vec::new(),
            search_field_idx: 0,
            filter_status: Vec::new(),
            filter_priority: Vec::new(),
            filter_stack: Vec::new(),
            filter_sprint: Vec::new(),
            filter_label: Vec::new(),
            filter_cursor: vec![0; 5],
            detail_scroll: 0,
            help_scroll: 0,
            dashboard_scroll: 0,
            settings_scroll: 0,
            bug_field_idx: 0,
            bug_title: String::new(),
            bug_description: String::new(),
            bug_precondition: String::new(),
            bug_test_data: String::new(),
            bug_steps: String::new(),
            bug_expected: String::new(),
            bug_actual: String::new(),
            bug_impact: String::new(),
            bug_evidence: String::new(),
            bug_security: String::new(),
            bug_priority_idx: 2, // default P2
            bug_stack_idx: 1,    // default BE
            bug_env_idx: 1,      // default STG
            bug_broken_idx: 0,
            bug_visible_idx: 0,
            bug_workaround_idx: 2, // default No Workaround
            bug_impact_prio_idx: 0,
            bug_assignees: Vec::new(),
            bug_kind: ReportKind::Bug,
            task_title: String::new(),
            task_description: String::new(),
            task_outcome: String::new(),
            task_priority_idx: 0,
            task_stack_idx: 0,
            task_assignees: Vec::new(),
            task_field_idx: 0,
            assignee_search: String::new(),
            assignee_cursor: 0,
            comment_text: String::new(),
            comment_cursor: 0,
            comment_draft_for: None,
            comment_discard_armed: false,
            bug_cursor: usize::MAX,
            task_cursor: usize::MAX,
            notifications: Vec::new(),
            history_events: Vec::new(),
            stack_lead_cursor: 0,
            stack_lead_editing: false,
            stack_lead_input: String::new(),
            menu_selected: 0,
            auth_status_text: None,
            auth_feedback: None,
            auth_state: None,
            auth_fix_in_progress: false,
            setup_field_idx: 0,
            setup_project_url: String::new(),
            setup_bug_repo_url: String::new(),
            slack_webhook_input: String::new(),
            slack_bot_token_input: String::new(),
            slack_setup_field: 0,
            slack_setup_status: None,
            slack_notify_cursor: 0,
            slack_members: Vec::new(),
            slack_map_cursor: 0,
            slack_map_picking: false,
            slack_map_search: String::new(),
            slack_map_member_cursor: 0,
            move_cursor: 0,
            last_auth_poll: None,
            last_refresh: None,
            last_refresh_attempt: None,
            running: true,
            detail_fetch_inflight: std::collections::HashSet::new(),
        }
    }

    /// Open the report form for a Bug or an Enhancement.
    ///
    /// The only entry point, so no caller can navigate to the form and leave
    /// the kind pointing at whatever was filed last.
    pub fn open_report_form(&mut self, parent: Option<usize>, is_fail: bool, kind: ReportKind) {
        self.bug_kind = kind;
        self.goto(Screen::BugReport(parent, is_fail));
    }

    /// Navigate to a screen, pushing current to history.
    pub fn goto(&mut self, screen: Screen) {
        if matches!(screen, Screen::BugReport(_, _)) {
            if self.bug_assignees.is_empty() && !self.config.current_user.is_empty() {
                self.bug_assignees.push(self.config.current_user.clone());
            }
            self.bug_cursor = usize::MAX; // end of the focused field
        }
        if matches!(screen, Screen::TaskForm(_)) {
            if self.task_assignees.is_empty() && !self.config.current_user.is_empty() {
                self.task_assignees.push(self.config.current_user.clone());
            }
            self.task_cursor = usize::MAX;
        }
        // Default the bug form Environment to where the parent is being tested:
        // In QA - Dev → Dev, In QA → STG, In UAT → UAT.
        if let Screen::BugReport(Some(parent_idx), _) = &screen {
            if let Some(item) = self.items.get(*parent_idx) {
                self.bug_env_idx = match item.status {
                    Status::InQADev => 0,
                    Status::InQA => 1,
                    Status::InUAT => 2,
                    _ => self.bug_env_idx,
                };
            }
        }
        self.previous_screens.push(self.screen.clone());
        self.screen = screen;
    }

    /// The navigation trail, oldest first, ending at the current screen.
    ///
    /// Rendered as a breadcrumb so "where am I" is always answerable: the
    /// sidebar can only highlight top-level destinations, and it is hidden
    /// entirely below 80 columns, which left sub-screens with no location
    /// indicator at all.
    pub fn breadcrumb(&self) -> Vec<String> {
        let mut trail: Vec<String> = Vec::new();
        for screen in self
            .previous_screens
            .iter()
            .chain(std::iter::once(&self.screen))
        {
            // The report form files bugs and enhancements from the same
            // screen variant, so the kind lives here, not in Screen.
            let label = match screen {
                Screen::BugReport(_, false) => {
                    format!("New {}", self.bug_kind.label().to_lowercase())
                }
                other => other.label(&self.items),
            };
            // Collapse repeats (Esc/goto cycles can stack the same screen).
            if trail.last() != Some(&label) {
                trail.push(label);
            }
        }
        trail
    }

    /// Go back to the previous screen.
    pub fn go_back(&mut self) {
        if let Some(prev) = self.previous_screens.pop() {
            self.screen = prev;
        }
    }

    /// Get items in a specific status column.
    pub fn items_in_column(&self, status: &Status) -> Vec<(usize, &Item)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.status == *status
                    && (self.board_sprint_filter.is_none()
                        || item.sprint.as_deref() == self.board_sprint_filter.as_deref())
            })
            .collect()
    }

    pub fn my_items(&self) -> Vec<(usize, &Item)> {
        let mut items: Vec<(usize, &Item)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.assignees.iter().any(|a| a == &self.current_user))
            .collect();

        let current_sprint = self.board_sprint_filter.clone();

        items.sort_by(|(_, a), (_, b)| {
            let a_is_current = a.sprint == current_sprint;
            let b_is_current = b.sprint == current_sprint;

            // 1. Current sprint first
            if a_is_current != b_is_current {
                return b_is_current.cmp(&a_is_current); // true before false
            }

            // 2. Other sprint name grouping (descending by sprint name string)
            if a.sprint != b.sprint {
                return b.sprint.cmp(&a.sprint);
            }

            // 3. Priority
            a.priority_sort_key().cmp(&b.priority_sort_key())
        });

        items
    }

    /// Environments selectable in the Bug Report form (shared by UI + submit).
    pub const BUG_ENVS: [&str; 5] = ["Dev", "STG", "UAT", "Demo", "Prod"];

    /// Labels QA adds to hand a ticket over to the devs. Nothing on GitHub
    /// removes them, so every path that sends a ticket back for retest has to
    /// clear them - otherwise the ticket stays hidden from My Tasks and the
    /// "already passed" guard blocks the next pass forever.
    pub const HANDOFF_LABELS: [&str; 2] = ["Ready-for-Staging", "Ready-for-UAT"];

    /// Which handoff labels an item currently carries (canonical spelling).
    pub fn handoff_labels_on(item: &Item) -> Vec<&'static str> {
        Self::HANDOFF_LABELS
            .iter()
            .filter(|canon| item.labels.iter().any(|l| Self::label_eq(l, canon)))
            .copied()
            .collect()
    }

    /// QA statuses shown on My Tasks screen.
    pub const MY_TASKS_STATUSES: [Status; 4] = [
        Status::InQA,
        Status::InQADev,
        Status::InUAT,
        Status::Blocked,
    ];

    /// Compare two label names ignoring case and treating '-' and ' ' as equivalent.
    /// Repo label naming is inconsistent ("Ready-for-Staging" vs "Ready for Release"),
    /// so exact matching would silently miss some labels.
    pub fn label_eq(a: &str, b: &str) -> bool {
        let norm = |s: &str| -> String {
            s.chars()
                .map(|c| {
                    if c == '-' {
                        ' '
                    } else {
                        c.to_ascii_lowercase()
                    }
                })
                .collect()
        };
        norm(a) == norm(b)
    }

    /// Whether a handoff label hides an item in the given status.
    ///
    /// Nothing in the workflow ever REMOVES these labels, so they must only
    /// hide the item while it waits in the column QA passed it FROM - once
    /// devs deploy and move it onward, it has to reappear as testable work:
    /// - `Ready-for-Staging` → hides only in "In QA - Dev" (waiting for STG deploy)
    /// - `Ready-for-UAT`     → hides only in "In QA" (waiting for UAT deploy)
    /// - `Ready-for-Release` → hides everywhere (QA is fully done with it)
    pub fn label_excludes_in_status(label: &str, status: &Status) -> bool {
        (Self::label_eq(label, "Ready-for-Staging") && *status == Status::InQADev)
            || (Self::label_eq(label, "Ready-for-UAT") && *status == Status::InQA)
            || Self::label_eq(label, "Ready-for-Release")
    }

    /// Whether an item is excluded from My Tasks / "Doing today" by its
    /// handoff labels (status-aware, see `label_excludes_in_status`).
    pub fn item_excluded_from_my_tasks(item: &Item) -> bool {
        item.labels
            .iter()
            .any(|l| Self::label_excludes_in_status(l, &item.status))
    }

    /// Check if an item passes the My Tasks filter (status + sprint + excluded labels).
    fn passes_my_tasks_filter(&self, item: &Item, status: &Status) -> bool {
        item.status == *status
            && (self.board_sprint_filter.is_none()
                || item.sprint.as_deref() == self.board_sprint_filter.as_deref())
            && !Self::item_excluded_from_my_tasks(item)
    }

    /// Items QA already passed that now wait for a deploy (labelled
    /// Ready-for-Staging / Ready-for-UAT and still sitting in the column they
    /// were passed from). Hidden from My Tasks, but shown in their own
    /// "Waiting for deploy" section so they never get lost.
    pub fn waiting_for_deploy(&self) -> Vec<(usize, &Item)> {
        self.my_items()
            .into_iter()
            .filter(|(_, item)| {
                (self.board_sprint_filter.is_none()
                    || item.sprint.as_deref() == self.board_sprint_filter.as_deref())
                    && item.labels.iter().any(|l| {
                        (Self::label_eq(l, "Ready-for-Staging") && item.status == Status::InQADev)
                            || (Self::label_eq(l, "Ready-for-UAT") && item.status == Status::InQA)
                    })
            })
            .collect()
    }

    /// Get My Tasks items grouped by status in flat order (InQA → InQADev → InUAT → Blocked).
    /// Returns `(original_index, &Item)` pairs matching the visual order.
    pub fn my_tasks_grouped(&self) -> Vec<(usize, &Item)> {
        let my_items = self.my_items();
        Self::MY_TASKS_STATUSES
            .iter()
            .flat_map(|status| {
                my_items
                    .iter()
                    .filter(|(_, item)| self.passes_my_tasks_filter(item, status))
                    .copied()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Get My Tasks items grouped per status (for rendering with group headers).
    /// Returns `(status, items_in_status)` only for non-empty groups.
    pub fn my_tasks_by_status(&self) -> Vec<(&Status, Vec<(usize, &Item)>)> {
        let my_items = self.my_items();
        Self::MY_TASKS_STATUSES
            .iter()
            .filter_map(|status| {
                let items: Vec<_> = my_items
                    .iter()
                    .filter(|(_, item)| self.passes_my_tasks_filter(item, status))
                    .copied()
                    .collect();
                if items.is_empty() {
                    None
                } else {
                    Some((status, items))
                }
            })
            .collect()
    }

    /// Get selected items (for batch ops). Restricted to the current sprint
    /// filter so items selected earlier but now hidden by a filter change
    /// can't be silently included in a batch move.
    pub fn selected_items(&self) -> Vec<(usize, &Item)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.selected
                    && (self.board_sprint_filter.is_none()
                        || item.sprint.as_deref() == self.board_sprint_filter.as_deref())
            })
            .collect()
    }

    /// Find a project field by name (case-insensitive).
    ///
    /// Case-insensitive on purpose: an exact match meant a board field named
    /// "status" made every write fail with "project fields not loaded yet",
    /// which points the user at the wrong problem entirely.
    fn field_by_name(&self, name: &str) -> Option<&Field> {
        self.fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// Find the Status field and its option IDs.
    pub fn status_field(&self) -> Option<&Field> {
        self.field_by_name("Status")
    }

    pub fn priority_field(&self) -> Option<&Field> {
        self.field_by_name("Priority")
    }

    pub fn stack_field(&self) -> Option<&Field> {
        self.field_by_name("Stack")
    }

    pub fn sprint_field(&self) -> Option<&Field> {
        self.field_by_name("Sprint")
    }

    pub fn type_field(&self) -> Option<&Field> {
        self.field_by_name("type")
    }

    /// Warn when the board's Status column set no longer matches this build.
    ///
    /// `Status::from_str` folds anything unrecognised into `Backlog`, so a
    /// renamed or added column shows up as a silently wrong board, an inflated
    /// "Not started" count and items vanishing from My Tasks - exactly what
    /// happened to "In UAT" before v3.2.0. The project's real options are
    /// already in memory, so the mismatch is cheap to detect.
    pub fn status_drift_warning(&self) -> Option<String> {
        let field = self.status_field()?;
        if field.options.is_empty() {
            return None;
        }
        let unknown: Vec<&str> = field
            .options
            .iter()
            .filter(|o| Status::parse_known(&o.name).is_none())
            .map(|o| o.name.as_str())
            .collect();
        let missing: Vec<&str> = Status::all_columns()
            .iter()
            .filter(|s| field.find_option(s.label()).is_none())
            .map(|s| s.label())
            .collect();

        if unknown.is_empty() && missing.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if !unknown.is_empty() {
            parts.push(format!(
                "unsupported column(s) [{}] - items there are counted as Backlog",
                unknown.join(", ")
            ));
        }
        if !missing.is_empty() {
            parts.push(format!(
                "app expects column(s) [{}] that the board no longer has",
                missing.join(", ")
            ));
        }
        Some(format!("Board Status mismatch: {}", parts.join("; ")))
    }

    /// Iteration start date for a sprint name, when the Sprint field carries
    /// iteration dates (fetched from GitHub, not guessed).
    pub fn sprint_start_date(&self, name: &str) -> Option<NaiveDate> {
        self.sprint_field()
            .and_then(|f| f.find_option(name))
            .and_then(|o| o.start_date)
    }

    /// Today's date in the configured timezone.
    fn today_local(&self) -> NaiveDate {
        let offset = FixedOffset::east_opt(self.config.timezone_offset_hours * 3600)
            .unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());
        Utc::now().with_timezone(&offset).date_naive()
    }

    /// Get available sprints from items, in chronological order.
    ///
    /// Ordered by the Sprint field's real iteration start dates when every
    /// name resolves to one; otherwise by the digit-scraping fallback (which
    /// mangles titles like "Sprint 8 (Q1)" → 81). Order matters: the sprint
    /// filter derives previous/next by index.
    pub fn available_sprints(&self) -> Vec<String> {
        let mut sprints: Vec<String> = self
            .items
            .iter()
            .filter_map(|item| item.sprint.clone())
            .collect();
        // Dedup by value first - `dedup()` only drops CONSECUTIVE duplicates,
        // and sorting by a lossy numeric key can leave equal names apart.
        sprints.sort();
        sprints.dedup();

        let dates: Vec<Option<NaiveDate>> =
            sprints.iter().map(|s| self.sprint_start_date(s)).collect();
        if !sprints.is_empty() && dates.iter().all(|d| d.is_some()) {
            let mut pairs: Vec<(NaiveDate, String)> =
                dates.into_iter().map(|d| d.unwrap()).zip(sprints).collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            return pairs.into_iter().map(|(_, name)| name).collect();
        }
        sprints.sort_by_key(|a| sprint_sort_key(a));
        sprints
    }

    /// Get the current sprint name based on real-time date calculation.
    /// Uses anchor: Sprint 12 = Jan 19, 2026. Each sprint = 14 days.
    /// Falls back to heuristic (most non-Done items) if no date-based match.
    pub fn current_sprint_name(&self) -> Option<String> {
        let sprints = self.available_sprints();
        if sprints.is_empty() {
            return None;
        }

        // Authoritative: the iteration whose [start, start+duration) window
        // contains today, straight from the project's Sprint field. Only
        // accepted when that sprint actually has items, so the filter can
        // still resolve previous/next by index.
        let today = self.today_local();
        if let Some(field) = self.sprint_field() {
            if field.has_iteration_dates() {
                if let Some(opt) = field.iteration_for_date(today) {
                    if sprints.iter().any(|s| s == &opt.name) {
                        return Some(opt.name.clone());
                    }
                }
            }
        }

        // Fallback: derive the number from the hardcoded anchor. Kept for the
        // window before fields load (and for boards whose iterations carry no
        // dates), but it is a second source of truth - it drifts the moment
        // the team shifts a sprint.
        let tz_offset = self.config.timezone_offset_hours;
        let sprint_num = current_sprint_number(tz_offset);

        // Search available sprints for one containing this number
        if let Some(matched) = sprints
            .iter()
            .find(|s| sprint_sort_key(s) == sprint_num as i64)
        {
            return Some(matched.clone());
        }

        // Fallback: sprint with most non-Done items
        let mut best_sprint = &sprints[sprints.len() - 1];
        let mut best_count = 0;
        for s in &sprints {
            let active = self
                .items
                .iter()
                .filter(|i| {
                    i.sprint.as_deref() == Some(s.as_str())
                        && i.status != crate::models::status::Status::Done
                })
                .count();
            if active > best_count {
                best_count = active;
                best_sprint = s;
            }
        }
        Some(best_sprint.clone())
    }

    /// Get sprint filter options in order: [current, previous, next, all].
    /// Current sprint determined by real-time date calculation.
    pub fn sprint_filter_options(&self) -> Vec<Option<String>> {
        let sprints = self.available_sprints();
        if sprints.is_empty() {
            return vec![None]; // Only "All"
        }

        // Use date-based current sprint detection
        let current_name = self.current_sprint_name();
        let current_idx = current_name
            .as_ref()
            .and_then(|name| sprints.iter().position(|s| s == name))
            .unwrap_or(sprints.len() - 1);

        let mut options: Vec<Option<String>> = Vec::new();

        // Current sprint
        options.push(Some(sprints[current_idx].clone()));

        // Previous sprint (if exists)
        if current_idx > 0 {
            options.push(Some(sprints[current_idx - 1].clone()));
        }

        // Next sprint (if exists)
        if current_idx + 1 < sprints.len() {
            options.push(Some(sprints[current_idx + 1].clone()));
        }

        // All sprints
        options.push(None);

        options
    }

    /// Cycle to the next sprint filter option.
    pub fn cycle_sprint_filter(&mut self) {
        let options = self.sprint_filter_options();
        if options.is_empty() {
            return;
        }
        let current_pos = options.iter().position(|o| o == &self.board_sprint_filter);
        let next_pos = match current_pos {
            Some(i) => (i + 1) % options.len(),
            None => 0,
        };
        self.board_sprint_filter = options[next_pos].clone();
    }

    /// Set a temporary status message (shown by the global status bar).
    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
        self.status_message_at = Some(std::time::Instant::now());
    }

    /// Remove a label from an item's local state (revert of add_label_local).
    pub fn remove_label_local(&mut self, item_id: &str, label: &str) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == item_id) {
            item.labels.retain(|l| !Self::label_eq(l, label));
        }
    }

    /// Reset the bug report form to its defaults (called once creation succeeded).
    pub fn clear_bug_form(&mut self) {
        self.bug_title.clear();
        self.bug_description.clear();
        self.bug_precondition.clear();
        self.bug_test_data.clear();
        self.bug_steps.clear();
        self.bug_expected.clear();
        self.bug_actual.clear();
        self.bug_impact.clear();
        self.bug_evidence.clear();
        self.bug_security.clear();
        self.bug_field_idx = 0;
        self.bug_broken_idx = 0;
        self.bug_visible_idx = 0;
        self.bug_workaround_idx = 2;
        self.bug_impact_prio_idx = 0;
        self.bug_priority_idx = 2; // default P2
        self.bug_stack_idx = 1; // default BE
        self.bug_assignees.clear();
        self.bug_kind = ReportKind::Bug;
    }

    /// Optimistically add a label to an item's local state (mirrors what the
    /// background `gh issue edit --add-label` call is doing remotely) so
    /// My Tasks exclusion and QA-action guards react immediately instead of
    /// waiting up to a full poll interval.
    pub fn add_label_local(&mut self, item_idx: usize, label: &str) {
        if let Some(item) = self.items.get_mut(item_idx) {
            if !item.labels.iter().any(|l| Self::label_eq(l, label)) {
                item.labels.push(label.to_string());
            }
        }
    }

    /// Whether an item carries a label (hyphen/space/case-insensitive).
    pub fn item_has_label(&self, item_idx: usize, label: &str) -> bool {
        self.items
            .get(item_idx)
            .map(|item| item.labels.iter().any(|l| Self::label_eq(l, label)))
            .unwrap_or(false)
    }

    /// Get all unique labels from items.
    pub fn available_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self
            .items
            .iter()
            .flat_map(|item| item.labels.iter().cloned())
            .collect();
        labels.sort();
        labels.dedup();
        labels
    }

    /// Get all unique priorities from items.
    pub fn available_priorities(&self) -> Vec<String> {
        let mut prios: Vec<String> = self
            .items
            .iter()
            .filter_map(|item| item.priority.clone())
            .collect();
        prios.sort();
        prios.dedup();
        prios
    }

    /// Get all unique stacks from items.
    pub fn available_stacks(&self) -> Vec<String> {
        let mut stacks: Vec<String> = self
            .items
            .iter()
            .filter_map(|item| item.stack.clone())
            .collect();
        stacks.sort();
        stacks.dedup();
        stacks
    }

    /// Whether any search filter is active (keyword or any dimension).
    pub fn has_active_filters(&self) -> bool {
        !self.search_query.is_empty()
            || !self.filter_status.is_empty()
            || !self.filter_priority.is_empty()
            || !self.filter_stack.is_empty()
            || !self.filter_sprint.is_empty()
            || !self.filter_label.is_empty()
    }

    /// Recompute `search_results`, keeping the cursor on the item it was on.
    ///
    /// For background refreshes: re-filtering used to snap the selection back
    /// to the top of the list every poll interval, so browsing search results
    /// for longer than three minutes was impossible.
    pub fn reapply_filters_keep_cursor(&mut self) {
        let prev_id = self
            .search_results
            .get(self.list_selected)
            .and_then(|i| self.items.get(*i))
            .map(|it| it.id.clone());
        let prev_offset = self.list_selected.saturating_sub(self.list_scroll);

        self.apply_filters();

        if let Some(id) = prev_id {
            if let Some(pos) = self
                .search_results
                .iter()
                .position(|i| self.items.get(*i).map(|it| it.id == id).unwrap_or(false))
            {
                self.list_selected = pos;
                self.list_scroll = pos.saturating_sub(prev_offset);
            }
        }
    }

    /// Apply all active filters and update search_results (resets the cursor -
    /// use `reapply_filters_keep_cursor` for refreshes).
    pub fn apply_filters(&mut self) {
        self.search_results = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                // Keyword filter
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    let matches = item.title.to_lowercase().contains(&q)
                        || item
                            .number
                            .map(|n| n.to_string().contains(&q))
                            .unwrap_or(false)
                        || item.assignees.iter().any(|a| a.to_lowercase().contains(&q));
                    if !matches {
                        return false;
                    }
                }
                // Status filter
                if !self.filter_status.is_empty()
                    && !multi_filter_match(&self.filter_status, item.status.label())
                {
                    return false;
                }
                // Priority filter
                if !self.filter_priority.is_empty() {
                    let prio = item.priority.as_deref().unwrap_or("--");
                    if !multi_filter_match(&self.filter_priority, prio) {
                        return false;
                    }
                }
                // Stack filter
                if !self.filter_stack.is_empty() {
                    let stack = item.stack.as_deref().unwrap_or("--");
                    if !multi_filter_match(&self.filter_stack, stack) {
                        return false;
                    }
                }
                // Sprint filter
                if !self.filter_sprint.is_empty() {
                    let sprint = item.sprint.as_deref().unwrap_or("--");
                    if !multi_filter_match(&self.filter_sprint, sprint) {
                        return false;
                    }
                }
                // Label filter
                if !self.filter_label.is_empty()
                    && !label_filter_match(&self.filter_label, &item.labels)
                {
                    return false;
                }
                true
            })
            .map(|(idx, _)| idx)
            .collect();
        self.list_selected = 0;
        self.list_scroll = 0;
    }
}

/// Label filter match: excludes always veto, then any include must match.
///
/// The label dimension used to fold includes and excludes into one `any()`,
/// so an exclude was ignored whenever some include also matched, and it
/// compared label names byte-for-byte while the rest of the app treats
/// "Ready-for-UAT" and "Ready for UAT" as the same label.
fn label_filter_match(filters: &[String], labels: &[String]) -> bool {
    let has = |name: &str| labels.iter().any(|l| App::label_eq(l, name));

    for ex in filters.iter().filter(|f| f.starts_with('!')) {
        if has(&ex[1..]) {
            return false;
        }
    }
    let mut includes = filters.iter().filter(|f| !f.starts_with('!')).peekable();
    if includes.peek().is_none() {
        return true;
    }
    includes.any(|inc| has(inc))
}

/// Multi-filter match: item passes if ANY include matches OR NO exclude matches.
fn multi_filter_match(filters: &[String], value: &str) -> bool {
    let includes: Vec<_> = filters.iter().filter(|f| !f.starts_with('!')).collect();
    let excludes: Vec<_> = filters.iter().filter(|f| f.starts_with('!')).collect();

    // If any exclude matches → fail
    for ex in &excludes {
        if value == &ex[1..] {
            return false;
        }
    }
    // If there are includes, value must match at least one
    if !includes.is_empty() {
        return includes.iter().any(|inc| value == inc.as_str());
    }
    true
}

/// Extract numeric key from sprint name for sorting (e.g. "Sprint 14" → 14).
fn sprint_sort_key(s: &str) -> i64 {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<i64>()
        .unwrap_or(0)
}

/// Calculate the current sprint number based on real-time date.
/// Anchor: Sprint 12 starts Jan 19, 2026. Each sprint = 14 days.
fn current_sprint_number(tz_offset_hours: i32) -> i32 {
    let offset = FixedOffset::east_opt(tz_offset_hours * 3600)
        .unwrap_or(FixedOffset::east_opt(7 * 3600).unwrap());
    let now = Utc::now().with_timezone(&offset);
    let today = now.date_naive();
    sprint_number_for_date(today)
}

/// Calculate the sprint number for a given date.
/// Anchor: Sprint 12 starts Jan 19, 2026. Each sprint = 14 days.
fn sprint_number_for_date(date: NaiveDate) -> i32 {
    let anchor = NaiveDate::from_ymd_opt(2026, 1, 19).unwrap(); // Sprint 12 start
    let days_since = (date - anchor).num_days();
    // Floor division: plain `/` truncates toward zero, so any date in the two
    // weeks BEFORE the anchor reported the anchor's own sprint number.
    let sprint_offset = days_since.div_euclid(14);
    12 + sprint_offset as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::item::Item;
    use crate::models::status::Status;

    #[test]
    fn test_multi_filter_match() {
        let empty: Vec<String> = vec![];
        assert!(multi_filter_match(&empty, "Val"));

        // Includes
        let inc = vec!["Feat".to_string(), "Bug".to_string()];
        assert!(multi_filter_match(&inc, "Bug"));
        assert!(!multi_filter_match(&inc, "Docs"));

        // Excludes
        let exc = vec!["!Bug".to_string()];
        assert!(!multi_filter_match(&exc, "Bug"));
        assert!(multi_filter_match(&exc, "Feat"));

        // Mix
        let mix = vec!["Feat".to_string(), "!Bug".to_string()];
        assert!(!multi_filter_match(&mix, "Bug"));
        assert!(multi_filter_match(&mix, "Feat"));
        assert!(!multi_filter_match(&mix, "Docs")); // Need include match
    }

    #[test]
    fn test_sprint_sort_key() {
        assert_eq!(sprint_sort_key("Sprint 14"), 14);
        assert_eq!(sprint_sort_key("KUP - Sprint 42"), 42);
        assert_eq!(sprint_sort_key("No Numbers Here"), 0);
    }

    #[test]
    fn test_sprint_number_for_date() {
        use chrono::NaiveDate;
        // Sprint 12: Jan 19 - Feb 1
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 1, 19).unwrap()),
            12
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
            12
        );
        // Sprint 13: Feb 2 - Feb 15
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 2, 2).unwrap()),
            13
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 2, 15).unwrap()),
            13
        );
        // Sprint 14: Feb 16 - Mar 1
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 2, 16).unwrap()),
            14
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
            14
        );
        // Sprint 15: Mar 2 - Mar 15
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 2).unwrap()),
            15
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 15).unwrap()),
            15
        );
        // Sprint 16: Mar 16 - Mar 29 (current)
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 16).unwrap()),
            16
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 17).unwrap()),
            16
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 29).unwrap()),
            16
        );
        // Sprint 17: Mar 30 - Apr 12
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 3, 30).unwrap()),
            17
        );
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 4, 12).unwrap()),
            17
        );
        // Sprint 18: Apr 13 - Apr 26
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 4, 13).unwrap()),
            18
        );
        // Sprint 19: Apr 27 - May 10
        assert_eq!(
            sprint_number_for_date(NaiveDate::from_ymd_opt(2026, 4, 27).unwrap()),
            19
        );
    }

    #[test]
    fn test_apply_filters() {
        let mut app = App::new();
        let item1 = Item {
            id: "1".into(),
            title: "Fix login button".into(),
            status: Status::InProgress,
            priority: Some("P1".into()),
            assignees: vec!["alice".into()],
            labels: vec!["frontend".into(), "bug".into()],
            ..Default::default()
        };
        let item2 = Item {
            id: "2".into(),
            title: "Write documentation".into(),
            status: Status::Backlog,
            priority: Some("P3".into()),
            assignees: vec!["bob".into()],
            labels: vec!["docs".into()],
            ..Default::default()
        };
        app.items = vec![item1, item2];

        // Empty filter
        app.search_query = "".into();
        app.apply_filters();
        assert_eq!(app.search_results.len(), 2);

        // Keyword filter
        app.search_query = "login".into();
        app.apply_filters();
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.search_results[0], 0); // item1

        // Assignee keyword filter
        app.search_query = "bob".into();
        app.apply_filters();
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.search_results[0], 1); // item2

        // Status filter
        app.search_query = "".into();
        app.filter_status = vec!["In Progress".to_string()];
        app.apply_filters();
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.search_results[0], 0);

        // Clear status filter, Priority filter exclude
        app.filter_status.clear();
        app.filter_priority = vec!["!P3".to_string()];
        app.apply_filters();
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.search_results[0], 0);

        // Label filter
        app.filter_priority.clear();
        app.filter_label = vec!["bug".to_string()];
        app.apply_filters();
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.search_results[0], 0);

        // Combined filters
        app.filter_label = vec!["docs".to_string()]; // matches 1
        app.filter_status = vec!["In Progress".to_string()]; // matches 0
        app.apply_filters();
        assert_eq!(app.search_results.len(), 0); // No intersection
    }

    #[test]
    fn test_my_tasks_excludes_ready_labels() {
        let mut app = App::new();
        app.current_user = "qa-user".into();
        app.board_sprint_filter = Some("Sprint 16".into());

        // Item 1: InQADev, no excluded labels → should appear
        let item1 = Item {
            id: "1".into(),
            title: "Normal QA Dev task".into(),
            status: Status::InQADev,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec!["feature".into()],
            ..Default::default()
        };
        // Item 2: InQADev + Ready-for-Staging → should be excluded
        let item2 = Item {
            id: "2".into(),
            title: "Passed Dev, waiting STG deploy".into(),
            status: Status::InQADev,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec!["Ready-for-Staging".into()],
            ..Default::default()
        };
        // Item 3: InQA, no excluded labels → should appear
        let item3 = Item {
            id: "3".into(),
            title: "Testing on STG".into(),
            status: Status::InQA,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec![],
            ..Default::default()
        };
        // Item 4: InQADev + Ready-for-Release → should be excluded
        let item4 = Item {
            id: "4".into(),
            title: "Ready for release".into(),
            status: Status::InQADev,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec!["Ready-for-Release".into()],
            ..Default::default()
        };
        // Item 5: Blocked + Ready-for-Staging → NOT excluded: the handoff label
        // only hides items still waiting in the column QA passed them from
        // (In QA - Dev); a Blocked item is actionable again.
        let item5 = Item {
            id: "5".into(),
            title: "Blocked with staging label".into(),
            status: Status::Blocked,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec!["Ready-for-Staging".into()],
            ..Default::default()
        };
        // Item 6: Blocked, no excluded labels → should appear
        let item6 = Item {
            id: "6".into(),
            title: "Blocked task".into(),
            status: Status::Blocked,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec![],
            ..Default::default()
        };
        // Item 7: InUAT, no excluded labels → should appear (QA verifies on UAT)
        let item7 = Item {
            id: "7".into(),
            title: "Testing on UAT".into(),
            status: Status::InUAT,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec![],
            ..Default::default()
        };
        // Item 8: InQA + Ready-for-UAT → excluded (passed STG, waiting UAT deploy)
        let item8 = Item {
            id: "8".into(),
            title: "Passed STG, waiting UAT deploy".into(),
            status: Status::InQA,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec!["Ready-for-UAT".into()],
            ..Default::default()
        };
        // Item 9: InQA + "Ready for Release" (space-separated repo spelling) → excluded
        let item9 = Item {
            id: "9".into(),
            title: "Space-separated label spelling".into(),
            status: Status::InQA,
            assignees: vec!["qa-user".into()],
            sprint: Some("Sprint 16".into()),
            labels: vec!["Ready for Release".into()],
            ..Default::default()
        };

        app.items = vec![
            item1, item2, item3, item4, item5, item6, item7, item8, item9,
        ];
        let grouped = app.my_tasks_grouped();

        // Should have: item3 (InQA), item1 (InQADev), item7 (InUAT),
        //              item5 + item6 (Blocked) = 5
        // Excluded: item2 (Ready-for-Staging while InQADev),
        //           item4 (Ready-for-Release - excluded in any status),
        //           item8 (Ready-for-UAT while InQA),
        //           item9 (space-separated Ready for Release)
        assert_eq!(grouped.len(), 5, "Expected 5 items, got {}", grouped.len());

        let ids: Vec<&str> = grouped.iter().map(|(_, item)| item.id.as_str()).collect();
        assert!(ids.contains(&"1"), "Normal InQADev should be included");
        assert!(
            !ids.contains(&"2"),
            "Ready-for-Staging InQADev should be excluded"
        );
        assert!(ids.contains(&"3"), "Normal InQA should be included");
        assert!(
            !ids.contains(&"4"),
            "Ready-for-Release InQADev should be excluded"
        );
        assert!(
            ids.contains(&"5"),
            "Blocked with Ready-for-Staging should reappear (label only hides in In QA - Dev)"
        );
        assert!(ids.contains(&"6"), "Normal Blocked should be included");
        assert!(ids.contains(&"7"), "InUAT should be included");
        assert!(!ids.contains(&"8"), "Ready-for-UAT InQA should be excluded");
        assert!(
            !ids.contains(&"9"),
            "Space-separated 'Ready for Release' should be excluded"
        );
    }

    #[test]
    fn test_waiting_for_deploy() {
        let mut app = App::new();
        app.current_user = "qa".into();
        app.board_sprint_filter = None;
        app.items = vec![
            // Passed Dev, waiting for STG deploy → listed
            Item {
                id: "1".into(),
                status: Status::InQADev,
                labels: vec!["Ready-for-Staging".into()],
                assignees: vec!["qa".into()],
                ..Default::default()
            },
            // Passed STG, waiting for UAT deploy → listed
            Item {
                id: "2".into(),
                status: Status::InQA,
                labels: vec!["Ready-for-UAT".into()],
                assignees: vec!["qa".into()],
                ..Default::default()
            },
            // Stale label from an earlier phase - item already moved on → not listed
            Item {
                id: "3".into(),
                status: Status::InQA,
                labels: vec!["Ready-for-Staging".into()],
                assignees: vec!["qa".into()],
                ..Default::default()
            },
            // No handoff label → not listed
            Item {
                id: "4".into(),
                status: Status::InQADev,
                labels: vec![],
                assignees: vec!["qa".into()],
                ..Default::default()
            },
        ];
        let ids: Vec<&str> = app
            .waiting_for_deploy()
            .iter()
            .map(|(_, i)| i.id.as_str())
            .collect();
        assert_eq!(ids, vec!["1", "2"]);
    }

    #[test]
    fn test_label_filter_exclude_always_vetoes() {
        let labels = vec!["Bug".to_string(), "FE".to_string()];

        // Include only.
        assert!(label_filter_match(&["Bug".to_string()], &labels));
        assert!(!label_filter_match(&["BE".to_string()], &labels));

        // An exclude vetoes even when an include also matches. The old
        // `any()` over mixed filters let the include win instead.
        assert!(!label_filter_match(
            &["Bug".to_string(), "!FE".to_string()],
            &labels
        ));

        // Exclude only.
        assert!(!label_filter_match(&["!FE".to_string()], &labels));
        assert!(label_filter_match(&["!BE".to_string()], &labels));

        // Hyphen/space + case normalisation, like the rest of the app.
        assert!(!label_filter_match(
            &["!ready for uat".to_string()],
            &["Ready-for-UAT".to_string()]
        ));
    }

    #[test]
    fn test_report_kind_only_differs_in_title_and_label() {
        assert_eq!(ReportKind::Bug.label(), "Bug");
        assert_eq!(ReportKind::Bug.title_prefix(), "[BUG]");
        assert_eq!(ReportKind::Enhancement.label(), "Enhancement");
        assert_eq!(ReportKind::Enhancement.title_prefix(), "[ENHANCEMENT]");
        assert_eq!(ReportKind::Enhancement.form_title(), "Enhancement Request");
        assert_eq!(ReportKind::Enhancement.slack_action(), "Enhancement");
        // An enhancement is scheduled work too, so it also carries Task, and
        // GitHub's native issue type is Task (the repo has Bug/Feature/Task).
        assert_eq!(ReportKind::Bug.labels(), &["Bug"]);
        assert_eq!(ReportKind::Enhancement.labels(), &["Enhancement", "Task"]);
        assert_eq!(ReportKind::Bug.issue_type(), "Bug");
        assert_eq!(ReportKind::Enhancement.issue_type(), "Task");
        // The form defaults to Bug, and clearing it returns to Bug so the next
        // report cannot inherit the previous kind.
        assert_eq!(ReportKind::default(), ReportKind::Bug);
    }

    #[test]
    fn test_open_report_form_sets_the_kind() {
        let mut app = App::new();
        app.open_report_form(None, false, ReportKind::Enhancement);
        assert_eq!(app.bug_kind, ReportKind::Enhancement);
        assert_eq!(app.screen, Screen::BugReport(None, false));

        app.clear_bug_form();
        assert_eq!(app.bug_kind, ReportKind::Bug);
    }

    #[test]
    fn test_handoff_labels_on() {
        let item = Item {
            labels: vec!["Bug".into(), "ready for uat".into()],
            ..Default::default()
        };
        assert_eq!(App::handoff_labels_on(&item), vec!["Ready-for-UAT"]);
        assert!(App::handoff_labels_on(&Item::default()).is_empty());
    }

    #[test]
    fn test_label_eq_normalization() {
        assert!(App::label_eq("Ready-for-Release", "Ready for Release"));
        assert!(App::label_eq("Ready for UAT", "Ready-for-UAT"));
        assert!(App::label_eq("ready-FOR-staging", "Ready for Staging"));
        assert!(!App::label_eq("Ready-for-UAT", "Ready-for-Staging"));
    }
}
