//! The phase-4 acceptance criterion, run.
//!
//! §8: "Three ported extensions render with no drawing code of their own."
//! This file proves the two halves that are not about pixels — that they are
//! real extensions, and that they draw nothing — and writes the fixture the
//! three renderers snapshot.

use std::path::Path;

use orrery_ported::{examples, fixture_text, fixtures_dir, run, run_all, workspace_root};
use orrery_proto::{LoadOutcome, SurfaceKind};

/// Three. Not two with a third promised.
#[test]
fn there_are_three() {
    assert_eq!(examples().len(), 3);
}

/// Each one loads through `orrery-host`, from an `orrery.toml` on disk, and
/// appears in the ledger the way a session would show it.
#[tokio::test]
async fn they_load_through_the_host() {
    for example in examples() {
        let manifest = workspace_root().join(example.manifest);
        assert!(
            manifest.is_file(),
            "{}: no manifest at {}",
            example.name,
            manifest.display()
        );
        let ported = run(&example).await;
        match &ported.load {
            LoadOutcome::Ok {
                ext, contributions, ..
            } => {
                assert_eq!(ext.as_str(), example.ext);
                assert!(
                    !contributions.is_empty(),
                    "{}: it loaded and contributed nothing",
                    example.name
                );
            }
            other => panic!("{}: {other:?}", example.name),
        }
        assert!(
            !ported.frames.is_empty(),
            "{}: nothing crossed the wire",
            example.name
        );
    }
}

/// An extension asked for no capabilities and was given none. Drawing a table
/// is not a privilege.
#[tokio::test]
async fn they_run_under_no_grant() {
    for example in examples() {
        let manifest = std::fs::read_to_string(workspace_root().join(example.manifest))
            .expect("the manifest reads");
        let parsed: toml::Value = manifest.parse().expect("the manifest is toml");
        let requires = parsed.get("requires").and_then(|r| r.as_table());
        assert!(
            requires.is_none_or(toml::map::Map::is_empty),
            "{} asks for {requires:?}: a ported extension that needs a grant to \
             describe something is drawing",
            example.name
        );
    }
}

/// The half §8 actually cares about: no drawing code.
///
/// A source scan, over the ported extensions' own files. If one of them
/// imported a terminal crate, reached for stdout, or took a width, this is
/// where the port stopped being a port.
#[test]
fn draws_nothing() {
    for example in examples() {
        let source = std::fs::read_to_string(workspace_root().join(example.source))
            .expect("the source reads");
        let body = strip_docs(&source);
        for forbidden in [
            "std::io",
            "Stdout",
            "Stderr",
            "print!",
            "eprint!",
            "io::Write",
            "crossterm",
            "ratatui",
            "termcolor",
            "ansi",
            "\\x1b",
        ] {
            assert!(
                !body.contains(forbidden),
                "{}: `{forbidden}` in a ported extension",
                example.name
            );
        }
        // And nothing that knows how wide a client is.
        for width in ["width", "columns()", "terminal_size"] {
            assert!(
                !body.contains(width),
                "{}: `{width}` — a surface that knew the width would be laying \
                 itself out, which is the client's job",
                example.name
            );
        }
    }
}

/// Their manifests name only published crates. A ported extension a third party
/// could not build is not a port; it is an internal.
#[test]
fn they_depend_only_on_what_a_third_party_can_depend_on() {
    let published = ["orrery-ext-api", "orrery-proto"];
    for example in examples() {
        let dir = workspace_root().join(example.source);
        let manifest_path = dir
            .parent()
            .and_then(Path::parent)
            .expect("src/ has a parent")
            .join("Cargo.toml");
        let manifest: toml::Value = std::fs::read_to_string(&manifest_path)
            .expect("Cargo.toml reads")
            .parse()
            .expect("Cargo.toml is toml");
        let deps = manifest
            .get("dependencies")
            .and_then(|d| d.as_table())
            .expect("it has dependencies");
        for name in deps.keys() {
            if !name.starts_with("orrery-") {
                continue;
            }
            assert!(
                published.contains(&name.as_str()),
                "{} depends on `{name}`, which is not one of the published \
                 crates {published:?}",
                example.name
            );
        }
    }
}

