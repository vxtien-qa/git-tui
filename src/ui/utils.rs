use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use super::theme;

/// Helper to create a centered rectangle within a given area.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Truncate a string to `max` display-width columns, appending `..` if truncated.
/// Uses unicode-width for proper CJK wide-character handling.
pub fn trunc(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let display_w = UnicodeWidthStr::width(s);
    if display_w <= max {
        return s.to_string();
    }
    // Need to truncate - walk chars and accumulate width
    let mut result = String::new();
    let mut w = 0;
    let target = max.saturating_sub(2); // room for ".."
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > target {
            break;
        }
        result.push(ch);
        w += cw;
    }
    result.push_str("..");
    result
}

/// Word-wrap text into lines of max `width` display-width columns, breaking at
/// word boundaries. Words wider than `width` (long URLs, tokens) are
/// hard-broken at character boundaries instead of silently overflowing.
/// Uses unicode-width for proper CJK wide-character handling.
pub fn word_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_w: usize = 0;

    let flush = |current: &mut String, current_w: &mut usize, lines: &mut Vec<String>| {
        if !current.is_empty() {
            lines.push(std::mem::take(current));
            *current_w = 0;
        }
    };

    for word in text.split_whitespace() {
        let word_w = UnicodeWidthStr::width(word);

        // Oversized word: hard-break into width-sized chunks.
        if word_w > width {
            flush(&mut current, &mut current_w, &mut lines);
            let mut chunk = String::new();
            let mut chunk_w = 0;
            for ch in word.chars() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if chunk_w + cw > width && !chunk.is_empty() {
                    lines.push(std::mem::take(&mut chunk));
                    chunk_w = 0;
                }
                chunk.push(ch);
                chunk_w += cw;
            }
            // Leftover chunk starts the next line so following words can join it.
            current = chunk;
            current_w = chunk_w;
            continue;
        }

        if current.is_empty() {
            current = word.to_string();
            current_w = word_w;
        } else if current_w + 1 + word_w <= width {
            current.push(' ');
            current.push_str(word);
            current_w += 1 + word_w;
        } else {
            lines.push(current);
            current = word.to_string();
            current_w = word_w;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Display width in terminal columns (Unicode-aware).
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate from the LEFT, keeping the end of the string.
/// Used by the breadcrumb, where the tail is the part that matters.
pub fn trunc_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(s) <= max {
        return s.to_string();
    }
    let target = max.saturating_sub(2);
    let mut kept: Vec<char> = Vec::new();
    let mut w = 0;
    for ch in s.chars().rev() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > target {
            break;
        }
        kept.push(ch);
        w += cw;
    }
    kept.reverse();
    format!("..{}", kept.into_iter().collect::<String>())
}

/// Downgrade the marks used in action messages when the terminal cannot
/// render Unicode.
///
/// Result messages are assembled in background threads all over the handlers,
/// so the marks are baked into their strings. Sanitising once at draw time
/// keeps those call sites plain while still honouring GIT_TUI_UNICODE=0 and
/// terminals without Unicode support (v2.3.2 centralised the UI icons but the
/// handler messages kept their hardcoded glyphs).
pub fn sanitize_glyphs(s: &str) -> String {
    if theme::supports_unicode() {
        return s.to_string();
    }
    s.replace('✓', "[OK]")
        .replace('✗', "[X]")
        .replace('⚠', "[!]")
        .replace('→', "->")
        .replace('←', "<-")
        .replace('×', "x")
        .replace('•', "*")
        .replace('…', "...")
}

/// Build a footer hint line that always fits `width`.
///
/// Hints are laid out from the left while they fit, and the LAST hint is
/// always kept, because it is the way out of the screen (Esc). When anything
/// had to be dropped, a `?:Help` marker takes its place, so the full key list
/// stays one keypress away instead of being silently clipped off the right
/// edge - which is what happened to every long footer below ~100 columns.
pub fn footer_line<'a>(hints: &[(&str, &str)], width: usize) -> Line<'a> {
    const GAP: usize = 2;
    let hint_w = |k: &str, l: &str| UnicodeWidthStr::width(k) + 1 + UnicodeWidthStr::width(l);

    let Some((last, head)) = hints.split_last() else {
        return Line::from("");
    };
    let avail = width.saturating_sub(2); // leading + trailing space
    let marker = ("?", "Help");

    let mut items: Vec<(String, String)> = Vec::new();
    let mut used = hint_w(last.0, last.1);
    let mut all_fit = true;
    for (k, l) in head {
        let w = hint_w(k, l) + GAP;
        if used + w <= avail {
            items.push((k.to_string(), l.to_string()));
            used += w;
        } else {
            all_fit = false;
            break;
        }
    }
    // Only skip the marker when the Help hint actually SURVIVED the fit: it is
    // usually near the end of the list, so it is among the first things dropped.
    let help_shown = items.iter().any(|(k, _)| k == marker.0) || last.0 == marker.0;
    if !all_fit && !help_shown {
        let marker_w = hint_w(marker.0, marker.1) + GAP;
        while !items.is_empty() && used + marker_w > avail {
            if let Some((k, l)) = items.pop() {
                used -= hint_w(&k, &l) + GAP;
            }
        }
        if used + marker_w <= avail {
            items.push((marker.0.to_string(), marker.1.to_string()));
        }
    }
    items.push((last.0.to_string(), last.1.to_string()));

    let mut spans = vec![Span::raw(" ")];
    for (i, (key, label)) in items.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key, theme::highlight()));
        spans.push(Span::styled(format!(":{}", label), theme::dim()));
    }
    Line::from(spans)
}

