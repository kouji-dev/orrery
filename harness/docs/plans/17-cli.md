# 17 · CLI — the command tree, and the proof that no client is privileged

**Goal.** One binary, `orrery`, that starts a kernel, attaches a renderer, and exposes everything the harness can explain about itself. Its interactive mode is the ratatui client attaching over the in-process transport — the same path an external client takes over a pipe. When this is done, `orrery run -p "…"` completes a turn and `orrery --ui ink` puts a different renderer on the same kernel.

**Covers.** §5.5 (non-interactive modes) · the CLI surfaces promised in §4.7 (`ext test`), §4.8 (`permissions explain`), §4.9 (`config explain`, `init`, `import`), §4.14 (`eval`).

**Crate.** `core/crates/orrery-cli`, binary name `orrery`.

**Depends on.** everything. Each subcommand lands with its own plan; this file owns the shape, the flags and the exit codes.

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **No privileged client.** The interactive TUI attaches over the transport, not through a back door.
- Sessions outlive clients. `orrery serve` keeps running when the terminal closes.
- `--json` emits the same `Event` frames as everything else — no separate reporting path to maintain.
- `consent = "never"` profiles deny instead of prompting, so CI cannot hang.

---

## The command tree

```
orrery                                   # kernel + ratatui, in-process
orrery --ui ink                          # kernel + spawn the Ink client
orrery --profile review                  # any command takes --profile

orrery run -p "<prompt>"                 # one turn, print final text, exit
orrery run -p "…" --json                 # line-delimited AG-UI events on stdout
orrery serve [--listen <addr>]           # kernel only; print the endpoint; keep running
orrery attach <endpoint> [--since <seq>] # ratatui against a running kernel
orrery replay <session> [--json]         # re-emit a stored session as events

orrery session list | show <id> | rm <id>

orrery ext list | install <name> | remove <name> | test [path]
orrery permissions explain <call>
orrery config explain <key>
orrery init [--profile <name>]
orrery import [--from claude-code|codex]

orrery eval run <suite> [--profile a,b] [--model x,y] [--format junit]
orrery eval compare <run-a> <run-b>
orrery eval replay <run> --case <id>

orrery ledger | telemetry                # the §4.12 streams, queryable
```

### Global flags

| Flag | Meaning |
|---|---|
| `--profile <name>` | Which profile to resolve (plan 10) |
| `--workspace <path>` | Defaults to cwd |
| `--ui <ratatui\|ink\|json>` | Renderer. Default ratatui when a tty, json otherwise |
| `--json` | Shorthand for `--ui json` |
| `--consent <always\|once\|never>` | Overrides the profile, within the managed clamp |
| `-v / -vv` | Log level to stderr; never mixed into `--json` stdout |

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | The turn completed but the task failed (grader/gate said no) |
| 2 | Usage error |
| 3 | Budget exceeded |
| 4 | Denied by policy |
| 5 | Needs login |
| 6 | Kernel or transport error |

Distinct codes for 3, 4 and 5 because a CI script's response to each is different.

### Streams discipline

**stdout is data, stderr is narration.** With `--json`, stdout carries only line-delimited events; every log, warning and progress line goes to stderr. A test asserts stdout parses as JSONL for every non-interactive command.

---

## File structure

**Create**

- `harness/core/crates/orrery-cli/{Cargo.toml,src/main.rs}`
- `harness/core/crates/orrery-cli/src/cmd/{run,serve,attach,replay,session,ext,permissions,config,init,import,eval,ledger}.rs`
- `harness/core/crates/orrery-cli/src/{args,ui,exit,term}.rs`
- `harness/core/crates/orrery-cli/tests/{cli,exit_codes,json}.rs`

**Also landed, and not in the list above.** `src/session.rs` (the kernel, the
hub and the two wrappers that turn a running turn into events), `src/control.rs`
(the kernel's side of the control RPC, one implementation for all three
listeners), `src/render.rs` (the drain loop both in-binary renderers share) and
`src/cmd/interactive.rs` (bare `orrery`). `tests/{serve,ext}.rs` and
`tests/common/mod.rs` alongside the three the plan named.

---

## Tasks

### Task 1 · Argument shape and help

Files: `src/{main,args}.rs`, `tests/cli.rs`

- [x] **Failing test first.** `cli::help_lists_every_command` — a snapshot of `--help` and of each subcommand's `--help`, so the surface cannot change silently. *(Landed as `cli::help_snapshot`, 26 committed snapshots under `tests/snapshots/`.)*
- [x] `cli::unknown_flag_is_exit_2`.
- [x] Implement the clap tree. Every not-yet-implemented subcommand is a `todo!` that exits 2 with "not implemented in this build", naming its plan file — so the tree is complete from phase 0 and fills in.

