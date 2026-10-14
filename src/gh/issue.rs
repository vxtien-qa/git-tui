use anyhow::Result;

use super::client;
use crate::models::item::LinkedPr;

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

/// Pull requests linked to an issue, from both places GitHub records them.
///
/// `closedByPullRequestsReferences` holds the PRs that close the issue
/// ("Fixes #n"); the timeline's cross-references and connections hold the PRs
/// that only mention it or were linked by hand - which is where bundle PRs
/// ("All outstanding FE work for s29 (#3920, ...)") and deploy-branch PRs
/// show up.
pub fn linked_prs(repo: &str, number: u32) -> Result<Vec<LinkedPr>> {
    let (owner, name) = repo
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("repository '{}' is not owner/name", repo))?;
    let json = client::exec(&[
        "api",
        "graphql",
        "-f",
        &format!("query={}", LINKED_PRS_QUERY),
        "-F",
        &format!("owner={}", owner),
        "-F",
        &format!("name={}", name),
        "-F",
        &format!("number={}", number),
    ])?;
    parse_linked_prs(&json)
}

const LINKED_PRS_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      closedByPullRequestsReferences(first: 25, includeClosedPrs: true) {
        nodes { ...pr }
      }
      timelineItems(last: 100, itemTypes: [CROSS_REFERENCED_EVENT, CONNECTED_EVENT]) {
        nodes {
          ... on CrossReferencedEvent { source { ... on PullRequest { ...pr } } }
          ... on ConnectedEvent { subject { ... on PullRequest { ...pr } } }
        }
      }
    }
  }
}
fragment pr on PullRequest {
  number title url state baseRefName
  mergeCommit { oid }
  repository { nameWithOwner }
}
"#;

/// Parse `LINKED_PRS_QUERY`'s response: deduplicated by URL, sorted by
/// repository then number. Timeline events whose source is an issue come back
/// as empty objects and are skipped.
pub fn parse_linked_prs(json: &str) -> Result<Vec<LinkedPr>> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    if let Some(errors) = v.get("errors") {
        anyhow::bail!("GraphQL error: {}", errors);
    }
    let issue = &v["data"]["repository"]["issue"];
    let mut nodes: Vec<&serde_json::Value> = Vec::new();
    if let Some(list) = issue["closedByPullRequestsReferences"]["nodes"].as_array() {
        nodes.extend(list.iter());
    }
    if let Some(list) = issue["timelineItems"]["nodes"].as_array() {
        for event in list {
            for key in ["source", "subject"] {
                if let Some(pr) = event.get(key) {
                    nodes.push(pr);
                }
            }
        }
    }

    let mut prs: Vec<LinkedPr> = Vec::new();
    for node in nodes {
        let (Some(number), Some(url)) = (node["number"].as_u64(), node["url"].as_str()) else {
            continue;
        };
        if prs.iter().any(|p| p.url == url) {
            continue;
        }
        let text = |k: &str| node[k].as_str().unwrap_or("").to_string();
        prs.push(LinkedPr {
            repo: node["repository"]["nameWithOwner"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            number: number as u32,
            title: text("title"),
            url: url.to_string(),
            state: text("state"),
            merge_commit: node["mergeCommit"]["oid"].as_str().map(str::to_string),
            base_ref: text("baseRefName"),
        });
    }
    prs.sort_by(|a, b| a.repo.cmp(&b.repo).then(a.number.cmp(&b.number)));
    Ok(prs)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_linked_prs_merges_both_sources() {
        // Trimmed from a real response for Kinatico/kup-project-management#4019.
        let json = r#"{"data":{"repository":{"issue":{
          "closedByPullRequestsReferences":{"nodes":[
            {"number":2104,"title":"Second","url":"https://github.com/Kinatico/be-compliance-api/pull/2104",
             "state":"MERGED","baseRefName":"develop","mergeCommit":{"oid":"6628eb44"},
             "repository":{"nameWithOwner":"Kinatico/be-compliance-api"}}
          ]},
          "timelineItems":{"nodes":[
            {"source":{}},
            {"subject":{"number":2103,"title":"First","url":"https://github.com/Kinatico/be-compliance-api/pull/2103",
             "state":"MERGED","baseRefName":"develop","mergeCommit":{"oid":"745f7db2"},
             "repository":{"nameWithOwner":"Kinatico/be-compliance-api"}}},
            {"subject":{"number":2104,"title":"Second","url":"https://github.com/Kinatico/be-compliance-api/pull/2104",
             "state":"MERGED","baseRefName":"develop","mergeCommit":{"oid":"6628eb44"},
             "repository":{"nameWithOwner":"Kinatico/be-compliance-api"}}},
            {"source":{"number":3793,"title":"Open one","url":"https://github.com/Kinatico/marvelx-web/pull/3793",
             "state":"OPEN","baseRefName":"uat","mergeCommit":null,
             "repository":{"nameWithOwner":"Kinatico/marvelx-web"}}}
          ]}
        }}}}"#;
        let prs = parse_linked_prs(json).expect("parses");
        let refs: Vec<String> = prs.iter().map(|p| p.short_ref()).collect();
        // Deduplicated, issue-only events skipped, sorted by repo then number.
        assert_eq!(
            refs,
            vec![
                "be-compliance-api#2103",
                "be-compliance-api#2104",
                "marvelx-web#3793"
            ]
        );
        assert!(prs[0].is_merged());
        assert_eq!(prs[0].merge_commit.as_deref(), Some("745f7db2"));
        assert_eq!(prs[2].merge_commit, None);
        assert_eq!(prs[2].base_ref, "uat");
    }

    #[test]
    fn test_parse_linked_prs_reports_graphql_errors() {
        let json = r#"{"errors":[{"message":"Could not resolve to an Issue"}]}"#;
        assert!(parse_linked_prs(json).is_err());
    }
}
