# 08 · Protocol and transport — one wire, three listeners, every client equal

**Goal.** The kernel is a server whether or not it looks like one. Same frames over an in-process channel, a named pipe or UDS, and AG-UI's native HTTP+SSE — all three in phase 1, so ratatui and Ink can sit on one session at the same time. Plus the client half: `AguiSession` and `SurfaceStore` in Rust and TypeScript, from one set of conformance fixtures, and the `json` renderer. When this is done there is no privileged client.

**Covers.** §5.1 (transport) · §5.2 (frames) · §5.3 (session lifecycle) · §5.4 (streaming, backpressure, cancellation) · §5.5 (non-interactive modes, with plan 17) · §5.6 (AG-UI adoption) · §6.3's `json` renderer.

**Crates.** `core/crates/orrery-transport` · `core/crates/orrery-agui` (encoder) · `clients/sdk-rs` (`orrery-client`) · `clients/sdk-ts` (`@orrery/client`) · `clients/json` (`orrery-client-json`) · `clients/conformance`.

**Depends on.** [`01`](01-proto-shared-types.md), [`02`](02-session-store.md) (replay), [`05`](05-kernel-loop.md) (the events to carry).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- `seq` is assigned **once, per session, at the differ's output** — never per connection.
- One coalescer **per connected client**. Two clients at different speeds must not share one.
- **Rendering never applies backpressure to the agent loop.** A stalled client degrades its own view, not the run.
- `#[serde(tag = "t")]` with explicit renames; CBOR via `ciborium`; MessagePack is impossible here.
- `tower` at the HTTP edge only, never inside the loop.
- Consent deadlines use a kernel-owned monotonic clock (translation #12).

This plan owns translation **#12**.

---

## Architecture

### Three listeners, one frame set

| Transport | Used by | Notes |
|---|---|---|
| In-process | `orrery` (ratatui), `orrery run`, single-shot | No serialisation; same request/event types. The kernel is still a server. |
| Named pipe / UDS | `orrery attach` on the same machine | `interprocess` gives one API over both. Default for local interactive use. |
| HTTP + SSE | Ink, any AG-UI client, the ADE later | `POST /run` (AG-UI `RunAgentInput`) → SSE stream; `POST /control` for what AG-UI has no equivalent for. |
| TCP + TLS | Remote execution host | Phase 4. Same frames, `tokio-rustls`. |

**The key decision: the kernel is always a server, even in-process.** A CLI that skips serialisation still goes through the same request/event types, so there is no second code path that drifts.

### What stays ours, and why

§5.6's three:

1. **The control RPC.** AG-UI's input path is a run invocation, not a session protocol. `session.attach(since)`, `turn.cancel`, `intent` and `query` have no counterpart. They ride `POST /control`.
2. **`seq` and replay.** AG-UI has no resumption story. Ours is the reason a dropped connection resumes instead of losing the turn.
3. **Consent.** Carried as AG-UI `Custom`. A prompt with a deadline and a declared fallback is more than an interrupt.

And one tension named rather than papered over: **AG-UI's shared state is bidirectional by design; our invariant is that clients send intents and never state mutations.** We keep the invariant — `StateSnapshot` and `StateDelta` flow outward only, a client edit arrives as an `intent` the kernel validates. That is a restriction of AG-UI, not a violation, and anyone porting a component that expects to write state directly needs to know. Put it in `clients/README.md`.

### The mapping

| Our frame | AG-UI event |
|---|---|
| `turn.started` / `turn.settled` / `error` | `RunStarted` / `RunFinished` / `RunError` |
| `delta` carrying text | `TextMessageStart` / `Content` / `End` |
| `tool.started` / `tool.settled` | `ToolCallStart` / `Args` / `ToolCallResult` / `End` |
| `delta` on the surface whose id **is** an open call's id | `ToolCallArgs` — our frames have no argument event; the call's own surface carries them |
| `delta` carrying a `SurfacePatch` | `StateDelta` — our four ops are already JSON-Patch in shape |
| step boundary, mode change (§4.6) | `StepStarted` / `StepFinished`, `ActivitySnapshot` |
| sub-agent spawn and return (§4.10) | `SubagentStarted` / `Finished` / `Error` |
| `consent.request` / `consent.answer` | no equivalent — `Custom`, plus our control RPC |

### Why we implement the encoder rather than depend on a crate

There is no first-party Rust SDK. Upstream has a community `sdks/community/rust` with no code owner and stalled PRs, plus three unaffiliated crates. We only ever **produce** AG-UI — the client crate would be dead weight. So: vendor the event enum in `orrery-agui`, pin the protocol version in `orrery-proto`, and run a CI job (`cargo xtask agui-drift`) that diffs our enum against upstream's published schema. Revisit if `ag-ui-core` gains an owner.

### The client layer — shared by every renderer

Two pieces, implemented twice (Rust and TS) against **one set of fixtures**.

```rust
// clients/sdk-rs — orrery-client
pub struct AguiSession { /* connection, seq, pending control calls */ }
impl AguiSession {
    pub async fn connect(endpoint: &Endpoint) -> Result<Self, ClientError>;
    pub async fn attach(&mut self, session: SessionId, since: Option<Seq>) -> Result<(), ClientError>;
    pub fn events(&mut self) -> impl Stream<Item = Result<AguiEvent, ClientError>>;
    pub async fn submit(&self, input: UserInput) -> Result<TurnId, ClientError>;
    pub async fn cancel(&self, turn: TurnId) -> Result<(), ClientError>;
    pub async fn answer(&self, prompt: PromptId, a: ConsentAnswerKind) -> Result<(), ClientError>;
    pub async fn intent(&self, surface: SurfaceId, value: serde_json::Value) -> Result<(), ClientError>;
}

pub struct SurfaceStore { /* IndexMap<SurfaceId, Surface> + per-turn grouping */ }
impl SurfaceStore {
    pub fn apply(&mut self, ev: &AguiEvent) -> Vec<StoreChange>;
    pub fn turn(&self, t: TurnId) -> Option<&TurnView>;
    pub fn settled(&self) -> impl Iterator<Item = &TurnView>;   // what scrollback shows
    pub fn live(&self) -> Option<&TurnView>;                    // what the live region shows
}
```

`SurfaceStore` is where text deltas become a markdown surface and tool calls become a tool `stack`, so **no renderer reimplements that**. It detects `seq` gaps and reports `StoreChange::GapDetected { expected, got }`, which is a renderer's cue to re-attach.

TS is the same two types on `@ag-ui/client`'s `HttpAgent` for the subscribe half — which *is* the "existing AG-UI clients work unmodified" proof (§5.6).

### Coalescing and replay do not conflict

They read from different places. A slow client loses intermediate **frames**; re-attachment replays from the **session store**, which has every settled turn in full. The kernel buffers frames only for as long as a connected client is behind.

```rust
pub struct Coalescer { tick: Duration /* ~33ms */, pending: IndexMap<SurfaceId, PendingPatch> }
// merges consecutive Append on one id; collapses Set+Replace to the last Replace
```

### Consent deadlines (translation #12)

Every `consent.request` carries `deadline_ms` **relative to a kernel monotonic clock**, and the frame also carries the absolute `expires_at_mono`. On `attach(since)`, replayed prompts whose deadline has passed are **not** re-emitted as live prompts; they are replayed as already-resolved with their fallback. Otherwise a client that reconnects after a minute is asked to approve something the kernel already denied.

---

## File structure

**Create**

- `harness/core/crates/orrery-transport/src/{lib,frame,seq,replay,coalesce,listener/{inproc,pipe,http},error}.rs`
- `harness/core/crates/orrery-agui/src/{lib,event,encode,map,drift}.rs`
- `harness/clients/sdk-rs/src/{lib,session,store,endpoint,error}.rs`
- `harness/clients/sdk-rs/tests/conformance.rs`
- `harness/clients/sdk-ts/{package.json,src/{index,session,store}.ts,test/conformance.test.ts}`
- `harness/clients/json/src/lib.rs`
- `harness/clients/conformance/{README.md,*.jsonl}`

---

## Tasks

### Task 1 · Conformance fixtures — write these first

Files: `harness/clients/conformance/*`

The fixtures are the contract between five implementations. They come before any of them.

- [x] Define the format in `README.md`: one JSONL file per scenario; each line is either `{"ev": <AguiEvent>}` or `{"expect": <SurfaceStore state>}`.
- [x] Write the scenarios: `text-only`, `tool-call`, `tool-args-interleaved`, `streaming-markdown` (partial fences), `consent-prompt`, `question-surface`, `table-then-resort` (the cost-guard case), `seq-gap`, `reattach-since`, `cancel-midturn`, `custom-with-fallback`.
- [x] Each scenario must be derivable from one of plan 03's six provider stream fixtures, so the same turn can be driven end-to-end or replayed as events alone.

### Task 2 · AG-UI encoder

Files: `orrery-agui/src/*`

- [x] **Failing test first.** `agui::maps_every_frame` — a table over every `Event` variant asserting the AG-UI event(s) it produces; fails until each mapping lands.
- [x] `agui::state_delta_is_json_patch_shaped` — a `SurfacePatch::Set` encodes as an RFC 6902 `replace` op with the right path.
- [x] `agui::consent_is_custom` — round-trips through `Custom` with name and payload intact.
- [x] **Added after the fact:** `agui::tool_args_are_emitted` and
  `agui::interleaved_calls_keep_their_arguments`. `TOOL_CALL_ARGS` was in the
  enum and never constructed, so a client saw a call start and end with no
  input between them. A `delta` on an open call's surface is that call's
  arguments; `clients/conformance/tool-args-interleaved.jsonl` is the
  seventeenth scenario and pins the reconstruction in both SDKs.
- [x] Implement the vendored event enum and the encoder.
- [x] `cargo xtask agui-drift` — fetch upstream's schema, diff variant names, fail on unknown additions. Networked, so CI-only and skippable offline.

### Task 3 · `seq`, replay, coalescing

Files: `orrery-transport/src/{seq,replay,coalesce}.rs`

- [x] **Failing test first.** `seq::is_assigned_once` — two connected clients see identical `seq` values for the same event.
- [x] `coalesce::merges_appends` — 100 single-character appends in one tick become one append.
- [x] `coalesce::collapses_to_last_replace`.
- [x] `coalesce::per_client` — a slow client and a fast client on one session; the fast one gets fine-grained frames, the slow one gets merged ones, **and both end at the same final state**. This is the test that matters.
- [x] `replay::attach_since_is_contiguous` — attach with `since = 5`, get 6..n with no gaps.
- [x] Implement.

### Task 4 · In-process listener

Files: `orrery-transport/src/listener/inproc.rs`

- [x] **Failing test first.** `inproc::same_types_no_serialisation` — a round trip that never touches serde (assert via a type-level marker or a counter in a custom serializer that must stay at zero).
- [x] Implement over `tokio::sync::mpsc`.

### Task 5 · Pipe / UDS listener

Files: `orrery-transport/src/listener/pipe.rs`

- [x] **Failing test first.** `pipe::two_clients_one_session` — two connections attach to one session, both receive the same turn, each with its own coalescer.
- [x] `pipe::disconnect_does_not_end_the_session` — drop a client mid-turn; the turn completes; re-attach and replay shows it.
- [x] Implement over `interprocess` with length-prefixed framing, JSON or CBOR by negotiation.

### Task 6 · HTTP + SSE listener

Files: `orrery-transport/src/listener/http.rs`

- [x] **Failing test first.** `http::run_returns_sse` — `POST /run` with a `RunAgentInput` body returns `text/event-stream` and the expected event sequence.
- [x] `http::control_endpoints` — attach, cancel, consent answer, intent, query.
- [x] `http::slow_consumer_does_not_stall_the_kernel` — a client that never reads; assert the turn still completes.
- [x] Implement with `axum`. Bind to loopback by default; a bearer token from the endpoint string. **No auth story beyond loopback + token in phase 1** — record that as a limitation.
- [x] **Added after the fact, and the Done-when named it:** `GET /events`, the
  passive subscribe AG-UI has no vocabulary for. `http::events_is_a_passive_subscribe`
  and `http::events_since_is_contiguous` — a session somebody else is driving,
  replay then live, one numbering space, guarded by the same bearer token.

### Task 7 · Rust client SDK

Files: `clients/sdk-rs/*`

- [x] **Failing test first.** `conformance::all_scenarios` — run every fixture through `SurfaceStore`, assert the expected state at each checkpoint.
- [x] `store::text_deltas_become_markdown` — deltas accumulate into one markdown surface with `complete: false`, flipped `true` at message end.
- [x] `store::gap_is_detected`.
- [x] Implement `AguiSession`, `SurfaceStore`, `Endpoint` parsing (`inproc:`, `pipe:<name>`, `http://…`).

### Task 8 · TypeScript client SDK

Files: `clients/sdk-ts/*`

- [x] **Failing test first.** `conformance.test.ts` — the **same fixture files**, the same assertions, under vitest.
- [x] Implement on `@ag-ui/client`'s `HttpAgent` for subscribe, plus a small fetch client for `/control`.
- [x] Consume `@orrery/protocol` types; no hand-written frame types.

### Task 9 · The json renderer

Files: `clients/json/src/lib.rs`

- [x] **Failing test first.** `json::emits_fallback_beside_payload` — a `custom` surface emits both the payload and its fallback, so a lazy fallback shows up in CI (§6.2).
- [x] `json::ignores_placement` — view bindings' `placement` is a human-client concern; the json renderer emits everything.
- [x] Implement: line-delimited AG-UI events on stdout.

### Task 10 · Consent deadlines

Files: `orrery-transport/src/lib.rs`

- [x] **Failing test first.** `consent::expired_prompt_is_not_replayed_live` — issue a prompt with a 50 ms deadline, let it lapse, attach with `since` before it; assert it replays as resolved-by-fallback, not as a live prompt.
- [x] Implement the monotonic clock and the replay rule.

---

## Done when

- `cargo test -p orrery-transport -p orrery-agui -p orrery-client -p orrery-client-json` green; `pnpm -C harness/clients/sdk-ts test` green.
- The **same** conformance fixtures pass in Rust and TypeScript.
- `orrery serve --provider fixture:…` accepts a ratatui client and an Ink
  client on one session simultaneously; both render the turn.
  **True as of the `GET /events` route.** `serve::two_renderers_one_session` in
  `orrery-cli` puts the ratatui client and the json client on one pipe and
  asserts both draw the same turn, tool call included; the HTTP listener now
  also has a passive subscribe route, so an SSE client renders a session it did
  not start. `serve::attach_over_http_renders_the_turn` is the literal form:
  one `serve`, a watcher on `http://…` that submits nothing, a submitter on the
  pipe, and the watcher draws the whole turn. Ink reaches the same route over
  `ORRERY_ENDPOINT` (`serve::the_ink_client_reaches_the_kernel`); what still
  stops a *headless* Ink from being the second renderer in CI is Ink's own
  raw-mode requirement, not the transport.
- A client that stops reading does not stall the kernel.

## Open questions

1. **Auth on the HTTP listener.** Loopback plus a bearer token is enough for a developer machine and not enough for §5.1's "remote execution host". Phase 4 needs a real answer (mTLS is in `ProviderAuth`'s vocabulary already). Decide before TLS lands.

   **Still open, deliberately.** Phase 1 ships loopback + a bearer token in the endpoint's authority (`http://<token>@host:port`, never a query parameter — a query string ends up in logs, shell history and `ps`). Recorded as a limitation in `listener/http.rs`: no per-session authorisation, no audience check, no replay protection. This is the question phase 4 answers together with TLS, not now.

