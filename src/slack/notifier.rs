use crate::config::SlackUserMapping;
use crate::slack::webhook;
use std::collections::HashMap;

/// Format and send a QA action notification to Slack.
/// Returns the webhook result so callers can surface a failed notify
/// (previously errors were silently swallowed).
///
/// - `user`: the QA user who performed the action
/// - `devs`: dev assignees to tag (may be empty)
/// - `repo`: GitHub repo (e.g. "acme/project-board") for issue links
/// - `user_map`: GitHub → Slack mapping for proper `<@U12345>` mentions
#[allow(clippy::too_many_arguments)]
pub fn notify_action(
    webhook_url: &str,
    user: &str,
    action_label: &str,
    item_title: &str,
    item_number: Option<u32>,
    repo: Option<&str>,
    devs: &[String],
    user_map: &HashMap<String, SlackUserMapping>,
    body: Option<&str>,
) -> anyhow::Result<()> {
    let issue_link = make_issue_link(item_number, repo);
    let dev_mentions = resolve_dev_mentions(devs, user_map);
    // Actor shown as display name (not <@id>) so the QA doesn't ping themself.
    let actor = if user.is_empty() {
        String::new()
    } else {
        user_map
            .get(user)
            .map(|m| m.slack_display.clone())
            .unwrap_or_else(|| user.to_string())
    };
    let msg = format_action_message(
        action_label,
        &issue_link,
        item_title,
        &actor,
        &dev_mentions,
        body,
    );
    webhook::send_message(webhook_url, &msg)
}

/// Format the Slack message for a QA action.
fn format_action_message(
    action_label: &str,
    issue_link: &str,
    title: &str,
    actor: &str,
    dev_mentions: &str,
    body: Option<&str>,
) -> String {
    let icon = match action_label {
        "Passed Dev" | "Passed STG" | "Passed UAT" => "✅",
        "Returned" => "🔄",
        "Bug Report" => "🐛",
        "Enhancement" => "✨",
        "Task Created" => "📋",
        "Comment" => "💬",
        "Moved" => "📦",
        _ => "🔔",
    };
    // User-supplied text (titles, comment bodies, display names) is escaped so
    // Slack can't interpret it - otherwise "<!channel>" in a ticket title
    // would ping the whole channel. Links/mentions built by US stay raw.
    let title = webhook::escape_mrkdwn(title);
    let mut msg = format!("{} *[{}]* {}{}", icon, action_label, issue_link, title);
    if !actor.is_empty() {
        msg.push_str(&format!(" - by {}", webhook::escape_mrkdwn(actor)));
    }

    if let Some(text) = body {
        if !text.trim().is_empty() {
            let safe = webhook::escape_mrkdwn(text.trim());
            msg.push_str(&format!("\n> {}", safe.replace('\n', "\n> ")));
        }
    }

    if !dev_mentions.is_empty() {
        // Plain label: the ZWJ emoji rendered as two glyphs (or a box) in
        // several Slack clients.
        msg.push_str(&format!("\nCC: {}", dev_mentions));
    }
    msg
}

/// Build a Slack-formatted issue link: `<https://github.com/org/repo/issues/42|#42> `
/// Falls back to `#42 ` if no repo, or empty string if no number.
fn make_issue_link(number: Option<u32>, repo: Option<&str>) -> String {
    match (number, repo) {
        (Some(n), Some(r)) => format!("<https://github.com/{}/issues/{}|#{}> ", r, n, n),
        (Some(n), None) => format!("#{} ", n),
        _ => String::new(),
    }
}

/// Resolve a username to a Slack mention.
/// If mapped, returns `<@U12345>`; otherwise returns `@username`.
pub fn resolve_mention(username: &str, user_map: &HashMap<String, SlackUserMapping>) -> String {
    if let Some(mapping) = user_map.get(username) {
        format!("<@{}>", mapping.slack_id)
    } else {
        format!("@{}", username)
    }
}

