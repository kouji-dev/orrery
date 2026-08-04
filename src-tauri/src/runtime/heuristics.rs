//! A0.3 — PTY-derived status heuristics, moved from the renderer into Rust.
//!
//! Tools without a usable permission hook (gemini) used to have their
//! working / needs-input state derived by parsing PTY text IN THE RENDERER
//! (terminal titles + a folded prompt tail). Under A0.2 a `none`-mode agent
//! ships no bytes to the renderer, which would silently break that parsing —
//! so the same heuristics now run here, next to the batcher, fed from the raw
//! stream, and only a tiny `agent://pty-status` event crosses to the frontend
//! on state TRANSITIONS. This is also the natural future home for the A5.4
//! loop detector (it already sees the raw stream).
//!
//! Ported from `src/app/utils.ts` (`detectTitleStatus`, `isAwaitingInput`,
//! `isPermissionPrompt`) and `agent-runtime.service.ts` (`liveState`, the
//! un-hooked branch). The regexes are ported as plain scans to avoid a regex
//! dependency for a handful of literals.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::digest;

/// Recency window for "output means working" and title freshness. Why 1500ms:
/// the renderer heuristic used the same figure — long enough to bridge chunk
/// gaps in a streaming answer, short enough that a real prompt-wait is
/// detected within ~2s.
const RECENT: Duration = Duration::from_millis(1500);

/// Re-evaluation cadence while the stream is quiet. Why 500ms: the falling
/// edge (working → idle/needs-input after RECENT of silence) can only be seen
/// by a timed re-check; 500ms keeps detection latency ≤ RECENT + 500ms at
/// negligible cost (one wakeup per agent, gemini-only).
const EVAL_EVERY: Duration = Duration::from_millis(500);

/// How many folded tail lines make the `detail` context. Why 5: mirrors the
/// renderer's promptTail (last 5 non-empty lines) that notifications showed.
const DETAIL_LINES: usize = 5;

/// What the frontend receives on `agent://pty-status` (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub id: String,
    pub working: bool,
    pub needs_input: bool,
    /// When `needs_input`: does the prompt read like a permission (y/n) ask
    /// rather than an open question?
    pub permission: bool,
    /// The folded prompt tail — notification detail / exit summary context.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleStatus {
    Working,
    Permission,
    Idle,
}

/// Classify an OSC terminal title (ported from `detectTitleStatus`).
/// None = no recognized signal (plain cwd) → fall back to output recency.
fn detect_title_status(title: &str) -> Option<TitleStatus> {
    if title.is_empty() {
        return None;
    }
    if title.contains('✋') {
        return Some(TitleStatus::Permission);
    }
    let braille = title.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c));
    if braille
        || title.contains('✦')
        || title.contains('⏲')
        || has_working_keyword(title)
    {
        return Some(TitleStatus::Working);
    }
    if title.contains('✳') || title.contains('◇') {
        return Some(TitleStatus::Idle);
    }
    None
}

/// "working" | "thinking" | "running" | "generating" as a standalone word —
/// not inside another word ("reworking") or a path ("~/codex/working").
fn has_working_keyword(title: &str) -> bool {
    let lower = title.to_lowercase();
    for kw in ["working", "thinking", "running", "generating"] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(kw) {
            let start = from + pos;
            let end = start + kw.len();
            let pre_ok = lower[..start]
                .chars()
                .next_back()
                .map(|c| !(c.is_alphanumeric() || "._/\\-".contains(c)))
                .unwrap_or(true);
            let post_ok = lower[end..]
                .chars()
                .next()
                .map(|c| !(c.is_alphanumeric() || c == '-' || c == '_'))
                .unwrap_or(true);
            if pre_ok && post_ok {
                return true;
            }
            from = end;
        }
    }
    false
}

