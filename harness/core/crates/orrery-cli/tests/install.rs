//! `orrery install` and `orrery remove`, as bare top-level verbs.
//!
//! Plan 17 task 7, amended by plan 15's Architecture section. Every test runs
//! with a **sandboxed home**, so nothing here can reach the developer's own
//! `~/.orrery`, and nothing here touches the network: the only sources
//! exercised from the binary are a local path and a bare registry name with no
//! index configured.

mod common;

use std::path::Path;

use common::{args, quiet};

/// The binary, with a **sandboxed home** and a working directory.
///
/// `orrery install ./x` resolves the path against the process's working
/// directory, the way a person typing it expects — so a test that installs a
/// relative path has to run from somewhere, and says where.
fn run(home: &Path, cwd: &Path, argv: &[String]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(argv)
        .current_dir(cwd)
        .env("COLUMNS", "100")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs")
}

/// A package on disk: a manifest and a file.
fn package(root: &Path, id: &str, version: &str, requires: &str) -> std::path::PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("orrery.toml"),
        format!(
            "api = \"orrery-ext/1\"\nruntime = \"native\"\n\n[extension]\nid = \"{id}\"\nversion = \"{version}\"\n\n[provides]\ntools = [\"build\"]\n\n[requires]\n{requires}\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), "// an extension\n").unwrap();
    dir
}

#[test]
fn a_local_path_installs_into_the_user_layer() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pkg = package(work.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");

    let out = run(
        home.path(),
        work.path(),
        &args(
            &quiet(work.path()),
            &[
                "install",
                &format!("./{}", relative(&pkg, work.path())),
                "--yes",
            ],
        ),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");

    let landed = home.path().join(".orrery/extensions/buildgraph");
    assert!(landed.join("orrery.toml").exists(), "{stdout}");
    assert!(stdout.contains("user layer"), "{stdout}");

    // A local path is not the registry, so it is visibly unpinned.
    assert!(stdout.contains("UNPINNED"), "{stdout}");

    // And `remove` takes it away again.
    let out = run(
        home.path(),
        work.path(),
        &args(&quiet(work.path()), &["remove", "buildgraph"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!landed.exists());
}

#[test]
fn the_workspace_layer_is_a_flag() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pkg = package(work.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");

    let out = run(
        home.path(),
        work.path(),
        &args(
            &quiet(work.path()),
            &[
                "install",
                &format!("./{}", relative(&pkg, work.path())),
                "--to",
                "workspace",
                "--yes",
            ],
        ),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        work.path()
            .join(".orrery/extensions/buildgraph/orrery.toml")
            .exists()
    );
    assert!(!home.path().join(".orrery/extensions/buildgraph").exists());
}

#[test]
fn remove_across_two_layers_asks_rather_than_guessing() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pkg = package(work.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let source = format!("./{}", relative(&pkg, work.path()));

    for layer in ["user", "workspace"] {
        let out = run(
            home.path(),
            work.path(),
            &args(
                &quiet(work.path()),
                &["install", &source, "--to", layer, "--yes"],
            ),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let out = run(
        home.path(),
        work.path(),
        &args(&quiet(work.path()), &["remove", "buildgraph"]),
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "an ambiguous remove is a usage error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("user"), "{stderr}");
    assert!(stderr.contains("workspace"), "{stderr}");
    assert!(
        out.stdout.is_empty(),
        "stdout is data, and nothing was removed"
    );

    // Naming one removes exactly that one.
    let out = run(
        home.path(),
        work.path(),
        &args(
            &quiet(work.path()),
            &["remove", "buildgraph", "--from", "workspace"],
        ),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(home.path().join(".orrery/extensions/buildgraph").exists());
}

#[test]
fn a_bare_name_names_the_index_and_never_another_host() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let out = run(
        home.path(),
        work.path(),
        &args(&quiet(work.path()), &["install", "buildgraph"]),
    );
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("index"), "{stderr}");
    assert!(
        !stderr.contains("crates.io"),
        "a bare name must not fall through to another host: {stderr}"
    );
}

#[test]
fn a_denied_capability_is_denied_by_default_and_the_install_still_lands() {
    // The non-interactive rule: without `--yes`, nothing is granted. The
    // extension still installs — a denial degrades, it does not fail.
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pkg = package(work.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");

    let out = run(
        home.path(),
        work.path(),
        &args(
            &quiet(work.path()),
            &["install", &format!("./{}", relative(&pkg, work.path()))],
        ),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        home.path()
            .join(".orrery/extensions/buildgraph/orrery.toml")
            .exists()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--yes"), "{stderr}");
    assert!(
        stderr.contains("read"),
        "the lost capability is named: {stderr}"
    );
}

#[test]
fn install_and_remove_are_no_longer_under_ext() {
    // They moved to the top level; `ext` keeps the things that are not
    // everyday actions.
    let work = tempfile::tempdir().unwrap();
    for sub in ["install", "remove"] {
        let out = run(
            work.path(),
            work.path(),
            &args(&quiet(work.path()), &["ext", sub, "buildgraph"]),
        );
        assert_eq!(
            out.status.code(),
            Some(2),
            "`orrery ext {sub}` should no longer parse"
        );
    }
    // And `ext list` and `ext test` still do.
    let out = run(
        work.path(),
        work.path(),
        &args(&quiet(work.path()), &["ext", "--help"]),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("list"), "{stdout}");
    assert!(stdout.contains("test"), "{stdout}");
    assert!(!stdout.contains("install"), "{stdout}");
}

/// A path relative to the workspace, forward-slashed, so `./x` is what the
/// binary is handed.
fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}
