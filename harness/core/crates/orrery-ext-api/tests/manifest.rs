//! The manifest is the contract. Everything here is a rule a third-party
//! extension will be held to, so every one of them is asserted rather than
//! documented.

use orrery_ext_api::manifest::{ExtensionManifest, ManifestError, Requirement, RuntimeKind};
use orrery_proto::{Aspect, ContributionKind, LoadStage};

/// The manifest as the plan writes it: everything under `[extension]`.
const CANONICAL: &str = r#"
[extension]
api     = "orrery-ext/1"
name    = "buildgraph"
version = "1.2.0"
runtime = "native"

[provides]
tools = ["impacted", "deps"]

[requires]
read  = ["$WORKSPACE/**"]
spawn = ["java"]
net   = false
"#;

#[test]
fn unknown_major_is_refused() {
    let src = CANONICAL.replace("orrery-ext/1", "orrery-ext/2");
    let err = ExtensionManifest::from_toml_str(&src, "buildgraph/orrery.toml")
        .expect_err("an api major this build does not know must not load");

    assert!(
        matches!(err, ManifestError::UnknownApi { found: 2, .. }),
        "expected UnknownApi, got {err:?}"
    );
    assert_eq!(
        err.stage(),
        LoadStage::Manifest,
        "a refused manifest fails at the manifest stage, not later"
    );
    assert!(
        err.to_string().contains("buildgraph/orrery.toml"),
        "the error names the file: {err}"
    );
}

#[test]
fn a_manifest_that_is_not_toml_names_the_file() {
    let err = ExtensionManifest::from_toml_str("{{{", "broken/orrery.toml").unwrap_err();
    assert_eq!(err.stage(), LoadStage::Manifest);
    assert!(err.to_string().contains("broken/orrery.toml"));
}

#[test]
fn provides_round_trips() {
    let src = r#"
[extension]
api     = "orrery-ext/1"
name    = "everything"
version = "0.1.0"
runtime = "native"

[provides]
tools        = ["a"]
providers    = ["b"]
agents       = ["c"]
workflows    = ["d"]
interceptors = ["e"]
lifecycle    = ["f"]
graders      = ["g"]
commands     = ["h"]
views        = ["i"]
renderers    = ["j"]
skills       = ["k"]
mcp          = ["l"]
memory      = "m"
session     = "n"
permissions = "o"
router      = "p"
"#;
    let m = ExtensionManifest::from_toml_str(src, "everything/orrery.toml").unwrap();

    assert_eq!(m.provides.tools, ["a"]);
    assert_eq!(m.provides.providers, ["b"]);
    assert_eq!(m.provides.agents, ["c"]);
    assert_eq!(m.provides.workflows, ["d"]);
    assert_eq!(m.provides.interceptors, ["e"]);
    assert_eq!(m.provides.lifecycle, ["f"]);
    assert_eq!(m.provides.graders, ["g"]);
    assert_eq!(m.provides.commands, ["h"]);
    assert_eq!(m.provides.views, ["i"]);
    assert_eq!(m.provides.renderers, ["j"]);
    assert_eq!(m.provides.skills, ["k"]);
    assert_eq!(m.provides.mcp, ["l"]);
    assert_eq!(m.provides.memory.as_deref(), Some("m"));
    assert_eq!(m.provides.session.as_deref(), Some("n"));
    assert_eq!(m.provides.permissions.as_deref(), Some("o"));
    assert_eq!(m.provides.router.as_deref(), Some("p"));

    // The field list and the contribution kinds are generated from one macro
    // invocation, so they cannot drift. Every field that has a kind today
    // appears once, named.
    let kinds: Vec<(ContributionKind, &str)> = m
        .provides
        .contributions()
        .iter()
        .map(|c| (c.kind, c.name.clone()))
        .map(|(k, n)| (k, Box::leak(n.into_boxed_str()) as &str))
        .collect();
    assert!(kinds.contains(&(ContributionKind::Tool, "a")));
    assert!(kinds.contains(&(ContributionKind::Provider, "b")));
    assert!(kinds.contains(&(ContributionKind::Grader, "g")));
    assert!(kinds.contains(&(ContributionKind::Command, "h")));
    assert!(kinds.contains(&(ContributionKind::View, "i")));
    assert!(kinds.contains(&(ContributionKind::Renderer, "j")));
    assert!(kinds.contains(&(ContributionKind::Skill, "k")));
    assert!(kinds.contains(&(ContributionKind::Memory, "m")));
    assert!(kinds.contains(&(ContributionKind::SessionStore, "n")));
    assert!(kinds.contains(&(ContributionKind::Router, "p")));
}

