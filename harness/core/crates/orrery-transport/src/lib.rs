//! Framing, seq authority, the replay ring, the per-client coalescer and every listener.
//!
//! # The kernel is a server, even in-process
//!
//! Three listeners in phase 1 — an in-process channel, a named pipe or UDS, and
//! AG-UI's native HTTP+SSE — over one frame set. A CLI that skips serialisation
//! still goes through the same request and event types as a browser tab, so
//! there is no second code path to drift. That is also why ratatui and Ink can
//! sit on one session at the same time.
//!
//! # The three invariants
//!
//! - **`seq` is assigned once, per session, at the differ's output** — never per
//!   connection. See [`seq::SeqAuthority`].
//! - **One coalescer per connected client.** Two clients at different speeds
//!   must not share one. See [`coalesce::Coalescer`].
//! - **Rendering never applies backpressure to the agent loop.** A stalled
//!   client degrades its own view, not the run: it is marked lagged and told to
//!   re-attach, and [`Hub::publish`] never waits for anybody.
//!
//! Implementation plan: `harness/docs/plans/08-protocol-transport.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod coalesce;
pub mod error;
pub mod frame;
pub mod listener;
pub mod replay;
pub mod seq;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use orrery_agui::{AguiEvent, Encoder, Frame};
use orrery_proto::{ConsentAnswerKind, Event, PromptId};
use tokio::sync::mpsc;

pub use coalesce::Coalescer;
pub use error::{ReplayError, TransportError};
pub use frame::{Codec, WireFormat};
pub use replay::ReplayRing;
pub use seq::SeqAuthority;

/// How many frames a connected client may fall behind before its view is
/// abandoned.
///
/// Rendering never applies backpressure to the agent loop, so a client that
/// stops reading cannot be allowed to hold the session's memory hostage either.
/// Past this it is marked lagged and re-attaches with `since`.
pub const CLIENT_BACKLOG_LIMIT: usize = 4096;

/// How many frames the replay ring holds by default.
pub const REPLAY_RING_FRAMES: usize = 8192;

/// A connected client's handle, for logs and for `per_client` assertions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(pub u64);

/// The kernel's monotonic clock (translation #12).
///
/// Consent deadlines are measured against **this**, not against wall time.
/// Wall time can jump — a laptop sleeping, an NTP correction — and a prompt
/// whose deadline moved is a prompt that was either answered for somebody or
/// left hanging.
#[derive(Clone, Debug)]
pub struct MonoClock {
    origin: Instant,
}

impl Default for MonoClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonoClock {
    /// Start a clock at zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Milliseconds since this clock started.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// One outstanding or settled consent prompt.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsentEntry {
    /// The frame the prompt was asked on.
    pub seq: u64,
    /// The prompt, as it went out.
    pub prompt: serde_json::Value,
    /// When it lapses, on the kernel's monotonic clock.
    pub expires_at_mono: u64,
    /// What applies if nobody answers in time.
    pub fallback: ConsentAnswerKind,
    /// What was actually answered, and by whom.
    pub answer: Option<(ConsentAnswerKind, &'static str)>,
}

impl ConsentEntry {
    /// How this prompt stands, given the clock.
    #[must_use]
    pub fn resolution(&self, now_ms: u64) -> Option<(ConsentAnswerKind, &'static str)> {
        self.answer
            .or_else(|| (now_ms >= self.expires_at_mono).then_some((self.fallback, "fallback")))
    }
}

/// The prompts a session has asked, and how they stand.
///
/// A declared fallback is not optional: a prompt with a deadline and no default
/// is a run that hangs when nobody is attending, which is the one thing §5.5's
/// non-interactive modes cannot survive.
#[derive(Debug, Default)]
pub struct ConsentLedger {
    entries: IndexMap<String, ConsentEntry>,
}

impl ConsentLedger {
    /// Record a prompt that has just gone out.
    pub fn asked(&mut self, id: String, entry: ConsentEntry) {
        self.entries.insert(id, entry);
    }

    /// Record an answer. Late answers are ignored: the fallback already applied.
    pub fn answered(&mut self, id: &str, answer: ConsentAnswerKind, now_ms: u64) -> bool {
        match self.entries.get_mut(id) {
            Some(e) if e.answer.is_none() && now_ms < e.expires_at_mono => {
                e.answer = Some((answer, "user"));
                true
            }
            _ => false,
        }
    }

    /// Look one up.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ConsentEntry> {
        self.entries.get(id)
    }
}

struct ClientTx {
    id: ClientId,
    tx: mpsc::UnboundedSender<Frame>,
    queued: Arc<AtomicUsize>,
    lagged: Arc<AtomicBool>,
    last_sent: Arc<AtomicU64>,
}

