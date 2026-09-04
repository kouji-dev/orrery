//! Every backend test, written once against `&dyn GitBackend` and run for
//! each implementation by `backend_tests!` below (gix is the only one today;
//! `for_each_backend!` keeps the shape for a second). Raw `git2` calls are
//! allowed here as an ORACLE — fixtures and state assertions made by an
//! independent implementation — which is why git2 stays a dev-dependency.

use std::path::Path;

use git2::{Repository, Signature};

use super::backend::GitBackend;
use super::gix_backend::GixBackend;
use super::types::*;

macro_rules! backend_tests {
    ($module:ident = $ctor:expr; $($name:ident),* $(,)?) => {
        mod $module {
            use super::*;
            $( #[test] fn $name() { super::$name(&$ctor); } )*
        }
    };
}

/// The same list, once per implementation.
macro_rules! for_each_backend {
    ($($name:ident),* $(,)?) => {
        backend_tests!(on_gix = GixBackend::new(); $($name),*);
    };
}

// ── service-level tests ──────────────────────────────────────────────────

    fn head_oid_none_unborn_then_moves_on_commit(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        assert!(b.head_oid(dir.path()).is_none(), "unborn HEAD");
        commit_file(dir.path(), "a.txt", "first");
        let h1 = b.head_oid(dir.path()).unwrap();
        commit_file(dir.path(), "b.txt", "second");
        let h2 = b.head_oid(dir.path()).unwrap();
        assert_ne!(h1, h2, "HEAD moves on commit");
        assert_eq!(h2.len(), 40, "full oid hex, not short sha");
    }
    fn detect_then_init_then_detect(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        assert!(!b.detect(dir.path()), "fresh dir is not a repo");
        b.init(dir.path()).unwrap();
        assert!(b.detect(dir.path()), "after init it is a repo");
    }
    fn head_info_is_none_without_commits(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        assert!(b.head_info(dir.path()).is_none());
    }

    fn commit_file(repo_path: &Path, name: &str, msg: &str) {

        std::fs::write(repo_path.join(name), "x").unwrap();
        let repo = Repository::open(repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "t@t").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap();
    }
    fn log_is_empty_without_commits(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        assert!(b.log(dir.path(), 10, 0).is_empty());
    }
    fn log_files_counts_changed_files_exactly(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // one commit touching two files

        std::fs::write(dir.path().join("x.txt"), "x\n").unwrap();
        std::fs::write(dir.path().join("y.txt"), "y\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("x.txt")).unwrap();
        index.add_path(Path::new("y.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("Test", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "two files", &tree, &[&parent])
            .unwrap();

        let log = b.log(dir.path(), 10, 0);
        assert_eq!(log[0].files, 2, "two-file commit counts 2");
        assert_eq!(log[1].files, 1, "single-file commit counts 1");
    }
    fn log_offset_pages_through_history(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        for i in 0..5 {
            commit_file(dir.path(), &format!("f{i}.txt"), &format!("c{i}"));
        }
        let p1 = b.log(dir.path(), 2, 0);
        let p2 = b.log(dir.path(), 2, 2);
        let p3 = b.log(dir.path(), 2, 4);
        assert_eq!(
            p1.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["c4", "c3"]
        );
        assert_eq!(
            p2.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["c2", "c1"]
        );
        assert_eq!(
            p3.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["c0"]
        );
        assert!(
            b.log(dir.path(), 2, 99).is_empty(),
            "past-the-end offset is empty"
        );
    }
    fn commit_stages_all_and_records(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "n\n").unwrap();
        b.commit(dir.path(), "wip changes", &[]).unwrap();
        assert!(b.status(dir.path()).is_empty(), "clean after commit");
        assert_eq!(b.log(dir.path(), 10, 0)[0].message, "wip changes");
    }
    fn commit_only_selected_paths(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();
        b.commit(dir.path(), "only a", &["a.txt".to_string()])
            .unwrap();
        let st = b.status(dir.path());
        assert!(
            !st.iter().any(|c| c.path == "a.txt"),
            "a.txt committed: {st:?}"
        );
        assert!(
            st.iter().any(|c| c.path == "b.txt"),
            "b.txt still pending: {st:?}"
        );
    }
    fn discard_resets_and_cleans(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first"); // committed content = "x"
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("untracked.txt"), "x\n").unwrap();
        b.discard(dir.path(), &[]).unwrap();
        assert!(b.status(dir.path()).is_empty(), "clean after discard");
        assert!(
            !dir.path().join("untracked.txt").exists(),
            "untracked removed"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "x"
        );
    }
    fn merge_fast_forward(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        let main = repo.head().unwrap().shorthand().unwrap().to_string();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base, false).unwrap();

        let ff = |to: &str| {
            repo.set_head(to).unwrap();
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_head(Some(&mut co)).unwrap();
        };
        ff("refs/heads/feature");
        commit_file(dir.path(), "b.txt", "second");
        let feat_tip = repo.head().unwrap().peel_to_commit().unwrap().id();
        ff(&format!("refs/heads/{main}"));

        b.merge(dir.path(), "feature").unwrap();
        let tip = Repository::open(dir.path())
            .unwrap()
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id();
        assert_eq!(tip, feat_tip, "main fast-forwarded to feature");
        assert!(dir.path().join("b.txt").exists());
    }
    fn push_sends_branch_to_a_local_origin(b: &dyn GitBackend) {
        let origin = tempfile::tempdir().unwrap();
        Repository::init_bare(origin.path()).unwrap();

        let work = tempfile::tempdir().unwrap();
        b.init(work.path()).unwrap();
        commit_file(work.path(), "a.txt", "hi");

        let repo = Repository::open(work.path()).unwrap();
        // forward-slash the path so the git CLI accepts it as a local remote on Windows
        let url = origin.path().to_str().unwrap().replace('\\', "/");
        repo.remote("origin", &url).unwrap();
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();

        b.push(work.path(), "origin", &branch).unwrap();

        let bare = Repository::open(origin.path()).unwrap();
        assert!(
            bare.find_reference(&format!("refs/heads/{branch}")).is_ok(),
            "origin should now have the pushed branch"
        );
    }
    fn file_diff_reports_old_and_new(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.ts", "first"); // committed content = "x"
        std::fs::write(dir.path().join("a.ts"), "const y = 2;\n").unwrap();
        let d = b.file_diff(dir.path(), "a.ts", None);
        assert_eq!(d.old, "x");
        assert_eq!(d.new, "const y = 2;\n");
        assert_eq!(d.lang, "javascript");
    }
    fn status_detects_a_move_as_one_renamed_entry(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(
            dir.path(),
            "a.txt",
            "hello world\nsecond line\nthird line\n",
        );
        // move a.txt -> sub/b.txt (unstaged, like dragging it in a file explorer)
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::rename(dir.path().join("a.txt"), dir.path().join("sub/b.txt")).unwrap();

        let st = b.status(dir.path());
        let renamed: Vec<_> = st.iter().filter(|f| f.state == "R").collect();
        assert_eq!(
            renamed.len(),
            1,
            "the move is ONE renamed entry, got: {st:?}"
        );
        assert_eq!(renamed[0].path, "sub/b.txt", "new path");
        assert_eq!(
            renamed[0].old_path.as_deref(),
            Some("a.txt"),
            "carries the old path"
        );
        assert!(
            !st.iter().any(|f| f.state == "A"),
            "no separate Added entry: {st:?}"
        );
        assert!(
            !st.iter().any(|f| f.state == "D"),
            "no separate Deleted entry: {st:?}"
        );
    }
    fn status_reports_modified_and_added(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        std::fs::write(dir.path().join("a.txt"), "x\ny\n").unwrap(); // modify
        std::fs::write(dir.path().join("new.txt"), "n\n").unwrap(); // add (untracked)
        let st = b.status(dir.path());
        assert!(
            st.iter().any(|c| c.path == "a.txt" && c.state == "M"),
            "{st:?}"
        );
        assert!(
            st.iter().any(|c| c.path == "new.txt" && c.state == "A"),
            "{st:?}"
        );
        // a brand-new untracked file must report its added line count, not +0
        let new = st.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(new.add, 1, "untracked file counts added lines: {st:?}");
        assert_eq!(new.del, 0, "untracked file has no deletions: {st:?}");
    }
    fn lists_local_branches(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &head, false).unwrap();
        let bs = b.branches(dir.path());
        assert!(bs.contains(&"feature".to_string()), "branches: {bs:?}");
        assert!(bs.len() >= 2, "default branch + feature: {bs:?}");
    }
    fn default_branch_prefers_origin_head(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        // simulate a clone whose remote declares `develop` as its default
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/develop",
            true,
            "test",
        )
        .unwrap();
        assert_eq!(b.default_branch(dir.path()).as_deref(), Some("develop"));
    }
    fn default_branch_prefers_main_over_alphabetical_first(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        // the conventional default this repo actually has (main or master,
        // depending on the git2/system default)
        let default = repo.head().unwrap().shorthand().unwrap().to_string();
        assert!(matches!(default.as_str(), "main" | "master"), "{default}");
        // a branch that sorts before it alphabetically must not win
        repo.branch("aaa-feature", &head, false).unwrap();
        assert_eq!(b.default_branch(dir.path()), Some(default));
    }
    fn default_branch_falls_back_to_head_branch(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = Repository::open(dir.path()).unwrap();
        // no origin/HEAD, no main/master: rename the default branch to `trunk`
        let head = repo.head().unwrap().shorthand().unwrap().to_string();
        let mut br = repo.find_branch(&head, git2::BranchType::Local).unwrap();
        br.rename("trunk", false).unwrap();
        repo.set_head("refs/heads/trunk").unwrap();
        assert_eq!(b.default_branch(dir.path()).as_deref(), Some("trunk"));
    }
    fn default_branch_none_for_non_repo(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(b.default_branch(dir.path()), None);
    }
    fn create_and_remove_worktree(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");

        let wt_root = tempfile::tempdir().unwrap();
        let wt_path = wt_root.path().join("my_task");
        b.create_worktree(dir.path(), "my_task", "agent/my_task", None, &wt_path)
            .unwrap();

        assert!(
            wt_path.join("a.txt").exists(),
            "worktree checked out the files"
        );
        let repo = Repository::open(dir.path()).unwrap();
        assert!(repo
            .find_branch("agent/my_task", git2::BranchType::Local)
            .is_ok());

        b.remove_worktree(dir.path(), "my_task", WorktreeDisposal::DeleteFolder)
            .unwrap();
        assert!(!wt_path.exists(), "worktree dir removed");
    }
    fn remove_worktree_keep_folder_deregisters_but_leaves_the_files(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");

        let wt_root = tempfile::tempdir().unwrap();
        let wt_path = wt_root.path().join("kept");
        b.create_worktree(dir.path(), "kept", "agent/kept", None, &wt_path)
            .unwrap();
        // an uncommitted file — exactly what a soft delete must not destroy
        std::fs::write(wt_path.join("scratch.txt"), "unsaved work").unwrap();

        b.remove_worktree(dir.path(), "kept", WorktreeDisposal::KeepFolder)
            .unwrap();

        assert!(wt_path.exists(), "folder kept");
        assert_eq!(
            std::fs::read_to_string(wt_path.join("scratch.txt")).unwrap(),
            "unsaved work",
            "uncommitted work survives"
        );
        let repo = Repository::open(dir.path()).unwrap();
        assert!(
            repo.find_worktree("kept").is_err(),
            "git no longer tracks the worktree"
        );
    }
    fn remove_worktree_is_a_noop_for_an_unknown_name(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // the agent layer deletes the folder itself; a missing registration is
        // not an error here
        b.remove_worktree(dir.path(), "never-existed", WorktreeDisposal::DeleteFolder)
            .unwrap();
    }
    fn worktree_on_empty_repo_creates_initial_commit(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        assert!(Repository::open(dir.path()).unwrap().head().is_err());

        let wt_root = tempfile::tempdir().unwrap();
        let wt_path = wt_root.path().join("t");
        b.create_worktree(dir.path(), "t", "agent/t", None, &wt_path)
            .unwrap();

        assert!(wt_path.exists());
        assert!(
            Repository::open(dir.path()).unwrap().head().is_ok(),
            "now has a HEAD"
        );
    }

    // ── tree diff tests (via range_diff / commit_files) ───────────────────

    /// Two commits: a.txt = "x\n", then a.txt = "x\ny\n". Returns
    /// (repo_path, first sha, second sha). The temp directory is kept alive
    /// until the OS cleans it up on process exit.
    fn two_commit_repo(b: &dyn GitBackend) -> (std::path::PathBuf, String, String) {
        let path = tempfile::tempdir().unwrap().keep();
        b.init(&path).unwrap();
        let c1 = commit_content(&path, "a.txt", "x\n", "first");
        let c2 = commit_content(&path, "a.txt", "x\ny\n", "second");
        (path, c1, c2)
    }

    fn diff_treeish_modified_file_add_del_and_state(b: &dyn GitBackend) {
        let (path, from, to) = two_commit_repo(b);
        let changes = b.range_diff(&path, &from, &to).unwrap();

        assert_eq!(changes.len(), 1, "exactly one changed file: {changes:?}");
        let fc = &changes[0];
        assert_eq!(fc.path, "a.txt", "path: {changes:?}");
        assert_eq!(fc.state, "M", "state should be M (modified): {changes:?}");
        assert_eq!(fc.add, 1, "one line added: {changes:?}");
        assert_eq!(fc.del, 0, "no lines deleted: {changes:?}");
        assert!(fc.old_path.is_none(), "no old_path for M: {changes:?}");
    }

    fn diff_treeish_added_file_from_empty_tree(b: &dyn GitBackend) {
        let (path, first, _second) = two_commit_repo(b);
        // a root commit diffs against the empty tree: a.txt shows as Added
        let changes = b.commit_files(&path, &first).unwrap();

        assert_eq!(changes.len(), 1, "one file added from empty: {changes:?}");
        let fc = &changes[0];
        assert_eq!(fc.state, "A", "state should be A (added): {changes:?}");
        assert!(fc.add > 0, "added lines > 0: {changes:?}");
    }

    fn diff_treeish_rename_shows_r_state(b: &dyn GitBackend) {
        let path = tempfile::tempdir().unwrap().keep();
        b.init(&path).unwrap();
        // commit 1: a.txt with enough content for rename detection
        let content = "line one\nline two\nline three\nline four\nline five\n";
        let c1 = commit_content(&path, "a.txt", content, "add a");

        // commit 2: rename a.txt → b.txt (same content) — staged via the oracle
        std::fs::rename(path.join("a.txt"), path.join("b.txt")).unwrap();
        let c2 = {
            let repo = git2::Repository::open(&path).unwrap();
            let sig = Signature::now("Test", "t@t").unwrap();
            let mut index = repo.index().unwrap();
            index.remove_path(Path::new("a.txt")).unwrap();
            index.add_path(Path::new("b.txt")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let parent = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "rename to b", &tree, &[&parent])
                .unwrap()
                .to_string()
        };
        let changes = b.range_diff(&path, &c1, &c2).unwrap();

        // Should be a single Renamed entry
        let renamed: Vec<_> = changes.iter().filter(|c| c.state == "R").collect();
        assert_eq!(renamed.len(), 1, "one rename entry: {changes:?}");
        assert_eq!(renamed[0].path, "b.txt");
        assert_eq!(renamed[0].old_path.as_deref(), Some("a.txt"));
    }

    fn ensure_main_branch_creates_main_in_empty_repo(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        assert!(
            Repository::open(dir.path()).unwrap().head().is_err(),
            "starts unborn"
        );

        b.ensure_main_branch(dir.path()).unwrap();

        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().unwrap(), "main");
        assert!(b.branches(dir.path()).contains(&"main".to_string()));
        b.ensure_main_branch(dir.path()).unwrap(); // idempotent on a repo with commits
    }
    fn log_returns_commits_newest_first(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "b.txt", "second");
        let log = b.log(dir.path(), 10, 0);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message, "second");
        assert_eq!(log[1].message, "first");
        assert!(log[0].files >= 1);
    }

    // ── commit_files tests ─────────────────────────────────────────────────
    fn commit_files_reports_changed_file_with_add_del(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        // second commit modifies a.txt
        std::fs::write(dir.path().join("a.txt"), "x\ny\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "modify a", &tree, &[&parent])
            .unwrap();

        let changes = b.commit_files(dir.path(), &oid.to_string()).unwrap();
        assert_eq!(changes.len(), 1, "one file changed: {changes:?}");
        assert_eq!(changes[0].path, "a.txt");
        assert_eq!(changes[0].state, "M");
        assert!(changes[0].add > 0 || changes[0].del > 0);
    }
    fn commit_files_root_commit_shows_added(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        std::fs::write(dir.path().join("init.txt"), "hello\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("init.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "root", &tree, &[])
            .unwrap();

        let changes = b.commit_files(dir.path(), &oid.to_string()).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].state, "A", "root commit file is Added");
    }

    // ── commit_file_diff tests (pathspec-limited) ──────────────────────────

    /// Helper: stage `name` with `content` and commit it onto HEAD, returning the sha.
    fn commit_content(repo_path: &Path, name: &str, content: &str, msg: &str) -> String {
        std::fs::create_dir_all(repo_path.join(name).parent().unwrap()).ok();
        std::fs::write(repo_path.join(name), content).unwrap();
        let repo = git2::Repository::open(repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo
            .head()
            .ok()
            .and_then(|h| h.target())
            .and_then(|oid| repo.find_commit(oid).ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)
            .unwrap()
            .to_string()
    }
    fn commit_file_diff_returns_old_and_new_for_nested_path(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "src/foo.txt", "line1\n", "init");
        let sha = commit_content(dir.path(), "src/foo.txt", "line1\nline2\n", "add line2");

        let fd = b.commit_file_diff(dir.path(), &sha, "src/foo.txt").unwrap();
        assert!(fd.new.contains("line2"), "new content has line2: {fd:?}");
        assert!(fd.new.contains("line1"));
        assert!(fd.old.contains("line1"));
        assert!(!fd.old.contains("line2"), "old content lacks line2");
    }
    fn commit_file_diff_isolates_one_file_among_many(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "aaa\n", "init a");
        // a commit that touches BOTH a.txt and b.txt
        std::fs::write(dir.path().join("a.txt"), "aaa\nAAA\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.add_path(Path::new("b.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let sha = repo
            .commit(Some("HEAD"), &sig, &sig, "touch both", &tree, &[&parent])
            .unwrap()
            .to_string();

        let fd = b.commit_file_diff(dir.path(), &sha, "a.txt").unwrap();
        assert!(fd.new.contains("AAA"), "a.txt diff has its own content: {fd:?}");
        assert!(!fd.new.contains("bbb"), "a.txt diff must not bleed b.txt content");
    }
    fn range_file_diff_returns_content_for_path(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        let from = commit_content(dir.path(), "f.txt", "v1\n", "v1");
        commit_content(dir.path(), "f.txt", "v1\nv2\n", "v2");
        let to = commit_content(dir.path(), "f.txt", "v1\nv2\nv3\n", "v3");

        let fd = b.range_file_diff(dir.path(), &from, &to, "f.txt").unwrap();
        assert!(fd.new.contains("v3"), "range new has latest content: {fd:?}");
        assert!(fd.old.contains("v1"));
        assert!(!fd.old.contains("v3"), "range old is the from-side");
    }

    // ── file_history tests ─────────────────────────────────────────────────
    fn file_history_returns_commits_that_touch_path(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "init a");
        commit_file(dir.path(), "b.txt", "init b"); // touches b, not a
        // touch a again
        std::fs::write(dir.path().join("a.txt"), "updated\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("T", "t@t").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "update a", &tree, &[&parent])
            .unwrap();

        let history = b.file_history(dir.path(), "a.txt", 10, 0).unwrap();
        assert_eq!(history.len(), 2, "two commits touch a.txt: {history:?}");
        assert_eq!(history[0].summary, "update a", "newest first");
        assert_eq!(history[1].summary, "init a");
    }
    fn file_history_offset_pages(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        // Four commits, each changing f.txt's *content* so every commit truly
        // touches the path (the shared commit_file helper writes identical bytes,
        // which would collapse to a single touching commit).
        for i in 0..4 {
            std::fs::write(dir.path().join("f.txt"), format!("content {i}\n")).unwrap();
            let repo = git2::Repository::open(dir.path()).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("f.txt")).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = git2::Signature::now("T", "t@t").unwrap();
            let parent = repo
                .head()
                .ok()
                .and_then(|h| h.target())
                .and_then(|oid| repo.find_commit(oid).ok());
            let parents: Vec<&git2::Commit> = parent.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, &format!("c{i}"), &tree, &parents)
                .unwrap();
        }
        let all = b.file_history(dir.path(), "f.txt", 10, 0).unwrap();
        assert_eq!(all.len(), 4);
        let page = b.file_history(dir.path(), "f.txt", 2, 2).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].summary, all[2].summary);
    }

    // ── blame tests ────────────────────────────────────────────────────────
    fn blame_returns_one_entry_per_line(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        // write a 3-line file
        std::fs::write(dir.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();
        let repo = git2::Repository::open(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("f.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Author", "a@a").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add file", &tree, &[])
            .unwrap();

        let b = b.blame(dir.path(), "f.txt", None).unwrap();
        // 3 content lines + trailing empty from split
        assert!(b.lines.len() >= 3, "at least 3 blame lines: {:?}", b.lines);
        assert_eq!(b.lines[0].n, 1);
        assert_eq!(b.lines[1].n, 2);
        assert_eq!(b.commits[b.lines[0].c as usize].author, "Author");
        assert_eq!(b.lines[0].line, "line1");
        assert_eq!(b.lines[1].line, "line2");
    }

    /// The A0.6 interning contract: commit metadata appears ONCE in `commits`,
    /// per-line entries carry only a u32 index into it.
    fn blame_interns_commits_once_with_per_line_indices(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "f.txt", "a\nb\nc\n", "c1");
        commit_content(dir.path(), "f.txt", "a\nb\nc\nd\ne\n", "c2");

        let b = b.blame(dir.path(), "f.txt", None).unwrap();
        assert_eq!(
            b.commits.len(),
            2,
            "one table entry per DISTINCT commit, not per line: {:?}",
            b.commits
        );
        assert!(b.lines.len() >= 5);
        assert!(
            b.lines.iter().all(|l| (l.c as usize) < b.commits.len()),
            "every line index resolves into the commit table"
        );
        let commit_of = |n: usize| {
            let l = b.lines.iter().find(|l| l.n == n).unwrap();
            &b.commits[l.c as usize]
        };
        assert_eq!(commit_of(1).summary, "c1", "untouched lines keep c1");
        assert_eq!(commit_of(4).summary, "c2", "appended lines carry c2");
        assert_eq!(commit_of(1).sha.len(), 7, "short sha in the table");
    }
    fn working_blame_marks_uncommitted_lines_via_the_commit_table(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "f.txt", "one\ntwo\n", "init");
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\nthree\n").unwrap();

        let (_old, new) = b.working_blame(dir.path(), "f.txt").unwrap();
        let line3 = new.lines.iter().find(|l| l.n == 3).unwrap();
        assert_eq!(
            new.commits[line3.c as usize].author, "Uncommitted",
            "your fresh edit maps to the interned Uncommitted entry"
        );
        let line1 = new.lines.iter().find(|l| l.n == 1).unwrap();
        assert_eq!(
            new.commits[line1.c as usize].author, "T",
            "untouched lines keep their original author"
        );
    }

    // ── status cache tests (A2.3) ──────────────────────────────────────────
    fn status_cache_serves_unchanged_key_and_invalidates_on_worktree_edit(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "one\n", "init");

        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        let s1 = b.status(dir.path());
        assert_eq!(s1.len(), 1);
        assert_eq!((s1[0].path.as_str(), s1[0].add), ("a.txt", 1));

        // unchanged key → cached result (same shape, no recompute needed)
        let s2 = b.status(dir.path());
        assert_eq!(s2.len(), 1);
        assert_eq!((s2[0].path.as_str(), s2[0].add), ("a.txt", 1));

        // a worktree edit changes the dirty fingerprint → fresh result
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree!\n").unwrap();
        let s3 = b.status(dir.path());
        assert_eq!(s3[0].add, 2, "stale cache would still say 1: {s3:?}");

        // a brand-new untracked file also invalidates (fingerprint sees creates)
        std::fs::write(dir.path().join("new.txt"), "n\n").unwrap();
        let s4 = b.status(dir.path());
        assert!(
            s4.iter().any(|c| c.path == "new.txt"),
            "created file must appear despite unchanged index/HEAD: {s4:?}"
        );

        // commit moves index + HEAD → key changes → clean status
        b.commit(dir.path(), "all", &[]).unwrap();
        assert!(b.status(dir.path()).is_empty(), "clean after commit");
    }
    fn status_counts_only_skips_line_counts_but_not_states(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "one\n", "init");
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "x\ny\n").unwrap();

        let st = b.status_counts_only(dir.path());
        let a = st.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!(a.state, "M");
        assert_eq!((a.add, a.del), (0, 0), "counts-only: no line diffing");
        let n = st.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(n.state, "A");
        assert_eq!((n.add, n.del), (0, 0));

        // a counts-only cache entry must NOT satisfy a later full request
        let full = b.status(dir.path());
        let a = full.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!(a.add, 1, "full scan recomputes real counts: {full:?}");
        let n = full.iter().find(|c| c.path == "new.txt").unwrap();
        assert_eq!(n.add, 2, "untracked content counted on the full scan");

        // …while a full entry DOES satisfy a counts-only request (with counts)
        let again = b.status_counts_only(dir.path());
        let a = again.iter().find(|c| c.path == "a.txt").unwrap();
        assert_eq!(a.add, 1, "served from the full cache entry");
    }

    // ── range_diff tests ───────────────────────────────────────────────────
    fn range_diff_shows_files_between_two_commits(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        let repo = git2::Repository::open(dir.path()).unwrap();
        let from_oid = repo.head().unwrap().target().unwrap();
        commit_file(dir.path(), "b.txt", "second");
        let to_oid = repo.head().unwrap().target().unwrap();

        let changes = b
            .range_diff(dir.path(), &from_oid.to_string(), &to_oid.to_string())
            .unwrap();
        assert_eq!(changes.len(), 1, "b.txt added in the range");
        assert_eq!(changes[0].path, "b.txt");
        assert_eq!(changes[0].state, "A");
    }

    // ── merge session tests (A3.5 / A3.6) ──────────────────────────────────

    /// Fixture: a repo where merging `feature` into the default branch
    /// conflicts on `f.txt` (both sides edited the same line since the base).
    /// Returns (tempdir, default-branch name). Built with the commit_content
    /// helper so each commit's content is explicit.
    fn conflicted_merge_repo(b: &dyn GitBackend) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "f.txt", "shared\nbase line\ntail\n", "base");
        let repo = Repository::open(dir.path()).unwrap();
        // Why: the temp repo inherits the developer's global core.autocrlf; a
        // `true` there makes every checkout smudge LF→CRLF and byte-exact
        // working-tree assertions become platform-dependent. Pin it off.
        repo.config()
            .unwrap()
            .set_bool("core.autocrlf", false)
            .unwrap();
        let main = repo.head().unwrap().shorthand().unwrap().to_string();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base, false).unwrap();

        let checkout = |to: &str| {
            repo.set_head(to).unwrap();
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            repo.checkout_head(Some(&mut co)).unwrap();
        };
        checkout("refs/heads/feature");
        commit_content(dir.path(), "f.txt", "shared\ntheirs line\ntail\n", "theirs edit");
        checkout(&format!("refs/heads/{main}"));
        commit_content(dir.path(), "f.txt", "shared\nours line\ntail\n", "ours edit");
        (dir, main)
    }
    fn merge_conflict_returns_session_and_keeps_state(b: &dyn GitBackend) {
        let (dir, main) = conflicted_merge_repo(b);
        let session = b.merge(dir.path(), "feature").unwrap();
        assert_eq!(session.ours, main, "ours label = HEAD branch");
        assert_eq!(session.theirs, "feature");
        assert_eq!(session.conflicts.len(), 1, "{:?}", session.conflicts);
        let cf = &session.conflicts[0];
        assert_eq!(cf.path, "f.txt");
        assert!(cf.ours.contains("ours line"), "stage 2: {cf:?}");
        assert!(cf.theirs.contains("theirs line"), "stage 3: {cf:?}");
        assert!(cf.base.contains("base line"), "stage 1: {cf:?}");
        assert!(cf.merged.contains("<<<<<<<"), "workdir has markers: {cf:?}");
        assert!(!cf.resolved);

        // the merge state is KEPT, not thrown away
        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.state(), git2::RepositoryState::Merge);
        let st = b.session_state(dir.path()).unwrap();
        assert_eq!(st.state, "merge");
        assert_eq!(st.conflicts, 1);
    }
    fn merge_clean_returns_empty_conflicts(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "a\n", "base");
        let repo = Repository::open(dir.path()).unwrap();
        let main = repo.head().unwrap().shorthand().unwrap().to_string();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature", &base, false).unwrap();
        repo.set_head("refs/heads/feature").unwrap();
        let mut co = git2::build::CheckoutBuilder::new();
        co.force();
        repo.checkout_head(Some(&mut co)).unwrap();
        commit_content(dir.path(), "b.txt", "b\n", "feature adds b");
        repo.set_head(&format!("refs/heads/{main}")).unwrap();
        let mut co2 = git2::build::CheckoutBuilder::new();
        co2.force();
        repo.checkout_head(Some(&mut co2)).unwrap();
        // diverge main so it isn't a fast-forward
        commit_content(dir.path(), "c.txt", "c\n", "main adds c");

        let session = b.merge(dir.path(), "feature").unwrap();
        assert!(session.conflicts.is_empty(), "{:?}", session.conflicts);
        assert_eq!(
            b.session_state(dir.path()).unwrap().state,
            "none",
            "clean merge leaves no session"
        );
        assert!(dir.path().join("b.txt").exists(), "merged content present");
    }
    fn conflict_resolve_stages_and_merge_continue_commits(b: &dyn GitBackend) {
        let (dir, _main) = conflicted_merge_repo(b);
        b.merge(dir.path(), "feature").unwrap();

        // continue before resolving must refuse
        assert!(b.merge_continue(dir.path(), None).is_err());

        b.conflict_resolve(dir.path(), "f.txt", "shared\nresolved line\ntail\n")
            .unwrap();
        assert!(
            b.conflict_files(dir.path()).unwrap().is_empty(),
            "resolved file left the conflict index"
        );
        assert_eq!(b.session_state(dir.path()).unwrap().conflicts, 0);

        let sha = b.merge_continue(dir.path(), Some("merge feature")).unwrap();
        assert_eq!(sha.len(), 7, "short sha");
        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.state(), git2::RepositoryState::Clean, "state cleaned");
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent_count(), 2, "a true merge commit");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "shared\nresolved line\ntail\n"
        );
    }
    fn merge_abort_restores_head_and_clears_state(b: &dyn GitBackend) {
        let (dir, _main) = conflicted_merge_repo(b);
        b.merge(dir.path(), "feature").unwrap();
        assert!(std::fs::read_to_string(dir.path().join("f.txt"))
            .unwrap()
            .contains("<<<<<<<"));

        b.merge_abort(dir.path()).unwrap();

        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(repo.state(), git2::RepositoryState::Clean);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "shared\nours line\ntail\n",
            "working tree restored to HEAD (ours)"
        );
        assert_eq!(b.session_state(dir.path()).unwrap().state, "none");
    }
    fn session_state_none_on_clean_repo(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_content(dir.path(), "a.txt", "a\n", "init");
        let st = b.session_state(dir.path()).unwrap();
        assert_eq!(st.state, "none");
        assert_eq!(st.conflicts, 0);
    }

