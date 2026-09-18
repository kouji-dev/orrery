//! The fixture runner, shared by every Rust client.
//!
//! `sdk-rs` runs these, and so do ratatui and `json` — a scenario that only the
//! SDK checks is a scenario that stops describing the clients.

use std::path::{Path, PathBuf};

use orrery_agui::Frame;

use crate::store::{StoreState, SurfaceStore};

/// One line of a scenario.
#[derive(Clone, Debug)]
pub enum Step {
    /// Apply this frame.
    Event(Box<Frame>),
    /// A frame this build cannot even parse. It still consumes its `seq`.
    Unknown {
        /// Its sequence number.
        seq: u64,
        /// Its `type`, as written.
        what: String,
    },
    /// The complete store state expected at this point.
    Expect(Box<StoreState>),
}

/// One fixture file.
#[derive(Clone, Debug)]
pub struct Scenario {
    /// The file's stem: `text-only`.
    pub name: String,
    /// Where it came from.
    pub path: PathBuf,
    /// Its lines, in order.
    pub steps: Vec<Step>,
}

/// A fixture that could not be read.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    /// The file would not open.
    #[error("{path}: {source}")]
    Io {
        /// Which file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// A line would not parse. Named, because a typo in a fixture must be a
    /// load error rather than a surprise in the middle of somebody else's test.
    #[error("{path}:{line}: {message}")]
    Parse {
        /// Which file.
        path: PathBuf,
        /// Which line, 1-based.
        line: usize,
        /// What was wrong with it.
        message: String,
    },
}

/// How many scenarios there are — the whole set, exactly.
///
/// Every client asserts **this** number rather than a floor of its own. Two
/// clients with two different floors is how six fixtures get deleted and only
/// one suite notices, which is what nearly happened: ratatui asked for 16 and
/// Ink for 10.
///
/// It counts the `*.jsonl` files in `clients/conformance` and nothing below it.
/// The scripts in `conformance/streams/` are `ModelEvent` streams for
/// `orrery-ext-provider-fixture`, not AG-UI scenarios for a renderer, and
/// [`load_all`] is deliberately not recursive — see that directory's README.
///
/// Adding a scenario means bumping this by one in the same commit, which is the
/// point: the set is a contract, not a directory listing.
pub const SCENARIO_COUNT: usize = 16;

/// Where the fixtures live, relative to this crate.
#[must_use]
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance")
}

/// Load one scenario file.
///
/// # Errors
///
/// [`FixtureError`] naming the file and the line.
pub fn load(path: &Path) -> Result<Scenario, FixtureError> {
    let text = std::fs::read_to_string(path).map_err(|source| FixtureError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut steps = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|e| FixtureError::Parse {
                path: path.to_path_buf(),
                line: index + 1,
                message: e.to_string(),
            })?;
        if let Some(event) = value.get("ev") {
            steps.push(match serde_json::from_value::<Frame>(event.clone()) {
                Ok(frame) => Step::Event(Box::new(frame)),
                Err(_) => Step::Unknown {
                    seq: event
                        .get("seq")
                        .and_then(serde_json::Value::as_u64)
                        .ok_or_else(|| FixtureError::Parse {
                            path: path.to_path_buf(),
                            line: index + 1,
                            message: "an event with no seq".into(),
                        })?,
                    what: event
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("?")
                        .to_owned(),
                },
            });
        } else if let Some(expect) = value.get("expect") {
            steps.push(Step::Expect(Box::new(
                serde_json::from_value(expect.clone()).map_err(|e| FixtureError::Parse {
                    path: path.to_path_buf(),
                    line: index + 1,
                    message: format!("not a store state: {e}"),
                })?,
            )));
        } else {
            return Err(FixtureError::Parse {
                path: path.to_path_buf(),
                line: index + 1,
                message: "expected `ev` or `expect`".into(),
            });
        }
    }
    Ok(Scenario {
        name: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_path_buf(),
        steps,
    })
}

/// Load every `*.jsonl` scenario in a directory, sorted by name.
///
/// # Errors
///
/// [`FixtureError`] for the first file that will not load.
pub fn load_all(dir: &Path) -> Result<Vec<Scenario>, FixtureError> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| FixtureError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    paths.sort();
    paths.iter().map(|p| load(p)).collect()
}

/// Replay a scenario through a fresh store, asserting each checkpoint.
///
/// # Errors
///
/// A human-readable description of the first checkpoint that did not match.
pub fn run(scenario: &Scenario) -> Result<SurfaceStore, String> {
    let mut store = SurfaceStore::new();
    let mut checkpoint = 0usize;
    for step in &scenario.steps {
        match step {
            Step::Event(frame) => {
                store.apply(frame);
            }
            Step::Unknown { seq, what } => {
                store.apply_unknown(*seq, what);
            }
            Step::Expect(expected) => {
                checkpoint += 1;
                if store.state() != expected.as_ref() {
                    return Err(format!(
                        "{}: checkpoint {checkpoint} does not match\n  expected: {}\n  got:      {}",
                        scenario.name,
                        serde_json::to_string(expected.as_ref()).unwrap_or_default(),
                        serde_json::to_string(store.state()).unwrap_or_default(),
                    ));
                }
            }
        }
    }
    Ok(store)
}