#[test]
fn an_unknown_provides_field_is_an_error_not_a_shrug() {
    let src = CANONICAL.replace("tools = [\"impacted\", \"deps\"]", "toolz = [\"impacted\"]");
    let err = ExtensionManifest::from_toml_str(&src, "typo/orrery.toml").unwrap_err();
    assert_eq!(err.stage(), LoadStage::Manifest);
    assert!(err.to_string().contains("toolz"), "{err}");
}

#[test]
fn requires_becomes_capabilities() {
    let m = ExtensionManifest::from_toml_str(CANONICAL, "buildgraph/orrery.toml").unwrap();
    let caps = m.capabilities();

    let read = caps
        .iter()
        .find(|c| c.aspect == Aspect::Read)
        .expect("read is required");
    assert_eq!(read.scope, ["$WORKSPACE/**"]);

    let spawn = caps
        .iter()
        .find(|c| c.aspect == Aspect::Spawn)
        .expect("spawn is required");
    assert_eq!(spawn.scope, ["java"]);

    assert!(
        !caps.iter().any(|c| c.aspect == Aspect::Net),
        "`net = false` asks for nothing, it does not ask for everything"
    );

    // And the other direction: a bare `true` is the unqualified capability.
    let src = CANONICAL.replace("net   = false", "net   = true");
    let m = ExtensionManifest::from_toml_str(&src, "buildgraph/orrery.toml").unwrap();
    let net = m
        .capabilities()
        .into_iter()
        .find(|c| c.aspect == Aspect::Net)
        .expect("`net = true` is the whole aspect");
    assert!(net.scope.is_empty(), "empty scope means unqualified");
    assert_eq!(
        m.requires.get(&Aspect::Net),
        Some(&Requirement::Unqualified(true))
    );
}

#[test]
fn process_required_for_process_runtime() {
    let src = CANONICAL.replace(r#"runtime = "native""#, r#"runtime = "process""#);
    let err = ExtensionManifest::from_toml_str(&src, "buildgraph/orrery.toml").unwrap_err();

    assert!(matches!(err, ManifestError::ProcessSpecMissing { .. }));
    assert_eq!(err.stage(), LoadStage::Manifest);
    assert!(
        err.to_string().contains("buildgraph/orrery.toml"),
        "the error names the file so a person can go and fix it: {err}"
    );

    let with_spec = format!(
        "{src}\n[process]\ncommand = \"./bin/buildgraph-server\"\nprotocol = \"orrery-ext/1\"\n"
    );
    let m = ExtensionManifest::from_toml_str(&with_spec, "buildgraph/orrery.toml").unwrap();
    assert_eq!(m.runtime, RuntimeKind::Process);
    assert_eq!(m.process.unwrap().command, "./bin/buildgraph-server");
}

/// The eleven first-party bundles were scaffolded before this parser existed,
/// with `api`/`runtime` at the top level and `[extension] id`. One parser reads
/// both spellings; see `ExtensionManifest`'s docs.
#[test]
fn the_scaffolded_first_party_spelling_parses_too() {
    let src = r#"
api = "orrery-ext/1"
runtime = "native"

[extension]
id = "builtin"
version = "0.0.0"

[provides]
tools = ["read", "write"]

[requires]
read = ["$WORKSPACE/**"]
"#;
    let m = ExtensionManifest::from_toml_str(src, "builtin/orrery.toml").unwrap();
    assert_eq!(m.name.as_str(), "builtin");
    assert_eq!(m.runtime, RuntimeKind::Native);
    assert_eq!(m.api.major, 1);
}

#[test]
fn every_shipped_first_party_manifest_parses() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extensions/crates")
        .canonicalize()
        .expect("the extensions tree is where the workspace says it is");

    let mut seen = 0;
    for entry in std::fs::read_dir(&root).unwrap() {
        let manifest = entry.unwrap().path().join("orrery.toml");
        if !manifest.exists() {
            continue;
        }
        ExtensionManifest::from_path(&manifest)
            .unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
        seen += 1;
    }
    assert!(seen >= 5, "expected the first-party bundles, found {seen}");
}