// ── branch tests ─────────────────────────────────────────────────────────

    fn br_commit_file(repo_path: &Path, name: &str, msg: &str) {
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
        br_commit_file(dir.path(), "a.txt", "first");
        dir
    }
    fn detail_reports_current_and_occupancy(b: &dyn GitBackend) {
        let dir = repo_with_commit();
        b.branch_create(dir.path(), "feature", None).unwrap();
        let infos = b.branches_detail(dir.path()).unwrap();
        let main = infos.iter().find(|b| b.current).unwrap();
        assert_eq!(main.checked_out_in.as_deref(), Some(MAIN_CHECKOUT));
        let feature = infos.iter().find(|b| b.name == "feature").unwrap();
        assert!(!feature.current);
        assert!(feature.checked_out_in.is_none());
    }
    fn worktree_occupancy_blocks_checkout_rename_delete(b: &dyn GitBackend) {
        let dir = repo_with_commit();
        b.branch_create(dir.path(), "held", None).unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let wt_dir = dir.path().join(".wt-held");
        let mut opts = git2::WorktreeAddOptions::new();
        let held_ref = repo.find_reference("refs/heads/held").unwrap();
        opts.reference(Some(&held_ref));
        repo.worktree("held-wt", &wt_dir, Some(&opts)).unwrap();

        let infos = b.branches_detail(dir.path()).unwrap();
        let held = infos.iter().find(|b| b.name == "held").unwrap();
        assert_eq!(held.checked_out_in.as_deref(), Some("held-wt"));

        assert!(b.branch_rename(dir.path(), "held", "renamed").is_err());
        assert!(b.branch_delete(dir.path(), "held", true).is_err());
        // and the MAIN checkout cannot steal it either
        assert!(b.checkout_branch(dir.path(), "held").is_err());
    }
    fn create_checkout_rename_delete_roundtrip(b: &dyn GitBackend) {
        let dir = repo_with_commit();
        b.branch_create(dir.path(), "feature", None).unwrap();
        b.checkout_branch(dir.path(), "feature").unwrap();
        let head = Repository::open(dir.path())
            .unwrap()
            .head()
            .unwrap()
            .shorthand()
            .unwrap()
            .to_string();
        assert_eq!(head, "feature");
        // occupied now — rename refused; go back, then rename + delete work
        assert!(b.branch_rename(dir.path(), "feature", "feat2").is_err());
        let main = b.branches_detail(dir.path())
            .unwrap()
            .into_iter()
            .find(|b| b.name != "feature")
            .unwrap();
        b.checkout_branch(dir.path(), &main.name).unwrap();
        b.branch_rename(dir.path(), "feature", "feat2").unwrap();
        b.branch_delete(dir.path(), "feat2", false).unwrap();
        assert!(b.branches_detail(dir.path())
            .unwrap()
            .iter()
            .all(|b| b.name != "feat2"));
    }
    fn unmerged_delete_needs_force(b: &dyn GitBackend) {
        let dir = repo_with_commit();
        b.branch_create(dir.path(), "wip", None).unwrap();
        b.checkout_branch(dir.path(), "wip").unwrap();
        br_commit_file(dir.path(), "b.txt", "wip work");
        let main = b.branches_detail(dir.path())
            .unwrap()
            .into_iter()
            .find(|b| b.name != "wip")
            .unwrap();
        b.checkout_branch(dir.path(), &main.name).unwrap();
        assert!(b.branch_delete(dir.path(), "wip", false).is_err());
        b.branch_delete(dir.path(), "wip", true).unwrap();
    }
    fn fetch_and_pull_work_against_a_local_path_remote(b: &dyn GitBackend) {
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
        br_commit_file(origin.path(), "b.txt", "second");
        b.fetch(&clone_path, Some("origin")).unwrap();
        b.pull_ff(&clone_path).unwrap();
        assert!(clone_path.join("b.txt").exists());

        let rl = b.remotes(&clone_path).unwrap();
        assert_eq!(rl.len(), 1);
        assert_eq!(rl[0].name, "origin");

        let infos = b.branches_detail(&clone_path).unwrap();
        let cur = infos.iter().find(|b| b.current).unwrap();
        assert!(cur.upstream.as_deref().unwrap_or("").starts_with("origin/"));
        assert_eq!((cur.ahead, cur.behind), (0, 0));
    }
    fn upstream_set_and_unset(b: &dyn GitBackend) {
        let dir = repo_with_commit();
        b.branch_create(dir.path(), "feature", None).unwrap();
        // no remote branch exists — setting a bogus upstream errors cleanly
        assert!(b.branch_set_upstream(dir.path(), "feature", Some("origin/nope")).is_err());
        // unset on a branch without upstream is a no-op success in git2
        b.branch_set_upstream(dir.path(), "feature", None).unwrap();
    }

    /// The libgit2 oracle agrees with the backend about the conflicted index
    /// after a merge, and about its absence after the resolution is staged.
    fn merge_conflict_index_agrees_with_git2_oracle(b: &dyn GitBackend) {
        let (dir, _main) = conflicted_merge_repo(b);
        let session = b.merge(dir.path(), "feature").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let oracle = repo.index().unwrap().conflicts().unwrap().flatten().count();
        assert_eq!(oracle, session.conflicts.len(), "git2 sees the same conflicts");
        assert_eq!(repo.state(), git2::RepositoryState::Merge);

        b.conflict_resolve(dir.path(), "f.txt", "shared
resolved
tail
")
            .unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        assert!(!repo.index().unwrap().has_conflicts(), "git2 sees the resolution staged");
        let entry = repo.index().unwrap().get_path(Path::new("f.txt"), 0);
        assert!(entry.is_some(), "one stage-0 entry for the resolved path");
    }
    /// A partial commit must not hide the OTHER file's uncommitted change:
    /// the rebuilt index may only carry stat data for files it knows match.
    fn partial_commit_keeps_other_dirty_file_visible(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "c.txt", "second");
        std::fs::write(dir.path().join("a.txt"), "a changed
").unwrap();
        std::fs::write(dir.path().join("c.txt"), "c changed
").unwrap();
        b.commit(dir.path(), "only a", &["a.txt".to_string()]).unwrap();
        let st = b.status(dir.path());
        assert!(!st.iter().any(|c| c.path == "a.txt"), "a.txt committed: {st:?}");
        assert!(
            st.iter().any(|c| c.path == "c.txt" && c.state == "M"),
            "c.txt still modified: {st:?}"
        );
        // and the oracle agrees the commit holds only a.txt's change
        let repo = Repository::open(dir.path()).unwrap();
        let tree = repo.head().unwrap().peel_to_tree().unwrap();
        let c = tree.get_path(Path::new("c.txt")).unwrap();
        let blob = repo.find_blob(c.id()).unwrap();
        assert_eq!(blob.content(), b"x", "c.txt in HEAD is the committed content");
    }
    /// Discard with a pathspec touches only the named path.
    fn discard_pathspec_leaves_other_changes(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        b.init(dir.path()).unwrap();
        commit_file(dir.path(), "a.txt", "first");
        commit_file(dir.path(), "c.txt", "second");
        std::fs::write(dir.path().join("a.txt"), "changed
").unwrap();
        std::fs::write(dir.path().join("c.txt"), "changed
").unwrap();
        std::fs::write(dir.path().join("untracked.txt"), "u
").unwrap();
        b.discard(dir.path(), &["a.txt".to_string()]).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(dir.path().join("c.txt")).unwrap(), "changed
");
        assert!(dir.path().join("untracked.txt").exists(), "unselected untracked file kept");
        let st = b.status(dir.path());
        assert!(st.iter().any(|c| c.path == "c.txt"), "c.txt still dirty: {st:?}");
    }
    /// Upstream to a LOCAL branch round-trips through the config and unsets.
    fn upstream_local_branch_set_then_unset(b: &dyn GitBackend) {
        let dir = repo_with_commit();
        b.branch_create(dir.path(), "feature", None).unwrap();
        let main = b.branches_detail(dir.path())
            .unwrap()
            .into_iter()
            .find(|i| i.current)
            .unwrap()
            .name;
        b.branch_set_upstream(dir.path(), "feature", Some(&main)).unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let cfg = repo.config().unwrap();
        assert_eq!(cfg.get_string("branch.feature.remote").unwrap(), ".");
        assert_eq!(
            cfg.get_string("branch.feature.merge").unwrap(),
            format!("refs/heads/{main}")
        );
        b.branch_set_upstream(dir.path(), "feature", None).unwrap();
        let cfg = Repository::open(dir.path()).unwrap().config().unwrap();
        assert!(cfg.get_string("branch.feature.merge").is_err(), "merge key removed");
    }

// ── hunk tests ───────────────────────────────────────────────────────────

    fn hk_commit_file(repo_path: &Path, name: &str, content: &str, msg: &str) {
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

    fn hunk_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        hk_commit_file(dir.path(), "f.txt", "one\ntwo\nthree\nfour\nfive\n", "base");
        dir
    }
    fn hunks_report_exact_modified_added_deleted_regions(b: &dyn GitBackend) {
        let dir = hunk_repo();
        // modify line 2, delete line 4, append line 6
        std::fs::write(dir.path().join("f.txt"), "one\nTWO\nthree\nfive\nsix\n").unwrap();
        let hunks = b.file_hunks(dir.path(), "f.txt").unwrap();
        assert_eq!(
            hunks,
            vec![
                Hunk { old_start: 2, old_lines: 1, new_start: 2, new_lines: 1 }, // modified
                Hunk { old_start: 4, old_lines: 1, new_start: 3, new_lines: 0 }, // deleted
                Hunk { old_start: 5, old_lines: 0, new_start: 5, new_lines: 1 }, // added
            ]
        );
    }
    fn untracked_file_is_one_added_hunk(b: &dyn GitBackend) {
        let dir = hunk_repo();
        std::fs::write(dir.path().join("new.txt"), "a\nb\n").unwrap();
        let hunks = b.file_hunks(dir.path(), "new.txt").unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!((hunks[0].new_start, hunks[0].new_lines), (1, 2));
        assert_eq!(hunks[0].old_lines, 0);
    }
    fn unchanged_file_has_no_hunks(b: &dyn GitBackend) {
        let dir = hunk_repo();
        assert!(b.file_hunks(dir.path(), "f.txt").unwrap().is_empty());
    }
    fn revert_one_hunk_restores_only_that_region(b: &dyn GitBackend) {
        let dir = hunk_repo();
        // two separated changes: line 1 and line 5
        std::fs::write(dir.path().join("f.txt"), "ONE\ntwo\nthree\nfour\nFIVE\n").unwrap();
        let hunks = b.file_hunks(dir.path(), "f.txt").unwrap();
        assert_eq!(hunks.len(), 2);
        // revert only the second (line 5) hunk
        b.revert_hunk(dir.path(), "f.txt", hunks[1].new_start).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "ONE\ntwo\nthree\nfour\nfive\n"
        );
        // the remaining hunk still reverts
        b.revert_hunk(dir.path(), "f.txt", 1).unwrap();
        assert!(b.file_hunks(dir.path(), "f.txt").unwrap().is_empty());
    }
    fn revert_reinserts_deleted_lines(b: &dyn GitBackend) {
        let dir = hunk_repo();
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\nfour\nfive\n").unwrap();
        let hunks = b.file_hunks(dir.path(), "f.txt").unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].new_lines, 0);
        b.revert_hunk(dir.path(), "f.txt", hunks[0].new_start).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "one\ntwo\nthree\nfour\nfive\n"
        );
    }
    fn revert_untracked_hunk_empties_the_file(b: &dyn GitBackend) {
        let dir = hunk_repo();
        std::fs::write(dir.path().join("new.txt"), "a\nb\n").unwrap();
        let hunks = b.file_hunks(dir.path(), "new.txt").unwrap();
        b.revert_hunk(dir.path(), "new.txt", hunks[0].new_start).unwrap();
        assert_eq!(std::fs::read(dir.path().join("new.txt")).unwrap(), b"");
    }
    fn stale_hunk_errors_instead_of_wrong_apply(b: &dyn GitBackend) {
        let dir = hunk_repo();
        std::fs::write(dir.path().join("f.txt"), "one\nTWO\nthree\nfour\nfive\n").unwrap();
        let err = b.revert_hunk(dir.path(), "f.txt", 99).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("hunk not found"), "{msg}");
    }
    fn crlf_content_reverts_cleanly(b: &dyn GitBackend) {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        hk_commit_file(dir.path(), "w.txt", "a\r\nb\r\nc\r\n", "crlf base");
        std::fs::write(dir.path().join("w.txt"), "a\r\nB!\r\nc\r\n").unwrap();
        let hunks = b.file_hunks(dir.path(), "w.txt").unwrap();
        assert_eq!(hunks.len(), 1);
        b.revert_hunk(dir.path(), "w.txt", hunks[0].new_start).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("w.txt")).unwrap(),
            "a\r\nb\r\nc\r\n"
        );
    }