/// Does the text read like a yes/no permission request (ported from
/// `isPermissionPrompt` — literal fragments instead of the regex)?
fn is_permission_prompt(text: &str) -> bool {
    let lower = text.to_lowercase();
    const NEEDLES: &[&str] = &[
        "y/n",
        "yes/no",
        "proceed",
        "do you want",
        "allow",
        "permission",
        "approve",
        "grant",
        "confirm",
        "continue?",
        "press y",
        "1. yes",
        "1) yes",
        "❯ 1",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

/// Does the folded tail read like the agent is blocked on the user (ported
/// from `isAwaitingInput`): a permission prompt, or a trailing question.
fn is_awaiting_input(tail: &str) -> bool {
    if tail.is_empty() {
        return false;
    }
    if is_permission_prompt(tail) {
        return true;
    }
    tail.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .next_back()
        .map(|l| l.trim_end().ends_with('?'))
        .unwrap_or(false)
}

/// Extract the LAST OSC window title (`ESC ] 0/2 ; title BEL|ST`) in `chunk`.
/// Chunk-local (a title split across two reads is missed) — the next repaint
/// re-sets it, so a missed frame self-heals; same tolerance the digest fold
/// accepts.
fn last_osc_title(chunk: &str) -> Option<String> {
    let bytes = chunk.as_bytes();
    let mut found: Option<String> = None;
    let mut i = 0;
    while i + 4 < bytes.len() {
        if bytes[i] == 0x1b
            && bytes[i + 1] == b']'
            && (bytes[i + 2] == b'0' || bytes[i + 2] == b'2')
            && bytes[i + 3] == b';'
        {
            let start = i + 4;
            let mut j = start;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    found = Some(String::from_utf8_lossy(&bytes[start..j]).into_owned());
                    break;
                }
                if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    found = Some(String::from_utf8_lossy(&bytes[start..j]).into_owned());
                    break;
                }
                j += 1;
            }
            i = j;
        }
        i += 1;
    }
    found
}

/// Pure per-agent heuristic state: feed it chunks, ask it for the live status.
pub struct Heuristics {
    /// Rolling plain-text fold of the stream (same fold as digest mode).
    screen: Vec<String>,
    title: Option<TitleStatus>,
    title_at: Option<Instant>,
    last_output_at: Option<Instant>,
}

impl Heuristics {
    pub fn new() -> Self {
        Self {
            screen: Vec::new(),
            title: None,
            title_at: None,
            last_output_at: None,
        }
    }

    /// Fold one raw chunk (title + tail extraction) at time `now`.
    pub fn push(&mut self, chunk: &str, now: Instant) {
        if let Some(title) = last_osc_title(chunk) {
            if let Some(s) = detect_title_status(&title) {
                self.title = Some(s);
                self.title_at = Some(now);
            }
        }
        digest::fold_lines(&mut self.screen, chunk, digest::FOLD_CAP);
        self.last_output_at = Some(now);
    }

    /// The folded prompt tail (last DETAIL_LINES non-empty lines, joined) —
    /// the same context the renderer's promptTail used to provide.
    pub fn tail(&self) -> String {
        digest::tail_lines(&self.screen, DETAIL_LINES).join("\n")
    }

    /// Current status snapshot — the port of the renderer's un-hooked
    /// `liveState` branch + `detectNeedsInput` classification.
    pub fn eval(&self, now: Instant, id: &str) -> StatusEvent {
        let recent = |t: Option<Instant>| t.map(|t| now.duration_since(t) < RECENT).unwrap_or(false);
        let output_recent = recent(self.last_output_at);
        let tail = self.tail();
        if self.title == Some(TitleStatus::Permission) {
            return StatusEvent {
                id: id.to_string(),
                working: false,
                needs_input: true,
                permission: true,
                detail: tail,
            };
        }
        let working = match self.title {
            Some(TitleStatus::Working) => recent(self.title_at) || output_recent,
            Some(TitleStatus::Idle) => false,
            _ => output_recent,
        };
        let needs_input = !working && is_awaiting_input(&tail);
        StatusEvent {
            id: id.to_string(),
            working,
            needs_input,
            permission: needs_input && is_permission_prompt(&tail),
            detail: tail,
        }
    }
}

