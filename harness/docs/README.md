# Orrery Harness — documentation

**Orrery Harness** is a Rust agent runtime: the thing Claude Code and Codex are, but composable — the loop is assembled from declared parts, extensions run out of process in any language, permissions are enforced in the core rather than configured around it, and benchmarks are a built-in capability instead of a throwaway script.

It is a **separate product from Orrery ADE** (the Tauri + Angular app in `ade/`). The ADE orchestrates *other people's* agent CLIs; the harness *is* an agent runtime. The ADE will eventually become one client of this runtime, but nothing here depends on that.

## Read in this order

| | File | What it gives you |
|---|---|---|
| 1 | [`architecture.md`](architecture.md) | The specification. 8 sections, the source of truth for every decision. Copied verbatim and versioned here so plans can cite its headings. |
| 2 | [`plans/00-overview.md`](plans/00-overview.md) | The Rust translation: crate map, dependency graph, cross-cutting decisions, where the spec does not survive contact with Rust, and the milestone table. |
| 3 | the plan for whatever you are building | One file per architecture section. Each is a standalone task list. |

## Plans

Repo work (do these first, in order):

| File | What it does |
|---|---|
| [`plans/00a-restructure-ade.md`](plans/00a-restructure-ade.md) | Move today's app under `ade/`, rename to `orrery-ade`, open `harness/`. Names and paths only — the bundle identity never changes. |
| [`plans/00b-scaffold-workspace.md`](plans/00b-scaffold-workspace.md) | The root cargo workspace and every crate as an empty compiling stub. |

Then, by build phase (see §8 of the architecture and the milestone table in `00-overview.md`):

| Phase | Files |
|---|---|
| 1 | `01-proto-shared-types`, `02-session-store`, `03-provider-layer`, `05-kernel-loop`, `06-extension-host`, `08-protocol-transport`, `09-surfaces`, `09b-client-ratatui`, `09c-client-ink`, `17-cli` |
| 2 | `04-tool-registry`, `14-wasm-wit`, `18-writing-an-extension` |
| 3 | `07-policy-broker-audit` |
| 4 | `09*` completed |
| 5 | `10-config-layers` |
| 6 | `11-router-roles-orchestrator`, `12-memory` |
| 7 | `13-skills-mcp` |
| 8 | `15-registry-supply-chain` |
| 9 | `16-eval-runner` |

## How to use a plan file

Each plan has the same shape:

- **Goal** — one paragraph, what exists when it is done.
- **Covers** — the architecture sections it implements.
- **Architecture** — the Rust shape: crates, modules, the public types and trait signatures.
- **Constraints** — the cross-cutting decisions from `00-overview.md` that bind this plan, and the spec-to-Rust translations it owns.
- **File structure** — create / modify.
- **Tasks** — numbered, each with its files, its interfaces, and `- [ ]` steps. **The first step of every task writes a failing test.**
- **Done when** — the acceptance line, usually the §8 row.
- **Open questions** — decided by the person executing, recorded back into the file.

Plans reference each other by file name. Nothing says "see above" across files.

## Conventions

- **Never run a full test suite.** Run the crate you touched: `cargo test -p orrery-proto`. Typechecks (`cargo check`) are cheap — run those freely.
- `cargo` runs from the repo root. `default-members` excludes the Tauri app, so a bare `cargo test` never builds the ADE.
- Commits wait for review. Do not commit because a task finished.
- One plan file per session where possible. They are sized to fit.
