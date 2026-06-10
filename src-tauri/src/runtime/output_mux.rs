//! Global PTY-output multiplexer: ONE `agent://output` emit per ~16ms frame
//! TOTAL, regardless of agent count.
//!
//! Why: each per-agent batcher used to `app.emit` its own flushes — on Windows
//! every emit is a PostMessageW + ExecuteScript on the Win32 UI thread (the
//! same thread Tauri invoke crosses twice), so 5 streaming agents ≈ 625
//! UI-thread jobs/s and every command gained 100-350ms. Now the per-agent
//! batchers push their UTF-8-safe chunks here, and a single drain thread
//! coalesces ALL agents' pending output into one array-payload emit per frame.
//! Idle costs nothing: the drain thread parks on a condvar — zero wakeups,
//! zero emits — until the next push.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// One agent's coalesced output inside a multiplexed frame. The event payload
/// is an array of these — one entry per agent that produced output during the
/// frame. `seq` is that agent's cumulative emitted-byte count (monotonic per
/// agent; the snapshot-dedup foundation), carried over from its batcher.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutputEntry {
    pub id: String,
    pub chunk: String,
    pub seq: u64,
}

#[derive(Default)]
struct State {
    /// agent id → (coalesced chunk, last seq). BTreeMap so a frame's entry
    /// order is deterministic.
    pending: BTreeMap<String, (String, u64)>,
    /// True while the drain thread has taken a frame out of `pending` but its
    /// emit has not finished — the drained-but-unemitted window. `take()`
    /// waits this out so the exit path can't emit `agent://exit` while the
    /// agent's tail frame is still in flight.
    draining: bool,
    shutdown: bool,
}

#[derive(Default)]
pub struct OutputMux {
    state: Mutex<State>,
    cv: Condvar,
}

impl OutputMux {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a batcher flush for `id`: appends to the agent's pending chunk and
    /// advances its seq (batcher seqs are cumulative, so "last wins" keeps it
    /// monotonic). Wakes the drain thread if it is parked.
    pub fn push(&self, id: &str, chunk: String, seq: u64) {
        {
            let mut st = self.state.lock().unwrap();
            let e = st.pending.entry(id.to_string()).or_default();
            e.0.push_str(&chunk);
            e.1 = seq;
        }
        self.cv.notify_one();
    }

    /// Remove and return `id`'s pending output, if any. The exit path uses
    /// this to force-drain an agent ahead of its `agent://exit` so no queued
    /// output event lands after the exit; it doubles as the stop/remove
    /// cleanup (the entry is gone either way).
    ///
    /// Waits out any in-flight drain emit first: the drain thread emits with
    /// the lock RELEASED (pushers must never block on UI-thread emit latency),
    /// so without the wait this could return while the agent's tail frame is
    /// drained-but-unemitted — and `agent://exit` would beat the tail.
    pub fn take(&self, id: &str) -> Option<OutputEntry> {
        let mut st = self.state.lock().unwrap();
        while st.draining {
            st = self.cv.wait(st).unwrap();
        }
        st.pending.remove(id).map(|(chunk, seq)| OutputEntry {
            id: id.to_string(),
            chunk,
            seq,
        })
    }

    /// End the drain loop: it final-drains whatever is pending, emits it, and
    /// returns. (Tests join on this; the app never calls it — the drain thread
    /// lives for the process.)
    pub fn shutdown(&self) {
        self.state.lock().unwrap().shutdown = true;
        self.cv.notify_all();
    }

    fn drain(st: &mut State) -> Vec<OutputEntry> {
        std::mem::take(&mut st.pending)
            .into_iter()
            .map(|(id, (chunk, seq))| OutputEntry { id, chunk, seq })
            .collect()
    }

