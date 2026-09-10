use anyhow::Result;
use serde::Deserialize;

use super::client;
use crate::models::project::GhProjectListEntry;

/// Wrapper for `gh project list --format json` response.
#[derive(Debug, Deserialize)]
struct GhProjectListResponse {
    projects: Vec<GhProjectListEntry>,
    #[serde(rename = "totalCount", default)]
    #[allow(dead_code)]
    total_count: u32,
}

/// List all accessible projects for an owner.
pub fn list(owner: &str) -> Result<Vec<GhProjectListEntry>> {
    let response: GhProjectListResponse = client::exec_json(&[
        "project", "list", "--owner", owner, "--format", "json", "--limit", "50",
    ])?;
    Ok(response.projects)
}

/// Get project ID by number.
pub fn get_id(owner: &str, number: u32) -> Result<String> {
    let projects = list(owner)?;
    let project = projects
        .iter()
        .find(|p| p.number == number)
        .ok_or_else(|| anyhow::anyhow!("Project #{} not found", number))?;
    Ok(project.id.clone())
}
