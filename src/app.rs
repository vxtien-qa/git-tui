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
    /// Pick tickets for a batch action; rows live in `App::batch_rows`.
    BatchPicker(BatchKind),
}

/// What a batch pick list is choosing tickets for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BatchKind {
    /// Pass a UAT regression run: In UAT tickets to Tech Complete.
    VerifyUat,
    /// Code is on UAT: In QA tickets with Ready-for-UAT to In UAT.
    MoveToUat,
}

impl BatchKind {
    pub fn title(self) -> &'static str {
        match self {
            BatchKind::VerifyUat => "Bulk verify UAT",
            BatchKind::MoveToUat => "Move to In UAT",
        }
    }
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
            Screen::BatchPicker(kind) => kind.title().to_string(),
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
    /// A confirm with a sprint to pick (←/→) for the ticket being sent back.
    ConfirmSprint {
        title: String,
        message: String,
        /// Names the ticket the sprint applies to: on Fail it is the parent,
        /// not the bug being filed.
        sprint_label: String,
        choices: Vec<SprintChoice>,
        selected: usize,
        on_confirm: ConfirmAction,
    },
    /// Choose the format for copying the Search results.
    CopyResults,
    /// Name the current Search filters to save them.
    SavePreset {
        name: String,
    },
    /// Pick a saved Search filter (index into `config.search_presets`).
    Presets {
        selected: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    PassDev(usize),
    PassStg(usize),
    /// Move In QA tickets carrying Ready-for-UAT into In UAT, remove the
    /// label, comment on each and send one Slack message.
    MoveToUat {
        indices: Vec<usize>,
    },
    /// Move In UAT tickets to Tech Complete, comment on each and send one
    /// Slack message. `bulk` is a regression run picked in Bulk verify UAT;
    /// otherwise it is the one ticket `U` was pressed on.
    PassUat {
        indices: Vec<usize>,
        bulk: bool,
    },
    /// Back to In Progress; `sprint` moves the ticket to that sprint too.
    Return {
        idx: usize,
        sprint: Option<String>,
    },
    MoveItems {
        indices: Vec<usize>,
        target: Status,
    },
    /// File the report form: (parent, is_fail, sprint for the parent when
    /// failing it - `None` keeps the parent's sprint).
    SubmitBugReport(Option<usize>, bool, Option<String>),
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
    /// Delete a saved Search filter, by name.
    DeleteSearchPreset(String),
    Quit,
}

impl Popup {
    /// A popup waiting on the user - a confirm, a prompt, a pick list - as
    /// opposed to a result message. A background result must not replace
    /// one: the pending decision would vanish, and the Enter meant for it
    /// would dismiss the result instead.
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            Popup::Confirm { .. }
                | Popup::ConfirmSprint { .. }
                | Popup::SavePreset { .. }
                | Popup::Presets { .. }
                | Popup::CopyResults
        )
    }
}

/// One sprint option when sending a ticket back (Fail, Return).
#[derive(Debug, Clone, PartialEq)]
pub struct SprintChoice {
    /// `Keep (Sprint 30)`, `Today (Sprint 31)`, `Next (Sprint 32)`.
    pub label: String,
    /// The sprint to move to; `None` keeps the ticket where it is.
    pub sprint: Option<String>,
}