impl Default for Heuristics {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread body for one un-hooked agent: consumes the batcher's chunks (a tee
/// off the same reader→batcher pipeline), re-evaluates every `EVAL_EVERY`
/// while quiet (the falling edge needs a timer, not just data), and calls
/// `emit` only on a state TRANSITION (working/needsInput/permission changed) —
/// so the event stays low-volume like the hook-driven `agent://status`.
/// Returns when the sender disconnects (PTY closed).
pub fn status_loop(rx: Receiver<String>, id: &str, mut emit: impl FnMut(&StatusEvent)) {
    let mut h = Heuristics::new();
    let mut last: Option<(bool, bool, bool)> = None;
    loop {
        match rx.recv_timeout(EVAL_EVERY) {
            Ok(chunk) => h.push(&chunk, Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let ev = h.eval(Instant::now(), id);
        let key = (ev.working, ev.needs_input, ev.permission);
        if last != Some(key) {
            last = Some(key);
            emit(&ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Instant {
        // A fixed base far enough in the past that subtracting is safe.
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    #[test]
    fn spinner_title_means_working() {
        let mut h = Heuristics::new();
        h.push("\x1b]0;⠋ gemini\x07thinking about it", at(0));
        let ev = h.eval(at(100), "g");
        assert!(ev.working);
        assert!(!ev.needs_input);
    }

    #[test]
    fn permission_glyph_title_raises_permission_immediately() {
        let mut h = Heuristics::new();
        h.push("\x1b]0;✋ approve tool\x07Allow shell command? (y/n)\r\n", at(0));
        let ev = h.eval(at(100), "g");
        assert!(!ev.working);
        assert!(ev.needs_input);
        assert!(ev.permission);
        assert!(ev.detail.contains("Allow shell command?"));
    }

    #[test]
    fn quiet_stream_with_trailing_question_becomes_a_question() {
        let mut h = Heuristics::new();
        h.push("thinking...\r\nApply this patch?\r\n", at(0));
        // Within the recency window: still working (output is fresh).
        assert!(h.eval(at(500), "g").working);
        // After the window: idle + a trailing '?' → needs input, open question.
        let ev = h.eval(at(2000), "g");
        assert!(!ev.working);
        assert!(ev.needs_input);
        assert!(!ev.permission, "a bare question is not a permission ask");
        assert_eq!(ev.detail, "thinking...\nApply this patch?");
    }

    #[test]
    fn quiet_yn_prompt_is_a_permission_ask() {
        let mut h = Heuristics::new();
        h.push("Run `rm -rf dist`? [y/N]\r\n", at(0));
        let ev = h.eval(at(2000), "g");
        assert!(ev.needs_input);
        assert!(ev.permission);
    }

    #[test]
    fn idle_title_overrides_output_recency() {
        let mut h = Heuristics::new();
        h.push("\x1b]0;◇ gemini\x07done.\r\n", at(0));
        let ev = h.eval(at(100), "g");
        assert!(!ev.working, "an idle glyph wins over fresh output");
    }

    #[test]
    fn working_keyword_matches_words_not_paths() {
        assert!(has_working_keyword("gemini is thinking"));
        assert!(has_working_keyword("Running tests"));
        assert!(!has_working_keyword("reworking the parser"));
        assert!(!has_working_keyword("~/codex/working"));
    }

    #[test]
    fn osc_title_extraction_takes_the_last_and_handles_st() {
        assert_eq!(
            last_osc_title("\x1b]0;first\x07mid\x1b]2;second\x1b\\tail"),
            Some("second".into())
        );
        assert_eq!(last_osc_title("no titles here"), None);
    }

    #[test]
    fn status_loop_emits_only_on_transitions() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let ev2 = events.clone();
        let h = std::thread::spawn(move || {
            status_loop(rx, "g", move |e| ev2.lock().unwrap().push(e.clone()));
        });
        tx.send("streaming...\r\n".into()).unwrap();
        tx.send("more output\r\n".into()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        {
            let log = events.lock().unwrap();
            assert_eq!(log.len(), 1, "two chunks, one WORKING transition: {log:?}");
            assert!(log[0].working);
        }
        drop(tx); // PTY closed → loop ends
        h.join().unwrap();
    }
}
