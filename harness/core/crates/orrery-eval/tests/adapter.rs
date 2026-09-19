//! Task 7, the phase-9 acceptance criterion: one suite, two of our profiles,
//! one competing harness, the same graders.
//!
//! **No real agent CLI is ever started.** The competitor is a fake binary this
//! test writes into a temporary directory — a `.cmd` on Windows, a `#!/bin/sh`
//! script elsewhere — which prints the JSON shape the real tool prints. That is
//! also what exercises the ported Windows shim handling: a `.cmd` cannot be
//! spawned directly, and `launch_prefix` is what makes it start at all.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use common::store::MemoryStore;
use common::{ScriptedProvider, fixture};
use orrery_eval::adapter::{
    AdapterSpec, ExternalRunner, ParseMode, exe_extensions, known_adapter, launch_prefix,
    parse_reported_cost,
};
use orrery_eval::case::{EvalCase, GraderSpec, Suite};
use orrery_eval::matrix::{Matrix, MatrixPoint};
use orrery_eval::report::CostProvenance;
use orrery_eval::run::{EvalRun, HarnessRunner, RoleBinding};
use orrery_eval::{CaseRunner, EvalRunner, GradeInput, Grader, Isolator, Score};
use orrery_grader::GradeError;
use orrery_proto::Role;
use orrery_session::SessionStore;

/// The same grader for both harnesses: it looks at the workspace, and has no
/// idea which one produced it.
struct WroteTheFile;

#[async_trait]
impl Grader for WroteTheFile {
    fn id(&self) -> &str {
        "wrote-the-file"
    }

    async fn grade(&self, input: GradeInput) -> Result<Score, GradeError> {
        Ok(if input.workspace.join("out.txt").exists() {
            Score::pass()
        } else {
            Score::fail("out.txt was never written")
        })
    }
}

/// Writes a fake agent CLI and returns its path.
///
/// It writes `out.txt` into its working directory — which is the isolated case
/// workspace — and prints one JSON object carrying a usage block, exactly as
/// `claude --print --output-format json` does.
fn fake_agent(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join("fake-agent.cmd");
        std::fs::write(
            &path,
            "@echo off\r\n\
             echo written> out.txt\r\n\
             echo {\"type\":\"result\",\"usage\":{\"input_tokens\":1200,\"output_tokens\":340},\"total_cost_usd\":0.0042}\r\n",
        )
        .expect("write the fake agent");
        path
    } else {
        let path = dir.join("fake-agent");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             echo written > out.txt\n\
             echo '{\"type\":\"result\",\"usage\":{\"input_tokens\":1200,\"output_tokens\":340},\"total_cost_usd\":0.0042}'\n",
        )
        .expect("write the fake agent");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        path
    }
}

fn ours(store: Arc<dyn SessionStore>, model: &str) -> HarnessRunner {
    let mut bindings = HashMap::new();
    bindings.insert(
        Role::Planner,
        RoleBinding {
            provider: Arc::new(ScriptedProvider::from_fixture(
                "fixture",
                fixture("planner.jsonl"),
            )),
            model: model.to_owned(),
        },
    );
    HarnessRunner::new("ours", store, bindings)
}

