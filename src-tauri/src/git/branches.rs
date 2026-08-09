//! A3.2 — native branch & remote operations for the Branches panel.
//!
//! Everything is git2 EXCEPT the auth-touching ops (fetch, pull), which shell
//! out to the system `git` exactly like `GitService::push` so the OS credential
//! helper handles auth (libgit2 would need manual credential callbacks).
//!
//! The branch→checkout occupancy map is the safety core: git refuses to check
//! out a branch that another worktree holds, and git2 does NOT enforce that for
//! rename/delete — so every mutating op pre-checks occupancy instead of
//! relying on downstream errors.

use std::collections::HashMap;
use std::path::Path;

use git2::{BranchType, Repository};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::projects::service::ProjectService;

fn app(e: git2::Error) -> AppError {
    AppError::Other(e.message().to_string())
}

fn open(path: &Path) -> AppResult<Repository> {
    Repository::open(path).map_err(|e| AppError::Other(format!("open: {}", e.message())))
}

/// Where the project checkout itself shows up in the occupancy map.
pub const MAIN_CHECKOUT: &str = "";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    /// Checked out in the project's main checkout (HEAD there).
    pub current: bool,
    /// Worktree NAME holding this branch ("" = the main checkout, None = free).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_out_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
}

/// branch name → the checkout that holds it (main checkout or a linked
/// worktree's name). A branch absent from the map is safe to mutate.
fn occupancy(repo: &Repository) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(head) = repo.head() {
        if let Ok(name) = head.shorthand() {
            map.insert(name.to_string(), MAIN_CHECKOUT.to_string());
        }
    }
    if let Ok(worktrees) = repo.worktrees() {
        for i in 0..worktrees.len() {
            let Ok(Some(wt_name)) = worktrees.get(i) else {
                continue;
            };
            let Some(head_name) = repo
                .find_worktree(wt_name)
                .ok()
                .and_then(|wt| Repository::open(wt.path()).ok())
                .and_then(|r| {
                    r.head()
                        .ok()
                        .and_then(|h| h.shorthand().ok().map(String::from))
                })
            else {
                continue;
            };
            map.entry(head_name).or_insert_with(|| wt_name.to_string());
        }
    }
    map
}

