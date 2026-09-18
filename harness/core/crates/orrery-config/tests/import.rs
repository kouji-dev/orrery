//! Task 8 · `init` and `import`.
//!
//! Import is **one-way and explicit**: it reads a foreign config once, when a
//! person runs the command, and writes an orrery config for them to review.
//! Nothing reads a foreign config at runtime.

mod common;

use common::Fixture;
use orrery_config::{StartupCtx, import, resolve};
use orrery_policy::{PendingCall, PolicyEngine, Verdict};
use orrery_proto::{AgentScope, BranchId, Grant, Layer, Subject};

const CLAUDE: &str = include_str!("fixtures/claude-settings.json");
const CODEX: &str = include_str!("fixtures/codex-config.toml");

fn scope() -> AgentScope {
    AgentScope {
        agent: "test".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

/// Round-trip: import, then load what was written back as a real layer.
fn engine(fx: &Fixture, written: &str) -> PolicyEngine {
    let file = orrery_config::LayerFile::new(Layer::User, "imported.toml", written);
    let rules = orrery_config::merge::policy(fx.root(), std::slice::from_ref(&file))
        .expect("what import wrote is a config the harness loads");
    PolicyEngine::new(rules)
}

#[test]
fn claude_code_permissions() {
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.root().join("src")).unwrap();
    std::fs::write(fx.root().join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(fx.root().join(".env"), "SECRET=1").unwrap();

    let imported = import::claude_code(CLAUDE).expect("a real-shaped settings file imports");
    let written = imported.to_toml();

    // The three lists came across in the rule grammar.
    assert!(written.contains("spawn(npm run test *)"), "{written}");
    assert!(written.contains("read(./src/**)"), "{written}");
    assert!(written.contains("net(domain: docs.rs)"), "{written}");
    assert!(written.contains("mcp(github.get_issue)"), "{written}");
    assert!(written.contains("write(./**)"), "{written}");
    assert!(written.contains("spawn(curl *)"), "{written}");

    // And it round-trips: what was written loads, and decides the same way.
    let engine = engine(&fx, &written);
    let spawn_npm = PendingCall::spawn("npm run test -- --quiet");
    assert_eq!(
        engine.check(&spawn_npm, &Subject::Agent, &scope()).verdict(),
        Verdict::Allow
    );
    assert_eq!(
        engine
            .check(&PendingCall::spawn("curl https://x"), &Subject::Agent, &scope())
            .verdict(),
        Verdict::Deny,
        "a denied Bash pattern is still denied"
    );
    assert_eq!(
        engine
            .check(&PendingCall::read("./.env"), &Subject::Agent, &scope())
            .verdict(),
        Verdict::Deny
    );
    assert_eq!(
        engine
            .check(&PendingCall::read("./src/main.rs"), &Subject::Agent, &scope())
            .verdict(),
        Verdict::Allow
    );
    assert_eq!(
        engine
            .check(&PendingCall::write("./src/main.rs"), &Subject::Agent, &scope())
            .verdict(),
        Verdict::Ask,
        "Claude Code's ask list stays an ask"
    );
    // A deny that a Claude allow overlaps still wins: WebFetch(domain:*) is
    // denied, so the docs.rs allow cannot carve an exception out of it.
    assert_eq!(
        engine
            .check(&PendingCall::net("docs.rs"), &Subject::Agent, &scope())
            .verdict(),
        Verdict::Deny
    );

    // The MCP server and the model came across too.
    assert!(written.contains("[mcp_servers.github]"), "{written}");
    assert!(written.contains("claude-sonnet-4-5"), "{written}");
    // And what could not be mapped is reported rather than dropped silently.
    assert!(
        imported.notes.iter().any(|n| n.contains("hooks")),
        "{:?}",
        imported.notes
    );
}

#[test]
fn codex_mcp_servers() {
    let imported = import::codex(CODEX).expect("a real-shaped codex config imports");
    let written = imported.to_toml();

    assert!(written.contains("[mcp_servers.ripgrep]"), "{written}");
    assert!(written.contains("command = \"rg-mcp\""), "{written}");
    assert!(written.contains("[mcp_servers.github]"), "{written}");
    assert!(written.contains("@modelcontextprotocol/server-github"), "{written}");
    assert!(written.contains("model = \"o3\""), "{written}");

    // It is a config the harness loads, and discovery finds both servers.
    let parsed: toml::Value = written.parse().expect("valid TOML");
    let servers = parsed["mcp_servers"].as_table().expect("a table");
    assert_eq!(servers.len(), 2);
    assert_eq!(
        servers["github"]["env"]["GITHUB_TOKEN"].as_str(),
        Some(""),
        "an empty credential is carried, never invented"
    );
}

#[test]
fn is_explicit_and_one_way() {
    let fx = Fixture::new();
    // Both foreign configs, sitting right there in a trusted workspace.
    fx.write_in_root(".claude/settings.json", CLAUDE);
    fx.write_in_root(".codex/config.toml", CODEX);
    fx.write_in_root(".orrery/config.toml", "model = \"ours\"\n");

    let cfg = resolve(&StartupCtx::new(fx.paths()).with_answer(true)).expect("resolve");
    assert!(cfg.trust.is_trusted());
    assert_eq!(cfg.values.str("model"), Some("ours"));
    assert!(
        cfg.manifest.mcp_servers.is_empty(),
        "nothing read the foreign configs: {:?}",
        cfg.manifest
    );
    assert!(
        !cfg.layers
            .iter()
            .any(|l| l.path.to_string_lossy().contains(".claude")
                || l.path.to_string_lossy().contains(".codex")),
        "no foreign file is a layer"
    );
    assert_eq!(cfg.values.str("approval_policy"), None);
}

#[test]
fn init_writes_a_workspace_config_from_a_profile() {
    let fx = Fixture::new();
    fx.write("home/.orrery/config.toml", CONFIG);

    let cfg = resolve(&StartupCtx::new(fx.paths()).with_profile("review")).expect("resolve");
    let written = import::init(&cfg.profile);

    assert!(written.contains("profile = \"review\""), "{written}");
    assert!(written.contains("model = \"claude-sonnet-5\""), "{written}");
    assert!(written.contains("\"git\""), "{written}");
    assert!(written.contains("deny"), "the shorthand became a real rule: {written}");
    assert!(written.contains("write(./**)"), "{written}");

    // What init writes is a config the harness loads.
    let file = orrery_config::LayerFile::new(Layer::Workspace, "config.toml", &written);
    let report = orrery_config::merge::merge(std::slice::from_ref(&file)).expect("it parses");
    assert_eq!(report.values.str("model"), Some("claude-sonnet-5"));
}

const CONFIG: &str = r#"
[profile.review]
model = "claude-sonnet-5"
extensions = ["git", "lsp"]
skills = ["review-checklist"]
permissions = { write = false }
"#;