### Task 2 · Renderer selection

Files: `src/ui.rs`

- [x] **Failing test first.** `ui::defaults_to_json_without_a_tty` — piping the output selects the json renderer. *(Twice: the rule in `ui::tests`, and the behaviour end to end in `json::defaults_to_json_without_a_tty`, where a piped bare `orrery` reads one prompt per line of stdin and emits events.)*
- [x] `ui::ink_requires_node` — with no `node` on PATH, `--ui ink` exits with a clear message, not a spawn stack trace. *(Landed as `serve::ink_reports_what_is_missing`: Node is normally present on a developer machine, so the test reaches the same code path through a bundle that is not built, and asserts the message names what is missing, says what to do instead, and is not a stack trace.)*
- [x] Implement.

**Decided while implementing: the tty rule is about the *interactive* surface.**
`run` is non-interactive and its default is its human output — the final text —
whether or not stdout is a pipe; `--json` is what asks for events. Guessing from
a pipe would mean `orrery run -p …` and `orrery run -p … | tee` printed
different things, which is the trap the rule was meant to avoid, not create.

### Task 3 · `run`

Files: `src/cmd/run.rs`, `tests/json.rs`

- [x] **Failing test first, and it is the phase-1 criterion.** `run::completes_a_turn_with_a_tool_call` — against the fixture provider, `orrery run -p "…"` prints final text and exits 0, having dispatched a real tool.
- [x] `run::json_is_parseable` — every stdout line is valid JSON and matches the `Event` schema. *(Asserted as AG-UI `Frame`s — a `seq` and an event `type` on every line, `seq` monotonic — because that is what is on the wire: `Event` is the kernel's vocabulary and the encoder is what a client reads. `run::the_tool_call_really_happened` pins the event *sequence* and checks the bytes in `TOOL_CALL_RESULT` are the bytes on disk.)*
- [x] `run::stderr_does_not_pollute_stdout` — with `-vv`, stdout is still clean JSONL.
- [x] Implement.

**How the events are produced.** The kernel returns a `TurnOutcome` and writes
rows; it publishes no event stream. Rather than open a hole in it, the CLI wraps
the two collaborators a turn already talks to — the provider, where text arrives
as it is generated, and the session store, where settled tool calls are
appended — and both go in as ordinary `Arc<dyn …>` values on `ResolvedConfig`.
Nothing in `core/` changed shape. The consequence, written down in
`session.rs`: the run id in `RUN_STARTED` is minted by the CLI, because
`Kernel::run_turn` mints a `TurnId` it never hands out.

### Task 4 · `serve` and `attach`

Files: `src/cmd/{serve,attach}.rs`

- [x] **Failing test first, and it is the "no privileged client" proof.** ~~`serve::two_renderers_one_session` — start `serve`, attach ratatui and the Ink client, submit one turn, assert both render it.~~ **Amended: the two renderers are ratatui and json, not ratatui and Ink.** The proof itself holds and is asserted — one `serve`, two renderer *processes*, neither of them the process holding the kernel, both over the same pipe, same handshake, same control RPC, both rendering the same turn including the tool call. Ink cannot be the second one **today**, for a reason that is not about privilege: AG-UI's HTTP transport has one way in, `POST /run`, and its SSE body carries that run's frames. There was no passive subscribe route, so an Ink client could only render a turn it started itself, and its only way to start one is a keystroke in its composer. `serve::the_ink_client_reaches_the_kernel` asserts the half this command owns — Ink attaches to the same kernel over the endpoint `serve` printed, with nothing but `ORRERY_ENDPOINT` and `ORRERY_SESSION`, and gets as far as Ink's own raw-mode requirement. **The `GET /events` route has since landed in plan 08's crate**, and `serve::attach_over_http_renders_the_turn` is this test in its literal form for a passive HTTP renderer; a headless *Ink* still stops at raw mode, which is Ink's own.
- [x] `serve::outlives_a_client` — attach, kill the client mid-turn, re-attach with `--since`, assert the turn completed and replays.
- [x] Implement; print the endpoint in a form `attach` and `ORRERY_ENDPOINT` both accept.

**`serve` opens both byte transports.** With no `--listen` it prints two
endpoints, most-local first — `pipe:orrery-<session>` then
`http://127.0.0.1:<port>` — because the two clients want different ones and one
hub serves both. `--listen` picks one. The session id goes on **stderr**
(`orrery: session <id>`) rather than into the endpoint, because stdout is data
and the endpoint grammar has no room for it; a client that speaks AG-UI names
the session it attaches to, which is what `ORRERY_SESSION` carries.

### Task 5 · Exit codes

Files: `src/exit.rs`, `tests/exit_codes.rs`

- [x] **Failing test first.** ~~One test per code: a fixture that exceeds budget ⇒ 3; a denied call with no fallback ⇒ 4; a provider reporting `NeedsLogin` ⇒ 5.~~ **Amended: 3 and 4 are reached by a run; 5 is not reachable from the command line in this build.** `exit_codes::a_ceiling_is_three` (the last stream repeats, so a fixture that only asks for tools never stops asking and the turn budget ends it) and `exit_codes::a_denial_with_no_fallback_is_four` (a hand-written stream reading outside the workspace, refused by the broker) are real runs. **5 is not:** every provider this build can select is the fixture provider, whose auth never says `NeedsLogin`, and the only way to reach it from a flag would be a "signed out" provider spec that exists for the test and for nothing else. The mapping is decided and tested in `exit::tests::needing_a_login_is_five`; the *command* cannot produce it until a provider that can be signed out is selectable (plan 10).
- [x] Implement the mapping from `TurnOutcome` and `KernelError`.

**Decided: a denial is read off the turn's last tool call.** A denial is not a
`TurnOutcome` — the kernel appends `Outcome::Denied` and asks the model again —
so a turn whose work was refused still reports `Completed`. `Exit::from_turn`
therefore takes the turn's tool outcomes: if the **last** one was denied, the
turn had no fallback and the code is 4; a denial the model worked around is a
turn with a fallback and exits 0. Anything else would make the exit code depend
on how many times the model tried.

### Task 6 · Explain commands

Files: `src/cmd/{permissions,config}.rs`

- [ ] **Failing test first.** `explain::permissions_names_rule_layer_file` — output contains the rule id, layer, file and line (plan 07 task 10). **Not landed: plan 07 task 10 has not.** There is no `explain` on `PolicyEngine` to call, and inventing one here would put the answer in the CLI instead of in the engine every client asks. The command is in the tree and exits 2 naming `07-policy-broker-audit.md`.
- [ ] `explain::config_names_the_winning_layer` (plan 10 task 7). **Not landed: plan 10 has not.** There is no layered config yet — `ResolvedConfig` is assembled from flags — so there is no winning layer to name. Exits 2 naming `10-config-layers.md`.
- [ ] Implement; both support `--json`.

### Task 7 · `ext` and `session`

Files: `src/cmd/{ext,session}.rs`

- [x] **Failing test first.** `ext::test_runs_without_a_model` — `orrery ext test` on a fixture extension passes with no network and no provider (plan 06 task 8). *(Plus `ext::a_broken_manifest_is_usage`: exit 2 naming the file, never a panic.)*
- [x] `ext::list_shows_the_ledger` — ~~including `degraded` and `skipped` entries with reasons.~~ **Amended: `ok` and `degraded` with reasons; `skipped` is not producible here.** `ext list` reports every first-party extension this build compiled in, run through `orrery_ext_api::testing::missing` — the same function the real host calls — so a degraded line says the words a session would. `Skipped` is a *loader* state (a deny rule, a disabled extension), and reaching it needs the policy engine, which means a session, which means a model. When `ext list` can take a live session's ledger (plan 07's `query extensions`), `skipped` comes with it.
- [ ] `session::list_and_show`. **Blocked, and not on this plan:** `SessionStore` has `open(id)` and no way to enumerate sessions, so `session list` has nothing to call. Plan 02 owns adding it; `session show` is a `materialise` away once it exists.
- [x] Implement.

