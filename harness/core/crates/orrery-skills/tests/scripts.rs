//! A skill's `scripts/`, and the grant they run under.
//!
//! # Read `run_under_the_declared_grant` against the counterfactual
//!
//! The script in `tests/fixtures/script/main.rs` asks to write two files: one
//! inside the directory its grant names, one outside it. In Claude Code, Codex
//! or Cursor that script is handed to the agent's shell tool and **both writes
//! land**, because a skill's bundled scripts run with the user's own
//! privileges — a skill is a Markdown file with a folder beside it, so it is
//! published like documentation, reviewed like documentation, and trusted like
//! documentation.
//!
//! Here the second one is refused by name, and the refusal is in the audit
//! stream with the rule that produced it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_audit::{AuditEvent, Verdict};
use orrery_broker::LocalBroker;
use orrery_policy::{PolicyBuilder, PolicyEngine};
use orrery_proto::{AgentScope, BranchId, Consent, Grant, GrantSpec, Layer};
use orrery_skills::{EffectOutcome, ScriptRunner, SkillError, SkillRef, SkillSettings};
use orrery_tools::ToolBudget;

/// The fixture script, built by cargo as a bin target of this crate.
///
/// A committed Rust program rather than a shell script: identical on every
/// platform, and incapable of reaching the network.
const FIXTURE: &str = env!("CARGO_BIN_EXE_orrery-skill-fixture");

struct Skill {
    _temp: tempfile::TempDir,
    root: PathBuf,
    skill: SkillRef,
}

/// A skill on disk with the fixture binary installed as its one script.
fn skill_with_grant(grant: Option<GrantSpec>) -> Skill {
    let temp = tempfile::tempdir().expect("a temp dir");
    let root = dunce::canonicalize(temp.path()).unwrap_or_else(|_| temp.path().to_path_buf());

    let dir = root.join("skills/release-notes");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: release-notes\ndescription: writes the notes\n---\n\n# Release notes\n",
    )
    .unwrap();
    let script = dir.join("scripts").join(script_name());
    std::fs::copy(FIXTURE, &script).expect("the fixture binary is built before the test runs");

    let doc = orrery_skills::parse::skill_file(dir.join("SKILL.md")).unwrap();
    let settings = SkillSettings {
        grant,
        ..SkillSettings::default()
    };
    let skill = SkillRef::new(&doc, Layer::Project, &settings);

    Skill {
        _temp: temp,
        root,
        skill,
    }
}

fn script_name() -> &'static str {
    if cfg!(windows) { "run.exe" } else { "run" }
}

/// Rules for the skill's subject: it may spawn anything under the skill
/// directory, and may write **only** under `<root>/out`.
fn rules(root: &Path) -> String {
    format!(
        r#"
[permissions]
allow = ["spawn(*)", "write({root}/**)"]

[permissions."agent:skill:release-notes"]
allow = ["spawn(*)", "write({root}/out/**)"]
"#,
        root = root.display().to_string().replace('\\', "/"),
    )
}

struct Harness {
    runner: ScriptRunner,
    audit: Arc<orrery_audit::MemorySink>,
}

fn harness(root: &Path) -> Harness {
    let resolved = PolicyBuilder::new(root)
        .layer_toml(&rules(root), "test.toml", Layer::Project, false)
        .expect("the rules parse")
        .build()
        .expect("the rules compile");

    let audit = orrery_audit::memory();
    let engine = Arc::new(PolicyEngine::new(resolved).with_audit(audit.clone()));
    let broker = Arc::new(LocalBroker::new(engine.ledger().clone()).with_audit(audit.clone()));
    Harness {
        runner: ScriptRunner::new(engine, broker).with_audit(audit.clone()),
        audit,
    }
}

/// The agent that loaded the skill. Its own grant is wide; the skill's narrows
/// it, and narrowing is the only direction available.
fn caller() -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant {
            capabilities: Vec::new(),
            consent: Consent::Always,
        },
    }
}

fn budget() -> ToolBudget {
    ToolBudget::new(15_000, 64 * 1024)
}

#[tokio::test]
async fn run_under_the_declared_grant() {
    let fixture = skill_with_grant(Some(GrantSpec::default()));
    let h = harness(&fixture.root);

    let inside = fixture.root.join("out/notes.txt");
    let outside = fixture.root.join("elsewhere/notes.txt");

    let run = h
        .runner
        .run(
            &fixture.skill,
            script_name(),
            &[
                "write-two".to_owned(),
                inside.display().to_string(),
                outside.display().to_string(),
            ],
            &caller(),
            budget(),
        )
        .await
        .expect("the script was allowed to start");

    assert_eq!(run.effects.len(), 2, "{run:#?}");
    assert_eq!(run.effects[0].outcome, EffectOutcome::Applied, "{run:#?}");
    assert!(
        run.effects[1].outcome.is_denied(),
        "the write outside the grant must be refused: {run:#?}"
    );

    // The write inside happened; the one outside did not. Not "was logged and
    // happened anyway".
    assert_eq!(std::fs::read_to_string(&inside).unwrap(), "inside");
    assert!(!outside.exists(), "the denied write must not have landed");

    // And the refusal is in the audit stream, naming the subject and carrying a
    // deny verdict.
    let denied = h.audit.records().into_iter().any(|r| {
        matches!(
            &r.event,
            AuditEvent::CapabilityDecision { subject, verdict: Verdict::Deny, request, .. }
                if subject.to_string() == "agent:skill:release-notes"
                    && request.starts_with("write(")
        )
    });
    assert!(denied, "the denial must be audited");
}

#[tokio::test]
async fn no_grant_means_no_scripts() {
    let fixture = skill_with_grant(None);
    let h = harness(&fixture.root);

    let err = h
        .runner
        .run(&fixture.skill, script_name(), &[], &caller(), budget())
        .await
        .expect_err("a skill with no grant cannot run scripts at all");
    assert!(matches!(err, SkillError::NoGrant { .. }), "{err}");
    assert!(err.to_string().contains("release-notes"));
}

#[tokio::test]
async fn budgeted() {
    let fixture = skill_with_grant(Some(GrantSpec::default()));
    let h = harness(&fixture.root);

    let run = h
        .runner
        .run(
            &fixture.skill,
            script_name(),
            &["spin".to_owned()],
            &caller(),
            // A script that never returns, and a ceiling that says otherwise.
            ToolBudget::new(300, 64 * 1024),
        )
        .await
        .expect("it started");

    assert!(
        run.timed_out,
        "the wall-clock budget must stop it: {run:#?}"
    );
    assert!(run.effects.is_empty());
}

#[tokio::test]
async fn a_script_that_is_not_there_is_not_a_spawn() {
    let fixture = skill_with_grant(Some(GrantSpec::default()));
    let h = harness(&fixture.root);

    let err = h
        .runner
        .run(&fixture.skill, "absent.sh", &[], &caller(), budget())
        .await
        .expect_err("no such script");
    assert!(matches!(err, SkillError::NoSuchScript { .. }), "{err}");
}
