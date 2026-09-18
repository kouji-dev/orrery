//! Plan 06, Task 4. Four assertions, and every one of them is about the broker.

mod common;

use std::time::Duration;

use common::{TestBroker, probe_exe};
use orrery_ext_api::NativeExtension;
use orrery_proto::Outcome;
use orrery_tools::ToolBudget;
use tokio_util::sync::CancellationToken;

use orrery_ext_tools_builtin::BuiltinTools;

/// 4 KB, the ceiling every bounded test uses.
const CEILING: u64 = 4 * 1024;

/// What `LimitedReader` is allowed to pull past the ceiling: one chunk.
const CHUNK: u64 = 8 * 1024;

fn text_of(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { .. } | Outcome::Truncated { .. } => value_text(value_of(outcome)),
        other => panic!("expected content, got {other:?}"),
    }
}

fn value_of(outcome: &Outcome) -> Option<&serde_json::Value> {
    match outcome {
        Outcome::Ok { value, .. } => value.as_ref(),
        _ => None,
    }
}

fn value_text(value: Option<&serde_json::Value>) -> String {
    value
        .and_then(|v| v.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// A 10 MB file read under a 4 KB ceiling comes back `Truncated` — and, the
/// half that matters, **nothing ever pulled more than the ceiling plus one
/// buffer**. A limit applied after the read has already put 10 MB in the heap.
#[tokio::test]
async fn read_respects_the_output_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.txt");
    std::fs::write(&big, "x".repeat(10 * 1024 * 1024)).unwrap();

    let broker = TestBroker::open(
        dir.path(),
        ToolBudget::new(30_000, CEILING),
        CancellationToken::new(),
    );
    let tools = BuiltinTools::new();
    let outcome = tools
        .call(
            "read",
            serde_json::json!({ "path": big.display().to_string() }),
            &broker.ctx("read"),
        )
        .await
        .expect("the harness carried the call");

    match &outcome {
        Outcome::Truncated {
            bytes_emitted,
            limit,
            ..
        } => {
            assert_eq!(*limit, CEILING);
            assert!(*bytes_emitted <= CEILING, "emitted {bytes_emitted} bytes");
        }
        other => panic!("a 10 MB file under a 4 KB ceiling must truncate, got {other:?}"),
    }

    let peak = broker.peak_pull();
    assert!(
        peak <= CEILING + CHUNK,
        "peak memory is not bounded: {peak} bytes were pulled for a {CEILING}-byte ceiling"
    );
}

/// Cancel while the bytes are going out: the original file is exactly as it
/// was, and no temporary file is left beside it.
#[tokio::test]
async fn write_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("kept.txt");
    std::fs::write(&target, "the original").unwrap();

    let cancel = CancellationToken::new();
    let broker = TestBroker::open(
        dir.path(),
        ToolBudget::new(30_000, 64 << 20),
        cancel.clone(),
    );
    let tools = BuiltinTools::new();

    // Big enough that the write goes out in many chunks, so the cancel lands in
    // the middle of one rather than before the first.
    let content = "y".repeat(8 * 1024 * 1024);
    let call = tokio::spawn({
        let ctx = broker.ctx("write");
        let path = target.display().to_string();
        async move {
            BuiltinTools::new()
                .call("write", serde_json::json!({ "path": path, "content": content }), &ctx)
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(15)).await;
    cancel.cancel();

    let outcome = call.await.unwrap().expect("the harness carried the call");
    assert!(
        matches!(outcome, Outcome::Cancelled { .. }),
        "a cancelled write settles cancelled, got {outcome:?}"
    );
    let _ = tools;

    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "the original",
        "the original must survive a cancelled write untouched"
    );
    let strays: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".orrery-") || n.ends_with(".tmp"))
        .collect();
    assert!(strays.is_empty(), "a temporary file was left behind: {strays:?}");
}