#[tokio::test]
async fn one_suite_two_profiles_one_competitor() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let agent = fake_agent(&bin);

    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    // The fake is not on PATH, and it does not need to be: the adapter is
    // pointed straight at it, which is how a competitor is driven in a test
    // without one being installed.
    let external = ExternalRunner::at(
        AdapterSpec::new("claude-code", "fake-agent --print", ParseMode::Json),
        Arc::clone(&store),
        &agent,
    );

    let suite = Suite::new("cross").with_case(EvalCase::new(
        "write-a-file",
        "write out.txt",
        GraderSpec::new("wrote-the-file"),
    ));

    let report = EvalRunner::new(Isolator::new(tmp.path().join("runs")))
        .with_runner("review", Arc::new(ours(Arc::clone(&store), "big")))
        .with_runner("fast", Arc::new(ours(Arc::clone(&store), "small")))
        .with_runner("claude-code", Arc::new(external) as Arc<dyn CaseRunner>)
        .with_grader(Arc::new(WroteTheFile))
        .run(
            &EvalRun::new(
                "cross",
                Matrix::new(["review", "fast", "claude-code"], ["m"]),
            ),
            &suite,
        )
        .await
        .expect("the run");

    assert_eq!(report.results.len(), 3, "two profiles and one competitor");

    // The competitor's case was graded by the same grader as ours, and it wrote
    // the file, so it passes where our fixture-driven runs do not.
    let theirs = report
        .result("write-a-file", "claude-code")
        .expect("the competitor's result");
    assert_eq!(theirs.outcome, orrery_eval::EvalOutcome::Pass);

    // Its cost is labelled as theirs, not as ours.
    assert_eq!(
        theirs.cost_provenance,
        CostProvenance::ReportedByTool {
            tool: "claude-code".to_owned()
        }
    );
    assert!(!theirs.cost_provenance.is_measured());
    assert_eq!(theirs.cost.input_tokens, 1200);
    assert_eq!(theirs.cost.output_tokens, 340);
    assert_eq!(theirs.cost.micro_usd, Some(4200));
    assert!(
        theirs.by_role.is_empty(),
        "role attribution is a fact about our loop; inventing one for theirs \
         would be a made-up number"
    );

    // Ours is labelled as measured.
    for profile in ["review", "fast"] {
        let mine = report
            .result("write-a-file", profile)
            .unwrap_or_else(|| panic!("no result for {profile}"));
        assert!(mine.cost_provenance.is_measured());
    }

    // And the rendered report says so in words, next to the numbers.
    let rendered = report.render();
    assert!(rendered.contains("reported by claude-code"), "{rendered}");
    assert!(
        rendered.contains("measured at the provider boundary"),
        "{rendered}"
    );
}

#[test]
fn argv_reuses_ade_knowledge() {
    // The non-interactive argv, as `ade/src-tauri/src/agents/adapters/` resolves
    // these same binaries.
    let claude = known_adapter("claude").expect("claude");
    assert_eq!(claude.argv()[0], "claude");
    assert!(claude.command.contains("--print"));
    assert!(claude.command.contains("--output-format json"));
    assert_eq!(claude.parse, ParseMode::Json);

    let codex = known_adapter("codex").expect("codex");
    assert_eq!(codex.argv(), vec!["codex", "exec", "--json"]);
    assert_eq!(codex.parse, ParseMode::Jsonl);

    assert_eq!(known_adapter("pi").expect("pi").argv()[0], "pi");
    assert!(known_adapter("nothing-like-this").is_none());
}

#[test]
#[cfg(windows)]
fn a_windows_shim_is_launched_through_its_interpreter() {
    // The ADE's `launch_prefix`, ported: `CreateProcessW` cannot start a
    // `.cmd`, so it goes through `cmd.exe /c call`, and a `.ps1` through
    // PowerShell.
    let (program, args) = launch_prefix(Path::new(r"C:\npm\claude.cmd"));
    assert_eq!(program, "cmd.exe");
    assert_eq!(args[0], "/c");
    assert_eq!(args[1], "call");
    assert!(args[2].ends_with("claude.cmd"));

    let (program, args) = launch_prefix(Path::new(r"C:\npm\claude.ps1"));
    assert!(program.to_lowercase().contains("powershell") || program.to_lowercase() == "pwsh.exe");
    assert!(args.iter().any(|a| a == "-File"));

    // A real image needs no wrapper at all.
    let (program, args) = launch_prefix(Path::new(r"C:\npm\claude.exe"));
    assert!(program.ends_with("claude.exe"));
    assert!(args.is_empty());
}

#[test]
#[cfg(windows)]
fn the_extension_order_puts_the_bare_name_last() {
    // `""` first is what used to resolve `claude` to the Git-Bash `sh` script
    // instead of the real launcher.
    let exts = exe_extensions();
    assert_eq!(exts[0], ".exe");
    assert_eq!(exts.last().expect("last"), "");
    let cmd = exts.iter().position(|e| e == ".cmd").expect(".cmd");
    let bare = exts.iter().position(String::is_empty).expect("bare");
    assert!(cmd < bare);
    assert!(
        !exts.iter().any(|e| e == ".js"),
        ".js is in a default PATHEXT and is opened, not run"
    );
}

#[test]
fn a_tool_that_reports_nothing_is_unknown_not_free() {
    let silent = parse_reported_cost("{\"type\":\"result\"}", ParseMode::Json);
    assert!(
        !silent.reported,
        "a silent tool must not be recorded as having spent zero"
    );

    let codex = parse_reported_cost(
        "{\"type\":\"item\"}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}\n",
        ParseMode::Jsonl,
    );
    assert!(codex.reported);
    assert_eq!(codex.usage.input_tokens, 7);
    assert_eq!(codex.usage.output_tokens, 3);

    // Dollars become micro-USD as an integer, like every other price here.
    let priced = parse_reported_cost("{\"total_cost_usd\":1.5}", ParseMode::Json);
    assert_eq!(priced.usage.micro_usd, Some(1_500_000));
}

