use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Per-action Slack notification toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackNotifyConfig {
    /// Notify when ticket passes Dev QA
    #[serde(default = "default_true")]
    pub pass_dev: bool,
    /// Notify when ticket passes STG QA
    #[serde(default = "default_true")]
    pub pass_stg: bool,
    /// Notify when ticket passes UAT
    #[serde(default = "default_true")]
    pub pass_uat: bool,
    /// Notify when ticket is returned (failed QA)
    #[serde(default = "default_true")]
    pub return_fail: bool,
    /// Notify when a bug report is created
    #[serde(default = "default_true")]
    pub bug_report: bool,
    /// Notify when an enhancement request is created
    #[serde(default = "default_true")]
    pub enhancement: bool,
    /// Notify when a comment is posted
    #[serde(default = "default_true")]
    pub comment: bool,
    /// Notify when items are moved
    #[serde(default = "default_true")]
    pub move_items: bool,
    /// Notify when daily standup is sent
    #[serde(default = "default_true")]
    pub daily_standup: bool,
    /// Notify when sprint report is sent
    #[serde(default = "default_true")]
    pub sprint_report: bool,
    /// Notify when a task is created
    #[serde(default = "default_true")]
    pub new_task: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SlackNotifyConfig {
    fn default() -> Self {
        Self {
            pass_dev: true,
            pass_stg: true,
            pass_uat: true,
            return_fail: true,
            bug_report: true,
            enhancement: true,
            comment: true,
            move_items: true,
            daily_standup: true,
            sprint_report: true,
            new_task: true,
        }
    }
}

impl SlackNotifyConfig {
    /// Labels for display in the config screen (ordered).
    pub const LABELS: [&str; 11] = [
        "Pass Dev",
        "Pass STG",
        "Pass UAT",
        "Return (Fail)",
        "Bug Report",
        "Enhancement",
        "Comment",
        "Move Items",
        "Daily Standup",
        "Sprint Report",
        "New Task",
    ];

    /// Get toggle value by index.
    pub fn get(&self, idx: usize) -> bool {
        match idx {
            0 => self.pass_dev,
            1 => self.pass_stg,
            2 => self.pass_uat,
            3 => self.return_fail,
            4 => self.bug_report,
            5 => self.enhancement,
            6 => self.comment,
            7 => self.move_items,
            8 => self.daily_standup,
            9 => self.sprint_report,
            10 => self.new_task,
            _ => false,
        }
    }

    /// Toggle value by index.
    pub fn toggle(&mut self, idx: usize) {
        match idx {
            0 => self.pass_dev = !self.pass_dev,
            1 => self.pass_stg = !self.pass_stg,
            2 => self.pass_uat = !self.pass_uat,
            3 => self.return_fail = !self.return_fail,
            4 => self.bug_report = !self.bug_report,
            5 => self.enhancement = !self.enhancement,
            6 => self.comment = !self.comment,
            7 => self.move_items = !self.move_items,
            8 => self.daily_standup = !self.daily_standup,
            9 => self.sprint_report = !self.sprint_report,
            10 => self.new_task = !self.new_task,
            _ => {}
        }
    }
}

/// A mapping entry from a GitHub username to a Slack user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUserMapping {
    /// Slack member ID (e.g. "U04ABC123")
    pub slack_id: String,
    /// Display label for the TUI (e.g. "Alex Lee")
    pub slack_display: String,
}

/// User configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// GitHub org/user that owns the project
    pub owner: String,
    /// Project number (e.g. 9)
    pub project_number: u32,
    /// Current logged-in user
    pub current_user: String,
    /// Bug report repo
    pub bug_repo: String,
    /// Notification poll interval in seconds
    pub poll_interval_secs: u64,
    /// Timezone offset in hours from UTC (e.g. 7 for Vietnam, 8 for Perth, 11 for Sydney)
    #[serde(default = "default_timezone")]
    pub timezone_offset_hours: i32,
    /// Slack Incoming Webhook URL
    #[serde(default)]
    pub slack_webhook_url: Option<String>,
    /// Whether Slack notifications are enabled
    #[serde(default)]
    pub slack_enabled: bool,
    /// Whether the Slack setup screen has been offered (skip = don't auto-show again)
    #[serde(default)]
    pub slack_setup_offered: bool,
    /// Per-action Slack notification toggles
    #[serde(default)]
    pub slack_notify: SlackNotifyConfig,
    /// Slack Bot Token for fetching workspace members (xoxb-...)
    #[serde(default)]
    pub slack_bot_token: Option<String>,
    /// GitHub username → Slack user mapping
    #[serde(default)]
    pub slack_user_map: HashMap<String, SlackUserMapping>,
    /// Stack → Lead usernames mapping (e.g. "FE" → ["fe-lead"])
    #[serde(default = "default_stack_leads")]
    pub stack_leads: HashMap<String, Vec<String>>,
}

