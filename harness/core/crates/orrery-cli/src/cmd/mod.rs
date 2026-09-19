//! One module per subcommand, as `17-cli.md` lays them out. Each lands with
//! its own plan; until then it exits 2 naming that plan.

pub mod attach;
pub mod config;
pub mod eval;
pub mod ext;
pub mod import;
pub mod init;
pub mod install;
pub mod interactive;
pub mod layers;
pub mod ledger;
pub mod mcp;
pub mod permissions;
pub mod replay;
pub mod run;
pub mod serve;
pub mod session;
pub mod skills;
pub mod workflow;

use std::path::PathBuf;

use orrery_harness::ProviderChoice;
use orrery_kernel::KernelConfig;

use crate::args::Cli;
use crate::exit::{Exit, fail};
use crate::session::Setup;

/// The forms `--provider` understands, in the order the error lists them.
///
/// Kept beside [`provider_choice`] because the message a person reads when they
/// get it wrong is the only documentation most people will see.
const FORMS: &str = "  fixture:<path-to.jsonl>            replay a committed stream; repeat for one per pass
  anthropic:<model>[@<base-url>]     the Messages API
  openai-compat:<model>@<base-url>   any chat-completions endpoint
  ollama:<model>[@<base-url>]        openai-compat, default http://localhost:11434/v1
  vllm:<model>[@<base-url>]          openai-compat, default http://localhost:8000/v1";

/// Turn the `--provider` flags into the choice they name.
///
/// `None` when none was given: the caller falls back to a `[provider]` table,
/// which is the other route to this same enum. The two must reach the same set
/// of variants — a flag that parses fewer kinds than a config file is a flag
/// that lies about what the build can do.
fn provider_choice(cli: &Cli) -> Option<ProviderChoice> {
    if cli.provider.is_empty() {
        return None;
    }
    // `fixture:` is the one repeatable form: one stream per pass, the last
    // repeating. Every other kind describes a single endpoint, so a second one
    // is an ambiguity rather than a second pass.
    let fixtures: Vec<PathBuf> = cli
        .provider
        .iter()
        .filter_map(|spec| spec.strip_prefix("fixture:"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect();
    if fixtures.len() == cli.provider.len() {
        return Some(ProviderChoice::Fixture { passes: fixtures });
    }
    if cli.provider.len() > 1 {
        fail(
            Exit::Usage,
            format!(
                "`--provider` was given {n} times with more than one kind;                  only `fixture:` repeats, one stream per pass",
                n = cli.provider.len()
            ),
        );
    }
    let spec = &cli.provider[0];
    let (kind, rest) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
    // `<model>@<base-url>` — split on the first `@`, because a model id never
    // has one and a URL after it may have anything.
    let (model, base_url) = match rest.split_once('@') {
        Some((model, url)) => (model, Some(url.to_owned())),
        None => (rest, None),
    };
    let model = model.to_owned();
    Some(match kind {
        "anthropic" => ProviderChoice::Anthropic {
            model: if model.is_empty() {
                "claude-sonnet-4-5".to_owned()
            } else {
                model
            },
            credential: "anthropic".to_owned(),
            base_url,
        },
        "openai-compat" | "ollama" | "vllm" => {
            let base_url = base_url.unwrap_or_else(|| match kind {
                "ollama" => "http://localhost:11434/v1".to_owned(),
                "vllm" => "http://localhost:8000/v1".to_owned(),
                // Guessing which local server somebody runs is worse than
                // asking, so plain `openai-compat` with no url is a usage error.
                _ => fail(
                    Exit::Usage,
                    "`openai-compat:` needs the server root:                      `openai-compat:<model>@http://host:port/v1`",
                ),
            });
            ProviderChoice::OpenAiCompat {
                model,
                credential: "openai-compat".to_owned(),
                base_url,
            }
        }
        "fixture" => fail(
            Exit::Usage,
            "`fixture:` needs a path: `fixture:<path-to.jsonl>`",
        ),
        other => fail(
            Exit::Usage,
            format!("`{other}` is not a provider. What is understood:
{FORMS}"),
        ),
    })
}

/// Turn the global flags into something [`Session::build`] can use.
///
/// [`Session::build`]: crate::session::Session::build
pub fn setup(cli: &Cli) -> Setup {
    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
    });
    let state_dir = cli
        .state_dir
        .clone()
        .unwrap_or_else(|| workspace.join(".orrery"));
    // The layers on disk, folded into the shape the kernel runs on. This is the
    // one path from a config file to a running turn: without it `maxUsd` is
    // inert, the retry policy is whatever was compiled in, and `--profile` picks
    // a name nothing reads. `layers::resolve` exits 2 on a file that will not
    // parse, which is the right answer for something a person wrote.
    let resolved = layers::resolve(cli);
    // The flag first, then the `[provider]` table in force. Neither is
    // privileged over the other in what it can *name*; the flag simply wins
    // when both speak.
    let provider = provider_choice(cli).or_else(|| {
        orrery_harness::config::provider_choice(&resolved.values, &resolved.profile.name)
    });
    let kernel = orrery_harness::kernel_config(
        &resolved.values,
        &resolved.profile.name,
        KernelConfig {
            model: "fixture".to_owned(),
            ..KernelConfig::default()
        },
    );

    // The `[[route]]` list lives beside `[permissions]`, in the workspace's own
    // `orrery.toml`. One file a person already has, rather than a flag: a
    // routing rule and a permission rule are the same kind of thing and belong
    // in the same place.
    let routing_toml = std::fs::read_to_string(workspace.join("orrery.toml")).ok();

    Setup {
        workspace,
        state_dir,
        profile: cli.profile.clone().unwrap_or_else(|| "default".to_owned()),
        provider,
        routing_toml,
        kernel,
        // What `orrery ext list` prints and what a turn can actually call have
        // to be the same set, or installing an extension is theatre.
        extensions: orrery_harness::extension_sources(&resolved),
        // Same argument, for `[permissions]`: what `permissions explain` says
        // and what a turn is allowed to do have to be the one rule set, or a
        // denial the operator was shown is a denial nothing enforces.
        policy: std::sync::Arc::new(layers::rules(cli, &resolved)),
    }
}

/// Build a session, or exit with the code the failure deserves.
pub fn session(cli: &Cli) -> crate::session::Session {
    match crate::session::Session::build(&setup(cli)) {
        Ok(session) => session,
        Err(e @ crate::session::SetupError::NoProvider) => fail(Exit::Usage, e),
        Err(e) => fail(Exit::Kernel, e),
    }
}
