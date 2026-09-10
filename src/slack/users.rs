use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::time::Duration;

/// A Slack workspace member (subset of API response fields).
/// Bots and deleted users are filtered out during fetch.
#[derive(Debug, Clone)]
pub struct SlackUser {
    pub id: String,
    pub name: String,         // workspace username
    pub real_name: String,    // full name
    pub display_name: String, // display name (may differ)
}

/// Raw API response structures for JSON deserialization.
#[derive(Deserialize)]
struct UsersListResponse {
    ok: bool,
    members: Option<Vec<MemberRaw>>,
    error: Option<String>,
    response_metadata: Option<ResponseMetadata>,
}

#[derive(Deserialize)]
struct ResponseMetadata {
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct MemberRaw {
    id: String,
    name: String,
    pub real_name: Option<String>,
    deleted: Option<bool>,
    pub is_bot: Option<bool>,
    profile: Option<ProfileRaw>,
}

#[derive(Deserialize)]
struct ProfileRaw {
    display_name: Option<String>,
    real_name: Option<String>,
}

/// Validate that a string looks like a Slack Bot Token.
/// Format: `xoxb-` followed by alphanumeric/dashes.
pub fn validate_bot_token(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed.starts_with("xoxb-") && trimmed.len() > 10
}

/// Fetch all non-bot, non-deleted human members from a Slack workspace.
/// Uses the `users.list` API with cursor-based pagination.
/// Requires a Bot Token with `users:read` scope.
pub fn fetch_users(bot_token: &str) -> Result<Vec<SlackUser>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();

    let mut all_users = Vec::new();
    let mut cursor = String::new();

    loop {
        let mut url = "https://slack.com/api/users.list?limit=200".to_string();
        if !cursor.is_empty() {
            // Slack cursors are base64 - URL-encode the chars that matter.
            let encoded = cursor
                .replace('%', "%25")
                .replace('+', "%2B")
                .replace('/', "%2F")
                .replace('=', "%3D");
            url.push_str(&format!("&cursor={}", encoded));
        }

        let response = agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", bot_token.trim()))
            .header("Content-Type", "application/json")
            .call();

        match response {
            Ok(mut resp) => {
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                let parsed: UsersListResponse = serde_json::from_str(&body)
                    .map_err(|e| anyhow!("Failed to parse Slack response: {}", e))?;

                if !parsed.ok {
                    let err = parsed.error.unwrap_or_else(|| "unknown_error".to_string());
                    return Err(anyhow!("Slack API error: {}", err));
                }

                if let Some(members) = parsed.members {
                    for m in members {
                        // Skip bots, deleted users, and Slackbot
                        if m.is_bot.unwrap_or(false) || m.deleted.unwrap_or(false) {
                            continue;
                        }
                        if m.id == "USLACKBOT" {
                            continue;
                        }

                        let display_name = m
                            .profile
                            .as_ref()
                            .and_then(|p| p.display_name.clone())
                            .unwrap_or_default();
                        let real_name = m.real_name.unwrap_or_else(|| {
                            m.profile
                                .as_ref()
                                .and_then(|p| p.real_name.clone())
                                .unwrap_or_default()
                        });

                        all_users.push(SlackUser {
                            id: m.id,
                            name: m.name,
                            real_name,
                            display_name,
                        });
                    }
                }

                // Check for next page
                let next = parsed
                    .response_metadata
                    .and_then(|m| m.next_cursor)
                    .unwrap_or_default();
                if next.is_empty() {
                    break;
                }
                cursor = next;
            }
            Err(ureq::Error::StatusCode(code)) => {
                return Err(anyhow!("Slack API returned HTTP {}", code));
            }
            Err(e) => {
                return Err(anyhow!("Connection to Slack failed: {}", e));
            }
        }
    }

    // Sort by real_name for easier browsing
    all_users.sort_by_key(|a| a.real_name.to_lowercase());
    Ok(all_users)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_bot_token_valid() {
        assert!(validate_bot_token("xoxb-example-token-value"));
        assert!(validate_bot_token("  xoxb-example-token  "));
    }

    #[test]
    fn test_validate_bot_token_invalid() {
        assert!(!validate_bot_token("xoxp-user-token-here")); // user token
        assert!(!validate_bot_token("xoxb-")); // too short
        assert!(!validate_bot_token("not-a-token"));
        assert!(!validate_bot_token(""));
    }

    #[test]
    fn test_parse_users_list_response() {
        let json = r#"{
            "ok": true,
            "members": [
                {
                    "id": "U04ABC123",
                    "name": "khanh.dh",
                    "real_name": "Alex Lee",
                    "deleted": false,
                    "is_bot": false,
                    "profile": {
                        "display_name": "Khánh",
                        "real_name": "Alex Lee"
                    }
                },
                {
                    "id": "U04BOT999",
                    "name": "mybot",
                    "real_name": "My Bot",
                    "deleted": false,
                    "is_bot": true,
                    "profile": {
                        "display_name": "bot",
                        "real_name": "My Bot"
                    }
                }
            ]
        }"#;

        let parsed: UsersListResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.ok);
        let members = parsed.members.unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "khanh.dh");
        assert!(members[1].is_bot.unwrap());
    }

    #[test]
    fn test_parse_error_response() {
        let json = r#"{"ok": false, "error": "invalid_auth"}"#;
        let parsed: UsersListResponse = serde_json::from_str(json).unwrap();
        assert!(!parsed.ok);
        assert_eq!(parsed.error.unwrap(), "invalid_auth");
    }
}
