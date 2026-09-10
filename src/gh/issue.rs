use anyhow::Result;

use super::client;

/// Create an issue in a repo.
pub fn create(
    repo: &str,
    title: &str,
    body: &str,
    labels: &[&str],
    assignees: &[&str],
) -> Result<String> {
    let mut args = vec![
        "issue", "create", "--repo", repo, "--title", title, "--body", body,
    ];

    let labels_str = labels.join(",");
    if !labels.is_empty() {
        args.push("--label");
        args.push(&labels_str);
    }

    let assignees_str = assignees.join(",");
    if !assignees.is_empty() {
        args.push("--assignee");
        args.push(&assignees_str);
    }

    let output = client::exec(&args)?;
    // Output contains the issue URL
    Ok(output.trim().to_string())
}

/// Add a comment to an issue.
pub fn comment(repo: &str, issue_number: u32, body: &str) -> Result<()> {
    client::exec(&[
        "issue",
        "comment",
        &issue_number.to_string(),
        "--repo",
        repo,
        "--body",
        body,
    ])?;
    Ok(())
}

/// Add a label to an issue.
pub fn add_label(repo: &str, issue_number: u32, label: &str) -> Result<()> {
    client::exec(&[
        "issue",
        "edit",
        &issue_number.to_string(),
        "--repo",
        repo,
        "--add-label",
        label,
    ])?;
    Ok(())
}

/// Remove a label from an issue.
///
/// Needed because the QA handoff labels (Ready-for-Staging / Ready-for-UAT)
/// are the only record that a ticket was passed - leaving them on a ticket
/// that came back for retest both hides it from My Tasks and makes the
/// "already passed" guard refuse the new pass.
pub fn remove_label(repo: &str, issue_number: u32, label: &str) -> Result<()> {
    client::exec(&[
        "issue",
        "edit",
        &issue_number.to_string(),
        "--repo",
        repo,
        "--remove-label",
        label,
    ])?;
    Ok(())
}

/// Get issue details with body and comments.
pub fn view(repo: &str, issue_number: u32) -> Result<String> {
    client::exec(&[
        "issue",
        "view",
        &issue_number.to_string(),
        "--repo",
        repo,
        "--json",
        "id,title,body,comments,labels,assignees,state,createdAt,number",
    ])
}

/// The issue's own GraphQL node ID (`I_kw...`).
///
/// Mutations that take an Issue (`updateIssue`, `addSubIssue`) need this, NOT
/// the project item ID (`PVTI_...`) returned by `project item-add`: they are
/// different nodes, and passing the latter fails with "could not resolve to a
/// node" - silently, wherever the result was discarded.
pub fn node_id(repo: &str, issue_number: u32) -> Result<String> {
    let json = view(repo, issue_number)?;
    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    parsed
        .get("id")
        .and_then(|id| id.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("issue view returned no node id"))
}

/// Add an issue as a sub-issue of another via GraphQL.
pub fn add_sub_issue(parent_node_id: &str, sub_issue_node_id: &str) -> Result<()> {
    let query = format!(
        r#"mutation {{
            addSubIssue(input: {{ issueId: "{}", subIssueId: "{}" }}) {{
                clientMutationId
            }}
        }}"#,
        parent_node_id, sub_issue_node_id
    );

    client::exec(&["api", "graphql", "-f", &format!("query={}", query)])?;
    Ok(())
}

/// Set the repository-level Issue Type (e.g., "Bug", "Task") for a given Issue node ID.
pub fn set_issue_type(repo: &str, issue_node_id: &str, type_name: &str) -> Result<()> {
    let parts: Vec<&str> = repo.split('/').collect();
    if parts.len() != 2 {
        return Err(anyhow::anyhow!("Invalid repo format for set_issue_type"));
    }
    let owner = parts[0];
    let name = parts[1];

    // 1. Fetch Repository Issue Types
    let query = format!(
        r#"query {{
            repository(owner: "{}", name: "{}") {{
                issueTypes(first: 20) {{
                    nodes {{
                        id
                        name
                    }}
                }}
            }}
        }}"#,
        owner, name
    );

    let res = client::exec(&["api", "graphql", "-f", &format!("query={}", query)])?;
    let parsed: serde_json::Value = serde_json::from_str(&res)?;

    // Navigate to nodes
    let nodes = parsed
        .get("data")
        .and_then(|d| d.get("repository"))
        .and_then(|r| r.get("issueTypes"))
        .and_then(|i| i.get("nodes"))
        .and_then(|n| n.as_array())
        .ok_or_else(|| anyhow::anyhow!("Failed to parse issueTypes from GraphQL response"))?;

    // 2. Find the matching IssueType ID
    let mut type_node_id = None;
    for node in nodes {
        if let Some(t_name) = node.get("name").and_then(|n| n.as_str()) {
            if t_name.eq_ignore_ascii_case(type_name) {
                type_node_id = node.get("id").and_then(|id| id.as_str()).map(String::from);
                break;
            }
        }
    }

    let Some(type_id) = type_node_id else {
        // Not an error the user can act on from here, but the caller should be
        // able to say "the board has no such issue type" instead of nothing.
        anyhow::bail!("repository has no issue type named '{}'", type_name);
    };

    // 3. Mutate the target Issue with the new IssueType ID
    let mutation = format!(
        r#"mutation {{
            updateIssue(input: {{ id: "{}", issueTypeId: "{}" }}) {{
                issue {{ id }}
            }}
        }}"#,
        issue_node_id, type_id
    );
    client::exec(&["api", "graphql", "-f", &format!("query={}", mutation)])?;

    Ok(())
}
