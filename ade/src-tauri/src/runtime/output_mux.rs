//! Global PTY-output multiplexer: ONE `agent://output` emit per ~16ms frame
//! TOTAL, regardless of agent count — routed by the A0.2 INTEREST SUBSCRIPTION.
//!
//! Why one drain thread: each per-agent batcher used to `app.emit` its own
//! flushes — on Windows every emit is a PostMessageW + ExecuteScript on the
//! Win32 UI thread (the same thread Tauri invoke crosses twice), so 5 streaming
//! agents ≈ 625 UI-thread jobs/s and every command gained 100-350ms. The
//! per-agent batchers push their UTF-8-safe chunks here, and a single drain
//! thread coalesces ALL subscribed agents' pending output into one array
//! payload per frame. Idle costs nothing: the drain thread parks on a condvar.
//!
//! Interest modes (A0.2 — supersedes the old `set_focus` single-agent cadence):
//! - `Stream`: full output on the per-frame path (a visible terminal pane).
//! - `Digest`: the raw stream is FOLDED backend-side into the last few rendered
//!   lines and shipped as a tiny `agent://digest` payload at 1Hz (overview
//!   mini-terminals). No raw bytes reach the renderer.
//! - `None` (or absent): DO NOT EMIT. This must never mean do-not-READ — the
//!   reader + batcher threads keep draining every PTY regardless of mode
//!   (a full kernel PTY buffer would BLOCK the agent process itself). Bytes
//!   land in the A1.2 scrollback ring only (bounded); `push` for a `None`
//!   agent stores NOTHING here, so the mux can never accumulate unbounded
//!   pending Strings for unwatched agents.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::digest;

/// Digest payload size. Why 5: the overview mini-terminal renders a 5-line
/// preview — shipping more would be paying for lines nobody can see.
pub const DIGEST_LINES: usize = 5;

/// Digest push cadence. Why 1Hz: mini-previews are glanceable status, not
/// terminals — 1 update/s is imperceptibly stale there and caps the digest
/// channel at (agents × ~300B)/s no matter how hard an agent floods.
pub const DIGEST_PERIOD: Duration = Duration::from_secs(1);

/// A0.2 interest mode for one agent. Absent from the subscription = `None`.
/// (Ord is for deterministic ordering in diffs/tests only.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Mode {
    Stream,
    Digest,
    #[default]
    None,
}

impl Mode {
    /// Wire string → mode. Unknown strings degrade to `None` (emit nothing) —
    /// the safe direction: output stops shipping rather than flooding.
    pub fn parse(s: &str) -> Self {
        match s {
            "stream" => Mode::Stream,
            "digest" => Mode::Digest,
            _ => Mode::None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Stream => "stream",
            Mode::Digest => "digest",
            Mode::None => "none",
        }
    }
}

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

/// One agent's digest inside an `agent://digest` payload: the last
/// [`DIGEST_LINES`] rendered lines plus the seq of the last folded chunk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DigestEntry {
    pub id: String,
    pub lines: Vec<String>,
    pub seq: u64,
}

/// One agent's interest change from a `set_interest` diff — recorded for the
/// A0.7 telemetry correlate ("emit volume is uninterpretable without knowing
/// the subscription state").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterestTransition {
    pub id: String,
    pub from: Mode,
    pub to: Mode,
}

#[derive(Default)]
struct DigestState {
    /// Rolling fold buffer (see `digest::fold_lines`), cap `digest::FOLD_CAP`.
    lines: Vec<String>,
    seq: u64,
    /// New output folded since the last digest emit.
    dirty: bool,
}

#[derive(Default)]
struct State {
    /// STREAM agents only: agent id → (coalesced chunk, last seq). BTreeMap so
    /// a frame's entry order is deterministic.
    pending: BTreeMap<String, (String, u64)>,
    /// The current interest set. Only Stream/Digest are stored — `None` is
    /// represented by absence, so the map is bounded by what is on screen.
    interest: HashMap<String, Mode>,
    /// DIGEST agents only: the backend-side line fold.
    digests: BTreeMap<String, DigestState>,
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
    /// Wakes the output drain thread (stream pushes, shutdown, take()).
    cv: Condvar,
    /// Wakes the digest thread (digest pushes, shutdown). Separate condvar so
    /// a digest push can't be swallowed by the drain thread's notify_one.
    digest_cv: Condvar,
}

