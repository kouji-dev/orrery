//! The clap tree. Every command in `17-cli.md` is present from phase 0; the
//! ones that have not landed yet exit 2 naming their plan file.

use clap::{Parser, Subcommand, ValueEnum};

use crate::ui::Ui;

/// One binary that starts a kernel, attaches a renderer, and explains itself.
#[derive(Debug, Parser)]
#[command(name = "orrery", version, about, long_about = None)]
pub struct Cli {
    /// Which profile to resolve.
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// The workspace root. Defaults to the current directory.
    #[arg(long, global = true, value_name = "PATH")]
    pub workspace: Option<std::path::PathBuf>,

    /// Renderer. Defaults to ratatui on a tty, json otherwise.
    #[arg(long, global = true, value_name = "UI")]
    pub ui: Option<Ui>,

    /// Shorthand for `--ui json`.
    #[arg(long, global = true, conflicts_with = "ui")]
    pub json: bool,

    /// Override the profile's consent mode, within the managed clamp.
    #[arg(long, global = true, value_name = "MODE")]
    pub consent: Option<Consent>,

    /// Log level on stderr. Never mixed into `--json` stdout. Repeatable.
    #[arg(short = 'v', global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Where the model comes from, as `<kind>:<what>`: `fixture:<path.jsonl>`,
    /// `anthropic:<model>[@<url>]`, `openai-compat:<model>@<url>`, or its
    /// `ollama:` / `vllm:` aliases. Only `fixture:` repeats, one stream per
    /// pass, the last repeating. Without it, a `[provider]` table decides.
    #[arg(long, global = true, value_name = "SPEC")]
    pub provider: Vec<String>,

    /// Where the session database lives. Defaults to `<workspace>/.orrery`.
    #[arg(long, global = true, value_name = "PATH")]
    pub state_dir: Option<std::path::PathBuf>,

    /// The command to run. With none, start an interactive session.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// When the harness asks before acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Consent {
    /// Ask every time.
    Always,
    /// Ask once per matching call, then remember for the session.
    Once,
    /// Never ask: deny instead, so CI cannot hang.
    Never,
}

/// Which harness to import configuration from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ImportFrom {
    /// Claude Code.
    ClaudeCode,
    /// Codex.
    Codex,
}

