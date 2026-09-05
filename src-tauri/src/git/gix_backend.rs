//! The gitoxide (`gix`) implementation of [`GitBackend`] — the app's only
//! backend. It replaced libgit2 one operation group at a time, with both
//! implementations passing the same `backend_tests!`; libgit2 remains only
//! as a test oracle there. Groups, and what is notable in each:
//!
//! - **worktrees** — the reason the port exists: libgit2 checks a worktree
//!   out on one thread, gix on all of them. The worktree registration itself
//!   (`.git/worktrees/<name>/{HEAD,commondir,gitdir}` + the `<wt>/.git`
//!   pointer file) is written by hand: it is the on-disk format git itself
//!   documents in gitrepository-layout(5), and gix has no worktree-add call.
//! - **reads** — repository probes, branch/remote listings, history, tree and
//!   blob diffs, blame, working-tree status/hunks and the gitignore oracle.
//!   Line counts and hunks come from imara-diff (the engine gix itself uses);
//!   the working-tree side is normalised the way the clean filter would
//!   (`core.autocrlf`) so counts match what git would commit.
//!
//! - **writes** — commit (there is no `git add` in gix: worktree bytes go
//!   through the clean filter to blobs, the HEAD tree is edited, the index is
//!   rebuilt from the new tree), discard, branch mutations and branch switch
//!   as ref transactions.
//! - **merge session** — `merge_commits` with diff3 markers, an index with
//!   stages 1/2/3 and the MERGE_HEAD/MERGE_MSG/MERGE_MODE files git itself
//!   expects, so abort/continue/resolve read like a git merge in progress.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use gix::bstr::{BStr, BString, ByteSlice};
use gix::diff::blob::{Algorithm, Diff, InternedInput};

use crate::core::errors::{AppError, AppResult};

use super::backend::{GitBackend, IgnoreMatcher, RepoLayout};
use super::service::remove_dir_all_retry;
use super::types::*;

/// Cache key for `status()` (A2.3): a scan whose key is unchanged returns the
/// cached result without touching the repository (no index load, no content
/// diffing). Same shape as the git2 backend's key, so both invalidate alike.
#[derive(Clone, PartialEq, Eq)]
struct StatusKey {
    /// `.git/index` (mtime as unix-nanos, len) — moves on stage/commit/reset.
    index: Option<(u128, u64)>,
    /// HEAD oid — moves on commit/checkout/reset.
    head: Option<String>,
    /// Stat-level fingerprint of the worktree (path/mtime/len of every
    /// non-ignored file) — catches edits, creates and deletes.
    worktree_fp: u64,
}

struct StatusCacheEntry {
    key: StatusKey,
    /// Whether `changes` carries per-file line counts. A full entry also
    /// serves counts-only requests; a counts-only entry never serves a full one.
    full: bool,
    changes: Vec<FileChange>,
    last_used: std::time::Instant,
}

/// Why 16: one live entry per open repo/worktree is plenty; the tiny cap keeps
/// abandoned worktrees from accumulating entries over a long session.
const STATUS_CACHE_REPOS: usize = 16;

#[derive(Default)]
pub struct GixBackend {
    /// Per-repo status cache, bounded to [`STATUS_CACHE_REPOS`] entries.
    status_cache: Mutex<HashMap<PathBuf, StatusCacheEntry>>,
}

impl GixBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

// ------------------------------------------------------------------ helpers

/// Map any gix error into the app's error type with a short prefix.
fn err<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> AppError {
    move |e| AppError::Other(format!("{what}: {e}"))
}

/// Paths in the worktree pointer files use forward slashes on every platform
/// (that is what git writes on Windows too, and what both libraries read).
fn slashed(p: &Path) -> String {
    p.display().to_string().replace(std::path::MAIN_SEPARATOR, "/")
}

fn open(path: &Path) -> AppResult<gix::Repository> {
    gix::open(path).map_err(err("open repo"))
}

/// Lexically resolve `.` and `..` (no filesystem access). gix reports a
/// linked worktree's common dir as `<gitdir>/../..` verbatim from its
/// `commondir` file; the watcher keys projects by that path, so two
/// spellings of one directory must compare equal.
fn normalized(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Repository-relative path as the frontend expects it (`/`-separated).
fn rel_str(p: &BStr) -> String {
    p.to_str_lossy().replace('\\', "/")
}

fn short(id: &gix::oid) -> String {
    id.to_hex_with_len(7).to_string()
}

/// The identity used when the repository has none configured — the same
/// fallback the git2 backend uses, so commits made by the app look alike.
fn fallback_signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: "orrery".into(),
        email: "orrery@local".into(),
        time: gix::date::Time::now_local_or_utc(),
    }
}

/// Make sure `repo` has at least one commit on its current HEAD branch.
/// An unborn HEAD gets an empty-tree "initial commit" — what a worktree
/// needs to branch from. Keeps whatever branch HEAD already names.
fn ensure_commit(repo: &gix::Repository) -> AppResult<()> {
    if repo.head_id().is_ok() {
        return Ok(());
    }
    let tree = repo
        .write_object(gix::objs::Tree::empty())
        .map_err(err("write empty tree"))?;
    let committer = repo.committer().transpose().map_err(err("committer"))?;
    let author = repo.author().transpose().map_err(err("author"))?;
    let fallback = fallback_signature();
    let (mut buf_c, mut buf_a) = (
        gix::date::parse::TimeBuf::default(),
        gix::date::parse::TimeBuf::default(),
    );
    let committer = match committer {
        Some(c) => c,
        None => fallback.to_ref(&mut buf_c),
    };
    let author = match author {
        Some(a) => a,
        None => fallback.to_ref(&mut buf_a),
    };
    // "HEAD" resolves through the unborn symref, so the branch git init chose
    // (main/master per config) is the one that gets created.
    repo.commit_as(
        committer,
        author,
        "HEAD",
        "initial commit",
        tree.detach(),
        std::iter::empty::<gix::ObjectId>(),
    )
    .map_err(err("initial commit"))?;
    Ok(())
}

/// `base` as a commit id: a revision when given and resolvable, else HEAD.
fn base_id(repo: &gix::Repository, base: Option<&str>) -> AppResult<gix::ObjectId> {
    let head = || repo.head_id().map(|id| id.detach()).map_err(err("no head"));
    match base {
        Some(b) if !b.is_empty() => match repo.rev_parse_single(b) {
            Ok(id) => Ok(id.detach()),
            Err(_) => head(),
        },
        _ => head(),
    }
}

/// Resolve a revision (full/short sha, ref) to an object id with the same
/// error text shape as the git2 backend.
fn resolve(repo: &gix::Repository, rev: &str) -> AppResult<gix::ObjectId> {
    repo.rev_parse_single(rev)
        .map(|id| id.detach())
        .map_err(|e| AppError::Other(format!("cannot resolve '{rev}': {e}")))
}

fn head_tree(repo: &gix::Repository) -> Option<gix::Tree<'_>> {
    repo.head_commit().ok()?.tree().ok()
}

/// HEAD's branch shorthand: `None` while unborn (git2 errors there), `HEAD`
/// when detached.
fn head_shorthand(repo: &gix::Repository) -> Option<String> {
    let head = repo.head().ok()?;
    if head.is_unborn() {
        return None;
    }
    Some(
        head.referent_name()
            .map(|n| n.shorten().to_string())
            .unwrap_or_else(|| "HEAD".to_string()),
    )
}

/// Bytes of the blob at `rel` in `tree` (None when absent or not a blob).
fn tree_blob(tree: &gix::Tree<'_>, rel: &str) -> Option<Vec<u8>> {
    let entry = tree.lookup_entry_by_path(rel).ok()??;
    if !entry.mode().is_blob_or_symlink() {
        return None;
    }
    Some(entry.object().ok()?.detach().data)
}

fn blob_bytes(repo: &gix::Repository, id: &gix::oid) -> Vec<u8> {
    repo.find_blob(id.to_owned())
        .map(|b| b.detach().data)
        .unwrap_or_default()
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

/// git's binary heuristic: a NUL in the first 8000 bytes.
fn is_binary(b: &[u8]) -> bool {
    b.iter().take(8000).any(|&c| c == 0)
}

/// What the clean filter would do to working-tree bytes before they compare
/// against a blob: with `core.autocrlf` on (or `input`) CRLF becomes LF.
/// libgit2 applies this inside its workdir diff; without it a CRLF checkout
/// would report every line as changed.
fn to_git_eol(repo: &gix::Repository, bytes: Vec<u8>) -> Vec<u8> {
    let autocrlf = repo
        .config_snapshot()
        .string("core.autocrlf")
        .map(|v| v.to_str_lossy().to_ascii_lowercase());
    match autocrlf.as_deref() {
        Some("true") | Some("input") => {
            if bytes.contains(&b'\r') {
                bytes.replace(b"\r\n", b"\n")
            } else {
                bytes
            }
        }
        _ => bytes,
    }
}

/// (added, deleted) line counts, or (0, 0) for binary content.
fn line_stats(old: &[u8], new: &[u8]) -> (i64, i64) {
    if old == new {
        return (0, 0);
    }
    if is_binary(old) || is_binary(new) {
        return (0, 0);
    }
    let input = InternedInput::new(old, new);
    let diff = Diff::compute(Algorithm::Myers, &input);
    (diff.count_additions() as i64, diff.count_removals() as i64)
}

/// Exact 0-context hunks of `old` → `new`, in git's hunk-header convention
/// (1-based starts; a pure insertion/deletion reports the line BEFORE it).
fn hunks_of(old: &[u8], new: &[u8]) -> Vec<Hunk> {
    if old == new {
        return Vec::new();
    }
    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|h| {
            let start = |r: &std::ops::Range<u32>| if r.is_empty() { r.start } else { r.start + 1 };
            Hunk {
                old_start: start(&h.before),
                old_lines: h.before.end - h.before.start,
                new_start: start(&h.after),
                new_lines: h.after.end - h.after.start,
            }
        })
        .collect()
}

/// Byte lines including their EOLs (a final line without newline included).
fn byte_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            out.push(&bytes[start..=i]);
            start = i + 1;
        }
    }
    if start < bytes.len() {
        out.push(&bytes[start..]);
    }
    out
}

/// Multi-threaded checkout of `tree_id` into the (empty) worktree at `wt_path`,
/// followed by writing its index — the recipe `gix clone` uses.
fn checkout_tree(wt_path: &Path, tree_id: gix::ObjectId) -> AppResult<usize> {
    let wt = open(wt_path)?;
    let mut index = wt.index_from_tree(&tree_id).map_err(err("index from tree"))?;
    let mut opts = wt
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(err("checkout options"))?;
    opts.destination_is_initially_empty = true;
    opts.thread_limit = None; // every logical core
    let files = gix::progress::Discard;
    let bytes = gix::progress::Discard;
    let interrupt = AtomicBool::new(false);
    let out = gix_worktree_state::checkout(
        &mut index,
        wt_path.to_path_buf(),
        wt.objects.clone(),
        &files,
        &bytes,
        &interrupt,
        opts,
    )
    .map_err(err("checkout"))?;
    if let Some(first) = out.errors.first() {
        return Err(AppError::Other(format!(
            "checkout: {} files failed, first {}: {}",
            out.errors.len(),
            first.path,
            first.error
        )));
    }
    index.write(Default::default()).map_err(err("write index"))?;
    Ok(index.entries().len())
}