pub fn branches_detail(repo_path: &Path) -> AppResult<Vec<BranchInfo>> {
    let repo = open(repo_path)?;
    let occ = occupancy(&repo);
    let head_name = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(String::from));
    let mut out = Vec::new();
    for b in repo.branches(Some(BranchType::Local)).map_err(app)? {
        let (branch, _) = b.map_err(app)?;
        let Some(name) = branch.name().map_err(app)?.map(String::from) else {
            continue;
        };
        let upstream = branch
            .upstream()
            .ok()
            .and_then(|u| u.name().ok().flatten().map(String::from));
        let (ahead, behind) = match (branch.get().target(), &upstream) {
            (Some(local), Some(up)) => repo
                .find_branch(up, BranchType::Remote)
                .ok()
                .and_then(|u| u.get().target())
                .and_then(|up_oid| repo.graph_ahead_behind(local, up_oid).ok())
                .unwrap_or((0, 0)),
            _ => (0, 0),
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

pub fn remotes(repo_path: &Path) -> AppResult<Vec<RemoteInfo>> {
    let repo = open(repo_path)?;
    let mut out = Vec::new();
    let names = repo.remotes().map_err(app)?;
    for i in 0..names.len() {
        let Ok(Some(name)) = names.get(i) else {
            continue;
        };
        let url = repo
            .find_remote(name)
            .ok()
            .and_then(|r| r.url().ok().map(String::from))
            .unwrap_or_default();
        out.push(RemoteInfo {
            name: name.to_string(),
            url,
        });
    }
    Ok(out)
}

/// Run a git CLI subcommand in `dir`, surfacing stderr on failure (the same
/// shell-out-for-auth pattern as `GitService::push`).
fn git_cli(dir: &Path, args: &[&str]) -> AppResult<()> {
    let out = crate::core::proc::cmd("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| AppError::Other(format!("git {}: {e}", args.first().unwrap_or(&""))))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Fetch (with prune) one remote, or every remote when `remote` is None.
pub fn fetch(repo_path: &Path, remote: Option<&str>) -> AppResult<()> {
    match remote {
        Some(r) => git_cli(repo_path, &["fetch", "--prune", r]),
        None => git_cli(repo_path, &["fetch", "--all", "--prune"]),
    }
}

/// Fast-forward pull in `checkout`. A diverged branch errors with git's own
/// message — the UI directs the user to "Merge in" (the conflict-session flow).
pub fn pull_ff(checkout: &Path) -> AppResult<()> {
    git_cli(checkout, &["pull", "--ff-only"])
}

pub fn branch_create(repo_path: &Path, name: &str, from: Option<&str>) -> AppResult<()> {
    let repo = open(repo_path)?;
    let commit = match from {
        Some(refname) => repo
            .revparse_single(refname)
            .map_err(app)?
            .peel_to_commit()
            .map_err(app)?,
        None => repo.head().map_err(app)?.peel_to_commit().map_err(app)?,
    };
    repo.branch(name, &commit, false).map_err(app)?;
    Ok(())
}

fn occupied_err(name: &str, holder: &str) -> AppError {
    let wher = if holder == MAIN_CHECKOUT {
        "the project checkout".to_string()
    } else {
        format!("worktree '{holder}'")
    };
    AppError::Other(format!("branch '{name}' is checked out in {wher}"))
}

/// Rename refuses while ANY checkout holds the branch — git2's rename does not
/// rewrite that checkout's HEAD symref, which would strand it on a dead ref.
pub fn branch_rename(repo_path: &Path, old: &str, new: &str) -> AppResult<()> {
    let repo = open(repo_path)?;
    if let Some(holder) = occupancy(&repo).get(old) {
        return Err(occupied_err(old, holder));
    }
    let mut b = repo.find_branch(old, BranchType::Local).map_err(app)?;
    b.rename(new, false).map_err(app)?;
    Ok(())
}

/// Delete refuses occupied branches (git2 does NOT enforce this for linked
/// worktrees) and — unless `force` — branches not merged into HEAD.
pub fn branch_delete(repo_path: &Path, name: &str, force: bool) -> AppResult<()> {
    let repo = open(repo_path)?;
    if let Some(holder) = occupancy(&repo).get(name) {
        return Err(occupied_err(name, holder));
    }
    let mut b = repo.find_branch(name, BranchType::Local).map_err(app)?;
    if !force {
        let head = repo.head().map_err(app)?.target();
        let tip = b.get().target();
        if let (Some(head), Some(tip)) = (head, tip) {
            let merged = head == tip || repo.graph_descendant_of(head, tip).unwrap_or(false);
            if !merged {
                return Err(AppError::Other(format!(
                    "branch '{name}' is not merged — use force to delete anyway"
                )));
            }
        }
    }
    b.delete().map_err(app)?;
    Ok(())
}

/// Set (Some("origin/main")) or unset (None) a branch's upstream. Unsetting a
/// branch that never had one is a no-op (git2 errors on the missing config key).
pub fn branch_set_upstream(repo_path: &Path, name: &str, upstream: Option<&str>) -> AppResult<()> {
    let repo = open(repo_path)?;
    let mut b = repo.find_branch(name, BranchType::Local).map_err(app)?;
    if upstream.is_none() && b.upstream().is_err() {
        return Ok(());
    }
    b.set_upstream(upstream).map_err(app)?;
    Ok(())
}

/// Check out `branch` in `checkout` (safe mode: dirty conflicts abort with
/// git2's own error). Pre-checks that no OTHER checkout holds the branch.
pub fn checkout(checkout_path: &Path, branch: &str) -> AppResult<()> {
    let repo = open(checkout_path)?;
    let occ = occupancy(&repo);
    let here = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(String::from));
    if let Some(holder) = occ.get(branch) {
        // fine when THIS checkout already holds it (no-op checkout)
        if here.as_deref() != Some(branch) {
            return Err(occupied_err(branch, holder));
        }
        return Ok(());
    }
    let refname = format!("refs/heads/{branch}");
    repo.find_reference(&refname).map_err(app)?;
    let obj = repo.revparse_single(&refname).map_err(app)?;
    let mut opts = git2::build::CheckoutBuilder::new();
    opts.safe();
    repo.checkout_tree(&obj, Some(&mut opts)).map_err(app)?;
    repo.set_head(&refname).map_err(app)?;
    Ok(())
}

// ------------------------------------------------------------- commands ----

#[tauri::command]
pub async fn project_branches_detail(
    projects: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Vec<BranchInfo>> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branches_detail", || {
            branches_detail(Path::new(&projects.path_of(id)?))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_remotes(
    projects: State<'_, ProjectService>,
    id: Uuid,
) -> AppResult<Vec<RemoteInfo>> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_remotes", || {
            remotes(Path::new(&projects.path_of(id)?))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_fetch(
    projects: State<'_, ProjectService>,
    id: Uuid,
    remote: Option<String>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_fetch", || {
            fetch(Path::new(&projects.path_of(id)?), remote.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_pull(projects: State<'_, ProjectService>, id: Uuid) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_pull", || {
            pull_ff(Path::new(&projects.path_of(id)?))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn agent_pull(agents: State<'_, AgentService>, id: Uuid) -> AppResult<()> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_pull", || {
            pull_ff(Path::new(&agents.get(id)?.worktree))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_create(
    projects: State<'_, ProjectService>,
    id: Uuid,
    name: String,
    from: Option<String>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_create", || {
            branch_create(Path::new(&projects.path_of(id)?), &name, from.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_rename(
    projects: State<'_, ProjectService>,
    id: Uuid,
    old: String,
    new: String,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_rename", || {
            branch_rename(Path::new(&projects.path_of(id)?), &old, &new)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_delete(
    projects: State<'_, ProjectService>,
    id: Uuid,
    name: String,
    force: Option<bool>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_delete", || {
            branch_delete(
                Path::new(&projects.path_of(id)?),
                &name,
                force.unwrap_or(false),
            )
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn project_branch_upstream(
    projects: State<'_, ProjectService>,
    id: Uuid,
    name: String,
    upstream: Option<String>,
) -> AppResult<()> {
    let projects = projects.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("project_branch_upstream", || {
            branch_set_upstream(Path::new(&projects.path_of(id)?), &name, upstream.as_deref())
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn agent_checkout(
    agents: State<'_, AgentService>,
    id: Uuid,
    branch: String,
) -> AppResult<()> {
    let agents = agents.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("agent_checkout", || {
            checkout(Path::new(&agents.get(id)?.worktree), &branch)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;

    fn commit_file(repo_path: &Path, name: &str, msg: &str) {
        std::fs::write(repo_path.join(name), msg).unwrap();
        let repo = Repository::open(repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("t", "t@example.com").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }

    fn repo_with_commit() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        dir
    }

    #[test]
    fn detail_reports_current_and_occupancy() {
        let dir = repo_with_commit();
        branch_create(dir.path(), "feature", None).unwrap();
        let infos = branches_detail(dir.path()).unwrap();
        let main = infos.iter().find(|b| b.current).unwrap();
        assert_eq!(main.checked_out_in.as_deref(), Some(MAIN_CHECKOUT));
        let feature = infos.iter().find(|b| b.name == "feature").unwrap();
        assert!(!feature.current);
        assert!(feature.checked_out_in.is_none());
    }

    #[test]
    fn worktree_occupancy_blocks_checkout_rename_delete() {
        let dir = repo_with_commit();
        branch_create(dir.path(), "held", None).unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let wt_dir = dir.path().join(".wt-held");
        let mut opts = git2::WorktreeAddOptions::new();
        let held_ref = repo.find_reference("refs/heads/held").unwrap();
        opts.reference(Some(&held_ref));
        repo.worktree("held-wt", &wt_dir, Some(&opts)).unwrap();

        let infos = branches_detail(dir.path()).unwrap();
        let held = infos.iter().find(|b| b.name == "held").unwrap();
        assert_eq!(held.checked_out_in.as_deref(), Some("held-wt"));

        assert!(branch_rename(dir.path(), "held", "renamed").is_err());
        assert!(branch_delete(dir.path(), "held", true).is_err());
        // and the MAIN checkout cannot steal it either
        assert!(checkout(dir.path(), "held").is_err());
    }

    #[test]
    fn create_checkout_rename_delete_roundtrip() {
        let dir = repo_with_commit();
        branch_create(dir.path(), "feature", None).unwrap();
        checkout(dir.path(), "feature").unwrap();
        let head = Repository::open(dir.path())
            .unwrap()
            .head()
            .unwrap()
            .shorthand()
            .unwrap()
            .to_string();
        assert_eq!(head, "feature");
        // occupied now — rename refused; go back, then rename + delete work
        assert!(branch_rename(dir.path(), "feature", "feat2").is_err());
        let main = branches_detail(dir.path())
            .unwrap()
            .into_iter()
            .find(|b| b.name != "feature")
            .unwrap();
        checkout(dir.path(), &main.name).unwrap();
        branch_rename(dir.path(), "feature", "feat2").unwrap();
        branch_delete(dir.path(), "feat2", false).unwrap();
        assert!(branches_detail(dir.path())
            .unwrap()
            .iter()
            .all(|b| b.name != "feat2"));
    }

    #[test]
    fn unmerged_delete_needs_force() {
        let dir = repo_with_commit();
        branch_create(dir.path(), "wip", None).unwrap();
        checkout(dir.path(), "wip").unwrap();
        commit_file(dir.path(), "b.txt", "wip work");
        let main = branches_detail(dir.path())
            .unwrap()
            .into_iter()
            .find(|b| b.name != "wip")
            .unwrap();
        checkout(dir.path(), &main.name).unwrap();
        assert!(branch_delete(dir.path(), "wip", false).is_err());
        branch_delete(dir.path(), "wip", true).unwrap();
    }

    #[test]
    fn fetch_and_pull_work_against_a_local_path_remote() {
        // origin repo with one commit
        let origin = repo_with_commit();
        // clone via CLI to wire origin + upstream exactly like a real checkout
        let clone_parent = tempfile::tempdir().unwrap();
        let clone_path = clone_parent.path().join("clone");
        let out = crate::core::proc::cmd("git")
            .args([
                "clone",
                origin.path().to_str().unwrap(),
                clone_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

        // new upstream commit → fetch sees it, pull ffs onto it
        commit_file(origin.path(), "b.txt", "second");
        fetch(&clone_path, Some("origin")).unwrap();
        pull_ff(&clone_path).unwrap();
        assert!(clone_path.join("b.txt").exists());

        let rl = remotes(&clone_path).unwrap();
        assert_eq!(rl.len(), 1);
        assert_eq!(rl[0].name, "origin");

        let infos = branches_detail(&clone_path).unwrap();
        let cur = infos.iter().find(|b| b.current).unwrap();
        assert!(cur.upstream.as_deref().unwrap_or("").starts_with("origin/"));
        assert_eq!((cur.ahead, cur.behind), (0, 0));
    }

    #[test]
    fn upstream_set_and_unset() {
        let dir = repo_with_commit();
        branch_create(dir.path(), "feature", None).unwrap();
        // no remote branch exists — setting a bogus upstream errors cleanly
        assert!(branch_set_upstream(dir.path(), "feature", Some("origin/nope")).is_err());
        // unset on a branch without upstream is a no-op success in git2
        branch_set_upstream(dir.path(), "feature", None).unwrap();
    }
}
