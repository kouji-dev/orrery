//! Remembers that a specific FILE answered `--version`, so the second and every
//! later detection pass costs no spawns at all.
//!
//! Detection's expensive half is the probe: up to five tools x a launch of a
//! ~100MB binary, and on Windows the first launch after boot drags an on-access
//! virus scan with it. The sweep that FINDS those files is cheap by comparison
//! (~34ms worst case, see [`resolve`](super::resolve)).
//!
//! So this caches the probe result and nothing else. The directory sweep still
//! runs in full on every pass, deliberately: resolution is what changes when the
//! user edits PATH, installs a tool, or switches node versions, and a cached
//! resolution would keep serving the old answer — reporting a tool at a path
//! that no longer wins, or missing one that just appeared. Caching the whole
//! [`ToolStatus`](super::ToolStatus) would have exactly that bug. Sweep always,
//! probe once.
//!
//! Only SUCCESSES are cached, and this is the load-bearing rule of the module: a
//! probe failure is overwhelmingly transient (a timeout under a cold scan, a
//! machine that was thrashing, a tool mid-upgrade), and a cached failure would
//! freeze the bogus amber "needs path" tile that this whole change set exists to
//! kill — with no way for the user to clear it short of touching the binary.
//! A failure therefore costs a re-probe next pass, forever, on purpose.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::settings::SettingsService;

/// Settings-table key holding the persisted entries. App bookkeeping, not a user
/// preference, so it is an auxiliary kv row rather than a `Settings` field —
/// same shape the Defender record uses.
const KV_KEY: &str = "tool_probe_cache";

/// How many probe results are kept. Five adapters x a few candidates each is the
/// real working set; the cap exists so a user who keeps re-pointing the manual
/// path picker at different binaries cannot grow the settings row without bound.
const MAX_ENTRIES: usize = 32;

/// One proven-good binary. The key is (`path`, `mtime_ms`, `size`): an upgrade
/// rewrites the file, so either stamp moving is enough to force a fresh probe,
/// and together they also catch a same-path replacement that preserved mtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Entry {
    path: String,
    mtime_ms: u64,
    size: u64,
    /// The parsed version, or `None` for a binary that ran but printed nothing
    /// we recognised — still a success, and still worth not re-spawning.
    version: Option<String>,
    /// Insertion time, used only to decide what the cap evicts.
    at_ms: u64,
}

fn cache() -> &'static Mutex<Vec<Entry>> {
    static CACHE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

/// The backing store, absent until [`attach`] runs. Tests (and the unit-test
/// binary at large) never attach, so they exercise the same code paths purely in
/// memory — a missing store is a supported state, not an error.
fn store() -> &'static Mutex<Option<SettingsService>> {
    static STORE: OnceLock<Mutex<Option<SettingsService>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(None))
}