fn default_timezone() -> i32 {
    7
}

/// Stack → leads starts EMPTY.
///
/// It used to ship one team's real GitHub handles, so a fresh install at any
/// other org would @-mention three strangers in every Pass Dev/STG comment.
/// Existing configs keep whatever they already persisted; new ones are filled
/// in via Settings → [l].
fn default_stack_leads() -> HashMap<String, Vec<String>> {
    HashMap::new()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            owner: String::new(),
            project_number: 0,
            current_user: String::new(),
            bug_repo: String::new(),
            poll_interval_secs: 180,  // 3 min
            timezone_offset_hours: 7, // UTC+7 Vietnam
            slack_webhook_url: None,
            slack_enabled: false,
            slack_setup_offered: false,
            slack_notify: SlackNotifyConfig::default(),
            slack_bot_token: None,
            slack_user_map: HashMap::new(),
            stack_leads: default_stack_leads(),
        }
    }
}

impl Config {
    /// Config file path: ~/.config/git-tui/config.json (macOS/Linux)
    /// or %APPDATA%\git-tui\config.json (Windows)
    pub fn path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("git-tui").join("config.json")
    }

    /// Load config from disk, or return default.
    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(cfg) => cfg,
                    Err(_) => {
                        // A corrupt config used to be silently replaced by
                        // defaults - and the next auto-save would PERSIST the
                        // wipe (owner, webhook, token, user map, leads all
                        // gone). Preserve the broken file for manual recovery.
                        let backup = path.with_extension("json.corrupt");
                        let _ = fs::copy(&path, &backup);
                        Self::default()
                    }
                },
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Save config to disk (write-to-temp + rename - a crash mid-write must
    /// not produce the corrupt file that `load` then resets to defaults).
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        // `fs::rename` replaces the destination on both Unix and Windows, so
        // no unlink first - deleting the old file opened a window where a
        // crash left NO config at all.
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Check if the config has been properly set up (owner + project_number are required).
    pub fn is_configured(&self) -> bool {
        !self.owner.is_empty() && self.project_number > 0
    }

    /// Check if Slack integration is fully configured and enabled.
    pub fn is_slack_configured(&self) -> bool {
        self.slack_enabled && self.slack_webhook_url.is_some()
    }

    /// Check if Slack Bot Token is configured (for fetching workspace members).
    pub fn is_slack_bot_configured(&self) -> bool {
        self.slack_bot_token
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false)
    }

    /// Get lead usernames for a stack - only the leads of THAT stack.
    ///
    /// There is deliberately no "all leads" fallback: an item with no Stack (or
    /// a Stack nobody has mapped) used to tag every lead of every stack on the
    /// GitHub comment. Callers tag the assignees instead when this is empty.
    /// Stack names are matched case-insensitively so "App UI" / "app ui" both
    /// resolve.
    pub fn get_leads(&self, stack: Option<&str>) -> Vec<String> {
        let Some(stack_name) = stack else {
            return Vec::new();
        };
        self.stack_leads
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(stack_name))
            .map(|(_, leads)| leads.clone())
            .unwrap_or_default()
    }

    /// Format leads as "@user1 @user2" string for GitHub comments.
    /// Empty when the stack has no mapped lead - see `get_leads`.
    pub fn leads_mention_string(&self, stack: Option<&str>) -> String {
        self.get_leads(stack)
            .iter()
            .map(|u| format!("@{}", u))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_leads() -> Config {
        let mut c = Config::default();
        c.stack_leads.insert("FE".into(), vec!["fe-lead".into()]);
        c.stack_leads.insert("BE".into(), vec!["be-lead".into()]);
        c
    }

    #[test]
    fn test_get_leads_exact_stack() {
        let c = cfg_with_leads();
        assert_eq!(c.get_leads(Some("FE")), vec!["fe-lead"]);
        assert_eq!(c.get_leads(Some("fe")), vec!["fe-lead"]);
    }

    #[test]
    fn test_get_leads_no_stack_tags_nobody() {
        let c = cfg_with_leads();
        // Used to return EVERY lead of EVERY stack.
        assert!(c.get_leads(None).is_empty());
        assert!(c.get_leads(Some("App UI")).is_empty());
        assert!(c.leads_mention_string(None).is_empty());
    }

    #[test]
    fn test_default_stack_leads_is_empty() {
        assert!(Config::default().stack_leads.is_empty());
    }
}
