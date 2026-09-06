# 06 — Positioning: Orrery vs Orca and Grok Bot

Research date 2026-09-06. Orrery facts are read from this repository at v0.22.2 and carry `path:line`. Competitor facts come from sources reachable from this container: `github.com` and `raw.githubusercontent.com` worked, so Orca's README, its orchestration guide and its releases page are first-hand. `onorca.dev`, `x.ai`, `docs.x.ai` and most press sites were blocked by the network egress proxy, so every Grok Bot claim below is secondary reporting and is marked as such. Treat the Grok Bot pricing figures as needing confirmation before you repeat them anywhere.

## 1. These are three different products

| | Orrery | Orca (`stablyai/orca`) | Grok Bot (xAI) |
|---|---|---|---|
| Shape | Local desktop console; one PTY + one git worktree per agent; one supervision surface | Local desktop ADE; same worktree model, plus coordinator/worker orchestration, remote workers, mobile companion, embedded browser | Always-on agents on a cloud VM, signing into web apps with the user's own logins (secondary) |
| Where agents run | The user's machine (`src-tauri/src/runtime/mod.rs`) | The user's machine, or a remote Orca instance (`worker-start --on <env>`) | xAI's cloud (secondary) |
| Platforms | Windows installer; an unsigned macOS dmg is built and published but hidden from the site (`.github/workflows/release.yml:84-90`, `landing/dl-button.js:14-18`) | macOS arm64+x86, Windows, Linux AppImage/rpm, iOS, Android | Desktop and mobile clients to a hosted service (secondary) |
| Agents supported | Four: claude, codex, cursor, gemini (`src-tauri/src/agents/adapters/mod.rs:296-303`, asserted by the test at `:879-882`) | 29 named CLIs plus "any CLI agent — if it runs in a terminal, it runs in Orca" | Grok models; Grok Build is the sibling CLI |
| Licence and price | Free, closed source, private repo (`release.yml:184`) | MIT, free, no paid tier in the README | No standalone SKU; bundled with SuperGrok Heavy $300/mo, Cursor Ultra $200/mo, Cursor Teams Premium $120/seat/mo (secondary) |
| Momentum | 95 commits, one author, v0.12→v0.22 in six weeks | ~27k GitHub stars on 2026-07-24 → 62.6k on 2026-09-06; releases v1.4.191→v1.4.197 in the week to Sep 4 | Beta opened 2026-08-11 (secondary) |

**Grok Bot is not your competitor.** It is a hosted general-purpose agent roster sold through Cursor and SuperGrok subscriptions. It does not manage local git worktrees, does not drive third-party agent CLIs, and does not review diffs per branch. It competes for the same developer's budget and attention, not for the same job. Positioning against it directly would be a category error.

**Grok Build is a supply-side opportunity, not a threat.** It is xAI's terminal coding agent, and per secondary sources it supports headless operation (`-p`, `--output-format json|streaming-json`, `--json-schema`) and project-local hooks in `.grok/hooks/` that can block or defer a tool call. Two consequences for you: it is a fifth adapter Orrery could support (Orca already lists Grok), and its hook model is strictly richer than the fire-and-forget bridge Orrery built, which bears directly on the headless-versus-PTY decision the roadmap has left open (roadmap open decision 6; `questions.md` Q8).

**Orca is the competitor.** Same job, same isolation model, same four agents plus twenty-five more.

## 2. Orca has moved past where Orrery is aiming

This is the finding that matters most, and it is not visible from the README alone. Orca's orchestration guide (`skill-guides/orchestration.md`) documents a coordinator/worker model: a **Run** is a namespace and inbox, **Tasks** are work items, **Dispatches** assign tasks to agent terminals, worktrees can be parented for stacked work or created with `--no-parent` for independent fixes, and there is a nested-worker depth setting so agents can dispatch agents. It has blocking questions (`orca orchestration ask` with a coordinator `reply --id`), gates for DAG decisions, and a `worker_done --outcome succeeded|failed` protocol.

