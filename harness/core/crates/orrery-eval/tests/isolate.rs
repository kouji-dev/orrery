//! Task 3: cases never share state, cleanup survives a panic, concurrency is
//! honoured.

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use common::store::MemoryStore;
use common::{ScriptedProvider, fixture};
use orrery_eval::case::{EvalCase, GraderSpec, Suite, WorkspaceSpec};
use orrery_eval::matrix::{Matrix, MatrixPoint};
use orrery_eval::report::CostProvenance;
use orrery_eval::run::{
    CaseCtx, CaseRunner, EvalRun, HarnessRunner, Isolation, RoleBinding, RunOutput,
};
use orrery_eval::{EvalError, EvalRunner, GradeInput, Grader, Isolator, Score};
use orrery_grader::GradeError;
use orrery_proto::Role;
use orrery_session::SessionStore;

fn point(profile: &str) -> MatrixPoint {
    MatrixPoint {
        profile: profile.to_owned(),
        model: "m".to_owned(),
        seed: None,
    }
}

fn case(id: &str) -> EvalCase {
    EvalCase::new(id, "do the thing", GraderSpec::new("noop"))
}

#[test]
fn cases_never_share_state() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let isolator = Isolator::new(tmp.path());

    let first = isolator.prepare(&case("a"), &point("p")).expect("first");
    let second = isolator.prepare(&case("b"), &point("p")).expect("second");

    // Both cases write the same relative path.
    std::fs::write(first.path().join("shared.txt"), "from a").expect("write a");
    std::fs::write(second.path().join("shared.txt"), "from b").expect("write b");

    assert_eq!(
        std::fs::read_to_string(first.path().join("shared.txt")).expect("read a"),
        "from a",
        "case b leaked into case a"
    );
    assert_eq!(
        std::fs::read_to_string(second.path().join("shared.txt")).expect("read b"),
        "from b",
        "case a leaked into case b"
    );
    assert_ne!(first.path(), second.path());
}

#[test]
fn the_same_case_twice_gets_two_workspaces() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let isolator = Isolator::new(tmp.path());
    let a = isolator.prepare(&case("same"), &point("p")).expect("a");
    let b = isolator.prepare(&case("same"), &point("p")).expect("b");
    assert_ne!(a.path(), b.path(), "a re-run must not reuse a workspace");
}

#[test]
fn worktree_is_cleaned_up_after_a_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let isolator = Isolator::new(tmp.path());
    let leaked: Arc<std::sync::Mutex<Option<PathBuf>>> = Arc::new(std::sync::Mutex::new(None));

    let seen = Arc::clone(&leaked);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let workspace = isolator
            .prepare(&case("panics"), &point("p"))
            .expect("prep");
        *seen.lock().expect("lock") = Some(workspace.path().to_path_buf());
        std::fs::write(workspace.path().join("half-done.txt"), "…").expect("write");
        panic!("the case blew up");
    }));

    assert!(result.is_err(), "the panic propagated");
    let path = leaked.lock().expect("lock").clone().expect("a workspace");
    assert!(
        !path.exists(),
        "the workspace survived the panic: {}",
        path.display()
    );
}

#[test]
fn a_fixture_workspace_is_copied_not_shared() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let source = tmp.path().join("fixture-src");
    std::fs::create_dir_all(source.join("nested")).expect("mkdir");
    std::fs::write(source.join("nested").join("seed.txt"), "original").expect("seed");

    let isolator = Isolator::new(tmp.path().join("runs"));
    let workspace = isolator
        .prepare(
            &case("fx").in_workspace(WorkspaceSpec::Fixture {
                archive: source.clone(),
            }),
            &point("p"),
        )
        .expect("prepared");

    let copied = workspace.path().join("nested").join("seed.txt");
    assert_eq!(std::fs::read_to_string(&copied).expect("read"), "original");
    std::fs::write(&copied, "changed").expect("overwrite");
    assert_eq!(
        std::fs::read_to_string(source.join("nested").join("seed.txt")).expect("read source"),
        "original",
        "the case wrote through to the fixture it was copied from"
    );
}

#[test]
fn container_isolation_says_so_by_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = Isolator::new(tmp.path())
        .with_isolation(Isolation::Container)
        .prepare(&case("c"), &point("p"))
        .expect_err("phase 9 ships worktree isolation only");
    assert!(matches!(err, EvalError::Unsupported { .. }));
    assert!(err.to_string().contains("container"));
}

