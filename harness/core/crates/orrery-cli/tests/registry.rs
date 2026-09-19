//! `orrery registry` — the half of phase 8 that did not ship.
//!
//! Section 8 phase 8 is "an admin pins a version set and unpinned extensions
//! refuse to load". The refusing half was real and driven. The pinning half was
//! not: `orrery-cli/src/cmd/install.rs` states that fetching a remote index is
//! not implemented, and no command in the binary could **create, sign or
//! publish** one. An admin could only pin a version set if somebody handed them
//! a file made by a tool that does not exist here.
//!
//! This drives the whole admin workflow through the built binary and then
//! installs from what it produced. It is offline from end to end: a local
//! mirror directory stands in for crates.io, and no test here names a provider.

mod common;

use std::path::Path;

/// The binary, with a **sandboxed home** and a working directory.
fn orrery(home: &Path, work: &Path, argv: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(argv)
        .current_dir(work)
        .env("COLUMNS", "100")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs")
}

fn package(work: &Path, id: &str, version: &str) -> std::path::PathBuf {
    let dir = work.join(id);
    std::fs::create_dir_all(dir.join("src")).expect("the package directory");
    std::fs::write(
        dir.join("orrery.toml"),
        format!(
            "api = \"orrery-ext/1\"\nruntime = \"native\"\n\n\
             [extension]\nid = \"{id}\"\nversion = \"{version}\"\n\n\
             [provides]\ntools = [\"graph\"]\n\n[requires]\nread = [\"./**\"]\n"
        ),
    )
    .expect("the manifest");
    std::fs::write(dir.join("src/lib.rs"), "// an extension\n").expect("a file");
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination");
    for entry in std::fs::read_dir(from).expect("a readable tree") {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &dest);
        } else {
            std::fs::copy(&path, &dest).expect("a copied file");
        }
    }
}