2. **CBOR negotiation.** Header, query parameter, or a control frame? Pick one and write it down; it is the kind of thing that gets decided twice.

   **Decided: a control frame, and it is always JSON.** On a byte-stream connection (pipe, UDS, later TCP) the first frame is a `Hello` naming the format, and everything after it is in that format. Not a header — a named pipe has none — and not a query parameter, which would put the answer in a URL only one of the three listeners has. The frame that names the format cannot itself be in the format it names, so `Hello` and `HelloAck` are JSON unconditionally. HTTP is the exception that proves the rule: there it is the `Accept` header, because that is what an off-the-shelf AG-UI client already sends. See `orrery-transport/src/frame.rs`.

3. **Does `events_since` live on `SessionStore` or a separate `EventLog`?** Cross-reference plan 02's open question 1 — this plan is the caller, so decide here.

   **Decided: on `SessionStore`, where plan 02 already put it, and the transport keeps a ring in front of it.** Two things follow, and they matter more than the placement:

   - **One numbering space.** `seq` is assigned by `SeqAuthority` at the **encoder's** output — downstream of the differ, upstream of every connection — so the unit it counts is an AG-UI frame, not a kernel event. One kernel event can expand to two frames (`tool.settled` is `TOOL_CALL_END` and `TOOL_CALL_RESULT`) and each gets its own number, because otherwise contiguity would not be arithmetic.
   - **Cold replay re-encodes.** `ReplayRing` serves the common case and returns `ReplayError::TooOld` rather than a silent hole when it cannot; the caller then re-encodes from the store through a fresh `Encoder`, which is deterministic and reproduces the same numbering. A separate `EventLog` would have been a second numbering space, which is exactly the drift this plan is trying not to create.

     **Amended 2026-09-19, when `orrery replay` was built and the sentence above turned out to name the wrong table.** Re-encoding from `events_since` reproduces the *numbering*, but not the stream: the sqlite backend writes exactly **one** event per turn — `turn.settled` — because the rich stream (text deltas, `TOOL_CALL_START`, argument fragments) is produced by the encoder at the hub while the turn runs and is never persisted. Feeding those rows to a fresh `Encoder` yields a run with no text in it. Cold replay therefore re-encodes the **turn rows**, not the event rows: `SessionStore::turns(branch)` landed with `orrery replay` and hands back `TurnKind`s with their calls, tools, outcomes and usage intact, and `cmd/replay.rs` narrates them through the same `Publisher` a live turn uses. The rejected alternative was persisting the encoder's output beside the turns, which doubles every write, puts a rendering concern inside the one store that cannot degrade, and makes a durable record depend on which AG-UI version wrote it. One consequence is visible and accepted: delta *boundaries* are not preserved, so a replayed turn emits one `TEXT_MESSAGE_CONTENT` where the live turn emitted several. `replay::renders_a_past_session` asserts the two streams match with runs of one kind collapsed, which is the honest statement of that.

