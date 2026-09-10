use anyhow::{anyhow, Result};
use serde::Serialize;
use std::time::Duration;

// No `link_names`: it no longer links individual @usernames (only groups /
// @channel-style keywords), so keeping it only widened the surface for a
// plain-text "@channel" in a ticket title to ping the whole channel.
#[derive(Serialize)]
struct SlackMessage<'a> {
    text: &'a str,
}

/// Escape Slack mrkdwn control characters in USER-SUPPLIED text (titles,
/// comment bodies). `<...>` delimits links/mentions in Slack - unescaped, a
/// ticket titled "<!channel> boom" would ping the whole channel, and
/// "Fix <input>" would render with the "<input>" swallowed.
/// Do NOT run this over already-built `<url|#42>` links or `<@UID>` mentions.
pub fn escape_mrkdwn(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Slack truncates `text` around 40k characters - cut with a marker instead
/// of letting Slack cut mid-word (mostly relevant for big sprint reports).
const MAX_TEXT_LEN: usize = 39_000;

/// Validate webhook URL format.
/// Must start with `https://hooks.slack.com/services/`.
pub fn validate_url(url: &str) -> bool {
    let trimmed = url.trim();
    trimmed.starts_with("https://hooks.slack.com/services/")
        && trimmed.len() > "https://hooks.slack.com/services/".len()
}

/// Send a plain text message to a Slack Incoming Webhook.
///
/// Uses ureq with 10s timeout. Returns Ok(()) on success (HTTP 200, body "ok").
/// Returns descriptive error on failure:
/// - Network/TLS errors
/// - HTTP 400 (invalid_payload, no_text)
/// - HTTP 403 (action_prohibited, posting_to_general_channel_denied)
/// - HTTP 404 (no_service, channel_not_found)
/// - HTTP 410 (channel_is_archived)
pub fn send_message(webhook_url: &str, text: &str) -> Result<()> {
    // One retry with a short backoff on transient failures (429 / 5xx /
    // network) - a QA handoff notification shouldn't be lost to a blip.
    match send_once(webhook_url, text) {
        Ok(()) => Ok(()),
        Err(SendError::Permanent(e)) => Err(e),
        Err(SendError::Transient(first)) => {
            std::thread::sleep(Duration::from_secs(2));
            match send_once(webhook_url, text) {
                Ok(()) => Ok(()),
                Err(SendError::Permanent(e)) | Err(SendError::Transient(e)) => {
                    Err(e.context(format!("(after retry; first error: {})", first)))
                }
            }
        }
    }
}

enum SendError {
    /// Won't succeed on retry (bad payload, dead webhook).
    Permanent(anyhow::Error),
    /// Worth one retry (rate limit, server error, network).
    Transient(anyhow::Error),
}

fn send_once(webhook_url: &str, text: &str) -> std::result::Result<(), SendError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();

    let truncated;
    let text = if text.len() > MAX_TEXT_LEN {
        let mut cut = MAX_TEXT_LEN;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        truncated = format!("{}\n… (truncated)", &text[..cut]);
        truncated.as_str()
    } else {
        text
    };

    let payload = SlackMessage { text };

    let response = agent.post(webhook_url).send_json(&payload);

    match response {
        Ok(mut resp) => {
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            if body == "ok" {
                Ok(())
            } else {
                // Slack returns error string in body: "no_text", "invalid_payload", etc.
                Err(SendError::Permanent(anyhow!("Slack rejected: {}", body)))
            }
        }
        Err(ureq::Error::StatusCode(code)) => {
            let err = anyhow!(
                "Slack returned HTTP {} - check webhook URL and channel",
                code
            );
            if code == 429 || code >= 500 {
                Err(SendError::Transient(err))
            } else {
                Err(SendError::Permanent(err))
            }
        }
        Err(e) => {
            // Network, DNS, TLS, timeout errors
            Err(SendError::Transient(anyhow!("Connection failed: {}", e)))
        }
    }
}

/// Send a test message to verify webhook URL works.
pub fn send_test(webhook_url: &str) -> Result<()> {
    send_message(
        webhook_url,
        "git-tui connected. Slack integration is working.",
    )
}

/// Mask a webhook URL for display: show only last 4 chars of the token.
/// e.g. "https://hooks.slack.com/services/T00/B00/abcdefghijklmnop" → "hooks.slack.com/...mnop"
pub fn mask_url(url: &str) -> String {
    let trimmed = url.trim();
    // char-based (not byte-sliced) so odd input can't panic
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() > 4 {
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("hooks.slack.com/...{}", suffix)
    } else {
        "hooks.slack.com/...".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_url_valid() {
        assert!(validate_url(
            "https://hooks.slack.com/services/T00000/B00000/xxxxxxxx"
        ));
        assert!(validate_url(
            "  https://hooks.slack.com/services/T00/B00/abc  "
        ));
    }

    #[test]
    fn test_validate_url_invalid() {
        assert!(!validate_url("https://hooks.slack.com/services/")); // no token
        assert!(!validate_url("https://example.com/webhook"));
        assert!(!validate_url("http://hooks.slack.com/services/T/B/x")); // http not https
        assert!(!validate_url(""));
        assert!(!validate_url("not a url"));
    }

    #[test]
    fn test_mask_url() {
        let masked = mask_url("https://hooks.slack.com/services/T00/B00/abcdefghijklmnop");
        assert_eq!(masked, "hooks.slack.com/...mnop");
    }

    #[test]
    fn test_mask_url_short() {
        let masked = mask_url("abc");
        assert_eq!(masked, "hooks.slack.com/...");
    }
}