### Task 8 · `replay`, `ledger`, `telemetry`

Files: `src/cmd/{replay,ledger}.rs`

- [ ] **Failing test first.** `replay::renders_a_past_session` — replay into the json renderer and diff against the original event stream. **Not landed, and it needs plan 02 first.** `SessionStore::events_since` exists, but the sqlite backend writes exactly one event per turn (`turn.settled`); the rich stream a client sees is produced by the encoder at the hub and is not persisted. Replaying it would mean re-encoding the turn rows, which is plan 08 open question 3's "cold replay re-encodes" — decided there, unbuilt. Exits 2 naming this file.
- [ ] Implement; `ledger` and `telemetry` are `query` frames rendered as tables. **Not landed: the `query` frame has no handler** (`KernelControl` refuses it by name), and the audit stream plan 07 writes has no reader. Both exit 2 naming `07-policy-broker-audit.md`.

### Task 9 · `init`, `import`, `eval`

Files: `src/cmd/{init,import,eval}.rs`

- [ ] Thin wrappers over plans 10 and 16. Each with one smoke test. **Not landed: there is nothing to wrap.** Plan 10 (config layers, `init`, `import`) and plan 16 (the eval runner) are both unlanded, and a wrapper over nothing is a command that lies. All four are in the tree and exit 2 naming their plan, which `cli::unimplemented_subcommands_name_their_plan` asserts.

