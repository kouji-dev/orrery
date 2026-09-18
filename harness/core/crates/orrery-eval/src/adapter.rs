//! Driving a competing harness under our cases, budgets and graders.
//!
//! # This is a port, not a rediscovery
//!
//! `ade/src-tauri/src/agents/adapters/{claude,codex,pi}.rs` and its `mod.rs`
//! already know how to start these CLIs, and the hard part is Windows: an npm
//! install leaves a `claude.cmd`, a `claude.ps1` and an extensionless `claude`
//! side by side, `CreateProcessW` rejects the extensionless one with os error
//! 193, and resolving in the wrong order finds the shell script a Git Bash
//! install left behind. The rules ported here, one for one:
//!
//! - **Extension order is ours, then `PATHEXT`, then `""` last** — the ADE's
//!   `resolve::exe_extensions`. `""` first is what used to pick the `sh` script.
//! - **A script shim is launched through its interpreter** — `.cmd`/`.bat`
//!   through `cmd.exe /c call`, `.ps1` through `pwsh` when present and
//!   `powershell.exe` otherwise. The ADE's `launch_prefix`.
//! - **An extensionless shim is redirected to its sibling** `.cmd`/`.exe`, for
//!   os error 193.
//!
//! What is *not* ported is the ADE's probe cache, its registry PATH sweep and
//! its hook merging: an eval adapter starts one process per case with a prompt
//! and reads what it prints. See `adapter::argv_reuses_ade_knowledge`.
//!
//! # Their cost is not our cost
//!
//! We read our own numbers at the provider boundary. For an external CLI there
//! is no boundary of ours to stand at, so what we get is what the tool chose to
//! print — and the result carries
//! [`CostProvenance::ReportedByTool`](crate::report::CostProvenance::ReportedByTool)
//! all the way into the report rather than being shown as if it were the same
//! kind of measurement.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use orrery_proto::{ContentBlock, SessionRef, Usage, UserInput};
use orrery_session::SessionStore;
use orrery_session::turn::{NewTurn, TurnKind};
use serde::{Deserialize, Serialize};

use crate::error::EvalError;
use crate::report::{ByRole, CostProvenance, Timing};
use crate::run::{CaseCtx, CaseRunner, RunOutput};

/// How an external CLI prints its result.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParseMode {
    /// One JSON object at the end of the stream.
    #[default]
    Json,
    /// One JSON object per line; the last one that carries usage wins.
    Jsonl,
}

/// One `[adapter.<id>]` block.
///
/// ```toml
/// [adapter.codex]
/// command = "codex exec --json"
/// parse   = "jsonl"
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdapterSpec {
    /// How the matrix names it.
    pub id: String,
    /// The command line, program first. Split on spaces, as the TOML writes it.
    pub command: String,
    /// How to read what it prints.
    #[serde(default)]
    pub parse: ParseMode,
}

impl AdapterSpec {
    /// A spec from a command line.
    #[must_use]
    pub fn new(id: impl Into<String>, command: impl Into<String>, parse: ParseMode) -> Self {
        Self {
            id: id.into(),
            command: command.into(),
            parse,
        }
    }

    /// The command line, split into program and arguments.
    #[must_use]
    pub fn argv(&self) -> Vec<String> {
        self.command
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }
}

/// The non-interactive argv for the three CLIs the ADE already drives.
///
/// Claude Code prints one JSON object with `--print --output-format json`;
/// codex streams JSONL from `exec --json`; pi takes the prompt as a positional
/// argument. These are the same binaries and the same names the ADE's
/// `base_argv` resolves — see the module docs.
#[must_use]
pub fn known_adapter(id: &str) -> Option<AdapterSpec> {
    Some(match id {
        "claude" | "claude-code" => AdapterSpec::new(
            "claude-code",
            "claude --print --output-format json",
            ParseMode::Json,
        ),
        "codex" => AdapterSpec::new("codex", "codex exec --json", ParseMode::Jsonl),
        "pi" => AdapterSpec::new("pi", "pi --print", ParseMode::Json),
        _ => return None,
    })
}

