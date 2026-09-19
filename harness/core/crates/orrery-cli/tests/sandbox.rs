//! The shared test helper does not read the machine it is running on.
//!
//! `common::orrery` set `COLUMNS` and nothing else, so every test that used it
//! — cli, eval, ledger, json, workflow, ext, session, replay, mcp, skills,
//! explain, exit_codes, serve, reachable — resolved its configuration against
//! the developer's real `~/.orrery` and the machine's real
//! `%ProgramData%\Orrery\managed.toml`. A managed deny on a CI box would have
//! changed what those tests assert, and nothing would have said so.
//!
//! This pins the fix from the other side: a file written into the *sandbox*
//! home is the file the binary reads.

mod common;

use common::{args, jsonl, orrery, quiet, sandbox_home};

#[test]
fn the_shared_helper_answers_from_its_own_home() {
    let home = sandbox_home();
    std::fs::create_dir_all(home.join(".orrery")).expect("the user directory");
    std::fs::write(
        home.join(".orrery/config.toml"),
        "[permissions]\nallow = [\"read(./**)\"]\n",
    )
    .expect("the user config");

    #[cfg(windows)]
    {
        let managed = home.join("ProgramData").join("Orrery");
        std::fs::create_dir_all(&managed).expect("the managed directory");
        std::fs::write(
            managed.join("managed.toml"),
            "[permissions]\ndeny = [\"net(domain: *)\"]\n",
        )
        .expect("the managed config");
    }

    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery(&args(
        &quiet(ws.path()),
        &["--json", "permissions", "explain", "read(./src/main.rs)"],
    ));
    assert!(
        out.status.success(),
        "explaining is not a failure: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = &jsonl(&out.stdout)[0];
    assert_eq!(v["verdict"], "allow", "the sandbox home's rule is in force");
    assert_eq!(v["rule"]["layer"], "user");

    #[cfg(windows)]
    {
        let out = orrery(&args(
            &quiet(ws.path()),
            &["--json", "permissions", "explain", "net(domain: evil.example)"],
        ));
        assert!(out.status.success());
        let v = &jsonl(&out.stdout)[0];
        assert_eq!(v["verdict"], "deny");
        assert_eq!(
            v["rule"]["layer"], "managed",
            "%ProgramData% points inside the sandbox too"
        );
    }
}