/// The registration directory of a linked worktree: `<common>/worktrees/<name>`.
fn registration_dir(repo: &gix::Repository, wt_name: &str) -> PathBuf {
    repo.common_dir().join("worktrees").join(wt_name)
}

/// Branch names a picker pins to the top, in this order, when they exist:
/// the trunk (`main`/`master`) and the usual integration branches. Exact
/// names — `release/2.4` is a topic branch and ranks by recency like any other.
pub(crate) const PINNED_BRANCHES: [&str; 8] = [
    "main", "master", "feature", "release", "prod", "prd", "dev", "develop",
];

/// Picker order for `(branch, last-used unix seconds)` pairs: the
/// [`PINNED_BRANCHES`] that exist first (in that fixed order), then the rest
/// newest-first, ties (same second, or no timestamp at all) broken by name so
/// the order is stable between refreshes.
pub(crate) fn order_branches(mut items: Vec<(String, i64)>) -> Vec<String> {
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut out = Vec::with_capacity(items.len());
    for pinned in PINNED_BRANCHES {
        if let Some(pos) = items.iter().position(|(name, _)| name == pinned) {
            out.push(items.remove(pos).0);
        }
    }
    out.extend(items.into_iter().map(|(name, _)| name));
    out
}

/// When a branch was last USED, as unix seconds — the best git can tell us.
/// Git keeps no "last checked out" stamp on a branch; what it does keep is:
///
/// 1. the branch's reflog (`logs/refs/heads/<name>`): one line per time the
///    ref moved — created, committed on, reset, merged into, fetched (with
///    `--update-head-ok`)… the newest line's time is the freshest signal;
/// 2. the tip commit's committer time — the fallback when there is no reflog
///    (`core.logAllRefUpdates=false`, some clones/tools) or it was expired.
///
/// The max of the two. A checkout onto the branch is NOT here (it only writes
/// HEAD's reflog) — [`head_checkout_times`] covers that.
fn branch_last_used(branch: &gix::Reference<'_>) -> i64 {
    let mut log = branch.log_iter();
    let from_log = log
        .rev()
        .ok()
        .flatten()
        .and_then(|mut lines| lines.next())
        .and_then(|line| line.ok())
        .map(|line| line.signature.time.seconds);
    let from_tip = branch
        .id()
        .object()
        .ok()
        .and_then(|obj| obj.try_into_commit().ok())
        .and_then(|commit| commit.committer().ok().map(|sig| sig.seconds()));
    from_log.into_iter().chain(from_tip).max().unwrap_or(0)
}

/// branch name → the newest time some checkout SWITCHED TO it, read from the
/// `checkout: moving from <a> to <b>` lines of every HEAD reflog: the main
/// checkout's and each linked worktree's (they keep their own `logs/HEAD`).
fn head_checkout_times(repo: &gix::Repository) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    collect_head_checkouts(repo, &mut map);
    if let Ok(proxies) = repo.worktrees() {
        for proxy in proxies {
            if let Ok(wt) = proxy.into_repo_with_possibly_inaccessible_worktree() {
                collect_head_checkouts(&wt, &mut map);
            }
        }
    }
    map
}

fn collect_head_checkouts(repo: &gix::Repository, map: &mut HashMap<String, i64>) {
    let Ok(head) = repo.find_reference("HEAD") else {
        return;
    };
    let mut log = head.log_iter();
    let Ok(Some(lines)) = log.all() else {
        return;
    };
    for line in lines.flatten() {
        let msg = line.message.to_str_lossy();
        let Some(rest) = msg.strip_prefix("checkout: moving from ") else {
            continue;
        };
        // branch names never contain spaces, so the LAST " to " is the split
        let Some((_, to)) = rest.rsplit_once(" to ") else {
            continue;
        };
        let secs = line.signature.seconds();
        let slot = map.entry(to.to_string()).or_insert(secs);
        *slot = (*slot).max(secs);
    }
}

/// branch name → the checkout that holds it (main checkout or a linked
/// worktree's name). A branch absent from the map is safe to mutate.
fn occupancy(repo: &gix::Repository) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(name) = head_shorthand(repo) {
        map.insert(name, MAIN_CHECKOUT.to_string());
    }
    if let Ok(proxies) = repo.worktrees() {
        for proxy in proxies {
            let wt_name = proxy.id().to_string();
            let Some(head_name) = proxy
                .into_repo_with_possibly_inaccessible_worktree()
                .ok()
                .and_then(|r| head_shorthand(&r))
            else {
                continue;
            };
            map.entry(head_name).or_insert(wt_name);
        }
    }
    map
}

/// Commits reachable from `from` but not from `hidden`.
fn count_exclusive(repo: &gix::Repository, from: gix::ObjectId, hidden: gix::ObjectId) -> usize {
    repo.rev_walk([from])
        .with_hidden([hidden])
        .all()
        .map(|walk| walk.filter(|r| r.is_ok()).count())
        .unwrap_or(0)
}

