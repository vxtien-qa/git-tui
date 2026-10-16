use anyhow::Result;

use super::client;
use crate::models::item::{LinkedPr, LinkedPrs};

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
/// ("Fixes #n") and the ones linked by hand in the Development panel; the
/// timeline's cross-references hold the PRs that only mention it - which is
/// where bundle PRs ("All outstanding FE work for s29 (#3920, ...)") and
/// deploy-branch PRs show up. Connection events are left out on purpose: a
/// hand-linked PR is already in the first list, and an event for one that was
/// later unlinked would bring it back.
pub fn linked_prs(repo: &str, number: u32) -> Result<LinkedPrs> {
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
    Ok(LinkedPrs {
        prs: parse_linked_prs(&json)?,
        partial: parse_linked_prs_partial(&json),
    })
}

/// Whether `LINKED_PRS_QUERY` came back with more links than it read: more
/// closing PRs than its first page, or references older than its last one.
pub fn parse_linked_prs_partial(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    let issue = &v["data"]["repository"]["issue"];
    issue["closedByPullRequestsReferences"]["pageInfo"]["hasNextPage"] == true
        || issue["timelineItems"]["pageInfo"]["hasPreviousPage"] == true
}

const LINKED_PRS_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      closedByPullRequestsReferences(first: 25, includeClosedPrs: true) {
        pageInfo { hasNextPage }
        nodes { ...pr }
      }
      timelineItems(last: 100, itemTypes: [CROSS_REFERENCED_EVENT]) {
        pageInfo { hasPreviousPage }
        nodes {
          ... on CrossReferencedEvent { source { ... on PullRequest { ...pr } } }
        }
      }
    }
  }
}
fragment pr on PullRequest {
  number title url state baseRefName headRefOid
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
            head_commit: node["headRefOid"].as_str().map(str::to_string),
            base_ref: text("baseRefName"),
        });
    }
    prs.sort_by(|a, b| a.repo.cmp(&b.repo).then(a.number.cmp(&b.number)));
    Ok(prs)
}

/// Whether a merged PR's change is on `branch`.
///
/// The merge commit is checked first. When the branch does not have it, the
/// PR's head commit is checked too: a fix promoted by merging its branch into
/// a per-environment branch (be-compliance-api's `fix/4001-...-staging`) is
/// on staging with every one of its commits while the develop merge commit
/// never is. Either one being in the branch means the change is there.
pub fn pr_deploy_state(pr: &LinkedPr, branch: &str) -> DeployState {
    let by_merge = pr
        .merge_commit
        .as_deref()
        .map(|sha| deploy_state(&pr.repo, branch, sha));
    if by_merge == Some(DeployState::Contained) {
        return DeployState::Contained;
    }
    let by_head = pr
        .head_commit
        .as_deref()
        .filter(|head| Some(*head) != pr.merge_commit.as_deref())
        .map(|head| deploy_state(&pr.repo, branch, head));
    combine_deploy_states(by_merge, by_head)
}

/// [`pr_deploy_state`]'s verdict from its two lookups: the head commit can
/// only turn the answer into ✓; otherwise the merge commit's answer stands.
pub fn combine_deploy_states(
    by_merge: Option<DeployState>,
    by_head: Option<DeployState>,
) -> DeployState {
    match (by_merge, by_head) {
        (_, Some(DeployState::Contained)) => DeployState::Contained,
        (Some(merge), _) => merge,
        (None, Some(head)) => head,
        (None, None) => DeployState::Failed("no commit to check".to_string()),
    }
}

/// Whether `sha` is on `branch` of `repo`, as GitHub's compare API sees it.
pub fn deploy_state(repo: &str, branch: &str, sha: &str) -> DeployState {
    // per_page=1: only `status` is read, so skip the commit list.
    let endpoint = format!("repos/{}/compare/{}...{}?per_page=1", repo, branch, sha);
    match client::exec(&["api", &endpoint, "--jq", ".status"]) {
        Ok(status) => DeployState::from_compare_status(status.trim()),
        Err(e) => DeployState::from_error(&e.to_string()),
    }
}

/// Where a merged PR's commit stands against one environment branch.
#[derive(Debug, Clone, PartialEq)]
pub enum DeployState {
    /// `identical` or `behind`: the branch contains the commit.
    Contained,
    /// `ahead`: the branch is an ancestor of the commit, so it lacks it.
    Missing,
    /// `diverged`: the commit is not in the branch's history, but the change
    /// may have arrived another way, such as a cherry-pick onto a deploy
    /// branch. Never reported as "not deployed".
    Unknown,
    /// GitHub answered 404: no such branch, commit or repository.
    NotFound,
    /// The check itself failed.
    Failed(String),
}

