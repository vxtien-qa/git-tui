use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::client;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProjectV2Item {
    #[allow(dead_code)]
    pub title: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StatusChangedEvent {
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "previousStatus")]
    pub previous_status: Option<String>,
    pub status: Option<String>,
    #[allow(dead_code)]
    pub project: Option<ProjectV2Item>,
    /// Who performed the status change (login).
    #[serde(default, deserialize_with = "deser_login")]
    pub actor: Option<String>,
}

/// A lightweight comment event (only author + timestamp, no body).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CommentEvent {
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    /// Comment author login.
    #[serde(default, deserialize_with = "deser_login")]
    pub author: Option<String>,
}

/// A label being added to an issue. Pass Dev / Pass STG are LABEL-ONLY
/// actions, so without these events the reports can't see the two most
/// common QA actions at all.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LabelEvent {
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    /// Name of the label that was added.
    pub label: String,
    /// Who added it (login).
    #[serde(default, deserialize_with = "deser_login")]
    pub actor: Option<String>,
}

/// Combined timeline data cached per issue.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct TimelineData {
    pub status_changes: Vec<StatusChangedEvent>,
    pub comments: Vec<CommentEvent>,
    /// Labels added (LabeledEvent). `default` keeps old cache entries readable.
    #[serde(default)]
    pub labels: Vec<LabelEvent>,
}

/// Deserialize login field from actor/author - handles BOTH formats:
/// 1. `{ "login": "username" }` from GraphQL response
/// 2. `"username"` plain string from cache
fn deser_login<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match v {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(serde_json::Value::Object(map)) => {
            Ok(map.get("login").and_then(|v| v.as_str()).map(String::from))
        }
        _ => Ok(None),
    }
}

/// Info for a batch timeline query entry.
pub struct BatchTimelineEntry {
    pub item_idx: usize,
    pub owner: String,
    pub repo_name: String,
    pub issue_number: u32,
}

/// Batch fetch status changes + comments for multiple issues in a single GraphQL query.
/// Groups by (owner, repo) and uses aliases like `issue0`, `issue1`, etc.
/// Maximum ~30 issues per query to stay within GraphQL complexity limits.
pub fn fetch_status_changes_batch(
    entries: &[BatchTimelineEntry],
) -> Result<Vec<(usize, TimelineData)>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    // Build aliased query fragments
    let mut fragments = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        // `last:` (not `first:`) - timeline items come oldest-first, and a
        // busy ticket with 250+ events would otherwise lose its NEWEST status
        // changes, exactly the ones the reports need.
        fragments.push(format!(
            r#"repo{i}: repository(owner: "{owner}", name: "{repo}") {{
                issue{i}: issue(number: {num}) {{
                    timelineItems(last: 250, itemTypes: [PROJECT_V2_ITEM_STATUS_CHANGED_EVENT, ISSUE_COMMENT, LABELED_EVENT]) {{
                        nodes {{
                            __typename
                            ... on ProjectV2ItemStatusChangedEvent {{
                                createdAt
                                previousStatus
                                status
                                project {{ title }}
                                actor {{ login }}
                            }}
                            ... on IssueComment {{
                                createdAt
                                author {{ login }}
                            }}
                            ... on LabeledEvent {{
                                createdAt
                                label {{ name }}
                                actor {{ login }}
                            }}
                        }}
                    }}
                }}
            }}"#,
            i = i,
            owner = entry.owner,
            repo = entry.repo_name,
            num = entry.issue_number,
        ));
    }

    let query = format!("query {{ {} }}", fragments.join("\n"));

    let output = client::exec(&["api", "graphql", "-F", &format!("query={}", query)])?;
    let raw: serde_json::Value =
        serde_json::from_str(&output).context("Failed to parse batch GraphQL response")?;

    // GraphQL returns partial data alongside errors (one deleted/transferred
    // issue errors its own alias, the other 29 are fine). Only bail when there
    // is NO data at all - otherwise one bad issue would permanently block its
    // whole batch on every refresh.
    let has_data = raw.get("data").map(|d| !d.is_null()).unwrap_or(false);
    if !has_data {
        let msg = raw
            .get("errors")
            .and_then(|e| e.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("unknown GraphQL error");
        anyhow::bail!("GraphQL batch error: {}", msg);
    }

    let mut results = Vec::new();

    if let Some(data) = raw.get("data") {
        for (i, entry) in entries.iter().enumerate() {
            let repo_key = format!("repo{}", i);
            let issue_key = format!("issue{}", i);

            let nodes = data
                .get(&repo_key)
                .and_then(|r| r.get(&issue_key))
                .and_then(|iss| iss.get("timelineItems"))
                .and_then(|tl| tl.get("nodes"))
                .and_then(|n| n.as_array());

            let mut timeline = TimelineData::default();

            if let Some(nodes) = nodes {
                for node in nodes {
                    let typename = node
                        .get("__typename")
                        .and_then(|t| t.as_str())
                        .unwrap_or("");

                    match typename {
                        "ProjectV2ItemStatusChangedEvent" => {
                            if let Ok(evt) =
                                serde_json::from_value::<StatusChangedEvent>(node.clone())
                            {
                                timeline.status_changes.push(evt);
                            }
                        }
                        "IssueComment" => {
                            if let Ok(evt) = serde_json::from_value::<CommentEvent>(node.clone()) {
                                timeline.comments.push(evt);
                            }
                        }
                        "LabeledEvent" => {
                            let label = node
                                .get("label")
                                .and_then(|l| l.get("name"))
                                .and_then(|n| n.as_str())
                                .map(String::from);
                            let created_at = node
                                .get("createdAt")
                                .and_then(|c| c.as_str())
                                .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                            let actor = node
                                .get("actor")
                                .and_then(|a| a.get("login"))
                                .and_then(|l| l.as_str())
                                .map(String::from);
                            if let (Some(label), Some(created_at)) = (label, created_at) {
                                timeline.labels.push(LabelEvent {
                                    created_at,
                                    label,
                                    actor,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }

            results.push((entry.item_idx, timeline));
        }
    }

    Ok(results)
}