/// Two trees → the frontend's `FileChange` list with rename detection and
/// line counts (the tree-diff twin of `status`).
fn diff_trees(
    repo: &gix::Repository,
    from: Option<&gix::Tree<'_>>,
    to: Option<&gix::Tree<'_>>,
) -> Vec<FileChange> {
    use gix::diff::tree_with_rewrites::Change;
    let opts = gix::diff::Options::default().with_rewrites(Some(gix::diff::Rewrites::default()));
    let Ok(changes) = repo.diff_tree_to_tree(from, to, opts) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ch in changes {
        let (state, path, old_path, old, new, precomputed) = match ch {
            Change::Addition {
                location,
                entry_mode,
                id,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                ('A', location, None, Vec::new(), blob_bytes(repo, &id), None)
            }
            Change::Deletion {
                location,
                entry_mode,
                id,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                ('D', location, None, blob_bytes(repo, &id), Vec::new(), None)
            }
            Change::Modification {
                location,
                previous_id,
                id,
                entry_mode,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                (
                    'M',
                    location,
                    None,
                    blob_bytes(repo, &previous_id),
                    blob_bytes(repo, &id),
                    None,
                )
            }
            Change::Rewrite {
                source_location,
                source_id,
                id,
                location,
                copy,
                diff,
                ..
            } => (
                if copy { 'A' } else { 'R' },
                location,
                (!copy).then(|| rel_str(source_location.as_ref())),
                blob_bytes(repo, &source_id),
                blob_bytes(repo, &id),
                diff.map(|d| (d.insertions as i64, d.removals as i64)),
            ),
        };
        let (add, del) = precomputed.unwrap_or_else(|| line_stats(&old, &new));
        out.push(FileChange {
            path: rel_str(path.as_ref()),
            add,
            del,
            state: state.to_string(),
            old_path,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Author/summary table + per-line indices from a gix blame outcome — the
/// same interning as the git2 backend (`blame_to_lines`).
fn blame_to_lines(repo: &gix::Repository, out: &gix::blame::Outcome) -> Blame {
    let content = lossy(&out.blob);
    let file_lines: Vec<&str> = content.split('\n').collect();
    let mut commits: Vec<BlameCommit> = Vec::new();
    let mut by_sha: HashMap<String, u32> = HashMap::new();
    let mut lines = Vec::new();
    let mut entries: Vec<&gix::blame::BlameEntry> = out.entries.iter().collect();
    entries.sort_by_key(|e| e.start_in_blamed_file);
    for e in entries {
        let sha = short(&e.commit_id);
        let c = match by_sha.get(&sha) {
            Some(&i) => i,
            None => {
                let commit = repo.find_commit(e.commit_id).ok();
                let (author, when) = commit
                    .as_ref()
                    .and_then(|c| c.author().ok().map(|a| (a.name.to_string(), a.seconds())))
                    .unwrap_or_else(|| ("unknown".to_string(), 0));
                let summary = commit
                    .as_ref()
                    .map(|c| first_line(c.message_raw_sloppy()))
                    .unwrap_or_default();
                let i = commits.len() as u32;
                commits.push(BlameCommit {
                    sha: sha.clone(),
                    author,
                    when,
                    summary,
                });
                by_sha.insert(sha, i);
                i
            }
        };
        let start = e.start_in_blamed_file as usize;
        for k in 0..e.len.get() as usize {
            let n = start + k + 1;
            lines.push(BlameLine {
                n,
                c,
                line: file_lines.get(n - 1).copied().unwrap_or("").to_string(),
            });
        }
    }
    Blame { commits, lines }
}

/// A whole-file "Uncommitted" blame (for new/untracked files): one commit
/// table entry, every line indexing it.
fn uncommitted_lines(content: &str) -> Blame {
    let commits = vec![uncommitted_commit()];
    let lines = content
        .split('\n')
        .enumerate()
        .map(|(i, line)| BlameLine {
            n: i + 1,
            c: 0,
            line: line.to_string(),
        })
        .collect();
    Blame { commits, lines }
}

fn uncommitted_commit() -> BlameCommit {
    BlameCommit {
        sha: "0000000".to_string(),
        author: "Uncommitted".to_string(),
        when: 0,
        summary: "Uncommitted changes".to_string(),
    }
}

fn first_line(msg: &BStr) -> String {
    msg.to_str_lossy()
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

/// gitignore oracle: one open repository plus its exclude stack, reused
/// across a directory walk.
struct GixIgnore {
    repo: gix::Repository,
    stack: Mutex<gix::worktree::Stack>,
    workdir: PathBuf,
}

impl IgnoreMatcher for GixIgnore {
    fn workdir(&self) -> &Path {
        &self.workdir
    }
    fn is_ignored(&self, rel: &str) -> bool {
        let is_dir = std::fs::symlink_metadata(self.workdir.join(rel))
            .map(|m| m.is_dir())
            .unwrap_or(false);
        let mode = if is_dir {
            gix::index::entry::Mode::DIR
        } else {
            gix::index::entry::Mode::FILE
        };
        let Ok(mut stack) = self.stack.lock() else {
            return false;
        };
        stack
            .at_path(Path::new(rel), Some(mode), &self.repo.objects)
            .map(|p| p.is_excluded())
            .unwrap_or(false)
    }
}

// ------------------------------------------------------------------- status

impl GixBackend {
    fn status_scan(&self, path: &Path, with_line_counts: bool) -> Vec<FileChange> {
        let key = Self::status_key(path);
        if let Some(k) = &key {
            let mut cache = self.status_cache.lock().unwrap();
            if let Some(e) = cache.get_mut(path) {
                if e.key == *k && (e.full || !with_line_counts) {
                    e.last_used = std::time::Instant::now();
                    return e.changes.clone();
                }
            }
        }
        let changes = Self::status_uncached(path, with_line_counts);
        if let Some(k) = key {
            let mut cache = self.status_cache.lock().unwrap();
            if cache.len() >= STATUS_CACHE_REPOS && !cache.contains_key(path) {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, e)| e.last_used)
                    .map(|(p, _)| p.clone())
                {
                    cache.remove(&oldest);
                }
            }
            cache.insert(
                path.to_path_buf(),
                StatusCacheEntry {
                    key: k,
                    full: with_line_counts,
                    changes: changes.clone(),
                    last_used: std::time::Instant::now(),
                },
            );
        }
        changes
    }

    fn status_key(path: &Path) -> Option<StatusKey> {
        let repo = gix::open(path).ok()?;
        let index = std::fs::metadata(repo.git_dir().join("index"))
            .ok()
            .and_then(|m| {
                let mtime = m
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_nanos();
                Some((mtime, m.len()))
            });
        let head = repo.head_id().ok().map(|id| id.to_string());
        let workdir = repo.workdir()?.to_path_buf();
        Some(StatusKey {
            index,
            head,
            worktree_fp: crate::search::worktree_fingerprint(&workdir),
        })
    }

    /// HEAD vs working tree, the way libgit2's `diff_tree_to_workdir_with_index`
    /// reports it: gix's two-layer status (HEAD→index, index→worktree, with
    /// rename tracking on both) names the candidate paths, and each one is
    /// then classified by its presence in HEAD and on disk, so a staged change
    /// and an unstaged one collapse into a single entry per path.
    fn status_uncached(path: &Path, with_line_counts: bool) -> Vec<FileChange> {
        use gix::status::index_worktree::{Item as IwItem, RewriteSource};
        use gix::status::plumbing::index_as_worktree::EntryStatus;
        use gix::status::Item;

        let Ok(repo) = gix::open(path) else {
            return Vec::new();
        };
        let Some(workdir) = repo.workdir().map(Path::to_path_buf) else {
            return Vec::new();
        };
        let head = head_tree(&repo);
        let rewrites = gix::diff::Rewrites::default();
        let Ok(platform) = repo.status(gix::progress::Discard) else {
            return Vec::new();
        };
        let Ok(iter) = platform
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_rewrites(rewrites)
            .tree_index_track_renames(gix::status::tree_index::TrackRenames::Given(rewrites))
            .into_iter(Vec::<BString>::new())
        else {
            return Vec::new();
        };

        // path → the pre-move path when some layer saw it as a rename
        let mut candidates: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut note = |p: &BStr, old: Option<&BStr>| {
            let entry = candidates.entry(rel_str(p)).or_insert(None);
            if let Some(o) = old {
                *entry = Some(rel_str(o));
            }
        };
        for item in iter.flatten() {
            match item {
                Item::IndexWorktree(iw) => match iw {
                    IwItem::Modification {
                        rela_path, status, ..
                    } => {
                        if !matches!(status, EntryStatus::NeedsUpdate(_)) {
                            note(rela_path.as_ref(), None);
                        }
                    }
                    IwItem::DirectoryContents { entry, .. } => {
                        let is_file = entry
                            .disk_kind
                            .is_some_and(|k| !matches!(k, gix::dir::entry::Kind::Directory));
                        if matches!(entry.status, gix::dir::entry::Status::Untracked) && is_file {
                            note(entry.rela_path.as_ref(), None);
                        }
                    }
                    IwItem::Rewrite {
                        source,
                        dirwalk_entry,
                        ..
                    } => {
                        let old = match &source {
                            RewriteSource::RewriteFromIndex {
                                source_rela_path, ..
                            } => Some(source_rela_path.as_ref()),
                            RewriteSource::CopyFromDirectoryEntry { .. } => None,
                        };
                        note(dirwalk_entry.rela_path.as_ref(), old);
                    }
                },
                Item::TreeIndex(ch) => {
                    use gix::diff::index::Change;
                    match ch {
                        Change::Addition { location, .. }
                        | Change::Deletion { location, .. }
                        | Change::Modification { location, .. } => note(location.as_ref(), None),
                        Change::Rewrite {
                            source_location,
                            location,
                            ..
                        } => note(location.as_ref(), Some(source_location.as_ref())),
                    }
                }
            }
        }

        let head_blob = |rel: &str| head.as_ref().and_then(|t| tree_blob(t, rel));
        let on_disk = |rel: &str| {
            std::fs::symlink_metadata(workdir.join(rel))
                .map(|m| !m.is_dir())
                .unwrap_or(false)
        };
        let mut out = Vec::new();
        for (p, old_path) in candidates {
            let in_head = head_blob(&p);
            let present = on_disk(&p);
            // a rename holds only when the source is gone from disk and still
            // in HEAD, and the destination is new to HEAD
            let rename = old_path
                .filter(|o| in_head.is_none() && present && !on_disk(o))
                .and_then(|o| head_blob(&o).map(|b| (o, b)));
            let (state, old_path, old_bytes) = match (rename, in_head, present) {
                (Some((o, b)), _, _) => ('R', Some(o), b),
                (None, Some(b), true) => ('M', None, b),
                (None, None, true) => ('A', None, Vec::new()),
                (None, Some(b), false) => ('D', None, b),
                (None, None, false) => continue,
            };
            let new_bytes = if present {
                to_git_eol(&repo, std::fs::read(workdir.join(&p)).unwrap_or_default())
            } else {
                Vec::new()
            };
            if state == 'M' && old_bytes == new_bytes {
                continue; // staged then reverted on disk: nothing vs HEAD
            }
            let (add, del) = if with_line_counts {
                line_stats(&old_bytes, &new_bytes)
            } else {
                (0, 0)
            };
            out.push(FileChange {
                path: p,
                add,
                del,
                state: state.to_string(),
                old_path,
            });
        }
        out
    }
}

// ----------------------------------------------------------- write helpers

/// The committer/author pair to sign with: the repository's configured
/// identity, else the same fallback the git2 backend uses.
fn signatures(repo: &gix::Repository) -> AppResult<(gix::actor::Signature, gix::actor::Signature)> {
    fn owned(
        s: Option<Result<gix::actor::SignatureRef<'_>, gix::config::time::Error>>,
    ) -> AppResult<Option<gix::actor::Signature>> {
        match s {
            Some(Ok(r)) => Ok(Some(r.to_owned().map_err(err("signature"))?)),
            Some(Err(e)) => Err(err("signature")(e)),
            None => Ok(None),
        }
    }
    let committer = owned(repo.committer())?.unwrap_or_else(fallback_signature);
    let author = owned(repo.author())?.unwrap_or_else(fallback_signature);
    Ok((committer, author))
}

/// Write a commit and move `reference` (usually `HEAD`, resolved through its
/// symref) onto it, reflog included.
fn commit_with(
    repo: &gix::Repository,
    reference: &str,
    message: &str,
    tree: gix::ObjectId,
    parents: Vec<gix::ObjectId>,
) -> AppResult<gix::ObjectId> {
    let (committer, author) = signatures(repo)?;
    let (mut buf_c, mut buf_a) = (
        gix::date::parse::TimeBuf::default(),
        gix::date::parse::TimeBuf::default(),
    );
    let id = repo
        .commit_as(
            committer.to_ref(&mut buf_c),
            author.to_ref(&mut buf_a),
            reference,
            message,
            tree,
            parents,
        )
        .map_err(err("commit"))?;
    Ok(id.detach())
}

fn full_name(name: &str) -> AppResult<gix::refs::FullName> {
    gix::refs::FullName::try_from(name).map_err(err("ref name"))
}

/// One ref update edit, reflog message included.
fn ref_update(
    name: &str,
    new: gix::refs::Target,
    expected: gix::refs::transaction::PreviousValue,
    message: &str,
) -> AppResult<gix::refs::transaction::RefEdit> {
    use gix::refs::transaction::{Change, LogChange, RefEdit, RefLog};
    Ok(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: message.into(),
            },
            expected,
            new,
        },
        name: full_name(name)?,
        deref: false,
    })
}

fn ref_delete(
    name: &str,
    expected: gix::refs::transaction::PreviousValue,
) -> AppResult<gix::refs::transaction::RefEdit> {
    use gix::refs::transaction::{Change, RefEdit, RefLog};
    Ok(RefEdit {
        change: Change::Delete {
            expected,
            log: RefLog::AndReference,
        },
        name: full_name(name)?,
        deref: false,
    })
}

/// Apply ref edits as one transaction, signed for the reflog.
fn edit_refs(repo: &gix::Repository, edits: Vec<gix::refs::transaction::RefEdit>) -> AppResult<()> {
    let (committer, _) = signatures(repo)?;
    let mut buf = gix::date::parse::TimeBuf::default();
    repo.edit_references_as(edits, Some(committer.to_ref(&mut buf)))
        .map_err(err("ref edit"))?;
    Ok(())
}

/// Point HEAD at `full_ref` symbolically (the branch need not exist yet).
fn set_head_symbolic(repo: &gix::Repository, full_ref: &str) -> AppResult<()> {
    edit_refs(
        repo,
        vec![ref_update(
            "HEAD",
            gix::refs::Target::Symbolic(full_name(full_ref)?),
            gix::refs::transaction::PreviousValue::Any,
            &format!(
                "checkout: moving to {}",
                full_ref.trim_start_matches("refs/heads/")
            ),
        )?],
    )
}

/// Tip commit of a local branch, or the git2-shaped "not found" error.
fn branch_tip(repo: &gix::Repository, name: &str) -> AppResult<gix::ObjectId> {
    let mut r = repo
        .find_reference(format!("refs/heads/{name}").as_str())
        .map_err(|_| AppError::Other(format!("branch '{name}' not found")))?;
    Ok(r.peel_to_id().map_err(err("branch"))?.detach())
}

fn tree_id_of(repo: &gix::Repository, commit: gix::ObjectId) -> AppResult<gix::ObjectId> {
    Ok(repo
        .find_commit(commit)
        .map_err(err("commit"))?
        .tree_id()
        .map_err(err("commit tree"))?
        .detach())
}

fn workdir_of(repo: &gix::Repository) -> AppResult<PathBuf> {
    repo.workdir()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::Other("bare repo has no working tree".into()))
}

fn occupied_err(name: &str, holder: &str) -> AppError {
    let wher = if holder == MAIN_CHECKOUT {
        "the project checkout".to_string()
    } else {
        format!("worktree '{holder}'")
    };
    AppError::Other(format!("branch '{name}' is checked out in {wher}"))
}

/// Stat data for the files this operation just wrote, keyed by index path.
/// Anything not in here keeps the previous index entry's stat when the blob
/// is unchanged, and no stat at all otherwise (so the next status rehashes).
type FreshStats = HashMap<BString, gix::index::entry::Stat>;

fn stat_of(abs: &Path) -> Option<gix::index::entry::Stat> {
    let md = gix::index::fs::Metadata::from_path_no_follow(abs).ok()?;
    gix::index::entry::Stat::from_fs(&md).ok()
}

fn kind_index_mode(kind: gix::objs::tree::EntryKind) -> gix::index::entry::Mode {
    use gix::index::entry::Mode;
    use gix::objs::tree::EntryKind;
    match kind {
        EntryKind::Tree => Mode::DIR,
        EntryKind::Blob => Mode::FILE,
        EntryKind::BlobExecutable => Mode::FILE_EXECUTABLE,
        EntryKind::Link => Mode::SYMLINK,
        EntryKind::Commit => Mode::COMMIT,
    }
}

