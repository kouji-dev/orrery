//! `orrery auth login|logout|status` — the verb the binary already told people
//! to run.
//!
//! # What was wrong
//!
//! `orrery-ext-provider-anthropic::oauth` is a complete RFC 8628 device-code
//! flow: `CredStore`, refresh with a mutex so two passes make one request,
//! `slow_down`, `expired_token`, `access_denied`, logout. Fourteen green tests.
//! And **no entry point from the product**: there was no `Auth` variant in the
//! command tree, so the binary's own message —
//!
//! ```text
//! orrery: sign in first - no credential for the `anthropic` grant:
//!         run `orrery auth login anthropic`
//! ```
//!
//! — named a command that answered `error: unrecognized subcommand 'auth'`.
//! The only working path to a real model was an undocumented
//! `ANTHROPIC_API_KEY`.
//!
//! This module is the entry point, and `tests/auth.rs` drives it through the
//! built binary against a loopback authorization server — no network, no key.
//!
//! # It draws nothing it invented
//!
//! The wait is a [`Surface`] the flow itself emits, printed as it came. One
//! description serves ratatui, Ink, the ADE and `--json`, which is the rule
//! every other surface in this binary follows; a login that hand-rolled its own
//! banner would be the one screen no other client could draw.

use std::sync::Arc;

use async_trait::async_trait;
use orrery_proto::{Surface, SurfaceKind};
use orrery_provider::{AuthCtx, AuthState, ProviderAuth, ProviderError};

use crate::args::{AuthCommand, Cli};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch an `auth` subcommand.
pub fn dispatch(cli: &Cli, command: &AuthCommand) -> ! {
    match command {
        AuthCommand::Login { provider, auth_url } => {
            login(cli, grant_of(provider.as_deref()), auth_url.as_deref())
        }
        AuthCommand::Logout { provider } => logout(cli, grant_of(provider.as_deref())),
        AuthCommand::Status { provider } => status(cli, grant_of(provider.as_deref())),
    }
}

/// The grant a bare `orrery auth status` means.
///
/// One provider needs a login in this build, so defaulting is not a guess. A
/// second one arriving is what turns this into a question.
const DEFAULT_GRANT: &str = "anthropic";

/// Every provider this binary knows how to sign in to.
///
/// A short list, and it is checked rather than assumed: `orrery auth login
/// nope` used to start an Anthropic login under a grant called `nope`, which is
/// a command that appears to work and signs nobody in.
const SIGNS_IN: &[&str] = &[DEFAULT_GRANT];

fn grant_of(provider: Option<&str>) -> String {
    let grant = provider.unwrap_or(DEFAULT_GRANT);
    if !SIGNS_IN.contains(&grant) {
        fail(
            Exit::Usage,
            format!(
                "`{grant}` is not a provider this build can sign in to. What it can: {}",
                SIGNS_IN.join(", ")
            ),
        );
    }
    grant.to_owned()
}

/// Sign in, and wait for the person to finish.
fn login(cli: &Cli, grant: String, auth_url: Option<&str>) -> ! {
    let auth = auth_for(cli, &grant, auth_url);
    let json = layers::wants_json(cli);
    let ctx = Printing { json };
    match runtime().block_on(auth.login(&ctx)) {
        Ok(state) => {
            report(&grant, &state, json);
            match state {
                AuthState::Ready { .. } | AuthState::Anonymous => Exit::Ok.exit(),
                // A login that came back anything else did not sign anybody in,
                // whatever it printed.
                _ => Exit::NeedsLogin.exit(),
            }
        }
        Err(e) => fail(Exit::NeedsLogin, e),
    }
}

/// Forget everything about one grant.
fn logout(cli: &Cli, grant: String) -> ! {
    let auth = auth_for(cli, &grant, None);
    match runtime().block_on(auth.logout()) {
        Ok(()) => {
            if layers::wants_json(cli) {
                println!(
                    "{}",
                    serde_json::json!({ "grant": grant, "state": "signed-out" })
                );
            } else {
                println!("signed out of `{grant}`");
            }
            Exit::Ok.exit()
        }
        Err(e) => fail(Exit::Kernel, e),
    }
}