impl OutputMux {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a batcher flush for `id`, routed by its interest mode:
    /// Stream → pending (drained per frame); Digest → folded into the digest
    /// lines (shipped at 1Hz); None → DROPPED here (the scrollback ring holds
    /// the bytes — the mux must never grow for unwatched agents).
    pub fn push(&self, id: &str, chunk: String, seq: u64) {
        let mut st = self.state.lock().unwrap();
        match st.interest.get(id).copied().unwrap_or_default() {
            Mode::Stream => {
                let e = st.pending.entry(id.to_string()).or_default();
                e.0.push_str(&chunk);
                e.1 = seq;
                drop(st);
                self.cv.notify_all();
            }
            Mode::Digest => {
                let d = st.digests.entry(id.to_string()).or_default();
                digest::fold_lines(&mut d.lines, &chunk, digest::FOLD_CAP);
                d.seq = seq;
                d.dirty = true;
                drop(st);
                self.digest_cv.notify_all();
            }
            Mode::None => {} // do-not-EMIT (never do-not-read — caller keeps reading)
        }
    }

    /// Replace the whole interest set (the backend diffs against the current
    /// one). Entries with `Mode::None` are equivalent to absence. Returns the
    /// transitions (old → new per changed agent) for telemetry.
    ///
    /// Downgrades drop mux-side state: leaving `Stream` discards the agent's
    /// pending chunk (the ring is the recovery source — the renderer replays a
    /// snapshot on resubscribe and dedups by seq), leaving `Digest` drops its
    /// fold buffer.
    pub fn set_interest(&self, entries: Vec<(String, Mode)>) -> Vec<InterestTransition> {
        let new_map: HashMap<String, Mode> = entries
            .into_iter()
            .filter(|(_, m)| *m != Mode::None)
            .collect();
        let mut transitions = Vec::new();
        {
            let mut st = self.state.lock().unwrap();
            let old_map = std::mem::take(&mut st.interest);
            for (id, old) in &old_map {
                let new = new_map.get(id).copied().unwrap_or_default();
                if new != *old {
                    transitions.push(InterestTransition {
                        id: id.clone(),
                        from: *old,
                        to: new,
                    });
                }
            }
            for (id, new) in &new_map {
                if !old_map.contains_key(id) {
                    transitions.push(InterestTransition {
                        id: id.clone(),
                        from: Mode::None,
                        to: *new,
                    });
                }
            }
            for t in &transitions {
                if t.from == Mode::Stream {
                    st.pending.remove(&t.id);
                }
                if t.from == Mode::Digest {
                    st.digests.remove(&t.id);
                }
            }
            st.interest = new_map;
        }
        // Wake both threads: a newly streaming/digesting agent's next push
        // routes correctly either way, but shutdown/park bookkeeping is cheap
        // to refresh and a pending-drop may let the drain thread re-park.
        self.cv.notify_all();
        self.digest_cv.notify_all();
        transitions
    }

    /// Current interest mode for `id` (`Mode::None` when absent from the set).
    /// Read-only snapshot — used by the watcher's A2.2 scan-detail decision.
    pub fn interest_of(&self, id: &str) -> Mode {
        self.state
            .lock()
            .unwrap()
            .interest
            .get(id)
            .copied()
            .unwrap_or_default()
    }