struct Inner {
    encoder: Mutex<Encoder>,
    seq: SeqAuthority,
    ring: Mutex<ReplayRing>,
    clients: Mutex<Vec<ClientTx>>,
    consent: Mutex<ConsentLedger>,
    clock: MonoClock,
    next_client: AtomicU64,
}

/// One session's fan-out: the encoder, the seq authority, the replay ring and
/// every connected client.
///
/// Cheap to clone; every clone is the same session.
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Inner>,
}

impl Hub {
    /// A hub for one session.
    #[must_use]
    pub fn new(thread_id: &str) -> Self {
        Self::with_capacity(thread_id, REPLAY_RING_FRAMES)
    }

    /// A hub whose replay ring holds `ring` frames.
    #[must_use]
    pub fn with_capacity(thread_id: &str, ring: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                encoder: Mutex::new(Encoder::new(thread_id)),
                seq: SeqAuthority::new(),
                ring: Mutex::new(ReplayRing::new(ring)),
                clients: Mutex::new(Vec::new()),
                consent: Mutex::new(ConsentLedger::default()),
                clock: MonoClock::new(),
                next_client: AtomicU64::new(1),
            }),
        }
    }

    /// The session's monotonic clock.
    #[must_use]
    pub fn clock(&self) -> &MonoClock {
        &self.inner.clock
    }

    /// The last `seq` this session has handed out.
    #[must_use]
    pub fn last_seq(&self) -> Option<u64> {
        self.inner.seq.last()
    }

    /// Encode one kernel event, number it, buffer it and fan it out.
    ///
    /// **Never waits for a client.** A client that has stopped reading is
    /// marked lagged and its backlog is dropped; the turn completes either way.
    pub fn publish(&self, event: &Event) -> Vec<Frame> {
        let encoded = {
            let mut enc = self.inner.encoder.lock().expect("encoder");
            enc.encode(event)
        };
        let now = self.inner.clock.now_ms();
        let mut frames = Vec::with_capacity(encoded.len());
        for ev in encoded {
            let frame = self.inner.seq.stamp(self.stamp_consent(ev, now));
            self.inner.ring.lock().expect("ring").push(frame.clone());
            frames.push(frame);
        }
        self.fan_out(&frames);
        frames
    }

    /// Put the absolute deadline on a consent prompt, and remember it.
    ///
    /// The frame carries `deadline_ms` *and* `expires_at_mono`, because a client
    /// that re-attaches a minute later needs to know whether the question it is
    /// looking at is still a question.
    fn stamp_consent(&self, ev: AguiEvent, now_ms: u64) -> AguiEvent {
        let AguiEvent::Custom { name, mut value } = ev else {
            return ev;
        };
        if name != orrery_agui::CONSENT_REQUEST {
            return AguiEvent::Custom { name, value };
        }
        let deadline = value
            .get("deadline_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let expires = now_ms.saturating_add(deadline);
        if let Some(obj) = value.as_object_mut() {
            obj.insert("expires_at_mono".into(), expires.into());
        }
        let prompt = value
            .get("prompt")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(id) = prompt.get("id").and_then(serde_json::Value::as_str) {
            self.inner.consent.lock().expect("consent").asked(
                id.to_owned(),
                ConsentEntry {
                    seq: self.inner.seq.peek(),
                    prompt: prompt.clone(),
                    expires_at_mono: expires,
                    // Deny, because a fallback that grants is a fallback that
                    // decides something irreversible on nobody's behalf.
                    fallback: ConsentAnswerKind::Deny,
                    answer: None,
                },
            );
        }
        AguiEvent::Custom { name, value }
    }

    /// Record a consent answer. Returns false if the deadline had already passed.
    pub fn answer(&self, prompt: PromptId, answer: ConsentAnswerKind) -> bool {
        let now = self.inner.clock.now_ms();
        self.inner
            .consent
            .lock()
            .expect("consent")
            .answered(&prompt.to_string(), answer, now)
    }

    /// How a prompt stands right now.
    #[must_use]
    pub fn consent_state(&self, prompt: &str) -> Option<ConsentEntry> {
        self.inner
            .consent
            .lock()
            .expect("consent")
            .get(prompt)
            .cloned()
    }

    /// Subscribe a new client, with its own coalescer.
    pub fn subscribe(&self, tick: Duration) -> ClientStream {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = ClientId(self.inner.next_client.fetch_add(1, Ordering::SeqCst));
        let queued = Arc::new(AtomicUsize::new(0));
        let lagged = Arc::new(AtomicBool::new(false));
        let last_sent = Arc::new(AtomicU64::new(0));
        self.inner.clients.lock().expect("clients").push(ClientTx {
            id,
            tx,
            queued: Arc::clone(&queued),
            lagged: Arc::clone(&lagged),
            last_sent: Arc::clone(&last_sent),
        });
        ClientStream {
            id,
            rx,
            coalescer: Coalescer::new(tick),
            queued,
            lagged,
        }
    }

    /// How many clients are connected.
    #[must_use]
    pub fn client_count(&self) -> usize {
        self.inner.clients.lock().expect("clients").len()
    }

    /// Replay from the ring, with expired consent prompts already resolved.
    ///
    /// # Errors
    ///
    /// [`ReplayError::TooOld`] when the ring no longer holds what is asked for;
    /// the caller falls back to `SessionStore::events_since`.
    pub fn attach(&self, since: Option<u64>) -> Result<Vec<Frame>, ReplayError> {
        let frames = self.inner.ring.lock().expect("ring").since(since)?;
        let now = self.inner.clock.now_ms();
        let ledger = self.inner.consent.lock().expect("consent");
        Ok(frames
            .into_iter()
            .map(|f| resolve_replayed_consent(f, &ledger, now))
            .collect())
    }

    fn fan_out(&self, frames: &[Frame]) {
        let mut clients = self.inner.clients.lock().expect("clients");
        clients.retain(|c| !c.tx.is_closed());
        for client in clients.iter() {
            if client.lagged.load(Ordering::SeqCst) {
                continue;
            }
            for frame in frames {
                if client.queued.load(Ordering::SeqCst) >= CLIENT_BACKLOG_LIMIT {
                    // Its view, not the run. It re-attaches with `since`.
                    client.lagged.store(true, Ordering::SeqCst);
                    tracing::warn!(
                        client = client.id.0,
                        "client fell behind; abandoning backlog"
                    );
                    break;
                }
                if client.tx.send(frame.clone()).is_ok() {
                    client.queued.fetch_add(1, Ordering::SeqCst);
                    client.last_sent.store(frame.seq, Ordering::SeqCst);
                }
            }
        }
    }
}