fn index_mode_kind(mode: gix::index::entry::Mode) -> gix::objs::tree::EntryKind {
    mode.to_tree_entry_mode()
        .map(|m| m.kind())
        .unwrap_or(gix::objs::tree::EntryKind::Blob)
}

/// Rewrite the index as `tree`, carrying stat data over from the previous
/// index for unchanged entries and taking `fresh` for files just written.
/// Never invents stat data: a stale stat on a dirty file would hide a change.
fn write_index_from_tree(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    fresh: &FreshStats,
) -> AppResult<()> {
    let prev = repo.open_index().ok();
    let mut file = repo
        .index_from_tree(&tree)
        .map_err(err("index from tree"))?;
    {
        let (entries, paths) = file.entries_mut_and_pathbacking();
        for e in entries.iter_mut() {
            let p = e.path_in(paths);
            if let Some(st) = fresh.get(p) {
                e.stat = *st;
                continue;
            }
            if let Some(pe) = prev.as_ref().and_then(|prev| {
                prev.entry_by_path_and_stage(p, gix::index::entry::Stage::Unconflicted)
            }) {
                if pe.id == e.id && pe.mode == e.mode {
                    e.stat = pe.stat;
                }
            }
        }
    }
    file.write(Default::default()).map_err(err("write index"))
}

/// Write `paths` (index paths) of `tree` into the working tree, overwriting
/// whatever is there, filters applied. Returns the written files' stats.
fn checkout_paths(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    paths: &[BString],
) -> AppResult<FreshStats> {
    let mut fresh = FreshStats::new();
    if paths.is_empty() {
        return Ok(fresh);
    }
    let workdir = workdir_of(repo)?;
    let full = repo
        .index_from_tree(&tree)
        .map_err(err("index from tree"))?;
    let want: std::collections::HashSet<&BStr> = paths.iter().map(|p| p.as_bstr()).collect();
    let mut sub = gix::index::State::new(repo.object_hash());
    for e in full.entries() {
        let p = e.path(&full);
        if want.contains(p) {
            sub.dangerously_push_entry(e.stat, e.id, e.flags, e.mode, p);
        }
    }
    if sub.entries().is_empty() {
        return Ok(fresh);
    }
    let mut opts = repo
        .checkout_options(gix::worktree::stack::state::attributes::Source::IdMapping)
        .map_err(err("checkout options"))?;
    opts.destination_is_initially_empty = false;
    opts.overwrite_existing = true;
    opts.thread_limit = None;
    let interrupt = AtomicBool::new(false);
    let out = gix_worktree_state::checkout(
        &mut sub,
        workdir,
        repo.objects.clone(),
        &gix::progress::Discard,
        &gix::progress::Discard,
        &interrupt,
        opts,
    )
    .map_err(err("checkout"))?;
    if let Some(first) = out.errors.first() {
        return Err(AppError::Other(format!(
            "checkout: {} files failed, first {}: {}",
            out.errors.len(),
            first.path,
            first.error
        )));
    }
    for e in sub.entries() {
        fresh.insert(e.path(&sub).to_owned(), e.stat);
    }
    Ok(fresh)
}

/// Remove one working-tree file and the empty directories it leaves behind
/// (up to the worktree root), the way git does after a delete.
fn delete_path(workdir: &Path, rel: &str) {
    let abs = workdir.join(rel);
    match std::fs::symlink_metadata(&abs) {
        Ok(md) if md.is_dir() => return,
        Ok(_) => {
            let _ = std::fs::remove_file(&abs);
        }
        Err(_) => return,
    }
    let mut dir = abs.parent();
    while let Some(d) = dir {
        if d == workdir || std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Does `tree` hold a file (blob or symlink) at `rel`?
fn tree_has_file(tree: &gix::Tree<'_>, rel: &BStr) -> bool {
    tree.lookup_entry_by_path(rel.to_str_lossy().as_ref())
        .ok()
        .flatten()
        .is_some_and(|e| e.mode().is_blob_or_symlink())
}

/// Make the working tree and the index equal `target` — git's force
/// checkout / `reset --hard`: every path that differs (per the HEAD→target
/// tree diff, the dirty worktree, and any conflicted index entry) is
/// rewritten from `target` or deleted. Untracked files are left alone unless
/// `target` claims their path. HEAD itself is not moved.
fn reset_to_tree(repo: &gix::Repository, target: gix::ObjectId) -> AppResult<()> {
    let workdir = workdir_of(repo)?;
    let target_tree = repo.find_tree(target).map_err(err("tree"))?;
    let head = head_tree(repo);
    let mut paths: std::collections::BTreeSet<BString> = std::collections::BTreeSet::new();
    for ch in diff_trees(repo, head.as_ref(), Some(&target_tree)) {
        paths.insert(ch.path.into());
        if let Some(o) = ch.old_path {
            paths.insert(o.into());
        }
    }
    for ch in GixBackend::status_uncached(&workdir, false) {
        let p: BString = ch.path.into();
        if ch.state != "A" || tree_has_file(&target_tree, p.as_bstr()) {
            paths.insert(p);
        }
        if let Some(o) = ch.old_path {
            paths.insert(o.into());
        }
    }
    if let Ok(idx) = repo.open_index() {
        paths.extend(conflicted_paths(&idx));
    }
    let (restore, gone): (Vec<BString>, Vec<BString>) = paths
        .into_iter()
        .partition(|p| tree_has_file(&target_tree, p.as_bstr()));
    for p in &gone {
        delete_path(&workdir, &p.to_str_lossy());
    }
    let fresh = checkout_paths(repo, target, &restore)?;
    write_index_from_tree(repo, target, &fresh)
}

/// A tree object from the index's stage-0 entries (gix has no index→tree).
fn tree_from_index(repo: &gix::Repository, index: &gix::index::State) -> AppResult<gix::ObjectId> {
    let mut editor = repo.empty_tree().edit().map_err(err("tree editor"))?;
    for e in index.entries() {
        if e.stage() != gix::index::entry::Stage::Unconflicted {
            continue;
        }
        editor
            .upsert(e.path(index), index_mode_kind(e.mode), e.id)
            .map_err(err("tree entry"))?;
    }
    Ok(editor.write().map_err(err("write tree"))?.detach())
}

const MERGE_FILES: [&str; 4] = ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "AUTO_MERGE"];

fn clear_merge_state(repo: &gix::Repository) {
    for f in MERGE_FILES {
        let _ = std::fs::remove_file(repo.git_dir().join(f));
    }
}

fn merge_heads(repo: &gix::Repository) -> Option<Vec<gix::ObjectId>> {
    let text = std::fs::read_to_string(repo.git_dir().join("MERGE_HEAD")).ok()?;
    Some(
        text.lines()
            .filter_map(|l| gix::ObjectId::from_hex(l.trim().as_bytes()).ok())
            .collect(),
    )
}

/// Distinct conflicted paths (any entry above stage 0).
fn conflicted_paths(index: &gix::index::State) -> std::collections::BTreeSet<BString> {
    index
        .entries()
        .iter()
        .filter(|e| e.stage() != gix::index::entry::Stage::Unconflicted)
        .map(|e| e.path(index).to_owned())
        .collect()
}

/// Edit `.git/config` in place (the shared one for linked worktrees).
fn edit_config(
    repo: &gix::Repository,
    f: impl FnOnce(&mut gix::config::File) -> AppResult<()>,
) -> AppResult<()> {
    let path = repo.common_dir().join("config");
    let mut file =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)
            .map_err(err("read config"))?;
    f(&mut file)?;
    let mut out = std::fs::File::create(&path).map_err(err("write config"))?;
    file.write_to(&mut out).map_err(err("write config"))
}

/// Drop `branch.<name>.*` from the config (best-effort, like git does on
/// branch delete).
fn remove_branch_config(repo: &gix::Repository, name: &str) {
    let _ = edit_config(repo, |cfg| {
        cfg.remove_section("branch", Some(name.into()));
        Ok(())
    });
}

/// Pathspec test the way the app uses it: exact path, or a directory prefix.
fn pathspec_matches(specs: &[String], path: &str) -> bool {
    specs.is_empty()
        || specs.iter().any(|s| {
            let s = s.trim_end_matches('/');
            path == s || path.starts_with(&format!("{s}/"))
        })
}