    /// Remove and return `id`'s pending stream output, if any. The exit path
    /// uses this to force-drain an agent ahead of its `agent://exit` so no
    /// queued output event lands after the exit; it doubles as the stop/remove
    /// cleanup. Digest state is dropped too (a relaunch starts a fresh fold —
    /// its seq restarts at 0). The interest entry is KEPT: a restart of a
    /// still-visible agent must keep streaming without waiting for the next
    /// frontend recompute.
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
        st.digests.remove(id);
        st.pending.remove(id).map(|(chunk, seq)| OutputEntry {
            id: id.to_string(),
            chunk,
            seq,
        })
    }

    /// End the drain + digest loops: each final-drains whatever is pending,
    /// emits it, and returns. (Tests join on this; the app never calls it —
    /// the threads live for the process.)
    pub fn shutdown(&self) {
        self.state.lock().unwrap().shutdown = true;
        self.cv.notify_all();
        self.digest_cv.notify_all();
    }

    /// Pull EVERYTHING pending (all pending entries are stream-mode by
    /// construction — `push` stores nothing for other modes).
    fn drain_all(st: &mut State) -> Vec<OutputEntry> {
        std::mem::take(&mut st.pending)
            .into_iter()
            .map(|(id, (chunk, seq))| OutputEntry { id, chunk, seq })
            .collect()
    }

    /// The single drain thread's body. Parks on the condvar while nothing is
    /// pending (ZERO idle wakeups). On wake it emits — immediately after an
    /// idle stretch (a subscribed agent's first output, and typing echo, are
    /// never delayed), otherwise paced so consecutive emits are ≥ `frame`
    /// apart, which caps the emit rate at one per frame no matter how many
    /// agents flood. Each emit carries every pending STREAM agent coalesced.
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
                    let batch = Self::drain_all(&mut st);
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
            // Take everything pending. Open the draining window BEFORE
            // unlocking, emit with the lock released (pushers stay
            // non-blocking), then close it and wake any take() parked on it.
            let batch = {
                let mut st = self.state.lock().unwrap();
                if st.shutdown {
                    continue; // outer loop re-parks → final drain
                }
                let b = Self::drain_all(&mut st);
                if b.is_empty() {
                    continue; // take()/set_interest raced it away — re-park
                }
                st.draining = true;
                b
            };
            emit(batch);
            last_emit = Some(Instant::now());
            self.state.lock().unwrap().draining = false;
            self.cv.notify_all();
        }
    }

    /// The digest thread's body: parks until some digest-mode agent has new
    /// folded output, emits one `agent://digest` array payload (dirty agents
    /// only, [`DIGEST_LINES`] tail lines each), then sleeps `period` — the 1Hz
    /// cadence. A first digest after an idle stretch ships immediately.
    pub fn digest_loop(&self, period: Duration, mut emit: impl FnMut(Vec<DigestEntry>)) {
        loop {
            let batch = {
                let mut st = self.state.lock().unwrap();
                loop {
                    let dirty: Vec<DigestEntry> = st
                        .digests
                        .iter_mut()
                        .filter(|(_, d)| d.dirty)
                        .map(|(id, d)| {
                            d.dirty = false;
                            DigestEntry {
                                id: id.clone(),
                                lines: digest::tail_lines(&d.lines, DIGEST_LINES),
                                seq: d.seq,
                            }
                        })
                        .collect();
                    if !dirty.is_empty() || st.shutdown {
                        break dirty;
                    }
                    st = self.digest_cv.wait(st).unwrap();
                }
            };
            if !batch.is_empty() {
                emit(batch);
            }
            if self.state.lock().unwrap().shutdown {
                return;
            }
            std::thread::sleep(period);
        }
    }

    /// Test-only visibility: bytes currently pending for `id` (must stay 0 for
    /// non-stream agents — the "none must not accumulate" budget).
    #[cfg(test)]
    pub(crate) fn pending_bytes(&self, id: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .pending
            .get(id)
            .map(|(c, _)| c.len())
            .unwrap_or(0)
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

    fn stream(mux: &OutputMux, ids: &[&str]) {
        mux.set_interest(ids.iter().map(|i| (i.to_string(), Mode::Stream)).collect());
    }

    // THE A0.2 budget: an unsubscribed (= none) agent ships NOTHING to the
    // renderer AND holds no pending memory in the mux — its bytes live in the
    // bounded scrollback ring only.
    #[test]
    fn unsubscribed_agent_emits_nothing_and_accumulates_nothing() {
        let (mux, emitted, h) = run(Duration::from_millis(5));
        for i in 0..50u64 {
            mux.push("hidden", format!("x{i};"), i + 1);
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            emitted.lock().unwrap().is_empty(),
            "none-mode renderer bytes/sec must be 0"
        );
        assert_eq!(
            mux.pending_bytes("hidden"),
            0,
            "none-mode must never grow the mux's pending map"
        );
        mux.shutdown();
        h.join().unwrap();
        assert!(emitted.lock().unwrap().is_empty(), "not even the final drain");
    }

    #[test]
    fn multi_agent_stream_pushes_coalesce_into_one_emit_per_frame() {
        let mux = Arc::new(OutputMux::new());
        stream(&mux, &["a", "b"]);
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
        stream(&mux, &["a"]);
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
    fn stream_flood_is_paced_to_at_most_one_emit_per_frame_and_loses_nothing() {
        let frame = Duration::from_millis(40);
        let (mux, emitted, h) = run(frame);
        stream(&mux, &["a"]);
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
        stream(&mux, &["a", "b"]);
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
        stream(&mux, &["a"]);
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
        stream(&mux, &["a"]);
        mux.push("a", "last words".into(), 10);
        mux.shutdown();
        h.join().unwrap();
        let log = emitted.lock().unwrap();
        let all: Vec<&OutputEntry> = log.iter().flatten().collect();
        assert_eq!(all.len(), 1);
        assert_eq!(*all[0], entry("a", "last words", 10), "nothing buffered is lost on shutdown");
    }

    // ---- interest diffing ---------------------------------------------------

    #[test]
    fn set_interest_diffs_against_the_current_set() {
        let mux = OutputMux::new();
        let t1 = mux.set_interest(vec![
            ("a".into(), Mode::Stream),
            ("b".into(), Mode::Digest),
        ]);
        let mut ids: Vec<(String, Mode, Mode)> =
            t1.iter().map(|t| (t.id.clone(), t.from, t.to)).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                ("a".into(), Mode::None, Mode::Stream),
                ("b".into(), Mode::None, Mode::Digest),
            ]
        );
        // b: digest → stream; a absent → none; c appears as digest.
        let t2 = mux.set_interest(vec![
            ("b".into(), Mode::Stream),
            ("c".into(), Mode::Digest),
        ]);
        let mut ids: Vec<(String, Mode, Mode)> =
            t2.iter().map(|t| (t.id.clone(), t.from, t.to)).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                ("a".into(), Mode::Stream, Mode::None),
                ("b".into(), Mode::Digest, Mode::Stream),
                ("c".into(), Mode::None, Mode::Digest),
            ]
        );
        // Unchanged set → no transitions (the frontend recomputes often;
        // steady state must be free).
        assert!(mux
            .set_interest(vec![
                ("b".into(), Mode::Stream),
                ("c".into(), Mode::Digest),
            ])
            .is_empty());
        // Explicit none behaves exactly like absence.
        let t3 = mux.set_interest(vec![
            ("b".into(), Mode::Stream),
            ("c".into(), Mode::None),
        ]);
        assert_eq!(
            t3,
            vec![InterestTransition {
                id: "c".into(),
                from: Mode::Digest,
                to: Mode::None
            }]
        );
    }

    #[test]
    fn downgrade_from_stream_drops_pending_so_nothing_leaks_or_ships_late() {
        let (mux, emitted, h) = run(Duration::from_secs(10)); // frame never paces
        stream(&mux, &["a"]);
        // Push while the drain thread may not have woken yet, then downgrade.
        mux.push("a", "held".into(), 4);
        mux.set_interest(vec![]); // a → none
        assert_eq!(mux.pending_bytes("a"), 0, "downgrade discards pending (ring recovers it)");
        mux.push("a", "more".into(), 8); // now none — dropped at the door
        mux.shutdown();
        h.join().unwrap();
        let log = emitted.lock().unwrap();
        // Depending on thread timing the pre-downgrade "held" frame MAY have
        // been drained-and-emitted before set_interest ran; what must never
        // happen is the post-downgrade chunk shipping.
        assert!(
            log.iter().flatten().all(|e| e.chunk != "more"),
            "post-downgrade output must not ship: {log:?}"
        );
    }

    // ---- digest mode --------------------------------------------------------

    /// Run digest_loop on a thread with a fast period (tests can't wait 1s).
    fn run_digest(
        period: Duration,
    ) -> (
        Arc<OutputMux>,
        Arc<Mutex<Vec<Vec<DigestEntry>>>>,
        std::thread::JoinHandle<()>,
    ) {
        let mux = Arc::new(OutputMux::new());
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.digest_loop(period, move |batch| e.lock().unwrap().push(batch));
        });
        (mux, emitted, h)
    }

    #[test]
    fn digest_agent_ships_last_lines_not_raw_output() {
        let (mux, digests, dh) = run_digest(Duration::from_millis(10));
        let out_emits = Arc::new(Mutex::new(Vec::new()));
        let (m2, oe) = (mux.clone(), out_emits.clone());
        let oh = std::thread::spawn(move || {
            m2.drain_loop(Duration::from_millis(1), move |b| oe.lock().unwrap().push(b));
        });
        mux.set_interest(vec![("d".into(), Mode::Digest)]);
        mux.push("d", "one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n".into(), 30);
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = digests.lock().unwrap();
            assert!(!log.is_empty(), "digest push must emit");
            let d = &log[0][0];
            assert_eq!(d.id, "d");
            assert_eq!(
                d.lines,
                vec!["two", "three", "four", "five", "six"],
                "last {DIGEST_LINES} rendered lines"
            );
            assert_eq!(d.seq, 30);
        }
        assert!(
            out_emits.lock().unwrap().is_empty(),
            "digest agents ship NO raw output frames"
        );
        assert_eq!(mux.pending_bytes("d"), 0, "digest mode holds no pending chunk");
        mux.shutdown();
        dh.join().unwrap();
        oh.join().unwrap();
    }

    #[test]
    fn digest_emits_are_paced_and_dirty_only() {
        let period = Duration::from_millis(50);
        let (mux, digests, h) = run_digest(period);
        mux.set_interest(vec![("d".into(), Mode::Digest)]);
        let start = Instant::now();
        for i in 0..40u64 {
            mux.push("d", format!("line{i}\n"), i + 1);
            std::thread::sleep(Duration::from_millis(4)); // ~160ms flood
        }
        // quiet stretch: no new folds → NO further digest emits
        std::thread::sleep(Duration::from_millis(150));
        let emits_after_quiet = digests.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(
            digests.lock().unwrap().len(),
            emits_after_quiet,
            "clean (non-dirty) digests must not re-emit"
        );
        // Pacing bound, measured-elapsed style (see stream flood test).
        let elapsed = start.elapsed();
        let max_emits = (elapsed.as_millis() / period.as_millis()) as usize + 4;
        assert!(
            emits_after_quiet <= max_emits,
            "{}ms / {}ms period → ≤ {max_emits} digest emits, got {emits_after_quiet}",
            elapsed.as_millis(),
            period.as_millis(),
        );
        // Lossless-at-the-tail: the LAST digest shows the newest lines.
        let log = digests.lock().unwrap();
        let last = log.last().unwrap().last().unwrap();
        assert_eq!(last.lines.last().unwrap(), "line39");
        drop(log);
        mux.shutdown();
        h.join().unwrap();
    }

    #[test]
    fn take_clears_digest_state_for_a_fresh_relaunch() {
        let mux = OutputMux::new();
        mux.set_interest(vec![("d".into(), Mode::Digest)]);
        mux.push("d", "old run\n".into(), 8);
        assert_eq!(mux.take("d"), None, "digest agents have no pending stream chunk");
        // A new run's first push folds into an EMPTY buffer.
        mux.push("d", "new run\n".into(), 8);
        let st = mux.state.lock().unwrap();
        assert_eq!(
            digest::tail_lines(&st.digests.get("d").unwrap().lines, DIGEST_LINES),
            vec!["new run"]
        );
    }

    // Locks the wire shapes the frontend types against.
    #[test]
    fn payloads_serialize_to_the_event_shapes() {
        let frame = vec![entry("a", "x", 3), entry("b", "y", 1)];
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!([
                { "id": "a", "chunk": "x", "seq": 3 },
                { "id": "b", "chunk": "y", "seq": 1 }
            ])
        );
        let d = vec![DigestEntry {
            id: "a".into(),
            lines: vec!["l1".into(), "l2".into()],
            seq: 12,
        }];
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!([{ "id": "a", "lines": ["l1", "l2"], "seq": 12 }])
        );
    }
}