/// Join `parts` with `sep`, dropping trailing parts until the result fits.
///
/// For header lines that pack several facts: truncating the joined string mid
/// word ("3 total | 2 i..") tells the user nothing, whereas dropping the least
/// important fact keeps every visible fact readable.
pub fn fit_parts(parts: &[String], sep: &str, width: usize) -> String {
    let mut kept: Vec<&String> = parts.iter().collect();
    while kept.len() > 1 {
        let joined = kept
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(sep);
        if UnicodeWidthStr::width(joined.as_str()) <= width {
            return joined;
        }
        kept.pop();
    }
    let first = kept.first().map(|s| s.as_str()).unwrap_or("");
    trunc(first, width)
}

/// Position indicator for a scrollable list, e.g. " 12/48 ".
/// Rendered into the block title so "where am I" is always answerable.
pub fn position_title(selected: usize, total: usize) -> String {
    if total == 0 {
        " 0/0 ".to_string()
    } else {
        format!(" {}/{} ", (selected + 1).min(total), total)
    }
}

/// Integer percentage of `part` in `whole`, and 0 when `whole` is 0.
pub fn percent(part: usize, whole: usize) -> usize {
    (part * 100).checked_div(whole).unwrap_or(0)
}

/// `part` of `whole` scaled onto `cells`, and 0 when `whole` is 0.
pub fn scale_to(part: usize, whole: usize, cells: usize) -> usize {
    (part * cells).checked_div(whole).unwrap_or(0)
}

/// Render a multi-segment progress bar as a Vec<Span>.
/// Segments: (filled_count, total, style_fn) pairs.
/// Returns spans: [ "[", segment1, segment2, ..., remaining, "] XX%" ]
pub fn progress_bar_spans<'a>(
    bar_w: usize,
    total: usize,
    segments: &[(usize, ratatui::style::Style)],
) -> Vec<Span<'a>> {
    let mut spans = vec![Span::raw("  [")];
    let mut used = 0;

    for (count, style) in segments {
        let filled = scale_to(*count, total, bar_w);
        let clamped = filled.min(bar_w - used);
        if clamped > 0 {
            spans.push(Span::styled(theme::icon_bar_full().repeat(clamped), *style));
        }
        used += clamped;
    }

    let remaining = bar_w.saturating_sub(used);
    if remaining > 0 {
        spans.push(Span::styled(
            theme::icon_bar_empty().repeat(remaining),
            theme::dim(),
        ));
    }

    let pct = segments
        .first()
        .map(|(c, _)| percent(*c, total))
        .unwrap_or(0);
    spans.push(Span::styled(format!("] {}%", pct), theme::normal()));
    spans
}

/// Render progress bar legend.
pub fn progress_bar_legend<'a>() -> Line<'a> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(theme::icon_bar_legend(), theme::success()),
        Span::styled(" Done  ", theme::dim()),
        Span::styled(theme::icon_bar_legend(), theme::highlight()),
        Span::styled(" QA  ", theme::dim()),
        Span::styled(theme::icon_bar_legend(), theme::normal()),
        Span::styled(" Progress  ", theme::dim()),
        Span::styled(theme::icon_bar_legend(), theme::dim()),
        Span::styled(" Other", theme::dim()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_wrap_basic() {
        let lines = word_wrap("hello world foo", 11);
        assert_eq!(lines, vec!["hello world", "foo"]);
    }

    #[test]
    fn test_word_wrap_hard_breaks_long_words() {
        let lines = word_wrap("see https://example.com/very/long/path/here ok", 12);
        assert!(
            lines
                .iter()
                .all(|l| UnicodeWidthStr::width(l.as_str()) <= 12),
            "no line may exceed the width: {:?}",
            lines
        );
        // The URL must survive intact when re-joined.
        let joined: String = lines.join("");
        assert!(joined.contains("example.com"));
    }

    #[test]
    fn test_footer_line_always_keeps_the_way_out() {
        let hints = [
            ("Up/Dn", "Nav"),
            ("Enter", "Detail"),
            ("p", "QA Actions"),
            ("f", "Fail"),
            ("m", "Move"),
            ("s", "Sprint"),
            ("Esc", "Back"),
        ];
        for width in [20usize, 40, 60, 80, 200] {
            let line = footer_line(&hints, width);
            let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
            assert!(
                UnicodeWidthStr::width(text.as_str()) <= width,
                "footer overflows at width {}: {:?}",
                width,
                text
            );
            assert!(
                text.contains("Esc"),
                "the Back hint must survive at width {}: {:?}",
                width,
                text
            );
        }
        // Narrow: the overflow marker points at the full key list.
        let narrow: String = footer_line(&hints, 40)
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(
            narrow.contains("?:Help"),
            "a clipped footer must point at Help: {:?}",
            narrow
        );

        // Wide enough: everything is shown, no overflow marker.
        let wide: String = footer_line(&hints, 200)
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(wide.contains("QA Actions") && !wide.contains("?:Help"));
    }

    #[test]
    fn test_position_title() {
        assert_eq!(position_title(0, 48), " 1/48 ");
        assert_eq!(position_title(47, 48), " 48/48 ");
        assert_eq!(position_title(0, 0), " 0/0 ");
    }

    #[test]
    fn test_word_wrap_empty() {
        assert_eq!(word_wrap("", 10), vec![String::new()]);
    }
}
