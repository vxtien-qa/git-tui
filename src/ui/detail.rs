use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::theme;
use super::utils::{trunc, word_wrap};
use crate::app::App;
use crate::models::item::MediaType;

/// Render the full item detail screen.
pub fn render(f: &mut Frame, app: &App, item_idx: usize, area: Rect) {
    let item = match app.items.get(item_idx) {
        Some(i) => i,
        None => return,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // content
            Constraint::Length(1), // footer
        ])
        .split(area);

    let mut lines = Vec::new();

    // ─── Header Fields ───
    lines.push(Line::from(""));
    add_field_row(
        &mut lines,
        "Status",
        item.status.label(),
        "Priority",
        item.priority.as_deref().unwrap_or("--"),
    );
    add_field_row(
        &mut lines,
        "Stack",
        item.stack.as_deref().unwrap_or("--"),
        "Size",
        item.size.as_deref().unwrap_or("--"),
    );
    add_field_row(
        &mut lines,
        "Sprint",
        item.sprint.as_deref().unwrap_or("--"),
        "Assignees",
        &item.assignees.join(", "),
    );
    add_field_row(
        &mut lines,
        "Labels",
        &item.labels.join(", "),
        "Created",
        &item
            .created_at
            .map(|d| d.format("%b %d").to_string())
            .unwrap_or("--".to_string()),
    );
    add_field_row(
        &mut lines,
        "Repo",
        item.repository.as_deref().unwrap_or("--"),
        "",
        "",
    );

    // Sub-issues parent, when the ticket belongs to one
    if let Some(parent_num) = item.parent_number {
        let title_w = (area.width.saturating_sub(24) as usize).max(10);
        let parent_display = format!(
            "#{} {}",
            parent_num,
            super::utils::trunc(item.parent_title.as_deref().unwrap_or(""), title_w)
        );
        add_field_row(&mut lines, "Parent", &parent_display, "", "");
    }

    // ─── Linked PRs ───
    if !item.linked_prs.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  -- Linked PRs ({}) --", item.linked_prs.len()),
            theme::title(),
        )));
        let url_width = (area.width.saturating_sub(8) as usize).max(20);
        for pr_url in &item.linked_prs {
            // Long PR URLs wrap instead of being clipped off-screen.
            for (i, part) in super::utils::word_wrap(pr_url, url_width)
                .into_iter()
                .enumerate()
            {
                let prefix = if i == 0 { theme::icon_arrow() } else { " " };
                lines.push(Line::from(Span::styled(
                    format!("  {} {}", prefix, part),
                    theme::normal(),
                )));
            }
        }
    }

    // ─── Body ───
    let body_width = area.width.saturating_sub(6) as usize; // 2 border + 2 padding + buffer
    if let Some(ref body) = item.body {
        lines.push(Line::from(""));
        let sep_w = body_width.min(50);
        lines.push(Line::from(Span::styled(
            format!("  {}", theme::icon_h_double().repeat(sep_w)),
            theme::dim(),
        )));

        for section_line in parse_body_sections(body, body_width) {
            lines.push(section_line);
        }
    }

    // ─── Comments ───
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ══════════════════════════════════════════════════",
        theme::dim(),
    )));

    if item.body.is_none() {
        // Detail not yet loaded
        lines.push(Line::from(Span::styled(
            "  COMMENTS - Loading... (press Shift+R to reload)",
            theme::dim(),
        )));
    } else if item.comments.is_empty() {
        lines.push(Line::from(Span::styled(
            "  COMMENTS - No comments",
            theme::dim(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("  COMMENTS ({})", item.comments.len()),
            theme::title(),
        )));

        for comment in &item.comments {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(format!("  @{}", comment.author), theme::highlight()),
                Span::styled(
                    format!(" · {}", comment.created_at.format("%b %d, %H:%M")),
                    theme::dim(),
                ),
            ]));

            // Render full comment body (wrapped)
            for text_line in comment.body.lines() {
                let trimmed = text_line.trim();
                if !trimmed.is_empty() {
                    for wrapped in word_wrap(trimmed, body_width) {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", wrapped),
                            theme::normal(),
                        )));
                    }
                }
            }

            // Media attachments
            for media in &comment.media {
                let icon = match media.media_type {
                    MediaType::Image => "[img]",
                    MediaType::Video => "[vid]",
                };
                let alt = media.alt.as_deref().unwrap_or("attachment");
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", icon), theme::highlight()),
                    Span::styled(alt, theme::normal()),
                    Span::styled("   [w] open in browser", theme::dim()),
                ]));
            }
        }
    }

    lines.push(Line::from(""));

    // Apply scroll (capped so content is never fully scrolled past)
    let max_scroll = lines.len().saturating_sub(1);
    let scroll = app.detail_scroll.min(max_scroll);
    let visible_lines: Vec<Line> = lines.into_iter().skip(scroll).collect();

    let title = format!(
        " #{} - {} ",
        item.number.unwrap_or(0),
        trunc(&item.title, (area.width as usize).saturating_sub(20)),
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::border())
        .title(Span::styled(title, theme::title()));

    let paragraph = Paragraph::new(visible_lines).block(block);
    f.render_widget(paragraph, chunks[0]);

    // Footer
    // Compact labels, most-used first: the full, self-explaining action list
    // lives one keypress away in the [p] menu, so this row does not need to
    // carry all sixteen bindings.
    let footer = super::utils::footer_line(
        &[
            ("p", "QA menu"),
            ("c", "Comment"),
            ("e", "Enhancement"),
            ("m", "Move"),
            ("h", "History"),
            ("y", "CopyMD"),
            ("w", "Web"),
            ("o", "Parent"),
            ("r", "Reload"),
            ("?", "Help"),
            ("Esc", "Back"),
        ],
        area.width as usize,
    );
    f.render_widget(Paragraph::new(footer), chunks[1]);
}

