//! Reads the agent's conversation transcript to mirror the LATEST message content
//! in the overview card — the same prose / thinking / tool use the user sees in
//! the live terminal, but pulled straight from the source of truth.
//!
//! Every Claude Code hook payload carries a `transcript_path`: the session JSONL
//! (one JSON object per line, append-only). We tail-read it (last ~64 KB) and walk
//! lines from the END to find the most recent `assistant` message, then render its
//! content blocks into a short, multi-line snippet. Kept dependency-light
//! (serde_json only) and bounded so it stays cheap on the hot hook path.
//!
//! Transcript line schema (confirmed against real CC 2.1.x transcripts):
//!   {"type":"assistant","message":{"role":"assistant","content":[ <block>… ]}}
//! where each block is one of:
//!   {"type":"text","text":"…"}
//!   {"type":"thinking","thinking":"…","signature":"…"}
//!   {"type":"tool_use","id":…,"name":"Bash","input":{…},"caller":…}
//! `message.content` may also be a plain string (simple user turns). Other line
//! types (system / attachment / file-history-snapshot / user / result …) and any
//! malformed line are skipped.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use serde_json::Value;

/// How much of the transcript tail to read. A single assistant turn (a few text /
/// thinking / tool_use blocks) fits comfortably; bounded so the hook path is fast
/// regardless of how large the session file has grown.
const TAIL_BYTES: u64 = 64 * 1024;
/// Max lines rendered into the snippet (mirrors the 3-row card preview).
const MAX_LINES: usize = 3;
/// Hard cap on the whole snippet so a giant tool input can't blow up the event.
const MAX_TOTAL: usize = 240;
/// Per-line cap so one long paragraph doesn't eat the whole budget.
const MAX_LINE: usize = 120;

/// Best-effort: the latest assistant message content from the hook payload's
/// transcript, rendered as a concise snippet. `None` when there is no
/// `transcript_path`, the file is unreadable, or nothing useful was found.
pub fn latest_content(payload: &Value) -> Option<String> {
    let path = match payload.get("transcript_path").and_then(Value::as_str) {
        Some(p) => p,
        None => {
            // No transcript wiring on this event — the preview will fall back to the
            // structured `summarize()` (e.g. "▸ Bash…" is then NOT from the transcript).
            log::debug!("transcript: no transcript_path in payload — using summarize() fallback");
            return None;
        }
    };
    let raw = match read_tail(path) {
        Some(r) => r,
        None => {
            log::warn!("transcript: could not read transcript_path={path} (missing/locked) — using summarize() fallback");
            return None;
        }
    };
    match latest_from_str(&raw) {
        Some(s) => Some(s),
        None => {
            log::warn!("transcript: no renderable assistant content parsed from transcript_path={path} — using summarize() fallback");
            None
        }
    }
}

/// Pure core (testable without a file): parse a transcript chunk and render the
/// latest assistant message's content. Walks lines from the END so we stop at the
/// first (most recent) assistant message.
fn latest_from_str(raw: &str) -> Option<String> {
    // The first line of a tail read may be a partial JSON fragment — that's fine,
    // it simply fails to parse and is skipped.
    for line in raw.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // malformed / partial line — skip
        };
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(snippet) = render_message(&v["message"]) {
            return Some(snippet);
        }
        // An assistant line with no renderable content — keep looking further back.
    }
    None
}

/// Render an assistant `message` into up to `MAX_LINES` trimmed lines, preferring
/// the LATEST content block first (so the card shows the most recent activity),
/// total length capped at `MAX_TOTAL`.
fn render_message(message: &Value) -> Option<String> {
    let content = message.get("content")?;

    // `content` may be a plain string (rare for assistant, but handle it).
    if let Some(s) = content.as_str() {
        let line = clip(s.split_whitespace().collect::<Vec<_>>().join(" "), MAX_LINE);
        return (!line.is_empty()).then(|| clip(line, MAX_TOTAL));
    }

    let blocks = content.as_array()?;
    let mut lines: Vec<String> = Vec::new();
    // Prefer the latest block: iterate blocks newest-first.
    for block in blocks.iter().rev() {
        if let Some(line) = render_block(block) {
            if !line.is_empty() {
                lines.push(line);
            }
        }
        if lines.len() >= MAX_LINES {
            break;
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(clip(lines.join("\n"), MAX_TOTAL))
}

/// One content block → one snippet line. `None` for block types we don't surface.
fn render_block(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str)? {
        "text" => {
            let t = block.get("text").and_then(Value::as_str)?;
            let t = collapse_ws(t);
            (!t.is_empty()).then(|| clip(t, MAX_LINE))
        }
        "thinking" => {
            let t = block.get("thinking").and_then(Value::as_str)?;
            let t = collapse_ws(t);
            (!t.is_empty()).then(|| clip(format!("💭 {t}"), MAX_LINE))
        }
        "tool_use" => {
            let name = block.get("name").and_then(Value::as_str)?;
            let hint = tool_input_hint(block.get("input"));
            let line = match hint {
                Some(h) => format!("▸ {name}: {h}"),
                None => format!("▸ {name}"),
            };
            Some(clip(line, MAX_LINE))
        }
        _ => None,
    }
}

/// A tiny, human-readable hint from a tool's `input` — the single most telling
/// field (command / path / pattern / url / query), best-effort across tools.
fn tool_input_hint(input: Option<&Value>) -> Option<String> {
    let obj = input?.as_object()?;
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "query",
        "url",
        "skill",
        "name",
        "prompt",
    ] {
        if let Some(s) = obj.get(key).and_then(Value::as_str) {
            let s = collapse_ws(s);
            if !s.is_empty() {
                return Some(clip(s, 80));
            }
        }
    }
    None
}

