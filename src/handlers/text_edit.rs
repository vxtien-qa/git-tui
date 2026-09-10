//! Cursor-aware editing helpers for plain-`String` input fields.
//!
//! The cursor is a byte offset into the string, always kept on a `char`
//! boundary and clamped to the string length - safe for Vietnamese and any
//! other multibyte text.

/// Clamp a cursor to the string length and snap it back to a char boundary.
pub fn clamp(s: &str, cursor: usize) -> usize {
    let mut c = cursor.min(s.len());
    while c > 0 && !s.is_char_boundary(c) {
        c -= 1;
    }
    c
}

fn prev_boundary(s: &str, c: usize) -> usize {
    if c == 0 {
        return 0;
    }
    let mut p = c - 1;
    while p > 0 && !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

fn next_boundary(s: &str, c: usize) -> usize {
    if c >= s.len() {
        return s.len();
    }
    let mut n = c + 1;
    while n < s.len() && !s.is_char_boundary(n) {
        n += 1;
    }
    n
}

pub fn insert_char(s: &mut String, cursor: &mut usize, ch: char) {
    *cursor = clamp(s, *cursor);
    s.insert(*cursor, ch);
    *cursor += ch.len_utf8();
}

pub fn insert_str(s: &mut String, cursor: &mut usize, text: &str) {
    *cursor = clamp(s, *cursor);
    s.insert_str(*cursor, text);
    *cursor += text.len();
}

/// Remove the char before the cursor.
pub fn backspace(s: &mut String, cursor: &mut usize) {
    *cursor = clamp(s, *cursor);
    if *cursor == 0 {
        return;
    }
    let prev = prev_boundary(s, *cursor);
    s.replace_range(prev..*cursor, "");
    *cursor = prev;
}

/// Remove the char under the cursor (Delete key).
pub fn delete(s: &mut String, cursor: &mut usize) {
    *cursor = clamp(s, *cursor);
    if *cursor >= s.len() {
        return;
    }
    let next = next_boundary(s, *cursor);
    s.replace_range(*cursor..next, "");
}

pub fn move_left(s: &str, cursor: &mut usize) {
    *cursor = prev_boundary(s, clamp(s, *cursor));
}

pub fn move_right(s: &str, cursor: &mut usize) {
    *cursor = next_boundary(s, clamp(s, *cursor));
}

/// Home = start of the current line.
pub fn move_home(s: &str, cursor: &mut usize) {
    let c = clamp(s, *cursor);
    *cursor = s[..c].rfind('\n').map(|i| i + 1).unwrap_or(0);
}

/// End = end of the current line.
pub fn move_end(s: &str, cursor: &mut usize) {
    let c = clamp(s, *cursor);
    *cursor = s[c..].find('\n').map(|i| c + i).unwrap_or(s.len());
}

/// Byte offset of the `col`-th char in `line`, clamped to the line's end.
fn col_to_byte(line: &str, col: usize) -> usize {
    line.char_indices()
        .nth(col)
        .map(|(i, _)| i)
        .unwrap_or(line.len())
}

/// Up = same char column on the previous logical line. Returns false
/// (cursor untouched) when already on the first line, so callers can move
/// focus to the previous field instead.
pub fn move_up(s: &str, cursor: &mut usize) -> bool {
    let c = clamp(s, *cursor);
    let line_start = s[..c].rfind('\n').map(|i| i + 1).unwrap_or(0);
    if line_start == 0 {
        return false;
    }
    let col = s[line_start..c].chars().count();
    let prev_start = s[..line_start - 1].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let prev_line = &s[prev_start..line_start - 1];
    *cursor = prev_start + col_to_byte(prev_line, col);
    true
}

/// Down = same char column on the next logical line. Returns false
/// (cursor untouched) when already on the last line, so callers can move
/// focus to the next field instead.
pub fn move_down(s: &str, cursor: &mut usize) -> bool {
    let c = clamp(s, *cursor);
    let next_start = match s[c..].find('\n') {
        Some(i) => c + i + 1,
        None => return false,
    };
    let line_start = s[..c].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = s[line_start..c].chars().count();
    let next_end = s[next_start..]
        .find('\n')
        .map(|i| next_start + i)
        .unwrap_or(s.len());
    *cursor = next_start + col_to_byte(&s[next_start..next_end], col);
    true
}

/// The string with the cursor glyph inserted at the cursor position -
/// the renderers word-wrap this so the cursor is always visible mid-text.
pub fn with_cursor_glyph(s: &str, cursor: usize, glyph: &str) -> String {
    let c = clamp(s, cursor);
    let mut out = String::with_capacity(s.len() + glyph.len());
    out.push_str(&s[..c]);
    out.push_str(glyph);
    out.push_str(&s[c..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_move_multibyte() {
        // Vietnamese text: mixed 1- and 2/3-byte chars
        let mut s = String::from("lỗi");
        let mut c = s.len();
        insert_char(&mut s, &mut c, '!');
        assert_eq!(s, "lỗi!");

        move_left(&s, &mut c); // before '!'
        move_left(&s, &mut c); // before 'i'
        move_left(&s, &mut c); // before 'ỗ'
        insert_char(&mut s, &mut c, 'x');
        assert_eq!(s, "lxỗi!");
    }

    #[test]
    fn test_backspace_delete_multibyte() {
        let mut s = String::from("aỗb");
        let mut c = s.len(); // end
        backspace(&mut s, &mut c);
        assert_eq!(s, "aỗ");
        backspace(&mut s, &mut c);
        assert_eq!(s, "a");

        let mut s = String::from("ỗb");
        let mut c = 0;
        delete(&mut s, &mut c);
        assert_eq!(s, "b");
    }

    #[test]
    fn test_home_end_multiline() {
        let s = "first\nsecond line";
        let mut c = s.len(); // end of "second line"
        move_home(s, &mut c);
        assert_eq!(&s[c..c + 6], "second");
        move_end(s, &mut c);
        assert_eq!(c, s.len());

        let mut c = 3; // inside "first"
        move_end(s, &mut c);
        assert_eq!(c, 5); // before '\n'
        move_home(s, &mut c);
        assert_eq!(c, 0);
    }

    #[test]
    fn test_clamp_snaps_to_boundary() {
        let s = "ỗ"; // 3 bytes
        assert_eq!(clamp(s, 1), 0);
        assert_eq!(clamp(s, 2), 0);
        assert_eq!(clamp(s, 3), 3);
        assert_eq!(clamp(s, 99), 3);
    }

    #[test]
    fn test_move_up_down_preserves_column() {
        let s = "abcdef\nxy\nlong line";
        let mut c = s.len(); // col 9 on "long line"
        assert!(move_up(s, &mut c));
        assert_eq!(c, 9); // "xy" is short → clamped to its end
        assert!(move_up(s, &mut c));
        assert_eq!(c, 2); // col 2 on "abcdef"
        assert!(!move_up(s, &mut c)); // first line → field jump
        assert_eq!(c, 2); // untouched

        assert!(move_down(s, &mut c));
        assert_eq!(c, 9); // "xy" end (col 2 == len)
        assert!(move_down(s, &mut c));
        assert_eq!(c, 12); // col 2 on "long line"
        assert!(!move_down(s, &mut c)); // last line → field jump
        assert_eq!(c, 12);
    }

    #[test]
    fn test_move_up_down_multibyte() {
        let s = "lỗi\nabc"; // "lỗi" = 5 bytes, 3 chars
        let mut c = s.len(); // col 3 on "abc"
        assert!(move_up(s, &mut c));
        assert_eq!(c, 5); // end of "lỗi" (3 chars)
        assert!(move_down(s, &mut c));
        assert_eq!(c, s.len());
    }

    #[test]
    fn test_with_cursor_glyph() {
        assert_eq!(with_cursor_glyph("abc", 1, "|"), "a|bc");
        assert_eq!(with_cursor_glyph("abc", 3, "|"), "abc|");
    }
}
