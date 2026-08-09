//! B4.4 — Local History: bounded, content-addressed snapshots of an agent's
//! changed files, so a destructive agent edit is recoverable without a commit.
//!
//! Layout (under the app data dir — agent repos stay untouched):
//!   local-history/{agent_id}/blobs/{hash[..2]}/{hash}   raw file bytes
//!   local-history/{agent_id}/index.jsonl                append-only timeline
//!
//! Snapshots are DELTAS: each entry lists (path, hash, size) of the files that
//! were dirty at that moment; unchanged content re-uses its blob (free dedup).
//! Restoring a point walks the timeline backward for the latest hash per path.
//! Retention prunes on write: max entries / age / total blob bytes per agent.

pub mod commands;

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::errors::{AppError, AppResult};

/// Skip files larger than this — big blobs would blow the size budget fast.
const MAX_FILE_BYTES: u64 = 2_000_000;
/// Per-agent retention budgets, enforced oldest-first on every write.
const MAX_ENTRIES: usize = 500;
const MAX_AGE_MS: u128 = 14 * 24 * 60 * 60 * 1000;
const MAX_BLOB_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotFile {
    pub path: String,
    pub hash: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    /// Unix ms.
    pub ts: u128,
    /// "watch" (settled fs burst) or "before-restore" (guard).
    pub trigger: String,
    pub files: Vec<SnapshotFile>,
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Local-history store rooted in the app data dir. One mutex serializes all
/// index writes (watcher thread + commands) — the work per write is tiny.
pub struct HistoryService {
    root: PathBuf,
    write_lock: Mutex<()>,
}

impl HistoryService {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            root: app_data_dir.join("local-history"),
            write_lock: Mutex::new(()),
        }
    }

    fn agent_dir(&self, agent_id: Uuid) -> PathBuf {
        self.root.join(agent_id.to_string())
    }

    fn blob_path(&self, agent_id: Uuid, hash: &str) -> PathBuf {
        self.agent_dir(agent_id)
            .join("blobs")
            .join(&hash[..2.min(hash.len())])
            .join(hash)
    }

    fn index_path(&self, agent_id: Uuid) -> PathBuf {
        self.agent_dir(agent_id).join("index.jsonl")
    }

    fn read_index(&self, agent_id: Uuid) -> Vec<Snapshot> {
        let Ok(text) = std::fs::read_to_string(self.index_path(agent_id)) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Newest-first timeline for the panel.
    pub fn list(&self, agent_id: Uuid) -> Vec<Snapshot> {
        let mut v = self.read_index(agent_id);
        v.reverse();
        v
    }

    /// Capture the CURRENT content of `paths` (worktree-relative) as one
    /// snapshot. Oversized/binary/unreadable files are skipped; an empty or
    /// unchanged file set writes nothing. Returns the entry when one landed.
    pub fn snapshot(
        &self,
        agent_id: Uuid,
        worktree: &Path,
        paths: &[String],
        trigger: &str,
    ) -> Option<Snapshot> {
        if paths.is_empty() {
            return None;
        }
        let _guard = self.write_lock.lock().unwrap();
        let mut files = Vec::new();
        for rel in paths {
            let abs = worktree.join(rel);
            let Ok(md) = std::fs::metadata(&abs) else {
                continue; // deleted mid-burst
            };
            if !md.is_file() || md.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(bytes) = std::fs::read(&abs) else {
                continue;
            };
            if bytes[..bytes.len().min(8192)].contains(&0) {
                continue; // binary
            }
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let blob = self.blob_path(agent_id, &hash);
            if !blob.exists() {
                if let Some(parent) = blob.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&blob, &bytes).is_err() {
                    continue;
                }
            }
            files.push(SnapshotFile {
                path: rel.replace('\\', "/"),
                hash,
                size: md.len(),
            });
        }
        if files.is_empty() {
            return None;
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        // identical to the latest entry → nothing new happened content-wise
        let index = self.read_index(agent_id);
        if let Some(last) = index.last() {
            let same = last.files.len() == files.len()
                && last
                    .files
                    .iter()
                    .zip(&files)
                    .all(|(a, b)| a.path == b.path && a.hash == b.hash);
            if same {
                return None;
            }
        }
        let entry = Snapshot {
            id: Uuid::new_v4().to_string(),
            ts: now_ms(),
            trigger: trigger.to_string(),
            files,
        };
        let path = self.index_path(agent_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let line = serde_json::to_string(&entry).ok()?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let _ = writeln!(f, "{line}");
        drop(f);
        self.prune(agent_id, index.len() + 1);
        Some(entry)
    }

    /// The stored content of `path` AT `snap_id`: the newest hash for the path
    /// at or before that snapshot in the timeline.
    pub fn file_at(&self, agent_id: Uuid, snap_id: &str, rel: &str) -> AppResult<String> {
        let index = self.read_index(agent_id);
        let pos = index
            .iter()
            .position(|s| s.id == snap_id)
            .ok_or_else(|| AppError::Other("snapshot not found".into()))?;
        let rel_norm = rel.replace('\\', "/");
        let hash = index[..=pos]
            .iter()
            .rev()
            .find_map(|s| {
                s.files
                    .iter()
                    .find(|f| f.path == rel_norm)
                    .map(|f| f.hash.clone())
            })
            .ok_or_else(|| AppError::Other("file not in this snapshot's history".into()))?;
        let bytes = std::fs::read(self.blob_path(agent_id, &hash))
            .map_err(|e| AppError::Other(format!("blob read: {e}")))?;
        String::from_utf8(bytes).map_err(|_| AppError::Other("blob is not UTF-8".into()))
    }

    /// Restore `paths` (None = every file of the snapshot) to their content at
    /// `snap_id`, guard-snapshotting their CURRENT content first so a restore
    /// is itself undoable. Returns the restored paths.
    pub fn restore(
        &self,
        agent_id: Uuid,
        worktree: &Path,
        snap_id: &str,
        paths: Option<Vec<String>>,
    ) -> AppResult<Vec<String>> {
        let index = self.read_index(agent_id);
        let snap = index
            .iter()
            .find(|s| s.id == snap_id)
            .ok_or_else(|| AppError::Other("snapshot not found".into()))?
            .clone();
        let targets: Vec<String> = match &paths {
            Some(list) => snap
                .files
                .iter()
                .map(|f| f.path.clone())
                .filter(|p| list.iter().any(|q| q.replace('\\', "/") == *p))
                .collect(),
            None => snap.files.iter().map(|f| f.path.clone()).collect(),
        };
        if targets.is_empty() {
            return Err(AppError::Other("nothing to restore".into()));
        }
        // undo guard: capture what those files look like RIGHT NOW
        self.snapshot(agent_id, worktree, &targets, "before-restore");
        let mut restored = Vec::new();
        for rel in &targets {
            let content = self.file_at(agent_id, snap_id, rel)?;
            let abs = worktree.join(rel);
            if let Some(parent) = abs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&abs, content)
                .map_err(|e| AppError::Other(format!("write '{rel}': {e}")))?;
            restored.push(rel.clone());
        }
        Ok(restored)
    }

    /// Drop an agent's entire history (agent removed).
    pub fn purge(&self, agent_id: Uuid) {
        let _ = std::fs::remove_dir_all(self.agent_dir(agent_id));
    }

    /// Enforce the per-agent budgets, oldest-first, then GC orphaned blobs.
    fn prune(&self, agent_id: Uuid, entry_count_hint: usize) {
        let cutoff = now_ms().saturating_sub(MAX_AGE_MS);
        let index = self.read_index(agent_id);
        let mut keep: Vec<&Snapshot> = index.iter().filter(|s| s.ts >= cutoff).collect();
        if keep.len() > MAX_ENTRIES {
            let drop_n = keep.len() - MAX_ENTRIES;
            keep.drain(..drop_n);
        }
        // size budget: total UNIQUE blob bytes of kept entries, oldest dropped
        loop {
            let mut seen = HashSet::new();
            let mut total = 0u64;
            for s in &keep {
                for f in &s.files {
                    if seen.insert(f.hash.as_str()) {
                        total += f.size;
                    }
                }
            }
            if total <= MAX_BLOB_BYTES || keep.len() <= 1 {
                break;
            }
            keep.remove(0);
        }
        let changed = keep.len() != index.len();
        if !changed && entry_count_hint <= MAX_ENTRIES {
            return;
        }
        if changed {
            let lines: Vec<String> = keep
                .iter()
                .filter_map(|s| serde_json::to_string(s).ok())
                .collect();
            let _ = std::fs::write(self.index_path(agent_id), lines.join("\n") + "\n");
            // GC blobs no longer referenced by any kept entry
            let referenced: HashSet<String> = keep
                .iter()
                .flat_map(|s| s.files.iter().map(|f| f.hash.clone()))
                .collect();
            let blobs_root = self.agent_dir(agent_id).join("blobs");
            let mut by_hash: HashMap<String, PathBuf> = HashMap::new();
            if let Ok(shards) = std::fs::read_dir(&blobs_root) {
                for shard in shards.flatten() {
                    if let Ok(files) = std::fs::read_dir(shard.path()) {
                        for f in files.flatten() {
                            by_hash.insert(f.file_name().to_string_lossy().into_owned(), f.path());
                        }
                    }
                }
            }
            for (hash, p) in by_hash {
                if !referenced.contains(&hash) {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> (HistoryService, tempfile::TempDir, tempfile::TempDir, Uuid) {
        let data = tempfile::tempdir().unwrap();
        let wt = tempfile::tempdir().unwrap();
        let s = HistoryService::new(data.path().to_path_buf());
        (s, data, wt, Uuid::new_v4())
    }

    #[test]
    fn snapshot_dedups_content_and_skips_identical_bursts() {
        let (s, _d, wt, id) = svc();
        std::fs::write(wt.path().join("a.txt"), "one").unwrap();
        let first = s
            .snapshot(id, wt.path(), &["a.txt".into()], "watch")
            .unwrap();
        assert_eq!(first.files.len(), 1);
        // identical burst → no new entry
        assert!(s.snapshot(id, wt.path(), &["a.txt".into()], "watch").is_none());
        // changed content → new entry, new blob; old blob still present
        std::fs::write(wt.path().join("a.txt"), "two").unwrap();
        let second = s
            .snapshot(id, wt.path(), &["a.txt".into()], "watch")
            .unwrap();
        assert_ne!(first.files[0].hash, second.files[0].hash);
        assert_eq!(s.list(id).len(), 2);
        assert_eq!(s.list(id)[0].id, second.id, "newest first");
    }

    #[test]
    fn skips_binary_oversized_and_missing() {
        let (s, _d, wt, id) = svc();
        std::fs::write(wt.path().join("bin.dat"), [0u8, 159, 146, 150]).unwrap();
        assert!(s
            .snapshot(id, wt.path(), &["bin.dat".into(), "missing.txt".into()], "watch")
            .is_none());
    }

    #[test]
    fn file_at_walks_the_timeline_backward() {
        let (s, _d, wt, id) = svc();
        std::fs::write(wt.path().join("a.txt"), "v1").unwrap();
        std::fs::write(wt.path().join("b.txt"), "b1").unwrap();
        let s1 = s
            .snapshot(id, wt.path(), &["a.txt".into(), "b.txt".into()], "watch")
            .unwrap();
        // later burst touches only b — a's content must still resolve at s2
        std::fs::write(wt.path().join("b.txt"), "b2").unwrap();
        let s2 = s.snapshot(id, wt.path(), &["b.txt".into()], "watch").unwrap();
        assert_eq!(s.file_at(id, &s1.id, "a.txt").unwrap(), "v1");
        assert_eq!(s.file_at(id, &s2.id, "a.txt").unwrap(), "v1");
        assert_eq!(s.file_at(id, &s2.id, "b.txt").unwrap(), "b2");
        assert_eq!(s.file_at(id, &s1.id, "b.txt").unwrap(), "b1");
    }

    #[test]
    fn restore_rewrites_files_and_guards_with_a_pre_snapshot() {
        let (s, _d, wt, id) = svc();
        std::fs::write(wt.path().join("a.txt"), "good").unwrap();
        let snap = s.snapshot(id, wt.path(), &["a.txt".into()], "watch").unwrap();
        std::fs::write(wt.path().join("a.txt"), "clobbered").unwrap();
        let restored = s.restore(id, wt.path(), &snap.id, None).unwrap();
        assert_eq!(restored, vec!["a.txt".to_string()]);
        assert_eq!(
            std::fs::read_to_string(wt.path().join("a.txt")).unwrap(),
            "good"
        );
        // the guard snapshot captured "clobbered" → the restore is undoable
        let timeline = s.list(id);
        assert_eq!(timeline[0].trigger, "before-restore");
        assert_eq!(
            s.file_at(id, &timeline[0].id, "a.txt").unwrap(),
            "clobbered"
        );
    }

    #[test]
    fn purge_removes_everything() {
        let (s, _d, wt, id) = svc();
        std::fs::write(wt.path().join("a.txt"), "x").unwrap();
        s.snapshot(id, wt.path(), &["a.txt".into()], "watch").unwrap();
        s.purge(id);
        assert!(s.list(id).is_empty());
    }
}