/// The top-level command tree.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run one turn, print the final text, and exit.
    Run {
        /// The prompt.
        #[arg(short = 'p', long, value_name = "PROMPT")]
        prompt: String,
    },
    /// Start a kernel, print its endpoint, and keep running without a client.
    Serve {
        /// The address or pipe name to listen on.
        #[arg(long, value_name = "ADDR")]
        listen: Option<String>,
    },
    /// Attach a renderer to a running kernel.
    Attach {
        /// The endpoint `serve` printed, or `$ORRERY_ENDPOINT`.
        #[arg(value_name = "ENDPOINT")]
        endpoint: String,
        /// Replay from this sequence number instead of the start.
        #[arg(long, value_name = "SEQ")]
        since: Option<u64>,
        /// Submit one turn after attaching, and render it.
        #[arg(long, short = 'p', value_name = "PROMPT")]
        submit: Option<String>,
    },
    /// Re-emit a stored session as events, so any renderer can draw any past session.
    Replay {
        /// The session id.
        #[arg(value_name = "SESSION")]
        session: String,
    },
    /// Inspect stored sessions.
    Session {
        /// What to do with them.
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Install an extension. A bare name is the signed registry.
    ///
    /// Seven source forms, disambiguated by prefix, in the one vocabulary
    /// `orrery registry add --source` also speaks: `name`, `name@1.2.0`,
    /// `github:owner/repo#ref`, a git URL, `crates-io:<name>` (also spelled
    /// `crate:<name>`), `npm:<name>`, and a path — `./path`, `../path`,
    /// `file:path` or an absolute one.
    Install {
        /// What to install.
        #[arg(value_name = "SOURCE")]
        source: String,
        /// Which layer to install into. Defaults to the user layer.
        ///
        /// Spelled `--to` rather than `--workspace` because `--workspace <PATH>`
        /// is already a global flag naming the workspace root.
        #[arg(long = "to", value_name = "LAYER")]
        to: Option<Layer>,
        /// Shorthand for `--to user`.
        #[arg(long, conflicts_with = "to")]
        user: bool,
        /// Symlink a local path instead of copying it: the development loop.
        #[arg(long)]
        link: bool,
        /// Grant everything the extension asks for without asking.
        #[arg(long)]
        yes: bool,
        /// Replace an existing install rather than refusing.
        #[arg(long)]
        force: bool,
        /// A signed index file to resolve registry names against.
        #[arg(long, value_name = "PATH")]
        index: Option<std::path::PathBuf>,
    },
    /// Remove an installed extension.
    Remove {
        /// The extension name.
        #[arg(value_name = "NAME")]
        name: String,
        /// Which layer to remove it from. Without it, being installed at two
        /// layers is a question rather than a guess.
        #[arg(long = "from", value_name = "LAYER")]
        from: Option<Layer>,
        /// Shorthand for `--from user`.
        #[arg(long, conflicts_with = "from")]
        user: bool,
    },
    /// Author a signed registry index: the pin set an organisation enforces.
    ///
    /// The other side of `[registry] unpinned = "refuse"`. Offline: the
    /// packages come from the same mirror directory an install stages from.
    Registry {
        /// What to do with an index.
        #[command(subcommand)]
        command: RegistryCommand,
    },
    /// Inspect and manage extensions.
    Ext {
        /// What to do with them.
        #[command(subcommand)]
        command: ExtCommand,
    },
    /// Sign in to a model provider, sign out, or say where you stand.
    ///
    /// The device-code flow: this prints a short code and a page to type it
    /// on, and waits. Nothing is written anywhere a person has to manage — the
    /// token goes to the `creds` grant under the state directory, which is
    /// what `orrery auth logout` takes back.
    Auth {
        /// What to do.
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Explain a permission decision.
    Permissions {
        /// What to explain.
        #[command(subcommand)]
        command: PermissionsCommand,
    },
    /// Explain resolved configuration.
    Config {
        /// What to explain.
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Decide whether a workspace's own configuration and code may load.
    Trust {
        /// What to do about it.
        #[command(subcommand)]
        command: TrustCommand,
    },
    /// Write a starter configuration into the workspace.
    Init {
        /// Which profile to scaffold.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
    },
    /// Import configuration from another harness.
    Import {
        /// The harness to import from.
        #[arg(long = "from", value_name = "HARNESS")]
        from: Option<ImportFrom>,
    },
    /// Inspect the MCP servers this workspace declares.
    Mcp {
        /// What to do with them.
        #[command(subcommand)]
        command: McpCommand,
    },
    /// Inspect the skills the discovery pass found.
    Skills {
        /// What to do with them.
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Check and run declared workflows.
    Workflow {
        /// What to do with one.
        #[command(subcommand)]
        command: WorkflowCommand,
    },
    /// Run, compare and replay evaluation suites.
    Eval {
        /// What to do.
        #[command(subcommand)]
        command: EvalCommand,
    },
    /// Query the decision ledger: what was asked, what was decided, and which
    /// rule decided it.
    Ledger {
        /// Only this session's stream. Without it, every session in the state
        /// directory, oldest first.
        #[arg(long, value_name = "ID")]
        session: Option<String>,
        /// Only decisions by this subject: `agent`, `ext:<id>`, `agent:<name>`.
        #[arg(long, value_name = "SUBJECT")]
        subject: Option<String>,
        /// Only decisions naming this rule, by id or by the text it was
        /// written as.
        #[arg(long, value_name = "RULE")]
        rule: Option<String>,
        /// Only one stream. Without it, every stream that carries a decision:
        /// what was loaded and what was refused as well as what was allowed.
        #[arg(long, value_name = "STREAM")]
        stream: Option<StreamName>,
        /// Show at most this many, counted from the end.
        #[arg(long, short = 'n', value_name = "N")]
        limit: Option<usize>,
    },
    /// Query the telemetry stream: model requests and routing decisions.
    Telemetry {
        /// Only this session's stream.
        #[arg(long, value_name = "ID")]
        session: Option<String>,
        /// Show at most this many, counted from the end.
        #[arg(long, short = 'n', value_name = "N")]
        limit: Option<usize>,
    },
}

/// One of the three audit streams, as `--stream` spells it.
///
/// `orrery ledger` reads both decision streams by default. This narrows it to
/// one, which is the question an operator asks about **retention** — the three
/// streams are separate files with separate retention — rather than about what
/// happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StreamName {
    /// What loaded, what degraded and what was refused.
    Load,
    /// Decisions, calls and consent. The evidence stream.
    Audit,
    /// Counts and timings. Also `orrery telemetry`.
    Telemetry,
}

impl From<StreamName> for orrery_audit::layer::Stream {
    fn from(name: StreamName) -> Self {
        match name {
            StreamName::Load => orrery_audit::layer::Stream::Load,
            StreamName::Audit => orrery_audit::layer::Stream::Audit,
            StreamName::Telemetry => orrery_audit::layer::Stream::Telemetry,
        }
    }
}

/// `orrery session ...`
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List stored sessions.
    List,
    /// Show one session.
    Show {
        /// The session id.
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Delete one session. History cannot be reconstructed, so this asks first.
    Rm {
        /// The session id.
        #[arg(value_name = "ID")]
        id: String,
        /// Skip the confirmation. Required in non-interactive use.
        #[arg(long)]
        yes: bool,
    },
}

/// Which configuration layer an install lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Layer {
    /// `~/.orrery/extensions/<id>/`. Personal, and available in every project.
    User,
    /// `<workspace>/.orrery/extensions/<id>/`. Committed with the repository.
    Workspace,
}

/// `orrery ext ...`
///
/// The everyday actions — install and remove — are bare top-level verbs
/// (`orrery install`, `orrery remove`). What stays here is what is not
/// everyday.
#[derive(Debug, Subcommand)]
pub enum ExtCommand {
    /// List loaded extensions, including degraded and skipped ones with reasons.
    List,
    /// Run an extension's tests against the mock broker, with no model and no network.
    Test {
        /// An extension's name, or a path written as one: `./dir`, `../dir`,
        /// an absolute path, or an `orrery.toml`. Anything else is a name,
        /// whatever the current directory happens to hold. Defaults to the
        /// workspace directory.
        #[arg(value_name = "NAME|PATH")]
        path: Option<std::path::PathBuf>,
    },
}

/// `orrery registry ...`
///
/// Section 8 phase 8 asks an admin to pin a version set. Until these landed
/// there was no way to **make** one: the binary could verify a signed index and
/// could not produce a signed index. Everything here is local; nothing fetches.
#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// Make a signing key and an empty index to fill.
    Init {
        /// Where the index goes.
        #[arg(long, value_name = "PATH")]
        index: std::path::PathBuf,
        /// Where the signing key goes. Defaults to `signing-key.toml` beside
        /// the index. **It is a secret**; the public half is printed.
        #[arg(long, value_name = "PATH")]
        key: Option<std::path::PathBuf>,
        /// How signatures name the key.
        #[arg(long = "key-id", value_name = "ID", default_value = "managed")]
        key_id: String,
        /// When the index stops being accepted, RFC 3339 UTC. Defaults to a
        /// year out: an index nobody maintains should eventually expire.
        #[arg(long, value_name = "INSTANT")]
        expires: Option<String>,
    },
    /// Pin one version, by staging it the way an install will.
    Add {
        /// The index to extend.
        #[arg(long, value_name = "PATH")]
        index: std::path::PathBuf,
        /// The extension's namespace.
        #[arg(long, value_name = "ID")]
        id: String,
        /// The exact version. Not a range: a range is not a pin.
        #[arg(long, value_name = "VERSION")]
        version: String,
        /// Where the bytes come from: `crates-io:<name>` (also spelled
        /// `crate:<name>`, as `orrery install` writes it), `npm:<name>` or
        /// `url:<url>`. Three of the one source vocabulary — the three whose
        /// bytes an index can hash. The registry indexes; it does not host.
        #[arg(long, value_name = "SOURCE")]
        source: String,
    },
    /// Sign every pin and then the document.
    Sign {
        /// The index to sign.
        #[arg(long, value_name = "PATH")]
        index: std::path::PathBuf,
        /// The signing key `init` wrote.
        #[arg(long, value_name = "PATH")]
        key: std::path::PathBuf,
    },
    /// Check an index back, the way an install checks it.
    Verify {
        /// The index to check.
        #[arg(long, value_name = "PATH")]
        index: std::path::PathBuf,
        /// The public key, as hex. Without it, the managed layer's.
        #[arg(long, value_name = "HEX")]
        key: Option<String>,
    },
}