Orrery's model is entirely human-driven: the user spawns each agent by hand from a prompt or a ticket, and there is no agent-to-agent dispatch, no task graph, no inbox. That is a defensible product choice, but it means the competitive gap is widening along an axis the roadmap does not mention at all.

One genuine weakness in Orca is worth knowing: its own guide says the framework is "mail-based, not automatic merging" and documents no merge or compare tooling for parallel results. The coordinator synthesises findings by hand. That is precisely the space Orrery's native git work occupies.

## 3. Orrery's claims, tested against its own code

The stated positioning (`docs/2026-08-03-orrery-roadmap-perf-and-ide-dx.md:7-10`) is: light on RAM, light on CPU, light on tokens, with IntelliJ-grade git as the wedge, and "the AI path always shows its price before it runs."

| Claim | What backs it | What undercuts it |
|---|---|---|
| Light on RAM and CPU | Interest subscription so hidden agents cost nothing to render (`src/app/data-source/bridge.ts:56-66`); one 16 ms output frame for all agents; 1 MB scrollback ring; adaptive 5 s/20 s metrics; explicit budgets — idle ≤250 MB, 5 agents ≤600 MB, 20 agents ≤900 MB, cold start ≤300 ms (roadmap `:1027-1053`) | **No published measurement exists.** The CI harness that would enforce the budgets (A7.2) is unbuilt, `tools/perf-smoke` is hand-run, and the landing page contains no mention of RAM, CPU or tokens at all (verified: all 11 grep hits for those words in `landing/index.html` are the CSS word "keyframes"). The differentiator is an internal design intent, invisible to every prospect. |
| Light on tokens via native git | Native commit, merge with a diff3 conflict session, blame, file history, hunk revert, branch ops, fetch/pull (`src-tauri/src/git/gix_backend.rs`, `branches.rs`, `hunks.rs`); budget line "Native git op tokens: **0**" | Rebase still has no native path — it goes to the AI prompt unconditionally (`src/app/agents/agent-actions.service.ts:74-76`). |
| "The AI path always shows its price" | An estimator exists with a rate table and per-operation heuristics | It is compiled out. `COST_FEATURES_ENABLED: boolean = false` (`src/app/cost/cost-flags.ts:12`) with a Rust twin, so no user has ever seen a price. The promise is currently false in shipped builds. |
| IntelliJ-grade git | Conflict view, blame with age fade, file history, local history, commit graph strip | The project's own gap analysis lists amend, revert, reset, cherry-pick, tags and interactive rebase as absent (`docs/git-feature-gap-analysis.md`). |
| Supervise many agents from one pane | Hook bridge plus PTY heuristics feeding status, permission and activity events; peek overlay; question stepper | Fire-and-forget by design, so allow/deny are blind keystrokes; Cursor emits no permission signal; the notification feed is in-memory only. Orca has real blocking ask/reply. |

## 4. Where Orca is ahead, and why that is on purpose

Every item below is listed in Orrery's roadmap as an explicit non-goal or deferral, with reasoning:

- **Fan-out to N agents and pick the winner** — Orrery: "3× the tokens to discard two results… Not a capability gap — a deliberate difference" (`:1061-1063`).
- **Embedded Chromium design mode** — Orrery: "irreconcilable with the memory budget above" (`:1064-1066`).
- **Mobile companion** — Orrery: "a product, not a feature. It does not serve the wedge" (`:1067-1068`).
- **SSH remote worktrees** — Orrery: "Real value, high cost. Revisit after Section A and B land" (`:1069`).
- Plus, not in the non-goals list: agent-to-agent orchestration, three desktop platforms, two mobile platforms, 29 agents, an MIT licence and a 62k-star community.