/// Where this machine stands, without asking anyone anything.
fn status(cli: &Cli, grant: String) -> ! {
    let auth = auth_for(cli, &grant, None);
    match runtime().block_on(auth.state()) {
        Ok(state) => {
            let code = match state {
                AuthState::Ready { .. } | AuthState::Anonymous => Exit::Ok,
                // 5, the same code a turn that needs a login exits with, so a
                // script asking "can I run?" gets one answer from both.
                _ => Exit::NeedsLogin,
            };
            report(&grant, &state, layers::wants_json(cli));
            code.exit()
        }
        Err(e) => fail(Exit::Kernel, e),
    }
}

/// The flow for one grant, over the store `orrery auth` and the provider share.
fn auth_for(cli: &Cli, grant: &str, auth_url: Option<&str>) -> Arc<dyn ProviderAuth> {
    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
    });
    let state_dir = cli
        .state_dir
        .clone()
        .unwrap_or_else(|| workspace.join(".orrery"));
    orrery_harness::features::anthropic_auth(grant, &state_dir, auth_url).unwrap_or_else(|e| {
        // Naming the flag is the whole message: a build without the feature has
        // no Anthropic login, and saying only "no provider" sends somebody
        // looking for a configuration key that does not exist.
        fail(
            Exit::Usage,
            format!(
                "{e}
  this build was made without it: `cargo build -p orrery-cli --features anthropic`"
            ),
        )
    })
}

/// Print an [`AuthState`] the way the rest of the binary prints things: data on
/// stdout, one object for `--json`.
fn report(grant: &str, state: &AuthState, json: bool) {
    if json {
        let value = match state {
            AuthState::Ready {
                account,
                expires_at,
            } => serde_json::json!({
                "grant": grant, "state": "ready",
                "account": account, "expires_at": expires_at,
            }),
            AuthState::Anonymous => {
                serde_json::json!({ "grant": grant, "state": "anonymous" })
            }
            AuthState::Expired => serde_json::json!({ "grant": grant, "state": "expired" }),
            AuthState::NeedsLogin { reason } => serde_json::json!({
                "grant": grant, "state": "needs-login", "reason": reason,
            }),
            AuthState::Pending {
                user_code,
                verification_uri,
                verification_uri_complete,
                expires_at,
                interval_secs,
            } => serde_json::json!({
                "grant": grant, "state": "pending",
                "user_code": user_code,
                "verification_uri": verification_uri,
                "verification_uri_complete": verification_uri_complete,
                "expires_at": expires_at,
                "interval_secs": interval_secs,
            }),
            // `AuthState` is `#[non_exhaustive]`.
            other => serde_json::json!({ "grant": grant, "state": format!("{other:?}") }),
        };
        println!("{value}");
        return;
    }
    match state {
        AuthState::Ready {
            account,
            expires_at,
        } => {
            let who = account.as_deref().unwrap_or("this machine");
            match expires_at {
                Some(at) => println!("signed in to `{grant}` as {who}, until unix {at}"),
                None => println!("signed in to `{grant}` as {who}"),
            }
        }
        AuthState::Anonymous => println!("`{grant}` needs no credential"),
        AuthState::Expired => {
            println!("the sign-in for `{grant}` has expired: run `orrery auth login {grant}`");
        }
        AuthState::NeedsLogin { reason } => println!("not signed in to `{grant}`: {reason}"),
        AuthState::Pending { user_code, .. } => {
            println!("waiting for `{user_code}` to be entered");
        }
        other => println!("`{grant}`: {other:?}"),
    }
}

/// A client that prints what it is asked to draw and answers nothing.
///
/// The device-code flow needs no answer — the person answers in a browser —
/// so this is the whole of the terminal's side of it.
struct Printing {
    json: bool,
}

#[async_trait]
impl AuthCtx for Printing {
    async fn ask(&self, surface: Surface) -> Result<serde_json::Value, ProviderError> {
        match &surface.kind {
            SurfaceKind::Markdown { value, .. } if !self.json => print!("{value}"),
            // `--json` stdout stays parseable: the surface goes out as the
            // object a client would have been handed, not as prose.
            _ => println!(
                "{}",
                serde_json::to_value(&surface).unwrap_or(serde_json::Value::Null)
            ),
        }
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        Ok(serde_json::Value::Object(serde_json::Map::new()))
    }
}

/// A runtime for the one async call this command makes.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| fail(Exit::Kernel, format!("could not start the runtime: {e}")))
}