/// The (mtime, size) stamp of a file, or `None` when it is gone — which is also
/// how an entry gets invalidated: an uninstall deletes the binary, the stamp
/// stops resolving, and the entry is dropped by [`prune`].
fn stamp(path: &str) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((mtime, meta.len()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Paths are compared the way the filesystem treats them: case-insensitively on
/// Windows, where the same binary reached through `C:\Bin\x.exe` and
/// `c:\bin\x.exe` would otherwise be probed twice.
fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Hand the cache its persistent store and load what the last run left behind,
/// so a cold start pays one sweep and zero spawns. Called once at setup.
pub fn attach(settings: SettingsService) {
    let mut entries: Vec<Entry> = settings
        .get_kv(KV_KEY)
        .ok()
        .flatten()
        .as_deref()
        .map(decode)
        .unwrap_or_default();
    prune(&mut entries);
    *cache().lock().unwrap() = entries;
    *store().lock().unwrap() = Some(settings);
}

/// The cached version for `path`, when a probe of THIS exact file already
/// succeeded. `Some(version)` means the caller must not spawn anything;
/// `None` means "no proof" and covers a miss, a stale stamp and a deleted file
/// alike.
pub fn get(path: &str) -> Option<Option<String>> {
    let (mtime_ms, size) = stamp(path)?;
    let entries = cache().lock().unwrap();
    entries
        .iter()
        .find(|e| same_path(&e.path, path) && e.mtime_ms == mtime_ms && e.size == size)
        .map(|e| e.version.clone())
}

/// Record that `path` RAN. Callers must only reach here on success — see the
/// module docs for why a failure is never stored.
pub fn remember(path: &str, version: Option<String>) {
    let Some((mtime_ms, size)) = stamp(path) else {
        return; // vanished between the probe and now — nothing worth keeping
    };
    let entry = Entry { path: path.to_string(), mtime_ms, size, version, at_ms: now_ms() };
    let snapshot = {
        let mut entries = cache().lock().unwrap();
        entries.retain(|e| !same_path(&e.path, path));
        entries.push(entry);
        prune(&mut entries);
        entries.clone()
    };
    // Outside the cache lock: the store takes the DB mutex, and holding both in
    // one order here while detection's threads take them in another is the
    // deadlock this ordering avoids.
    if let Some(settings) = store().lock().unwrap().as_ref() {
        if let Err(e) = settings.set_kv(KV_KEY, &encode(&snapshot)) {
            log::debug!("probe cache not persisted: {e}");
        }
    }
}

/// Drop entries whose binary is gone (uninstalls, a version manager switching
/// away) and then the oldest beyond [`MAX_ENTRIES`].
fn prune(entries: &mut Vec<Entry>) {
    entries.retain(|e| Path::new(&e.path).is_file());
    if entries.len() > MAX_ENTRIES {
        entries.sort_by_key(|e| e.at_ms);
        let excess = entries.len() - MAX_ENTRIES;
        entries.drain(..excess);
    }
}

/// Serialised form of the whole table — one JSON array in one settings row.
/// Malformed input decodes to nothing rather than erroring: a cache is never a
/// reason detection fails.
fn decode(raw: &str) -> Vec<Entry> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn encode(entries: &[Entry]) -> String {
    serde_json::to_string(entries).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    use rusqlite::Connection;

    fn tool(dir: &Path, name: &str, body: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_hit_needs_the_same_path_mtime_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = tool(dir.path(), "probecache-key", "v1");
        remember(&path, Some("1.2.3".into()));
        assert_eq!(get(&path), Some(Some("1.2.3".into())), "same file → hit");

        // Same length, different bytes and a bumped mtime: an in-place upgrade.
        std::fs::write(&path, "v2").unwrap();
        assert_eq!(get(&path), None, "an mtime bump re-probes");

        // …and a size change alone is enough, even if mtime were preserved.
        remember(&path, Some("2.0.0".into()));
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "v2-but-longer").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_eq!(get(&path), None, "a size change re-probes");
    }

    #[test]
    fn a_deleted_binary_is_never_a_hit_and_is_evicted() {
        let dir = tempfile::tempdir().unwrap();
        let path = tool(dir.path(), "probecache-gone", "x");
        remember(&path, Some("9.9.9".into()));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(get(&path), None, "an uninstalled tool cannot hit");

        let mut entries = vec![Entry { path, ..Default::default() }];
        prune(&mut entries);
        assert!(entries.is_empty(), "a path that is gone is dropped");
    }

    #[test]
    fn a_success_with_no_parseable_version_is_still_cached() {
        // The tool ran — that is the expensive fact worth keeping. An
        // unrecognised banner is not a failure and must not cost a respawn.
        let dir = tempfile::tempdir().unwrap();
        let path = tool(dir.path(), "probecache-noversion", "x");
        remember(&path, None);
        assert_eq!(get(&path), Some(None));
    }

    #[test]
    fn the_cap_evicts_the_oldest_entries_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut entries: Vec<Entry> = (0..MAX_ENTRIES + 5)
            .map(|i| Entry {
                path: tool(dir.path(), &format!("probecache-cap-{i}"), "x"),
                at_ms: i as u64,
                ..Default::default()
            })
            .collect();
        prune(&mut entries);
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries.first().unwrap().at_ms, 5, "the oldest five went");
    }

    #[test]
    fn entries_survive_a_restart_and_garbage_decodes_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![Entry {
            path: tool(dir.path(), "probecache-persist", "x"),
            mtime_ms: 7,
            size: 1,
            version: Some("3.1.4".into()),
            at_ms: 11,
        }];
        // Round-trips through the same settings row a real restart reads.
        let db = Arc::new(StdMutex::new(Connection::open_in_memory().unwrap()));
        let settings = SettingsService::new(db);
        settings.set_kv(KV_KEY, &encode(&entries)).unwrap();
        let back = decode(&settings.get_kv(KV_KEY).unwrap().unwrap());
        assert_eq!(back, entries);

        // A hand-edited or half-written row must not break detection.
        assert!(decode("not json {").is_empty());
    }
}