/// Executable extensions to try, in order.
///
/// Ported from the ADE's `resolve::exe_extensions`. Real images first, then
/// whatever `PATHEXT` adds, and `""` **last** — an extensionless npm shim and a
/// Git-Bash `sh` script have the same name as the real thing, and putting `""`
/// first is how detection used to find the shell script.
#[must_use]
pub fn exe_extensions() -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }
    let preferred = [".exe", ".com", ".cmd", ".bat", ".ps1"];
    let mut exts: Vec<String> = preferred.iter().map(|s| (*s).to_owned()).collect();
    if let Some(pathext) = std::env::var_os("PATHEXT") {
        for raw in pathext.to_string_lossy().split(';') {
            let ext = raw.trim().to_ascii_lowercase();
            // `.js`, `.vbs` and `.wsf` are in a default PATHEXT and would
            // resolve to something Windows opens rather than something it runs.
            if ext.is_empty() || matches!(ext.as_str(), ".js" | ".vbs" | ".wsf" | ".msc") {
                continue;
            }
            if !exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                exts.push(ext);
            }
        }
    }
    exts.push(String::new());
    exts
}

/// The `(program, leading args)` an OS process launcher can actually start.
///
/// Ported from the ADE's `launch_prefix`. On Windows a `.cmd`/`.bat` goes
/// through `cmd.exe /c call`, a `.ps1` through PowerShell, and an
/// extensionless shim is redirected to a sibling with an extension, because
/// `CreateProcessW` rejects it with os error 193. Off Windows the path is the
/// program.
#[must_use]
pub fn launch_prefix(path: &Path) -> (String, Vec<String>) {
    if !cfg!(windows) {
        return (path.display().to_string(), Vec::new());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("cmd" | "bat") => (
            "cmd.exe".to_owned(),
            vec![
                "/c".to_owned(),
                "call".to_owned(),
                path.display().to_string(),
            ],
        ),
        Some("ps1") => (
            powershell_program(),
            vec![
                "-NoProfile".to_owned(),
                "-ExecutionPolicy".to_owned(),
                "Bypass".to_owned(),
                "-File".to_owned(),
                path.display().to_string(),
            ],
        ),
        Some(_) => (path.display().to_string(), Vec::new()),
        // Extensionless: `CreateProcessW` will not take it. Look for a sibling
        // that it will, in our own preference order, and only then give up.
        None => {
            for candidate in [".exe", ".cmd", ".bat", ".ps1"] {
                let sibling = path.with_extension(candidate.trim_start_matches('.'));
                if sibling.is_file() {
                    return launch_prefix(&sibling);
                }
            }
            (path.display().to_string(), Vec::new())
        }
    }
}

/// PowerShell 7 when it is on PATH, Windows PowerShell otherwise. The ADE's
/// `powershell_program`, and for its reasons: faster start, better UTF-8.
#[must_use]
pub fn powershell_program() -> String {
    if which(Path::new("pwsh")).is_some() {
        "pwsh.exe".to_owned()
    } else {
        "powershell.exe".to_owned()
    }
}

