//! A path that is too long for Windows' classic `MAX_PATH` is still a path
//! inside the workspace.
//!
//! The defect this pins: an eval case workspace adds about 110 characters to
//! whatever root it is given, so a 143-character workspace puts
//! `<case>/Cargo.toml` past 260. `canonicalize` then hands back the
//! extended-length (`\?\`) form, `dunce` refuses to simplify it *because* it is
//! long, and the target no longer has the same shape as the compiled rule — so
//! a filesystem limit came out of the engine as "no rule allows read(...)".
//! A length is not a permission decision.

mod common;

use std::path::PathBuf;

use common::Workspace;
use orrery_policy::{PendingCall, PolicyBuilder, PolicyEngine, Verdict};
use orrery_proto::{Layer, Subject};

/// A directory under `root` whose absolute path is at least `want` characters.
fn deep_dir(root: &std::path::Path, want: usize) -> PathBuf {
    let mut dir = root.to_path_buf();
    while dir.display().to_string().len() < want {
        dir.push("a-deliberately-long-directory-segment");
    }
    std::fs::create_dir_all(&dir).expect("windows can make a long directory");
    dir
}

#[test]
fn a_file_past_max_path_is_still_inside_the_workspace() {
    let ws = Workspace::new();
    let deep = deep_dir(ws.root(), 250);
    let file = deep.join("Cargo.toml");
    std::fs::write(&file, "[package]\n").expect("and write into it");
    assert!(
        file.display().to_string().len() > 260,
        "the fixture has to be past MAX_PATH to be testing anything: {}",
        file.display()
    );

    let engine = PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(
                "[permissions]\nallow = [\"read(./**)\"]\n",
                "project.toml",
                Layer::Project,
                false,
            )
            .expect("parses")
            .build()
            .expect("compiles"),
    );

    let explained = engine.explain(
        &PendingCall::read(file.display().to_string()),
        &Subject::Agent,
    );
    assert_eq!(
        explained.verdict,
        Verdict::Allow,
        "`read(./**)` covers every file in the workspace, however long its name: {explained}"
    );
}