### Task 10 · Terminal hygiene

Files: `src/term.rs`

- [x] **Failing test first.** `term::panic_restores_the_terminal` — force a panic in the TUI path; assert raw mode is off afterwards. *(The raw-mode **flag** is what is asserted, not a real tty: a test process has no terminal to put into raw mode. What could break is the bookkeeping — a guard that does not fire, a hook that never installed — and both are exercised for real, through `std::panic::catch_unwind` around the actual guard.)*
- [x] ~~`term::ctrl_c_during_startup` — interrupting before attach exits cleanly.~~ **Amended: landed as `term::restoring_twice_is_fine`.** Nothing in this build installs a `^C` handler — the default disposition kills the process — so "interrupting before attach" is exactly "the process died before `setup` ran", and what has to be true is that there is nothing to undo and that undoing it anyway is safe. That is what is asserted. A `^C` that cancels a *turn* rather than the process is a different feature and belongs with the interactive loop.
- [x] Implement a guard that restores on drop and on panic. *(Both: a `RestoreGuard` whose `Drop` restores, and a panic hook installed at `main` that restores **before** the default hook prints, so the backtrace lands in a terminal that can render it.)*

---

## Done when

- `cargo test -p orrery-cli` green; `--help` snapshots committed. *(33 tests: 10
  unit, and `cli`, `json`, `exit_codes`, `serve`, `ext` as integration suites.
  26 snapshots under `tests/snapshots/`.)*
- `orrery run -p "…"` completes a real turn with a real tool call.
  *(`json::completes_a_turn_with_a_tool_call`, the phase-1 acceptance criterion,
  and `json::the_tool_call_really_happened`, which checks the bytes in
  `TOOL_CALL_RESULT` are the bytes on disk.)*
- ~~`orrery serve` + ratatui + Ink on one session renders in both.~~
  **Not true, and amended here rather than caveated below: the second renderer
  is the json client.** `serve::two_renderers_one_session` runs one `serve` and
  two renderer processes over one pipe — the ratatui client and the json client,
  neither of them the process holding the kernel — and both render the same
  turn, tool call included. Ink is *attached* to the same kernel over the
  endpoint `serve` prints and asserted to get there
  (`serve::the_ink_client_reaches_the_kernel`). **The transport half is no
  longer missing:** `orrery-transport` serves `GET /events`, so an SSE client
  renders a session it did not start, and `attach` takes the `http://` endpoint
  as readily as the pipe — `serve::attach_over_http_renders_the_turn` watches a
  turn somebody else submitted. What keeps a *headless* Ink out of
  `two_renderers_one_session` is Ink's own raw-mode requirement.
- ~~Every exit code is reachable and tested.~~ **Amended: five of seven are
  reached by a command; 1 and 5 are decided and unit-tested.** 0, 2, 3, 4 and 6
  each have a run in `tests/exit_codes.rs`, and 1 is also reached by
  `orrery ext test` on a degraded extension. **5 cannot be produced from the
  command line in this build**: the only selectable provider is the fixture
  one, whose auth never says `NeedsLogin`. `exit::tests` covers all seven
  mappings.
- stdout is machine-readable for every non-interactive command.
  *(`json::json_is_parseable` and `json::stderr_does_not_pollute_stdout` for
  `run`; `cli::not_implemented_does_not_pollute_stdout` for a command that does
  nothing yet; `serve` prints endpoints and nothing else, with the session id on
  stderr. `run` without `--json` prints the final text on stdout and every other
  word on stderr, so `orrery run -p … > answer.txt` holds the answer alone.)*

## State

**Tasks 1–5, 7 (`ext`) and 10 are done; 6, 8, 9 and `session list|show` are
not, and each says against itself which unlanded plan it is waiting for.** The
harness is runnable: `orrery run -p "…"` completes a turn end to end against the
fixture provider with a real tool call, `--json` emits AG-UI frames on a clean
stdout, `orrery serve` prints endpoints that `attach` and `ORRERY_ENDPOINT` both
take, and two renderer processes draw one session at once.

