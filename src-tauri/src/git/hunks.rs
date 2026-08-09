//! B4.3 — per-file hunks vs HEAD + reverse-apply of one hunk (the editor's
//! gutter change markers and their click-to-revert). The A3.7 hunk-machinery
//! subset: backend-authoritative hunks so markers and revert share coordinates.

use std::path::Path;

use git2::{DiffOptions, Repository};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};

fn app(e: git2::Error) -> AppError {
    AppError::Other(e.message().to_string())
}

/// One changed region of the working file vs HEAD (context 0 — exact lines).
/// `new_lines == 0` marks a pure deletion AFTER `new_start`; `old_lines == 0`
/// a pure insertion. 1-based starts, git hunk-header convention.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

fn diff_opts(rel: &str, reverse: bool) -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.pathspec(rel)
        .disable_pathspec_match(true)
        .context_lines(0)
        .include_untracked(true)
        .show_untracked_content(true)
        .reverse(reverse);
    opts
}

fn head_tree(repo: &Repository) -> Option<git2::Tree<'_>> {
    repo.head().ok()?.peel_to_tree().ok()
}

/// The file's changed hunks vs HEAD (exact, no context). Untracked files show
/// as one all-added hunk; an unchanged file yields an empty list.
pub fn file_hunks(worktree: &Path, rel: &str) -> AppResult<Vec<Hunk>> {
    let repo = Repository::open(worktree).map_err(app)?;
    let tree = head_tree(&repo);
    let diff = repo
        .diff_tree_to_workdir(tree.as_ref(), Some(&mut diff_opts(rel, false)))
        .map_err(app)?;
    let mut out = Vec::new();
    diff.foreach(
        &mut |_, _| true,
        None,
        Some(&mut |_, hunk| {
            out.push(Hunk {
                old_start: hunk.old_start(),
                old_lines: hunk.old_lines(),
                new_start: hunk.new_start(),
                new_lines: hunk.new_lines(),
            });
            true
        }),
        None,
    )
    .map_err(app)?;
    Ok(out)
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

/// Revert ONE hunk of `rel` in the working tree by splicing the HEAD blob's
/// bytes over the changed region — recomputed at revert time (a hunk that
/// moved errors as stale) and byte-exact (no `git2::apply`, whose checkout
/// filters would re-run autocrlf and rewrite line endings on Windows).
pub fn revert_hunk(worktree: &Path, rel: &str, new_start: u32) -> AppResult<()> {
    let repo = Repository::open(worktree).map_err(app)?;
    let hunk = file_hunks(worktree, rel)?
        .into_iter()
        .find(|h| h.new_start == new_start)
        .ok_or_else(|| {
            AppError::Other(
                "hunk not found — the file changed since the markers were computed".into(),
            )
        })?;

    // HEAD-side bytes (empty for an untracked file — reverting removes lines).
    let head_bytes: Vec<u8> = head_tree(&repo)
        .and_then(|t| t.get_path(Path::new(rel)).ok())
        .and_then(|e| e.to_object(&repo).ok())
        .and_then(|o| o.into_blob().ok())
        .map(|b| b.content().to_vec())
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

// ------------------------------------------------------------- commands ----

#[tauri::command]
pub async fn agent_file_hunks(
    agents: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<Vec<Hunk>> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_file_hunks", || {
            file_hunks(Path::new(&agents.get(id)?.worktree), &path)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn agent_hunk_revert(
    agents: State<'_, AgentService>,
    id: Uuid,
    path: String,
    new_start: u32,
) -> AppResult<()> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_hunk_revert", || {
            revert_hunk(Path::new(&agents.get(id)?.worktree), &path, new_start)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;

    fn commit_file(repo_path: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(repo_path.join(name), content).unwrap();
        let repo = Repository::open(repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("t", "t@example.com").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        commit_file(dir.path(), "f.txt", "one\ntwo\nthree\nfour\nfive\n", "base");
        dir
    }

    #[test]
    fn hunks_report_exact_modified_added_deleted_regions() {
        let dir = repo();
        // modify line 2, delete line 4, append line 6
        std::fs::write(dir.path().join("f.txt"), "one\nTWO\nthree\nfive\nsix\n").unwrap();
        let hunks = file_hunks(dir.path(), "f.txt").unwrap();
        assert_eq!(
            hunks,
            vec![
                Hunk { old_start: 2, old_lines: 1, new_start: 2, new_lines: 1 }, // modified
                Hunk { old_start: 4, old_lines: 1, new_start: 3, new_lines: 0 }, // deleted
                Hunk { old_start: 5, old_lines: 0, new_start: 5, new_lines: 1 }, // added
            ]
        );
    }

    #[test]
    fn untracked_file_is_one_added_hunk() {
        let dir = repo();
        std::fs::write(dir.path().join("new.txt"), "a\nb\n").unwrap();
        let hunks = file_hunks(dir.path(), "new.txt").unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!((hunks[0].new_start, hunks[0].new_lines), (1, 2));
        assert_eq!(hunks[0].old_lines, 0);
    }

    #[test]
    fn unchanged_file_has_no_hunks() {
        let dir = repo();
        assert!(file_hunks(dir.path(), "f.txt").unwrap().is_empty());
    }

    #[test]
    fn revert_one_hunk_restores_only_that_region() {
        let dir = repo();
        // two separated changes: line 1 and line 5
        std::fs::write(dir.path().join("f.txt"), "ONE\ntwo\nthree\nfour\nFIVE\n").unwrap();
        let hunks = file_hunks(dir.path(), "f.txt").unwrap();
        assert_eq!(hunks.len(), 2);
        // revert only the second (line 5) hunk
        revert_hunk(dir.path(), "f.txt", hunks[1].new_start).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "ONE\ntwo\nthree\nfour\nfive\n"
        );
        // the remaining hunk still reverts
        revert_hunk(dir.path(), "f.txt", 1).unwrap();
        assert!(file_hunks(dir.path(), "f.txt").unwrap().is_empty());
    }

    #[test]
    fn revert_reinserts_deleted_lines() {
        let dir = repo();
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\nfour\nfive\n").unwrap();
        let hunks = file_hunks(dir.path(), "f.txt").unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].new_lines, 0);
        revert_hunk(dir.path(), "f.txt", hunks[0].new_start).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "one\ntwo\nthree\nfour\nfive\n"
        );
    }

    #[test]
    fn revert_untracked_hunk_empties_the_file() {
        let dir = repo();
        std::fs::write(dir.path().join("new.txt"), "a\nb\n").unwrap();
        let hunks = file_hunks(dir.path(), "new.txt").unwrap();
        revert_hunk(dir.path(), "new.txt", hunks[0].new_start).unwrap();
        assert_eq!(std::fs::read(dir.path().join("new.txt")).unwrap(), b"");
    }

    #[test]
    fn stale_hunk_errors_instead_of_wrong_apply() {
        let dir = repo();
        std::fs::write(dir.path().join("f.txt"), "one\nTWO\nthree\nfour\nfive\n").unwrap();
        let err = revert_hunk(dir.path(), "f.txt", 99).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("hunk not found"), "{msg}");
    }

    #[test]
    fn crlf_content_reverts_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        commit_file(dir.path(), "w.txt", "a\r\nb\r\nc\r\n", "crlf base");
        std::fs::write(dir.path().join("w.txt"), "a\r\nB!\r\nc\r\n").unwrap();
        let hunks = file_hunks(dir.path(), "w.txt").unwrap();
        assert_eq!(hunks.len(), 1);
        revert_hunk(dir.path(), "w.txt", hunks[0].new_start).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("w.txt")).unwrap(),
            "a\r\nb\r\nc\r\n"
        );
    }
}