/// The first on-disk hit for a bare program name, in [`exe_extensions`] order.
///
/// A path with a separator in it is taken as given, which is what lets a test
/// hand the runner a fake binary.
#[must_use]
pub fn which(program: &Path) -> Option<PathBuf> {
    let raw = program.to_string_lossy();
    if raw.contains('/') || raw.contains('\\') {
        return program.is_file().then(|| program.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in exe_extensions() {
            let candidate = dir.join(format!("{raw}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// What an external CLI said it spent.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportedCost {
    /// The usage it printed. Zeroes when it printed none.
    pub usage: Usage,
    /// Whether the tool said anything about cost at all. When false, the run's
    /// cost is unknown rather than free, and the report must not read it as
    /// zero spend.
    pub reported: bool,
}

/// Read a cost out of what a CLI printed.
///
/// Tolerant on purpose: each tool names its fields differently and they move
/// between versions. `usage.input_tokens` / `usage.output_tokens` (Claude
/// Code), `token_usage.input_tokens` (codex) and a `total_cost_usd` in dollars
/// are all understood; anything else leaves `reported` false.
#[must_use]
pub fn parse_reported_cost(text: &str, parse: ParseMode) -> ReportedCost {
    let mut out = ReportedCost::default();
    let documents: Vec<&str> = match parse {
        ParseMode::Json => vec![text.trim()],
        ParseMode::Jsonl => text.lines().collect(),
    };
    for doc in documents {
        let doc = doc.trim();
        if doc.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(doc) else {
            continue;
        };
        let usage = value
            .get("usage")
            .or_else(|| value.get("token_usage"))
            .or_else(|| value.pointer("/info/token_usage"));
        if let Some(usage) = usage {
            let n = |key: &str| usage.get(key).and_then(serde_json::Value::as_u64);
            let input = n("input_tokens").or_else(|| n("prompt_tokens"));
            let output = n("output_tokens").or_else(|| n("completion_tokens"));
            if input.is_some() || output.is_some() {
                out.usage.input_tokens = input.unwrap_or(0);
                out.usage.output_tokens = output.unwrap_or(0);
                out.usage.cache_hits = n("cache_read_input_tokens").unwrap_or(0);
                out.reported = true;
            }
        }
        if let Some(usd) = value
            .get("total_cost_usd")
            .or_else(|| value.get("cost_usd"))
            .and_then(serde_json::Value::as_f64)
        {
            // Micro-USD as an integer, like everywhere else in the harness.
            out.usage.micro_usd = Some((usd * 1_000_000.0).round().max(0.0) as u64);
            out.reported = true;
        }
    }
    out
}

/// Runs one case against an external agent CLI.
pub struct ExternalRunner {
    spec: AdapterSpec,
    store: Arc<dyn SessionStore>,
    program: PathBuf,
    timeout_ms: u64,
}

impl ExternalRunner {
    /// An adapter over a resolved program path.
    ///
    /// The path is resolved once, here, rather than per case: a suite that
    /// takes an hour should not depend on PATH staying the same throughout.
    ///
    /// # Errors
    ///
    /// [`EvalError::Adapter`] when the program cannot be found at all.
    pub fn new(spec: AdapterSpec, store: Arc<dyn SessionStore>) -> Result<Self, EvalError> {
        let argv = spec.argv();
        let program = argv.first().ok_or_else(|| EvalError::Adapter {
            adapter: spec.id.clone(),
            detail: "the command line is empty".to_owned(),
        })?;
        let resolved = which(Path::new(program)).ok_or_else(|| EvalError::Adapter {
            adapter: spec.id.clone(),
            detail: format!("`{program}` is not on PATH"),
        })?;
        Ok(Self {
            spec,
            store,
            program: resolved,
            timeout_ms: 120_000,
        })
    }

    /// Point the adapter at one particular binary, PATH or no PATH. This is how
    /// a test drives a fake agent.
    #[must_use]
    pub fn with_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.program = program.into();
        self
    }

    /// How long one case may take.
    #[must_use]
    pub const fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// The full argv for one prompt: the launch prefix, the spec's own
    /// arguments, then the prompt.
    #[must_use]
    pub fn argv_for(&self, prompt: &str) -> (String, Vec<String>) {
        let (program, mut args) = launch_prefix(&self.program);
        args.extend(self.spec.argv().into_iter().skip(1));
        args.push(prompt.to_owned());
        (program, args)
    }
}

#[async_trait]
impl CaseRunner for ExternalRunner {
    fn label(&self) -> &str {
        &self.spec.id
    }

    fn cost_provenance(&self) -> CostProvenance {
        CostProvenance::ReportedByTool {
            tool: self.spec.id.clone(),
        }
    }

    async fn run(&self, ctx: CaseCtx<'_>) -> Result<RunOutput, EvalError> {
        let started = Instant::now();
        let (program, args) = self.argv_for(&ctx.case.prompt);

        let output = tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            tokio::process::Command::new(&program)
                .args(&args)
                .current_dir(ctx.workspace)
                .output(),
        )
        .await
        .map_err(|_| EvalError::Adapter {
            adapter: self.spec.id.clone(),
            detail: format!("timed out after {}ms", self.timeout_ms),
        })?
        .map_err(|e| EvalError::Adapter {
            adapter: self.spec.id.clone(),
            detail: format!("could not start `{program}`: {e}"),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let cost = parse_reported_cost(&stdout, self.spec.parse);

        // The transcript is ours even when the run was not: a grader and
        // `eval replay` need the same shape whichever harness produced it.
        let session = self
            .store
            .create(&ctx.workspace.display().to_string(), &ctx.point.profile)
            .await?;
        let handle = self.store.open(session).await?;
        let lease = self.store.lease(handle.root).await?;
        self.store
            .append(
                &lease,
                NewTurn::new(TurnKind::User {
                    input: UserInput::text(&ctx.case.prompt),
                }),
            )
            .await?;
        self.store
            .append(
                &lease,
                NewTurn::new(TurnKind::Assistant {
                    content: vec![ContentBlock::Text {
                        text: stdout.clone(),
                    }],
                    usage: cost.usage,
                }),
            )
            .await?;

        Ok(RunOutput {
            stopped_by: None,
            cost: cost.usage,
            // Empty on purpose. Role attribution is a fact about *our* loop;
            // inventing one for another harness would be a made-up number.
            by_role: ByRole::new(),
            timing: Timing {
                wall_ms: started.elapsed().as_millis() as u64,
                model_ms: 0,
                tool_ms: 0,
            },
            turns: 1,
            tool_calls: 0,
            transcript: SessionRef {
                session,
                branch: handle.root,
                turn: None,
            },
        })
    }
}