Two things worth knowing beyond the tasks:

- **`cargo xtask deps-check` rule 1 now excuses a binary-only core crate for a
  `clients/` dependency.** As written the rule forbade the dependency
  00-overview's own crate table prescribes (`orrery-cli` → `client-ratatui`,
  `client-json`). The rule exists to stop a renderer ending up underneath the
  kernel, which is a property of libraries; a binary produces nothing anybody
  can link. Two fixtures pin both halves, and the same dependency from a core
  *library* is still a violation.
- **Nothing in `core/` grew a hook for the CLI.** The event stream is produced
  by wrapping the provider and the session store, both of which are already
  `Arc<dyn …>` on `ResolvedConfig`. The cost is written down in `session.rs`:
  the run id in `RUN_STARTED` is the CLI's, because `Kernel::run_turn` mints a
  `TurnId` it never hands out.

Known limits, each with its owner: no
session enumeration on `SessionStore` (plan 02); no `explain` on the policy
engine (plan 07); no layered config, so no selectable provider other than the
fixture and no `NeedsLogin` from a flag (plan 10); no persisted rich event
stream, so no `replay` (plans 02/08); no eval runner (plan 16).

## Open questions

1. **Default subcommand.** Bare `orrery` starting an interactive session is friendly; it also means a typo'd subcommand might launch a TUI. Clap can distinguish; confirm the behaviour is unsurprising.

   **Decided: keep it, and it is checked rather than assumed.** `Cli` has no
   top-level positional argument, so clap has nothing to bind an unrecognised
   word to: `orrery rnu -p hi` is `error: unrecognized subcommand 'rnu'` and
   exit 2, never a TUI. `cli::a_typo_is_not_an_interactive_session` asserts both
   the code and the message. The behaviour only becomes surprising if a
   positional is ever added to the root command — which is the thing that test
   would then fail on.

2. **Endpoint format.** `serve` prints something `attach` consumes. A URL (`http://127.0.0.1:PORT/s/<id>?token=…`) covers SSE; pipes need a different form. One string with a scheme prefix (`pipe:orrery-1234`) is probably right — decide with plan 08's `Endpoint` parser.

   **Decided: one string with a scheme prefix, and the grammar is
   `orrery_client::Endpoint`'s — there is no second parser.** `serve` prints
   `pipe:orrery-<session>` and `http://127.0.0.1:<port>`, one per line,
   most-local first; `attach` parses either with `FromStr`, and
   `ORRERY_ENDPOINT` carries the same string. Two things follow:

   - **The token stays out of the query string**, because `Endpoint` already
     puts it in the authority (`http://<token>@host:port`) for the reason
     written there: a query string ends up in logs, shell history and `ps`.
   - **The session id is not in the endpoint.** It goes on stderr as
     `orrery: session <id>`, and a client names it the way AG-UI does, in
     `threadId` / `ORRERY_SESSION`. Putting it in the URL would have meant a
     `pipe:` form carrying a session and an `http://` form carrying it
     somewhere else, which is the two-spellings problem the question is about.

3. **`session rm`.** Deleting history is a destructive operation on the one thing that cannot be reconstructed. Require a confirmation, or an `--yes` flag in non-interactive mode. Do not make it easy.

   **Decided: `--yes` is required whenever stdin is not a terminal, and there is
   no `--force`, no `--all` and no glob.** The flag is in the tree and in the
   committed help today; the command itself lands with plan 02's enumeration.
   Two details that are the whole point of the question: the prompt on a tty
   types the session id back rather than answering `y`, and a `rm` that matched
   nothing is an error rather than a no-op — "it was already gone" and "you
   named the wrong one" must not look the same.

4. **Config for the CLI itself** (default profile, default renderer) — does it live in the same five layers, or a separate CLI section? Same layers, `[cli]` table; no second config system.

   **Decided: the same five layers, a `[cli]` table, and plan 10 owns it.** The
   question answers itself — a second config system would need its own
   precedence, its own `config explain` and its own answer to "why did it pick
   that", and `config explain cli.ui` is exactly as useful as
   `config explain model`. What this wave adds is why it is not built yet: every
   flag the CLI reads today (`--provider`, `--workspace`, `--state-dir`, `--ui`)
   has no default beyond a literal in `cmd::setup`, and the moment `[cli]`
   exists those literals become layer 1. The one thing to hold to when it lands:
   a `--ui` resolved from config still loses to the tty rule for the
   *interactive* surface and still does not apply to `run`, or a config file
   starts deciding what `orrery run` prints.