fn add_field_row(lines: &mut Vec<Line<'_>>, label1: &str, val1: &str, label2: &str, val2: &str) {
    if label2.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<10}", label1), theme::dim()),
            Span::styled(val1.to_string(), theme::normal()),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<10}", label1), theme::dim()),
            Span::styled(format!("{:<20}", val1), theme::normal()),
            Span::styled(format!("{:<10}", label2), theme::dim()),
            Span::styled(val2.to_string(), theme::normal()),
        ]));
    }
}

/// Parse a ticket body into styled Lines, recognizing sections.
fn parse_body_sections(body: &str, max_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut _current_section = String::new();

    for raw_line in body.lines() {
        let trimmed = raw_line.trim();

        // Section headers: ### Description, ### Steps to Reproduce, etc.
        if trimmed.starts_with("### ") || trimmed.starts_with("## ") {
            let header = trimmed.trim_start_matches('#').trim().to_uppercase();
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}", header),
                theme::title(),
            )));
            _current_section = header;
            continue;
        }

        // Checkbox lines: - [x] Dev, - [ ] Staging
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [ ]") {
            let checked = trimmed.starts_with("- [x]");
            let text = trimmed[5..].trim();
            let icon = if checked {
                theme::icon_checked()
            } else {
                theme::icon_unchecked()
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", icon, text),
                theme::normal(),
            )));
            continue;
        }

        // Image tags: <img ... src="url" />
        if trimmed.contains("<img") && trimmed.contains("src=") {
            if let Some(_url) = extract_img_src(trimmed) {
                let dims = extract_img_dims(trimmed);
                let dim_str = dims
                    .map(|(w, h)| format!(" ({}x{})", w, h))
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::styled(format!("  [img] image{}", dim_str), theme::highlight()),
                    Span::styled("   [w] open in browser", theme::dim()),
                ]));
            }
            continue;
        }

        // Video links (GitHub user-attachments)
        if trimmed.contains("user-attachments/assets") && !trimmed.contains("<img") {
            lines.push(Line::from(vec![
                Span::styled("  [vid] video attachment", theme::highlight()),
                Span::styled("   [w] open in browser", theme::dim()),
            ]));
            continue;
        }

        // Skip _No response_ placeholders
        if trimmed == "_No response_" {
            lines.push(Line::from(Span::styled("  --", theme::dim())));
            continue;
        }

        // Error messages (from failed fetch)
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {} Fetch Error", theme::icon_warning()),
                theme::warning(),
            )));
            for wrapped in word_wrap(inner, max_width) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", wrapped),
                    theme::dim(),
                )));
            }
            lines.push(Line::from(""));
            continue;
        }

        // Regular text - wrap to fit width
        if !trimmed.is_empty() {
            for wrapped in word_wrap(raw_line.trim(), max_width) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", wrapped),
                    theme::normal(),
                )));
            }
        }
    }

    lines
}

fn extract_img_src(s: &str) -> Option<String> {
    let start = s.find("src=\"")? + 5;
    let end = s[start..].find('"')? + start;
    Some(s[start..end].to_string())
}

fn extract_img_dims(s: &str) -> Option<(String, String)> {
    let w = {
        let start = s.find("width=\"")? + 7;
        let end = s[start..].find('"')? + start;
        s[start..end].to_string()
    };
    let h = {
        let start = s.find("height=\"")? + 8;
        let end = s[start..].find('"')? + start;
        s[start..end].to_string()
    };
    Some((w, h))
}
