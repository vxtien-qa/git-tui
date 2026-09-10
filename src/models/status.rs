use serde::{Deserialize, Serialize};
use std::fmt;

/// All 11 status columns on the KUP CX board.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Status {
    #[default]
    Backlog,
    ReadyForDev,
    InProgress,
    Blocked,
    InReview,
    InQADev,
    InQA,
    InUAT,
    TechComplete,
    ReadyForRelease,
    Done,
}

impl Status {
    /// Parse a status label, returning `None` when the column is unknown to
    /// this build. `from_str` folds unknown columns into `Backlog` so the UI
    /// always has something to draw; drift detection needs to tell the two
    /// apart (see `App::status_drift_warning`) - an unrecognised column used
    /// to be silently counted as Backlog, which is exactly how "In UAT" items
    /// went missing before v3.2.0.
    pub fn parse_known(s: &str) -> Option<Self> {
        match s.trim() {
            s if s.eq_ignore_ascii_case("Backlog") => Some(Self::Backlog),
            s if s.eq_ignore_ascii_case("Ready for Dev") => Some(Self::ReadyForDev),
            s if s.eq_ignore_ascii_case("In Progress") => Some(Self::InProgress),
            s if s.eq_ignore_ascii_case("Blocked") => Some(Self::Blocked),
            s if s.eq_ignore_ascii_case("In Review") => Some(Self::InReview),
            s if s.eq_ignore_ascii_case("In QA - Dev") => Some(Self::InQADev),
            s if s.eq_ignore_ascii_case("In QA") => Some(Self::InQA),
            s if s.eq_ignore_ascii_case("In UAT") => Some(Self::InUAT),
            s if s.eq_ignore_ascii_case("Tech Complete") => Some(Self::TechComplete),
            s if s.eq_ignore_ascii_case("Ready for Release") => Some(Self::ReadyForRelease),
            s if s.eq_ignore_ascii_case("Done") => Some(Self::Done),
            _ => None,
        }
    }

    /// Parse from the raw status string returned by the GraphQL sync.
    /// Case-insensitive and whitespace-trimmed for robustness; unknown columns
    /// fall back to `Backlog` for display purposes only.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self::parse_known(s).unwrap_or(Self::Backlog)
    }

    /// Display label for the TUI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::ReadyForDev => "Ready for Dev",
            Self::InProgress => "In Progress",
            Self::Blocked => "Blocked",
            Self::InReview => "In Review",
            Self::InQADev => "In QA - Dev",
            Self::InQA => "In QA",
            Self::InUAT => "In UAT",
            Self::TechComplete => "Tech Complete",
            Self::ReadyForRelease => "Ready for Release",
            Self::Done => "Done",
        }
    }

    /// Short label for compact display (board cards).
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::ReadyForDev => "Ready",
            Self::InProgress => "In Prog",
            Self::Blocked => "Blocked",
            Self::InReview => "Review",
            Self::InQADev => "QA-Dev",
            Self::InQA => "In QA",
            Self::InUAT => "UAT",
            Self::TechComplete => "T.Comp",
            Self::ReadyForRelease => "Release",
            Self::Done => "Done",
        }
    }

    /// Key for the Move dialog (1-0 in board order; In UAT uses 'u' since digits ran out).
    pub fn move_key(&self) -> char {
        match self {
            Self::Backlog => '1',
            Self::ReadyForDev => '2',
            Self::InProgress => '3',
            Self::Blocked => '4',
            Self::InReview => '5',
            Self::InQADev => '6',
            Self::InQA => '7',
            Self::InUAT => 'u',
            Self::TechComplete => '8',
            Self::ReadyForRelease => '9',
            Self::Done => '0',
        }
    }

    /// All statuses in board column order.
    pub fn all_columns() -> &'static [Status] {
        &[
            Self::Backlog,
            Self::ReadyForDev,
            Self::InProgress,
            Self::Blocked,
            Self::InReview,
            Self::InQADev,
            Self::InQA,
            Self::InUAT,
            Self::TechComplete,
            Self::ReadyForRelease,
            Self::Done,
        ]
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_and_label_roundtrip() {
        let statuses = Status::all_columns();
        for status in statuses {
            let label = status.label();
            let parsed = Status::from_str(label);
            assert_eq!(*status, parsed, "Failed roundtrip for label: {}", label);
        }
    }

    #[test]
    fn test_from_str_unknown() {
        assert_eq!(Status::from_str("Unknown Status XYZ"), Status::Backlog);
        assert_eq!(Status::from_str(""), Status::Backlog);
    }

    #[test]
    fn test_parse_known_distinguishes_unknown_from_backlog() {
        assert_eq!(Status::parse_known("Backlog"), Some(Status::Backlog));
        assert_eq!(Status::parse_known("in uat"), Some(Status::InUAT));
        // The whole point: an unrecognised column must NOT look like Backlog.
        assert_eq!(Status::parse_known("Ready for QA"), None);
        assert_eq!(Status::parse_known(""), None);
    }

    #[test]
    fn test_from_str_case_insensitive() {
        assert_eq!(Status::from_str("ready for Dev"), Status::ReadyForDev);
        assert_eq!(Status::from_str("In Qa"), Status::InQA);
        assert_eq!(Status::from_str("IN PROGRESS"), Status::InProgress);
        assert_eq!(Status::from_str("  Done  "), Status::Done);
    }

    #[test]
    fn test_from_str_in_uat() {
        assert_eq!(Status::from_str("In UAT"), Status::InUAT);
        assert_eq!(Status::from_str("in uat"), Status::InUAT);
        assert_eq!(Status::from_str("  In UAT  "), Status::InUAT);
    }

    #[test]
    fn test_move_keys_are_unique() {
        // The Move dialog is driven by move_key(); duplicates would make one
        // status unreachable.
        let mut keys: Vec<char> = Status::all_columns().iter().map(|s| s.move_key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Status::all_columns().len());
    }

    #[test]
    fn test_move_key_in_uat() {
        assert_eq!(Status::InUAT.move_key(), 'u');
    }
}