The roadmap already names the strategic problem in one sentence (`:1081-1083`): "Competing on feature velocity against a daily-shipping MIT project from a private Windows-only beta is the hardest version of this fight."

That sentence is correct, and the six weeks since it was written have made it more correct: Orca roughly doubled its stars and shipped seven releases in the last week alone.

## 5. The market is consolidating around Orca

Two of the mid-tier desktop orchestrators are gone. Vibe Kanban's README now opens "Vibe Kanban is sunsetting". Crystal was deprecated in February 2026 in favour of Nimbalyst. The 2026 roundups I could reach place tools in three tiers — TUI (Claude Squad), desktop (Conductor, macOS-only; Vibe Kanban; Crystal), and platform (Orca, described as having "the widest agent-CLI coverage and remote monitoring"). Orrery appears in none of the roundups I found.

Being unlisted is the more urgent commercial fact than any feature gap. A private, Windows-only, unsigned-installer beta with a landing page that makes no differentiated claim is invisible to the buyers who are actively comparing these tools right now.

## 6. What the site actually communicates today

- The hero reads "Every agent in orbit. One core to command them." and the subhead is "Claude Code, Codex, Cursor and Gemini circle a single console — each on its own branch, all under your hand. Dispatch, observe, merge." (`landing/index.html:386-387`). Orca can make every one of those claims, for more agents, on more platforms, for free and open source.
- `llms.txt` recommends Orrery for running "several coding agents at the same time on one repository" — the same sentence Orca's README uses.
- The two claims only Orrery can make — measured footprint and token-free native git — appear nowhere on the site.
- The site links `github.com/kouji-dev/orrery` from the nav, the footer and the JSON-LD `sameAs`, but that repo is private, so every visitor who clicks gets a 404.
- The footer, the download button and the changelog bake `v0.4.0` while the product is at 0.22.2, corrected at runtime only if the visitor's unauthenticated GitHub API call succeeds.

## 7. Recommendations

Ordered by leverage, all of them cheap relative to closing any feature gap.

1. **Answer the open-source question now.** The roadmap says it must be decided before Section B because it changes what "differentiator" means. A closed, single-author, one-platform beta cannot out-ship a 62k-star MIT project on features. It can out-measure it.
2. **Publish numbers, because nobody else in this market does.** Orca makes no resource claims whatsoever, and its architecture (embedded Chromium, WebGL terminal splits) invites an unflattering comparison. Build the A7.2 harness, measure RSS and CPU at 1, 5 and 20 agents against Orca on the same machine, and put the result above the fold. This is the single highest-leverage marketing asset available to you and it costs one engineering week.
3. **Make the wedge true before advertising it.** Turn `COST_FEATURES_ENABLED` on so the AI path shows its price as promised, add a native rebase, and fix the merge behaviour flagged in Pass 1 — the wedge feature currently carries the audit's most severe candidate defect.
4. **Fix the contradictions a prospect can see in five minutes:** unbake the landing version, either sign and offer the macOS build or stop publishing it, and stop linking a private repo from a public site.
5. **Add Grok Build as the fifth adapter and use it as the headless pilot.** Its hooks can block a tool call, which would let Orrery's allow/deny become real approvals rather than keystrokes typed into a PTY after a 1.8 second guess.
6. **Do not chase fan-out, mobile, design mode or SSH.** Those decisions are sound and documented; the audit found no reason to reopen them. The gap that should worry you is agent-to-agent orchestration, which the roadmap has not considered at all.

## 8. Confidence and gaps

- Orca facts: first-hand from its README, orchestration guide and releases page on 2026-09-06. Its tech stack is not stated publicly; my inference that it is Electron-based rests on its feature set, not on evidence, and should not be repeated as fact.
- Grok Bot and Grok Build: secondary sources only, because xAI's own domains were blocked from this container. The pricing figures in particular should be re-checked before use.
- No competitor was installed or benchmarked. Every performance comparison above is a claim about what is measurable, not a measurement.