impl DeployState {
    pub fn from_compare_status(status: &str) -> Self {
        match status {
            "identical" | "behind" => DeployState::Contained,
            "ahead" => DeployState::Missing,
            "diverged" => DeployState::Unknown,
            other => DeployState::Failed(format!("unexpected compare status '{}'", other)),
        }
    }

    fn from_error(message: &str) -> Self {
        if message.contains("HTTP 404") || message.contains("Not Found") {
            DeployState::NotFound
        } else {
            DeployState::Failed(client::gh_reason(message))
        }
    }
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
             "headRefOid":"1faa5620",
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
        assert_eq!(prs[1].head_commit.as_deref(), Some("1faa5620"));
        assert_eq!(prs[2].merge_commit, None);
        assert_eq!(prs[2].base_ref, "uat");
    }

    #[test]
    fn test_deploy_state_from_compare_status() {
        assert_eq!(
            DeployState::from_compare_status("identical"),
            DeployState::Contained
        );
        assert_eq!(
            DeployState::from_compare_status("behind"),
            DeployState::Contained
        );
        assert_eq!(
            DeployState::from_compare_status("ahead"),
            DeployState::Missing
        );
        // Not in the branch's history, but maybe cherry-picked: unknown,
        // never "not deployed".
        assert_eq!(
            DeployState::from_compare_status("diverged"),
            DeployState::Unknown
        );
        assert!(matches!(
            DeployState::from_compare_status("weird"),
            DeployState::Failed(_)
        ));
        assert_eq!(
            DeployState::from_error(
                "gh api repos/x/compare/nope...abc failed: gh: Not Found (HTTP 404)"
            ),
            DeployState::NotFound
        );
        assert!(matches!(
            DeployState::from_error("gh: API rate limit exceeded (HTTP 403)"),
            DeployState::Failed(_)
        ));
        // The reason, not the echoed command.
        assert_eq!(
            DeployState::from_error(
                "gh api repos/x/compare/uat...abc?per_page=1 --jq .status failed: gh: Bad credentials (HTTP 401)"
            ),
            DeployState::Failed("gh: Bad credentials (HTTP 401)".to_string())
        );
    }

    #[test]
    fn test_head_commit_on_the_branch_means_the_change_is_there() {
        use DeployState::*;
        // be-compliance-api#2104: the develop merge commit never reaches
        // staging (diverged), the branch head does (merged via -staging).
        assert_eq!(
            combine_deploy_states(Some(Unknown), Some(Contained)),
            Contained
        );
        assert_eq!(
            combine_deploy_states(Some(Missing), Some(Contained)),
            Contained
        );
        // The head can only turn the answer into ✓.
        assert_eq!(combine_deploy_states(Some(Unknown), Some(Missing)), Unknown);
        assert_eq!(combine_deploy_states(Some(Missing), Some(Unknown)), Missing);
        assert_eq!(combine_deploy_states(Some(Contained), None), Contained);
        assert_eq!(combine_deploy_states(Some(NotFound), None), NotFound);
        assert!(matches!(
            combine_deploy_states(Some(Failed("x".into())), Some(Unknown)),
            Failed(_)
        ));
    }

    #[test]
    fn test_linked_prs_partial_when_github_holds_links_back() {
        let full = r#"{"data":{"repository":{"issue":{
            "closedByPullRequestsReferences":{"pageInfo":{"hasNextPage":false},"nodes":[]},
            "timelineItems":{"pageInfo":{"hasPreviousPage":false},"nodes":[]}}}}}"#;
        assert!(!parse_linked_prs_partial(full));
        let older = full.replace(r#""hasPreviousPage":false"#, r#""hasPreviousPage":true"#);
        assert!(parse_linked_prs_partial(&older));
        let more = full.replace(r#""hasNextPage":false"#, r#""hasNextPage":true"#);
        assert!(parse_linked_prs_partial(&more));
    }

    #[test]
    fn test_parse_linked_prs_reports_graphql_errors() {
        let json = r#"{"errors":[{"message":"Could not resolve to an Issue"}]}"#;
        assert!(parse_linked_prs(json).is_err());
    }
}
