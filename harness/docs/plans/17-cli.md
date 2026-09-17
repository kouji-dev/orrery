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

---

## Tasks

### Task 1 · Argument shape and help

Files: `src/{main,args}.rs`, `tests/cli.rs`

- [ ] **Failing test first.** `cli::help_lists_every_command` — a snapshot of `--help` and of each subcommand's `--help`, so the surface cannot change silently.
- [ ] `cli::unknown_flag_is_exit_2`.
- [ ] Implement the clap tree. Every not-yet-implemented subcommand is a `todo!` that exits 2 with "not implemented in this build", naming its plan file — so the tree is complete from phase 0 and fills in.

### Task 2 · Renderer selection

Files: `src/ui.rs`

- [ ] **Failing test first.** `ui::defaults_to_json_without_a_tty` — piping the output selects the json renderer.
- [ ] `ui::ink_requires_node` — with no `node` on PATH, `--ui ink` exits with a clear message, not a spawn stack trace.
- [ ] Implement.

### Task 3 · `run`

Files: `src/cmd/run.rs`, `tests/json.rs`

- [ ] **Failing test first, and it is the phase-1 criterion.** `run::completes_a_turn_with_a_tool_call` — against the fixture provider, `orrery run -p "…"` prints final text and exits 0, having dispatched a real tool.
- [ ] `run::json_is_parseable` — every stdout line is valid JSON and matches the `Event` schema.
- [ ] `run::stderr_does_not_pollute_stdout` — with `-vv`, stdout is still clean JSONL.
- [ ] Implement.

### Task 4 · `serve` and `attach`

Files: `src/cmd/{serve,attach}.rs`

- [ ] **Failing test first, and it is the "no privileged client" proof.** `serve::two_renderers_one_session` — start `serve`, attach ratatui and the Ink client, submit one turn, assert both render it.
- [ ] `serve::outlives_a_client` — attach, kill the client mid-turn, re-attach with `--since`, assert the turn completed and replays.
- [ ] Implement; print the endpoint in a form `attach` and `ORRERY_ENDPOINT` both accept.

### Task 5 · Exit codes

Files: `src/exit.rs`, `tests/exit_codes.rs`

- [ ] **Failing test first.** One test per code: a fixture that exceeds budget ⇒ 3; a denied call with no fallback ⇒ 4; a provider reporting `NeedsLogin` ⇒ 5.
- [ ] Implement the mapping from `TurnOutcome` and `KernelError`.

### Task 6 · Explain commands

Files: `src/cmd/{permissions,config}.rs`

- [ ] **Failing test first.** `explain::permissions_names_rule_layer_file` — output contains the rule id, layer, file and line (plan 07 task 10).
- [ ] `explain::config_names_the_winning_layer` (plan 10 task 7).
- [ ] Implement; both support `--json`.

### Task 7 · `ext` and `session`

Files: `src/cmd/{ext,session}.rs`

- [ ] **Failing test first.** `ext::test_runs_without_a_model` — `orrery ext test` on a fixture extension passes with no network and no provider (plan 06 task 8).
- [ ] `ext::list_shows_the_ledger` — including `degraded` and `skipped` entries with reasons.
- [ ] `session::list_and_show`.
- [ ] Implement.

### Task 8 · `replay`, `ledger`, `telemetry`

Files: `src/cmd/{replay,ledger}.rs`

- [ ] **Failing test first.** `replay::renders_a_past_session` — replay into the json renderer and diff against the original event stream.
- [ ] Implement; `ledger` and `telemetry` are `query` frames rendered as tables.

### Task 9 · `init`, `import`, `eval`

Files: `src/cmd/{init,import,eval}.rs`

- [ ] Thin wrappers over plans 10 and 16. Each with one smoke test.

### Task 10 · Terminal hygiene

Files: `src/term.rs`

- [ ] **Failing test first.** `term::panic_restores_the_terminal` — force a panic in the TUI path; assert raw mode is off afterwards.
- [ ] `term::ctrl_c_during_startup` — interrupting before attach exits cleanly.
- [ ] Implement a guard that restores on drop and on panic.

---

## Done when

- `cargo test -p orrery-cli` green; `--help` snapshots committed.
- `orrery run -p "…"` completes a real turn with a real tool call.
- `orrery serve` + ratatui + Ink on one session renders in both.
- Every exit code is reachable and tested.
- stdout is machine-readable for every non-interactive command.

## Open questions

1. **Default subcommand.** Bare `orrery` starting an interactive session is friendly; it also means a typo'd subcommand might launch a TUI. Clap can distinguish; confirm the behaviour is unsurprising.
2. **Endpoint format.** `serve` prints something `attach` consumes. A URL (`http://127.0.0.1:PORT/s/<id>?token=…`) covers SSE; pipes need a different form. One string with a scheme prefix (`pipe:orrery-1234`) is probably right — decide with plan 08's `Endpoint` parser.
3. **`session rm`.** Deleting history is a destructive operation on the one thing that cannot be reconstructed. Require a confirmation, or an `--yes` flag in non-interactive mode. Do not make it easy.
4. **Config for the CLI itself** (default profile, default renderer) — does it live in the same five layers, or a separate CLI section? Same layers, `[cli]` table; no second config system.
