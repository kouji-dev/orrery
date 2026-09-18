//! `SKILL.md` parsing, held against files that were published elsewhere.
//!
//! The fixtures under `tests/fixtures/wild/` are **verbatim copies** of skills
//! shipped by Anthropic, by the Superpowers plugin, by Google and by the
//! Playwright distribution; `tests/fixtures/wild/PROVENANCE.md` says where each
//! one came from. Nothing in them was edited to suit this parser. That is the
//! point: if one of them fails to parse we are extending the format rather than
//! adopting it, and the plan says that is a reason to think again.

use std::path::{Path, PathBuf};

use orrery_skills::SkillError;
use orrery_skills::parse::{self, SkillDoc};

fn wild(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wild")
        .join(file)
}

fn read(file: &str) -> (String, PathBuf) {
    let path = wild(file);
    let text = std::fs::read_to_string(&path).expect("a committed fixture");
    (text, path)
}

#[test]
fn real_skills_from_the_wild() {
    // file, expected name, a phrase the description must contain.
    let table = [
        (
            "test-driven-development.SKILL.md",
            "test-driven-development",
            "before writing implementation code",
        ),
        (
            "frontend-design.SKILL.md",
            "frontend-design",
            "distinctive, intentional visual design",
        ),
        (
            "playwright-cli.SKILL.md",
            "playwright-cli",
            "Automate browser interactions",
        ),
        (
            "angular-developer.SKILL.md",
            "angular-developer",
            "Generates Angular code",
        ),
        // A name with a colon in it. Valid YAML — the colon is not followed by
        // a space — and the kind of thing a hand-rolled splitter gets wrong.
        (
            "kouji-analyzer.SKILL.md",
            "kouji:analyzer",
            "analyze their LinkedIn profile",
        ),
        ("docx.SKILL.md", "docx", "manipulate Word documents"),
    ];

    for (file, name, phrase) in table {
        let (text, path) = read(file);
        let doc = match parse::skill_md(&text, &path) {
            Ok(doc) => doc,
            Err(e) => panic!("{file} is a published skill and did not parse: {e}"),
        };
        assert_eq!(doc.name, name, "{file}");
        assert!(
            doc.description.contains(phrase),
            "{file}: description {:?} does not contain {phrase:?}",
            doc.description
        );
        assert!(!doc.body.is_empty(), "{file}: body is empty");
    }
}

#[test]
fn unknown_frontmatter_is_preserved() {
    // `license` and `metadata` are not keys this crate knows. Google ships
    // them; the parser must carry them through rather than refuse or drop them.
    let (text, path) = read("angular-developer.SKILL.md");
    let doc = parse::skill_md(&text, &path).expect("a published skill parses");

    assert_eq!(
        doc.front
            .get(serde_yaml_ng::Value::from("license"))
            .and_then(serde_yaml_ng::Value::as_str),
        Some("MIT"),
    );
    let metadata = doc
        .front
        .get(serde_yaml_ng::Value::from("metadata"))
        .and_then(serde_yaml_ng::Value::as_mapping)
        .expect("the nested mapping survived");
    assert_eq!(
        metadata
            .get(serde_yaml_ng::Value::from("version"))
            .and_then(serde_yaml_ng::Value::as_str),
        Some("1.0"),
    );

    // And a round trip: re-rendering and re-parsing keeps every key, including
    // the ones this crate has never heard of.
    let round: SkillDoc = parse::skill_md(&doc.to_markdown(), &path).expect("the round trip");
    assert_eq!(round.front, doc.front);
    assert_eq!(round.body, doc.body);
    assert_eq!(round.name, doc.name);
}

#[test]
fn an_unknown_key_alone_is_not_an_error() {
    let text = "---\nname: x\ndescription: d\nsomething-from-2029: [1, 2]\n---\nbody\n";
    let doc = parse::skill_md(text, Path::new("x/SKILL.md")).expect("unknown keys are ignored");
    assert_eq!(doc.name, "x");
    assert!(
        doc.front
            .contains_key(serde_yaml_ng::Value::from("something-from-2029")),
    );
}

#[test]
fn missing_name_is_an_error_naming_the_file() {
    let path = Path::new("some/where/SKILL.md");
    let err = parse::skill_md("---\ndescription: d\n---\nbody\n", path)
        .expect_err("a skill with no name is not a skill");
    assert!(matches!(err, SkillError::MissingName { .. }));
    let rendered = err.to_string();
    assert!(
        rendered.contains("some") && rendered.contains("SKILL.md"),
        "the error must name the file: {rendered}"
    );
}

#[test]
fn missing_front_matter_is_an_error_naming_the_file() {
    let path = Path::new("some/where/SKILL.md");
    let err = parse::skill_md("# just a document\n", path).expect_err("no front matter");
    assert!(matches!(err, SkillError::MissingFrontMatter { .. }));
    assert!(err.to_string().contains("SKILL.md"));
}
