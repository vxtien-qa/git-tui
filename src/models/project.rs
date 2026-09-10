use serde::Deserialize;

/// Raw JSON entry from `gh project list --format json`.
/// Real format: {"number":9,"title":"KUP CX","id":"PVT_...","closed":false,
///   "owner":{"login":"acme","type":"Organization"},
///   "items":{"totalCount":1270},"fields":{"totalCount":15},...}
#[derive(Debug, Deserialize)]
pub struct GhProjectListEntry {
    pub number: u32,
    #[allow(dead_code)]
    pub title: String,
    pub id: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub closed: bool,
    #[allow(dead_code)]
    #[serde(default)]
    pub owner: Option<GhOwner>,
    #[allow(dead_code)]
    #[serde(default)]
    pub items: Option<GhTotalCount>,
}

#[derive(Debug, Deserialize)]
pub struct GhOwner {
    #[allow(dead_code)]
    pub login: String,
    #[allow(dead_code)]
    #[serde(rename = "type", default)]
    pub owner_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GhTotalCount {
    #[allow(dead_code)]
    #[serde(rename = "totalCount", default)]
    pub total_count: u32,
}