/// The surface an extension returns is the one it described last: composing a
/// surface through `ctx.ui` describes its children too, and the whole is the
/// last thing built.
#[tokio::test]
async fn what_it_returns_is_what_it_described() {
    for ported in run_all().await {
        let last = ported.described.last().expect("something was described");
        let (_, returned) = ported.returned.last().expect("something was returned");
        assert_eq!(
            &last.kind, &returned.kind,
            "{}: the sink and the outcome disagree",
            ported.name
        );
    }
}

/// §6.2, with teeth: the one custom surface here has a fallback that names
/// every stage its payload names, and says where the release has got to.
#[tokio::test]
async fn the_fallback_is_informative() {
    let example = examples()
        .into_iter()
        .find(|e| e.name == "ported-release-train")
        .expect("the release train is one of the three");
    let ported = run(&example).await;
    let (_, surface) = ported.returned.first().expect("it returned a surface");
    let SurfaceKind::Stack { children, .. } = &surface.kind else {
        panic!("expected a stack");
    };
    let SurfaceKind::Custom {
        kind,
        payload,
        fallback,
    } = &children[2].kind
    else {
        panic!("expected a custom surface");
    };
    assert_eq!(kind, "example-release-train.timeline");

    let text = orrery_client_json::render_text(fallback);
    for stage in payload["stages"].as_array().expect("stages") {
        let label = stage["label"].as_str().expect("a label");
        assert!(
            text.contains(label),
            "the fallback drops `{label}`, which the payload carries: {text}"
        );
    }
    assert!(text.len() >= 16, "a fallback that short is a shrug: {text}");
    for lazy in ["open the", "see the", "not supported", "unsupported"] {
        assert!(
            !text.to_lowercase().contains(lazy),
            "a fallback saying `{lazy}` is the one 6.2 is about: {text}"
        );
    }
}

/// Re-emission crosses the wire as a patch, not a second copy.
#[tokio::test]
async fn a_second_emission_is_a_patch() {
    let example = examples()
        .into_iter()
        .find(|e| e.name == "ported-release-train")
        .expect("the release train is one of the three");
    let ported = run(&example).await;
    assert_eq!(ported.returned.len(), 2, "two calls");
    assert_eq!(
        ported.returned[0].0, ported.returned[1].0,
        "under the same extension-minted id"
    );
    let replaces = ported
        .frames
        .iter()
        .filter(|f| format!("{:?}", f.event).contains("path: \"/surfaces/"))
        .count();
    assert!(
        replaces >= 1,
        "the second emission produced no state delta at all: {:?}",
        ported.frames
    );
    assert!(
        ported.frames.len() < 12,
        "a re-emission that costs this many frames is a differ that gave up: {}",
        ported.frames.len()
    );
}

/// The fixtures the Ink suite reads are written from the extensions, not by
/// hand. Set `ORRERY_BLESS=1` to rewrite them.
#[tokio::test]
async fn fixtures_are_what_the_extensions_produce() {
    let bless = std::env::var("ORRERY_BLESS").is_ok();
    std::fs::create_dir_all(fixtures_dir()).expect("the directory is there");
    for (example, ported) in examples().into_iter().zip(run_all().await) {
        let header = format!(
            "{} - generated by `cargo test -p orrery-ported`, from the extension\n\
             at {}. Do not edit: run with ORRERY_BLESS=1 instead.\n\
             Phase 4 (plan 09, section 8): the extension emits surfaces and draws none of them.",
            ported.name, example.source
        );
        let text = fixture_text(&ported, &header);
        let path = fixtures_dir().join(format!("{}.jsonl", ported.name));
        if bless {
            std::fs::write(&path, &text).expect("the fixture writes");
            continue;
        }
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} — run with ORRERY_BLESS=1", path.display()));
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            text,
            "{} has drifted from what the extension produces",
            path.display()
        );
    }
}

/// Everything above the first blank line that is not a doc comment: the scan
/// must not trip over a comment that names what the file refuses to do.
fn strip_docs(source: &str) -> String {
    source
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("#!")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