impl ConfirmAction {
    /// The action with the sprint picked in a `ConfirmSprint` popup.
    pub fn with_sprint(self, sprint: Option<String>) -> Self {
        match self {
            ConfirmAction::Return { idx, .. } => ConfirmAction::Return { idx, sprint },
            ConfirmAction::SubmitBugReport(parent, is_fail, _) => {
                ConfirmAction::SubmitBugReport(parent, is_fail, sprint)
            }
            other => other,
        }
    }
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
        /// `None` when there is nothing to look up (a pull request card);
        /// `Err` when the lookup failed, which keeps what was shown.
        linked_prs: Option<Result<Vec<crate::models::item::LinkedPr>, String>>,
    },
    /// Linked PRs read outside the detail fetch (the pick list's `d`).
    LinkedPrsFetched {
        item_id: String,
        result: Result<Vec<crate::models::item::LinkedPr>, String>,
    },
    /// A ticket's Sprint field was written on GitHub.
    SprintChanged {
        item_id: String,
        sprint: String,
    },
    /// Deploy check results for one ticket's merged PRs, by PR URL.
    DeployChecked {
        item_id: String,
        label: String,
        results: Vec<(String, Vec<(String, crate::gh::issue::DeployState)>)>,
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
    /// EPIC / root ticket, by `models::root::Root::filter_value`.
    pub filter_epic: Vec<String>,
    /// Assignee login, or [`App::NO_ASSIGNEE`].
    pub filter_assignee: Vec<String>,
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
    pub bug_priority_idx: usize,    // 0=--, 1=P0..5=P4
    pub bug_stack_idx: usize,       // 0=--, 1=FE, 2=BE, 3=App UI, 4=App BE
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
    /// Rows of the batch pick list: (item id, checked). By id, so a refresh
    /// reordering `items` can't check a different ticket.
    pub batch_rows: Vec<(String, bool)>,
    pub batch_cursor: usize,
    /// Items whose linked PRs have been looked up this session, so an empty
    /// list reads as "none linked" rather than "not loaded yet".
    pub linked_prs_loaded: std::collections::HashSet<String>,
    /// Items whose linked-PR lookup failed, with the error, until a reload
    /// succeeds.
    pub linked_prs_failed: std::collections::HashMap<String, String>,
    /// Deploy check results by PR URL: (environment, state) in
    /// `config.deploy_branches` order. Kept for the session, across syncs.
    pub deploy_checks:
        std::collections::HashMap<String, Vec<(String, crate::gh::issue::DeployState)>>,
    /// Item IDs with a deploy check running.
    pub deploy_check_inflight: std::collections::HashSet<String>,
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
            filter_epic: Vec::new(),
            filter_assignee: Vec::new(),
            filter_cursor: vec![0; App::SEARCH_FILTER_ROWS],
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
            // `--` by default: GitHub's priority automation scores an unset
            // Priority from the scoring answers, and the QA picks the Stack.
            bug_priority_idx: 0,
            bug_stack_idx: 0,
            bug_env_idx: 1, // default STG
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
            linked_prs_loaded: std::collections::HashSet::new(),
            linked_prs_failed: std::collections::HashMap::new(),
            batch_rows: Vec::new(),
            batch_cursor: 0,
            deploy_checks: std::collections::HashMap::new(),
            deploy_check_inflight: std::collections::HashSet::new(),
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

    /// Search filter rows below the keyword box: Status, Priority, Stack,
    /// Sprint, Label, EPIC, Assignee.
    pub const SEARCH_FILTER_ROWS: usize = 7;

    /// Open the batch pick list over `candidates` (item indices), checking
    /// the ones `checked` says.
    pub fn open_batch_picker(
        &mut self,
        kind: BatchKind,
        candidates: &[usize],
        from: usize,
        checked: impl Fn(usize) -> bool,
    ) {
        self.batch_rows = candidates
            .iter()
            .filter_map(|&i| self.items.get(i).map(|it| (it.id.clone(), checked(i))))
            .collect();
        // The cursor starts on the ticket the list was opened from.
        let from_id = self.items.get(from).map(|it| it.id.clone());
        self.batch_cursor = self
            .batch_rows
            .iter()
            .position(|(id, _)| Some(id) == from_id.as_ref())
            .unwrap_or(0);
        self.goto(Screen::BatchPicker(kind));
    }

    /// Item indices of the checked rows that are still on the board.
    pub fn batch_checked_indices(&self) -> Vec<usize> {
        self.batch_rows
            .iter()
            .filter(|(_, checked)| *checked)
            .filter_map(|(id, _)| self.items.iter().position(|i| &i.id == id))
            .collect()
    }

    /// Where a ticket sent back (Fail, Return) can go: keep its sprint, the
    /// sprint running today, or the one after. Choices naming the same sprint
    /// collapse; without iteration dates only Keep is offered.
    pub fn sprint_choices(&self, item_idx: usize) -> Vec<SprintChoice> {
        self.sprint_choices_on(item_idx, self.today_local())
    }

    /// `sprint_choices` for an explicit date (the testable seam).
    pub fn sprint_choices_on(&self, item_idx: usize, today: NaiveDate) -> Vec<SprintChoice> {
        let current = self.items.get(item_idx).and_then(|i| i.sprint.clone());
        let mut choices = vec![SprintChoice {
            label: format!("Keep ({})", current.as_deref().unwrap_or("no sprint")),
            sprint: None,
        }];
        let Some(field) = self.sprint_field().filter(|f| f.has_iteration_dates()) else {
            return choices;
        };
        let now = field.iteration_for_date(today);
        let after = now.and_then(|o| o.start_date).unwrap_or(today);
        let next = field
            .options
            .iter()
            .filter(|o| o.start_date.is_some_and(|d| d > after))
            .min_by_key(|o| o.start_date);
        for (label, opt) in [("Today", now), ("Next", next)] {
            let Some(opt) = opt else {
                continue;
            };
            let taken = current.as_deref() == Some(opt.name.as_str())
                || choices
                    .iter()
                    .any(|c| c.sprint.as_deref() == Some(opt.name.as_str()));
            if !taken {
                choices.push(SprintChoice {
                    label: format!("{} ({})", label, opt.name),
                    sprint: Some(opt.name.clone()),
                });
            }
        }
        choices
    }

    /// The UAT verdict of a ticket's deploy check, if one ran this session.
    ///
    /// ✓ only when every linked PR that still counts (open or merged; a PR
    /// closed without merging does not) is merged and its check says the UAT
    /// branch has it. An open PR, or a merged one not checked yet, makes it
    /// ? - part of the change may not be on UAT. Otherwise the worst result
    /// wins: ✗, then a failed check, then ? and not-found.
    pub fn uat_deploy_state(
        &self,
        item: &crate::models::item::Item,
    ) -> Option<crate::gh::issue::DeployState> {
        use crate::gh::issue::DeployState;
        let env_idx = self.config.deploy_branches.iter().position(|b| {
            b.env.eq_ignore_ascii_case("UAT") || b.branch.eq_ignore_ascii_case("uat")
        })?;
        let counted: Vec<&crate::models::item::LinkedPr> = item
            .linked_prs
            .iter()
            .filter(|pr| pr.is_merged() || pr.state.eq_ignore_ascii_case("OPEN"))
            .collect();
        let states: Vec<Option<&DeployState>> = counted
            .iter()
            .map(|pr| {
                self.deploy_checks
                    .get(&pr.url)
                    .and_then(|r| r.get(env_idx).map(|(_, s)| s))
            })
            .collect();
        if states.iter().all(|s| s.is_none()) {
            return None; // never checked
        }
        let is = |want: &DeployState| states.contains(&Some(want));
        Some(if is(&DeployState::Missing) {
            DeployState::Missing
        } else if let Some(Some(failed @ DeployState::Failed(_))) = states
            .iter()
            .find(|s| matches!(s, Some(DeployState::Failed(_))))
        {
            (*failed).clone()
        } else if states.iter().all(|s| *s == Some(&DeployState::Contained)) {
            DeployState::Contained
        } else if states.iter().all(|s| *s == Some(&DeployState::NotFound)) {
            // No UAT branch where any of the PRs live: the same - as Item Detail.
            DeployState::NotFound
        } else {
            DeployState::Unknown
        })
    }

    /// Candidates for Bulk verify UAT: every ticket In UAT in the active
    /// sprint, plus the ticket it was opened from, by number.
    ///
    /// A UAT run verifies everything deployed there at once, so passing it one
    /// ticket at a time meant a keypress and a Slack ping per ticket for a
    /// single regression result. The triggering ticket is always included,
    /// even when the sprint filter would otherwise hide it.
    pub fn verify_uat_candidates(&self, item_idx: usize) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .items_in_column(&Status::InUAT)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        if !indices.contains(&item_idx) {
            indices.push(item_idx);
        }
        indices.sort_by_key(|i| {
            self.items
                .get(*i)
                .and_then(|it| it.number)
                .unwrap_or(u32::MAX)
        });
        indices
    }

    /// Tickets Move to In UAT offers: In QA with the Ready-for-UAT label, in
    /// the active sprint, plus the ticket it was opened from, by number.
    pub fn move_to_uat_candidates(&self, item_idx: usize) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .items_in_column(&Status::InQA)
            .into_iter()
            .map(|(i, _)| i)
            .filter(|&i| self.item_has_label(i, "Ready-for-UAT"))
            .collect();
        if !indices.contains(&item_idx) {
            indices.push(item_idx);
        }
        indices.sort_by_key(|i| {
            self.items
                .get(*i)
                .and_then(|it| it.number)
                .unwrap_or(u32::MAX)
        });
        indices
    }

    /// One line on where a ticket's merged PRs are, for the status bar:
    /// `#4019 UAT: 2/2 in · Prod: 1/2 in, 1 ?`. Empty when nothing was checked.
    pub fn deploy_summary(&self, item: &crate::models::item::Item) -> String {
        use crate::gh::issue::DeployState;
        let checked: Vec<&Vec<(String, DeployState)>> = item
            .linked_prs
            .iter()
            .filter_map(|pr| self.deploy_checks.get(&pr.url))
            .collect();
        if checked.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        for (i, db) in self.config.deploy_branches.iter().enumerate() {
            let states: Vec<&DeployState> = checked
                .iter()
                .filter_map(|r| r.get(i).map(|(_, s)| s))
                .collect();
            let count = |want: fn(&DeployState) -> bool| states.iter().filter(|s| want(s)).count();
            let inn = count(|s| *s == DeployState::Contained);
            let unknown = count(|s| *s == DeployState::Unknown);
            let mut part = format!("{}: {}/{} in", db.env, inn, states.len());
            if unknown > 0 {
                part.push_str(&format!(", {} ?", unknown));
            }
            parts.push(part);
        }
        parts.join(" · ")
    }

    /// Assignee filter value for a ticket nobody is assigned to.
    pub const NO_ASSIGNEE: &'static str = "(none)";

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

    /// Every selectable My Tasks row, in screen order: the status groups,
    /// then the "Waiting for deploy" tickets. Those used to be display-only,
    /// so moving one on (Move to In UAT, clearing a stale label) meant
    /// finding it again on the Board or in Search.
    pub fn my_tasks_rows(&self) -> Vec<(usize, &Item)> {
        let mut rows = self.my_tasks_grouped();
        rows.extend(self.waiting_for_deploy());
        rows
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

    /// The sprint a newly filed bug/task belongs to: the iteration whose
    /// [start, start + duration) window contains today, straight from the
    /// project's Sprint field.
    ///
    /// Deliberately NOT `current_sprint_name`: that one only accepts a sprint
    /// that already has items, and only ever seeds `board_sprint_filter`,
    /// which the user is free to cycle to any sprint. Either path files a
    /// brand new bug into a sprint that closed weeks ago - on day one of a
    /// sprint every ticket still sits in the previous one, so the "sprint with
    /// the most open items" fallback answers with the old sprint. New items
    /// follow the calendar, nothing else.
    pub fn sprint_for_new_item(&self) -> Option<String> {
        self.sprint_for_new_item_on(self.today_local())
    }

    /// `sprint_for_new_item` for an explicit date (the testable seam).
    pub fn sprint_for_new_item_on(&self, today: NaiveDate) -> Option<String> {
        if let Some(opt) = self
            .sprint_field()
            .and_then(|field| field.iteration_for_date(today))
        {
            return Some(opt.name.clone());
        }
        // No iteration dates to read (fields not loaded yet, or a board whose
        // iterations carry none): the old heuristic beats no sprint at all.
        self.current_sprint_name()
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

    /// The confirm line about Slack: `sentence` when the message will go
    /// out, otherwise why it will not.
    pub fn slack_promise(&self, toggle_on: bool, sentence: &str) -> String {
        if !self.config.is_slack_configured() {
            "No Slack message: Slack is not set up.".to_string()
        } else if !toggle_on {
            "No Slack message: its toggle is off (Settings > n).".to_string()
        } else {
            sentence.to_string()
        }
    }

    /// Drop an "...ing..." progress note once its result popup is up, so the
    /// status bar stops saying "verifying..." under the outcome.
    pub fn clear_progress_status(&mut self) {
        if self
            .status_message
            .as_deref()
            .is_some_and(|m| m.ends_with("..."))
        {
            self.status_message = None;
            self.status_message_at = None;
        }
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
        self.bug_priority_idx = 0; // --
        self.bug_stack_idx = 0; // --
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

    /// The sprints a preset can name relatively: `@current`, `@previous`,
    /// `@next`, with today's names.
    pub fn relative_sprints(&self) -> Vec<(&'static str, String)> {
        let sprints = self.available_sprints();
        let Some(current) = self.current_sprint_name() else {
            return Vec::new();
        };
        let Some(idx) = sprints.iter().position(|s| *s == current) else {
            return Vec::new();
        };
        let mut out = vec![("@current", current)];
        if idx > 0 {
            out.push(("@previous", sprints[idx - 1].clone()));
        }
        if let Some(next) = sprints.get(idx + 1) {
            out.push(("@next", next.clone()));
        }
        out
    }

    /// The current Search as a preset named `name`.
    pub fn search_preset(&self, name: &str) -> crate::config::SearchPreset {
        self.search_preset_with(name, &self.relative_sprints())
    }

    /// `search_preset` against explicit relative sprints (the testable seam:
    /// which sprint is current depends on today's date).
    fn search_preset_with(
        &self,
        name: &str,
        relative: &[(&'static str, String)],
    ) -> crate::config::SearchPreset {
        let to_token = |value: &String| {
            let (bang, sprint) = match value.strip_prefix('!') {
                Some(rest) => ("!", rest),
                None => ("", value.as_str()),
            };
            match relative.iter().find(|(_, n)| n == sprint) {
                Some((token, _)) => format!("{}{}", bang, token),
                None => value.clone(),
            }
        };
        crate::config::SearchPreset {
            name: name.to_string(),
            query: self.search_query.clone(),
            status: self.filter_status.clone(),
            priority: self.filter_priority.clone(),
            stack: self.filter_stack.clone(),
            sprint: self.filter_sprint.iter().map(to_token).collect(),
            label: self.filter_label.clone(),
            epic: self.filter_epic.clone(),
            assignee: self.filter_assignee.clone(),
        }
    }

    /// Save the current Search under `name`, replacing a preset of that name.
    /// Only `config` in memory changes; the caller persists it.
    pub fn upsert_search_preset(&mut self, name: &str) {
        let preset = self.search_preset(name);
        self.upsert_preset(preset);
    }

    fn upsert_preset(&mut self, preset: crate::config::SearchPreset) {
        let name = preset.name.clone();
        match self
            .config
            .search_presets
            .iter_mut()
            .find(|p| p.name == name)
        {
            Some(existing) => *existing = preset,
            None => self.config.search_presets.push(preset),
        }
    }

    /// Load a preset into Search. Returns the sprint tokens it could not
    /// resolve (no iteration data, or no such sprint any more), which are
    /// left out rather than silently matching nothing.
    pub fn apply_search_preset(&mut self, preset: &crate::config::SearchPreset) -> Vec<String> {
        let relative = self.relative_sprints();
        self.apply_search_preset_with(preset, &relative)
    }

    /// `apply_search_preset` against explicit relative sprints.
    fn apply_search_preset_with(
        &mut self,
        preset: &crate::config::SearchPreset,
        relative: &[(&'static str, String)],
    ) -> Vec<String> {
        let mut dropped = Vec::new();
        let mut sprint = Vec::new();
        for value in &preset.sprint {
            let (bang, name) = match value.strip_prefix('!') {
                Some(rest) => ("!", rest),
                None => ("", value.as_str()),
            };
            if name.starts_with('@') {
                match relative.iter().find(|(token, _)| *token == name) {
                    Some((_, resolved)) => sprint.push(format!("{}{}", bang, resolved)),
                    None => dropped.push(value.clone()),
                }
            } else {
                sprint.push(value.clone());
            }
        }
        self.search_query = preset.query.clone();
        self.filter_status = preset.status.clone();
        self.filter_priority = preset.priority.clone();
        self.filter_stack = preset.stack.clone();
        self.filter_sprint = sprint;
        self.filter_label = preset.label.clone();
        self.filter_epic = preset.epic.clone();
        self.filter_assignee = preset.assignee.clone();
        self.filter_cursor = vec![0; Self::SEARCH_FILTER_ROWS];
        self.search_field_idx = 0;
        self.apply_filters();
        dropped
    }

    /// Every login assigned on the board, sorted, then [`App::NO_ASSIGNEE`].
    pub fn available_assignees(&self) -> Vec<String> {
        let mut logins: Vec<String> = self
            .items
            .iter()
            .flat_map(|item| item.assignees.iter().cloned())
            .collect();
        logins.sort_by_key(|l| l.to_lowercase());
        logins.dedup();
        logins.push(Self::NO_ASSIGNEE.to_string());
        logins
    }

    /// Options of one Search filter row as (filter value, label).
    ///
    /// `field_idx` counts the keyword box as 0, like `search_field_idx`. The
    /// value is what a filter stores and a saved filter keeps; the label is
    /// what the row shows. They differ only for the EPIC row, whose values
    /// carry the repository because issue numbers repeat across repositories.
    pub fn search_filter_options(&self, field_idx: usize) -> Vec<(String, String)> {
        let mut options = self.search_filter_options_on_board(field_idx);
        // A selected value the board no longer offers (an epic whose last
        // open ticket closed, a label nobody carries any more, a saved filter
        // from another sprint) stays listed, so it can still be seen and
        // switched off on its own.
        if (1..=Self::SEARCH_FILTER_ROWS).contains(&field_idx) {
            for value in self.search_filter(field_idx) {
                let bare = value.strip_prefix('!').unwrap_or(value);
                if !options.iter().any(|(v, _)| v == bare) {
                    let label = match bare.rsplit_once('#') {
                        Some((_, n)) if field_idx == 6 => format!("#{} (no open work)", n),
                        _ => bare.to_string(),
                    };
                    options.push((bare.to_string(), label));
                }
            }
        }
        options
    }

    /// `search_filter_options` from the board alone.
    fn search_filter_options_on_board(&self, field_idx: usize) -> Vec<(String, String)> {
        let same = |v: Vec<String>| v.into_iter().map(|o| (o.clone(), o)).collect();
        match field_idx {
            1 => same(
                Status::all_columns()
                    .iter()
                    .map(|s| s.label().to_string())
                    .collect(),
            ),
            2 => same(self.available_priorities()),
            3 => same(self.available_stacks()),
            4 => same(self.sprint_filter_options().into_iter().flatten().collect()),
            5 => same(self.available_labels()),
            6 => crate::models::root::root_filter_options(&self.items),
            7 => same(self.available_assignees()),
            _ => Vec::new(),
        }
    }

    /// The filter values of one Search row (`field_idx` as in
    /// `search_filter_options`).
    pub fn search_filter(&self, field_idx: usize) -> &Vec<String> {
        match field_idx {
            1 => &self.filter_status,
            2 => &self.filter_priority,
            3 => &self.filter_stack,
            4 => &self.filter_sprint,
            5 => &self.filter_label,
            6 => &self.filter_epic,
            _ => &self.filter_assignee,
        }
    }

    /// Mutable `search_filter`.
    pub fn search_filter_mut(&mut self, field_idx: usize) -> &mut Vec<String> {
        match field_idx {
            1 => &mut self.filter_status,
            2 => &mut self.filter_priority,
            3 => &mut self.filter_stack,
            4 => &mut self.filter_sprint,
            5 => &mut self.filter_label,
            6 => &mut self.filter_epic,
            _ => &mut self.filter_assignee,
        }
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
            || !self.filter_epic.is_empty()
            || !self.filter_assignee.is_empty()
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
        // Built only when the EPIC row is in use: it indexes the whole board.
        let roots = (!self.filter_epic.is_empty())
            .then(|| crate::models::root::RootIndex::new(&self.items));
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
                // EPIC / root filter
                if let Some(roots) = &roots {
                    let root = roots.resolve(item).filter_value();
                    if !multi_filter_match(&self.filter_epic, &root) {
                        return false;
                    }
                }
                // Assignee filter
                if !self.filter_assignee.is_empty()
                    && !assignee_filter_match(&self.filter_assignee, &item.assignees)
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

/// Assignee filter match, the same rules as labels: an exclude vetoes when any
/// assignee matches it, an include passes when any assignee matches it.
/// [`App::NO_ASSIGNEE`] stands for a ticket with no assignee at all.
fn assignee_filter_match(filters: &[String], assignees: &[String]) -> bool {
    let has = |login: &str| {
        if login == App::NO_ASSIGNEE {
            assignees.is_empty()
        } else {
            assignees.iter().any(|a| a.eq_ignore_ascii_case(login))
        }
    };
    if filters.iter().filter_map(|f| f.strip_prefix('!')).any(&has) {
        return false;
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

    fn relative() -> Vec<(&'static str, String)> {
        vec![
            ("@current", "Sprint 31".to_string()),
            ("@previous", "Sprint 30".to_string()),
            ("@next", "Sprint 32".to_string()),
        ]
    }

    #[test]
    fn test_search_preset_saves_sprints_relative_to_today() {
        let mut app = App::new();
        app.search_query = "copy".into();
        app.filter_status = vec!["In QA".into()];
        app.filter_sprint = vec!["Sprint 31".into(), "!Sprint 30".into(), "Sprint 12".into()];
        app.filter_label = vec!["!Ready-for-UAT".into()];
        app.filter_epic = vec!["acme/board#3111".into()];
        app.filter_assignee = vec!["vxtien-qa".into()];

        let preset = app.search_preset_with("In QA mine", &relative());
        assert_eq!(preset.name, "In QA mine");
        assert_eq!(preset.query, "copy");
        // Current and previous become tokens; an old sprint stays by name.
        assert_eq!(preset.sprint, vec!["@current", "!@previous", "Sprint 12"]);
        assert_eq!(preset.label, vec!["!Ready-for-UAT"]);
        assert_eq!(preset.epic, vec!["acme/board#3111"]);
        assert_eq!(preset.assignee, vec!["vxtien-qa"]);

        // Two weeks later the same preset means the new current sprint.
        let later = vec![
            ("@current", "Sprint 32".to_string()),
            ("@previous", "Sprint 31".to_string()),
        ];
        let mut fresh = App::new();
        let dropped = fresh.apply_search_preset_with(&preset, &later);
        assert!(dropped.is_empty());
        assert_eq!(
            fresh.filter_sprint,
            vec!["Sprint 32", "!Sprint 31", "Sprint 12"]
        );
        assert_eq!(fresh.search_query, "copy");
        assert_eq!(fresh.filter_assignee, vec!["vxtien-qa"]);
        assert_eq!(fresh.search_field_idx, 0);
        assert_eq!(fresh.filter_cursor.len(), App::SEARCH_FILTER_ROWS);
    }

    #[test]
    fn test_apply_search_preset_drops_sprints_it_cannot_resolve() {
        let preset = crate::config::SearchPreset {
            name: "next".into(),
            sprint: vec!["@next".into(), "Sprint 9".into()],
            ..Default::default()
        };
        let mut app = App::new();
        let dropped = app.apply_search_preset_with(&preset, &[("@current", "Sprint 31".into())]);
        assert_eq!(dropped, vec!["@next"]);
        assert_eq!(app.filter_sprint, vec!["Sprint 9"]);
    }

    #[test]
    fn test_upsert_preset_replaces_by_name() {
        let mut app = App::new();
        app.config.search_presets.clear();
        let mk = |name: &str, query: &str| crate::config::SearchPreset {
            name: name.into(),
            query: query.into(),
            ..Default::default()
        };
        app.upsert_preset(mk("a", "one"));
        app.upsert_preset(mk("b", "two"));
        app.upsert_preset(mk("a", "three"));
        let names: Vec<(&str, &str)> = app
            .config
            .search_presets
            .iter()
            .map(|p| (p.name.as_str(), p.query.as_str()))
            .collect();
        assert_eq!(names, vec![("a", "three"), ("b", "two")]);
    }

    #[test]
    fn test_deploy_summary_counts_per_environment() {
        use crate::gh::issue::DeployState::*;
        use crate::models::item::LinkedPr;
        let mut app = App::new();
        app.config.deploy_branches = crate::config::Config::default().deploy_branches;
        let pr = |n: u32| LinkedPr {
            repo: "org/api".into(),
            number: n,
            url: format!("u{}", n),
            state: "MERGED".into(),
            ..Default::default()
        };
        let item = Item {
            number: Some(4019),
            linked_prs: vec![pr(1), pr(2), pr(3)],
            ..Default::default()
        };
        assert_eq!(app.deploy_summary(&item), "");

        let env = |states: [crate::gh::issue::DeployState; 4]| {
            ["Dev", "STG", "UAT", "Prod"]
                .iter()
                .map(|e| e.to_string())
                .zip(states)
                .collect::<Vec<_>>()
        };
        app.deploy_checks.insert(
            "u1".into(),
            env([Contained, Contained, Contained, Contained]),
        );
        app.deploy_checks
            .insert("u2".into(), env([Contained, Unknown, Unknown, Missing]));
        // u3 not checked: left out of the counts.
        assert_eq!(
            app.deploy_summary(&item),
            "Dev: 2/2 in · STG: 1/2 in, 1 ? · UAT: 1/2 in, 1 ? · Prod: 1/2 in"
        );
    }

    #[test]
    fn test_uat_deploy_state_needs_every_pr_merged_and_checked() {
        use crate::gh::issue::DeployState::*;
        use crate::models::item::LinkedPr;
        let mut app = App::new();
        app.config.deploy_branches = crate::config::Config::default().deploy_branches;
        let pr = |u: &str, state: &str| LinkedPr {
            url: u.into(),
            state: state.into(),
            ..Default::default()
        };
        let env = |uat: crate::gh::issue::DeployState| {
            vec![
                ("Dev".to_string(), Contained),
                ("STG".to_string(), Contained),
                ("UAT".to_string(), uat),
                ("Prod".to_string(), Missing),
            ]
        };
        let mut item = Item {
            linked_prs: vec![pr("a", "MERGED"), pr("b", "MERGED")],
            ..Default::default()
        };
        assert_eq!(app.uat_deploy_state(&item), None, "never checked");

        app.deploy_checks.insert("a".into(), env(Contained));
        // b is merged but not checked: part of the change may be missing.
        assert_eq!(app.uat_deploy_state(&item), Some(Unknown));
        app.deploy_checks.insert("b".into(), env(Contained));
        assert_eq!(app.uat_deploy_state(&item), Some(Contained));

        // An open PR means the change is not all merged yet.
        item.linked_prs.push(pr("c", "OPEN"));
        assert_eq!(app.uat_deploy_state(&item), Some(Unknown));
        // A PR closed without merging does not count.
        item.linked_prs.pop();
        item.linked_prs.push(pr("d", "CLOSED"));
        assert_eq!(app.uat_deploy_state(&item), Some(Contained));

        app.deploy_checks
            .insert("b".into(), env(Failed("rate limit".into())));
        assert!(matches!(app.uat_deploy_state(&item), Some(Failed(_))));
        app.deploy_checks.insert("b".into(), env(Missing));
        assert_eq!(app.uat_deploy_state(&item), Some(Missing));

        // No UAT branch anywhere: - as in Item Detail, not ?.
        app.deploy_checks.insert("a".into(), env(NotFound));
        app.deploy_checks.insert("b".into(), env(NotFound));
        assert_eq!(app.uat_deploy_state(&item), Some(NotFound));
    }

    #[test]
    fn test_waiting_for_deploy_rows_are_selectable_and_take_i() {
        let mut app = App::new();
        app.current_user = "qa".into();
        app.config.current_user = "qa".into();
        app.board_sprint_filter = Some("Sprint 31".into());
        let mk = |id: &str, n: u32, status: Status, labels: &[&str]| Item {
            id: id.into(),
            number: Some(n),
            repository: Some("acme/board".into()),
            status,
            sprint: Some("Sprint 31".into()),
            assignees: vec!["qa".into()],
            labels: labels.iter().map(|l| l.to_string()).collect(),
            ..Default::default()
        };
        app.items = vec![
            mk("a", 1, Status::InQA, &[]),                // to test
            mk("b", 2, Status::InQA, &["Ready-for-UAT"]), // waiting for UAT deploy
        ];
        let rows: Vec<usize> = app.my_tasks_rows().iter().map(|(i, _)| *i).collect();
        assert_eq!(rows, vec![0, 1]);

        app.screen = Screen::MyTasks;
        crate::handlers::board::handle_my_tasks(&mut app, crossterm::event::KeyCode::Down);
        assert_eq!(app.list_selected, 1);
        crate::handlers::board::handle_my_tasks(&mut app, crossterm::event::KeyCode::Char('I'));
        assert_eq!(app.screen, Screen::BatchPicker(BatchKind::MoveToUat));
        assert_eq!(app.batch_checked_indices(), vec![1]);
    }

    #[test]
    fn test_menu_search_keeps_a_loaded_saved_filter_as_it_is() {
        let mut app = App::new();
        // Fresh Search: the usual open-work default.
        crate::handlers::screens::activate_menu_item(&mut app, 2);
        assert_eq!(app.filter_status.len(), 3);

        // A saved filter with no Status on purpose is not topped up.
        let mut app = App::new();
        app.filter_label = vec!["Ready-for-UAT".into()];
        crate::handlers::screens::activate_menu_item(&mut app, 2);
        assert!(app.filter_status.is_empty());
    }

    #[test]
    fn test_selected_filter_values_stay_listed_after_they_leave_the_board() {
        let mut app = App::new();
        app.items = vec![Item {
            id: "1".into(),
            number: Some(1),
            repository: Some("acme/board".into()),
            assignees: vec!["dev-a".into()],
            ..Default::default()
        }];
        app.filter_epic = vec!["!acme/board#3111".into()];
        app.filter_assignee = vec!["gone-user".into()];
        let epic = app.search_filter_options(6);
        assert!(epic
            .iter()
            .any(|(v, l)| v == "acme/board#3111" && l == "#3111 (no open work)"));
        let who = app.search_filter_options(7);
        assert!(who.iter().any(|(v, _)| v == "gone-user"));
        assert!(who.iter().any(|(v, _)| v == "dev-a"));
    }

    #[test]
    fn test_only_pending_decisions_are_interactive() {
        assert!(Popup::CopyResults.is_interactive());
        assert!(Popup::Presets { selected: 0 }.is_interactive());
        assert!(Popup::SavePreset {
            name: String::new()
        }
        .is_interactive());
        assert!(Popup::ConfirmSprint {
            title: String::new(),
            message: String::new(),
            sprint_label: String::new(),
            choices: Vec::new(),
            selected: 0,
            on_confirm: ConfirmAction::Quit,
        }
        .is_interactive());
        assert!(!Popup::Success("done".into()).is_interactive());
        assert!(!Popup::Error("x".into()).is_interactive());
        assert!(!Popup::None.is_interactive());
    }

    #[test]
    fn test_paste_goes_to_the_name_prompt_not_the_search_behind_it() {
        let mut app = App::new();
        app.screen = Screen::Search;
        app.popup = Popup::SavePreset {
            name: "In QA ".into(),
        };
        crate::handlers::screens::handle_paste(&mut app, "mine\n");
        assert_eq!(
            app.popup,
            Popup::SavePreset {
                name: "In QA mine".into()
            }
        );
        assert!(app.search_query.is_empty());

        // Any other popup swallows the paste.
        app.popup = Popup::CopyResults;
        crate::handlers::screens::handle_paste(&mut app, "text");
        assert!(app.search_query.is_empty());

        // No popup: the keyword box gets it, as before.
        app.popup = Popup::None;
        crate::handlers::screens::handle_paste(&mut app, "copy");
        assert_eq!(app.search_query, "copy");
    }

    #[test]
    fn test_assignee_filter_match() {
        let both = vec!["vxtien-qa".to_string(), "dev-a".to_string()];
        let none: Vec<String> = Vec::new();
        assert!(assignee_filter_match(&["vxtien-qa".into()], &both));
        assert!(assignee_filter_match(&["VXTIEN-QA".into()], &both));
        assert!(!assignee_filter_match(&["mmaguad".into()], &both));
        // An exclude vetoes even when an include matches.
        assert!(!assignee_filter_match(
            &["dev-a".into(), "!vxtien-qa".into()],
            &both
        ));
        assert!(assignee_filter_match(&["!mmaguad".into()], &both));
        // (none) matches only an unassigned ticket.
        assert!(assignee_filter_match(&[App::NO_ASSIGNEE.into()], &none));
        assert!(!assignee_filter_match(&[App::NO_ASSIGNEE.into()], &both));
        assert!(!assignee_filter_match(
            &[format!("!{}", App::NO_ASSIGNEE)],
            &none
        ));
    }

    #[test]
    fn test_epic_and_assignee_filters_narrow_search() {
        let mut app = App::new();
        let repo = Some("acme/board".to_string());
        let mk = |n: u32, title: &str, parent: Option<u32>, who: &[&str]| Item {
            id: n.to_string(),
            number: Some(n),
            title: title.into(),
            repository: repo.clone(),
            parent_number: parent,
            assignees: who.iter().map(|w| w.to_string()).collect(),
            status: Status::InQA,
            ..Default::default()
        };
        app.items = vec![
            mk(3111, "[Epic] Dave project", None, &[]),
            mk(1, "Under Dave", Some(3111), &["vxtien-qa"]),
            mk(2, "Under Dave too", Some(3111), &["mmaguad"]),
            mk(3, "Elsewhere", Some(4101), &["vxtien-qa"]),
            mk(4, "No parent", None, &[]),
        ];

        app.filter_epic = vec!["acme/board#3111".into()];
        app.apply_filters();
        assert_eq!(app.search_results, vec![1, 2]);

        app.filter_assignee = vec!["vxtien-qa".into()];
        app.apply_filters();
        assert_eq!(app.search_results, vec![1]);

        // Exclude the epic: what's left that is mine.
        app.filter_epic = vec!["!acme/board#3111".into()];
        app.apply_filters();
        assert_eq!(app.search_results, vec![3]);

        app.filter_epic = vec![crate::models::root::NO_PARENT.into()];
        app.filter_assignee = vec![App::NO_ASSIGNEE.into()];
        app.apply_filters();
        assert_eq!(app.search_results, vec![0, 4]);
        assert!(app.has_active_filters());
    }

    #[test]
    fn test_search_filter_options_line_up_with_rows() {
        let app = App::new();
        // Every row has a backing filter, and the keyword box (0) has no options.
        assert!(app.search_filter_options(0).is_empty());
        for field in 1..=App::SEARCH_FILTER_ROWS {
            let _ = app.search_filter(field);
        }
        // Assignee always offers (none), EPIC always offers No parent.
        assert_eq!(
            app.search_filter_options(7).last().map(|(v, _)| v.as_str()),
            Some(App::NO_ASSIGNEE)
        );
        assert_eq!(
            app.search_filter_options(6).last().map(|(v, _)| v.as_str()),
            Some(crate::models::root::NO_PARENT)
        );
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
    fn test_bug_form_defaults_leave_priority_and_stack_unset() {
        // Index 0 is `--` in both selectors (ui::bug_report PRIORITIES and
        // STACKS). The old defaults 2 and 1 were written as "P2" and "BE"
        // before `--` was added in front, and silently became P1 and FE.
        let mut app = App::new();
        assert_eq!(app.bug_priority_idx, 0);
        assert_eq!(app.bug_stack_idx, 0);

        app.bug_priority_idx = 3;
        app.bug_stack_idx = 2;
        app.clear_bug_form();
        assert_eq!(app.bug_priority_idx, 0);
        assert_eq!(app.bug_stack_idx, 0);
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

    // ── Sprint for newly filed items ──────────────────────────────

    /// A Sprint iteration field with real start dates, 14-day windows.
    fn sprint_field_fixture() -> crate::models::field::Field {
        use crate::models::field::{Field, FieldOption, FieldType};
        let mut options = Vec::new();
        // Sprint 12 starts Jan 19, 2026 - the project's real anchor.
        let anchor = NaiveDate::from_ymd_opt(2026, 1, 19).unwrap();
        for n in 12..=30 {
            options.push(FieldOption {
                id: format!("opt{}", n),
                name: format!("Sprint {}", n),
                start_date: Some(anchor + chrono::Duration::days((n - 12) as i64 * 14)),
                duration_days: Some(14),
            });
        }
        Field {
            id: "sprint_field".into(),
            name: "Sprint".into(),
            field_type: FieldType::Iteration,
            options,
        }
    }

    fn app_with_sprint_field() -> App {
        let mut app = App::new();
        app.fields = vec![sprint_field_fixture()];
        app
    }

    #[test]
    fn test_sprint_choices_keep_today_next() {
        let mut app = app_with_sprint_field();
        app.items = vec![
            Item {
                sprint: Some("Sprint 28".into()),
                ..Default::default()
            },
            Item {
                sprint: Some("Sprint 29".into()),
                ..Default::default()
            },
            Item::default(),
        ];
        // Sep 16, 2026 sits in Sprint 29 (Sep 14 - Sep 27).
        let day = NaiveDate::from_ymd_opt(2026, 9, 16).unwrap();
        let labels = |idx: usize| -> Vec<(String, Option<String>)> {
            app.sprint_choices_on(idx, day)
                .into_iter()
                .map(|c| (c.label, c.sprint))
                .collect()
        };
        // A carried-over ticket: keep, today's sprint, or the next one.
        assert_eq!(
            labels(0),
            vec![
                ("Keep (Sprint 28)".to_string(), None),
                (
                    "Today (Sprint 29)".to_string(),
                    Some("Sprint 29".to_string())
                ),
                (
                    "Next (Sprint 30)".to_string(),
                    Some("Sprint 30".to_string())
                ),
            ]
        );
        // Already in today's sprint: Today collapses into Keep.
        assert_eq!(
            labels(1),
            vec![
                ("Keep (Sprint 29)".to_string(), None),
                (
                    "Next (Sprint 30)".to_string(),
                    Some("Sprint 30".to_string())
                ),
            ]
        );
        assert_eq!(labels(2)[0].0, "Keep (no sprint)");

        // No iteration dates: only Keep.
        let mut bare = App::new();
        bare.items = app.items.clone();
        assert_eq!(bare.sprint_choices_on(0, day).len(), 1);
    }

    #[test]
    fn test_with_sprint_carries_the_pick_into_the_action() {
        let a = ConfirmAction::Return {
            idx: 3,
            sprint: None,
        }
        .with_sprint(Some("Sprint 31".into()));
        assert_eq!(
            a,
            ConfirmAction::Return {
                idx: 3,
                sprint: Some("Sprint 31".into())
            }
        );
        let b = ConfirmAction::SubmitBugReport(Some(2), true, None).with_sprint(None);
        assert_eq!(b, ConfirmAction::SubmitBugReport(Some(2), true, None));
        // Actions without a sprint are left alone.
        assert_eq!(
            ConfirmAction::Quit.with_sprint(Some("x".into())),
            ConfirmAction::Quit
        );
    }

    #[test]
    fn test_sprint_for_new_item_is_the_iteration_containing_today() {
        let mut app = app_with_sprint_field();
        app.items = vec![Item {
            sprint: Some("Sprint 28".into()),
            ..Default::default()
        }];

        // Sprint 29: Sep 14 - Sep 27, 2026.
        assert_eq!(
            app.sprint_for_new_item_on(NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()),
            Some("Sprint 29".to_string())
        );
        // And it rolls over on its own once Sprint 30 starts (Sep 28).
        assert_eq!(
            app.sprint_for_new_item_on(NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()),
            Some("Sprint 30".to_string())
        );
    }

    #[test]
    fn test_sprint_for_new_item_ignores_a_stale_board_filter() {
        let mut app = app_with_sprint_field();
        app.items = vec![Item {
            sprint: Some("Sprint 28".into()),
            ..Default::default()
        }];
        // The user cycled the board back to the previous sprint - a view
        // filter must not decide where a brand new bug is filed.
        app.board_sprint_filter = Some("Sprint 28".to_string());

        assert_eq!(
            app.sprint_for_new_item_on(NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()),
            Some("Sprint 29".to_string())
        );
    }

    #[test]
    fn test_sprint_for_new_item_does_not_need_the_sprint_to_have_items_yet() {
        let mut app = app_with_sprint_field();
        // Day one of a new sprint: every existing ticket still sits in the old
        // one. current_sprint_name() gates on sprints that have items, so it
        // answers "Sprint 28" here - the new-item sprint must not.
        app.items = vec![Item {
            sprint: Some("Sprint 28".into()),
            ..Default::default()
        }];

        assert_eq!(
            app.sprint_for_new_item_on(NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()),
            Some("Sprint 29".to_string())
        );
    }

    #[test]
    fn test_sprint_for_new_item_falls_back_when_the_field_carries_no_dates() {
        use crate::models::field::{Field, FieldOption, FieldType};
        let mut app = App::new();
        app.fields = vec![Field {
            id: "sprint_field".into(),
            name: "Sprint".into(),
            field_type: FieldType::Iteration,
            options: vec![FieldOption {
                id: "o28".into(),
                name: "Sprint 28".into(),
                start_date: None,
                duration_days: None,
            }],
        }];
        app.items = vec![Item {
            sprint: Some("Sprint 28".into()),
            ..Default::default()
        }];

        // No dates to read - fall back to the existing heuristic rather than
        // filing the item with no sprint at all.
        assert_eq!(
            app.sprint_for_new_item_on(NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()),
            app.current_sprint_name()
        );
    }
}