/// `orrery auth ...`
///
/// Three verbs, and a provider name that defaults rather than being required:
/// `orrery auth status` with nothing after it is the question a person actually
/// asks. The name is the **grant** name, which is what every message about a
/// missing credential already says (`no credential for the `anthropic` grant`),
/// so the word in the error is the word to type.
#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Sign in. Prints a code and a page, then waits for the person.
    Login {
        /// Which provider. Defaults to `anthropic`.
        #[arg(value_name = "PROVIDER")]
        provider: Option<String>,
        /// The authorization server to talk to, for a gateway that fronts it.
        ///
        /// Also the seam the tests drive: a loopback server in the test's own
        /// process, so signing in is exercised end to end with nothing leaving
        /// the machine.
        #[arg(long = "auth-url", value_name = "URL")]
        auth_url: Option<String>,
    },
    /// Sign out: forget the token, the refresh token and the expiry.
    Logout {
        /// Which provider. Defaults to `anthropic`.
        #[arg(value_name = "PROVIDER")]
        provider: Option<String>,
    },
    /// Say whether this machine is signed in, and until when.
    ///
    /// Exits 0 when it is and 5 when a login is needed, so a script can ask.
    Status {
        /// Which provider. Defaults to `anthropic`.
        #[arg(value_name = "PROVIDER")]
        provider: Option<String>,
    },
}