for_each_backend!(
    merge_conflict_index_agrees_with_git2_oracle,
    partial_commit_keeps_other_dirty_file_visible,
    discard_pathspec_leaves_other_changes,
    upstream_local_branch_set_then_unset,
    head_oid_none_unborn_then_moves_on_commit,
    detect_then_init_then_detect,
    head_info_is_none_without_commits,
    log_is_empty_without_commits,
    log_files_counts_changed_files_exactly,
    log_offset_pages_through_history,
    commit_stages_all_and_records,
    commit_only_selected_paths,
    discard_resets_and_cleans,
    merge_fast_forward,
    push_sends_branch_to_a_local_origin,
    file_diff_reports_old_and_new,
    status_detects_a_move_as_one_renamed_entry,
    status_reports_modified_and_added,
    lists_local_branches,
    default_branch_prefers_origin_head,
    default_branch_prefers_main_over_alphabetical_first,
    default_branch_falls_back_to_head_branch,
    default_branch_none_for_non_repo,
    create_and_remove_worktree,
    remove_worktree_keep_folder_deregisters_but_leaves_the_files,
    remove_worktree_is_a_noop_for_an_unknown_name,
    worktree_on_empty_repo_creates_initial_commit,
    diff_treeish_modified_file_add_del_and_state,
    diff_treeish_added_file_from_empty_tree,
    diff_treeish_rename_shows_r_state,
    ensure_main_branch_creates_main_in_empty_repo,
    log_returns_commits_newest_first,
    commit_files_reports_changed_file_with_add_del,
    commit_files_root_commit_shows_added,
    commit_file_diff_returns_old_and_new_for_nested_path,
    commit_file_diff_isolates_one_file_among_many,
    range_file_diff_returns_content_for_path,
    file_history_returns_commits_that_touch_path,
    file_history_offset_pages,
    blame_returns_one_entry_per_line,
    blame_interns_commits_once_with_per_line_indices,
    working_blame_marks_uncommitted_lines_via_the_commit_table,
    status_cache_serves_unchanged_key_and_invalidates_on_worktree_edit,
    status_counts_only_skips_line_counts_but_not_states,
    range_diff_shows_files_between_two_commits,
    merge_conflict_returns_session_and_keeps_state,
    merge_clean_returns_empty_conflicts,
    conflict_resolve_stages_and_merge_continue_commits,
    merge_abort_restores_head_and_clears_state,
    session_state_none_on_clean_repo,
    detail_reports_current_and_occupancy,
    worktree_occupancy_blocks_checkout_rename_delete,
    create_checkout_rename_delete_roundtrip,
    unmerged_delete_needs_force,
    fetch_and_pull_work_against_a_local_path_remote,
    upstream_set_and_unset,
    hunks_report_exact_modified_added_deleted_regions,
    untracked_file_is_one_added_hunk,
    unchanged_file_has_no_hunks,
    revert_one_hunk_restores_only_that_region,
    revert_reinserts_deleted_lines,
    revert_untracked_hunk_empties_the_file,
    stale_hunk_errors_instead_of_wrong_apply,
    crlf_content_reverts_cleanly,
);