fn say(out: &std::process::Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// PHASE 8, end to end: an admin **makes** a pin set, and then only what it
/// pins may be installed.
#[test]
fn an_admin_can_produce_the_pin_set_they_are_meant_to_pin() {
    let home = tempfile::tempdir().expect("a sandboxed home");
    let work = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(home.path().join("ProgramData")).expect("a sandboxed ProgramData");

    let state = work.path().join(".orrery");
    let state_flag = state.display().to_string();
    let index = work.path().join("registry/index.toml");
    let index_flag = index.display().to_string();
    let key = work.path().join("registry/signing-key.toml");
    let key_flag = key.display().to_string();

    // 1. A key and an empty index. Nothing on the network; the key is made here.
    let out = orrery(
        home.path(),
        work.path(),
        &[
            "--state-dir",
            &state_flag,
            "registry",
            "init",
            "--index",
            &index_flag,
            "--key",
            &key_flag,
            "--key-id",
            "managed",
        ],
    );
    assert!(out.status.success(), "registry init: {}", say(&out));
    let public = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert_eq!(public.len(), 64, "the public key, as hex: {public}");
    assert!(index.is_file(), "the index is written: {}", say(&out));
    assert!(key.is_file(), "and the signing key beside it");

    // 2. The package, in the mirror the installer fetches from.
    let pkg = package(work.path(), "buildgraph", "1.2.0");
    copy_tree(
        &pkg,
        &state.join("registry/mirror/orrery-ext-buildgraph"),
    );

    // 3. Pin it. The hash comes from staging the package exactly as an install
    //    will, not from a walk of its own.
    let out = orrery(
        home.path(),
        work.path(),
        &[
            "--state-dir",
            &state_flag,
            "registry",
            "add",
            "--index",
            &index_flag,
            "--id",
            "buildgraph",
            "--version",
            "1.2.0",
            "--source",
            "crates-io:orrery-ext-buildgraph",
        ],
    );
    assert!(out.status.success(), "registry add: {}", say(&out));
    let listed = String::from_utf8_lossy(&out.stdout);
    assert!(
        listed.contains("buildgraph") && listed.contains("1.2.0"),
        "it says what it pinned: {listed}"
    );
    assert!(
        listed.contains("read(./**)"),
        "…and what that version asks for, which is what an admin reviews: {listed}"
    );

    // 4. Sign the document.
    let out = orrery(
        home.path(),
        work.path(),
        &[
            "registry",
            "sign",
            "--index",
            &index_flag,
            "--key",
            &key_flag,
        ],
    );
    assert!(out.status.success(), "registry sign: {}", say(&out));
    assert!(
        index.with_extension("toml.sig").is_file(),
        "the detached signature is beside the index: {}",
        say(&out)
    );

    // 5. And check it back, which is the command an air-gapped admin runs on
    //    the machine that receives the index.
    let out = orrery(
        home.path(),
        work.path(),
        &[
            "registry",
            "verify",
            "--index",
            &index_flag,
            "--key",
            &public,
        ],
    );
    assert!(out.status.success(), "registry verify: {}", say(&out));
    let report = String::from_utf8_lossy(&out.stdout);
    assert!(
        report.contains("managed") && report.contains("buildgraph"),
        "it names the key that verified each pin: {report}"
    );

    // 6. The admin distributes the index and the key through the managed layer.
    let managed = home.path().join("ProgramData").join("Orrery");
    std::fs::create_dir_all(&managed).expect("the managed directory");
    std::fs::write(
        managed.join("managed.toml"),
        format!(
            "[registry]\nindex = \"{}\"\nkey = \"{public}\"\nunpinned = \"refuse\"\n",
            index.display().to_string().replace('\\', "\\\\")
        ),
    )
    .expect("managed.toml");

    // 7. Now the pin bites: a local path is not in the index and is refused.
    let out = orrery(
        home.path(),
        work.path(),
        &[
            "--state-dir",
            &state_flag,
            "install",
            "./buildgraph",
            "--yes",
        ],
    );
    assert_ne!(
        out.status.code(),
        Some(0),
        "an unpinned source is refused: {}",
        say(&out)
    );

    // 8. …and the pinned version installs, through the index the admin made.
    let out = orrery(
        home.path(),
        work.path(),
        &[
            "--state-dir",
            &state_flag,
            "install",
            "buildgraph@1.2.0",
            "--yes",
        ],
    );
    assert!(
        out.status.success(),
        "the pinned version installs from the index this binary produced: {}",
        say(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("pinned by key managed"),
        "and the supply-chain record names the key that pinned it, not `UNPINNED`: {stdout}"
    );
    assert!(
        home.path()
            .join(".orrery/extensions/buildgraph/orrery.toml")
            .is_file(),
        "the package landed: {stdout}"
    );
}

/// An index whose bytes changed after signing does not verify, and `verify`
/// says so rather than exiting 0 on a document nobody signed.
#[test]
fn verify_refuses_an_index_that_was_edited_after_signing() {
    let home = tempfile::tempdir().expect("a sandboxed home");
    let work = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(home.path().join("ProgramData")).expect("a sandboxed ProgramData");

    let index = work.path().join("registry/index.toml");
    let index_flag = index.display().to_string();
    let key = work.path().join("registry/signing-key.toml");
    let key_flag = key.display().to_string();

    let out = orrery(
        home.path(),
        work.path(),
        &[
            "registry",
            "init",
            "--index",
            &index_flag,
            "--key",
            &key_flag,
        ],
    );
    assert!(out.status.success(), "registry init: {}", say(&out));
    let public = String::from_utf8_lossy(&out.stdout).trim().to_owned();

    let out = orrery(
        home.path(),
        work.path(),
        &[
            "registry",
            "sign",
            "--index",
            &index_flag,
            "--key",
            &key_flag,
        ],
    );
    assert!(out.status.success(), "registry sign: {}", say(&out));

    let text = std::fs::read_to_string(&index).expect("the index");
    std::fs::write(&index, text.replace("expires = ", "expires  =")).ok();
    std::fs::write(&index, format!("{text}\n# somebody edited this\n")).expect("the edit");

    let out = orrery(
        home.path(),
        work.path(),
        &[
            "registry",
            "verify",
            "--index",
            &index_flag,
            "--key",
            &public,
        ],
    );
    assert_ne!(
        out.status.code(),
        Some(0),
        "an edited index is not a signed index: {}",
        say(&out)
    );
}
