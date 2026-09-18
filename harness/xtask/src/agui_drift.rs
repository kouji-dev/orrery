//! `cargo xtask agui-drift` — is the vendored AG-UI enum still AG-UI?
//!
//! We vendor the event enum because there is no first-party Rust SDK and we only
//! ever produce AG-UI. The cost of vendoring is that upstream can move without
//! us noticing, so this job reads upstream's event list and diffs it against
//! [`orrery_agui::drift`].
//!
//! # Offline by design
//!
//! Reading upstream needs the network, which no test in this repo is allowed to
//! touch. So there are two sources, in order:
//!
//! 1. `@ag-ui/core`, if `clients/sdk-ts` has it installed. That is a file on
//!    disk, put there by `pnpm install` — no request from this process.
//! 2. The published schema at [`AGUI_SCHEMA_URL`], fetched with `curl`, and only
//!    when `--fetch` is passed. CI passes it; nobody else has to.
//!
//! With neither, the job compares against the pinned `UPSTREAM` list and says
//! so. It never fails for being offline unless `--strict` says it must.
//!
//! No model, no key, no paid API: this reads a list of event names.

use std::path::{Path, PathBuf};

/// What the job found.
#[derive(Debug, Default)]
pub struct Drift {
    /// Where the upstream list came from.
    pub source: String,
    /// Names we emit that upstream does not have. A red build: we invented one.
    pub invented: Vec<String>,
    /// Names upstream has that we neither emit nor deliberately skip. Reported,
    /// not failed: a new upstream event is news, not a bug.
    pub unaccounted: Vec<String>,
    /// Names we skip that upstream has dropped. Also just news.
    pub stale: Vec<String>,
}

impl Drift {
    /// Whether this is a red build.
    #[must_use]
    pub fn is_failure(&self) -> bool {
        !self.invented.is_empty()
    }

    /// Whether anything at all moved.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.invented.is_empty() && self.unaccounted.is_empty() && self.stale.is_empty()
    }
}

/// Anything that stopped the job.
#[derive(Debug, thiserror::Error)]
pub enum DriftError {
    /// No upstream list could be read and `--strict` was set.
    #[error(
        "no upstream event list: install `clients/sdk-ts` (pnpm install) or pass --fetch with a network"
    )]
    NoSource,
    /// The fetch failed.
    #[error("fetching {url}: {message}")]
    Fetch {
        /// What was being fetched.
        url: String,
        /// Why it failed.
        message: String,
    },
}

/// Read upstream's event names from the installed `@ag-ui/core`.
///
/// A file on disk, put there by `pnpm install`. This process makes no request.
#[must_use]
pub fn from_installed_package(root: &Path) -> Option<(PathBuf, Vec<String>)> {
    let candidates = [
        root.join("harness/clients/sdk-ts/node_modules/@ag-ui/core/dist/index.d.ts"),
        root.join("node_modules/@ag-ui/core/dist/index.d.ts"),
        root.join("harness/clients/sdk-ts/node_modules/@ag-ui/core/dist/index.js"),
        root.join("node_modules/@ag-ui/core/dist/index.js"),
    ];
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let names = scrape(&text);
        if !names.is_empty() {
            return Some((path, names));
        }
    }
    None
}

