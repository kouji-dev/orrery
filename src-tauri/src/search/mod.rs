// Find-in-files (roadmap B3.1): streaming, cancellable project search built on
// the grep crates as LIBRARIES (grep-searcher + grep-regex + ignore) — no
// bundled ripgrep binary. A search runs on its own thread, streams result
// batches to the frontend as `search://results` events and finishes with one
// `search://done`, so the UI renders matches as they arrive and can cancel
// mid-flight. Scope selection (this worktree / this project / all worktrees)
// is resolved here from the agent/project records.

pub mod commands;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use uuid::Uuid;

use crate::core::emit::emit_tracked;

/// Why: a code line longer than this is minified/generated — ship a window
/// around the first match instead of megabyte-long lines.
const MAX_LINE_BYTES: usize = 500;
/// Why: beyond this many matches in ONE file the list is noise; the group
/// header still shows the true per-file count is capped.
const MAX_PER_FILE: usize = 200;
/// Default overall cap (the UI can lower it); the search stops and reports
/// `truncated: true` once reached.
const DEFAULT_MAX_RESULTS: usize = 2000;
/// Skip files larger than this — binary blobs / bundles, never useful hits.
const MAX_FILE_SIZE: u64 = 2_000_000;
/// Result batching: flush to the frontend on size or age, whichever first.
const BATCH_SIZE: usize = 60;
const BATCH_AGE: Duration = Duration::from_millis(50);

pub const EV_RESULTS: &str = "search://results";
pub const EV_DONE: &str = "search://done";

/// One search root: an absolute directory plus the agent it belongs to (None =
/// the project's main checkout). Matches carry both back to the UI so a result
/// row can open the file in the right worktree.
#[derive(Clone)]
pub struct SearchRoot {
    pub path: PathBuf,
    pub agent_id: Option<Uuid>,
    pub label: Option<String>,
}

/// One match line. `ranges` are UTF-16 offsets into `text` (what JS string
/// indexing uses) so the frontend can highlight without re-matching.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    /// Root-relative path, forward-slash separated.
    pub path: String,
    /// Agent owning the worktree the match came from (None = project checkout).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<Uuid>,
    /// Human label for the root (agent name); None = project checkout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// 1-based line number.
    pub line: u64,
    /// The (possibly windowed) line text, trailing newline stripped.
    pub text: String,
    /// [start, end) UTF-16 offsets of each match within `text`.
    pub ranges: Vec<(usize, usize)>,
    /// Replace mode (B3.2): the line AFTER `replace_all`, windowed like `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Replace mode: staleness fingerprint — mtime in ns since epoch + length.
    /// Apply refuses files whose stamp changed since this scan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultsPayload {
    search_id: Uuid,
    items: Vec<SearchMatch>,
    /// Cumulative files scanned so far (progress hint).
    files: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DonePayload {
    search_id: Uuid,
    files: usize,
    matches: usize,
    truncated: bool,
    cancelled: bool,
}

/// Tracks in-flight searches so they can be cancelled. Managed Tauri state.
#[derive(Default)]
pub struct SearchService {
    active: Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
}

impl SearchService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new search; returns its id + cancel flag.
    fn register(&self) -> (Uuid, Arc<AtomicBool>) {
        let id = Uuid::new_v4();
        let flag = Arc::new(AtomicBool::new(false));
        self.active.lock().unwrap().insert(id, flag.clone());
        (id, flag)
    }

    /// Cancel a running search (no-op when already finished).
    pub fn cancel(&self, id: Uuid) {
        if let Some(flag) = self.active.lock().unwrap().get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn finish(&self, id: Uuid) {
        self.active.lock().unwrap().remove(&id);
    }
}