#[test]
fn a_repo_workspace_is_a_git_worktree() {
    let Some(repo) = tiny_repo() else {
        // git is how this mode isolates; without it there is nothing to assert.
        eprintln!("skipped: git is not on PATH");
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let isolator = Isolator::new(tmp.path().join("runs"));
    let path = {
        let workspace = isolator
            .prepare(
                &case("repo").in_workspace(WorkspaceSpec::Repo {
                    repo: repo.path().to_path_buf(),
                    commit: None,
                }),
                &point("p"),
            )
            .expect("git worktree add");
        // Trimmed: `core.autocrlf` rewrites the line ending on checkout, and
        // the line ending is not what this test is about.
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("file.txt"))
                .expect("checked out")
                .trim(),
            "committed"
        );
        // The worktree is a real checkout, not a copy: git knows about it.
        let listed = std::process::Command::new("git")
            .current_dir(repo.path())
            .args(["worktree", "list"])
            .output()
            .expect("git worktree list");
        assert!(
            String::from_utf8_lossy(&listed.stdout)
                .to_lowercase()
                .contains(
                    &workspace
                        .path()
                        .file_name()
                        .expect("name")
                        .to_string_lossy()
                        .to_lowercase()
                )
        );
        workspace.path().to_path_buf()
    };
    assert!(!path.exists(), "the worktree was not removed on drop");
}

/// A repository with one commit in it, or `None` when git is missing.
fn tiny_repo() -> Option<tempfile::TempDir> {
    let dir = tempfile::tempdir().ok()?;
    let git = |args: &[&str]| -> bool {
        std::process::Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    if !git(&["init", "-q"]) {
        return None;
    }
    git(&["config", "user.email", "eval@example.invalid"]);
    git(&["config", "user.name", "eval"]);
    std::fs::write(dir.path().join("file.txt"), "committed\n").ok()?;
    git(&["add", "."]);
    git(&["commit", "-qm", "one"]).then_some(dir)
}

/// Records how many cases were in flight at once.
struct Watcher {
    live: AtomicUsize,
    peak: AtomicUsize,
}

#[async_trait]
impl CaseRunner for Watcher {
    fn label(&self) -> &str {
        "watcher"
    }

    fn cost_provenance(&self) -> CostProvenance {
        CostProvenance::MeasuredAtProviderBoundary
    }

    async fn run(&self, _ctx: CaseCtx<'_>) -> Result<RunOutput, EvalError> {
        let live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(live, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        self.live.fetch_sub(1, Ordering::SeqCst);
        Ok(RunOutput {
            stopped_by: None,
            cost: orrery_proto::Usage::default(),
            by_role: orrery_eval::report::ByRole::new(),
            timing: orrery_eval::Timing::default(),
            turns: 0,
            tool_calls: 0,
            transcript: orrery_proto::SessionRef {
                session: orrery_proto::SessionId::new(),
                branch: orrery_proto::BranchId::new(),
                turn: None,
            },
        })
    }
}

struct Noop;

#[async_trait]
impl Grader for Noop {
    fn id(&self) -> &str {
        "noop"
    }

    async fn grade(&self, _input: GradeInput) -> Result<Score, GradeError> {
        Ok(Score::pass())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_is_honoured() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let watcher = Arc::new(Watcher {
        live: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
    });
    let suite = (0..6).fold(Suite::new("many"), |s, i| {
        s.with_case(case(&format!("c{i}")))
    });

    let report = EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner("p", Arc::clone(&watcher) as Arc<dyn CaseRunner>)
        .with_grader(Arc::new(Noop))
        .run(
            &EvalRun::new("many", Matrix::new(["p"], ["m"])).with_concurrency(2),
            &suite,
        )
        .await
        .expect("the run");

    assert_eq!(report.results.len(), 6);
    let peak = watcher.peak.load(Ordering::SeqCst);
    assert!(peak <= 2, "concurrency 2 was exceeded: {peak} in flight");
    assert!(peak > 1, "nothing ran concurrently at all");

    // And the order is deterministic whatever order they finished in.
    let keys: Vec<String> = report.results.iter().map(|r| r.case.clone()).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[tokio::test]
async fn a_case_runs_in_its_own_workspace() {
    // The runner must hand the case the isolated path, not the process cwd.
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let mut bindings = HashMap::new();
    bindings.insert(
        Role::Planner,
        RoleBinding {
            provider: Arc::new(ScriptedProvider::from_fixture(
                "fixture",
                fixture("planner.jsonl"),
            )),
            model: "m".to_owned(),
        },
    );

    let base = tmp.path().to_path_buf();
    let report = EvalRunner::new(Isolator::new(&base))
        .with_runner(
            "p",
            Arc::new(HarnessRunner::new("ours", Arc::clone(&store), bindings)),
        )
        .with_grader(Arc::new(Noop))
        .run(
            &EvalRun::new("one", Matrix::new(["p"], ["m"])),
            &Suite::new("one").with_case(case("w")),
        )
        .await
        .expect("the run");

    let handle = store
        .open(report.results[0].transcript.session)
        .await
        .expect("session");
    assert!(
        handle.workspace.starts_with(&base.display().to_string()),
        "the session recorded `{}`, which is not under the isolation root",
        handle.workspace
    );
}