/// Fetch upstream's `events.ts` with `curl`.
///
/// `curl` rather than an HTTP crate on purpose: the fetch is the one part of
/// this repo that touches the network, and keeping it in a subprocess means no
/// build of the harness ever links a client that could.
///
/// # Errors
///
/// [`DriftError::Fetch`] when `curl` is missing or the fetch fails.
pub fn fetch() -> Result<Vec<String>, DriftError> {
    let url = orrery_agui::AGUI_SCHEMA_URL;
    let out = std::process::Command::new("curl")
        .args(["-sSfL", "--max-time", "30", url])
        .output()
        .map_err(|e| DriftError::Fetch {
            url: url.to_owned(),
            message: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(DriftError::Fetch {
            url: url.to_owned(),
            message: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(scrape(&String::from_utf8_lossy(&out.stdout)))
}

/// Pull `SCREAMING_SNAKE` event names out of whatever upstream calls its source.
///
/// Deliberately dumb: the shape of upstream's file is not ours to depend on, and
/// a scraper that only knows "a quoted all-caps token" survives a refactor that
/// a parser would not.
#[must_use]
pub fn scrape(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .split(|c: char| !(c.is_ascii_uppercase() || c == '_'))
        .filter(|token| token.len() > 2 && !token.starts_with('_') && !token.ends_with('_'))
        .map(str::to_owned)
        .collect();
    names.sort_unstable();
    names.dedup();
    // Only names that look like an event, not every constant upstream declares.
    // `CUSTOM` and `RAW` are events with no underscore in them, which is why
    // this is a prefix list rather than "anything SCREAMING_SNAKE".
    names.retain(|n| {
        [
            "RUN_",
            "STEP_",
            "TEXT_",
            "TOOL_",
            "STATE_",
            "MESSAGES_",
            "ACTIVITY_",
            "REASONING_",
            "SUBAGENT_",
            "CUSTOM",
            "RAW",
        ]
        .iter()
        .any(|prefix| n.starts_with(prefix))
    });
    names
}

/// Run the check.
///
/// # Errors
///
/// [`DriftError`] when `--strict` was set and no upstream list could be read.
pub fn run(root: &Path, allow_fetch: bool, strict: bool) -> Result<Drift, DriftError> {
    let (source, upstream) = match from_installed_package(root) {
        Some((path, names)) => (path.display().to_string(), names),
        None if allow_fetch => match fetch() {
            Ok(names) => (orrery_agui::AGUI_SCHEMA_URL.to_owned(), names),
            Err(e) if strict => return Err(e),
            Err(_) => pinned(),
        },
        None if strict => return Err(DriftError::NoSource),
        None => pinned(),
    };

    let emitted = orrery_agui::drift::EMITTED;
    let skipped: Vec<&str> = orrery_agui::drift::KNOWN_UNEMITTED
        .iter()
        .map(|(name, _)| *name)
        .collect();

    Ok(Drift {
        source,
        invented: emitted
            .iter()
            .filter(|n| !upstream.iter().any(|u| u == *n))
            .map(|n| (*n).to_owned())
            .collect(),
        unaccounted: upstream
            .iter()
            .filter(|u| !emitted.contains(&u.as_str()) && !skipped.contains(&u.as_str()))
            .cloned()
            .collect(),
        stale: skipped
            .iter()
            .filter(|n| !upstream.iter().any(|u| u == *n))
            .map(|n| (*n).to_owned())
            .collect(),
    })
}

fn pinned() -> (String, Vec<String>) {
    (
        format!(
            "the pinned list (AG-UI {}) — offline",
            orrery_agui::AGUI_PROTOCOL_VERSION
        ),
        orrery_agui::drift::UPSTREAM
            .iter()
            .map(|n| (*n).to_owned())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scraper finds event names in the shape upstream writes them, and
    /// leaves everything else alone.
    #[test]
    fn scrape_finds_event_names() {
        let sample = r#"
            export enum EventType {
              TEXT_MESSAGE_START = "TEXT_MESSAGE_START",
              RUN_STARTED = "RUN_STARTED",
              CUSTOM = "CUSTOM",
            }
            export const AGUI_METADATA_KEY = "ag-ui";
        "#;
        let found = scrape(sample);
        assert!(found.contains(&"TEXT_MESSAGE_START".to_owned()));
        assert!(found.contains(&"RUN_STARTED".to_owned()));
        assert!(
            found.contains(&"CUSTOM".to_owned()),
            "an event with no underscore in it is still an event: {found:?}"
        );
        assert!(
            !found.iter().any(|n| n.contains("METADATA")),
            "not every constant is an event: {found:?}"
        );
    }

    /// Offline, against the pinned list, the check is clean — which is what
    /// makes it safe to run anywhere.
    #[test]
    fn offline_against_the_pinned_list_is_clean() {
        let drift = run(Path::new("/nonexistent"), false, false).expect("no strict");
        assert!(drift.source.contains("offline"));
        assert!(drift.is_clean(), "{drift:?}");
        assert!(!drift.is_failure());
    }
}