/// Escape a literal query so it can go through the regex engine unchanged
/// (the grep-regex builder has no fixed-string mode on the version pinned).
fn escape_literal(q: &str) -> String {
    let mut out = String::with_capacity(q.len() * 2);
    for c in q.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
                | '#' | '&' | '-' | '~'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Build the matcher up-front so an invalid regex fails the command (a clear
/// error in the panel) instead of dying silently on the search thread.
pub fn build_matcher(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
) -> Result<RegexMatcher, String> {
    let pattern = if regex {
        query.to_string()
    } else {
        escape_literal(query)
    };
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .word(whole_word)
        .build(&pattern)
        .map_err(|e| format!("invalid pattern: {e}"))
}

/// The `regex`-crate twin of [`build_matcher`] for REPLACEMENT (B3.2): same
/// pattern semantics (literal escaping, word boundaries, case), but with the
/// crate's `$1`/`${name}` capture interpolation — previews computed here in
/// Rust so the dialect can never diverge from what apply does.
pub fn build_replacer(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
) -> Result<regex::Regex, String> {
    let mut pattern = if regex {
        query.to_string()
    } else {
        escape_literal(query)
    };
    if whole_word {
        pattern = format!(r"\b(?:{pattern})\b");
    }
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|e| format!("invalid pattern: {e}"))
}

/// Post-replacement preview of one matched line (trailing EOL stripped,
/// windowed to the same budget as `text`).
pub fn preview_line(re: &regex::Regex, line: &str, replacement: &str) -> String {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    let mut s = re.replace_all(trimmed, replacement).into_owned();
    if s.len() > MAX_LINE_BYTES {
        let cut = floor_char_boundary(&s, MAX_LINE_BYTES);
        s.truncate(cut);
        s.push('…');
    }
    s
}

/// Stat stamp used for replace staleness checks: (mtime ns since epoch, len).
fn stamp_of(md: &std::fs::Metadata) -> (u64, u64) {
    let ns = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (ns, md.len())
}

/// Why a selected-line replace can fail per file.
#[derive(Debug)]
pub enum ReplaceError {
    /// File changed since the scan (stamp mismatch) — skipped, not an error.
    Stale,
    Io(String),
}

/// Apply `re → replacement` to the SELECTED 1-based `lines` of `rel` under
/// `root`. All-or-nothing per file: content is rewritten via a sibling temp
/// file + rename. Returns the number of lines actually changed.
pub fn apply_replace(
    root: &Path,
    rel: &str,
    re: &regex::Regex,
    replacement: &str,
    mtime: u64,
    size: u64,
    lines: &std::collections::HashSet<u64>,
) -> Result<usize, ReplaceError> {
    // paths come from our own scan results, but never trust them with the FS
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
    {
        return Err(ReplaceError::Io(format!("unsafe path: {rel}")));
    }
    let abs = root.join(p);
    let md = std::fs::metadata(&abs).map_err(|e| ReplaceError::Io(format!("stat: {e}")))?;
    if stamp_of(&md) != (mtime, size) {
        return Err(ReplaceError::Stale);
    }
    let content =
        std::fs::read_to_string(&abs).map_err(|e| ReplaceError::Io(format!("read: {e}")))?;
    let mut out = String::with_capacity(content.len());
    let mut changed = 0usize;
    for (i, raw) in content.split_inclusive('\n').enumerate() {
        let n = (i + 1) as u64;
        if !lines.contains(&n) {
            out.push_str(raw);
            continue;
        }
        let body_end = raw.trim_end_matches(['\n', '\r']).len();
        let (body, eol) = raw.split_at(body_end);
        let replaced = re.replace_all(body, replacement);
        if replaced != body {
            changed += 1;
        }
        out.push_str(&replaced);
        out.push_str(eol);
    }
    if changed == 0 {
        return Ok(0); // nothing matched on the selected lines — no write
    }
    let file_name = abs
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let tmp = abs.with_file_name(format!(".{file_name}.orrery-replace-tmp"));
    std::fs::write(&tmp, &out).map_err(|e| ReplaceError::Io(format!("write: {e}")))?;
    if let Err(e) = std::fs::rename(&tmp, &abs) {
        let _ = std::fs::remove_file(&tmp);
        return Err(ReplaceError::Io(format!("rename: {e}")));
    }
    Ok(changed)
}

/// Floor `i` to a char boundary of `s`.
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// UTF-16 length of `s[..byte]` (byte must be a char boundary).
fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

/// Window a long line around its first match; returns the emitted text plus
/// the UTF-16 ranges remapped into it.
fn window_line(line: &str, byte_ranges: &[(usize, usize)]) -> (String, Vec<(usize, usize)>) {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    let (start, end, prefix, suffix) = if trimmed.len() <= MAX_LINE_BYTES {
        (0, trimmed.len(), false, false)
    } else {
        let first = byte_ranges.first().map(|r| r.0).unwrap_or(0);
        let s = if first > MAX_LINE_BYTES.saturating_sub(120) {
            floor_char_boundary(trimmed, first.saturating_sub(80))
        } else {
            0
        };
        let e = floor_char_boundary(trimmed, (s + MAX_LINE_BYTES).min(trimmed.len()));
        (s, e, s > 0, e < trimmed.len())
    };
    let slice = &trimmed[start..end];
    let lead = if prefix { 1 } else { 0 }; // the "…" char is 1 UTF-16 unit
    let mut text = String::new();
    if prefix {
        text.push('…');
    }
    text.push_str(slice);
    if suffix {
        text.push('…');
    }
    let mut out = Vec::new();
    for &(s, e) in byte_ranges {
        if e <= start || s >= end {
            continue;
        }
        let cs = floor_char_boundary(trimmed, s.max(start));
        let ce = floor_char_boundary(trimmed, e.min(end));
        if ce <= cs {
            continue;
        }
        let u_start = lead + utf16_len(&trimmed[start..cs]);
        let u_end = lead + utf16_len(&trimmed[start..ce]);
        out.push((u_start, u_end));
    }
    (text, out)
}

/// Walk one root honoring .gitignore (hidden files INCLUDED, `.git` excluded)
/// — shared by the search runner and the file-lister.
fn walker(root: &Path) -> ignore::Walk {
    walker_with(root, Some(MAX_FILE_SIZE))
}

/// [`walker`] with the size cap injectable: search skips huge files, while the
/// status-cache fingerprint below must see EVERY file's stat. One ignore-aware
/// walker configuration for the whole backend (A2.4).
fn walker_with(root: &Path, max_filesize: Option<u64>) -> ignore::Walk {
    WalkBuilder::new(root)
        .hidden(false)
        .follow_links(false)
        .max_filesize(max_filesize)
        .filter_entry(|e| e.file_name().to_str() != Some(".git"))
        .build()
}

/// Stat-level fingerprint of a worktree: hash of every non-ignored file's
/// (path, len, mtime) via the SAME ignore-aware walker the search corpus uses
/// (A2.4: one walk implementation, not two). Content is never read — this is
/// the cheap "did anything change" component of the status cache key (A2.3).
pub(crate) fn worktree_fingerprint(root: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // No size cap: a >MAX_FILE_SIZE tracked file's edits must still change the key.
    for entry in walker_with(root, None).flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        entry
            .path()
            .strip_prefix(root)
            .unwrap_or_else(|_| entry.path())
            .hash(&mut h);
        if let Ok(md) = entry.metadata() {
            md.len().hash(&mut h);
            if let Ok(mtime) = md.modified() {
                if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    d.as_nanos().hash(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// The streaming search body — runs on its own thread per search. `replacer`
/// (B3.2 replace mode) adds a per-line post-replacement `preview` and a
/// staleness stamp to every match.
#[allow(clippy::too_many_arguments)]
pub fn run<R: Runtime>(
    app: AppHandle<R>,
    search_id: Uuid,
    cancel: Arc<AtomicBool>,
    roots: Vec<SearchRoot>,
    matcher: RegexMatcher,
    max_results: usize,
    replacer: Option<(regex::Regex, String)>,
) -> (usize, usize, bool, bool) {
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();

    let mut batch: Vec<SearchMatch> = Vec::new();
    let mut last_flush = Instant::now();
    let mut files = 0usize;
    let mut total = 0usize;
    let mut truncated = false;

    'roots: for root in &roots {
        for entry in walker(&root.path).flatten() {
            if cancel.load(Ordering::Relaxed) {
                break 'roots;
            }
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let abs = entry.path();
            let rel = abs
                .strip_prefix(&root.path)
                .unwrap_or(abs)
                .to_string_lossy()
                .replace('\\', "/");
            files += 1;

            // replace mode: one stamp per file, carried on each of its matches
            let stamp = replacer
                .as_ref()
                .and_then(|_| entry.metadata().ok())
                .map(|md| stamp_of(&md));

            let mut per_file = 0usize;
            let result = searcher.search_path(
                &matcher,
                abs,
                UTF8(|lnum, line| {
                    let mut byte_ranges: Vec<(usize, usize)> = Vec::new();
                    let _ = matcher.find_iter(line.as_bytes(), |m| {
                        byte_ranges.push((m.start(), m.end()));
                        byte_ranges.len() < 40
                    });
                    if byte_ranges.is_empty() {
                        return Ok(true); // e.g. a match the iter missed — skip
                    }
                    let (text, ranges) = window_line(line, &byte_ranges);
                    batch.push(SearchMatch {
                        path: rel.clone(),
                        agent_id: root.agent_id,
                        root: root.label.clone(),
                        line: lnum,
                        text,
                        ranges,
                        preview: replacer
                            .as_ref()
                            .map(|(re, rep)| preview_line(re, line, rep)),
                        mtime: stamp.map(|s| s.0),
                        size: stamp.map(|s| s.1),
                    });
                    per_file += 1;
                    total += 1;
                    Ok(per_file < MAX_PER_FILE && total < max_results)
                }),
            );
            if let Err(e) = result {
                log::debug!("search: skipping {}: {e}", abs.display());
            }

            if batch.len() >= BATCH_SIZE || (!batch.is_empty() && last_flush.elapsed() > BATCH_AGE)
            {
                let _ = emit_tracked(
                    &app,
                    EV_RESULTS,
                    &ResultsPayload {
                        search_id,
                        items: std::mem::take(&mut batch),
                        files,
                    },
                );
                last_flush = Instant::now();
            }

            if total >= max_results {
                truncated = true;
                break 'roots;
            }
        }
    }

    if !batch.is_empty() {
        let _ = emit_tracked(
            &app,
            EV_RESULTS,
            &ResultsPayload {
                search_id,
                items: std::mem::take(&mut batch),
                files,
            },
        );
    }
    let cancelled = cancel.load(Ordering::Relaxed);
    let _ = emit_tracked(
        &app,
        EV_DONE,
        &DonePayload {
            search_id,
            files,
            matches: total,
            truncated,
            cancelled,
        },
    );
    (files, total, truncated, cancelled)
}

/// All file paths under `root` (relative, forward-slash), gitignore-aware,
/// capped — feeds the Search-Everywhere "Files" corpus.
pub fn list_files(root: &Path, cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    for entry in walker(root).flatten() {
        if out.len() >= cap {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_literals() {
        assert_eq!(escape_literal("a.b(c)"), "a\\.b\\(c\\)");
        assert_eq!(escape_literal("plain"), "plain");
    }

    #[test]
    fn windows_short_lines_verbatim() {
        let (text, ranges) = window_line("hello world\n", &[(6, 11)]);
        assert_eq!(text, "hello world");
        assert_eq!(ranges, vec![(6, 11)]);
    }

    #[test]
    fn windows_long_lines_around_first_match() {
        let long = "x".repeat(1000) + "needle" + &"y".repeat(1000);
        let (text, ranges) = window_line(&long, &[(1000, 1006)]);
        assert!(text.contains("needle"));
        assert!(text.len() <= MAX_LINE_BYTES + 8);
        let (s, e) = ranges[0];
        assert_eq!(text.chars().next(), Some('…'));
        // ranges are UTF-16 offsets; this text is BMP-only so chars == units
        assert_eq!(text.chars().skip(s).take(e - s).collect::<String>(), "needle");
    }

    #[test]
    fn utf16_ranges_for_wide_chars() {
        // "é" is 2 bytes / 1 UTF-16 unit; "𐍈" is 4 bytes / 2 UTF-16 units.
        let line = "é𐍈abc";
        let m = build_matcher("abc", true, false, false).unwrap();
        let mut byte_ranges = Vec::new();
        let _ = m.find_iter(line.as_bytes(), |mm| {
            byte_ranges.push((mm.start(), mm.end()));
            true
        });
        let (text, ranges) = window_line(line, &byte_ranges);
        assert_eq!(text, line);
        assert_eq!(ranges, vec![(3, 6)]); // 1 (é) + 2 (𐍈) = 3 UTF-16 units in
    }

    #[test]
    fn matcher_word_and_case() {
        let m = build_matcher("Foo", true, true, false).unwrap();
        let mut hits = 0;
        let _ = m.find_iter(b"Foo foo Food Foo", |_| {
            hits += 1;
            true
        });
        assert_eq!(hits, 2);
    }

    #[test]
    fn replacer_matches_grep_semantics_and_interpolates() {
        // literal mode escapes regex metachars
        let re = build_replacer("a.b", true, false, false).unwrap();
        assert_eq!(re.replace_all("a.b axb", "X"), "X axb");
        // case-insensitive default
        let re = build_replacer("foo", false, false, false).unwrap();
        assert_eq!(re.replace_all("Foo foo", "x"), "x x");
        // whole word
        let re = build_replacer("foo", true, true, false).unwrap();
        assert_eq!(re.replace_all("foo food", "x"), "x food");
        // regex mode with $1 capture interpolation
        let re = build_replacer(r"greet(\w+)", true, false, true).unwrap();
        assert_eq!(re.replace_all("greetHumain()", "salute$1"), "saluteHumain()");
    }

    #[test]
    fn preview_windows_long_replacements() {
        let re = build_replacer("x", true, false, false).unwrap();
        let long = "x".repeat(2000);
        let p = preview_line(&re, &long, "yy");
        assert!(p.len() <= MAX_LINE_BYTES + 4);
        assert!(p.ends_with('…'));
        assert_eq!(preview_line(&re, "axa\n", "B"), "aBa");
    }

    #[test]
    fn apply_replace_selected_lines_only_with_stamp_guard() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "foo\nfoo\nfoo\r\nlast foo").unwrap();
        let md = std::fs::metadata(&p).unwrap();
        let (mtime, size) = stamp_of(&md);
        let re = build_replacer("foo", true, false, false).unwrap();

        // replace lines 2 and 4 only; CRLF on line 3 must survive untouched
        let lines: std::collections::HashSet<u64> = [2u64, 4].into_iter().collect();
        let n = apply_replace(dir.path(), "f.txt", &re, "bar", mtime, size, &lines).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "foo\nbar\nfoo\r\nlast bar"
        );

        // stamp now differs from the original scan → stale skip
        let out = apply_replace(dir.path(), "f.txt", &re, "baz", mtime, size, &lines);
        assert!(matches!(out, Err(ReplaceError::Stale)));

        // unsafe paths refused
        let out = apply_replace(dir.path(), "../f.txt", &re, "z", 0, 0, &lines);
        assert!(matches!(out, Err(ReplaceError::Io(_))));
    }

    #[test]
    fn apply_replace_no_matching_selection_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "alpha\nbeta").unwrap();
        let (mtime, size) = stamp_of(&std::fs::metadata(&p).unwrap());
        let re = build_replacer("zzz", true, false, false).unwrap();
        let lines: std::collections::HashSet<u64> = [1u64, 2].into_iter().collect();
        let n = apply_replace(dir.path(), "f.txt", &re, "x", mtime, size, &lines).unwrap();
        assert_eq!(n, 0);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "alpha\nbeta");
    }

    #[test]
    fn list_files_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "skipped.txt\n").unwrap();
        std::fs::write(dir.path().join("kept.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("skipped.txt"), "hello").unwrap();
        // an actual repo is required for .gitignore to apply via the ignore crate
        git2::Repository::init(dir.path()).unwrap();
        let files = list_files(dir.path(), 100);
        assert!(files.contains(&"kept.txt".to_string()));
        assert!(!files.contains(&"skipped.txt".to_string()));
    }
}
