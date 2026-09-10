use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use crate::app::App;

/// Render the Setup Wizard screen.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let fi = app.setup_field_idx;
    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  GIT-TUI   Project setup   step 2 of 3",
            theme::title(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Paste your GitHub Project URL and Bug Repo URL below.",
            theme::normal(),
        )),
        Line::from(""),
    ];

    // ─── Field 0: Project URL ───
    render_url_field(
        &mut lines,
        fi == 0,
        "Project URL",
        &app.setup_project_url,
        "e.g. https://github.com/orgs/MyOrg/projects/9",
    );

    // ─── Field 1: Bug Repo URL ───
    render_url_field(
        &mut lines,
        fi == 1,
        "Bug Repo URL",
        &app.setup_bug_repo_url,
        "e.g. https://github.com/MyOrg/my-repo",
    );

    // ─── Preview parsed values ───
    let (owner, number) = parse_project_url(&app.setup_project_url);
    let bug_repo = parse_repo_url(&app.setup_bug_repo_url);

    lines.push(Line::from(Span::styled("  Parsed:", theme::title())));
    let owner_display = owner.as_deref().unwrap_or("-").to_string();
    let number_display = number.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
    let repo_display = bug_repo.as_deref().unwrap_or("-").to_string();

    lines.push(Line::from(vec![
        Span::styled("    Owner:     ", theme::dim()),
        Span::styled(
            owner_display,
            if owner.is_some() {
                theme::success()
            } else {
                theme::dim()
            },
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("    Project #: ", theme::dim()),
        Span::styled(
            number_display,
            if number.is_some() {
                theme::success()
            } else {
                theme::dim()
            },
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("    Bug repo:  ", theme::dim()),
        Span::styled(
            repo_display,
            if bug_repo.is_some() {
                theme::success()
            } else {
                theme::dim()
            },
        ),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(" Setup ", theme::title()));

    f.render_widget(Paragraph::new(lines).block(block), chunks[0]);

    // Footer
    // Esc quits only on first run (no screen to go back to).
    let back_label = if app.previous_screens.is_empty() {
        "Quit"
    } else {
        "Back"
    };
    let footer = super::utils::footer_line(
        &[
            ("Tab", "Switch field"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+S", "Save & Start"),
            ("Esc", back_label),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}

/// Render a URL input field with label + example.
fn render_url_field(lines: &mut Vec<Line>, focused: bool, label: &str, value: &str, example: &str) {
    let cursor = if focused { theme::icon_cursor() } else { "" };
    let style = if focused {
        theme::selected()
    } else {
        theme::normal()
    };
    let prefix = if focused { theme::icon_arrow() } else { " " };

    lines.push(Line::from(vec![Span::styled(
        format!("  {} {} ", prefix, label),
        theme::dim(),
    )]));
    lines.push(Line::from(vec![
        Span::styled("    ", theme::dim()),
        Span::styled(format!("[{}{}]", value, cursor), style),
    ]));
    lines.push(Line::from(Span::styled(
        format!("    {}", example),
        theme::dim(),
    )));
    lines.push(Line::from(""));
}

/// Parse project URL like `https://github.com/orgs/MyOrg/projects/9`
/// Returns (owner, project_number).
pub fn parse_project_url(url: &str) -> (Option<String>, Option<u32>) {
    let url = url.trim().trim_end_matches('/');

    // Try: github.com/orgs/{owner}/projects/{number}
    if let Some(rest) = url
        .strip_prefix("https://github.com/orgs/")
        .or_else(|| url.strip_prefix("github.com/orgs/"))
    {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 3 && parts[1] == "projects" {
            let owner = parts[0].to_string();
            let number = parts[2].parse::<u32>().ok();
            return (Some(owner), number);
        }
    }

    // Try: github.com/users/{owner}/projects/{number}
    if let Some(rest) = url
        .strip_prefix("https://github.com/users/")
        .or_else(|| url.strip_prefix("github.com/users/"))
    {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 3 && parts[1] == "projects" {
            let owner = parts[0].to_string();
            let number = parts[2].parse::<u32>().ok();
            return (Some(owner), number);
        }
    }

    (None, None)
}

/// Parse repo URL like `https://github.com/MyOrg/my-repo`
/// Returns `Some("MyOrg/my-repo")`.
pub fn parse_repo_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');

    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("github.com/"))?;

    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        // Skip "orgs" or "users" prefixes
        if parts[0] == "orgs" || parts[0] == "users" {
            return None;
        }
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_url() {
        let (owner, num) = parse_project_url("https://github.com/orgs/MyOrg/projects/9");
        assert_eq!(owner.as_deref(), Some("MyOrg"));
        assert_eq!(num, Some(9));

        let (owner, num) = parse_project_url("https://github.com/orgs/TestCo/projects/42/");
        assert_eq!(owner.as_deref(), Some("TestCo"));
        assert_eq!(num, Some(42));

        let (owner, num) = parse_project_url("not a url");
        assert!(owner.is_none());
        assert!(num.is_none());
    }

    #[test]
    fn test_parse_repo_url() {
        let repo = parse_repo_url("https://github.com/MyOrg/my-repo");
        assert_eq!(repo.as_deref(), Some("MyOrg/my-repo"));

        let repo = parse_repo_url("https://github.com/MyOrg/my-repo/");
        assert_eq!(repo.as_deref(), Some("MyOrg/my-repo"));

        let repo = parse_repo_url("bad url");
        assert!(repo.is_none());
    }
}
