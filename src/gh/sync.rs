use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::client;
use crate::models::item::{ContentType, Item};
use crate::models::status::Status;

// ── GraphQL Response Structs ──────────────────────────────────

#[derive(Debug, Deserialize)]
struct GraphQLResponse {
    data: Option<ResponseData>,
    errors: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct ResponseData {
    organization: OrgData,
}

#[derive(Debug, Deserialize)]
struct OrgData {
    #[serde(rename = "projectV2")]
    project_v2: ProjectData,
}

#[derive(Debug, Deserialize)]
struct ProjectData {
    items: ItemsConnection,
}

#[derive(Debug, Deserialize)]
struct ItemsConnection {
    nodes: Vec<ProjectItemNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemNode {
    id: String,
    #[serde(rename = "fieldValues")]
    field_values: FieldValuesConnection,
    content: Option<ContentNode>,
}

#[derive(Debug, Deserialize)]
struct FieldValuesConnection {
    nodes: Vec<serde_json::Value>,
}

// Tagged by GraphQL `__typename` - with `untagged`, PRs matched the Issue
// variant first (identical fields) and every PR was misparsed as an Issue.
#[derive(Debug, Deserialize)]
#[serde(tag = "__typename")]
enum ContentNode {
    Issue(IssueContent),
    DraftIssue(DraftIssueContent),
    PullRequest(PullRequestContent),
}

#[derive(Debug, Deserialize)]
struct IssueContent {
    id: String,
    number: u32,
    title: String,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    assignees: AssigneesConnection,
    labels: LabelsConnection,
    repository: Option<RepoInfo>,
    /// Sub-issues parent (GitHub sub-issues feature), if any.
    parent: Option<ParentInfo>,
}

#[derive(Debug, Deserialize)]
struct ParentInfo {
    number: u32,
    title: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestContent {
    id: String,
    number: u32,
    title: String,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    assignees: AssigneesConnection,
    labels: LabelsConnection,
    repository: RepoInfo, // Pull requests always have a repository
}

#[derive(Debug, Deserialize)]
struct DraftIssueContent {
    title: String,
    #[serde(default)]
    #[allow(dead_code)]
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssigneesConnection {
    nodes: Vec<LoginNode>,
}

#[derive(Debug, Deserialize)]
struct LoginNode {
    login: String,
}

#[derive(Debug, Deserialize)]
struct LabelsConnection {
    nodes: Vec<LabelNode>,
}

#[derive(Debug, Deserialize)]
struct LabelNode {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RepoInfo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

// ── GraphQL Query ─────────────────────────────────────────────

const ITEMS_QUERY: &str = r#"
query($owner: String!, $number: Int!, $cursor: String) {
  organization(login: $owner) {
    projectV2(number: $number) {
      items(first: 100, after: $cursor) {
        nodes {
          id
          fieldValues(first: 20) {
            nodes {
              ... on ProjectV2ItemFieldSingleSelectValue {
                name
                field { ... on ProjectV2SingleSelectField { name } }
              }
              ... on ProjectV2ItemFieldTextValue {
                text
                field { ... on ProjectV2Field { name } }
              }
              ... on ProjectV2ItemFieldIterationValue {
                title
                field { ... on ProjectV2IterationField { name } }
              }
            }
          }
          content {
            __typename
            ... on Issue {
              id
              number
              title
              createdAt
              updatedAt
              assignees(first: 10) { nodes { login } }
              labels(first: 30) { nodes { name } }
              repository { nameWithOwner }
              parent { number title }
            }
            ... on DraftIssue {
              title
              body
            }
            ... on PullRequest {
              id
              number
              title
              createdAt
              updatedAt
              assignees(first: 10) { nodes { login } }
              labels(first: 30) { nodes { name } }
              repository { nameWithOwner }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;

// ── Field Value Parsing ───────────────────────────────────────

struct ParsedFields {
    status: Option<String>,
    priority: Option<String>,
    size: Option<String>,
    stack: Option<String>,
    sprint: Option<String>,
}

fn parse_field_values(nodes: &[serde_json::Value]) -> ParsedFields {
    let mut fields = ParsedFields {
        status: None,
        priority: None,
        size: None,
        stack: None,
        sprint: None,
    };

    for node in nodes {
        let field_name = node
            .get("field")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str());

        // Case-insensitive: matching field names byte-for-byte meant a board
        // field named "status" produced items with no status at all (which
        // then rendered as Backlog).
        let Some(field_name) = field_name else {
            continue;
        };
        let select_value = || node.get("name").and_then(|v| v.as_str()).map(String::from);
        if field_name.eq_ignore_ascii_case("Status") {
            fields.status = select_value();
        } else if field_name.eq_ignore_ascii_case("Priority") {
            fields.priority = select_value();
        } else if field_name.eq_ignore_ascii_case("Size") {
            fields.size = select_value();
        } else if field_name.eq_ignore_ascii_case("Stack") {
            fields.stack = select_value();
        } else if field_name.eq_ignore_ascii_case("Sprint") {
            fields.sprint = node.get("title").and_then(|v| v.as_str()).map(String::from);
        }
    }

    fields
}

// ── Convert GraphQL Node to Item ──────────────────────────────

fn node_to_item(node: ProjectItemNode) -> Item {
    let fields = parse_field_values(&node.field_values.nodes);

    let (
        content_node_id,
        number,
        content_type,
        title,
        assignees,
        labels,
        repository,
        created_at,
        updated_at,
        parent,
    ) = match node.content {
        Some(ContentNode::Issue(issue)) => (
            Some(issue.id),
            Some(issue.number),
            ContentType::Issue,
            issue.title,
            issue.assignees.nodes.into_iter().map(|n| n.login).collect(),
            issue.labels.nodes.into_iter().map(|n| n.name).collect(),
            issue.repository.map(|r| r.name_with_owner),
            issue
                .created_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            issue
                .updated_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            issue.parent.map(|p| (p.number, p.title)),
        ),
        Some(ContentNode::PullRequest(pr)) => (
            Some(pr.id),
            Some(pr.number),
            ContentType::PullRequest,
            pr.title,
            pr.assignees.nodes.into_iter().map(|n| n.login).collect(),
            pr.labels.nodes.into_iter().map(|n| n.name).collect(),
            Some(pr.repository.name_with_owner),
            pr.created_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            pr.updated_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            None,
        ),
        Some(ContentNode::DraftIssue(draft)) => (
            None,
            None,
            ContentType::DraftIssue,
            draft.title,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
        ),
        None => (
            None,
            None,
            ContentType::DraftIssue, // Default to DraftIssue if no content
            String::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
        ),
    };

    Item {
        id: node.id,
        content_node_id,
        title,
        status: Status::from_str(fields.status.as_deref().unwrap_or("Backlog")),
        priority: fields.priority,
        size: fields.size,
        stack: fields.stack,
        sprint: fields.sprint,
        assignees,
        labels,
        repository,
        number,
        content_type,
        body: None,
        comments: Vec::new(),
        linked_prs: Vec::new(),
        created_at,
        updated_at,
        status_history: None,
        comment_history: None,
        label_history: None,
        parent_number: parent.as_ref().map(|(n, _)| *n),
        parent_title: parent.map(|(_, t)| t),
        selected: false,
    }
}

// ── Public API ────────────────────────────────────────────────

/// Full sync: fetch ALL project items using paginated GraphQL.
/// Cost: ~3 pts per 100 items → ~39 pts for 1300 items
/// (vs 1,314 pts for `gh project item-list --limit 1500`)
pub fn full_sync(owner: &str, project_number: u32) -> Result<Vec<Item>> {
    let mut all_items = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let owner_arg = format!("owner={}", owner);
        let number_arg = format!("number={}", project_number);
        let query_arg = format!("query={}", ITEMS_QUERY);
        let cursor_arg = cursor.as_ref().map(|c| format!("cursor={}", c));

        let mut args = vec![
            "api",
            "graphql",
            "-F",
            &owner_arg,
            "-F",
            &number_arg,
            "-F",
            &query_arg,
        ];

        // Add cursor if we have one
        if let Some(ref cf) = cursor_arg {
            args.push("-F");
            args.push(cf);
        }

        let output = client::exec(&args)?;
        let response: GraphQLResponse =
            serde_json::from_str(&output).context("Failed to parse GraphQL sync response")?;

        if let Some(errors) = &response.errors {
            if !errors.is_empty() {
                // Surface the human-readable message, not a debug dump.
                let msg = errors[0]
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown GraphQL error");
                anyhow::bail!("GraphQL sync error: {}", msg);
            }
        }

        let data = response
            .data
            .ok_or_else(|| anyhow::anyhow!("No data in GraphQL response"))?;

        let items_conn = data.organization.project_v2.items;

        for node in items_conn.nodes {
            all_items.push(node_to_item(node));
        }

        if items_conn.page_info.has_next_page {
            cursor = items_conn.page_info.end_cursor;
            // hasNextPage with a null cursor would refetch page 1 forever.
            if cursor.is_none() {
                break;
            }
        } else {
            break;
        }
    }

    Ok(all_items)
}
