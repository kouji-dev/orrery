//! Task 2 · rule parsing.

mod common;

use common::Workspace;
use orrery_policy::{parse, PolicyBuilder, Selector};
use orrery_proto::{Aspect, Layer};

/// The grammar table from the plan, one row at a time.
#[test]
fn every_aspect_form() {
    let table: &[(&str, Aspect, Selector)] = &[
        ("tool(ripgrep.search)", Aspect::Tool, Selector::on("ripgrep.search")),
        (
            "tool(shell.exec: npm run *)",
            Aspect::Tool,
            Selector::on("shell.exec").with_specifier("npm run *"),
        ),
        ("mcp(github.*)", Aspect::Mcp, Selector::on("github.*")),
        ("mcp(github.get_*)", Aspect::Mcp, Selector::on("github.get_*")),
        ("skill(review-*)", Aspect::Skill, Selector::on("review-*")),
        ("ext(buildgraph)", Aspect::Ext, Selector::on("buildgraph")),
        ("mode(plan)", Aspect::Mode, Selector::on("plan")),
        ("mode(execute)", Aspect::Mode, Selector::on("execute")),
        ("read(./**)", Aspect::Read, Selector::on("./**")),
        ("write(./src/**)", Aspect::Write, Selector::on("./src/**")),
        ("spawn(bazel *)", Aspect::Spawn, Selector::on("bazel *")),
        (
            "net(domain: *.corp.internal)",
            Aspect::Net,
            Selector::on("*.corp.internal"),
        ),
        ("creds(*)", Aspect::Creds, Selector::on("*")),
        ("mem.write(global)", Aspect::MemWrite, Selector::on("global")),
        ("mem.read(session)", Aspect::MemRead, Selector::on("session")),
        // Unqualified: the whole aspect, not nothing.
        ("creds", Aspect::Creds, Selector::any()),
        // `param:value` matches one named input.
        (
            "tool(jira.create, param:project=ORR)",
            Aspect::Tool,
            Selector::on("jira.create").with_param("project", "ORR"),
        ),
    ];

    for (text, aspect, selector) in table {
        let parsed = parse::rule(text).unwrap_or_else(|e| panic!("`{text}` did not parse: {e}"));
        assert_eq!(parsed.0, *aspect, "aspect of `{text}`");
        assert_eq!(parsed.1, *selector, "selector of `{text}`");
    }
}

/// A Windows path is one pattern, not a prefix and a glob.
#[test]
fn a_drive_letter_is_not_a_specifier() {
    let (aspect, selector) = parse::rule(r"read(C:\work\**)").expect("parses");
    assert_eq!(aspect, Aspect::Read);
    assert_eq!(selector, Selector::on(r"C:\work\**"));
}

/// A `{a,b}` glob survives the term splitter.
#[test]
fn a_brace_glob_is_one_term() {
    let (_, selector) = parse::rule("read(./src/**/*.{rs,toml})").expect("parses");
    assert_eq!(selector, Selector::on("./src/**/*.{rs,toml}"));
}

#[test]
fn re_escape_hatch_warns() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
allow = ["tool(re:^git\\.(status|log)$)"]
"#;

    // Off by default: a rule that would not apply is worse than one that does
    // not parse, so this is an error and not a silent ignore.
    let refused = PolicyBuilder::new(ws.root()).layer_toml(toml, "p.toml", Layer::Project, false);
    let message = refused.expect_err("`re:` is off by default").to_string();
    assert!(message.contains("re:"), "{message}");
    assert!(message.contains("p.toml"), "{message}");

    // On deliberately: it parses, and it warns.
    let built = PolicyBuilder::new(ws.root())
        .layer_toml(toml, "p.toml", Layer::Project, true)
        .expect("the escape hatch is on")
        .build()
        .expect("it compiles");
    assert_eq!(built.warnings().len(), 1);
    assert!(built.warnings()[0].message.contains("escape hatch"));
    assert_eq!(built.warnings()[0].line, 3);
}

/// Open question 2, decided: a managed layer can forbid `re:` outright.
#[test]
fn a_managed_layer_can_forbid_the_escape_hatch() {
    let ws = Workspace::new();
    let toml = "[permissions]\nallow = [\"tool(re:.*)\"]\n";
    let err = PolicyBuilder::new(ws.root())
        .forbid_regex()
        .layer_toml(toml, "p.toml", Layer::User, true)
        .expect_err("forbidden outright");
    assert!(err.to_string().contains("re:"), "{err}");
}

#[test]
fn bad_rule_names_the_file_and_line() {
    let ws = Workspace::new();
    let toml = "\
[permissions]
allow = [
  \"tool(git.*)\",
  \"wrte(./src/**)\",
]
";
    let err = PolicyBuilder::new(ws.root())
        .layer_toml(toml, "permissions.toml", Layer::User, false)
        .expect_err("`wrte` is not an aspect");
    let text = err.to_string();
    assert!(text.contains("permissions.toml"), "{text}");
    assert!(text.contains(":4"), "the line is named: {text}");
    assert!(text.contains("wrte"), "{text}");
}

#[test]
fn an_unbalanced_rule_is_named_too() {
    let err = parse::rule("read(./**").expect_err("no closing paren");
    assert!(err.to_string().contains("closing"), "{err}");
}

#[test]
fn a_subject_key_that_is_not_a_subject_is_named() {
    let ws = Workspace::new();
    let toml = "[permissions.\"nonsense:thing\"]\nallow = [\"tool(git.*)\"]\n";
    let err = PolicyBuilder::new(ws.root())
        .layer_toml(toml, "permissions.toml", Layer::User, false)
        .expect_err("not a subject");
    assert!(err.to_string().contains("nonsense:thing"), "{err}");
}
