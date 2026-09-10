use anyhow::Result;

use super::client;
/// Edit an item's single-select field (e.g., status).
pub fn edit_field(item_id: &str, project_id: &str, field_id: &str, option_id: &str) -> Result<()> {
    client::exec(&[
        "project",
        "item-edit",
        "--id",
        item_id,
        "--project-id",
        project_id,
        "--field-id",
        field_id,
        "--single-select-option-id",
        option_id,
    ])?;
    Ok(())
}

/// Edit an item's iteration field (e.g., Sprint).
pub fn edit_iteration_field(
    item_id: &str,
    project_id: &str,
    field_id: &str,
    iteration_id: &str,
) -> Result<()> {
    client::exec(&[
        "project",
        "item-edit",
        "--id",
        item_id,
        "--project-id",
        project_id,
        "--field-id",
        field_id,
        "--iteration-id",
        iteration_id,
    ])?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct ItemAddResponse {
    id: String,
}

/// Add an issue/PR URL to a project board and return the new Item ID.
pub fn add_to_project(owner: &str, project_number: u32, url: &str) -> Result<String> {
    let res: ItemAddResponse = client::exec_json(&[
        "project",
        "item-add",
        &project_number.to_string(),
        "--owner",
        owner,
        "--url",
        url,
        "--format",
        "json",
    ])?;
    Ok(res.id)
}

#[derive(serde::Deserialize, Debug)]
struct NodeQueryResponse {
    data: Option<NodeQueryData>,
}

#[derive(serde::Deserialize, Debug)]
struct NodeQueryData {
    node: Option<ProjectV2ItemNode>,
}

#[derive(serde::Deserialize, Debug)]
struct ProjectV2ItemNode {
    #[serde(rename = "fieldValues")]
    field_values: FieldValuesConnection,
}

#[derive(serde::Deserialize, Debug)]
struct FieldValuesConnection {
    nodes: Vec<serde_json::Value>,
}

/// Fetch the live value of one single-select field from a ProjectV2Item.
pub fn get_item_single_select(item_id: &str, field: &str) -> Result<Option<String>> {
    let query = r#"
query($id: ID!) {
  node(id: $id) {
    ... on ProjectV2Item {
      fieldValues(first: 20) {
        nodes {
          ... on ProjectV2ItemFieldSingleSelectValue {
            name
            field { ... on ProjectV2SingleSelectField { name } }
          }
        }
      }
    }
  }
}
"#;
    let out = client::exec(&[
        "api",
        "graphql",
        "-F",
        &format!("id={}", item_id),
        "-F",
        &format!("query={}", query),
    ])?;
    let res: NodeQueryResponse = serde_json::from_str(&out)?;
    if let Some(nodes) = res.data.and_then(|d| d.node).map(|n| n.field_values.nodes) {
        for node in nodes {
            let field_name = node
                .get("field")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str());
            if field_name == Some(field) {
                if let Some(val) = node.get("name").and_then(|n| n.as_str()) {
                    return Ok(Some(val.to_string()));
                }
            }
        }
    }
    Ok(None)
}

/// Fetch the live Priority from a ProjectV2Item (after bot automations).
pub fn get_item_priority(item_id: &str) -> Result<Option<String>> {
    get_item_single_select(item_id, "Priority")
}

/// Fetch the live Status from a ProjectV2Item - used to revalidate right
/// before a QA write action (local state can be a poll interval stale).
pub fn get_item_status(item_id: &str) -> Result<Option<String>> {
    get_item_single_select(item_id, "Status")
}