/// Collapse all runs of whitespace (incl. newlines) to single spaces, trimmed —
/// so a multi-line block becomes one clean line.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate to `max` CHARACTERS (not bytes — keeps multibyte text valid), adding
/// an ellipsis when clipped.
fn clip(s: impl Into<String>, max: usize) -> String {
    let s = s.into();
    if s.chars().count() <= max {
        return s;
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Read the last `TAIL_BYTES` of a file as a lossy UTF-8 string. Small files are
/// read whole. `None` on any IO error (missing / locked file). The first line of
/// the returned chunk may be a partial fragment — callers tolerate that.
fn read_tail(path: &str) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    if start > 0 {
        f.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buf = Vec::with_capacity(TAIL_BYTES.min(len) as usize);
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write `content` to a temp file and return a payload `{transcript_path: …}`.
    fn payload_with_transcript(content: &str) -> (Value, tempfile::TempPath) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        let path = f.into_temp_path();
        let payload = serde_json::json!({
            "transcript_path": path.to_str().unwrap(),
            "hook_event_name": "PostToolUse",
        });
        (payload, path)
    }

    const SAMPLE: &str = concat!(
        r#"{"type":"system","subtype":"bridge_status","content":"active"}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"fix the bug"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Let me look at the file first","signature":"x"},{"type":"text","text":"I'll start by reading main.rs"}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Now running the tests to confirm"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test --lib","description":"run tests"},"caller":"x"}]}}"#,
        "\n",
    );

    #[test]
    fn extracts_latest_assistant_tool_use_first() {
        let (payload, _keep) = payload_with_transcript(SAMPLE);
        let out = latest_content(&payload).expect("some content");
        // Latest assistant message is the second one; latest block in it is the
        // tool_use, so it leads.
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "▸ Bash: cargo test --lib", "got: {out:?}");
        // The text block from the SAME (latest) message follows.
        assert_eq!(lines[1], "Now running the tests to confirm");
        // It must NOT include the earlier assistant message's content.
        assert!(
            !out.contains("read main.rs"),
            "leaked older message: {out:?}"
        );
    }

    #[test]
    fn renders_text_and_thinking_blocks() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"considering options"},{"type":"text","text":"Here is the plan"}]}}"#,
            "\n"
        );
        let out = latest_from_str(jsonl).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "Here is the plan"); // latest block first
        assert_eq!(lines[1], "💭 considering options");
    }

    #[test]
    fn caps_lines_to_max() {
        // 4 blocks but only the latest MAX_LINES (3) are kept, newest-first.
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":["#,
            r#"{"type":"text","text":"a"},{"type":"text","text":"b"},"#,
            r#"{"type":"text","text":"c"},{"type":"text","text":"d"}]}}"#,
            "\n"
        );
        let out = latest_from_str(jsonl).unwrap();
        assert_eq!(out.lines().count(), MAX_LINES, "got: {out:?}");
        assert_eq!(out, "d\nc\nb"); // newest blocks first, oldest ("a") dropped
    }

    #[test]
    fn clips_long_line_and_total_length() {
        // A single very long text block (the latest block) is clipped per-line and
        // the whole snippet stays under the total cap.
        let long = "x".repeat(500);
        let jsonl = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{long}"}}]}}}}"#
        );
        let out = latest_from_str(&jsonl).unwrap();
        assert!(
            out.chars().count() <= MAX_TOTAL,
            "too long: {}",
            out.chars().count()
        );
        assert!(out.ends_with('…'), "long line should be clipped: {out:?}");
    }

    #[test]
    fn skips_non_assistant_and_finds_most_recent() {
        // Trailing user line after the assistant must not hide the assistant msg.
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello there"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"thanks"}}"#,
            "\n",
            r#"{"type":"file-history-snapshot","snapshot":{}}"#,
            "\n"
        );
        assert_eq!(latest_from_str(jsonl).unwrap(), "hello there");
    }

    #[test]
    fn tolerates_partial_first_line_and_garbage() {
        // Simulate a tail that begins mid-line, plus a garbage line.
        let jsonl = concat!(
            r#"ng":"a partial fragment from a cut JSON object"}]}}"#,
            "\n",
            "not json at all\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"recovered"}]}}"#,
            "\n"
        );
        assert_eq!(latest_from_str(jsonl).unwrap(), "recovered");
    }

    #[test]
    fn none_when_no_transcript_path() {
        let payload = serde_json::json!({ "tool_name": "Bash" });
        assert!(latest_content(&payload).is_none());
    }

    #[test]
    fn none_when_unreadable_path() {
        let payload = serde_json::json!({ "transcript_path": "Z:/no/such/file.jsonl" });
        assert!(latest_content(&payload).is_none());
    }

    #[test]
    fn none_when_nothing_useful() {
        // Only non-assistant lines / empty assistant content → nothing to show.
        let jsonl = concat!(
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[]}}"#,
            "\n",
            r#"{"type":"system","content":"x"}"#,
            "\n"
        );
        assert!(latest_from_str(jsonl).is_none());
    }

    #[test]
    fn handles_string_content() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":"a plain   string  reply"}}"#,
            "\n"
        );
        assert_eq!(latest_from_str(jsonl).unwrap(), "a plain string reply");
    }

    #[test]
    fn tool_use_without_known_input_falls_back_to_name() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"x","name":"EnterWorktree","input":{"foo":1}}]}}"#,
            "\n"
        );
        assert_eq!(latest_from_str(jsonl).unwrap(), "▸ EnterWorktree");
    }
}