/// `orrery permissions ...`
#[derive(Debug, Subcommand)]
pub enum PermissionsCommand {
    /// Explain what would happen to a call, naming the rule, layer, file and line.
    Explain {
        // Every example below is run back through the parser by
        // `tests/permissions_enforced.rs`. Help that documents a spelling the
        // parser rejects — as this line did, with
        // `builtin.write:$WORKSPACE/src/**` — is its own defect.
        /// One call in the rule grammar: `read(./src/main.rs)`,
        /// `write(./src/main.rs)`, `spawn(cmd: git)` or `net(domain: docs.rs)`.
        /// A call names one thing; it is not a pattern.
        #[arg(value_name = "CALL")]
        call: String,
        /// Whose permissions to answer for: `agent` (the default), `agent:<name>`
        /// for a sub-agent, `ext:<id>` for an extension.
        ///
        /// A sub-agent's answer is not the agent's: it inherits the agent's
        /// rules and its own file narrows them. The tool list a sub-agent is
        /// offered is filtered through exactly this question.
        #[arg(long, value_name = "SUBJECT")]
        subject: Option<String>,
    },
}

/// `orrery trust ...`
///
/// The verb the trust store never had. A workspace's own `config.toml`, its
/// extensions and its interceptors do not load until somebody has vouched for
/// the workspace, and until this landed the only way to say so was to hand-edit
/// `trust.auto = true` into the user layer — so `orrery init` wrote a file that
/// nothing could activate.
#[derive(Debug, Subcommand)]
pub enum TrustCommand {
    /// Trust a workspace: its own configuration and code load from now on.
    Grant {
        /// Which workspace. Defaults to the one `--workspace` names.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Forget the answer for a workspace, so it is untrusted again.
    Revoke {
        /// Which workspace. Defaults to the one `--workspace` names.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },
    /// Every stored answer, and what this workspace's is.
    List,
}

/// `orrery config ...`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Explain where a key's value came from, naming the winning layer.
    Explain {
        /// The configuration key.
        #[arg(value_name = "KEY")]
        key: String,
    },
}

/// `orrery eval ...`
#[derive(Debug, Subcommand)]
pub enum EvalCommand {
    /// Run a suite.
    Run {
        /// The suite to run.
        #[arg(value_name = "SUITE")]
        suite: String,
        /// Profiles to run it against.
        #[arg(long, value_name = "NAMES", value_delimiter = ',')]
        profile: Vec<String>,
        /// Models to run it against.
        #[arg(long, value_name = "MODELS", value_delimiter = ',')]
        model: Vec<String>,
        /// Report format.
        #[arg(long, value_name = "FORMAT")]
        format: Option<String>,
    },
    /// Compare two runs.
    Compare {
        /// The baseline run.
        #[arg(value_name = "RUN-A")]
        run_a: String,
        /// The run to compare against it.
        #[arg(value_name = "RUN-B")]
        run_b: String,
    },
    /// Re-open one case of a past run.
    Replay {
        /// The run id.
        #[arg(value_name = "RUN")]
        run: String,
        /// The case id.
        #[arg(long, value_name = "ID")]
        case: String,
    },
}

/// `orrery mcp ...`
///
/// Two questions, and they are deliberately separate: **what is declared** does
/// not start anything, and **what a server offers** does. §4.11's lifecycle —
/// discovered at session start, connected when first needed — would be a
/// sentence in a plan rather than a property if one command did both.
#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// List declared servers, the layer that declared them, and their health.
    /// Starts no process.
    List,
    /// Connect to one server and list the tools it offers, by the namespaced
    /// `ToolRef` a policy rule would name.
    Tools {
        /// The server name, without the `mcp.` prefix.
        #[arg(value_name = "SERVER")]
        server: String,
    },
}

/// `orrery skills ...`
#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// List discovered skills, with what each one's scripts may do.
    List {
        /// Only the skills this agent may load. A skill scoped elsewhere is
        /// absent, not refused.
        #[arg(long, value_name = "AGENT")]
        agent: Option<String>,
    },
    /// Show one skill's front matter and body.
    Show {
        /// The skill name, from its front matter.
        #[arg(value_name = "NAME")]
        name: String,
    },
}

/// `orrery workflow ...`
///
/// Two verbs, and the split is the same one `mcp` makes: **asking** whether a
/// file is well-formed must work with no provider in sight, and **running** it
/// is the thing that costs money. Translation #5 — an invalid workflow fails at
/// load, not after three model calls — is a property only if the two are
/// separable.
#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    /// Typecheck one and print its steps. Starts no kernel.
    Check {
        /// The `.toml` file.
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
    },
    /// Run one to its end and report what it came to.
    Run {
        /// The `.toml` file.
        #[arg(value_name = "FILE")]
        file: std::path::PathBuf,
        /// A token ceiling for the whole run, overriding the workflow's own.
        #[arg(long, value_name = "N")]
        max_tokens: Option<u64>,
    },
}
