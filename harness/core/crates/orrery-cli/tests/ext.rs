//! `orrery ext` — the command plan 06 task 8 was waiting for.
//!
//! Both subcommands here run with **no model, no network and no database**:
//! they go through `orrery_ext_api::testing`, which is the same mock broker an
//! extension author writes their own tests against.

mod common;

use common::orrery;

/// `orrery ext test` passes on a fixture extension with no provider named at
/// all — no `--provider`, no key, nothing to reach (plan 06 task 8).
#[test]
fn test_runs_without_a_model() {
    let dir = tempfile::tempdir().expect("a temporary extension");
    std::fs::write(
        dir.path().join("orrery.toml"),
        "\
api = \"orrery-ext/1\"
runtime = \"native\"

[extension]
id = \"fixture-ext\"
version = \"0.1.0\"

[provides]
tools = [\"hello\"]

[requires]
read = [\"$WORKSPACE/**\"]
",
    )
    .expect("the manifest is writable");

    let out = orrery(&[
        "ext".to_owned(),
        "test".to_owned(),
        dir.path().display().to_string(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr was: {stderr}");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fixture-ext"), "it names it: {stdout}");
    assert!(
        stdout.contains("hello"),
        "…and what it contributes: {stdout}"
    );
    assert!(
        stdout.contains("no model, no network"),
        "…and says what it did not need: {stdout}"
    );
}

/// A manifest that will not parse is a usage error naming the file, not a panic.
#[test]
fn a_broken_manifest_is_usage() {
    let dir = tempfile::tempdir().expect("a temporary extension");
    std::fs::write(dir.path().join("orrery.toml"), "this is not toml = = =")
        .expect("the manifest is writable");
    let out = orrery(&[
        "ext".to_owned(),
        "test".to_owned(),
        dir.path().display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}

/// `orrery ext list` reports the first-party set this build has, with what each
/// one contributes.
#[test]
fn list_shows_the_ledger() {
    let out = orrery(&["ext".to_owned(), "list".to_owned()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("builtin"), "the builtin bundle: {stdout}");
    for tool in ["read", "write", "edit", "bash", "grep", "glob"] {
        assert!(stdout.contains(tool), "`{tool}` is missing: {stdout}");
    }
    assert!(stdout.contains("ok"), "and it says how it loaded: {stdout}");
}

/// Round 5, item 4: `ext test <name>` works on a name, not only a path.
///
/// `orrery ext test builtin` used to fail with `could not read builtin\orrery.toml`,
/// because the argument was only ever treated as a directory. A compiled-in
/// bundle has no `orrery.toml` on disk to point at, so a name has to be a way in.
#[test]
fn test_takes_the_name_of_a_compiled_in_bundle() {
    let out = orrery(&["ext".to_owned(), "test".to_owned(), "builtin".to_owned()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "stderr was: {stderr}");
    assert!(stdout.contains("builtin"), "it names it: {stdout}");
    assert!(
        stdout.contains("bash"),
        "…and what it contributes: {stdout}"
    );
    assert!(
        stdout.contains("no model, no network"),
        "…and says what it did not need: {stdout}"
    );
}

/// A name nothing answers to is a usage error that says where it looked.
#[test]
fn an_unknown_name_says_where_it_looked() {
    let out = orrery(&[
        "ext".to_owned(),
        "test".to_owned(),
        "no-such-extension".to_owned(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no-such-extension") && err.contains("compiled in"),
        "it says what it looked for and where: {err}"
    );
    assert!(!err.contains("panicked"));
}

/// Round 5, item 4: `ext list` reports the **installed** set, not only the
/// compiled-in one. Installing an extension and then not seeing it is the
/// clearest possible way for the command to be useless.
#[test]
fn list_shows_an_installed_extension() {
    let home = tempfile::tempdir().expect("a sandboxed home");
    let work = tempfile::tempdir().expect("a workspace");

    // A package on disk, installed into the user layer, which is a discovery
    // root whether or not the workspace is trusted.
    let pkg = work.path().join("buildgraph");
    std::fs::create_dir_all(&pkg).expect("the package directory");
    std::fs::write(
        pkg.join("orrery.toml"),
        "api = \"orrery-ext/1\"\nruntime = \"native\"\n\n\
         [extension]\nid = \"buildgraph\"\nversion = \"1.2.0\"\n\n\
         [provides]\ntools = [\"graph\"]\n\n[requires]\nread = [\"./**\"]\n",
    )
    .expect("the manifest");

    let run = |argv: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
            .args(argv)
            .current_dir(work.path())
            .env("COLUMNS", "100")
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("ProgramData", home.path().join("ProgramData"))
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the orrery binary runs")
    };

    let installed = run(&[
        "--workspace",
        &work.path().display().to_string(),
        "install",
        "./buildgraph",
        "--yes",
    ]);
    assert!(
        installed.status.success(),
        "install: {}",
        String::from_utf8_lossy(&installed.stderr)
    );

    let out = run(&[
        "--workspace",
        &work.path().display().to_string(),
        "ext",
        "list",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stdout.contains("buildgraph"),
        "the installed extension is listed: {stdout}"
    );
    assert!(
        stdout.contains("graph"),
        "…with what it contributes: {stdout}"
    );
    assert!(
        stdout.contains("builtin"),
        "…and the compiled-in set is still there: {stdout}"
    );
    assert!(
        stdout.contains("user"),
        "…and which layer it came from: {stdout}"
    );
}

/// Round 5, item 1: the section 6.7 floor and the section 4.6 roles ship in the
/// default build.
///
/// Without `views-default` a client has nothing bound and can legitimately draw
/// a blank screen; without `agents-default` the five named roles are names
/// nothing answers to. Both were compiled nowhere near the binary — not even
/// with `--all-features` — so this asserts the shipped default set through the
/// binary, which is the only place the claim means anything.
#[test]
fn the_default_build_ships_the_floor() {
    let out = orrery(&["ext".to_owned(), "list".to_owned()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for ext in ["builtin", "views-default", "agents-default"] {
        assert!(stdout.contains(ext), "`{ext}` is missing: {stdout}");
    }
}

/// And each one loads through the same door a third-party bundle goes through.
#[test]
fn the_floor_loads_like_any_other_bundle() {
    for ext in ["views-default", "agents-default"] {
        let out = orrery(&["ext".to_owned(), "test".to_owned(), ext.to_owned()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "`{ext}`: {stderr}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains(ext), "{stdout}");
        assert!(stdout.contains("no model, no network"), "{stdout}");
    }
}
