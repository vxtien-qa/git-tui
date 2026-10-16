use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::status::Status;

/// A ticket's linked PRs as read from GitHub. `partial` when GitHub held some
/// back (more links than one page returns): the list is then not the whole
/// set, and no verdict built on it can be "all on UAT".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkedPrs {
    pub prs: Vec<LinkedPr>,
    pub partial: bool,
}

/// A pull request linked to an issue: one that closes it ("Fixes #n"), one
/// that mentions it, or one linked by hand in the Development panel.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LinkedPr {
    /// `owner/name` of the PR's repository.
    pub repo: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    /// GitHub's state: `OPEN`, `CLOSED` or `MERGED`.
    pub state: String,
    /// The commit the merge produced, for merged PRs.
    pub merge_commit: Option<String>,
    /// The last commit of the PR's branch. Teams that promote a fix by
    /// merging its branch into a per-environment branch (`fix/x-staging`)
    /// never put the merge commit on staging, but this one gets there.
    #[serde(default)]
    pub head_commit: Option<String>,
    /// The branch the PR targets.
    pub base_ref: String,
}

impl LinkedPr {
    /// `repo#number` without the owner, for display.
    pub fn short_ref(&self) -> String {
        let name = self.repo.rsplit('/').next().unwrap_or(&self.repo);
        format!("{}#{}", name, self.number)
    }

    pub fn is_merged(&self) -> bool {
        self.state.eq_ignore_ascii_case("MERGED")
    }
}

/// A project board item (issue or draft).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    /// Project item ID (PVTI_xxx)
    pub id: String,
    /// Underlying Content Node ID (I_kw... for Issues)
    pub content_node_id: Option<String>,
    /// Issue/draft title
    pub title: String,
    /// Current status column
    pub status: Status,
    /// Priority (P0-P4)
    pub priority: Option<String>,
    /// Size (XS-XL)
    pub size: Option<String>,
    /// Stack (FE/BE/App UI/App BE)
    pub stack: Option<String>,
    /// Sprint iteration name
    pub sprint: Option<String>,
    /// Assigned usernames
    pub assignees: Vec<String>,
    /// Labels (Bug, FE, BE, etc.)
    pub labels: Vec<String>,
    /// Repository name
    pub repository: Option<String>,
    /// Issue number (if it's an issue, not a draft)
    pub number: Option<u32>,
    /// Content type
    pub content_type: ContentType,
    /// Issue body (markdown)
    pub body: Option<String>,
    /// Comments
    pub comments: Vec<Comment>,
    /// Pull requests linked to the issue, loaded with its detail.
    #[serde(default)]
    pub linked_prs: Vec<LinkedPr>,
    /// Created timestamp
    pub created_at: Option<DateTime<Utc>>,
    /// Updated timestamp (from GitHub API)
    pub updated_at: Option<DateTime<Utc>>,
    /// Status change history (fetched dynamically)
    #[serde(skip)]
    pub status_history: Option<Vec<crate::gh::timeline::StatusChangedEvent>>,
    /// Comment history (fetched dynamically alongside status_history)
    #[serde(skip)]
    pub comment_history: Option<Vec<crate::gh::timeline::CommentEvent>>,
    /// Label-added history (fetched alongside status_history) - Pass Dev/STG
    /// are label-only actions, so reports need these events.
    #[serde(skip)]
    pub label_history: Option<Vec<crate::gh::timeline::LabelEvent>>,
    /// Sub-issues parent (GitHub sub-issues), fetched at sync time.
    #[serde(default)]
    pub parent_number: Option<u32>,
    #[serde(default)]
    pub parent_title: Option<String>,
    /// The parent's repository: a code-repo ticket often hangs off an epic in
    /// the board repo. `None` from caches written before it was synced; the
    /// child's own repository is assumed then.
    #[serde(default)]
    pub parent_repo: Option<String>,
    /// Whether this item is selected for batch operations
    #[serde(skip)]
    pub selected: bool,
}

/// Ticket type as shown in the list views, derived from the labels.
///
/// One definition for the board, My Tasks and Search: each used to inline its
/// own `labels contains "bug"` test, so an Enhancement rendered as "Task".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Bug,
    Enhancement,
    Task,
}

