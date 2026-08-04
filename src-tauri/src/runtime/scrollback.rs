//! A1.2 — Rust-owned scrollback ring: the bounded, backend-side source of
//! truth for each agent's recent raw PTY output.
//!
//! Why it exists: terminal truth used to live only in renderer xterms — the
//! hidden-queue lossy cap threw backlogs away for good, and a webview reload
//! lost every terminal. The ring is fed from the SAME reader→batcher pipeline
//! that ships output (see `runtime/mod.rs`), tagged with the batcher's
//! cumulative byte `seq`, so a renderer can `runtime_snapshot` → replay →
//! dedup live chunks by seq. It is also the safety precondition for A0.2's
//! `none` interest mode: bytes of unsubscribed agents land HERE (bounded)
//! instead of accumulating in the mux.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

/// Per-agent ring capacity. Why 1MB: the roadmap's bounded worst case — big
/// enough to refill a 5000-row xterm scrollback with typical line lengths,
/// small enough that 20 agents cost ≤ 20MB backend-side.
pub const RING_CAP: usize = 1024 * 1024;

/// What `runtime_snapshot` returns: the ring's current content as a lossy
/// string (output ships as UTF-8 strings today — same wire shape) plus the
/// cumulative byte seq of the last byte in it. Live chunks with
/// `seq <= end_seq` are already inside `text` and must be dropped on replay.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub text: String,
    pub end_seq: u64,
}

struct Ring {
    /// Raw bytes, oldest first. Overflow is drained from the FRONT, so the cut
    /// may land mid-UTF-8-char — `snapshot()` trims to the next char boundary.
    buf: VecDeque<u8>,
    cap: usize,
    /// The batcher's cumulative emitted-byte seq of the LAST appended chunk.
    end_seq: u64,
}

impl Ring {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap,
            end_seq: 0,
        }
    }

    fn append(&mut self, chunk: &str, seq: u64) {
        self.buf.extend(chunk.as_bytes());
        if self.buf.len() > self.cap {
            let overflow = self.buf.len() - self.cap;
            self.buf.drain(..overflow);
        }
        self.end_seq = seq;
    }

    fn snapshot(&self) -> Snapshot {
        // The front may sit inside a multi-byte char after an overflow drain —
        // skip leading UTF-8 continuation bytes (0b10xxxxxx) so the replay
        // never starts with U+FFFD garbage.
        let (a, b) = self.buf.as_slices();
        let mut bytes: Vec<u8> = Vec::with_capacity(a.len() + b.len());
        bytes.extend_from_slice(a);
        bytes.extend_from_slice(b);
        let start = bytes
            .iter()
            .position(|&x| x & 0xC0 != 0x80)
            .unwrap_or(bytes.len());
        Snapshot {
            text: String::from_utf8_lossy(&bytes[start..]).into_owned(),
            end_seq: self.end_seq,
        }
    }
}

/// All agents' rings, keyed by agent id. One process-wide instance (like the
/// output mux) — the reader/batcher pipeline appends, `runtime_snapshot` reads.
#[derive(Default)]
pub struct Rings {
    map: Mutex<HashMap<String, Ring>>,
}

impl Rings {
    /// Append one batcher flush for `id` (creates the ring on first output).
    pub fn append(&self, id: &str, chunk: &str, seq: u64) {
        let mut m = self.map.lock().unwrap();
        m.entry(id.to_string())
            .or_insert_with(|| Ring::new(RING_CAP))
            .append(chunk, seq);
    }

    /// Current snapshot for `id` (None = the agent never produced output).
    pub fn snapshot(&self, id: &str) -> Option<Snapshot> {
        self.map.lock().unwrap().get(id).map(Ring::snapshot)
    }

    /// Clear content + seq for `id` but keep the ring registered. Called at
    /// (re)launch: the new run's batcher seq restarts at 0, so stale content
    /// tagged with the OLD run's seqs would corrupt snapshot dedup.
    pub fn reset(&self, id: &str) {
        if let Some(r) = self.map.lock().unwrap().get_mut(id) {
            r.buf.clear();
            r.end_seq = 0;
        }
    }

    /// Drop the ring entirely (agent removed).
    pub fn remove(&self, id: &str) {
        self.map.lock().unwrap().remove(id);
    }
}

static RINGS: OnceLock<Rings> = OnceLock::new();

/// The process-wide ring registry.
pub fn rings() -> &'static Rings {
    RINGS.get_or_init(Rings::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_snapshot_roundtrip_with_seq() {
        let mut r = Ring::new(64);
        r.append("hello ", 6);
        r.append("world", 11);
        let s = r.snapshot();
        assert_eq!(s.text, "hello world");
        assert_eq!(s.end_seq, 11, "endSeq = last appended chunk's seq");
    }

    #[test]
    fn overflow_drops_oldest_bytes_and_stays_bounded() {
        let mut r = Ring::new(8);
        r.append("0123456789", 10); // 10 > 8 → the front 2 bytes drop
        assert_eq!(r.buf.len(), 8, "ring never exceeds its cap");
        assert_eq!(r.snapshot().text, "23456789");
        r.append("ab", 12);
        assert_eq!(r.snapshot().text, "456789ab", "wrap keeps the newest tail");
        assert_eq!(r.snapshot().end_seq, 12);
    }

    #[test]
    fn snapshot_trims_a_char_split_by_overflow() {
        let mut r = Ring::new(4);
        // "aé" = [0x61, 0xC3, 0xA9]; append twice (6 bytes) → front 2 drop,
        // leaving [0xA9, 0x61, 0xC3, 0xA9] — a leading continuation byte.
        r.append("aé", 3);
        r.append("aé", 6);
        let s = r.snapshot();
        assert_eq!(s.text, "aé", "leading partial char is trimmed, not U+FFFD'd");
    }

    #[test]
    fn registry_reset_clears_content_and_seq_for_a_new_run() {
        let rings = Rings::default();
        rings.append("a", "old run", 7);
        rings.reset("a");
        let s = rings.snapshot("a").expect("ring stays registered after reset");
        assert_eq!(s.text, "");
        assert_eq!(s.end_seq, 0, "new run's seq restarts at 0 — dedup must too");
        rings.append("a", "new", 3);
        assert_eq!(rings.snapshot("a").unwrap().end_seq, 3);
    }

    #[test]
    fn registry_remove_and_unknown_agent() {
        let rings = Rings::default();
        assert_eq!(rings.snapshot("ghost"), None, "no output → no snapshot");
        rings.append("a", "x", 1);
        rings.remove("a");
        assert_eq!(rings.snapshot("a"), None);
    }

    #[test]
    fn snapshot_serializes_camel_case() {
        let s = Snapshot {
            text: "t".into(),
            end_seq: 9,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({ "text": "t", "endSeq": 9 })
        );
    }
}