4. **`RunAgentInput` mapping.** AG-UI's run invocation carries messages and state; ours carries a `UserInput` against an existing session. Confirm the adapter is lossless enough that a stock AG-UI client works without special-casing.

   **Confirmed, with one deliberate loss.** The adapter takes `threadId` as the session, the **last `user` message** as the input, and ignores the rest of the message list and the `state` blob. A stock client works unmodified; what it loses is the ability to rewrite history on the way in, which it was never allowed to do — the kernel's transcript is the authority, and a client that replayed its idea of the history would be writing state. `http::run_agent_input_maps_to_user_input` pins it.

## State

**Landed 2026-09-18.** All ten tasks. 36 Rust tests across four crates plus 18
under vitest, all green, none of them touching the network or a paid API.
`GET /events` and `AguiSession::subscribe` landed after the first pass, which is
what closed the Done-when bullet above and let `orrery attach` take either
endpoint `serve` prints.

- `orrery-agui` — vendored AG-UI event enum (15 variants, diffed against the real `@ag-ui/core`), `Encoder`, `Frame` with `seq` and `merged_from`. `PatchOp::Append` is ours and documented as such: JSON Patch cannot express string concatenation, and re-sending a markdown body per token would undo the point of `SurfacePatch::Append`.
- `orrery-transport` — `SeqAuthority`, `ReplayRing`, per-client `Coalescer`, `Hub`, `MonoClock` + `ConsentLedger` (translation #12), and all three listeners: in-process (generic, no serde bound anywhere), pipe/UDS over `interprocess` with length-prefixed framing, and HTTP+SSE on `axum`.
- `clients/sdk-rs` (`orrery-client`) — `AguiSession` over all three transports, `SurfaceStore`, `Endpoint` parsing, and the fixture runner the other Rust clients will reuse.
- `clients/sdk-ts` (`@orrery/client`) — the same store on stock `@ag-ui/client`, the same ten fixtures, under vitest.
- `clients/json` (`orrery-client-json`) — line-delimited AG-UI, with each custom surface's rendered fallback beside its payload.
- `clients/conformance` — ten scenarios, each derivable from one of plan 03's six provider streams. `README.md` fixes the format.
- `cargo xtask agui-drift` — implemented, offline by default; `--fetch` is CI-only. Running it against the installed `@ag-ui/core` corrected the pinned list: upstream calls them `REASONING_*`, not `THINKING_*`, and the protocol version is 1.0.

**Not done here, at the time.** The kernel (plan 05) did not exist yet, so every listener in this wave was driven by a hand-written event script or a `ControlHandler` stand-in. **Both have since landed:** plan 05 built the kernel and plan 17 wrote `orrery serve`, which is `orrery-cli/src/cmd/serve.rs`. The stand-ins stay in the transport tests on purpose — a transport test that needed a kernel would be testing the kernel. `sdk-rs` is `publish = false` while it depends on the unpublished `orrery-transport` — splitting the store (pure data, publishable) from the session is the phase-2 move, and there is no third-party consumer yet to need it.
