# Agents

**Context.** An agent is an isolated execution unit inside a project. Each agent has its own
coding tool (Claude/Codex/Cursor/Gemini), model (and reasoning effort for Codex), task prompt,
git branch + worktree, conversation, terminal stream, changed‑files set, commit count, elapsed
timer, and progress. Agents run in parallel and share the project repo context.

Source: `models.ts` (`Agent`), `data.ts` (`AGENTS`, `AGENT_TOOLS`), `orchestra.store.ts`,
`sidebar/agent-row.component.ts`, `overview/agent-card.component.ts`.

## Agent properties

- [x] Owning project + dedicated branch (`agent/<name>`) and worktree (`agent-<id>`)
- [x] Coding tool with monogram badge: Claude ✳ · Codex ◆ · Cursor ▸ · Gemini ✦
- [x] Model selection; reasoning effort (low/medium/high) for Codex
- [x] Task description / initial prompt
- [x] Base commit, commit count ("ahead" of base)
- [x] Changed files with per‑file `A`/`M`/`D` state and +adds / −dels
- [x] Aggregated +adds / −dels totals
- [x] Elapsed time (human‑formatted) and progress ring (0–100%)
- [x] Pending items (permission / decision / review) feeding the inbox

## Status model

- [x] Six statuses: `running`, `blocked`, `waiting`, `done`, `idle`, `queued`
- [x] Per‑status color + animated status dot (pulsing for running/blocked/queued)
- [x] Status pill (filled + outline variants)
- [x] Block reason surfaced on the card and in chat
- [x] Wait reason tracked (e.g. "waiting on CI")
- [x] Status‑priority sort in the sidebar (blocked → running → … → idle)

## Agent actions

- [x] Open workspace / open terminal / view diff
- [x] Pause ↔ Resume
- [x] Start (for queued agents)
- [x] Commit changes (increments commit count, clears working set, logs commit)
- [x] Push to origin
- [x] Open pull request
- [x] Merge → project default branch (marks done, appends a merge commit)
- [x] Discard working changes
- [x] Rename branch / duplicate agent
- [x] Delete worktree (removes the agent, closes its tab; the confirm dialog's
      **Hard delete** checkbox also erases the worktree folder — off by default,
      so uncommitted work survives a plain delete)
- [x] Answer a blocked agent’s decision in chat → agent resumes live

## Spawning

- [x] Spawn dialog: project, source branch, tool, model, effort, prompt (see [spawn-agent.md](spawn-agent.md))
- [x] Spawn allocates a worktree + branch and starts a streaming boot log
- [x] Spawned agent opens its terminal automatically
- [x] Generated agent names cycle through a preset pool
- [x] Duplicate reuses a source agent’s tool/model/effort/task

## Where agents appear

- [x] Sidebar rows (nested under their project) with branch + diff stats + elapsed
- [x] Orchestrator cards / kanban cards / graph nodes / timeline rows
- [x] Top‑bar tabs (one per opened agent)
- [x] Right panel scopes Files/Inbox/Git to the selected agent