    /// The single drain thread's body. Parks on the condvar while nothing is
    /// pending (ZERO idle wakeups). On wake it emits — immediately after an
    /// idle stretch (typing echo stays snappy), otherwise paced so consecutive
    /// emits are ≥ `frame` apart, which caps the emit rate at one per frame no
    /// matter how many agents flood. Each emit carries EVERY pending agent.
    pub fn drain_loop(&self, frame: Duration, mut emit: impl FnMut(Vec<OutputEntry>)) {
        let mut last_emit: Option<Instant> = None;
        loop {
            // Park until there is something to do.
            {
                let mut st = self.state.lock().unwrap();
                while st.pending.is_empty() && !st.shutdown {
                    st = self.cv.wait(st).unwrap();
                }
                if st.shutdown {
                    let batch = Self::drain(&mut st);
                    if !batch.is_empty() {
                        st.draining = true;
                        drop(st);
                        emit(batch);
                        self.state.lock().unwrap().draining = false;
                        self.cv.notify_all();
                    }
                    return;
                }
            }
            // Frame pacing: pushes landing within the remainder of the frame
            // coalesce into this batch instead of becoming their own emits.
            if let Some(t) = last_emit {
                let since = t.elapsed();
                if since < frame {
                    std::thread::sleep(frame - since);
                }
            }
            // Open the draining window BEFORE unlocking, emit with the lock
            // released (pushers stay non-blocking), then close it and wake any
            // take() parked on the window.
            let batch = {
                let mut st = self.state.lock().unwrap();
                let b = Self::drain(&mut st);
                if !b.is_empty() {
                    st.draining = true;
                }
                b
            };
            // Empty only if take() raced everything away — nothing to emit.
            if !batch.is_empty() {
                emit(batch);
                last_emit = Some(Instant::now());
                self.state.lock().unwrap().draining = false;
                self.cv.notify_all();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Run drain_loop on a thread; returns (mux, emitted frame log, handle).
    /// Each logged frame is the full Vec<OutputEntry> of one emit.
    fn run(
        frame: Duration,
    ) -> (
        Arc<OutputMux>,
        Arc<Mutex<Vec<Vec<OutputEntry>>>>,
        std::thread::JoinHandle<()>,
    ) {
        let mux = Arc::new(OutputMux::new());
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(frame, move |batch| e.lock().unwrap().push(batch));
        });
        (mux, emitted, h)
    }

    fn entry(id: &str, chunk: &str, seq: u64) -> OutputEntry {
        OutputEntry {
            id: id.into(),
            chunk: chunk.into(),
            seq,
        }
    }

    #[test]
    fn multi_agent_pushes_coalesce_into_one_emit_per_frame() {
        let mux = Arc::new(OutputMux::new());
        // Queue BEFORE the drain thread runs so everything is one frame:
        // two flushes for "a" (must coalesce, seq = last) and one for "b".
        mux.push("a", "a1".into(), 2);
        mux.push("a", "a2".into(), 4);
        mux.push("b", "b1".into(), 2);
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(Duration::from_millis(10), move |batch| {
                e.lock().unwrap().push(batch)
            });
        });
        std::thread::sleep(Duration::from_millis(150));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log.len(), 1, "one frame → ONE emit for ALL agents: {log:?}");
            assert_eq!(log[0], vec![entry("a", "a1a2", 4), entry("b", "b1", 2)]);
        }
        mux.shutdown();
        h.join().unwrap();
    }

    #[test]
    fn idle_mux_emits_nothing_after_draining() {
        let (mux, emitted, h) = run(Duration::from_millis(10));
        mux.push("a", "x".into(), 1);
        std::thread::sleep(Duration::from_millis(150)); // long idle stretch
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "no pushes → parked on the condvar, no further emits"
        );
        mux.shutdown();
        h.join().unwrap();
        assert_eq!(emitted.lock().unwrap().len(), 1, "shutdown adds nothing when empty");
    }

    #[test]
    fn flood_is_paced_to_at_most_one_emit_per_frame_and_loses_nothing() {
        let frame = Duration::from_millis(40);
        let (mux, emitted, h) = run(frame);
        let start = Instant::now();
        let mut sent = String::new();
        for i in 0..50 {
            let piece = format!("c{i};");
            sent.push_str(&piece);
            mux.push("a", piece, (i + 1) as u64);
            std::thread::sleep(Duration::from_millis(4)); // ~200ms flood, 10x faster than the frame
        }
        mux.shutdown();
        h.join().unwrap();
        // Bound the emit count from MEASURED elapsed, not the nominal ~200ms:
        // under full-suite parallel load the 4ms sleeps stretch (timer
        // granularity + thread starvation), so more frames legitimately pass
        // and pacing legitimately allows more emits. Consecutive paced emits
        // are >= frame apart, so the hard cap is floor(elapsed/frame)
        // + 1 (immediate first emit) + 1 (unpaced shutdown final drain);
        // +2 extra slack.
        let elapsed = start.elapsed();
        let max_emits = (elapsed.as_millis() / frame.as_millis()) as usize + 4;
        let log = emitted.lock().unwrap();
        assert!(
            log.len() <= max_emits,
            "{}ms elapsed / {}ms frame → ≤ {} emits, got {}",
            elapsed.as_millis(),
            frame.as_millis(),
            max_emits,
            log.len()
        );
        assert!(log.len() >= 2, "a flood spans multiple frames: {}", log.len());
        let joined: String = log.iter().flatten().map(|e| e.chunk.as_str()).collect();
        assert_eq!(joined, sent, "coalescing must preserve every byte, in order");
        let seqs: Vec<u64> = log.iter().flatten().map(|e| e.seq).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "per-agent seq stays monotonic: {seqs:?}");
        assert_eq!(*seqs.last().unwrap(), 50, "last seq = the agent's final batcher seq");
    }

    #[test]
    fn take_force_drains_one_agent_without_touching_others() {
        let mux = OutputMux::new();
        mux.push("a", "tail".into(), 7);
        mux.push("a", "!".into(), 8);
        mux.push("b", "keep".into(), 3);
        assert_eq!(
            mux.take("a"),
            Some(entry("a", "tail!", 8)),
            "take returns the agent's coalesced pending output"
        );
        assert_eq!(mux.take("a"), None, "second take finds nothing — exit emits once");
        assert_eq!(mux.take("b"), Some(entry("b", "keep", 3)), "other agents untouched");
    }

    /// The exit-path race: drain_loop takes pending under lock, UNLOCKS, then
    /// emits. If the exit path's take() lands inside that drained-but-unemitted
    /// window and returns immediately, `agent://exit` goes out while the tail
    /// frame is still in flight — output after exit. take() must WAIT for the
    /// in-flight emit to finish.
    #[test]
    fn take_waits_for_in_flight_drain_emit_so_no_chunk_lands_after_it() {
        let mux = Arc::new(OutputMux::new());
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (m, l) = (mux.clone(), log.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(Duration::from_millis(1), move |batch| {
                let _ = started_tx.send(());
                // Slow UI-thread emit: the tail frame is in flight, undelivered.
                std::thread::sleep(Duration::from_millis(100));
                let mut l = l.lock().unwrap();
                for e in batch {
                    l.push(format!("delivered:{}", e.id));
                }
            });
        });
        mux.push("a", "tail".into(), 1);
        // Once the emit closure has started, "a" is already drained out of
        // pending (draining window open) but NOT yet delivered.
        started_rx.recv().unwrap();
        let taken = mux.take("a"); // exit path: must block until the emit lands
        log.lock().unwrap().push("take_returned".into());
        assert_eq!(taken, None, "tail was in the drained frame — nothing left over");
        assert_eq!(
            *log.lock().unwrap(),
            vec!["delivered:a".to_string(), "take_returned".to_string()],
            "no chunk may be delivered after take() returned — exit would beat the tail"
        );
        mux.shutdown();
        h.join().unwrap();
    }

    #[test]
    fn shutdown_final_drains_pending_like_the_batcher_does() {
        let (mux, emitted, h) = run(Duration::from_secs(10)); // frame never paces
        mux.push("a", "last words".into(), 10);
        mux.shutdown();
        h.join().unwrap();
        let log = emitted.lock().unwrap();
        let all: Vec<&OutputEntry> = log.iter().flatten().collect();
        assert_eq!(all.len(), 1);
        assert_eq!(*all[0], entry("a", "last words", 10), "nothing buffered is lost on shutdown");
    }

    // Locks the wire shape the frontend types against: an ARRAY of
    // {id, chunk, seq} objects.
    #[test]
    fn frame_serializes_to_the_event_payload_shape() {
        let frame = vec![entry("a", "x", 3), entry("b", "y", 1)];
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!([
                { "id": "a", "chunk": "x", "seq": 3 },
                { "id": "b", "chunk": "y", "seq": 1 }
            ])
        );
    }
}