impl ItemKind {
    /// Compact label for list views.
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Bug => "Bug",
            Self::Enhancement => "Enh",
            Self::Task => "Task",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ContentType {
    #[default]
    Issue,
    DraftIssue,
    PullRequest,
}

/// A comment on an issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    /// Media attachments extracted from comment body
    pub media: Vec<MediaItem>,
}

/// An image or video reference found in ticket/comment markdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub media_type: MediaType,
    pub url: String,
    pub alt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaType {
    Image,
    Video,
}

impl Item {
    /// Get the devs assigned (all assignees minus the current user).
    pub fn dev_assignees(&self, current_user: &str) -> Vec<String> {
        self.assignees
            .iter()
            .filter(|a| *a != current_user)
            .cloned()
            .collect()
    }

    /// Ticket type for list views (Bug wins if an item carries both labels).
    pub fn kind(&self) -> ItemKind {
        let has = |name: &str| self.labels.iter().any(|l| l.to_lowercase().contains(name));
        if has("bug") {
            ItemKind::Bug
        } else if has("enhancement") {
            ItemKind::Enhancement
        } else {
            ItemKind::Task
        }
    }

    /// Check if this item has linked PRs.
    pub fn has_prs(&self) -> bool {
        !self.linked_prs.is_empty()
    }

    /// Format PR count for display.
    pub fn pr_display(&self) -> String {
        let count = self.linked_prs.len();
        if count == 0 {
            "--".to_string()
        } else if count == 1 {
            "1 PR".to_string()
        } else {
            format!("{} PRs", count)
        }
    }

    /// Priority sort key (P0 = highest = 0).
    pub fn priority_sort_key(&self) -> u8 {
        match self.priority.as_deref() {
            Some("P0") => 0,
            Some("P1") => 1,
            Some("P2") => 2,
            Some("P3") => 3,
            Some("P4") => 4,
            _ => 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_sort_key() {
        let p0 = Item {
            priority: Some("P0".to_string()),
            ..Default::default()
        };
        let p4 = Item {
            priority: Some("P4".to_string()),
            ..Default::default()
        };
        let none = Item {
            priority: None,
            ..Default::default()
        };

        assert!(p0.priority_sort_key() < p4.priority_sort_key());
        assert!(p4.priority_sort_key() < none.priority_sort_key());
        assert_eq!(none.priority_sort_key(), 5);
    }

    #[test]
    fn test_dev_assignees() {
        let current_user = "brucevu";
        let item = Item {
            assignees: vec![
                "brucevu".to_string(),
                "alice".to_string(),
                "bob".to_string(),
            ],
            ..Default::default()
        };
        let devs = item.dev_assignees(current_user);
        assert_eq!(devs, vec!["alice", "bob"]);

        let empty_item = Item {
            assignees: vec![],
            ..Default::default()
        };
        assert!(empty_item.dev_assignees(current_user).is_empty());
    }

    #[test]
    fn test_kind_from_labels() {
        let kind = |labels: &[&str]| {
            Item {
                labels: labels.iter().map(|l| l.to_string()).collect(),
                ..Default::default()
            }
            .kind()
        };
        assert_eq!(kind(&["Bug"]), ItemKind::Bug);
        // An enhancement also carries Task; Enhancement must win the display.
        assert_eq!(kind(&["Enhancement", "Task"]), ItemKind::Enhancement);
        assert_eq!(kind(&["Task"]), ItemKind::Task);
        assert_eq!(kind(&[]), ItemKind::Task);
        // Case and surrounding words do not matter.
        assert_eq!(kind(&["FE", "bug report"]), ItemKind::Bug);
    }

    #[test]
    fn test_pr_display() {
        let mut item = Item {
            ..Default::default()
        };
        assert_eq!(item.pr_display(), "--");

        item.linked_prs = vec![];
        assert_eq!(item.pr_display(), "--");

        let pr = |n: u32| LinkedPr {
            repo: "org/repo".into(),
            number: n,
            url: format!("https://github.com/org/repo/pull/{}", n),
            ..Default::default()
        };
        item.linked_prs = vec![pr(123)];
        assert_eq!(item.pr_display(), "1 PR");

        item.linked_prs.push(pr(456));
        assert_eq!(item.pr_display(), "2 PRs");
        assert_eq!(item.linked_prs[1].short_ref(), "repo#456");
    }
}
