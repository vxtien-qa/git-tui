use anyhow::{Context, Result};

use super::client;
use crate::models::field::{Field, GhFieldsResponse};

/// Project fields, including single-select options AND iteration windows.
///
/// Uses GraphQL rather than `gh project field-list`: the CLI's JSON omits the
/// iteration `configuration` entirely, so the Sprint field arrived with no
/// options and every "set the Sprint" write silently found nothing to set.
const FIELDS_QUERY: &str = r#"
query($owner: String!, $number: Int!) {
  organization(login: $owner) {
    projectV2(number: $number) {
      fields(first: 50) {
        nodes {
          __typename
          ... on ProjectV2FieldCommon { id name dataType }
          ... on ProjectV2SingleSelectField { options { id name } }
          ... on ProjectV2IterationField {
            configuration {
              iterations { id title startDate duration }
              completedIterations { id title startDate duration }
            }
          }
        }
      }
    }
  }
}
"#;

/// List all fields for a project.
pub fn list(owner: &str, project_number: u32) -> Result<Vec<Field>> {
    let out = client::exec(&[
        "api",
        "graphql",
        "-F",
        &format!("owner={}", owner),
        "-F",
        &format!("number={}", project_number),
        "-F",
        &format!("query={}", FIELDS_QUERY),
    ])?;

    let res: GhFieldsResponse =
        serde_json::from_str(&out).context("Failed to parse project fields response")?;

    let nodes = res
        .data
        .and_then(|d| d.organization)
        .and_then(|o| o.project_v2)
        .map(|p| p.fields.nodes)
        .ok_or_else(|| anyhow::anyhow!("No project fields in response (org project not found?)"))?;

    Ok(nodes.into_iter().map(|n| n.into_field()).collect())
}