/// A prompt whose deadline lapsed while nobody was attached replays as
/// *resolved*, not as a live question.
///
/// Otherwise a client that reconnects after a minute is asked to approve
/// something the kernel already denied — and whichever way the person answers,
/// the answer is about a decision that has been made.
fn resolve_replayed_consent(frame: Frame, ledger: &ConsentLedger, now_ms: u64) -> Frame {
    let AguiEvent::Custom { name, value } = &frame.event else {
        return frame;
    };
    if name != orrery_agui::CONSENT_REQUEST {
        return frame;
    }
    let Some(id) = value
        .get("prompt")
        .and_then(|p| p.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return frame;
    };
    let Some((answer, by)) = ledger.get(id).and_then(|e| e.resolution(now_ms)) else {
        return frame;
    };
    Frame {
        seq: frame.seq,
        merged_from: frame.merged_from,
        event: AguiEvent::Custom {
            name: orrery_agui::CONSENT_RESOLVED.to_owned(),
            value: serde_json::json!({
                "prompt": value.get("prompt").cloned().unwrap_or(serde_json::Value::Null),
                "prompt_id": id,
                "answer": answer,
                "by": by,
            }),
        },
    }
}

/// One client's view of the session: its own receiver and its own coalescer.
#[derive(Debug)]
pub struct ClientStream {
    id: ClientId,
    rx: mpsc::UnboundedReceiver<Frame>,
    coalescer: Coalescer,
    queued: Arc<AtomicUsize>,
    lagged: Arc<AtomicBool>,
}

impl ClientStream {
    /// This client's handle.
    #[must_use]
    pub fn id(&self) -> ClientId {
        self.id
    }

    /// Whether this client fell behind and has to re-attach.
    #[must_use]
    pub fn is_lagged(&self) -> bool {
        self.lagged.load(Ordering::SeqCst)
    }

    /// Whether nothing is waiting, here or upstream.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.rx.is_empty() && !self.coalescer.has_pending()
    }

    /// The next batch of frames, merged over this client's tick.
    ///
    /// `None` when the session is over. A tick of zero returns everything
    /// available, unmerged.
    pub async fn next_batch(&mut self) -> Option<Vec<Frame>> {
        let first = self.rx.recv().await?;
        self.queued.fetch_sub(1, Ordering::SeqCst);
        self.coalescer.push(first);
        let tick = self.coalescer.tick();
        if !tick.is_zero() {
            tokio::time::sleep(tick).await;
        }
        while let Ok(frame) = self.rx.try_recv() {
            self.queued.fetch_sub(1, Ordering::SeqCst);
            self.coalescer.push(frame);
        }
        Some(self.coalescer.drain())
    }
}
