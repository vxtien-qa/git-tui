use ratatui::style::{Color, Modifier, Style};

/// Whether we're on a platform that supports Unicode well.
/// Non-Windows terminals are assumed capable; on Windows we detect the modern
/// terminals that render Unicode fine (Windows Terminal, VS Code, Warp,
/// Alacritty, WezTerm, ConEmu). `GIT_TUI_UNICODE=1|0` overrides the detection.
pub fn supports_unicode() -> bool {
    match std::env::var("GIT_TUI_UNICODE").as_deref() {
        Ok("1") | Ok("true") => return true,
        Ok("0") | Ok("false") => return false,
        _ => {}
    }
    if !cfg!(target_os = "windows") {
        return true;
    }
    std::env::var("WT_SESSION").is_ok()
        || std::env::var("TERM_PROGRAM").is_ok() // vscode, WarpTerminal, ...
        || std::env::var("WEZTERM_EXECUTABLE").is_ok()
        || std::env::var("ALACRITTY_WINDOW_ID").is_ok()
        || std::env::var("ConEmuANSI").map(|v| v == "ON").unwrap_or(false)
}

// ── Color Palette ─────────────────────────────────────────────

pub const BG: Color = Color::Rgb(22, 22, 30);
pub const TEXT: Color = Color::Rgb(205, 214, 244);
pub const SUBTEXT: Color = Color::Rgb(147, 153, 178);
pub const BORDER: Color = Color::Rgb(69, 71, 90);
pub const ACCENT: Color = Color::Rgb(137, 180, 250); // blue
pub const GREEN: Color = Color::Rgb(166, 227, 161);
pub const RED: Color = Color::Rgb(243, 139, 168);
pub const YELLOW: Color = Color::Rgb(249, 226, 175);
pub const PEACH: Color = Color::Rgb(250, 179, 135);

// ── Styles ────────────────────────────────────────────────────

pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn normal() -> Style {
    Style::default().fg(TEXT)
}

pub fn dim() -> Style {
    Style::default().fg(SUBTEXT)
}

pub fn selected() -> Style {
    Style::default()
        .fg(BG)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::default().fg(BORDER)
}

pub fn success() -> Style {
    Style::default().fg(GREEN)
}

pub fn error() -> Style {
    Style::default().fg(RED)
}

pub fn warning() -> Style {
    Style::default().fg(YELLOW)
}

pub fn highlight() -> Style {
    Style::default().fg(PEACH)
}

pub fn priority_style(priority: &str) -> Style {
    match priority {
        "P0" => Style::default().fg(RED).add_modifier(Modifier::BOLD),
        "P1" => Style::default().fg(RED),
        "P2" => Style::default().fg(YELLOW),
        "P3" => Style::default().fg(SUBTEXT),
        "P4" => dim(),
        _ => dim(),
    }
}

/// One semantic color per status, used consistently on the board, dashboard
/// and detail views (previously each screen picked its own color).
pub fn status_style(status: &crate::models::status::Status) -> Style {
    use crate::models::status::Status;
    match status {
        Status::Blocked => warning(),
        Status::InQADev | Status::InQA | Status::InUAT => Style::default().fg(PEACH),
        Status::TechComplete | Status::ReadyForRelease | Status::Done => success(),
        Status::InProgress | Status::InReview => Style::default().fg(ACCENT),
        Status::Backlog | Status::ReadyForDev => dim(),
    }
}

/// Style for a ticket type - one color everywhere (board, My Tasks, Search).
pub fn item_kind_style(kind: crate::models::item::ItemKind) -> Style {
    use crate::models::item::ItemKind;
    match kind {
        ItemKind::Bug => error(),
        ItemKind::Enhancement => Style::default().fg(ACCENT),
        ItemKind::Task => success(),
    }
}

// ── Icons (ASCII fallback for Windows) ────────────────────────

pub fn icon_ok() -> &'static str {
    if supports_unicode() {
        "✓"
    } else {
        "[OK]"
    }
}

pub fn icon_arrow() -> &'static str {
    if supports_unicode() {
        "▸"
    } else {
        ">"
    }
}

pub fn icon_checked() -> &'static str {
    if supports_unicode() {
        "☑"
    } else {
        "[x]"
    }
}

pub fn icon_unchecked() -> &'static str {
    if supports_unicode() {
        "☐"
    } else {
        "[ ]"
    }
}

pub fn icon_fail() -> &'static str {
    if supports_unicode() {
        "✗"
    } else {
        "[X]"
    }
}

pub fn icon_cursor() -> &'static str {
    if supports_unicode() {
        "▏"
    } else {
        "|"
    }
}

pub fn icon_left_right() -> &'static str {
    if supports_unicode() {
        "◄►"
    } else {
        "<>"
    }
}

pub fn icon_bullet() -> &'static str {
    if supports_unicode() {
        "●"
    } else {
        "(*)"
    }
}

pub fn icon_bullet_empty() -> &'static str {
    if supports_unicode() {
        "○"
    } else {
        "( )"
    }
}

pub fn icon_up_down() -> &'static str {
    if supports_unicode() {
        "↑↓"
    } else {
        "Up/Dn"
    }
}

pub fn icon_left_arrow() -> &'static str {
    if supports_unicode() {
        "←"
    } else {
        "<-"
    }
}

pub fn icon_right_arrow() -> &'static str {
    if supports_unicode() {
        "→"
    } else {
        "->"
    }
}

pub fn icon_warning() -> &'static str {
    if supports_unicode() {
        "⚠"
    } else {
        "[!]"
    }
}

pub fn icon_spinner() -> &'static str {
    if supports_unicode() {
        "⟳"
    } else {
        "..."
    }
}

pub fn icon_bar_full() -> &'static str {
    if supports_unicode() {
        "█"
    } else {
        "#"
    }
}

pub fn icon_bar_empty() -> &'static str {
    if supports_unicode() {
        "░"
    } else {
        "-"
    }
}

pub fn icon_bar_legend() -> &'static str {
    if supports_unicode() {
        "■"
    } else {
        "#"
    }
}

pub fn icon_h_line() -> &'static str {
    if supports_unicode() {
        "─"
    } else {
        "-"
    }
}

pub fn icon_h_double() -> &'static str {
    if supports_unicode() {
        "═"
    } else {
        "="
    }
}