/// The half of the criterion that was built and unreachable.
///
/// `ExternalRunner` and `AdapterSpec` shipped and were exported, and nothing a
/// *suite* could say reached them: `run.rs`, `case.rs` and `matrix.rs` between
/// them mentioned `adapter` once, in a doc comment. A suite now declares the
/// competitor in `[adapter.<id>]`, the matrix grows a point for it, and the
/// runner binds it — which is what makes `orrery eval run` able to put the two
/// harnesses in one report.
#[tokio::test]
async fn a_suite_declares_the_competitor() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let agent = fake_agent(&bin);

    // The suite document, exactly as a repository would commit it — except for
    // the absolute path, which is how a test drives a competitor without
    // installing one. `which` takes a path with a separator in it as given.
    let text = format!(
        r#"
name = "cross"

[adapter.fake-agent]
command = "{agent} --print"
parse = "json"
timeout-ms = 30000

[[case]]
id = "write-a-file"
prompt = "write out.txt"

[case.workspace]
kind = "empty"

[case.grade]
grader = "wrote-the-file"
"#,
        agent = agent.display().to_string().replace('\\', "\\\\"),
    );
    let suite = Suite::from_toml(&text).expect("the suite parses");

    assert_eq!(
        suite.adapter_ids(),
        vec!["fake-agent"],
        "the suite knows its competitor"
    );

    // The matrix grows a point for it, after ours, and does not multiply it
    // across our model axis: their model is not ours to set.
    let matrix = Matrix::new(["review", "fast"], ["m"]);
    let points = matrix.expand_with_adapters(suite.adapter_ids());
    assert_eq!(
        points.iter().map(MatrixPoint::label).collect::<Vec<_>>(),
        vec!["review/m", "fast/m", "fake-agent/profile-default"],
    );

    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let report = EvalRunner::new(Isolator::new(tmp.path().join("runs")))
        .with_runner("review", Arc::new(ours(Arc::clone(&store), "big")))
        .with_runner("fast", Arc::new(ours(Arc::clone(&store), "small")))
        .with_grader(Arc::new(WroteTheFile))
        // The one new line of wiring: the suite's own declaration, resolved.
        .with_adapters(&suite, Arc::clone(&store))
        .expect("the declared adapter resolves")
        .run(&EvalRun::new("cross", matrix), &suite)
        .await
        .expect("the run");

    assert_eq!(report.results.len(), 3, "two profiles and one competitor");
    let theirs = report
        .result("write-a-file", "fake-agent")
        .expect("the competitor's result");
    assert_eq!(theirs.outcome, orrery_eval::EvalOutcome::Pass);
    assert_eq!(
        theirs.cost_provenance,
        CostProvenance::ReportedByTool {
            tool: "fake-agent".to_owned()
        }
    );
    for profile in ["review", "fast"] {
        assert!(
            report
                .result("write-a-file", profile)
                .unwrap_or_else(|| panic!("no result for {profile}"))
                .cost_provenance
                .is_measured()
        );
    }
}

/// A bare `[adapter.claude]` is the ADE's own argv, and an id nobody knows
/// without a `command` is the person's mistake, said by name.
#[test]
fn a_bare_block_falls_back_to_the_known_argv() {
    let suite = Suite::from_toml(
        r#"
name = "cross"
[adapter.claude]
"#,
    )
    .expect("the suite parses");
    let specs = suite.adapter_specs().expect("claude is known");
    assert_eq!(specs.len(), 1);
    // The id is the block's key, and the command is `known_adapter`'s.
    assert_eq!(specs[0].id, "claude");
    assert!(specs[0].command.contains("--output-format json"));
    assert_eq!(specs[0].parse, ParseMode::Json);

    let unknown = Suite::from_toml(
        r#"
name = "cross"
[adapter.nothing-like-this]
"#,
    )
    .expect("the suite parses");
    let err = unknown.adapter_specs().expect_err("no argv is knowable").to_string();
    assert!(err.contains("nothing-like-this"), "{err}");
    assert!(err.contains("command"), "it says what to write: {err}");
}
