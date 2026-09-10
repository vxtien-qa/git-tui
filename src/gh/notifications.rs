use anyhow::Result;
use serde::Deserialize;

use super::client;

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubNotification {
    pub id: String,
    pub reason: String,
    pub unread: bool,
    pub updated_at: String,
    pub subject: NotificationSubject,
    pub repository: NotificationRepo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationSubject {
    pub title: String,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    pub subject_type: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationRepo {
    pub full_name: String,
}

/// Fetch notifications from GitHub API.
pub fn fetch() -> Result<Vec<GitHubNotification>> {
    let output = client::api("notifications?all=true", None, &[])?;
    // Surface parse failures instead of silently showing an empty list.
    let notifications: Vec<GitHubNotification> = serde_json::from_str(&output)
        .map_err(|e| anyhow::anyhow!("Failed to parse notifications: {}", e))?;
    Ok(notifications)
}

/// Mark a notification as read.
pub fn mark_read(notification_id: &str) -> Result<()> {
    let endpoint = format!("notifications/threads/{}", notification_id);
    client::api(&endpoint, Some("PATCH"), &[])?;
    Ok(())
}

/// Mark all notifications as read.
pub fn mark_all_read() -> Result<()> {
    client::api("notifications", Some("PUT"), &[("read", "true")])?;
    Ok(())
}