/// Resolve a list of dev usernames to Slack mentions, joined by space.
fn resolve_dev_mentions(devs: &[String], user_map: &HashMap<String, SlackUserMapping>) -> String {
    if devs.is_empty() {
        return String::new();
    }
    devs.iter()
        .map(|d| resolve_mention(d, user_map))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One ticket in a release-ready batch.
pub struct ReleaseItem {
    pub number: Option<u32>,
    pub repo: Option<String>,
    pub title: String,
}

/// Send the aggregated "cleared UAT regression, ready for release" message.
///
/// One message for the whole batch instead of one per ticket: a regression run
/// covers everything currently deployed to UAT, so fifteen separate pings say
/// less than one list and bury the channel.
pub fn notify_release_ready(
    webhook_url: &str,
    user: &str,
    items: &[ReleaseItem],
    devs: &[String],
    user_map: &HashMap<String, SlackUserMapping>,
) -> anyhow::Result<()> {
    webhook::send_message(
        webhook_url,
        &format_release_ready(user, items, devs, user_map),
    )
}

fn format_release_ready(
    user: &str,
    items: &[ReleaseItem],
    devs: &[String],
    user_map: &HashMap<String, SlackUserMapping>,
) -> String {
    let actor = if user.is_empty() {
        String::new()
    } else {
        user_map
            .get(user)
            .map(|m| m.slack_display.clone())
            .unwrap_or_else(|| user.to_string())
    };

    let mut msg = format!(
        "✅ *[Ready for release]* {} ticket(s) cleared UAT regression",
        items.len()
    );
    if !actor.is_empty() {
        msg.push_str(&format!(" by {}", webhook::escape_mrkdwn(&actor)));
    }
    msg.push('\n');

    for item in items {
        let link = make_issue_link(item.number, item.repo.as_deref());
        let link = if link.is_empty() { String::new() } else { link };
        msg.push_str(&format!(
            "• {}{}\n",
            link,
            webhook::escape_mrkdwn(&item.title)
        ));
    }

    let mentions = resolve_dev_mentions(devs, user_map);
    if !mentions.is_empty() {
        msg.push_str(&format!("CC: {}", mentions));
    }
    msg
}

/// Send a daily standup report to Slack. Returns the webhook result so the
/// caller can report success/failure instead of claiming success blindly.
pub fn notify_daily_standup(webhook_url: &str, report_text: &str) -> anyhow::Result<()> {
    webhook::send_message(webhook_url, report_text)
}

/// Send a sprint report to Slack. Returns the webhook result so the caller
/// can report success/failure instead of claiming success blindly.
pub fn notify_sprint_report(webhook_url: &str, report_text: &str) -> anyhow::Result<()> {
    webhook::send_message(webhook_url, report_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_action_pass_dev_no_devs() {
        let msg = format_action_message("Passed Dev", "#42 ", "Fix login", "", "", None);
        assert_eq!(msg, "✅ *[Passed Dev]* #42 Fix login");
    }

    #[test]
    fn test_format_action_with_devs() {
        let msg = format_action_message(
            "Passed Dev",
            "#42 ",
            "Fix login",
            "",
            "<@U111> <@U222>",
            None,
        );
        assert_eq!(msg, "✅ *[Passed Dev]* #42 Fix login\nCC: <@U111> <@U222>");
    }

    #[test]
    fn test_format_action_with_actor() {
        let msg = format_action_message("Passed STG", "#42 ", "Fix login", "Tien Vu", "", None);
        assert_eq!(msg, "✅ *[Passed STG]* #42 Fix login - by Tien Vu");
    }

    #[test]
    fn test_format_action_pass_uat_icon() {
        let msg = format_action_message("Passed UAT", "#42 ", "Fix login", "", "", None);
        assert!(msg.starts_with("✅ *[Passed UAT]*"));
    }

    #[test]
    fn test_format_action_escapes_mrkdwn_injection() {
        // A hostile/unlucky ticket title must not become a Slack directive.
        let msg = format_action_message(
            "Passed Dev",
            "#42 ",
            "<!channel> Fix <input> & stuff",
            "",
            "",
            None,
        );
        assert!(msg.contains("&lt;!channel&gt; Fix &lt;input&gt; &amp; stuff"));
        assert!(!msg.contains("<!channel>"));

        // Comment bodies too
        let msg = format_action_message("Comment", "#10 ", "t", "", "", Some("ping <@U123>"));
        assert!(msg.contains("&lt;@U123&gt;"));
    }

    #[test]
    fn test_format_action_bug_report() {
        let msg = format_action_message("Bug Report", "", "[BUG] Crash (P1, FE)", "", "", None);
        assert_eq!(msg, "🐛 *[Bug Report]* [BUG] Crash (P1, FE)");
    }

    #[test]
    fn test_format_action_comment_with_body() {
        let msg = format_action_message("Comment", "#10 ", "Some ticket", "", "", Some("LGTM!"));
        assert_eq!(msg, "💬 *[Comment]* #10 Some ticket\n> LGTM!");
    }

    #[test]
    fn test_format_action_comment_multiline_body() {
        let msg = format_action_message(
            "Comment",
            "#10 ",
            "Some ticket",
            "",
            "",
            Some("Line1\nLine2"),
        );
        assert_eq!(msg, "💬 *[Comment]* #10 Some ticket\n> Line1\n> Line2");
    }

    #[test]
    fn test_make_issue_link_with_repo() {
        let link = make_issue_link(Some(42), Some("acme/project-board"));
        assert_eq!(
            link,
            "<https://github.com/acme/project-board/issues/42|#42> "
        );
    }

    #[test]
    fn test_make_issue_link_no_repo() {
        let link = make_issue_link(Some(42), None);
        assert_eq!(link, "#42 ");
    }

    #[test]
    fn test_make_issue_link_no_number() {
        let link = make_issue_link(None, Some("org/repo"));
        assert_eq!(link, "");
    }

    #[test]
    fn test_format_release_ready_lists_every_ticket() {
        let mut map = HashMap::new();
        map.insert(
            "tien".to_string(),
            SlackUserMapping {
                slack_id: "U111".to_string(),
                slack_display: "Tien Vu".to_string(),
            },
        );
        let items = vec![
            ReleaseItem {
                number: Some(401),
                repo: Some("Org/repo".to_string()),
                title: "Fix login".to_string(),
            },
            ReleaseItem {
                number: Some(402),
                repo: Some("Org/repo".to_string()),
                title: "Fix logout".to_string(),
            },
        ];
        let msg = format_release_ready("tien", &items, &["tien".to_string()], &map);
        assert!(msg
            .starts_with("✅ *[Ready for release]* 2 ticket(s) cleared UAT regression by Tien Vu"));
        assert!(msg.contains("<https://github.com/Org/repo/issues/401|#401> Fix login"));
        assert!(msg.contains("<https://github.com/Org/repo/issues/402|#402> Fix logout"));
        assert!(msg.contains("CC: <@U111>"));
    }

    #[test]
    fn test_format_release_ready_escapes_titles() {
        let map = HashMap::new();
        let items = vec![ReleaseItem {
            number: Some(7),
            repo: None,
            title: "<!channel> ship it".to_string(),
        }];
        let msg = format_release_ready("", &items, &[], &map);
        assert!(
            !msg.contains("<!channel>"),
            "must not be able to ping the channel: {}",
            msg
        );
        assert!(msg.contains("&lt;!channel&gt;"));
    }

    #[test]
    fn test_format_action_enhancement() {
        let msg =
            format_action_message("Enhancement", "", "[ENHANCEMENT] Bulk export", "", "", None);
        assert_eq!(msg, "✨ *[Enhancement]* [ENHANCEMENT] Bulk export");
    }

    #[test]
    fn test_resolve_mention_mapped() {
        let mut map = HashMap::new();
        map.insert(
            "fe-lead".to_string(),
            SlackUserMapping {
                slack_id: "U04ABC123".to_string(),
                slack_display: "Alex Lee".to_string(),
            },
        );
        assert_eq!(resolve_mention("fe-lead", &map), "<@U04ABC123>");
    }

    #[test]
    fn test_resolve_mention_unmapped() {
        let map = HashMap::new();
        assert_eq!(resolve_mention("some-user", &map), "@some-user");
    }

    #[test]
    fn test_resolve_dev_mentions_mixed() {
        let mut map = HashMap::new();
        map.insert(
            "dev1".to_string(),
            SlackUserMapping {
                slack_id: "U111".to_string(),
                slack_display: "Dev One".to_string(),
            },
        );
        let devs = vec!["dev1".to_string(), "dev2".to_string()];
        assert_eq!(resolve_dev_mentions(&devs, &map), "<@U111> @dev2");
    }

    #[test]
    fn test_resolve_dev_mentions_empty() {
        let map = HashMap::new();
        let devs: Vec<String> = vec![];
        assert_eq!(resolve_dev_mentions(&devs, &map), "");
    }
}