/// A child that starts a grandchild and then ignores everything: when the call's
/// wall clock runs out, **both** are gone. A `Child::kill` would reach only the
/// first of them.
#[tokio::test]
async fn bash_is_contained() {
    let dir = tempfile::tempdir().unwrap();
    let beacon = dir.path().join("alive.log");
    let probe = probe_exe();
    assert!(
        probe.exists(),
        "the contain_probe example must be built beside the test: {}",
        probe.display()
    );

    let broker = TestBroker::open(
        dir.path(),
        ToolBudget::new(700, 1 << 20),
        CancellationToken::new(),
    );
    // Unquoted on purpose: `cmd /C` eats the outer pair of quotes in a command
    // line that begins with one, and neither path here contains a space.
    let command = format!(
        "{probe} spawn {beacon}",
        probe = probe.display(),
        beacon = beacon.display()
    );
    let outcome = BuiltinTools::new()
        .call(
            "bash",
            serde_json::json!({ "command": command }),
            &broker.ctx("bash"),
        )
        .await
        .expect("the harness carried the call");
    assert!(
        !matches!(outcome, Outcome::Denied { .. }),
        "spawn is granted in this rig, got {outcome:?}"
    );

    // Give the tree time to notice, then watch: a grandchild still running
    // would keep appending.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let first = std::fs::metadata(&beacon).map(|m| m.len()).unwrap_or(0);
    assert!(
        first > 0,
        "the grandchild never ran, so nothing was contained; the call said {outcome:?}"
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    let second = std::fs::metadata(&beacon).map(|m| m.len()).unwrap_or(0);
    assert_eq!(
        first, second,
        "the grandchild outlived the call: the beacon grew from {first} to {second}"
    );
}

/// Searching a tree pulls no more than the ceiling out of any one file.
#[tokio::test]
async fn grep_streams() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/small.txt"),
        "a needle in here\nand nothing else\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/huge.txt"),
        format!("{}\nneedle at the end\n", "z".repeat(10 * 1024 * 1024)),
    )
    .unwrap();

    let broker = TestBroker::open(
        dir.path(),
        ToolBudget::new(30_000, CEILING),
        CancellationToken::new(),
    );
    let outcome = BuiltinTools::new()
        .call(
            "grep",
            serde_json::json!({ "pattern": "needle", "path": dir.path().display().to_string() }),
            &broker.ctx("grep"),
        )
        .await
        .expect("the harness carried the call");

    let text = text_of(&outcome);
    assert!(
        text.contains("small.txt"),
        "the match in the small file must be reported: {text}"
    );
    let peak = broker.peak_pull();
    assert!(
        peak <= CEILING + CHUNK,
        "grep buffered a whole file: {peak} bytes for a {CEILING}-byte ceiling"
    );
}

/// `glob` lists what matches and nothing else.
#[tokio::test]
async fn glob_lists_matches_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
    std::fs::write(dir.path().join("src/b.txt"), "").unwrap();

    let broker = TestBroker::open(
        dir.path(),
        ToolBudget::new(30_000, CEILING),
        CancellationToken::new(),
    );
    let outcome = BuiltinTools::new()
        .call(
            "glob",
            serde_json::json!({ "pattern": "**/*.rs", "path": dir.path().display().to_string() }),
            &broker.ctx("glob"),
        )
        .await
        .expect("the harness carried the call");
    let text = text_of(&outcome);
    assert!(text.contains("a.rs"), "`a.rs` matches `**/*.rs`: {text}");
    assert!(!text.contains("b.txt"), "`b.txt` does not: {text}");
}

/// An edit rewrites the file it was pointed at, through the broker, atomically.
#[tokio::test]
async fn edit_replaces_through_the_broker() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("edit.txt");
    std::fs::write(&file, "hello world\n").unwrap();

    let broker = TestBroker::open(
        dir.path(),
        ToolBudget::new(30_000, 1 << 20),
        CancellationToken::new(),
    );
    let outcome = BuiltinTools::new()
        .call(
            "edit",
            serde_json::json!({
                "path": file.display().to_string(),
                "old_text": "world",
                "new_text": "harness",
            }),
            &broker.ctx("edit"),
        )
        .await
        .expect("the harness carried the call");
    assert!(outcome.is_ok(), "the edit should apply: {outcome:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello harness\n");
}
