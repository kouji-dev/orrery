//! A0.2 `digest` mode — minimal VT line folding in Rust.
//!
//! Folds raw PTY chunks into a small rolling screen of plain-text lines so the
//! overview mini-terminals can be fed "last N rendered lines" at 1Hz WITHOUT
//! shipping the full stream to the renderer (roadmap open decision 7: last N
//! rendered lines, via a line-level fold — not a full terminal grid).
//!
//! Handled: `\n`, `\r` (return to column 0 → subsequent text overwrites the
//! line), `\t`, CSI erase-in-line (`K`) and erase-in-display (`J`, incl. the
//! full-clear `2J`/`3J` that kills a stale TUI frame). Every other escape
//! (SGR colors, cursor addressing, OSC titles, 2-char escapes) is stripped —
//! a 5-line glanceable preview does not need cursor-accurate repaints, and
//! full-frame TUIs clear before redrawing anyway.

/// Rolling fold-buffer cap. Why 40: enough working set that a TUI's redraw
/// (clears + rewrites) still leaves ≥5 real content lines behind, while the
/// per-agent digest state stays a few KB.
pub const FOLD_CAP: usize = 40;

/// Fold one raw PTY chunk into `lines` (a rolling screen, oldest first,
/// bounded at `max` lines — oldest dropped past the cap).
pub fn fold_lines(lines: &mut Vec<String>, chunk: &str, max: usize) {
    if lines.is_empty() {
        lines.push(String::new());
    }
    // Cursor: always the LAST line; `col` is the overwrite position within it.
    let mut col = lines.last().map(String::len).unwrap_or(0);

    let mut chars = chunk.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                // Escape sequence: consume it; act only on CSI K / J finals.
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        let mut params = String::new();
                        let mut fin = None;
                        for e in chars.by_ref() {
                            if ('\u{40}'..='\u{7e}').contains(&e) {
                                fin = Some(e);
                                break;
                            }
                            params.push(e);
                        }
                        match fin {
                            Some('K') => {
                                // erase in line: 0/none = to end, 2 = whole line
                                let line = lines.last_mut().expect("non-empty");
                                if params == "2" {
                                    line.clear();
                                    col = 0;
                                } else if params != "1" {
                                    truncate_chars(line, col);
                                }
                            }
                            Some('J') if params == "2" || params == "3" => {
                                // clear screen → reset (kills a stale TUI frame)
                                lines.clear();
                                lines.push(String::new());
                                col = 0;
                            }
                            _ => {} // SGR / cursor moves / others: dropped
                        }
                    }
                    Some(']') => {
                        // OSC: skip to BEL or ST
                        chars.next();
                        let mut prev = '\0';
                        for e in chars.by_ref() {
                            if e == '\x07' || (prev == '\x1b' && e == '\\') {
                                break;
                            }
                            prev = e;
                        }
                    }
                    _ => {
                        chars.next(); // 2-char escape — drop
                    }
                }
            }
            '\n' => {
                lines.push(String::new());
                col = 0;
                if lines.len() > max {
                    let overflow = lines.len() - max;
                    lines.drain(..overflow);
                }
            }
            '\r' => col = 0,
            '\t' => {
                write_at(lines.last_mut().expect("non-empty"), col, "  ");
                col += 2;
            }
            c if c >= ' ' => {
                let mut s = String::new();
                s.push(c);
                write_at(lines.last_mut().expect("non-empty"), col, &s);
                col += 1;
            }
            _ => {} // stray control chars (\x07, \x08, …) dropped
        }
    }
}

/// The last `n` NON-EMPTY trimmed lines — what the digest event actually ships.
pub fn tail_lines(lines: &[String], n: usize) -> Vec<String> {
    let mut out: Vec<String> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if out.len() > n {
        out.drain(..out.len() - n);
    }
    out
}

/// Overwrite `text` into `line` at char position `col` (padding with spaces
/// when the cursor is past the end) — the `\r`-overwrite behavior a progress
/// bar relies on.
fn write_at(line: &mut String, col: usize, text: &str) {
    let chars: Vec<char> = line.chars().collect();
    let mut next = String::with_capacity(line.len() + text.len());
    next.extend(chars.iter().take(col));
    for _ in chars.len()..col {
        next.push(' ');
    }
    next.push_str(text);
    let written = text.chars().count();
    next.extend(chars.iter().skip(col + written));
    *line = next;
}

/// Truncate `line` to `col` CHARS (not bytes — a byte-index truncate can panic
/// mid-UTF-8-char).
fn truncate_chars(line: &mut String, col: usize) {
    if let Some((idx, _)) = line.char_indices().nth(col) {
        line.truncate(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(chunks: &[&str]) -> Vec<String> {
        let mut lines = Vec::new();
        for c in chunks {
            fold_lines(&mut lines, c, FOLD_CAP);
        }
        lines
    }

    #[test]
    fn plain_lines_accumulate_and_tail_takes_last_nonempty() {
        let lines = fold(&["one\r\ntwo\r\n\r\nthree\r\nfour\r\n"]);
        assert_eq!(tail_lines(&lines, 2), vec!["three", "four"]);
        assert_eq!(
            tail_lines(&lines, 10),
            vec!["one", "two", "three", "four"],
            "blank rows are skipped, order kept"
        );
    }

    #[test]
    fn carriage_return_overwrites_like_a_progress_bar() {
        let lines = fold(&["10%\r50%\r100% done\r\n"]);
        assert_eq!(tail_lines(&lines, 5), vec!["100% done"]);
    }

    #[test]
    fn erase_line_then_redraw_shows_only_the_final_text() {
        let lines = fold(&["downloading\x1b[2K\rdownloaded\r\n"]);
        assert_eq!(tail_lines(&lines, 5), vec!["downloaded"]);
    }

    #[test]
    fn clear_screen_resets_the_fold_buffer() {
        let lines = fold(&["old frame line\r\n", "\x1b[2Jnew frame"]);
        assert_eq!(tail_lines(&lines, 5), vec!["new frame"]);
    }

    #[test]
    fn sgr_colors_and_osc_titles_are_stripped() {
        let lines = fold(&["\x1b]0;my title\x07\x1b[31mred\x1b[0m text\r\n"]);
        assert_eq!(tail_lines(&lines, 5), vec!["red text"]);
    }

    #[test]
    fn fold_buffer_is_bounded() {
        let mut lines = Vec::new();
        for i in 0..200 {
            fold_lines(&mut lines, &format!("line {i}\n"), FOLD_CAP);
        }
        assert!(lines.len() <= FOLD_CAP, "cap holds: {}", lines.len());
        assert_eq!(tail_lines(&lines, 1), vec!["line 199"], "newest survives");
    }

    #[test]
    fn multibyte_text_with_erase_does_not_panic() {
        let lines = fold(&["héllo wörld\r\x1b[Kok\r\n"]);
        assert_eq!(tail_lines(&lines, 5), vec!["ok"]);
    }
}
