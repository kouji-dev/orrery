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

orrery install <source> [--to user|workspace] [--user] [--link] [--yes]
orrery remove <name> [--from user|workspace]
orrery ext list | test [path]
orrery registry init | add | sign | verify   # author the pin set phase 8 enforces
orrery auth login | logout | status [<provider>]  # the device-code flow, phase 5's
orrery permissions explain <call>
orrery config explain <key>
orrery init [--profile <name>]
orrery import [--from claude-code|codex]

orrery workflow check <file>             # typecheck one; starts no kernel
orrery workflow run <file> [--max-tokens N]

orrery eval run <suite> [--profile a,b] [--model x,y] [--format junit]
orrery eval compare <run-a> <run-b>
orrery eval replay <run> --case <id>

orrery ledger | telemetry                # the §4.12 streams, queryable
```

**Amended, and decided with the user: install and remove are bare top-level
verbs.** They were `orrery ext install` / `orrery ext remove`. Installing an
extension is an everyday action, so it gets a verb of its own, Pi-style; what
stays under `ext` is what is not everyday. Plan 15's Architecture section is
where the source forms and the layer rule are written down. Two details that
belong here rather than there:

- **The layer flag is `--to <user|workspace>`, not `--workspace`.** `--workspace
  <PATH>` is already in the global table below, and clap will not let one long
  name mean two things. `--user` is the bare shorthand.
- **`orrery ext install` no longer parses**, and
  `install::install_and_remove_are_no_longer_under_ext` asserts it, so the move
  is a fact rather than an intention.

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
- `harness/core/crates/orrery-cli/src/cmd/{run,serve,attach,replay,session,ext,permissions,config,init,import,eval,ledger,registry}.rs`
- `harness/core/crates/orrery-cli/src/{args,ui,exit,term}.rs`
- `harness/core/crates/orrery-cli/tests/{cli,exit_codes,json}.rs`

**Also landed, and not in the list above.** `src/session.rs` (the kernel, the
hub and the two wrappers that turn a running turn into events), `src/control.rs`
(the kernel's side of the control RPC, one implementation for all three
listeners), `src/render.rs` (the drain loop both in-binary renderers share) and
`src/cmd/interactive.rs` (bare `orrery`). `src/cmd/layers.rs` (the five config layers, for the commands that answer from
files rather than from a kernel). `tests/{serve,ext,explain,init,session}.rs`
and `tests/common/mod.rs` alongside the three the plan named.

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

- [x] **Failing test first.** ~~One test per code: a fixture that exceeds budget ⇒ 3; a denied call with no fallback ⇒ 4; a provider reporting `NeedsLogin` ⇒ 5.~~ **Amended: 3 and 4 are reached by a run; 5 is not reachable from the command line in this build.** `exit_codes::a_ceiling_is_three` (the last stream repeats, so a fixture that only asks for tools never stops asking and the turn budget ends it) and `exit_codes::a_denial_with_no_fallback_is_four` (a hand-written stream reading outside the workspace, refused by the broker) are real runs. **5 is not:** every provider this build can select is the fixture provider, whose auth never says `NeedsLogin`, and the only way to reach it from a flag would be a "signed out" provider spec that exists for the test and for nothing else. The mapping is decided and tested in `exit::tests::needing_a_login_is_five`; the *command* cannot produce it until a provider that can be signed out is selectable (plan 10). **Amended in place 2026-09-19: 5 is reachable now.** `orrery auth status` exits 5 when this machine is not signed in and 0 when it is, so a script asking "can I run?" gets the same code from the check as from the turn; `auth::login_then_status_then_logout` asserts both, through the binary. A turn with `--features anthropic` and no credential also exits 5, and its message names a command that now exists.
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

- [x] **Failing test first.** `explain::permissions_names_rule_layer_file` — output contains the rule, the layer, the file and the line. ~~**Not landed: plan 07 task 10 has not.**~~ **It had, and the note was stale.** `PolicyEngine::explain` shipped with plan 07; what actually kept this unbuilt was duller — `orrery-cli` did not depend on `orrery-policy` at all. The command reads the call in the **rule grammar** (`read(./src/main.rs)`, `net(domain: docs.rs)`), which is the spelling `PendingCall::match_text` prints, so what a person types to ask is what they read back in the audit.
- [x] `explain::config_names_the_winning_layer` (plan 10 task 7). ~~**Not landed: plan 10 has not.**~~ **Stale as well: plan 10 landed 2026-09-18.** The command resolves the five layers with `orrery_config::resolve` and prints `ResolvedConfig::explain` — the winner **and** what it shadowed, because naming only the winner is how somebody spends an afternoon editing a file that is not in force.
- [x] Implement; both support `--json`. *(One JSON object on one line each. Neither command builds a kernel, a store or a provider: explaining a rule has to work in a checkout with no key in sight, and `tests/explain.rs` runs exactly that — sandboxed `HOME`, no `--provider`.)*
- [x] **Round 7, amended in place: the explanation now describes the rules that
  actually decide.** It did not. `cmd::setup` set nine fields on the harness
  config and never `policy_toml`, which had no writer anywhere in the tree, so
  every run dispatched through the hardcoded default while this command read the
  layers: `permissions explain 'read(./Cargo.toml)'` printed `Deny ... [User]
  config.toml:2` and the same binary's `run` read the file, with the ledger
  recording `allow`. `cmd::layers::rules` is now the one place the rule set is
  chosen — layers, plus the profile's shorthands under `--profile` — and both
  this command and `Session::build` take it. `tests/permissions_enforced.rs`
  asserts the **equivalence** rather than two separate facts: for one call under
  one set of layers, explain-says-deny ⇒ exit 4 with the denial in the ledger,
  explain-says-allow ⇒ exit 0 with the read performed.
- [x] **Round 8, amended in place: the command can ask about a sub-agent, and
  no longer names a file that is not there.** It took a call and assumed
  `Subject::Agent`, so the one disagreement that mattered — `explain` and
  `check` answering differently for `agent:<name>`, which emptied the offered
  tool list — could not be seen from outside the binary at all.
  `--subject <agent|agent:<name>|ext:<id>>` asks the real question, and the
  answer says when a verdict was inherited. Two attributions were fiction as
  well: the built-in `DEFAULT_PERMISSIONS` were attributed to
  `<workspace>/orrery.toml`, sending anyone who went looking to a file that does
  not exist, and a profile shorthand reported an empty file on line 0. The
  default now names itself (`<built-in default>:2`, the line within that text)
  and a shorthand carries its origin. `explain::{a_built_in_default_says_so,
  a_sub_agent_can_be_asked_about, a_profile_shorthand_names_where_it_was_written}`.
- [x] **And the help text names a call the parser accepts.** `--help` offered
  `builtin.write:$WORKSPACE/src/**`, which this command rejects as "is not a
  call". It now shows `read(./src/main.rs)`, `write(./src/main.rs)`,
  `spawn(cmd: git)` and `net(domain: docs.rs)`, and
  `permissions_enforced::every_example_in_the_help_parses` reads the examples
  back out of `--help` and runs each one, so the two cannot drift again.

**Decided while implementing: an untrusted workspace says so, on stderr.** A
person running either command in a workspace whose own `.orrery/config.toml` is
not in force gets one stderr line saying why; an answer that silently ignores
the file in front of them is worse than no answer. stdout is untouched.

### Task 7 · `ext` and `session`

Files: `src/cmd/{ext,session}.rs`

- [x] **Failing test first.** `ext::test_runs_without_a_model` — `orrery ext test` on a fixture extension passes with no network and no provider (plan 06 task 8). *(Plus `ext::a_broken_manifest_is_usage`: exit 2 naming the file, never a panic.)* **Amended, round 5:** `ext test` accepted only a path, so `orrery ext test builtin` — the obvious first thing to type — failed with `could not read builtin\orrery.toml`. It now resolves a **name** as well: compiled-in first, then installed, and a name nothing answers to is a usage error that says where it looked (`ext::test_takes_the_name_of_a_compiled_in_bundle`, `ext::an_unknown_name_says_where_it_looked`).
- [x] `orrery install` / `orrery remove` (plan 15). *(`tests/install.rs`: a local
  path into the user layer and into the workspace, the grant diff rendered from
  the `Surface` itself, an unanswered install granting nothing and saying what
  it lost, an ambiguous `remove` refusing to guess, and a bare registry name
  naming the index rather than falling through to crates.io. Sandboxed home, no
  network. **What is not here: fetching a remote index**, which is a `net` call
  and therefore needs a broker and a session; `--index <path>` and a managed
  `[registry]` pointing at a file both work today.)*
- [x] `ext::list_shows_the_ledger` — ~~including `degraded` and `skipped` entries with reasons.~~ **Amended: `ok` and `degraded` with reasons; `skipped` is not producible here.** ~~`ext list` reports every first-party extension this build compiled in~~ **Amended again, round 5: the compiled-in set was all it reported, which made the command useless the moment anybody installed anything.** It now reports the compiled-in set *and* the set plan 10's discovery found in the layers in force, each line naming its layer — `ext::list_shows_an_installed_extension` installs one with the real `orrery install` and then finds it. Both halves run through `orrery_ext_api::testing::missing` — the same function the real host calls — so a degraded line says the words a session would. `Skipped` is a *loader* state (a deny rule, a disabled extension), and reaching it needs the policy engine, which means a session, which means a model. When `ext list` can take a live session's ledger (plan 07's `query extensions`), `skipped` comes with it.
- [x] `session::list_and_show`. ~~**Blocked, and not on this plan:**~~ **True, and unblocked by building it.** `SessionStore` really did have no enumeration, so `list_sessions` landed on the trait with this command — id, workspace, profile, `created_at` and turn count, newest first — implemented in `orrery-ext-session-sqlite` as one correlated subquery, with `conformance::sessions_can_be_enumerated` making every backend provide it. `session show` is the `materialise` it was always going to be. `tests/session.rs` runs both against history the binary itself wrote from a fixture stream.
- [x] `session rm`. ~~**Blocked, re-checked 2026-09-19 and still true:** there is no delete on `SessionStore`.~~ **Unblocked by answering the question rather than waiting on it.** Plan 02 open question 2 was parked, not undecidable, and the note that parked it already contained the answer: retention must drop whole sessions, never trim turns. So `SessionStore::delete(session)` landed on the trait with this command, implemented in `orrery-ext-session-sqlite` as one transaction over compactions, turns, events, branches and the session row, with `conformance::a_session_can_be_deleted` making every backend provide it. `--yes` is still required, and refusing without it is `cli::session_rm_without_yes_refuses`; `session::rm_deletes_one_session_and_leaves_the_other` runs two real fixture turns, deletes one session and checks the other still shows its transcript.
- [x] Implement.

### Task 8 · `replay`, `ledger`, `telemetry`

Files: `src/cmd/{replay,ledger}.rs`

- [x] **Failing test first.** `replay::renders_a_past_session` — replay into the json renderer and diff against the original event stream. ~~**Not landed… the sqlite backend writes exactly one event per turn.**~~ **The diagnosis was right and was the design.** Verified again before building: the backend really does persist only `turn.settled`, so replaying from `events_since` would draw a run with no text in it. So replay re-encodes the **turn rows** — `SessionStore::turns(branch)` landed with this command — through the same `Publisher` a live turn narrates through, into the same `Hub`. Nothing here builds a kernel or a provider, so `replay` needs no `--provider`, which `replay::replay_needs_no_provider` pins. Plan 08 open question 3 is amended in place to say the same thing. The one accepted loss is delta boundaries: a replayed turn emits one `TEXT_MESSAGE_CONTENT` where the live turn emitted several, so the test compares the two streams with runs of one kind collapsed and everything else exactly.
- [x] Implement. ~~`ledger` and `telemetry` are `query` frames rendered as tables. **Not landed: the `query` frame still has no handler**, and the audit stream plan 07 writes has no reader.~~ **Half of that was true and the other half was the wrong design.** The reader really was missing and now exists — `orrery_audit::scan` with a `Query` over stream, subject, rule and a tail limit, seven unit tests. But routing it through a `query` **frame** was wrong: reading what a *past* session decided must not need a running kernel to ask, exactly as `session list` must not. So both commands read the file directly, and neither builds a kernel. Two things had to land with them: the CLI now writes an audit stream at all (it was passing `orrery_audit::null()`, so "every decision is logged" was logging into a bin), one file per session under `<state-dir>/audit/`, and the sink buffers until `create` names the session so nothing before that is lost. `ledger` and `telemetry` are the same scan split by `AuditEvent::stream()`. `tests/ledger.rs` runs real fixture turns and reads back the decisions those turns actually made. **Amended round 7: `ledger` read one stream and there are two decision streams.** `AuditEvent::ExtensionLoad` is `Stream::Load`, `ledger` named `Stream::Audit`, and no command in the binary named `Load` at all — so a run that refused to load an extension wrote the refusal, `ext list` printed it, and the one operator command for "what was decided" said "nothing matched". `Query.stream: Option<Stream>` is now `Query.streams: Streams`, and `Streams::Decisions` asks `Stream::is_decision()`, the single definition of the word; `--stream <load|audit|telemetry>` narrows when **retention** is the question, which is the only thing the file split is about. `ledger::a_refused_load_is_in_the_ledger`.

### Task 9 · `init`, `import`, `eval`

Files: `src/cmd/{init,import,eval}.rs`

- [x] Thin wrappers over plan 10. ~~**Not landed: there is nothing to wrap.**~~ **Stale for `init` and `import`: plan 10 landed.** `orrery init` writes `<workspace>/.orrery/config.toml` from `orrery_config::import::init`, shorthands expanded so the file says what it does, and **refuses to overwrite** one that is already there. `orrery import --from claude-code|codex` finds the foreign file (workspace first, then the home directory), maps it, and prints the config on **stdout** with every unmapped note on **stderr** — so `orrery import --from codex > .orrery/config.toml` writes a file that parses and nothing is dropped in silence. `tests/init.rs` covers all four paths. **Amended round 7: with no `--profile` there was nothing to wrap, and it showed.** `orrery init` wrote a 49-byte file whose entire content was a comment with an empty profile name interpolated into it, and then said "review it before you use it" — the first command a new user runs. It now writes a documented starter configuration: the five layers and how to ask which one won, a `[permissions]` table denying `creds`, asking for `write`/`spawn`/`net` and allowing `read`, and a commented profile to copy. The rule texts come from `orrery_config::profile::SHORTHANDS`, so the starter cannot come to hold a rule the parser rejects; when profiles *are* defined and none was named, stderr names them. `init::init_with_no_profile_writes_a_starter_config`.
- [x] `orrery eval`. ~~**Blocked: `orrery-eval` is still a stub.** … it belongs with plan 16's own landing wave rather than being raced from here.~~ **Neither plan built it, so it was built here.** The estimate — three functions of wiring — held. `run` loads a suite, expands the matrix, binds a `HarnessRunner` per point over one shared store, installs the built-in graders and writes the report to `<state-dir>/eval/<run-id>.json`; `compare` and `replay` read it back by run id or by path. `--format text|json|junit`, and a red suite exits non-zero through `junit::exit_code` so CI and the command cannot disagree. `tests/eval.rs` drives the binary end to end against a committed fixture stream. Ticked in plan 16 as well; the stale `not_implemented("16-eval-runner.md")` is gone from the tree.

### Task 10 · Terminal hygiene

Files: `src/term.rs`

- [x] **Failing test first.** `term::panic_restores_the_terminal` — force a panic in the TUI path; assert raw mode is off afterwards. *(The raw-mode **flag** is what is asserted, not a real tty: a test process has no terminal to put into raw mode. What could break is the bookkeeping — a guard that does not fire, a hook that never installed — and both are exercised for real, through `std::panic::catch_unwind` around the actual guard.)*
- [x] ~~`term::ctrl_c_during_startup` — interrupting before attach exits cleanly.~~ **Amended: landed as `term::restoring_twice_is_fine`.** Nothing in this build installs a `^C` handler — the default disposition kills the process — so "interrupting before attach" is exactly "the process died before `setup` ran", and what has to be true is that there is nothing to undo and that undoing it anyway is safe. That is what is asserted. A `^C` that cancels a *turn* rather than the process is a different feature and belongs with the interactive loop.
- [x] Implement a guard that restores on drop and on panic. *(Both: a `RestoreGuard` whose `Drop` restores, and a panic hook installed at `main` that restores **before** the default hook prints, so the backtrace lands in a terminal that can render it.)*

### Task 11 · `mcp` and `skills`

Files: `src/cmd/{mcp,skills}.rs`

**Added 2026-09-19.** This task was not in the plan and should have been: this
file is the command tree of record, `orrery-mcp` and `orrery-skills` were both
built and tested, and **neither was in the binary's dependency closure at all**,
which is the whole reason section 8 phase 7 read PARTIAL. A capability nothing
can reach is not shipped.

- [x] **Failing test first.** `mcp::list_shows_what_configuration_declares` — a `[mcp_servers.jira]` in a config layer is listed with its extension id `mcp.jira`, the layer that declared it, and health `discovered`. *(The id is the assertion that matters: §4.11 says an MCP server is an extension id like any other, and if the command invented an MCP-shaped name here there would be two namespacing schemes.)*
- [x] `orrery mcp list` starts **no process**. Discovery and connection are separate subcommands because §4.11's lifecycle — discovered at session start, connected when first needed — is a property, not a sentence. `Servers::started()` is zero after a list, and the command asserts it.
- [x] `orrery mcp tools <server>` connects to one server and prints the namespaced `ToolRef` a policy rule would name. *(**What the CLI suite does not cover: a successful handshake.** That is `orrery-mcp`'s own `client::real_server_works_unmodified`, which spawns the committed spec-conformant fixture server — a bin `cargo test -p orrery-cli` does not build. What is covered here is everything the command adds: the declaration is found, an undeclared server is exit 2 pointing at `mcp list`, and a server that will not start fails with a sentence rather than a hang.)*
- [x] `orrery skills list [--agent <name>]` — name, layer, and **whether its scripts may run at all**. That column is the point: everywhere else a skill's bundled `scripts/` run with the user's full privileges, and the absence of a grant needs to be a visible, ordinary state. A skill scoped to another agent is *absent* rather than listed and refused, which is what `SkillSet::for_agent` already meant.
- [x] `orrery skills show <name>` — the front matter and the body an agent would be handed.
- [x] Scope and grant are read from **configuration**, never from the front matter. The `SKILL.md` format is adopted unchanged, so a skill cannot widen its own reach by editing itself; `skills::a_scoped_skill_is_absent_from_another_agent` declares the scope in `[skills.<name>]` and proves it.
- [x] Neither command builds a kernel or a provider, for the same reason `config explain` does not: asking what is declared must work in a checkout with no key in sight.

### Task 12 · `workflow`

Files: `src/cmd/workflow.rs`

**Added 2026-09-19.** Not in the plan, and it should have been: this file is
the command tree of record, and `orrery-router` and `orrery-orchestrator` were
absent from `cargo tree -p orrery-cli` altogether. `run -p` submits exactly one
turn and has nowhere to put a workflow file, so plan 11's loop machine, its
caps and every routing rule were green library tests a person could not reach.

- [x] **Failing test first.** `workflow::a_verify_loop_terminates_on_its_own_cap` — against the fixture provider, a loop whose predicate never holds stops at `max_iterations`, names the cap and the iteration count on stdout, and exits **0**. A capped loop is an ordinary ending, not a failure and not a hang. This is plan 11's headline criterion, driven from the binary rather than from `orrery-orchestrator/tests`.
- [x] `workflow check <file>` starts **no kernel and no provider**, for the same reason `config explain` and `mcp list` do not: asking whether a file is well-formed must work in a checkout with no key in sight. It is also where translation #5 becomes a product property — `workflow::an_invalid_workflow_fails_at_load` gets exit 2 naming the offending step, with **nothing on stdout**.
- [x] A `tool` step makes no model call. `workflow::a_tool_only_workflow_needs_no_model` asserts zero tokens and the file's real bytes in the step's value.
- [x] A sub-agent's work is turn rows on its **own branch**, and the parent wrote the join. `workflow::a_sub_agent_leaves_its_own_branch_behind` reads the tree back through `orrery session show`: four branches for three iterations, one join row each.
- [x] `[[route]]` rules are read from the workspace's own `orrery.toml`, beside `[permissions]`. `workflow::a_declared_route_rule_reaches_the_run` denies the sub-agent rung there and finds the router's own words in the step's failure — the router in the product, not in its own test suite.
- [x] Exit codes: completed **0**, a budget ceiling **3**, a gate or a failed step **1**. A capped loop is `Completed`, so it is 0.

---

## Done when

- `cargo test -p orrery-cli` green; `--help` snapshots committed. *(Unit tests,
  and `cli`, `json`, `exit_codes`, `serve`, `ext`, `explain`,
  `permissions_enforced`, `init`, `session`, `replay`, `ledger`, `eval`, `mcp`
  and `skills` as integration suites. 32 snapshots under `tests/snapshots/`.)*
- **What the binary reports and what the binary enforces are the same answer.**
  Added round 7, because it was not true and no library test could have caught
  it: `permissions explain` and `run` were built from two different rule sets.
  They are built from one now, and `permissions_enforced` asserts the
  equivalence directly rather than asserting each half separately.
- **`ext list` and `ext test` report the state the run path reaches.** Added
  round 6, because `ext list --help` promised "including degraded and skipped
  ones with reasons" and the command printed `ok` for extensions the session
  skipped. Both go through `orrery_harness::plan::skip_for` — the same call the
  builder makes before handing a manifest to a host — so the listing cannot
  drift from the loader, and `ext test` starts the guest for any runtime this
  build hosts. `ext.rs`, three tests driving install-then-list; a native
  extension on disk now says `skipped` and why.
  **Amended round 7: the one case left.** `ext list` still printed a bare `ok`
  for an rpc extension whose *guest* failed at `ext/load` — the run path logged
  `Failed { stage: Activate }` and the turn answered no-such-tool for both its
  tools — because a listing checks the manifest and `skip_for` and never boots
  anything. The status word is now derived from a `Checked` value saying how far
  the caller looked: `ext list` can only pass `ManifestOnly`, and only a caller
  holding a real `LoadOutcome` can print `ok`.
  `ext::list_does_not_claim_a_guest_it_never_started`.
  **Amended round 10:** `ManifestOnly` printed
  `ok (manifest only; guest not started)`, and the hedge did not save it — the
  status *column* is what a person scans, it led with `ok`, and an extension
  `ext test` had just proved broken rendered identically to a working one. The
  word is now `unchecked`. `ext::an_unchecked_extension_does_not_scan_as_ok`.
  **Also round 10:** the command that line tells you to run gave false failures.
  `ext test <NAME>` meant a *path* whenever a directory of that name happened to
  sit in the process cwd, and the host then joined the relative root again — from
  the directory it was installed from, `orrery ext test pyext` answered
  `Activate: can't open file '…\ws\pyext\pyext\main.py'` while the same
  extension from anywhere else answered `pyext 0.1.0 ok`. A name and a path are
  now told apart by how they are **written** (`./x`, `../x`, `sub/x`, an absolute
  path or a `.toml` file is a path; anything else is a name), and a path leaves
  `resolve` absolute so nothing downstream can join it twice.
  `ext::a_name_is_not_shadowed_by_a_directory_of_the_same_name`,
  `ext::ext_test_answers_the_same_from_any_directory`.
- **Three things the binary said that were not so, fixed 2026-09-19**, each found
  by driving it rather than by a test: `install --yes` printed "1 capability
  request(s) were denied by default; pass --yes to grant them" **after** `--yes`
  had been passed and then succeeded — it counted the defaults rather than the
  decision; `ext test` with no target read the process cwd and ignored the global
  `--workspace <PATH>`, so it only worked after a `cd`; and `config explain
  permissions` said "not set in any layer" for a shorthand `permissions explain`
  and `run` both enforced. Tests: `install::passing_yes_does_not_then_ask_for_yes`,
  `ext::test_with_no_target_honours_the_workspace_flag`,
  `explain::config_explain_surfaces_a_profiles_permission_shorthands`.
- **A call that reached no tool is still a logged decision.** A model naming a tool
  it was not offered settles as `no-such-tool` and the turn exits 0 — which is
  right: the model wrote the name, the failure goes back to it as a value, and the
  next pass recovers. The silence was not: the ledger was empty about the one thing
  that had happened. The kernel now writes a failed `tool.call` line before it
  settles the call (`ledger::a_tool_name_that_resolved_to_nothing_is_still_logged`).
  The exit code is deliberately unchanged.
- **`orrery registry`, added round 7: the admin could not make what the admin is
  told to pin.** Phase 8 says an admin pins a version set; `install.rs` says
  fetching a remote index is not implemented, and nothing in the binary could
  create, sign or publish a local one. `registry init|add|sign|verify` is that
  missing half, offline, staging from the same mirror an install fetches from.
  `tests/registry.rs` drives the whole workflow and then installs from what the
  binary produced. Publishing an index is still not shipped, and plan 15 says so
  in its Architecture section rather than implying a workflow that does not run.
- **Every command in `--help` answers for itself.** As of 2026-09-19 nothing in
  the tree exits 2 saying "not implemented in this build";
  `cli::no_subcommand_is_a_stub` reads the command list out of `--help` and
  checks each one, which is the property the old list of unimplemented
  subcommands existed to protect.
- **`--provider` reaches every provider a `[provider]` table can name.** Round
  5 left the flag parsing exactly one prefix, `fixture:`, while
  `orrery_harness::config::provider_choice` could name Anthropic and any
  OpenAI-compatible endpoint — and `Session::build` refused to start at all
  unless fixtures had been passed, so a configured provider could never run a
  turn. `<kind>:<model>[@<base-url>]` now covers all of them, only `fixture:`
  repeats, an unknown word prints the forms, and with no flag the table in force
  decides. `orrery_harness::provider_for` is the single selector both `assemble`
  and the CLI go through, so neither can grow an arm the other lacks.
  `tests/provider.rs`, six tests. Both TLS-linking providers stay **off by
  default** and are reachable through this crate's own `anthropic` and
  `openai-compat` passthrough features — which had to be added, because until
  round 6 the error named a feature no `cargo build` of this package could set.
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

**Tasks 1–7 (including `orrery install` and `orrery remove`), task 9's `init`
and `import`, and task 10 are done; task 8, `orrery eval` and `session rm` are
not, and each says against itself what it is waiting for and why.** The
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

Known limits, each with its owner and each **re-checked on 2026-09-19 rather
than inherited**: no **delete** on `SessionStore`, so no `session rm` (plan 02,
open question 2 — the trait still has `create`, `open`, `list_sessions`,
`lease`, `append`, `branch`, `close_branch`, `materialise`, `compact` and
`events_since`, and nothing that removes); no persisted rich event stream and no
`query` handler, so no `replay` and no `ledger`/`telemetry` (plans 02/07/08 —
`KernelControl` still refuses `query` by name); and no selectable provider other
than the fixture, so no `NeedsLogin` from a flag. **Two limits on this list are
gone:** the passive HTTP subscribe landed with plan 08, and the eval runner
landed with plan 16 — only its three-function CLI wiring is outstanding, and
plan 16 owns it.

**Also landed this wave:** `orrery install <source>` and `orrery remove <name>`,
with `orrery-registry` behind them. `tests/install.rs` runs six of them against
a sandboxed home with nothing on the network.

**Corrected 2026-09-19: every run in this crate's tests is sandboxed, not just
the ones that remembered.** `tests/budget.rs` and `tests/install.rs` pointed
`HOME`, `USERPROFILE` and `%ProgramData%` inside a temporary directory; the
shared `common::orrery` set `COLUMNS` and nothing else, so the other fourteen
test files resolved their configuration against the developer's real
`~/.orrery` and the machine's real `%ProgramData%\Orrery\managed.toml` — a
managed `deny` on a CI box would have changed what they assert, silently. The
helper now sandboxes, `common::sandbox_home` names the directory, and
`tests/sandbox.rs` pins it from the other side: a config written into the
sandbox home is the one the binary answers from.

**Three limits listed here were no longer true, which is the point of writing
them down.** The policy engine had grown `explain`, the layered config had
landed, and `SessionStore` could be taught to enumerate. What actually kept
tasks 6, 7 and 9 unbuilt was narrower and duller than the plans they blamed:
`orrery-cli` did not depend on `orrery-config` or `orrery-policy` at all. A
blocker is worth re-reading before it is quoted.

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