impl GitBackend for GixBackend {
    fn ensure_main_branch(&self, path: &Path) -> AppResult<()> {
        let repo = open(path)?;
        if repo.head_id().is_ok() {
            return Ok(()); // already has a commit → already has a branch
        }
        set_head_symbolic(&repo, "refs/heads/main")?;
        ensure_commit(&repo)
    }
    fn branch_create(&self, repo_path: &Path, name: &str, from: Option<&str>) -> AppResult<()> {
        let repo = open(repo_path)?;
        let id = match from {
            Some(rev) => repo
                .find_object(resolve(&repo, rev)?)
                .map_err(err("revision"))?
                .peel_to_commit()
                .map_err(err("revision"))?
                .id,
            None => repo.head_id().map_err(err("head"))?.detach(),
        };
        repo.reference(
            format!("refs/heads/{name}").as_str(),
            id,
            gix::refs::transaction::PreviousValue::MustNotExist,
            format!("branch: Created from {}", from.unwrap_or("HEAD")),
        )
        .map_err(err("branch create"))?;
        Ok(())
    }
    /// Rename refuses while ANY checkout holds the branch — that checkout's
    /// HEAD symref would be stranded on a dead ref.
    fn branch_rename(&self, repo_path: &Path, old: &str, new: &str) -> AppResult<()> {
        use gix::refs::transaction::PreviousValue;
        use gix::refs::Target;
        let repo = open(repo_path)?;
        if let Some(holder) = occupancy(&repo).get(old) {
            return Err(occupied_err(old, holder));
        }
        let tip = branch_tip(&repo, old)?;
        edit_refs(
            &repo,
            vec![
                ref_update(
                    &format!("refs/heads/{new}"),
                    Target::Object(tip),
                    PreviousValue::MustNotExist,
                    &format!("branch: renamed refs/heads/{old} to refs/heads/{new}"),
                )?,
                ref_delete(
                    &format!("refs/heads/{old}"),
                    PreviousValue::MustExistAndMatch(Target::Object(tip)),
                )?,
            ],
        )?;
        // the branch's own config section follows it
        let _ = edit_config(&repo, |cfg| {
            let _ = cfg.rename_section("branch", Some(old.into()), "branch", Some(new.into()));
            Ok(())
        });
        Ok(())
    }
    /// Delete refuses occupied branches and — unless `force` — branches not
    /// merged into HEAD.
    fn branch_delete(&self, repo_path: &Path, name: &str, force: bool) -> AppResult<()> {
        use gix::refs::transaction::PreviousValue;
        use gix::refs::Target;
        let repo = open(repo_path)?;
        if let Some(holder) = occupancy(&repo).get(name) {
            return Err(occupied_err(name, holder));
        }
        let tip = branch_tip(&repo, name)?;
        if !force {
            if let Ok(head) = repo.head_id().map(|id| id.detach()) {
                let merged = head == tip
                    || repo
                        .merge_base(head, tip)
                        .map(|b| b.detach() == tip)
                        .unwrap_or(false);
                if !merged {
                    return Err(AppError::Other(format!(
                        "branch '{name}' is not merged — use force to delete anyway"
                    )));
                }
            }
        }
        edit_refs(
            &repo,
            vec![ref_delete(
                &format!("refs/heads/{name}"),
                PreviousValue::MustExistAndMatch(Target::Object(tip)),
            )?],
        )?;
        remove_branch_config(&repo, name);
        Ok(())
    }
    /// Set (`Some("origin/main")`) or unset (`None`) a branch's upstream by
    /// writing `branch.<name>.remote` / `.merge`. Unsetting a branch that
    /// never had one is a no-op.
    fn branch_set_upstream(
        &self,
        repo_path: &Path,
        name: &str,
        upstream: Option<&str>,
    ) -> AppResult<()> {
        let repo = open(repo_path)?;
        branch_tip(&repo, name)?;
        let Some(up) = upstream else {
            let has = repo
                .config_snapshot()
                .string(format!("branch.{name}.merge").as_str())
                .is_some();
            if !has {
                return Ok(());
            }
            return edit_config(&repo, |cfg| {
                let empty = match cfg.section_mut("branch", Some(name.into())) {
                    Ok(mut s) => {
                        s.remove("remote");
                        s.remove("merge");
                        s.num_values() == 0
                    }
                    Err(_) => false,
                };
                if empty {
                    cfg.remove_section("branch", Some(name.into()));
                }
                Ok(())
            });
        };
        // `<remote>/<branch>` (a remote-tracking ref) or a local branch (remote ".")
        let (remote, merge_ref) = if repo
            .find_reference(format!("refs/remotes/{up}").as_str())
            .is_ok()
        {
            let remotes: Vec<String> = repo
                .remote_names()
                .into_iter()
                .map(|n| n.to_string())
                .collect();
            let Some(remote) = remotes
                .iter()
                .filter(|r| up.starts_with(&format!("{r}/")))
                .max_by_key(|r| r.len())
                .cloned()
            else {
                return Err(AppError::Other(format!(
                    "upstream '{up}' does not name a known remote"
                )));
            };
            let branch = &up[remote.len() + 1..];
            (remote, format!("refs/heads/{branch}"))
        } else if repo
            .find_reference(format!("refs/heads/{up}").as_str())
            .is_ok()
        {
            (".".to_string(), format!("refs/heads/{up}"))
        } else {
            return Err(AppError::Other(format!(
                "upstream '{up}' not found: no such remote-tracking or local branch"
            )));
        };
        edit_config(&repo, |cfg| {
            cfg.set_raw_value_by("branch", Some(name.into()), "remote", remote.as_str())
                .map_err(err("config"))?;
            cfg.set_raw_value_by("branch", Some(name.into()), "merge", merge_ref.as_str())
                .map_err(err("config"))?;
            Ok(())
        })
    }
    /// Switch `checkout` to `branch` (safe mode: a dirty file the switch would
    /// touch aborts). Refuses a branch another checkout holds.
    fn checkout_branch(&self, checkout: &Path, branch: &str) -> AppResult<()> {
        let repo = open(checkout)?;
        let occ = occupancy(&repo);
        let here = head_shorthand(&repo);
        if let Some(holder) = occ.get(branch) {
            // fine when THIS checkout already holds it (no-op checkout)
            if here.as_deref() != Some(branch) {
                return Err(occupied_err(branch, holder));
            }
            return Ok(());
        }
        let workdir = workdir_of(&repo)?;
        let tip = branch_tip(&repo, branch)?;
        let target = tree_id_of(&repo, tip)?;
        let target_tree = repo.find_tree(target).map_err(err("tree"))?;
        let head = head_tree(&repo);
        let changes = diff_trees(&repo, head.as_ref(), Some(&target_tree));

        let mut dirty: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ch in Self::status_uncached(&workdir, false) {
            dirty.insert(ch.path);
            if let Some(o) = ch.old_path {
                dirty.insert(o);
            }
        }
        for ch in &changes {
            let touched = std::iter::once(&ch.path).chain(ch.old_path.iter());
            if let Some(p) = touched.into_iter().find(|p| dirty.contains(*p)) {
                return Err(AppError::Other(format!(
                    "checkout: '{p}' has local changes that would be overwritten — commit or discard them first"
                )));
            }
        }

        let mut restore: Vec<BString> = Vec::new();
        for ch in &changes {
            match ch.state.as_str() {
                "D" => delete_path(&workdir, &ch.path),
                "R" => {
                    if let Some(o) = &ch.old_path {
                        delete_path(&workdir, o);
                    }
                    restore.push(ch.path.clone().into());
                }
                _ => restore.push(ch.path.clone().into()),
            }
        }
        let fresh = checkout_paths(&repo, target, &restore)?;
        write_index_from_tree(&repo, target, &fresh)?;
        set_head_symbolic(&repo, &format!("refs/heads/{branch}"))
    }
    /// Stage `paths` (everything changed when empty) into a new tree and
    /// commit it. gix has no `git add`: each file is filtered, written as a
    /// blob and placed in a copy of HEAD's tree; deleted files leave it.
    fn commit(&self, worktree: &Path, message: &str, paths: &[String]) -> AppResult<String> {
        let repo = open(worktree)?;
        ensure_commit(&repo)?;
        let workdir = workdir_of(&repo)?;
        let head = repo.head_id().map_err(err("head"))?.detach();
        let head_tree = repo
            .find_commit(head)
            .map_err(err("head"))?
            .tree()
            .map_err(err("head tree"))?;
        let mut editor = head_tree.edit().map_err(err("tree editor"))?;
        let (mut pipeline, index) = repo.filter_pipeline(None).map_err(err("filters"))?;

        // (path, present on disk): everything status sees, or the requested
        // pathspecs (a directory expands to the changes below it)
        let mut ops: Vec<(String, bool)> = Vec::new();
        let changes = Self::status_uncached(&workdir, false);
        let from_change = |ch: &FileChange, ops: &mut Vec<(String, bool)>| {
            match ch.state.as_str() {
                "D" => ops.push((ch.path.clone(), false)),
                "R" => {
                    if let Some(o) = &ch.old_path {
                        ops.push((o.clone(), false));
                    }
                    ops.push((ch.path.clone(), true));
                }
                _ => ops.push((ch.path.clone(), true)),
            }
        };
        if paths.is_empty() {
            for ch in &changes {
                from_change(ch, &mut ops);
            }
        } else {
            for p in paths {
                let p = p.trim_end_matches('/');
                let abs = workdir.join(p);
                if abs.is_dir() {
                    for ch in changes.iter().filter(|c| pathspec_matches(&[p.to_string()], &c.path)) {
                        from_change(ch, &mut ops);
                    }
                } else {
                    ops.push((p.to_string(), abs.symlink_metadata().is_ok()));
                }
            }
        }

        let mut fresh = FreshStats::new();
        for (p, present) in ops {
            let rel: &BStr = p.as_str().into();
            if !present {
                let _ = editor.remove(rel);
                continue;
            }
            match pipeline
                .worktree_file_to_object(rel, &index)
                .map_err(err("stage"))?
            {
                Some((id, kind, _)) => {
                    editor.upsert(rel, kind, id).map_err(err("stage"))?;
                    if let Some(st) = stat_of(&workdir.join(&p)) {
                        fresh.insert(p.into(), st);
                    }
                }
                None => {
                    let _ = editor.remove(rel);
                }
            }
        }
        let tree = editor.write().map_err(err("write tree"))?.detach();
        let msg = if message.trim().is_empty() { "wip" } else { message };
        let oid = commit_with(&repo, "HEAD", msg, tree, vec![head])?;
        write_index_from_tree(&repo, tree, &fresh)?;
        Ok(short(&oid))
    }
    /// Discard working-tree changes for `paths` (or all when empty): tracked
    /// files go back to HEAD, untracked ones are removed (ignored files stay).
    fn discard(&self, worktree: &Path, paths: &[String]) -> AppResult<()> {
        let repo = open(worktree)?;
        let workdir = workdir_of(&repo)?;
        let head_tree = repo
            .head_commit()
            .map_err(err("head"))?
            .tree_id()
            .map_err(err("head tree"))?
            .detach();
        let mut restore: Vec<BString> = Vec::new();
        for ch in Self::status_uncached(&workdir, false) {
            let selected = pathspec_matches(paths, &ch.path)
                || ch.old_path.as_deref().is_some_and(|o| pathspec_matches(paths, o));
            if !selected {
                continue;
            }
            match ch.state.as_str() {
                "A" => delete_path(&workdir, &ch.path),
                "R" => {
                    delete_path(&workdir, &ch.path);
                    if let Some(o) = ch.old_path {
                        restore.push(o.into());
                    }
                }
                _ => restore.push(ch.path.into()),
            }
        }
        let fresh = checkout_paths(&repo, head_tree, &restore)?;
        write_index_from_tree(&repo, head_tree, &fresh)
    }
    /// Merge `branch` into HEAD (fast-forward when possible). A conflicted
    /// merge is left in progress — merged tree with diff3 markers in the
    /// working tree, stages 1/2/3 in the index, MERGE_HEAD written — for
    /// `conflict_resolve` + `merge_continue`, or `merge_abort`.
    fn merge(&self, repo_path: &Path, branch: &str) -> AppResult<MergeSession> {
        use gix::merge::blob::builtin_driver::text::{Conflict, ConflictStyle, Labels};
        use gix::merge::tree::{apply_index_entries::RemovalMode, TreatAsUnresolved};
        use gix::refs::transaction::PreviousValue;
        use gix::refs::Target;

        let repo = open(repo_path)?;
        let ours_label = head_shorthand(&repo).unwrap_or_else(|| "HEAD".into());
        let session = |conflicts: Vec<ConflictFile>| MergeSession {
            ours: head_shorthand(&repo).unwrap_or_else(|| "HEAD".into()),
            theirs: branch.to_string(),
            conflicts,
        };
        let theirs = branch_tip(&repo, branch)?;
        let head = repo.head_id().map_err(err("head"))?.detach();
        let base = repo.merge_base(head, theirs).ok().map(|b| b.detach());
        if head == theirs || base == Some(theirs) {
            return Ok(session(Vec::new())); // already up to date
        }
        if base == Some(head) {
            // fast-forward: worktree first (dirty detection reads HEAD), then the ref
            let head_ref = repo
                .head_ref()
                .map_err(err("head"))?
                .ok_or_else(|| AppError::Other("HEAD is detached".into()))?;
            let name = head_ref.name().as_bstr().to_string();
            reset_to_tree(&repo, tree_id_of(&repo, theirs)?)?;
            edit_refs(
                &repo,
                vec![ref_update(
                    &name,
                    Target::Object(theirs),
                    PreviousValue::MustExistAndMatch(Target::Object(head)),
                    "fast-forward merge",
                )?],
            )?;
            return Ok(session(Vec::new()));
        }

        // true merge: diff3 markers so the 3-way UI can show the base per hunk
        let mut inner: gix::merge::plumbing::tree::Options = repo
            .tree_merge_options()
            .map_err(err("merge options"))?
            .into();
        inner.blob_merge.text.conflict = Conflict::Keep {
            style: ConflictStyle::Diff3,
            marker_size: std::num::NonZeroU8::new(7).expect("7 > 0"),
        };
        let opts: gix::merge::commit::Options = gix::merge::tree::Options::from(inner).into();
        let labels = Labels {
            ancestor: Some("base".into()),
            current: Some(ours_label.as_str().into()),
            other: Some(branch.into()),
        };
        let mut outcome = repo
            .merge_commits(head, theirs, labels, opts)
            .map_err(err("merge"))?;
        let merged = outcome
            .tree_merge
            .tree
            .write()
            .map_err(err("merge tree"))?
            .detach();
        let how = TreatAsUnresolved::git();
        let unresolved = outcome.tree_merge.has_unresolved_conflicts(how);
        reset_to_tree(&repo, merged)?;
        if unresolved {
            let mut index = repo.open_index().map_err(err("index"))?;
            outcome
                .tree_merge
                .index_changed_after_applying_conflicts(&mut index, how, RemovalMode::Prune);
            index.write(Default::default()).map_err(err("write index"))?;
            let gitdir = repo.git_dir();
            std::fs::write(gitdir.join("MERGE_HEAD"), format!("{theirs}\n"))
                .map_err(err("MERGE_HEAD"))?;
            let _ = std::fs::write(gitdir.join("MERGE_MSG"), format!("Merge branch '{branch}'\n"));
            let _ = std::fs::write(gitdir.join("MERGE_MODE"), "");
            return Ok(session(self.conflict_files(repo_path)?));
        }
        commit_with(&repo, "HEAD", &format!("merge {branch}"), merged, vec![head, theirs])?;
        clear_merge_state(&repo);
        Ok(session(Vec::new()))
    }
    /// The currently conflicted files, read from index stages 1/2/3.
    fn conflict_files(&self, repo_path: &Path) -> AppResult<Vec<ConflictFile>> {
        use gix::index::entry::Stage;
        let repo = open(repo_path)?;
        let workdir = workdir_of(&repo)?;
        let index = repo.open_index().map_err(err("index"))?;
        let mut by_path: BTreeMap<String, [Option<gix::ObjectId>; 3]> = BTreeMap::new();
        for e in index.entries() {
            let slot = match e.stage() {
                Stage::Unconflicted => continue,
                Stage::Base => 0,
                Stage::Ours => 1,
                Stage::Theirs => 2,
            };
            by_path.entry(rel_str(e.path(&index))).or_default()[slot] = Some(e.id);
        }
        let text = |id: Option<gix::ObjectId>| {
            id.map(|id| lossy(&blob_bytes(&repo, &id))).unwrap_or_default()
        };
        Ok(by_path
            .into_iter()
            .map(|(path, [base, ours, theirs])| ConflictFile {
                merged: std::fs::read_to_string(workdir.join(&path)).unwrap_or_default(),
                ours: text(ours),
                theirs: text(theirs),
                base: text(base),
                resolved: false,
                lang: lang_from_path(&path).to_string(),
                path,
            })
            .collect())
    }
    /// Write `content` as the resolution of `rel` and stage it: the path's
    /// stage 1/2/3 entries collapse into one stage-0 entry.
    fn conflict_resolve(&self, repo_path: &Path, rel: &str, content: &str) -> AppResult<()> {
        let repo = open(repo_path)?;
        let workdir = workdir_of(&repo)?;
        let abs = workdir.join(rel);
        if let Some(parent) = abs.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&abs, content).map_err(|e| AppError::Other(format!("write '{rel}': {e}")))?;
        let rel_b: &BStr = rel.into();
        let (mut pipeline, index) = repo.filter_pipeline(None).map_err(err("filters"))?;
        let Some((id, kind, _)) = pipeline
            .worktree_file_to_object(rel_b, &index)
            .map_err(err("stage"))?
        else {
            return Err(AppError::Other(format!("stage '{rel}': not a file")));
        };
        drop(index);
        let mut index = repo.open_index().map_err(err("index"))?;
        index.remove_entries(|_, p, _| p == rel_b);
        let stat = stat_of(&abs).unwrap_or_default();
        index.dangerously_push_entry(
            stat,
            id,
            gix::index::entry::Flags::empty(),
            kind_index_mode(kind),
            rel_b,
        );
        index.sort_entries();
        index.write(Default::default()).map_err(err("write index"))
    }
    /// Abort the in-progress merge: working tree + index back to HEAD, merge
    /// files gone. Untracked files are left alone (`reset --hard` semantics).
    fn merge_abort(&self, repo_path: &Path) -> AppResult<()> {
        let repo = open(repo_path)?;
        let head_tree = repo
            .head_commit()
            .map_err(err("head"))?
            .tree_id()
            .map_err(err("head tree"))?
            .detach();
        reset_to_tree(&repo, head_tree)?;
        clear_merge_state(&repo);
        Ok(())
    }
    /// Finish the merge once every conflict is resolved: commit the index
    /// with HEAD + MERGE_HEAD(s) as parents, then drop the merge files.
    fn merge_continue(&self, repo_path: &Path, message: Option<&str>) -> AppResult<String> {
        let repo = open(repo_path)?;
        let index = repo.open_index().map_err(err("index"))?;
        if !conflicted_paths(&index).is_empty() {
            return Err(AppError::Other(
                "unresolved conflicts remain — resolve every file first".into(),
            ));
        }
        let heads = merge_heads(&repo)
            .ok_or_else(|| AppError::Other("no merge in progress (MERGE_HEAD missing)".into()))?;
        if heads.is_empty() {
            return Err(AppError::Other("no merge in progress".into()));
        }
        let tree = tree_from_index(&repo, &index)?;
        let head = repo.head_id().map_err(err("head"))?.detach();
        let default_msg = std::fs::read_to_string(repo.git_dir().join("MERGE_MSG"))
            .ok()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| format!("merge {}", short(&heads[0])));
        let msg = match message {
            Some(m) if !m.trim().is_empty() => m,
            _ => default_msg.as_str(),
        };
        let mut parents = vec![head];
        parents.extend(heads);
        let oid = commit_with(&repo, "HEAD", msg, tree, parents)?;
        clear_merge_state(&repo);
        Ok(short(&oid))
    }
    /// Whether a merge / rebase / cherry-pick / revert is in progress, and
    /// how many files are still conflicted.
    fn session_state(&self, repo_path: &Path) -> AppResult<SessionState> {
        let repo = open(repo_path)?;
        let gitdir = repo.git_dir();
        let state = if gitdir.join("MERGE_HEAD").exists() {
            "merge"
        } else if gitdir.join("rebase-merge").exists() || gitdir.join("rebase-apply").exists() {
            "rebase"
        } else if gitdir.join("CHERRY_PICK_HEAD").exists() {
            "cherrypick"
        } else if gitdir.join("REVERT_HEAD").exists() {
            "revert"
        } else if gitdir.join("BISECT_LOG").exists() {
            "other"
        } else {
            "none"
        };
        let conflicts = repo
            .open_index()
            .map(|i| conflicted_paths(&i).len())
            .unwrap_or(0);
        Ok(SessionState {
            state: state.into(),
            conflicts,
            ours: head_shorthand(&repo).unwrap_or_else(|| "HEAD".into()),
        })
    }
    // ------------------------------------------------------------ repository

    fn detect(&self, path: &Path) -> bool {
        gix::open(path).is_ok()
    }
    fn init(&self, path: &Path) -> AppResult<()> {
        gix::init(path).map_err(err("git init"))?;
        Ok(())
    }
    fn head_branch(&self, path: &Path) -> Option<String> {
        head_shorthand(&gix::open(path).ok()?)
    }
    fn head_info(&self, path: &Path) -> Option<(String, String)> {
        let repo = gix::open(path).ok()?;
        let branch = head_shorthand(&repo)?;
        let id = repo.head_id().ok()?;
        Some((branch, short(&id)))
    }
    fn head_oid(&self, path: &Path) -> Option<String> {
        gix::open(path).ok()?.head_id().ok().map(|id| id.to_string())
    }
    fn repo_layout(&self, path: &Path) -> Option<RepoLayout> {
        let repo = gix::open(path).ok()?;
        let gitdir = normalized(repo.git_dir());
        let common_dir = normalized(repo.common_dir());
        let (main_workdir, worktrees) = match gix::open(&common_dir) {
            Ok(main) => {
                let wd = main.workdir().map(normalized);
                let wts = main
                    .worktrees()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|p| Some((p.id().to_string(), normalized(&p.base().ok()?))))
                    .collect();
                (wd, wts)
            }
            Err(_) => (None, Vec::new()),
        };
        Some(RepoLayout {
            gitdir,
            common_dir,
            main_workdir,
            worktrees,
        })
    }
    fn ignore_matcher(&self, path: &Path) -> Option<Box<dyn IgnoreMatcher>> {
        let repo = gix::open(path).ok()?;
        let workdir = repo.workdir().unwrap_or(path).to_path_buf();
        let index = repo.index_or_empty().ok()?;
        let stack = repo
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .ok()?
            .detach();
        Some(Box::new(GixIgnore {
            repo,
            stack: Mutex::new(stack),
            workdir,
        }))
    }

    // -------------------------------------------------------------- branches

    // Ordered for a picker: the conventional trunk/integration names first, then
    // every other branch by the last time it was USED — see `branch_last_used`
    // for what git records about that — so the branch you were on yesterday
    // sits right under `main` instead of somewhere in an alphabet of `feat/…`.
    fn branches(&self, path: &Path) -> Vec<String> {
        let Ok(repo) = gix::open(path) else {
            return Vec::new();
        };
        let Ok(platform) = repo.references() else {
            return Vec::new();
        };
        let Ok(iter) = platform.local_branches() else {
            return Vec::new();
        };
        let mut items: Vec<(String, i64)> = iter
            .flatten()
            .map(|r| (r.name().shorten().to_string(), branch_last_used(&r)))
            .collect();
        // A checkout onto the branch moves no ref of its own, so it only shows
        // in the HEAD reflogs — fold those in on top of the per-branch times.
        let checkouts = head_checkout_times(&repo);
        for (name, ts) in &mut items {
            if let Some(t) = checkouts.get(name.as_str()) {
                *ts = (*ts).max(*t);
            }
        }
        order_branches(items)
    }
    fn default_branch(&self, path: &Path) -> Option<String> {
        let repo = gix::open(path).ok()?;
        // 1. the remote's declared default: refs/remotes/origin/HEAD → "origin/<x>"
        if let Ok(reference) = repo.find_reference("refs/remotes/origin/HEAD") {
            if let gix::refs::TargetRef::Symbolic(name) = reference.target() {
                if let Some(rest) = name.as_bstr().strip_prefix(b"refs/remotes/origin/") {
                    return Some(rest.to_str_lossy().to_string());
                }
            }
        }
        let locals = self.branches(path);
        // 2. a conventional default that exists locally
        for cand in ["main", "master"] {
            if locals.iter().any(|b| b == cand) {
                return Some(cand.to_string());
            }
        }
        // 3. the current HEAD branch, but only when it's a genuine local branch
        if let Some((b, _)) = self.head_info(path) {
            if locals.iter().any(|x| *x == b) {
                return Some(b);
            }
        }
        None
    }
    fn branches_detail(&self, repo_path: &Path) -> AppResult<Vec<BranchInfo>> {
        let repo = open(repo_path)?;
        let occ = occupancy(&repo);
        let head_name = head_shorthand(&repo);
        let platform = repo.references().map_err(err("references"))?;
        let mut out = Vec::new();
        for r in platform
            .local_branches()
            .map_err(err("branches"))?
            .flatten()
        {
            let name = r.name().shorten().to_string();
            let local_id = r.id().detach();
            // upstream counts only when its remote-tracking ref really exists
            // (git2's `upstream()` errors otherwise)
            let upstream_ref = repo
                .branch_remote_tracking_ref_name(r.name(), gix::remote::Direction::Fetch)
                .and_then(|res| res.ok())
                .and_then(|full| repo.find_reference(full.as_ref()).ok());
            let (upstream, ahead, behind) = match upstream_ref {
                Some(mut up) => {
                    let label = up.name().shorten().to_string();
                    match up.peel_to_id().ok() {
                        Some(up_id) => {
                            let up_id = up_id.detach();
                            (
                                Some(label),
                                count_exclusive(&repo, local_id, up_id),
                                count_exclusive(&repo, up_id, local_id),
                            )
                        }
                        None => (Some(label), 0, 0),
                    }
                }
                None => (None, 0, 0),
            };
            out.push(BranchInfo {
                current: head_name.as_deref() == Some(name.as_str()),
                checked_out_in: occ.get(&name).cloned(),
                upstream,
                ahead,
                behind,
                name,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
    fn remotes(&self, repo_path: &Path) -> AppResult<Vec<RemoteInfo>> {
        let repo = open(repo_path)?;
        let mut out = Vec::new();
        for name in repo.remote_names() {
            let url = repo
                .find_remote(name.as_bstr())
                .ok()
                .and_then(|r| {
                    r.url(gix::remote::Direction::Fetch)
                        .map(|u| u.to_bstring().to_string())
                })
                .unwrap_or_default();
            out.push(RemoteInfo {
                name: name.to_string(),
                url,
            });
        }
        Ok(out)
    }

    // --------------------------------------------------------------- history

    fn log(&self, path: &Path, limit: usize, offset: usize) -> Vec<LogEntry> {
        let Ok(repo) = gix::open(path) else {
            return Vec::new();
        };
        let Ok(head) = repo.head_id() else {
            return Vec::new(); // no HEAD (no commits yet)
        };
        let Ok(walk) = repo
            .rev_walk([head.detach()])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(Default::default()))
            .all()
        else {
            return Vec::new();
        };
        walk.flatten()
            .skip(offset)
            .take(limit)
            .filter_map(|info| info.object().ok())
            .map(|commit| {
                // delta count only — no rename detection, no line diffing
                let files = commit
                    .tree()
                    .ok()
                    .map(|tree| {
                        let parent_tree = commit
                            .parent_ids()
                            .next()
                            .and_then(|p| repo.find_commit(p).ok())
                            .and_then(|p| p.tree().ok());
                        repo.diff_tree_to_tree(
                            parent_tree.as_ref(),
                            Some(&tree),
                            gix::diff::Options::default(),
                        )
                        .map(|d| d.len())
                        .unwrap_or(0)
                    })
                    .unwrap_or(0);
                LogEntry {
                    sha: short(&commit.id),
                    message: first_line(commit.message_raw_sloppy()),
                    author: commit
                        .author()
                        .map(|a| a.name.to_string())
                        .unwrap_or_else(|_| "unknown".to_string()),
                    time: commit.time().map(|t| t.seconds).unwrap_or(0),
                    files,
                }
            })
            .collect()
    }
    fn commit_time(&self, path: &Path, rev: &str) -> AppResult<(i64, String)> {
        let repo = open(path)?;
        let id = resolve(&repo, rev)?;
        let commit = repo
            .find_commit(id)
            .map_err(|e| AppError::Other(format!("find '{rev}': {e}")))?;
        Ok((commit.time().map(|t| t.seconds).unwrap_or(0), id.to_string()))
    }
    fn commit_files(&self, path: &Path, sha: &str) -> AppResult<Vec<FileChange>> {
        let repo = open(path)?;
        let commit = repo
            .find_commit(resolve(&repo, sha)?)
            .map_err(err("find commit"))?;
        let to = commit.tree().map_err(err("commit tree"))?;
        let from = commit
            .parent_ids()
            .next()
            .and_then(|p| repo.find_commit(p).ok())
            .and_then(|p| p.tree().ok());
        Ok(diff_trees(&repo, from.as_ref(), Some(&to)))
    }
    fn commit_file_diff(&self, path: &Path, sha: &str, rel: &str) -> AppResult<FileDiff> {
        let repo = open(path)?;
        let commit = repo
            .find_commit(resolve(&repo, sha)?)
            .map_err(err("find commit"))?;
        let to = commit.tree().map_err(err("commit tree"))?;
        let from = commit
            .parent_ids()
            .next()
            .and_then(|p| repo.find_commit(p).ok())
            .and_then(|p| p.tree().ok());
        Ok(FileDiff {
            old: from
                .as_ref()
                .and_then(|t| tree_blob(t, rel))
                .map(|b| lossy(&b))
                .unwrap_or_default(),
            new: tree_blob(&to, rel).map(|b| lossy(&b)).unwrap_or_default(),
            lang: lang_from_path(rel).to_string(),
        })
    }
    fn range_diff(&self, path: &Path, from: &str, to: &str) -> AppResult<Vec<FileChange>> {
        let repo = open(path)?;
        let from_tree = repo
            .find_commit(resolve(&repo, from)?)
            .map_err(err("from commit"))?
            .tree()
            .map_err(err("from tree"))?;
        let to_tree = repo
            .find_commit(resolve(&repo, to)?)
            .map_err(err("to commit"))?
            .tree()
            .map_err(err("to tree"))?;
        Ok(diff_trees(&repo, Some(&from_tree), Some(&to_tree)))
    }
    fn range_file_diff(
        &self,
        path: &Path,
        from: &str,
        to: &str,
        rel: &str,
    ) -> AppResult<FileDiff> {
        let repo = open(path)?;
        let from_tree = repo
            .find_commit(resolve(&repo, from)?)
            .map_err(err("from commit"))?
            .tree()
            .map_err(err("from tree"))?;
        let to_tree = repo
            .find_commit(resolve(&repo, to)?)
            .map_err(err("to commit"))?
            .tree()
            .map_err(err("to tree"))?;
        Ok(FileDiff {
            old: tree_blob(&from_tree, rel).map(|b| lossy(&b)).unwrap_or_default(),
            new: tree_blob(&to_tree, rel).map(|b| lossy(&b)).unwrap_or_default(),
            lang: lang_from_path(rel).to_string(),
        })
    }
    fn file_history(
        &self,
        path: &Path,
        rel: &str,
        limit: usize,
        offset: usize,
    ) -> AppResult<Vec<FileHistoryEntry>> {
        let repo = open(path)?;
        let head = repo.head_id().map_err(err("push head"))?.detach();
        let walk = repo
            .rev_walk([head])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(Default::default()))
            .all()
            .map_err(err("revwalk"))?;
        let mut results = Vec::new();
        let mut seen = 0usize;
        for info in walk.flatten() {
            let Ok(commit) = info.object() else { continue };
            let Ok(to_tree) = commit.tree() else { continue };
            let from_tree = commit
                .parent_ids()
                .next()
                .and_then(|p| repo.find_commit(p).ok())
                .and_then(|p| p.tree().ok());
            // Cheap "did this path change?" check: compare the entry oid in
            // this commit's tree vs the (first) parent's — no diff computed.
            let new_oid = to_tree
                .lookup_entry_by_path(rel)
                .ok()
                .flatten()
                .map(|e| e.object_id());
            let old_oid = from_tree
                .as_ref()
                .and_then(|t| t.lookup_entry_by_path(rel).ok().flatten())
                .map(|e| e.object_id());
            if new_oid == old_oid {
                continue;
            }
            if seen < offset {
                seen += 1;
                continue;
            }
            let old = from_tree
                .as_ref()
                .and_then(|t| tree_blob(t, rel))
                .unwrap_or_default();
            let new = tree_blob(&to_tree, rel).unwrap_or_default();
            let (add, del) = line_stats(&old, &new);
            let (author, email) = commit
                .author()
                .map(|a| (a.name.to_string(), a.email.to_string()))
                .unwrap_or_else(|_| ("unknown".to_string(), String::new()));
            results.push(FileHistoryEntry {
                sha: short(&commit.id),
                author,
                email,
                when: commit.time().map(|t| t.seconds).unwrap_or(0),
                summary: first_line(commit.message_raw_sloppy()),
                add,
                del,
            });
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }
    fn blame(&self, path: &Path, rel: &str, rev: Option<&str>) -> AppResult<Blame> {
        let repo = open(path)?;
        let suspect = match rev {
            Some(r) => resolve(&repo, r)?,
            None => repo
                .head_id()
                .map_err(|_| AppError::Other("no HEAD".into()))?
                .detach(),
        };
        let out = repo
            .blame_file(
                BStr::new(rel),
                suspect,
                gix::repository::blame_file::Options::default(),
            )
            .map_err(|e| AppError::Other(format!("blame '{rel}': {e}")))?;
        Ok(blame_to_lines(&repo, &out))
    }
    /// HEAD side blamed at HEAD; working side re-mapped line by line: a
    /// working line that is unchanged vs HEAD keeps its author, an edited or
    /// new line is "Uncommitted" (what libgit2's `blame_buffer` computes).
    fn working_blame(&self, path: &Path, rel: &str) -> AppResult<(Blame, Blame)> {
        let repo = open(path)?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| AppError::Other("no workdir".into()))?
            .to_path_buf();
        let working_raw = std::fs::read(workdir.join(rel)).unwrap_or_default();
        let working_str = lossy(&working_raw);
        let working = to_git_eol(&repo, working_raw);

        // A brand-new (untracked) file has no HEAD history to blame — the whole
        // file is your uncommitted work, and there is no old side.
        let Some(head_bytes) = head_tree(&repo).and_then(|t| tree_blob(&t, rel)) else {
            return Ok((Blame::default(), uncommitted_lines(&working_str)));
        };
        let old = match self.blame(path, rel, None) {
            Ok(b) => b,
            Err(_) => return Ok((Blame::default(), uncommitted_lines(&working_str))),
        };

        // working line (0-based) → HEAD line (0-based) for every unchanged line
        let input = InternedInput::new(head_bytes.as_slice(), working.as_slice());
        let diff = Diff::compute(Algorithm::Histogram, &input);
        let n_new = input.after.len();
        let mut map: Vec<Option<usize>> = vec![None; n_new];
        let equal_run = |from_b: usize, from_a: usize, len: usize, map: &mut Vec<Option<usize>>| {
            for k in 0..len {
                if let Some(slot) = map.get_mut(from_a + k) {
                    *slot = Some(from_b + k);
                }
            }
        };
        let (mut pb, mut pa) = (0usize, 0usize);
        for h in diff.hunks() {
            let (bs, be) = (h.before.start as usize, h.before.end as usize);
            let (_as, ae) = (h.after.start as usize, h.after.end as usize);
            equal_run(pb, pa, bs.saturating_sub(pb), &mut map);
            pb = be;
            pa = ae;
        }
        equal_run(pb, pa, n_new.saturating_sub(pa), &mut map);

        let text_lines: Vec<&str> = working_str.split('\n').collect();
        let mut commits: Vec<BlameCommit> = Vec::new();
        let mut by_sha: HashMap<String, u32> = HashMap::new();
        let mut lines = Vec::with_capacity(n_new);
        for (j, mapped) in map.iter().enumerate() {
            let src = mapped
                .and_then(|i| old.lines.get(i))
                .and_then(|l| old.commits.get(l.c as usize))
                .cloned()
                .unwrap_or_else(uncommitted_commit);
            let c = match by_sha.get(&src.sha) {
                Some(&i) => i,
                None => {
                    let i = commits.len() as u32;
                    by_sha.insert(src.sha.clone(), i);
                    commits.push(src);
                    i
                }
            };
            lines.push(BlameLine {
                n: j + 1,
                c,
                line: text_lines.get(j).copied().unwrap_or("").to_string(),
            });
        }
        Ok((old, Blame { commits, lines }))
    }

    // ---------------------------------------------------------- working tree

    fn status(&self, path: &Path) -> Vec<FileChange> {
        self.status_scan(path, true)
    }
    fn status_counts_only(&self, path: &Path) -> Vec<FileChange> {
        self.status_scan(path, false)
    }
    fn file_diff(&self, worktree: &Path, rel: &str, old_rel: Option<&str>) -> FileDiff {
        let old_path = old_rel.unwrap_or(rel);
        let old = gix::open(worktree)
            .ok()
            .and_then(|repo| head_tree(&repo).and_then(|t| tree_blob(&t, old_path)))
            .map(|b| lossy(&b))
            .unwrap_or_default();
        let new = std::fs::read_to_string(worktree.join(rel)).unwrap_or_default();
        FileDiff {
            old,
            new,
            lang: lang_from_path(rel).to_string(),
        }
    }
    fn file_hunks(&self, worktree: &Path, rel: &str) -> AppResult<Vec<Hunk>> {
        let repo = open(worktree)?;
        let head = head_tree(&repo)
            .and_then(|t| tree_blob(&t, rel))
            .unwrap_or_default();
        let work = to_git_eol(&repo, std::fs::read(worktree.join(rel)).unwrap_or_default());
        Ok(hunks_of(&head, &work))
    }
    /// Revert ONE hunk of `rel` in the working tree by splicing the HEAD blob's
    /// bytes over the changed region — recomputed at revert time (a hunk that
    /// moved errors as stale) and byte-exact, adapted to the file's own EOLs.
    fn revert_hunk(&self, worktree: &Path, rel: &str, new_start: u32) -> AppResult<()> {
        let repo = open(worktree)?;
        let hunk = self
            .file_hunks(worktree, rel)?
            .into_iter()
            .find(|h| h.new_start == new_start)
            .ok_or_else(|| {
                AppError::Other(
                    "hunk not found — the file changed since the markers were computed".into(),
                )
            })?;

        // HEAD-side bytes (empty for an untracked file — reverting removes lines).
        let head_bytes = head_tree(&repo)
            .and_then(|t| tree_blob(&t, rel))
            .unwrap_or_default();
        let abs = worktree.join(rel);
        let work_bytes =
            std::fs::read(&abs).map_err(|e| AppError::Other(format!("read '{rel}': {e}")))?;

        let head_lines = byte_lines(&head_bytes);
        let work_lines = byte_lines(&work_bytes);

        let old_start = hunk.old_start as usize;
        let old_n = hunk.old_lines as usize;
        if old_n > 0 && (old_start == 0 || old_start + old_n - 1 > head_lines.len()) {
            return Err(AppError::Other("hunk out of range of HEAD content".into()));
        }
        // The blob stores the NORMALIZED form (autocrlf strips CR on add) — adapt
        // restored lines to the working file's EOL convention so a revert never
        // introduces mixed line endings.
        let work_crlf = work_bytes.windows(2).any(|w| w == b"\r\n");
        let to_work_eol = |line: &[u8]| -> Vec<u8> {
            let mut v: Vec<u8>;
            if line.ends_with(b"\n") && !line.ends_with(b"\r\n") && work_crlf {
                v = line[..line.len() - 1].to_vec();
                v.extend_from_slice(b"\r\n");
            } else if line.ends_with(b"\r\n") && !work_crlf {
                v = line[..line.len() - 2].to_vec();
                v.push(b'\n');
            } else {
                v = line.to_vec();
            }
            v
        };
        let replacement: Vec<Vec<u8>> = if old_n == 0 {
            Vec::new()
        } else {
            head_lines[old_start - 1..old_start - 1 + old_n]
                .iter()
                .map(|l| to_work_eol(l))
                .collect()
        };

        // The workdir region the hunk covers. Pure deletion (`new_lines == 0`)
        // INSERTS after line `new_start` instead of replacing anything.
        let (region_from, region_to) = if hunk.new_lines == 0 {
            let after = hunk.new_start as usize; // may be 0 = top of file
            (after, after)
        } else {
            let from = hunk.new_start as usize - 1;
            (from, from + hunk.new_lines as usize)
        };
        if region_to > work_lines.len() {
            return Err(AppError::Other("hunk out of range of working content".into()));
        }

        let mut out: Vec<u8> = Vec::with_capacity(work_bytes.len() + head_bytes.len());
        for l in &work_lines[..region_from] {
            out.extend_from_slice(l);
        }
        for l in &replacement {
            out.extend_from_slice(l);
        }
        for l in &work_lines[region_to..] {
            out.extend_from_slice(l);
        }
        std::fs::write(&abs, out).map_err(|e| AppError::Other(format!("write '{rel}': {e}")))?;
        Ok(())
    }

    // ------------------------------------------------------------- worktrees

    /// Branch + registration + parallel checkout. Idempotent on the branch
    /// (an existing `branch` is reused, as with git2), strict on the target
    /// directory (must be absent or empty, as `git worktree add` demands).
    fn create_worktree(
        &self,
        repo_path: &Path,
        wt_name: &str,
        branch: &str,
        base: Option<&str>,
        wt_path: &Path,
    ) -> AppResult<()> {
        crate::perf::timed("git_worktree_add", || {
            let repo = open(repo_path)?;
            ensure_commit(&repo)?;
            let base = base_id(&repo, base)?;
            let full_ref = format!("refs/heads/{branch}");
            let tip = match repo.find_reference(full_ref.as_str()) {
                Ok(mut r) => r.peel_to_id().map_err(err("branch"))?.detach(),
                Err(_) => {
                    repo.reference(
                        full_ref.as_str(),
                        base,
                        gix::refs::transaction::PreviousValue::MustNotExist,
                        format!("worktree add: {wt_name}"),
                    )
                    .map_err(err("branch"))?;
                    base
                }
            };
            let tree_id = repo
                .find_commit(tip)
                .map_err(err("tip commit"))?
                .tree_id()
                .map_err(err("tip tree"))?
                .detach();

            // the target must be absent or an empty directory
            if wt_path.exists()
                && std::fs::read_dir(wt_path)
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(true)
            {
                return Err(AppError::Other(format!(
                    "worktree add: '{}' exists and is not empty",
                    wt_path.display()
                )));
            }
            let private = registration_dir(&repo, wt_name);
            if private.exists() {
                return Err(AppError::Other(format!(
                    "worktree add: a worktree named '{wt_name}' is already registered"
                )));
            }
            std::fs::create_dir_all(&private).map_err(err("worktree metadata"))?;
            std::fs::create_dir_all(wt_path).map_err(err("worktree dir"))?;
            let wt_gitfile = wt_path.join(".git");
            let write = |name: &str, content: String| -> AppResult<()> {
                std::fs::write(private.join(name), content).map_err(err("worktree metadata"))
            };
            write("HEAD", format!("ref: {full_ref}\n"))?;
            write("commondir", "../..\n".into())?;
            write("gitdir", format!("{}\n", slashed(&wt_gitfile)))?;
            std::fs::write(&wt_gitfile, format!("gitdir: {}\n", slashed(&private)))
                .map_err(err("worktree pointer"))?;

            match checkout_tree(wt_path, tree_id) {
                Ok(n) => {
                    log::info!(
                        "worktree '{wt_name}': {n} entries checked out at {}",
                        wt_path.display()
                    );
                    Ok(())
                }
                Err(e) => {
                    // never leave a half-checked-out registration behind
                    let _ = remove_dir_all_retry(&private);
                    let _ = remove_dir_all_retry(wt_path);
                    Err(e)
                }
            }
        })
    }

    /// Drop the registration (and, on `DeleteFolder`, the directory it points
    /// at). An unknown name is not an error: the folder is torn down by the
    /// caller, and a registration that is already gone is the wanted state.
    fn remove_worktree(
        &self,
        repo_path: &Path,
        wt_name: &str,
        disposal: WorktreeDisposal,
    ) -> AppResult<()> {
        let repo = open(repo_path)?;
        let private = registration_dir(&repo, wt_name);
        if !private.is_dir() {
            return Ok(());
        }
        if disposal == WorktreeDisposal::DeleteFolder {
            // `gitdir` holds `<wt>/.git`; its parent is the working directory.
            if let Ok(gitfile) = std::fs::read_to_string(private.join("gitdir")) {
                if let Some(dir) = Path::new(gitfile.trim()).parent() {
                    let _ = remove_dir_all_retry(dir);
                }
            }
        }
        remove_dir_all_retry(&private).map_err(err("worktree prune"))?;
        Ok(())
    }

    // --------------------------------------------------------- merge session

}

#[cfg(test)]
mod branch_order_tests {
    use super::order_branches;

    fn items(v: &[(&str, i64)]) -> Vec<(String, i64)> {
        v.iter().map(|(n, t)| (n.to_string(), *t)).collect()
    }

    // Pinned names in their fixed order regardless of timestamp, then the rest
    // newest-first, and a same-second tie falls back to the name.
    #[test]
    fn pinned_first_in_fixed_order_then_newest_first_then_name() {
        let out = order_branches(items(&[
            ("zeta", 5),
            ("dev", 1),
            ("alpha", 5),
            ("main", 0),
            ("feat/x", 9),
            ("release", 3),
        ]));
        assert_eq!(out, ["main", "release", "dev", "feat/x", "alpha", "zeta"]);
    }

    // No pinned branch at all → pure recency; a prefixed name like
    // `release/2.4` is NOT pinned.
    #[test]
    fn prefixed_conventional_names_are_ordinary_topic_branches() {
        let out = order_branches(items(&[("release/2.4", 1), ("feat/y", 2)]));
        assert_eq!(out, ["feat/y", "release/2.4"]);
    }
}
