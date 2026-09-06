# 01 — Correctness and bugs

Audit pass 1, at v0.22.2. Seventeen module groups were read for defects, and every finding was then re-opened by a second reviewer instructed to refute it. What survives: **5 critical, 42 high, 101 medium, 137 low**. Six claims were refuted outright and are listed at the end with the reason, so you can see what was considered and dropped; 52 more were downgraded on verification.

Nothing was executed. The Rust crate cannot be compiled in this container, and the app cannot be launched, so every finding is derived from reading the code and citing `path:line`. The frontend suite was run: 67 files, 602 tests, all passing — which is itself a finding, since several defects below sit in code those tests cover. Full detail is given for critical and high; medium and low are one line each, with the complete data in `.audit-work/pass1-merged.json`.

## The short version

**Four distinct critical defects, all of them data loss.** The five critical entries below include one duplicate pair, because two independent readers found the project-removal bug from opposite ends.

1. **Removing a project destroys every agent's uncommitted work.** "Remove project" is a plain context-menu item with no confirmation dialog anywhere. It cascades to `remove_for_project`, which runs `remove_dir_all` on every agent worktree — no disposal flag, no trash rename, no undo. The per-agent delete path deliberately makes this opt-in and says so in a comment; this path ignores that entirely. It also never stops the running agents first, so their PTYs keep running against directories that no longer exist.

2. **`merge()` force-resets the worktree with no dirty check.** Both the fast-forward and true-merge paths call `reset_to_tree`, which adds every non-added status path to the restore set and overwrites it. Uncommitted changes to tracked files are destroyed silently, including files the merge does not touch. Git itself refuses to merge in this situation; this is the wedge feature of the product.

3. **A malformed `~/.claude/settings.json` (or `.cursor`, `.gemini`) is replaced with an Orrery-only file.** The merge loads the existing config with `read_to_string(...).ok().and_then(from_str.ok()).unwrap_or_default()`, so *every* failure mode — a BOM, a trailing comma, a comment, non-UTF-8 bytes — becomes an empty map, and the user's entire agent configuration is overwritten with just Orrery's hooks. This runs unconditionally at every launch, and the write is not atomic.

4. **The same defect on TOML for `~/.codex/config.toml`**, plus a second one: the Codex hook install silently overwrites the user's own `[hooks]` entries.

**Two settings are silently discarded on every save.** The frontend persists the manual tool-executable path as `toolPath`; the Rust struct calls it `tool_paths`. `settings_set` deserializes into the typed struct and re-serializes, so the key disappears with no error. The launch path reads the Rust field, so the override never reaches the launcher at all — not merely after a restart. `keymapTerminal` has no Rust field whatsoever. Both suites stay green.

**The supervision loop has correctness bugs, not just UX ones.** A permission answered in the agent's own terminal is never cleared, so the card says "needs your input" for the rest of the run. Claude's idle-prompt notification is mapped to status "working", so an idle agent shows as busy indefinitely. A second, different permission request de-dupes on agent-plus-kind and is silently dropped, so Approve then approves a command the user never saw. Project-shell PTY output is discarded by the unknown-agent guard, so project tab terminals render nothing at all.

**Two races and a dedup floor in the PTY layer.** The `is_running` check and the process-map insert are not atomic, so two agent CLIs can end up running in one worktree. On relaunch the backend sequence counter restarts at zero while the renderer's dedup floor does not reset, so a restarted agent's output is silently dropped.

**Unsaved editor work has no guard on three paths.** The titlebar close button, quitting the app, and rename/delete in the sidebar file tree all discard dirty buffers with no prompt.

**`Agent.commits` is hard-coded to zero by the backend**, and it gates the Push button in two surfaces. An agent that commits its own work — which is what these agents do — has Push permanently greyed out after any restart.

**Several menu items are theatre.** "Pull latest" flashes "pulled <project>" and never calls the pull command that exists two files away. "Copy path" copies nothing. "Rename branch" only prints a toast.

## What is excluded

Documented trade-offs were not counted as defects: fire-and-forget hooks with best-effort keystroke allow/deny, no fan-out or mobile or SSH remote, gitoxide with system git for network operations, the compiled-out cost feature, Windows-first path handling, and decorator inputs where the test runner requires them.


## Critical (5)

### 1. An unreadable/unparseable ~/.codex/config.toml is silently replaced with an orrery-only hooks table

`src-tauri/src/agents/adapters/codex.rs:104-125` — data-loss

**How it breaks.** Identical mechanism to rust-adapters-01, on TOML. The user's `~/.codex/config.toml` is loaded with `.ok().and_then(|s| s.parse::<DocumentMut>().ok()).unwrap_or_default()`, so any read or parse failure (non-UTF-8 bytes, a syntax typo, or a TOML construct toml_edit 0.22 rejects) yields an EMPTY document. The code then sets its five hook keys and unconditionally writes the document back at line 125. The result is a `config.toml` containing only `[hooks]` — the user's `model`, `approval_policy`, `sandbox_*`, `[mcp_servers]`, `[model_providers]` (which hold API-key env references) and `[profiles]` are gone. Triggered on every app start via install_global_hooks (src-tauri/src/lib.rs:171). Because the pre-existing file is truncated in place there is no recovery. The adapter's own comment at codex.rs:100-103 claims the load 'tolerates missing/malformed → empty' as if that were safe, but the malformed case is the destructive one.

  ```
  104:        let mut doc = std::fs::read_to_string(&path)
  105:            .ok()
  106:            .and_then(|s| s.parse::<DocumentMut>().ok())
  107:            .unwrap_or_default();
  …
  125:        std::fs::write(path, doc.to_string())
  ```

<sub>Verifier: codex.rs:104-107 is the same shape on TOML (`read_to_string(&path).ok().and_then(|s| s.parse::<DocumentMut>().ok()).unwrap_or_default()`) and codex.rs:125 unconditionally writes `doc.to_string()` back to the same path, so a config.toml that toml_edit rejects (or that isn't UTF-8) is replaced by a document containing only the five hook keys — model / approval_policy / [mcp_servers] / [model_providers] / [profiles] gone, no backup. Not a duplicate of -01: different file, different parser, different code site. Same reachability caveat (needs an unparseable file); toml_edit is more permissive than serde_json, so this one is somewhat harder to trigger than -01.</sub>

### 2. An unreadable/unparseable user hook-config JSON is silently replaced with an orrery-only file (whole-config data loss)

`src-tauri/src/agents/adapters/mod.rs:486-527` — data-loss

**How it breaks.** `merge_json_hooks` loads the existing file with `read_to_string(...).ok().and_then(serde_json::from_str.ok())` and falls back to `unwrap_or_default()` — an EMPTY map — for every failure mode: file not valid UTF-8, a UTF-8 BOM, a trailing comma, `//` comments, or a top-level non-object. It then writes the whole re-serialised root back with `std::fs::write` (line 527). Nothing distinguishes 'file absent' from 'file present but unparseable', so the user's entire config is overwritten with just `{"hooks": {...orrery...}}` plus `defaults`. This is not a rare path: `install_global_hooks` runs on EVERY app start (src-tauri/src/lib.rs:169-172, unconditional, no is_installed gate) against three real user files — `~/.claude/settings.json` (claude.rs:85-87), `~/.cursor/hooks.json` (cursor.rs:71-73), `~/.gemini/settings.json` (gemini.rs:62-64). Concrete Windows-first scenario: the user edits `~/.claude/settings.json` from Windows PowerShell 5.1 (`Set-Content`/`Out-File`/`>` write UTF-16LE or UTF-8-with-BOM by default). `read_to_string` fails on UTF-16 / `from_str` fails on the BOM; the next Orrery launch destroys their permissions allowlist, env, statusLine, model and MCP settings with no error, no backup, and only the file it just wrote to show for it. Same for a JSONC-style `~/.gemini/settings.json`.

  ```
  486:    let mut root: Map<String, Value> = std::fs::read_to_string(path)
  487:        .ok()
  488:        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
  489:        .and_then(|v| match v { Value::Object(m) => Some(m), _ => None })
  493:        .unwrap_or_default();
  …
  527:    std::fs::write(path, serde_json::to_vec_pretty(&Value::Object(root))?)
  ```

<sub>Verifier: Re-read mod.rs:485-493 and 526-527: the load is `read_to_string(path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).and_then(|v| match v { Object(m) => Some(m), _ => None }).unwrap_or_default()` — every failure mode collapses to an empty Map, and line 526-527 then inserts `hooks` and writes the whole re-serialised root with std::fs::write, so a present-but-unparseable file is truncated to `{"hooks":…}` (+ defaults). No is_installed / is_present gate exists anywhere: lib.rs:169-175 calls install_global_hooks unconditionally in the Tauri setup closure on every start, and claude.rs:85-87 / cursor.rs:71-73 / gemini.rs:62-64 target the user's real ~/.claude/settings.json, ~/.cursor/hooks.json, ~/.gemini/settings.json. serde_json rejects a UTF-8 BOM, UTF-16 (read_to_string fails first), comments and trailing commas, so the trigger is reachable on a Windows-first app. Critical per the rubric (corruption of the user's config), with the caveat that it needs a malformed file to fire.</sub>

### 3. Removing a project hard-deletes every agent worktree folder (uncommitted work) with no opt-in, no confirmation and no PTY stop

`src-tauri/src/agents/service.rs:499-519` — data-loss

**How it breaks.** `Remove project` is a plain context-menu item (src/app/projects/project-actions.service.ts:145 -> :72-79) with no confirm modal; it invokes project_remove, which cascades into remove_for_project (src-tauri/src/projects/commands.rs:74-86). remove_for_project deletes the rows and then calls std::fs::remove_dir_all on EVERY agent's worktree unconditionally — no WorktreeDisposal choice, no `.trash-` rename, no git deregistration, and no rt.stop / watch.unwatch / history.purge. So one mis-click erases all uncommitted work in every worktree of that project, while the agents' PTY processes keep running (RuntimeService still holds them) with their cwd deleted, and their watchers keep scanning vanished directories. This directly contradicts the stated contract in src-tauri/src/agents/commands.rs:79-81 ('deleting someone's uncommitted work is opt-in, never a side effect of removing an agent') and docs/projects.md:37, which promises only 'removes its agents ... with confirmation toast'. Because the folders are erased in place (not renamed aside) the failure is also unrecoverable and synchronous on the command thread.

  ```
  499  pub fn remove_for_project(&self, project_id: Uuid) -> AppResult<Vec<Uuid>> {
  ...
  508      c.execute("DELETE FROM agents WHERE project_id = ?1", ...)
  513      }
  514      // best-effort: drop each agent's worktree working dir
  515      for r in &recs {
  516          let _ = std::fs::remove_dir_all(&r.worktree);
  517      }
  ```

<sub>Verifier: Re-read service.rs:499-519: after `DELETE FROM agents WHERE project_id`, the loop at 515-517 is `let _ = std::fs::remove_dir_all(&r.worktree)` with no disposal argument, no trash rename, no rt.stop/watch.unwatch/history.purge and no git deregistration. projects/commands.rs:67-95 (project_remove) calls it unconditionally, and src/app/projects/project-actions.service.ts:145 wires `Remove project` straight to removeProject (:72-79) which only flashes a toast AFTER the fact — there is no confirm modal anywhere (grep for openDeleteWorktree/confirm in projects/* finds none). docs/projects.md:37 promises 'with confirmation toast'. This directly inverts the contract stated at commands.rs:79-81. Uncommitted work in every worktree of the project is erased in place, unrecoverably.</sub>

### 4. Removing a project hard-deletes every agent worktree folder unconditionally, destroying uncommitted work

`src-tauri/src/projects/commands.rs:77-86` — data-loss

**How it breaks.** `project_remove` calls `agents.remove_for_project(id)`, which deletes the agent rows and then runs `std::fs::remove_dir_all(&r.worktree)` on every one of the project's agent worktrees. Any uncommitted or unpushed work in those worktrees is gone immediately — there is no `hard` flag, no rename-aside/trash step, and no confirmation: the single-agent path deliberately defaults to `WorktreeDisposal::KeepFolder` (agents/commands.rs:106-110) and even its hard variant renames the folder to `*.trash-*` and purges it in the background (agents/service.rs:409-423) so a mistake is recoverable. The frontend offers this as a plain context-menu item with no dialog — project-actions.service.ts:145 wires `Remove project` straight to `removeProject`, which at :72-79 calls `projectsStore.remove(id)` and flashes success. So one mis-click on a project with three running agents permanently deletes three worktrees of in-flight changes.

  ```
  src-tauri/src/projects/commands.rs:77
          if let Ok(removed) = agents.remove_for_project(id) {
  
  src-tauri/src/agents/service.rs:514-517
          // best-effort: drop each agent's worktree working dir
          for r in &recs {
              let _ = std::fs::remove_dir_all(&r.worktree);
          }
  ```

<sub>Verifier: Re-read src-tauri/src/projects/commands.rs:74-86 and src-tauri/src/agents/service.rs:499-518: `remove_for_project` DELETEs the rows then unconditionally `let _ = std::fs::remove_dir_all(&r.worktree)` for every agent (:514-516) — no disposal flag, no rename-aside. It directly contradicts the invariant written at agents/commands.rs:78-81 ("deleting someone's uncommitted work is opt-in, never a side effect") and the single-agent hard path's recoverable rename-to-trash at agents/service.rs:415-421. The frontend wires it with no dialog (project-actions.service.ts:145 → :72-79), while the app *does* have a delete-worktree confirmation modal for a single agent (ui.store.ts:547) — so the N-worktree path is the only unguarded one.</sub>

### 5. merge() hard-resets the worktree with no dirty check — uncommitted agent work is destroyed

`src-tauri/src/git/gix_backend.rs:1666` — data-loss

**How it breaks.** `merge()` never checks whether the worktree is dirty. Both of its writing paths call `reset_to_tree` (fast-forward at :1627, true merge at :1666), and `reset_to_tree` deliberately adds EVERY non-'A' status path to the restore set (:1169-1173) and then re-checks it out with `overwrite_existing = true` (:1098). So a file the agent modified but has not committed — including files the merged branch does not touch at all — is silently overwritten with the merged/target blob. The UI reaches this in one click: `agent-actions.service.ts:77-82` maps the agent row's "merge" verb straight to `mergeAgent()` → `agent_merge` (agents/commands.rs:844) with no dirty guard anywhere in between, while the agent CLI may be actively editing files in that worktree. `checkout_branch` (:1454-1468) has exactly the pre-check `merge` lacks, so the omission is not a house rule.

  ```
  1169:    for ch in GixBackend::status_uncached(&workdir, false) {
  1170:        let p: BString = ch.path.into();
  1171:        if ch.state != "A" || tree_has_file(&target_tree, p.as_bstr()) {
  1172:            paths.insert(p);
  ...
  1187:    let fresh = checkout_paths(repo, target, &restore)?;
  (1098:    opts.overwrite_existing = true;)
  (1666:        reset_to_tree(&repo, merged)?;)
  ```

<sub>Verifier: Re-read gix_backend.rs:1601-1683 and 1158-1189. Both writing paths call reset_to_tree (FF at :1627, true merge at :1666) and reset_to_tree seeds its path set from `for ch in GixBackend::status_uncached(&workdir,false)` (:1169-1173), adding every non-'A' path (i.e. every modified/deleted tracked file, even one the merge does not touch) and then re-checking it out with `opts.overwrite_existing = true` (:1098) — a `reset --hard` in all but name, with no dirty guard anywhere in merge(). checkout_branch does have that guard (:1454-1468: "has local changes that would be overwritten"), so this is an omission, not a house rule. The path is one click: git-action-bar.component.ts:97-99 `(native)="agentActions.mergeAgent(ag.id, baseBranch())"` -> agent_merge (agents/commands.rs:834-850) -> merge(), with no confirmation dialog and while the agent CLI may be mid-edit. Uncommitted agent work is destroyed silently; untracked files survive (they are excluded at :1171).</sub>


## High (42)

### 6. 'Push to origin' is disabled for any agent that committed on its own - ag.commits is always 0

`src/app/agents/agent-actions.service.ts:349` — correctness _(added by verifier)_

**How it breaks.** The backend never computes a commit count: AgentService::enrich hardcodes `commits: 0` for every record (src-tauri/src/agents/service.rs:160) and no code in src-tauri ever assigns that field, so every agent://created/updated and agent_list row carries 0. The only thing that ever makes ag.commits non-zero is the optimistic overlay patch after a Commit click (agent-actions.service.ts:100). So for the normal case - the coding agent commits inside its own worktree - the context-menu item `{ label: "Push to origin", disabled: !ag.commits }` (agent-actions.service.ts:349) stays greyed out for the whole session, and the overview card (overview/agent-card.component.ts:73), timeline (overview/timeline-view.component.ts:52) and graph strip (overview/graph-view.component.ts:86) all display '0' commits for an agent with a stack of them. AgentWorkStore already has the real list (agent_commits) but it is not used for this gate.

  ```
  agent-actions.service.ts:349 `{ label: "Push to origin", icon: "push", disabled: !ag.commits, onClick: () => this.act(id, "push") }` ; src-tauri/src/agents/service.rs:144-165 `fn enrich(&self, rec: AgentRecord) -> Agent { Agent { ... commits: 0, elapsed: 0, ... } }` with the doc comment 'Runtime fields are defaulted (no disk/process access yet)'.
  ```

### 7. Project-shell PTY output is discarded by the unknown-agent guard, so project tab terminals render nothing

`src/app/agents/agent-runtime.service.ts:217-219` — logic

**How it breaks.** A v2 project tab's shell is keyed by the PROJECT id (src-tauri/src/agents/commands.rs:388 shell_start -> rt.start_shell), and that id resolves only to a pseudo record synthesized backend-side on lookup (src-tauri/src/agents/service.rs:733 'None => self.project_pseudo_record(id)'). It is never a row in the agents table, so it is never in AgentsStore.all() (src/app/projects/pseudo-agent.ts:8: 'never stored, never in AgentsStore'). The mux does stream its bytes over agent://output keyed by that id (runtime_subscribe validates no ids, src-tauri/src/agents/commands.rs:532-535), but onOutput drops every entry whose id is absent from agentsStore.all(). onExit has an explicit project-shell carve-out (:533); onOutput and onDigest (:178) do not. Open a project tab: the terminal pane auto-starts the shell (src/app/workspace/pane-node.component.ts:417-426), keystrokes reach the PTY via agents.input, but no byte is ever written to xterm - not the prompt, not the echo of what the user types. The digest preview for the project is blanked by the same guard.

  ```
  212	      .onOutput((entries) => {
  214	        for (const { id, chunk, seq } of entries) {
  215	          // Why: the mux's exit force-drain can land after agent removal -
  216	          // writing then would recreate (and leak) a disposed terminal.
  217	          if (!this.agentsStore.all().some((a) => a.id === id)) continue;
  219	          this.terminals.write(id, chunk, seq);
  ```

<sub>Verifier: Re-derived end to end: terminals.write() has exactly one caller (agent-runtime.service.ts:219) and it is gated by the guard at :217 on agentsStore.all(); AgentsStore.all is the entity store fed by AgentList (agents.store.ts:40), and enrich()/list() never emit a project row (src-tauri/src/agents/service.rs:169-176), while a project shell's PTY is keyed by the project uuid (commands.rs:388-401 -> runtime/mod.rs:244 start_shell -> spawn_session, same mux). Interest DOES include project tabs (interest.service.ts:98 'kind === "agent" || kind === "project"'), so bytes are emitted and then dropped in the renderer. The digest guard at :178 blanks the project preview the same way; only onExit carves out shells (:532-537). One correction to the finder: the pane is not completely empty - attach() on the first (fresh) handle replays the backend scrollback snapshot (terminal.service.ts:424-426), so the project terminal shows a frozen one-shot snapshot and never updates again (a later tab switch does not re-attach or mark stale, see finding 08), which is if anything worse to diagnose.</sub>

### 8. Hook-driven permission notifications are never resolved except at process exit: needs-input sticks and later permission alerts are dedup-swallowed

`src/app/agents/agent-runtime.service.ts:515-528` — logic

**How it breaks.** For HOOK_TOOLS (claude/codex/cursor) needsInput is derived purely from 'a permission notification is still pending' (:515, :524-528), and detectNeedsInput - the only in-run caller of dismissPendingFor - returns immediately for hook-driven tools (:622). There is no backend 'permission resolved' event (grep: agent://permission is only ever emitted, never retracted). The documented reliable path is to approve in the agent's own TUI (notification.service.ts:168, docs/specs/2026-06-05-agent-hooks-permissions.md:117). Do that: the notification stays pending for the rest of the run, so (a) the card/status keeps showing 'input' while the agent is actually working (overview/agent-card.component.ts:181), and (b) NotificationStore.push de-dupes every later permission for that agent (notifications.store.ts:44-47 returns null on a still-pending same agent+kind), so no further toast, sound, or feed entry ever fires and the feed card keeps displaying the FIRST request's command while Accept sends allow keystrokes to whatever prompt is on screen now.

  ```
  514	      const working = hs === "working" || (hs === undefined && outputRecent);
  515	      return { working, needsInput: this.hasPendingPermission(id) };
  524	  private hasPendingPermission(id: string): boolean {
  526	      .pending()
  527	      .some((n) => n.agentId === id && n.kind === "permission");
  621	    // hook-driven tools get permission/question from the backend - don't double-raise
  622	    if (this.hookDriven(ag.tool)) return;
  ```

<sub>Verifier: Verified there is no retraction path: agent://permission is only ever emitted (hooks/mod.rs:268, 441); status pings only set hookState (:193-197) and never touch the feed; dismissPendingFor has exactly two callers (agent-runtime.service.ts:560 onExit, :635 the un-hooked falling edge) and detectNeedsInput returns at :622 for HOOK_TOOLS. NotificationStore.push returns null for a still-pending same agent+kind (notifications.store.ts:44-47) and NotificationAlertService.raise then fires nothing at all (:64). So approving in the agent's own TUI - the path the code and docs/specs/2026-06-05-agent-hooks-permissions.md:117 call the reliable one - leaves the notification pending for the rest of the run: needsInput stays true (:515) and every later permission for that agent is swallowed. The documented trade-off covers best-effort allow/deny keystrokes, not this; the spec never mentions the sticky-pending consequence.</sub>

### 9. minSeq survives a process restart while the backend seq restarts at 0, so a restarted agent's output is silently dropped

`src/app/terminal.service.ts:277` — off-by-one-boundary

**How it breaks.** recover() sets minSeq[id] = snapshot.endSeq (:310) and write() drops any chunk with seq <= minSeq (:277). minSeq is cleared ONLY in dispose(id) (:471) - never on start or exit. The backend's seq is per-run and restarts at 0 on every (re)launch: src-tauri/src/runtime/mod.rs:326 'A relaunch restarts the batcher seq at 0' and scrollback::rings().reset(&id) zeroes end_seq. Sequence: the user opens an agent terminal after the app started (fresh handle -> recover -> minSeq = e.g. 500_000), later presses Stop then Start in that same still-mounted pane. attach() is not re-run, nothing marks the terminal stale, so minSeq stays 500_000 while the new run emits seq 4096, 8192, ... All of it is dropped and the terminal looks dead until the new run has produced 500 KB of output. Any prior hide/show cycle (which recovers via markStale) arms the same trap.

  ```
  277	    if (seq !== undefined && seq <= (this.minSeq.get(id) ?? -1)) return;
  310	      this.minSeq.set(id, snap.endSeq);
  471	    this.minSeq.delete(id);
  -- src-tauri/src/runtime/mod.rs
  326	        // A relaunch restarts the batcher seq at 0 - stale ring content tagged
  327	        // with the previous run's seqs would corrupt snapshot dedup.
  328	        scrollback::rings().reset(&id.to_string());
  ```

<sub>Verifier: minSeq is written only in recover() (terminal.service.ts:310) and deleted only in dispose() (:471) - grep over src/app shows no other writer. The backend seq is per-spawn: batch_loop starts `let mut seq: u64 = 0` (runtime/output_batcher.rs:70) and spawn_session creates a new batcher plus scrollback::rings().reset() on every (re)launch (runtime/mod.rs:326-328). attach() only runs when the pane's agent id changes (workspace/terminal.component.ts:113-122), and neither startProcess (:377-398) nor onExit (:530-574) touches minSeq or markStale, so a Stop/Start inside a mounted pane leaves minSeq at the old run's endSeq and write() (:277) silently drops the new run's chunks until it exceeds it. Recoverable only by a hide/show cycle (interest.service.ts:66-77 -> recoverIfStale).</sub>

### 10. Alt-only chords (Alt+N queue peek, any user-recorded Alt binding) never fire while an input, editor or terminal has focus

`src/app/commands/command-registry.service.ts:285-286` — logic

**How it breaks.** The dispatcher treats "is this a modifier chord?" as `e.ctrlKey || e.metaKey`, so an Alt-only binding is dropped by the `typing` guard before the terminal steal-list is ever consulted. `typing` is true for INPUT/TEXTAREA/contentEditable, which covers the xterm helper textarea (every agent terminal), the CodeMirror editor and every text field in the shell. So Alt+N — the advertised binding for the 'Needs You' queue (status bar shows 'Alt+N') — does nothing whenever the user is in a terminal or an editor, i.e. in the app's normal focus state; it only works when focus happens to sit on a non-input surface. The same applies to any Alt-only chord the user records in Settings → Keymap, which fuzzy.ts:70-81 explicitly accepts as a valid recordable binding (`if (!primary && !e.altKey) return null` allows Alt without Ctrl).

  ```
  278  for (const cmd of this.commands()) {
  279    const kbds = [cmd.kbd, cmd.kbdAlt].filter((k): k is string => !!k && k !== "Shift Shift");
  280    if (!kbds.some((k) => matchBinding(e, k))) continue;
  285    if (typing && !(e.ctrlKey || e.metaKey)) return;
  286    if (inTerminal && !cmd.terminal) return;
  ```

<sub>Verifier: command-registry.service.ts:249 computes `typing` from tag INPUT/TEXTAREA/isContentEditable, and xterm's helper element is a <textarea class="xterm-helper-textarea"> (:252 tests exactly that class on the same target), so inside any terminal `typing` is true. :285 `if (typing && !(e.ctrlKey || e.metaKey)) return;` fires before the steal-list check at :286 and Alt+n (:162) sets neither ctrlKey nor metaKey, so the queue-peek chord advertised as 'Alt+N' in status-bar.component.ts:39 and notification-center.component.ts:37 is dropped in the terminal, the CodeMirror editor and every text field. fuzzy.ts:74 (`if (!primary && !e.altKey) return null`) does accept Alt-only chords as recordable, so user keymap entries hit the same wall.</sub>

### 11. Settings.toolPath never reaches the backend: Rust calls the field tool_paths, so the manual tool executable path is dropped on every save and never used at launch

`src/app/models.ts:446` — wire-contract-mismatch _(added by verifier)_

**How it breaks.** The frontend persists the per-tool executable override under `toolPath` (models.ts:446; written by settings.store.ts setMap, runtime-row.component.ts:221 and :250) and sends the whole document as `settings_set {settings}` (settings.store.ts:247-249). The Rust Settings struct declares `pub tool_paths: BTreeMap<String, String>` under `#[serde(rename_all = "camelCase", default)]` (src-tauri/src/settings/model.rs:14, :29), i.e. it expects `toolPaths` and has no serde alias. With no deny_unknown_fields, `toolPath` is silently ignored on deserialize and `tool_paths` falls back to empty; SettingsService::set then re-serializes the PARSED struct (src-tauri/src/settings/service.rs:83-90), so the key is erased from the stored row. Two consequences: (a) agent_start's `program_override` reads `cfg.tool_paths` (src-tauri/src/agents/commands.rs:331-336) and detect_tools reads `settings.get().tool_paths` (:948), so a user who locates a binary that is not on PATH still gets the PATH lookup — the agent launches wrong or not at all, and the Settings tile keeps reporting the tool missing after a re-detect; (b) settings_get returns no `toolPath`, so `toolPath: { ...(p.toolPath ?? {}) }` (settings.store.ts:179) yields {} and the override is gone after a restart. runtime-row.component.ts:221's comment 'persist + use at launch' is false on both halves.

  ```
  models.ts:446  `toolPath: Record<string, string>;` (under the header at :396-398 'this mirrors the Rust `Settings` struct exactly') vs src-tauri/src/settings/model.rs:29 `pub tool_paths: BTreeMap<String, String>,` with `#[serde(rename_all = "camelCase", default)]` at :14; consumer src-tauri/src/agents/commands.rs:331-336 `let program_override = cfg.tool_paths.get(&agent.tool)...`
  ```

### 12. Settings.keymapTerminal has no field in the Rust Settings struct, so the per-command terminal-steal overrides are discarded on save and never survive a restart

`src/app/models.ts:463` — wire-contract-mismatch _(added by verifier)_

**How it breaks.** The settings modal writes `keymapTerminal[cmd.id]` through SettingsStore.setKeymapTerminal (settings-modal.component.ts:924, :929 -> settings.store.ts:221-230) and CommandRegistryService reads it live (command-registry.service.ts:142, :230), so the toggle appears to work in-session. But the Rust Settings struct has no `keymap_terminal` field at all (src-tauri/src/settings/model.rs:15-70 lists keymap at :42 and nothing else keymap-related), and with `#[serde(default)]`/no deny_unknown_fields the unknown `keymapTerminal` key is ignored on deserialize and dropped when SettingsService::set re-serializes the parsed struct (src-tauri/src/settings/service.rs:83-90). After a restart settings_get carries no `keymapTerminal`, so settings.store.ts:181 rebuilds it as {} and every override reverts to the built-in steal-list default.

  ```
  models.ts:463 `keymapTerminal: Record<string, boolean>;`; grep for keymap in src-tauri/src returns only settings/model.rs:42 `pub keymap: BTreeMap<String, String>,`, :127 and a test at :183 — no keymap_terminal anywhere
  ```

### 13. Permission notifications de-dupe on agent+kind only, so a second different prompt is silently dropped and Approve then approves the wrong command

`src/app/stores/notifications.store.ts:44-47` — logic

**How it breaks.** `push()` refuses any notification whose agentId+kind matches a still-pending one, regardless of content. For hook-driven tools (claude/codex/cursor) every distinct permission request goes through `onPermissionRequest` -> `alerts.raise` -> `push` with its own `command`/`filePath` (agent-runtime.service.ts:584-613), and nothing auto-resolves a pending permission except the user or process exit: `dismissPendingFor` is called only on exit (agent-runtime.service.ts:560) and inside `detectNeedsInput`, which returns early for hook-driven tools (agent-runtime.service.ts:622). Scenario: agent A asks to run `npm test`; the user ignores the card and approves it directly in the TUI; A then asks to run `rm -rf dist`. The second request is dropped entirely — `raise` returns null so there is no feed entry, no OS toast, no sound (notification-alert.service.ts:64-65) — and `hasPendingPermission` still points at the stale card, so the sidebar needs-input dot describes the old prompt. If the user now clicks Approve on that stale card, `NotificationService.accept` calls `agentsStore.allow(n.agentId)` (notification.service.ts:33-38), which types the allow keystrokes into whatever prompt is currently on the PTY — the user approves `rm -rf dist` while reading `npm test`. (The declared trade-off covers allow/deny being best-effort keystrokes, not the card describing a different request than the one being answered.)

  ```
  44:    const dup = this.list().find(
  45:      (n) => n.agentId === input.agentId && n.kind === input.kind && n.status === "pending",
  46:    );
  47:    if (dup) return null;
  ```

<sub>Verifier: Re-read notifications.store.ts:44-47: push() rejects on agentId+kind+pending only, ignoring command/filePath. Nothing else resolves a pending permission — dismissPendingFor is called only from onExit (agent-runtime.service.ts:560) and from detectNeedsInput, which returns at :622 for HOOK_TOOLS (claude/codex/cursor, :124). accept() then types allow keystrokes at whatever prompt is live on the PTY (notification.service.ts:33-38 -> agents.store.ts allow()), so a stale card can approve a different command; hasPendingPermission (agent-runtime.service.ts:524-528) also keeps needsInput pinned to the stale entry. Approving in the TUI is an endorsed path (notification.service.ts:14-15), so this is reachable in normal use. The same line has a second consequence the finder missed: onExit only dismisses ["permission","question"], so a still-pending "done" card suppresses the next run's "X finished" notification entirely.</sub>

### 14. Spawn modal throws on construction when there are no projects (first run)

`src/app/modals/spawn-modal.component.ts:351` — null-undefined

**How it breaks.** `this.projects.all()[0].id` is a field initializer, so it runs the moment the component is instantiated. On a fresh install `ProjectsStore.all()` is `[]`, so `all()[0]` is undefined and reading `.id` throws a TypeError. The Spawn button is unconditionally available in that state — sidebar.component.ts:89 and overview.component.ts:54 both call `ui.openSpawn(null)` with no guard, and shell.component.ts:167 opens the component from inside an effect — so the very first thing a new user is invited to do (before adding a project) blows up inside change detection and no dialog appears. Even with the constructor fixed, `project()` falls back to `all()[0]` (line 364-366) and the template dereferences `proj.path` (line 122), which throws for the same reason.

  ```
  351  readonly projectId = signal<string>(this.defaultProject || this.projects.all()[0].id);
  364  readonly project = computed(
  365    () => this.projects.all().find((p) => p.id === this.projectId()) || this.projects.all()[0],
  122  <div class="trunc" …>{{ proj.path }}</div>
  ```

<sub>Verifier: spawn-modal.component.ts:351 `this.projects.all()[0].id` is a class field initializer, which runs before the constructor body for a base class. ProjectsStore starts empty (projects.store.ts:27-33: `load()` on an empty DB, and the catch comment says "start empty"), and nothing guards the entry points: sidebar.component.ts:89, sidebar.component.ts:237, overview.component.ts:54 all call `ui.openSpawn(null)` unconditionally. shell.component.ts:170 constructs the component inside `untracked()` in an effect, so the TypeError surfaces there and `ref` stays null while `ui.spawning()` stays truthy — the effect will not re-run on a repeat click, so the button is dead. The constructor's own `void this.refreshBranches()` (spawn-modal.component.ts:505 → :409 `this.project()`) hits the same `all()[0]` a second time.</sub>

### 15. Project menu "Pull latest" does nothing but claim success; "Copy path" copies nothing

`src/app/projects/project-actions.service.ts:118-122` — logic

**How it breaks.** `Pull latest` is wired to `this.ui.flash("pulled " + p.name)` — no git call anywhere in the chain. The user right-clicks a project, chooses Pull latest, sees the toast "pulled orrery", and reasonably believes the working copy now matches the remote; they then spawn agents on a stale base. `Copy path` likewise only flashes the path as a toast; nothing is written to the clipboard (contrast dev-panel.component.ts:698, which does use `navigator.clipboard.writeText`), so a subsequent paste yields whatever was in the clipboard before.

  ```
  117      {
  118        label: "Pull latest",
  119        icon: "refresh",
  120        disabled: !p.folderExists,
  121        onClick: () => this.ui.flash("pulled " + p.name),
  122      },
  135      { label: "Copy path", icon: "dup", onClick: () => this.ui.flash(p.path) },
  ```

<sub>Verifier: project-actions.service.ts:117-122: the "Pull latest" item's entire onClick is `this.ui.flash("pulled " + p.name)` — no ProjectsStore call, no bridge invoke. Line 135: "Copy path" is `onClick: () => this.ui.flash(p.path)`, with no `navigator.clipboard` anywhere in the file (the codebase does use it elsewhere, dev-panel.component.ts:698). Both toasts assert an action that did not happen.</sub>

### 16. Manual tool executable path is persisted under `toolPath`, which the Rust Settings struct does not have (`toolPaths`) — the override is silently dropped

`src/app/settings/settings.store.ts:33` — logic

**How it breaks.** Settings → Agent defaults → Executable → "Use this path" calls `store.setMap("toolPath", …)` (runtime-row.component.ts:221). The debounced whole-document write sends `{ … toolPath: {claude: 'C:/x/claude.exe'}, toolPaths: {} … }` to `settings_set`, which deserializes into the typed `Settings` struct — `toolPath` is an unknown key (serde ignores it) and `tool_paths` stays empty. The echo is ignored by `persist()` (settings.store.ts:247-249) so nothing surfaces the loss. The row keeps showing "detected · manual path" because `runtime.setDetection` only patched an in-memory signal. Every backend reader (`detect_tools` at agents/commands.rs:948, the launch path at agents/commands.rs:332) sees an empty map, so the tool is still launched from PATH in THIS session, and after a restart the row reverts to "not installed" with the user's path gone.

  ```
  settings.store.ts:33   toolPath: {}, // absent tool = auto-detect on PATH
  settings.store.ts:179   toolPath: { ...(p.toolPath ?? {}) },
  runtime-row.component.ts:221        this.store.setMap("toolPath", this.toolId(), v); // persist + use at launch
  src-tauri/src/settings/model.rs:29    pub tool_paths: BTreeMap<String, String>,
  src-tauri/src/agents/commands.rs:948  let overrides = settings.get().unwrap_or_default().tool_paths;
  ```

<sub>Verifier: Re-derived end to end. src/app/settings/settings.store.ts:33,179 and src/app/models.ts:446 use `toolPath`; src-tauri/src/settings/model.rs:29 declares `pub tool_paths` under `#[serde(rename_all="camelCase")]` → `toolPaths` (asserted at model.rs:158). `settings_set` takes a typed `Settings` (src-tauri/src/settings/commands.rs:16) whose container `#[serde(default)]` silently drops unknown keys (test at model.rs:193-199), and service.rs:86 re-serializes the typed struct, so `toolPath` never lands. Both readers use `tool_paths`: agents/commands.rs:948 (`detect_tools`) and agents/commands.rs:331-335 (launch `program_override`). No mapping exists anywhere (grep over src/ and src-tauri/src/). The write site is runtime-row.component.ts:221.</sub>

### 17. `keymapTerminal` has no backend field at all — the keymap 'tty' overrides never survive a restart

`src/app/settings/settings.store.ts:42` — logic

**How it breaks.** The keymap section's `tty` button calls `setKeymapTerminal(cmd.id, …)` (settings-modal.component.ts:551 → settings.store.ts:221-229) and `CommandRegistryService` reads `settings().keymapTerminal[cmd.id]` to decide whether a chord fires inside a focused terminal (command-registry.service.ts:142,230). The whole-document `settings_set` payload carries `keymapTerminal`, but the Rust `Settings` struct has no such field (only `keymap`), so serde discards it. On the next launch `settings_get` returns no `keymapTerminal`, `merge` re-seeds `{}` (line 181) and every tty override the user set is gone — while the row's dirty dot and reset pill claimed it was saved.

  ```
  settings.store.ts:42     keymapTerminal: {}, // absent id = default terminal steal-list rule
  settings.store.ts:181    keymapTerminal: { ...(p.keymapTerminal ?? {}) },
  settings.store.ts:221-228 setKeymapTerminal(...) { ...next... this.persist(); }
  (src-tauri/src/settings/model.rs:15-72 declares keymap but no keymap_terminal)
  ```

<sub>Verifier: src-tauri/src/settings/model.rs:15-71 has `keymap` but no `keymap_terminal` field, so the `keymapTerminal` key the store writes (settings.store.ts:42,181,221-229) is dropped by the same typed-deserialize path as finding 01 and reloads as `{}`. The consumer is real: command-registry.service.ts:142 reads `settings().keymapTerminal` and :230 applies `keymapTerminal[cmd.id] ?? terminalDefaultOf(eff)`, and the settings row (settings-modal.component.ts:538) shows a dirty dot for it. A setting that does not persist = high.</sub>

### 18. `SettingsStore.install()` never relaunches — on any platform where `update_install` returns, the card sticks on "Installing…" forever

`src/app/settings/settings.store.ts:300-346` — async _(upgraded on verification)_

**How it breaks.** The manual install path invokes `update_install` and, on the success path, does nothing else: `installing` stays true, `installPhase` stays 'installing', and `relaunch()` is never called (unlike `UpdaterService.run()` at updater.service.ts:87-88). This is only harmless because on Windows `install_via_stub` exits the process (update.rs:169-171). On the non-Windows branch `perform()` does `update.install(&bytes)?; Ok(())` and returns normally, so the promise resolves, the finally-block unhooks the listeners, and the UI is left permanently showing "Handing off to the installer — Orrery restarts when it finishes" with the Install button disabled; the app never restarts and there is no way back except quitting. The update-resume list also stays armed (it is cleared only in the catch, line 340).

  ```
  334      await this.bridge.invoke(Commands.UpdateInstall, { channel: this.settings().channel });
  335    } catch {
  342    } finally {
  343      // On success the process exits before this runs; on failure it unhooks.
  344      subs.forEach((off) => off());
  src-tauri/src/update.rs:175    update.install(&bytes).map_err(|e| e.to_string())?;
  ```

<sub>Verifier: Verified the non-Windows return path: src-tauri/src/update.rs:166-177 — the `#[cfg(windows)]` stub block early-returns, and everything else falls through to `update.install(&bytes)?; Ok(())`, so `update_install` resolves normally. settings.store.ts:319-334 then completes the try with no `relaunch()` (contrast updater.service.ts:87-88), leaving `installing`=true and `installPhase`='installing' forever — the card is stuck on "Handing off to the installer — Orrery restarts when it finishes" (settings-modal.component.ts:337-341) with the Install button disabled. macOS is a shipped release target (.github/workflows/release.yml build matrix `os: macos-14` producing dmg + `.app.tar.gz` updater artifacts, staged at the `matrix.slug == 'mac'` step), so this is a real platform, not a hypothetical — upgrading to high.</sub>

### 19. Ticket edit drafts are silently reset (typed title/notes lost) whenever any ticket in the store changes

`src/app/backlog/ticket-page.component.ts:417-428` — race-concurrency

**How it breaks.** The draft linkedSignals declare `source: this.ticketId`, but their computation also reads `this.ticketsStore.byId(id)`, which reads the store's `all()` signal (tickets.store.ts:37-39, entity-store.ts:26 `all = computed(() => ids().map(...))`). Angular evaluates a linkedSignal computation inside the node's own reactive consumer (@angular/core/fesm2022/_untracked-chunk.mjs:62-73), so every signal read inside it is a tracked producer: when it changes the node recomputes and the user's `set()` value is thrown away. I verified this with a throwaway vitest against the installed Angular: writing to the linked signal, then changing an unrelated key of the store signal, restores the computed value. Concretely: open a ticket, click Edit, type a new title and notes, then add a tag in the side rail (onTagsChange at :611-618 calls `ticketsStore.update`, whose ticket://updated event upserts the entity → new `all()` array). draftTitle/draftNotes/draftProjectId immediately re-seed from the stored ticket, the `<input [value]="draftTitle()">` reverts mid-edit, and pressing Save then writes the OLD title back. The same reset fires for any unrelated ticket event (an agent attaching to another ticket, a status drag on the board in another tab).

  ```
  417:  readonly draftTitle = linkedSignal({
  418:    source: this.ticketId,
  419:    computation: (id) => (id === "draft" ? "" : (this.ticketsStore.byId(id)?.title ?? "")),
  420:  });
  421:  readonly draftNotes = linkedSignal({
  422:    source: this.ticketId,
  423:    computation: (id) => (id === "draft" ? "" : (this.ticketsStore.byId(id)?.notes ?? "")),
  424:  });
  ```

<sub>Verifier: Re-derived from Angular 22.1.3's own source: LINKED_SIGNAL_NODE.producerRecomputeValue (node_modules/@angular/core/fesm2022/_untracked-chunk.mjs:57-73) runs BOTH node.source() and node.computation() inside consumerBeforeComputation(node)/consumerAfterComputation(node), so every signal read in the computation is a tracked producer; set() only calls producerMarkClean, which a later producer change undoes. The computation at ticket-page.component.ts:419/423/427 calls TicketsStore.byId (stores/tickets.store.ts:37-39) which reads this.all(), a computed over ids()/entities() (state/entity-store.ts:26) that produces a NEW array on any upsert of ANY ticket. So a tag write from the side rail (onTagsChange, :611-618 -> ticketsStore.update -> ticket://updated upsert) or any unrelated ticket event re-seeds draftTitle/draftNotes/draftProjectId mid-edit and Save (:552-556) then writes the stale title back.</sub>

### 20. Ticket page offers "No project" but saving it can never clear the project — the change silently reverts

`src/app/backlog/ticket-page.component.ts:552-556` — logic

**How it breaks.** The edit-mode select includes `{ value: "", label: "No project" }` (:441-445). On Save, `projectId: this.draftProjectId() || null` sends JSON null, which deserializes to `project_id: None` in TicketUpdateRequest (src-tauri/src/tickets/model.rs:131-138). The service treats None as "no change": `if let Some(pid) = req.project_id { rec.project_id = Some(pid); }` with the explicit comment "Some(id) sets it, None leaves it" (src-tauri/src/tickets/service.rs:186-193). A user who moves a ticket from project P to "No project" and clicks Save gets no error; the ticket:// echo re-renders P, and the side rail shows the old project again. (Creating a ticket with no project works — only update is affected.)

  ```
  552:      try {
  553:        await this.ticketsStore.update(id, {
  554:          title,
  555:          notes: this.draftNotes() || undefined,
  556:          projectId: this.draftProjectId() || null,
  ```

<sub>Verifier: Verified end to end. The select offers {value:""} 'No project' (ticket-page.component.ts:441-445); save sends projectId: draftProjectId() || null (:555). TicketUpdateRequest.project_id is Option<Uuid> (src-tauri/src/tickets/model.rs:133) and update() only assigns inside `if let Some(pid) = req.project_id` with the comment 'Some(id) sets it, None leaves it' (src-tauri/src/tickets/service.rs:188-192), so JSON null == absent == no change. The UPDATE statement then rewrites the unchanged rec.project_id. Clearing a ticket's project is impossible and fails silently.</sub>

### 21. Board view silently drops every `idle` agent — no column matches that status

`src/app/overview/kanban-view.component.ts:66-75` — logic

**How it breaks.** `cols` covers queued / running / blocked(+waiting) / done, and `colItems` keeps an agent only when its status equals a column key or its `alt`. `idle` is the status every agent is BORN with (src-tauri/src/agents/service.rs:227 `status: "idle"`, mirrored optimistically in agent-actions.service.ts:194) and the status every agent returns to when its process exits without finishing (agent-runtime.service.ts:563 `update(id, { status: "idle" })`). So an agent created with "Create" (spawn without start), or any agent whose CLI exited, disappears completely from the Board while still showing in Grid/Timeline/Graph — the four column counts do not add up to the "N agents" in the header. The same gap exists in the overview stat header (overview.component.ts:36-39 counts running/blocked/waiting+queued/done only), so idle agents are invisible there too.

  ```
  66:  readonly cols: Col[] = [
  67:    { key: "queued", label: "Queued" },
  68:    { key: "running", label: "Running" },
  69:    { key: "blocked", label: "Needs you", alt: "waiting" },
  70:    { key: "done", label: "Done" },
  71:  ];
  73:  colItems(c: Col): Agent[] {
  74:    return this.agents().filter((a) => a.status === c.key || a.status === c.alt);
  ```

<sub>Verifier: cols (kanban-view.component.ts:66-71) has no 'idle' key or alt, and colItems (:73-75) filters strictly on key/alt, so idle agents render in no column. 'idle' is a real AgentStatus (utils.ts:15 STATUS_META.idle) and is both the birth status (src-tauri/src/agents/service.rs:227) and the post-exit status (agent-runtime.service.ts:562-563 sets idle for any non-done agent when the process ends). Create-without-start is a real path (spawn-modal.component.ts:584 `submit(start: boolean)`). Grid (grid-view.component.ts:12) and Graph render every agent, so the Board silently loses rows; overview.component.ts:36-39 likewise counts only running/blocked/waiting+queued/done.</sub>

### 22. Rename/Delete in the sidebar file tree discards unsaved editor edits with no guard and leaves the pane's file tab on a dead path

`src/app/sidebar/files/file-tree.component.ts:227` — data-loss

**How it breaks.** `EditsStore.close` unconditionally drops the buffer (edits.store.ts:105-113 — no dirty check), and neither path removes the file from the pane tree (`closeFileInLeaf` is never called). Scenario: the user opens `src/app/foo.ts` from the tree, types edits (autosave defaults to false — settings.store.ts:40), then right-clicks that same file in the tree and picks Rename (or Delete). The rename moves the OLD on-disk content, `edits.close(id, from)` throws the modified buffer away without any prompt, and the pane keeps a tab pointing at the now-nonexistent path. The app has a whole unsaved-changes dialog for exactly this loss on tab close (tab-close-guard.service.ts:31-44), which this path bypasses.

  ```
  file-tree.component.ts:
    222        if (mode === "rename") {
    223          const from = m.node!.path.replace(/\\/g, "/");
    226          await this.bridge.invoke(Commands.FileRename, { id, from, to });
    227          this.edits.close(id, from); // stale buffer under the old path
    272        await this.bridge.invoke(Commands.FileDelete, { id, path });
    273        this.edits.close(id, path);
  ```

<sub>Verifier: `EditsStore.close` (edits.store.ts:105-112) deletes the key with no dirty check, and `closeFileInLeaf` is called only from pane-manager.component.ts:104 — never from the tree, so the leaf's `files`/`activeFile` keep the dead path (pane-model.ts:211). Autosave is off by default (settings.store.ts:40), so an unsaved buffer is the normal state, and the same loss on tab close does raise the Save-all/Discard/Cancel dialog (tab-close-guard.service.ts:31-44, rendered at top-bar.component.ts:219-237). The rename case (file-tree.component.ts:222-227) is the true silent-loss path; delete (:266-274) mainly leaves the dangling tab.</sub>

### 23. Titlebar × closes the window with no unsaved-buffer guard, silently dropping editor edits

`src/app/top-bar/window-controls.component.ts:97` — data-loss

**How it breaks.** `close()` calls the Tauri window close directly. Nothing on the Rust side prevents it — the only close handler tears agents down (src-tauri/src/lib.rs:397-400, `WindowEvent::CloseRequested { .. } => teardown()`, no `prevent_close`) — and `EditsStore` state is in-memory only (edits.store.ts:25), with autosave off by default (settings.store.ts:40). Scenario: user edits two files in panes, does not press Ctrl+S, clicks the titlebar ×; the app exits and both buffers are gone with no prompt, while closing the same tab would have raised the Save all / Discard / Cancel dialog (tab-close-guard.service.ts:31-44, rendered at top-bar.component.ts:219-237).

  ```
  window-controls.component.ts:
    96    close() {
    97      if (inTauri()) void getCurrentWindow().close();
    98    }
  src-tauri/src/lib.rs:
    397                RunEvent::WindowEvent {
    398                    event: WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed,
    400                } => teardown(),
  ```

<sub>Verifier: `close()` calls `getCurrentWindow().close()` directly (window-controls.component.ts:96-98) and there is no counterweight anywhere: grep for `beforeunload|onCloseRequested|prevent_close` across src/ and src-tauri/src/ returns nothing, and lib.rs:396-400 only tears agents down on `CloseRequested`. EditsStore state is in-memory (edits.store.ts:25) and autosave defaults false (settings.store.ts:40), so unsaved buffers are dropped without the dialog the tab-close path raises.</sub>

### 24. Any tweak change (theme toggle, density, motion) resets the Orchestrator visualization the user picked

`src/app/ui/ui.store.ts:209` — logic

**How it breaks.** `viz` is a separate signal the Overview writes when the user picks a visualization (overview.component.ts:44 `(valueChange)="ui.viz.set(...)"`). The sync effect reads the WHOLE `tweaks()` object, and `setTweak` (ui.store.ts:221-223) always replaces that object, so every tweak write re-runs the effect and overwrites `viz` with `defaultViz`. Concretely: user is on the Orchestrator with the Kanban visualization selected, clicks the theme toggle in the top bar (top-bar.component.ts:201 → `ui.toggleTheme()` → `setTweak('theme', …)`), and the board silently snaps back to `grid`. Same for the Tweaks panel's density and Live-motion switches.

  ```
  ui.store.ts:
    208    // keep viz in sync with the default
    209    effect(() => this.viz.set(this.tweaks().defaultViz));
    221    setTweak<K extends keyof Tweaks>(key: K, value: Tweaks[K]) {
    222      this.tweaks.update((t) => ({ ...t, [key]: value }));
    223    }
    225      this.setTweak("theme", this.tweaks().theme === "dark" ? "light" : "dark");
  ```

<sub>Verifier: Re-derived end to end: `effect(() => this.viz.set(this.tweaks().defaultViz))` (ui.store.ts:209) tracks the whole `tweaks` signal, and every writer replaces the object (`setTweak` :221-223 spreads into a new object), so any tweak write re-runs the effect. `viz` is written independently by the Overview's visualization tabs (overview.component.ts:44 `(valueChange)="ui.viz.set($any($event))"`), and the tweaks panel writes theme/density/motion through the same `setTweak` (tweaks-panel.component.ts:32, :45, :61), as does the top-bar theme button (top-bar.component.ts:201). Nothing in ui.store.spec.ts asserts this behaviour as intended.</sub>

### 25. No dirty-buffer guard on window/app close — quitting silently discards every unsaved editor buffer

`src/app/workspace/tab-close-guard.service.ts:20-31` — data-loss

**How it breaks.** EditsStore buffers live only in memory (edits.store.ts:25, never persisted anywhere — WorkspaceStore.currentDoc serializes pane roots and scroll state but no buffers), and autosave is off by default (settings.store.ts:40 `autosave: false`). TabCloseGuardService guards workspace-TAB closes only; there is no beforeunload handler and no getCurrentWindow().onCloseRequested listener anywhere in src/ (grep for beforeunload/CloseRequested finds only src-tauri/src/lib.rs:398). The title-bar × calls getCurrentWindow().close() unconditionally (top-bar/window-controls.component.ts:96-98) and the Rust side treats WindowEvent::CloseRequested as a teardown signal (src-tauri/src/lib.rs:397-400), never asking the frontend. So: type in the editor, hit the window ×, and every unsaved buffer in every tab is gone with no prompt — the one class of user data this app owns exclusively.

  ```
  src/app/settings/settings.store.ts:40   autosave: false,
  src/app/top-bar/window-controls.component.ts:96-98
    close() {
      if (inTauri()) void getCurrentWindow().close();
    }
  src-tauri/src/lib.rs:397-400
    RunEvent::WindowEvent {
      event: WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed, ..
    } => teardown(),
  ```

<sub>Verifier: grep over src/ and src-tauri/src/ for beforeunload|onCloseRequested|CloseRequested returns only src-tauri/src/lib.rs:398, and that arm just calls teardown() (stop_all + reset_running) — the frontend is never consulted. window-controls.component.ts:96-98 closes unconditionally, settings.store.ts:40 has autosave:false, and EditsStore state (edits.store.ts:25) is in-memory only. TabCloseGuardService is wired only to tab closes (top-bar / command-registry). Quitting therefore discards every unsaved buffer with no prompt.</sub>

### 26. Every mounted terminal pane swallows Ctrl+F window-wide, breaking find in the editor and blame views

`src/app/workspace/terminal.component.ts:99` — logic

**How it breaks.** TerminalComponent registers a WINDOW-level keydown listener in the CAPTURE phase at construction and calls preventDefault()+stopPropagation() for any Ctrl/Cmd+F regardless of what is focused. The default workspace layout is a terminal pane beside a file/diff pane, so: (a) pressing Ctrl+F with the caret inside the Monaco file editor never reaches Monaco's find widget — the terminal's search box opens instead and steals focus (queueMicrotask focus, :135); (b) AnnotateBlameComponent's scoped find listens on `(document:keydown)` in the bubble phase (annotate-blame.component.ts:126,322-333), which the capture-phase stopPropagation guarantees will never fire; (c) with two terminal panes in a split, both instances' listeners run (same target, stopPropagation does not stop other listeners on window), so two search boxes open at once and only one can hold focus.

  ```
  terminal.component.ts:99
      window.addEventListener("keydown", this.onWindowKeydown, true);
  terminal.component.ts:130-136
    private onWindowKeydown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && (e.key === "f" || e.key === "F")) {
        e.preventDefault();
        e.stopPropagation();
        this.open();
  ```

<sub>Verifier: terminal.component.ts:94-103 registers the handler on window in the capture phase from the constructor, and :130-137 preventDefault()+stopPropagation() on any Ctrl/Cmd+F with no focus or hit test. Window capture is the first phase to run, so Monaco's container-level handler and AnnotateBlameComponent's (document:keydown) bubble handler (annotate-blame.component.ts:132,322-333) can never see the key while any terminal pane is mounted, which is the app's default terminal+file/diff split. The N-panes claim also holds: stopPropagation does not stop other listeners on the same node (that needs stopImmediatePropagation), so every mounted terminal opens its own box.</sub>

### 27. A conflicted file with no diff3 markers (modify/delete, binary) can never be staged, so the merge can never be committed from the UI

`src/app/workspace/git/conflict-view.component.ts:567` — logic

**How it breaks.** `conflict_files` (src-tauri/src/git/gix_backend.rs:1685-1714) lists every path with an index entry above stage 0, and reads `merged` straight from the worktree. For a modify/delete conflict there is no stage-3 entry and git leaves the worktree file with plain content and NO `<<<<<<<` markers; for a delete/modify the file may be gone entirely (`read_to_string(...).unwrap_or_default()` → ""); a binary conflict is lossy-decoded garbage. parseConflictSegments then finds 0 conflict segments, `conflictCount` forces the count to 1 via `|| 1`, and `resolvedCount` stays 0 because the template renders no conflict block to click. `Stage file` is therefore permanently disabled ([kjDisabled]="resolvedCount(f) !== conflictCount(f)"), `allDone()` never becomes true, `Commit merge` stays disabled and the progress meter never reaches 100%. The user's only exits are Abort or handing the merge to the agent's PTY.

  ```
  184  <kj-button ... [kjDisabled]="resolvedCount(f) !== conflictCount(f) || busy()" ... (click)="stageFile(f)">
  565  conflictCount(f: ConflictFile): number {
  566    if (f.resolved) return this.segCountOf(f);
  567    return parseConflictSegments(f.merged).filter((s) => s.type === "conflict").length || 1;
  568  }
  573  resolvedCount(f: ConflictFile): number {
  575    const rm = this.resMap()[f.path] ?? {};
  576    return Object.values(rm).filter((r) => r.res).length;
  ```

<sub>Verifier: Re-derived end to end. `conflict_files` (src-tauri/src/git/gix_backend.rs:1685-1714) lists every path with a stage>0 index entry and sets `merged: std::fs::read_to_string(workdir.join(&path)).unwrap_or_default()`, so a deleted worktree file or a non-UTF8 blob yields "", and a modify/delete leaves the merged-tree content with no markers (reset_to_tree writes the merge tree at :1666 before the conflict index is applied at :1668-1671). conflict-view.component.ts:567 then returns `…filter(conflict).length || 1` = 1 while resolvedCount (:573-577) stays 0 (resMap is only written by resolveSeg/acceptAll, and acceptAll iterates confIdxs() which is empty), so `[kjDisabled]="resolvedCount(f) !== conflictCount(f) || busy()"` (:184) is permanently true and allDone (:589-592, every file `resolved`) never flips. Commit merge (:148) stays disabled. Abort or the agent PTY are the only exits.</sub>

### 28. After a reload with every conflict already staged, Commit merge is permanently disabled and only Abort remains — which discards the staged resolutions

`src/app/workspace/git/conflict-view.component.ts:589` — correctness _(added by verifier)_

**How it breaks.** User hits conflicts on a merge, stages every file in the 3-way view, then reloads/restarts the app before pressing Commit merge (the conflict GitView is persisted, ui.store.ts:77-82,179 → workspace.store.ts:187, so the pane comes back). The recovery effect calls session_state, gets state "merge", and calls ConflictStore.load, which re-reads conflict_files — but staged files have collapsed to stage 0 and are gone from the index, so `files` is []. `allDone()` requires `s.files.length > 0`, so it is false: the Commit merge button stays disabled forever and the main pane reads "no conflicted files". The only enabled action is Abort, which runs merge_abort = reset_to_tree(HEAD) and throws away every staged resolution the user just made. The merge can only be finished from the agent's PTY.

  ```
  conflict-view.component.ts:589-592 `readonly allDone = computed(() => { const s = this.session(); return !!s && s.files.length > 0 && s.files.every((f) => f.resolved); });` — gating :148 `<kj-button ... [kjDisabled]="!allDone() || busy()" (click)="commitMerge()">`. The empty-list source is conflict.store.ts:43-46, whose own comment states "Files already resolved before the reload are gone from the index, so the denominator restarts at the remaining set", plus gix_backend.rs:1685-1691 which only lists index entries with a stage above Unconflicted. Abort → conflict-view.component.ts:697-708 → gix_backend.rs:1750-1760 reset_to_tree(head_tree).
  ```

### 29. Read-only commit/range history views offer permanent worktree Delete/Rename on the historical file paths

`src/app/workspace/git/diff-file-list.component.ts:329-343` — data-loss

**How it breaks.** DiffFileListComponent is the file list of the commit and range diff views (commit-diff-view.component.ts:132-139, range-diff-view.component.ts:321-328), which agent-git-view.component.ts:10-15 documents as "read-only inspection" views. Its row context menu invokes FileDelete/FileRename against the agent's live worktree with the path as recorded in that historical commit. `delete_path` (src-tauri/src/agents/fs_commands.rs:85-94) is an unrecoverable remove_file / remove_dir_all — no trash. So right-clicking a file while browsing last week's commit deletes whatever lives at that path in the worktree today, including uncommitted edits (and edits.close then drops the in-memory buffer). The confirm popup names only the file, giving no hint that this touches the working tree rather than the commit. Nothing refreshes the list afterwards either, so the deleted/renamed file stays listed and selectable with its old diff.

  ```
  329  async confirmDelete(): Promise<void> {
  332    const path = m.file.path.replace(/\\/g, "/");
  336    this.closeMenu();
  338      await this.bridge.invoke(Commands.FileDelete, { id: this.agent().id, path });
  339      this.edits.close(this.agent().id, path);
  305      await this.bridge.invoke(Commands.FileRename, { id: this.agent().id, from, to });
  ```

<sub>Verifier: DiffFileListComponent is the file list of both read-only inspection views (commit-diff-view.component.ts:83-90, range-diff-view.component.ts:63-70; agent-git-view.component.ts:10-15 calls them "read-only inspection"), and its row context menu (diff-file-list.component.ts:179-200) invokes Commands.FileRename (:305) and Commands.FileDelete (:338) with `this.agent().id` + the historical commit path against the live worktree; delete_path (src-tauri/src/agents/fs_commands.rs:85-94) is remove_file/remove_dir_all with no trash. Neither handler refreshes GitInspectStore, unlike the working-tree list. One correction to the finder: delete does go through a confirm popup (:191-199) — it just names the file only, with nothing saying the worktree rather than the commit is affected; rename (:296-311) has no confirmation at all.</sub>

### 30. File-history compare ignores the A selection and the Swap button — the diff is always compareSha^..compareSha

`src/app/workspace/git/file-history-view.component.ts:347-360` — logic

**How it breaks.** The diff effect depends only on `compareSha` and calls `loadCommitFileDiff(id, cSha, path)`, which the backend implements as "this commit vs its FIRST PARENT" (src-tauri/src/git/gix_backend.rs:2083-2093). `baseSha` only drives the A chip and the context bar. With revisions [c3,c2,c1] the seed A=c2/B=c3 happens to match, so the feature looks right; click c1 (pick() assigns it to A because idx(c1) > idx(c3)) and the header now reads A=c1 B=c3 while the diff body still shows c2..c3 — the changes made in c2 are silently missing. `swap()` is worse: it makes compareSha the OLDER commit, so the pane shows that older commit's own parent-diff instead of a reversed A/B diff. A `range_diff(from,to)` command exists in the backend and is unused here.

  ```
  347    effect(() => {
  348      const id   = this.agent().id;
  350      const cSha = this.compareSha();
  351      if (!cSha) { this.activeDiff.set(null); return; }
  359      untracked(() => void this.loadRevisionDiff(id, cSha, p, gen));
  365    this.store.loadCommitFileDiff(id, sha, path);
  409  swap(): void { ... this.baseSha.set(c); this.compareSha.set(b); }
  ```

<sub>Verifier: gix_backend.rs:2083-2102 `commit_file_diff` diffs the commit's tree against `commit.parent_ids().next()` — first parent only; `sha` is the sole revision argument. file-history-view.component.ts:347-360 tracks only compareSha and calls loadRevisionDiff(id, cSha, p) (:365 → store.loadCommitFileDiff), so baseSha never reaches the backend; it only feeds baseRev/the A chip (:302-304, template :131-132, :142-144). swap() (:406-411) makes compareSha the older sha, so the pane shows that older commit's own parent-diff. A `range_diff(from,to)` exists (gix_backend.rs:2104, backend.rs:116, wired at agents/commands.rs:729) and is not used here. pick() (:392-404) also can never move B off revs[0] by clicking (idx(sha) > idx(c) always sends the click to A), which makes the mismatch permanent once the seed is in place.</sub>

### 31. Codex hook install silently overwrites the user's own [hooks] entries in ~/.codex/config.toml

`src-tauri/src/agents/adapters/codex.rs:114` — data-loss _(added by verifier)_

**How it breaks.** Unlike the JSON adapters, which keep every user group and only append (mod.rs:516-523), the codex path assigns five flat scalar keys unconditionally. A codex user who has their own `[hooks] pre_tool_use = "~/bin/my-audit.sh"` (or post_tool_use / permission_request / session_start / stop) loses it on the first Orrery launch: the prior value is never read, never merged, never backed up, and their hook stops firing. install_global_hooks runs on every start (src-tauri/src/lib.rs:171), so re-adding the line by hand is undone at the next launch. codex.rs:100-102 claims the merge is 'preserving every other key and comment' — true only for keys Orrery does not write, and the five it writes are precisely the ones a hook-using codex user cares about.

  ```
  codex.rs:114-123:
          doc["hooks"]["pre_tool_use"] = value(pre);
          doc["hooks"]["post_tool_use"] = value(post);
          doc["hooks"]["permission_request"] = value(permission);
          doc["hooks"]["session_start"] = value(session);
          doc["hooks"]["stop"] = value(stop);
  (no read of the existing value, no append; contrast mod.rs:519 `.filter(|g| !is_orrery(g))` which keeps the user's groups)
  ```

### 32. enrich() always reports commits = 0, so the Push button is disabled and commit counts read 0 for every agent after an app restart

`src-tauri/src/agents/service.rs:160` — logic

**How it breaks.** enrich() hard-codes commits: 0 (and elapsed/progress/pending) for every agent returned by list()/get(), and nothing on the backend ever fills it. The frontend treats the field as authoritative: src/app/workspace/git-action-bar.component.ts:34 `@let ahead = ag.commits > 0;` and :71 `[kjDisabled]="!ahead"` gate the Push button, and src/app/agents/agent-actions.service.ts:349 disables the 'Push to origin' menu item on `!ag.commits`. The only writer is an optimistic in-session patch after a commit made through orrery's own UI (agent-actions.service.ts:100, patchRuntime commits: ag.commits + 1); the runtime overlay is merged over the backend agent (agent-runtime.service.ts:47-53) and starts empty each launch. So the normal case — the CLI agent makes the commits itself, or the app is restarted — leaves commits at 0 forever, Push is permanently greyed out, and the overview/timeline/graph cards show '0c' for agents that are many commits ahead.

  ```
  144  fn enrich(&self, rec: AgentRecord) -> Agent {
  ...
  159      ticket_id: rec.ticket_id,
  160      commits: 0,
  161      elapsed: 0,
  162      progress: 0.0,
  163      pending: Vec::new(),
  ```

<sub>Verifier: service.rs:160 hard-codes commits: 0 in enrich(), and grep over src-tauri shows no other writer of Agent.commits. src/app/workspace/git-action-bar.component.ts:34 `@let ahead = ag.commits > 0` and :71 `[kjDisabled]="!ahead"` gate Push; agent-actions.service.ts:349 disables 'Push to origin' on `!ag.commits`. The only writer is the optimistic patchRuntime at agent-actions.service.ts:100, merged over the backend agent by the overlay at agent-runtime.service.ts:47-53, which starts empty every launch. So after a restart, or whenever the CLI agent commits itself, Push is permanently greyed out and the overview/timeline/graph cards show 0.</sub>

### 33. A branch template without {name} puts every agent of a project on ONE branch, with several worktrees checked out on it and no warning

`src-tauri/src/agents/service.rs:201` — correctness _(added by verifier)_

**How it breaks.** branch_from_template (service.rs:803-816) only substitutes the tokens present in the template and sanitizes the result; there is no requirement that the expansion be unique per agent and no post-check that the branch is free. The Settings row is free text with no validation (src/app/modals/settings-modal.component.ts:361-372; the preview at :1031-1040 just substitutes tokens), so a user who types e.g. 'wip/{date}' or 'feature' gives every agent spawned that day the identical branch. create_worktree then takes the existing-branch arm and registers a SECOND worktree whose HEAD is 'ref: refs/heads/<same branch>' — real git refuses this ('is already checked out'), gix_backend.rs:2419-2497 has no such check. Both agents' worktrees now share one ref: a commit made in agent A moves the branch under agent B, whose index and HEAD no longer match its working tree, so B's next status/commit sees A's files as spurious changes. Nothing in the settings UI or the spawn flow warns that {name} is load-bearing.

  ```
  service.rs:201 `let branch = branch_from_template(&prefs.branch_template, &wt_name, &req.tool, &mmdd_now());` — no uniqueness check follows; service.rs:806-808 `let t = if t.is_empty() { "agent/{name}" } else { t }; let raw = t.replace("{name}", wt_name)...` (a template with no {name} token expands to a constant); gix_backend.rs:2461-2468 only rejects a duplicate WORKTREE NAME, never a branch already checked out elsewhere.
  ```

### 34. create_worktree failure is swallowed with log::warn; the agent row is still inserted pointing at a directory that does not exist

`src-tauri/src/agents/service.rs:209-217` — error-handling

**How it breaks.** spawn() calls git.create_worktree and discards any error with a log::warn, then unconditionally INSERTs the row (service.rs:219-257) with `worktree` = wt_path and `branch` = the template branch. Any real failure — a base ref that cannot be resolved (base_id in gix_backend), a name already registered (gix_backend.rs:2464-2468), a non-empty target dir (2453-2462), a checkout error, or a permission problem on the worktree root — produces an agent that looks completely normal in the sidebar (status 'idle', a branch name) but has no directory and no branch. The user only discovers it on Start, where rt.start returns the bare string 'worktree not found' (src-tauri/src/runtime/mod.rs:181-183); every git/tree/file command on it silently returns empty (gix open fails -> empty status/tree). Nothing offers a retry or repair, and the phantom branch name is displayed as if the branch existed.

  ```
  209  // best-effort: only real git projects get a worktree
  210  if self.git.detect(project_path) {
  211      if let Err(e) =
  212          self.git
  213              .create_worktree(project_path, &wt_name, &branch, Some(&req.base), &wt_path)
  214      {
  215          log::warn!("worktree create failed for agent {id}: {e:?}");
  216      }
  217  }
  ```

<sub>Verifier: service.rs:210-217 discards the create_worktree error with log::warn and the INSERT at 219-257 runs unconditionally with worktree=wt_path and branch=the template branch. gix_backend.rs:2453-2462 ('exists and is not empty') and :2464-2468 ('a worktree named X is already registered') are real, reachable failure returns, as are base_id/checkout errors. runtime/mod.rs:181-183 confirms the only user-visible symptom is `Err("worktree not found")` on Start. Nothing repairs or retries.</sub>

### 35. project_remove never stops the project's running agents — PTYs keep running unreachable, and their scrollback/watchers/history/git metadata leak

`src-tauri/src/projects/commands.rs:74-86` — resource-leak

**How it breaks.** `agent_remove` documents the required order explicitly: stop the PTY first, then drop the scrollback ring, unwatch the worktree, purge local history, and only then delete rows and worktree (agents/commands.rs:96-121). `project_remove` does none of it — it deletes the project row, then calls `remove_for_project`, which only DELETEs the rows and `remove_dir_all`s the folders. Concretely: with two agents running, remove their project. Both PTY children keep running (they only die at app exit via `RuntimeService::stop_all`, runtime/mod.rs:477-489, or the Windows Job Object), still burning CPU and API tokens, with no UI row left to stop them. Because those processes hold their cwd open, `remove_dir_all` fails on Windows — the exact reason agent_remove's comment gives — and the error is swallowed by `let _ =`, so the worktree folders survive while their DB rows are gone. `scrollback::rings()` keeps a ring per removed agent id for the rest of the session (only `rt.drop_scrollback` removes it), WatchService keeps watching the worktree, and HistoryService's snapshots on disk are never purged. `git.remove_worktree` is also skipped, so the user's real repo keeps stale `.git/worktrees/<name>` registrations that later block re-creating an agent with the same name.

  ```
  src-tauri/src/projects/commands.rs:74-77
      crate::perf::timed("project_remove", || {
          svc.remove(id)?;
          // cascade: drop the project's agents and announce each removal
          if let Ok(removed) = agents.remove_for_project(id) {
  
  src-tauri/src/agents/commands.rs:96-100 (the path that does it right)
      // Kill the agent's PTY first (no-op when idle) — a live process holds its
      // cwd open, which makes remove_dir_all fail on Windows.
      rt.stop(id);
      rt.drop_scrollback(id); // the A1.2 ring dies with the agent
      watch.unwatch(id); // stop watching its worktree before tearing it down
  ```

<sub>Verifier: projects/commands.rs:74-95 calls only `svc.remove(id)` and `agents.remove_for_project(id)`; there is no `rt.stop`, `rt.drop_scrollback`, `watch.unwatch`, `history.purge` or `git.remove_worktree`, all of which agents/commands.rs:96-105 performs and documents as required ("a live process holds its cwd open, which makes remove_dir_all fail on Windows"). `project_remove` does not even take `State<RuntimeService>`/`WatchService`. RuntimeService keeps the Proc until `stop_all()` at runtime/mod.rs:477-489 (app exit), and scrollback rings are only dropped via `drop_scrollback` (runtime/mod.rs:519-521), so the PTY, ring, watcher and history snapshots all outlive the deleted rows with no UI row left to stop them.</sub>

### 36. settings_set silently erases every key the Rust struct doesn't know — `keymapTerminal` never survives a restart

`src-tauri/src/settings/commands.rs:16-23` — logic

**How it breaks.** `settings_set` takes a typed `Settings`, so Tauri deserializes the incoming document into the struct (unknown keys discarded — there is no `deny_unknown_fields` and no capture map) and `svc.set(&settings)` writes back only the struct's fields. The round-trip is therefore lossy, which contradicts settings/mod.rs:1-5's claim that documents written by NEWER versions load cleanly: they load, but the first save from this build deletes their extra keys. The live instance of this: the frontend has `keymapTerminal: Record<string, boolean>` (models.ts:463), a real UI toggle (settings-modal.component.ts:538, `toggleTerminal` → `setKeymapTerminal` at :922-925), and there is no `keymap_terminal` field anywhere in src-tauri. So a user flips "fires inside the terminal" for Ctrl+Shift+F, the debounced `settings_set` persists a document without that key, the command works for the rest of the session (the frontend keeps its own signal and ignores the echoed doc — settings.store.ts:246 `.catch(...)` discards the response), and after the next launch `settings_get` returns no `keymapTerminal`, `merge` fills `{}` (settings.store.ts:181) and every terminal override is silently back to default.

  ```
  src-tauri/src/settings/commands.rs:16-19
  pub fn settings_set(svc: State<'_, SettingsService>, settings: Settings) -> AppResult<Settings> {
      crate::perf::timed("settings_set", || {
          svc.set(&settings)?;
  
  src/app/models.ts:463
    keymapTerminal: Record<string, boolean>;
  
  (grep for `keymap` in src-tauri/src returns only `keymap: BTreeMap<String,String>` at settings/model.rs:42 — no keymap_terminal field exists)
  ```

<sub>Verifier: settings/commands.rs:16-23 takes `settings: Settings` and `svc.set(&settings)` re-serializes only the struct's fields (service.rs:85-96), so the round-trip is lossy. `keymapTerminal` exists in the frontend as a real per-command toggle (models.ts:463, settings-modal.component.ts:549-557 `toggleTerminal` → store.setKeymapTerminal at :922-925, read by command-registry.service.ts:142,230) and grep finds no `keymap_terminal` anywhere in src-tauri. On restart `merge` fills `{}` (settings.store.ts:181) and every override is gone. Overlaps rust-core-data-03 in root cause but is a distinct missing field with its own user-visible loss, so not a duplicate.</sub>

### 37. Settings key mismatch `toolPaths` (Rust) vs `toolPath` (frontend): the manual tool-executable override never persists and is never used

`src-tauri/src/settings/model.rs:29` — logic

**How it breaks.** `Settings::tool_paths` under `#[serde(rename_all = "camelCase")]` (model.rs:14) serializes as `toolPaths`. The frontend's `Settings` declares `toolPath` (models.ts:446), writes it via `store.setMap("toolPath", …)` from the Settings tool row (runtime-row.component.ts:221) and reads it back as `p.toolPath` (settings.store.ts:179). Nothing bridges the two names. So: the user opens Settings → Agent defaults, sets `/opt/cursor` as Cursor's executable, and the save invoke sends `toolPath: {cursor: "/opt/cursor"}`, which the backend drops as an unknown key and persists `toolPaths: {}`. Every Rust consumer of the override then reads an always-empty map — `detect_tools` (agents/commands.rs:948) and the spawn path's `program_override` (agents/commands.rs:331-335) — so the override is ignored for both detection and launch, not just after a restart. On restart `p.toolPath` is undefined and the row shows blank again. The whole "tool found but couldn't run → point at the binary" recovery flow is inert.

  ```
  src-tauri/src/settings/model.rs:26-29
      /// Per-tool manual executable path override (tool id → absolute path). Set
      /// when auto-detection on PATH is wrong or a tool was found-but-couldn't-run.
      /// A non-empty entry wins over the PATH lookup for both detection and launch.
      pub tool_paths: BTreeMap<String, String>,
  
  src/app/models.ts:446
    toolPath: Record<string, string>;
  
  src-tauri/src/agents/commands.rs:331-335
          let program_override = cfg
              .tool_paths
              .get(&agent.tool)
  ```

<sub>Verifier: settings/model.rs:29 `tool_paths` under `rename_all="camelCase"` serializes as `toolPaths` (asserted by the test at model.rs:158); the frontend declares `toolPath` (models.ts:446), writes `setMap("toolPath", …)` (runtime-row.component.ts:221) and merges `p.toolPath` (settings.store.ts:179). A repo-wide grep for `toolPaths`/`tool_paths` finds no bridge — only the Rust readers agents/commands.rs:331-335 (spawn override) and :948 (detect_tools). Since `settings_set` takes a typed `Settings`, the incoming `toolPath` key is dropped on the first save, so the override is inert immediately, not just after restart.</sub>

### 38. A ticket's project can be set but never cleared — choosing "No project" silently reverts

`src-tauri/src/tickets/service.rs:189-193` — logic

**How it breaks.** `TicketUpdateRequest.project_id` is `Option<Uuid>`, and `update` treats `None` as "leave unchanged". JSON `null` deserializes to `None`, so "clear it" and "don't touch it" are indistinguishable on the wire. The ticket page's project dropdown offers `{ value: "", label: "No project" }` (ticket-page.component.ts:442-445) and Save sends `projectId: this.draftProjectId() || null` (:555) — i.e. exactly `null` when the user picks "No project". The backend leaves `project_id` untouched, the UPDATE re-writes the old value, and the echoed `Ticket` / `ticket://updated` payload carries the old project, so the UI snaps the dropdown back to the original project. Unassigning a ticket from a project is impossible from the UI. The same shape bites notes: `notes: this.draftNotes() || undefined` (:554) omits the key when the user empties the notes body, so `req.notes` is `None` and the old notes are restored. The in-code comment at :189-190 acknowledges the ambiguity but calls the wire shape settled — the frontend was written assuming it works.

  ```
  src-tauri/src/tickets/service.rs:189-193
          // project_id: Option<Option<Uuid>> would be ideal for "clear vs no-change",
          // but the request shape uses Option<Uuid> per spec — Some(id) sets it, None leaves it.
          if let Some(pid) = req.project_id {
              rec.project_id = Some(pid);
          }
  
  src/app/backlog/ticket-page.component.ts:552-556
          await this.ticketsStore.update(id, {
            title,
            notes: this.draftNotes() || undefined,
            projectId: this.draftProjectId() || null,
          });
  ```

<sub>Verifier: tickets/model.rs:134 `project_id: Option<Uuid>` and service.rs:189-193 `if let Some(pid) = req.project_id` — JSON `null` and an absent key are both `None` = leave unchanged, and the UPDATE at :200-212 re-writes `rec.project_id`. ticket-page.component.ts:442-445 offers `{value:"",label:"No project"}` and :555 sends `projectId: this.draftProjectId() || null`, so picking "No project" is a no-op and the echoed Ticket snaps the dropdown back. The same shape applies to `notes: this.draftNotes() || undefined` at :554 (emptying the body restores the old notes).</sub>

### 39. agent_checkout never updates the agent row's branch, so Push afterwards pushes the old branch

`src-tauri/src/git/branches.rs:178` — correctness _(added by verifier)_

**How it breaks.** agent_checkout switches the worktree's HEAD and nothing else: no UPDATE of the agents table (the only `UPDATE agents SET` statements are status/task/model/name at agents/service.rs:283, started at :527, session_id at :539, and the startup status reset at :571), and Agent.branch is served straight from the stored record by enrich() (agents/service.rs:144-167). A real agent's branch is never re-read from HEAD — only the shell pseudo-agent reads it live (agents/service.rs:757-759). So after the user checks a different branch into an agent's worktree from the Branches panel (branches-panel.component.ts:274-278 -> checkoutAgent -> Commands.AgentCheckout), the sidebar/action bar keeps showing the old branch, and Push runs `git push -u origin <stored branch>` from that worktree (AgentService::push, agents/service.rs:368-372 -> backend.rs:202-206): the commits the user just made on the newly checked-out branch are not pushed, a stale branch is pushed and given an upstream instead, and the UI flashes "pushed <agent>". The AI verbs have the same defect (agents/prompts.rs:14 types `git push -u origin {branch}` from the stored value).

  ```
  branches.rs:184-189 `crate::perf::timed("agent_checkout", || { agents.git().checkout_branch(Path::new(&agents.get(id)?.worktree), &branch) })` — the command's whole body; agents/service.rs:368-372 `pub fn push(&self, id: Uuid) -> AppResult<()> { let rec = self.record(id)?; self.git.push(Path::new(&rec.worktree), "origin", &rec.branch) }`
  ```

### 40. create_worktree silently reuses an existing branch and ignores the requested base

`src-tauri/src/git/gix_backend.rs:2432-2444` — logic

**How it breaks.** When `refs/heads/<branch>` already exists, `create_worktree` takes its tip and drops the `base` argument entirely — no error, no log. Branch names are derived from the agent name only (`branch_from_template` default `agent/{name}` over the slug from `worktree_name`, agents/service.rs:803-816, :47-68) and `remove_worktree` never deletes a branch (nothing outside git/ calls `branch_delete`). So: create agent "fix login" (branch `agent/fix_login`, commits made), delete it, create a new agent with the same name and base `main` → the new worktree is checked out at the OLD agent's tip, not at `main`, while the agent row records `base: "main"` and the UI shows it. The user's fresh agent starts on top of abandoned work. `base_id` (:175-184) compounds this by falling back to HEAD whenever the requested base does not resolve, also without error.

  ```
  2432:            let tip = match repo.find_reference(full_ref.as_str()) {
  2433:                Ok(mut r) => r.peel_to_id().map_err(err("branch"))?.detach(),
  2434:                Err(_) => {
  2435:                    repo.reference(
  2436:                        full_ref.as_str(),
  2437:                        base,
  ...
  2442:                    base
  ```

<sub>Verifier: gix_backend.rs:2432-2444: when refs/heads/<branch> resolves, `tip` is its existing tip and `base` (computed at :2430) is never used again; no log, no error. The re-use chain is real: AgentService::worktree_name (agents/service.rs:47-67) slugs the NAME only, the `-<id6>` suffix is added only while another agent with that name still exists in the project (:193-196), branch_from_template defaults to `agent/{name}` (:802-816), and nothing outside git/ ever calls branch_delete (grep) — remove_worktree (:2502-2523) deletes only the registration and folder. So delete agent "fix login", recreate it, and the new worktree is checked out at the OLD tip while the row stores base:"main". base_id (:175-184) does fall back to HEAD silently on an unresolvable base, compounding it.</sub>

### 41. remove_worktree treats a name it does not know as success, leaking the registration and blocking the branch forever

`src-tauri/src/git/gix_backend.rs:2510-2512` — logic

**How it breaks.** `remove_worktree` returns Ok when `<common>/worktrees/<name>` is missing. The caller derives that name from the recorded FOLDER basename (agents/service.rs:428-433) while `create_worktree` registered the un-suffixed `wt_name`: on a cross-project name clash `spawn` suffixes only `wt_path` (`wt_path = root.join(format!("{wt_name}-{id6}"))`, agents/service.rs:203-206) and leaves `wt_name` alone, so folder and registration names differ. Deleting that agent therefore silently leaves `<common>/worktrees/<wt_name>` behind forever — nothing in the app prunes registrations. Consequences: `occupancy()` keeps reporting the dead worktree as holding its branch, so `branch_delete`/`branch_rename` of that branch are refused permanently with "checked out in worktree 'x'"; `repo_layout` keeps feeding the watcher a path that no longer exists (watch/mod.rs:170); and creating an agent with the same name again fails the duplicate check at :2464-2468, which `spawn` only `log::warn!`s (agents/service.rs:210-216) — the agent row is created with a worktree path that has no repo.

  ```
  2509:        let private = registration_dir(&repo, wt_name);
  2510:        if !private.is_dir() {
  2511:            return Ok(());
  2512:        }
  (agents/service.rs:429  .file_name() -> the possibly-suffixed FOLDER name)
  (2464:            if private.exists() { ... "already registered" })
  ```

<sub>Verifier: gix_backend.rs:2509-2512 returns Ok when the registration dir is missing, and agents/service.rs:428-433 derives the name from `Path::new(&rec.worktree).file_name()` — the FOLDER basename — while create_worktree registered `wt_name` (:2461, registration_dir(&repo, wt_name)). spawn suffixes only the path (`wt_path = root.join(format!("{}-{}", wt_name, &id.to_string()[..6]))`, agents/service.rs:203-206) because the worktree root is one flat shared directory, so a same-named agent in a second project is enough to desynchronise them. The leaked registration keeps a live gitdir file, so gix's `worktrees()` still yields a proxy for it and occupancy (:441-460) keeps mapping that branch to a dead worktree — branch_delete/branch_rename refuse it forever via occupied_err (:999-1006) — and a later agent with the same name hits `already registered` (:2463-2468), which spawn only log::warn!s (agents/service.rs:208-217) before inserting a row whose worktree path was never created.</sub>

### 42. A permission ask answered in the agent's own terminal is never cleared — the card stays "needs your input" for the rest of the run

`src-tauri/src/hooks/mod.rs:262` — correctness _(added by verifier)_

**How it breaks.** The bridge emits agent://permission when the ask arrives (mod.rs:262-277 → emit_permission) but no arm ever emits a counterpart when the ask is resolved: ToolStart/ToolEnd/TurnEnd only touch emit_status/agent://activity (mod.rs:282-298). On the frontend, hook-driven tools derive needsInput purely from `hasPendingPermission(id)` (agent-runtime.service.ts:515, 543-546), and the only auto-resolve paths are onExit (`dismissPendingFor(id, ["permission","question"])`, :560) and detectNeedsInput — which returns immediately for hook-driven tools (:622). The documented flow is that the user approves in the agent's own terminal (fire-and-forget tradeoff), and that produces no event that clears the pending alert. Scenario: Claude asks to run `git push`, the user presses y in the terminal, the turn continues and finishes; the overview card and the alert list keep flagging that agent as needing input until the process exits or the user manually dismisses.

  ```
  mod.rs:262-277 — `AgentEvent::PermissionRequest {..} => { emit_permission(...); event.activity_detail() }` (no status emit, no resolve emit anywhere in handle). agent-runtime.service.ts:515 `return { working, needsInput: this.hasPendingPermission(id) };`; :622 `if (this.hookDriven(ag.tool)) return;` guarding the only other dismissPendingFor call (:635); :560 `this.notifications.dismissPendingFor(id, ["permission", "question"]);` inside onExit.
  ```

### 43. Claude's idle-prompt Notification is mapped to status "working", so an idle agent is shown as busy indefinitely

`src-tauri/src/hooks/mod.rs:285-290` — logic

**How it breaks.** Claude Code fires the `Notification` hook both when it needs permission AND when the prompt has been idle for ~60s. `parse()` turns the idle case into `AgentEvent::Notification` (only `permission_prompt` is promoted to a PermissionRequest, protocol.rs:293), and `handle()` groups every Notification into the working-status arm. Scenario: the agent finishes a turn (Stop -> emit_status "idle"); the user walks away; 60s later Claude emits Notification{notification_type:"idle_prompt"}; the bridge emits agent://status "working"; the frontend sets hookState=working and, because status is only re-emitted on a change, the card stays "working" until the next turn actually ends. The same file's own taxonomy contradicts this: kind() classifies idle_prompt/needs_input as "question", not activity.

  ```
  mod.rs:282-290
          | AgentEvent::Notification { .. }
          | AgentEvent::UserPrompt { .. }
          | AgentEvent::SessionStart { .. } => {
              emit_status(emit, last_status, &env.agent_id, "working");
  protocol.rs:394-396
              AgentEvent::Notification { kind, .. } => match kind.as_deref() {
                  Some("permission_prompt") | Some("needs_input") | Some("idle_prompt") => "question",
  ```

<sub>Verifier: Re-read src-tauri/src/hooks/mod.rs:282-290: every `AgentEvent::Notification` falls into the arm that calls `emit_status(..., "working")`, and only `notification_type == "permission_prompt"` with `env.tool == "claude"` is promoted away from Notification (protocol.rs:291-299). The author's own test data treats `idle_prompt` as a real Claude notification_type (protocol.rs:879-905: "confirmed notification_type values include permission_prompt / idle_prompt"), and protocol.rs:395 classifies it as "question", not activity. Frontend liveState (src/app/agents/agent-runtime.service.ts:509-515) makes `hookState==='working'` sticky with no recency correction, so an idle agent reads busy until the next Stop.</sub>

### 44. Digest fold is O(line²) with no per-line cap, executed under the global mux lock

`src-tauri/src/runtime/digest.rs:98-133` — other

**How it breaks.** `fold_lines` caps the NUMBER of lines (FOLD_CAP=40) but never the LENGTH of a line, and every printable char goes through `write_at`, which collects the whole current line into a `Vec<char>` and builds a fresh `String` (digest.rs:124-133). Folding a line of length L therefore costs O(L²). `OutputMux::push` calls this while holding the process-wide `state` mutex (output_mux.rs:146 acquires, :157 folds). A digest-mode agent that prints one very long line without a newline — minified JSON, a base64 blob, `npm ls --json`, a long single-line stack trace — accumulates it in `d.lines` across 16KB chunks: at 200KB the fold does ~2e10 char copies, seconds-to-minutes of CPU, all with the mux lock held. During that time every other agent's `push` blocks, the drain thread cannot emit a frame, every exiting agent's `take()` blocks, all PTY readers back up, and the agents' kernel PTY buffers fill and stall the agent processes themselves. The digest line also grows without bound in memory.

  ```
  output_mux.rs:146  let mut st = self.state.lock().unwrap();
  output_mux.rs:157      digest::fold_lines(&mut d.lines, &chunk, digest::FOLD_CAP);
  digest.rs:98           write_at(lines.last_mut().expect("non-empty"), col, &s);
  digest.rs:124          let chars: Vec<char> = line.chars().collect();
  digest.rs:126          next.extend(chars.iter().take(col));
  ```

<sub>Verifier: Re-read digest.rs:95-100 and :123-134: every printable char calls write_at, which does `let chars: Vec<char> = line.chars().collect()` plus a fresh String, so folding L chars onto one line is O(L^2); FOLD_CAP (digest.rs:18/85-88) bounds only the line COUNT, nothing truncates a line's length. output_mux.rs:146 takes `self.state.lock()` and :157 calls fold_lines while still holding it, so the cost is paid under the process-wide mux lock that every other agent's push (:146), the drain thread (:315) and every exiting agent's take() (:246) contend on. Additional consequence the finder did not name: the uncapped line is also what digest_loop ships — output_mux.rs:350 `tail_lines(&d.lines, DIGEST_LINES)` re-serializes that megabyte line every second, directly contradicting the module's own bound at output_mux.rs:35-37 ('(agents × ~300B)/s no matter how hard an agent floods').</sub>

### 45. Double-start race: is_running check and proc insert are not atomic, so two agent CLIs can run in one worktree

`src-tauri/src/runtime/mod.rs:177-392` — race-concurrency

**How it breaks.** `start()` checks `is_running(id)` at mod.rs:177 and only registers the Proc at mod.rs:384 — hundreds of microseconds later, with no lock held across the spawn. `agent_start` is `#[tauri::command(async)]` and the UI's play button decides from `ag.status`, which only flips after the backend round-trip (agent-actions.service.ts:247-250), so a double-click issues two `agent_start` invokes that both see is_running=false. Both open a PTY and spawn the tool in the SAME git worktree — two coding agents editing the same files. The second `insert` overwrites the first Proc, but the first PTY's reader thread holds its own cloned master fd (mod.rs:295), so dropping the map's master does not close the PTY and child #1 gets no hangup: it keeps running, untracked (`stop`/`stop_all` now only reach child #2), and both children's output is appended to the same scrollback ring and mux id (mod.rs:361-365) with two independent `seq` counters, interleaving the terminal. Secondary: the discarded `Option<Proc>` returned by `insert` drops the old master INSIDE the map lock, contrary to the file's own LOCK RULE at mod.rs:92-94 and the care taken at mod.rs:410-418.

  ```
  mod.rs:177  if self.is_running(id) {
  mod.rs:178      return Ok(());
  mod.rs:293  let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
  mod.rs:384  self.procs.lock().unwrap().insert(
  mod.rs:385      id,
  mod.rs:386      Proc {
  ```

<sub>Verifier: mod.rs:177 checks is_running and mod.rs:384 inserts, with openpty (:284), spawn_command (:293), and on Windows a full PATH walk in resolve_program_command (:726-741, :815-825) in between and no lock held across it. agent_start is `#[tauri::command(async)]` (agents/commands.rs:309) and nothing debounces the button: pane-node.component.ts:133 disables it only for `ag.status === 'done'`, and startProcess (agent-runtime.service.ts:377-398) has no in-flight guard. The survival claim also holds: `reader` comes from `pair.master.try_clone_reader()` (mod.rs:295), a dup of the master, so dropping the overwritten Proc's master does not hang up PTY #1 and child #1 keeps running untracked while both children append to the same ring/mux id (:361-365). The secondary LOCK-RULE point is also correct — in `self.procs.lock().unwrap().insert(...)` the returned Option<Proc> is a later temporary than the MutexGuard, so it drops (ConPTY teardown) while the map lock is still held, contradicting mod.rs:92-94 and the care taken at :410-418.</sub>

### 46. A relaunch restarts the batcher seq at 0 but nothing resets the renderer's dedup floor, so the new run's output is silently dropped

`src-tauri/src/runtime/mod.rs:328` — correctness _(added by verifier)_

**How it breaks.** The backend resets the ring and restarts `seq` at 0 on every (re)launch, while the renderer keeps a per-agent `minSeq` floor that is only ever set from a snapshot and only ever cleared on agent DISPOSAL. Sequence a normal user hits: (1) agent A runs and prints, say, 500KB; (2) the user switches to another pane, so A leaves stream interest and is marked stale (interest.service.ts:66-70); (3) the user comes back, A re-enters stream and recoverIfStale fires (interest.service.ts:74-76) → recover() sets minSeq = 500000 (terminal.service.ts:310); (4) the user pauses A and presses play in the same, still-visible pane. spawn_session resets the ring and the new batcher counts from 0, so live chunks arrive with seq 200, 1400, … all <= 500000, and terminal.service.ts:277 drops every one. Nothing re-runs recovery (markStale is only called from the interest diff, which is visibility-driven and does not change on relaunch) and terminals.exit() (terminal.service.ts:361-367) does not touch minSeq — so the restarted agent's terminal shows the '▪ process exited' line and then nothing at all until the new run has emitted 500KB.

  ```
  mod.rs:326-328  // A relaunch restarts the batcher seq at 0 — stale ring content tagged\n                // with the previous run's seqs would corrupt snapshot dedup.\n                scrollback::rings().reset(&id.to_string());\nscrollback.rs:102-107  pub fn reset(&self, id: &str) { ... r.end_seq = 0; }\noutput_batcher.rs:70  let mut seq: u64 = 0;\nterminal.service.ts:277  if (seq !== undefined && seq <= (this.minSeq.get(id) ?? -1)) return;\nterminal.service.ts:310      this.minSeq.set(id, snap.endSeq);\nterminal.service.ts:471    this.minSeq.delete(id);   // only inside dispose()
  ```

### 47. project_remove cascades agent deletion without unwatching: notify watcher, debounce thread and local-history dirs leak for the app's lifetime

`src-tauri/src/projects/commands.rs:77-86` — resource-leak

**How it breaks.** agent_remove is the only caller of WatchService::unwatch (src-tauri/src/agents/commands.rs:100) and of HistoryService::purge (:102-104). project_remove deletes the project's agents straight through AgentService::remove_for_project (src-tauri/src/agents/service.rs:499-511), which only touches SQLite. So removing a project leaves the ProjectWatcher in WatchService::projects forever: its RecommendedWatcher keeps every recursive OS watch it registered (agent worktrees, the main workdir, the common gitdir — watch/mod.rs:336-343), and project_loop never sees closed=true so its thread never returns (watch/mod.rs:481-483). agent_index also keeps the dead agent ids. The loop keeps waking on every fs event in the ex-project's tree and running scan_detail for agents whose records are gone. Separately, {app_data}/local-history/{agent_id} for each of those agents is never purged, so up to MAX_BLOB_BYTES (100MB, history/mod.rs:30) per agent stays on disk with nothing that can ever reclaim it. Add and remove a few projects in one session and you accumulate one leaked thread plus a full recursive watch set per project, and hundreds of MB of orphaned history.

  ```
  src-tauri/src/projects/commands.rs
  74	    crate::perf::timed("project_remove", || {
  75	        svc.remove(id)?;
  76	        // cascade: drop the project's agents and announce each removal
  77	        if let Ok(removed) = agents.remove_for_project(id) {
  78	            for aid in removed {
  79	                emit_entity(&app, "agent", Change::Deleted, ... );
  80	            }
  81	        }   // no watch.unwatch(aid), no history.purge(aid)
  ```

<sub>Verifier: Re-derived: `grep -rn '\.unwatch(|history\.purge'` over src-tauri/src returns only agents/commands.rs:100 and :103 (plus watch/mod.rs:242's self-unwatch and tests), so projects/commands.rs:74-94 really does cascade agent deletion with no unwatch and no purge. Every agent is watched at startup (src/app/agents/agent-runtime.service.ts:141-148 watches every agent once), so the leak is realized for each removed project: the ProjectWatcher stays in WatchService::projects, closed is never set (watch/mod.rs:391-397 is the only writer), project_loop never returns (:481-483), and {app_data}/local-history/{id} is never reclaimed. One correction to the finder's reasoning: AgentService::remove_for_project does NOT 'only touch SQLite' — it also does std::fs::remove_dir_all(&r.worktree) for every agent (agents/service.rs:514-517), which makes it worse, not better: the worktree tree is deleted out from under a still-registered recursive OS watch. Also note the post-removal scans are cheap, not full status walks: scan_detail early-returns an empty ScanResult once the record is gone (agents/service.rs:310-316).</sub>


## Medium (101)

One line each; full detail is in `.audit-work/pass1-merged.json`.

- `src/app/agents/agent-runtime.service.ts:386-398` **A failed start leaves working:true in the runtime overlay permanently** — startProcess optimistically patches {working:true} (:386) and the catch path only deletes startedAt, parks the tick and flashes - it never reverts the overlay.
- `src/app/agents/agent-runtime.service.ts:401` **stoppingByUser sticks when no exit follows agent_stop, suppressing the next run's finished notification** — Any agent_stop that produces no agent://exit (rejected invoke, or a PTY already dead while the row still reads running because the frontend's status->idle write at :562 failed) leaves stoppingByUser[id] set; startProcess never clears it, so the NEXT natural exit takes the `!stoppingByUser` branch as false and raises no 'finished' notification.
- `src/app/notifications/peek-overlay.component.ts:180-186` **Peek overlay handles Ctrl+Enter before the typing guard, so the chord approves a permission instead of sending the typed reply** — The window keydown listener is registered in capture phase (:169) and the Ctrl/Cmd+Enter branch runs before the `if (typing) return;` line, contradicting its own doc comment ('Queue keys - inert while the user is typing (the reply input included)', :175).
- `src/app/notifications/peek-overlay.component.ts:203` **Peek overlay Ctrl+Enter answers an AskUserQuestion with option 1 - the intended question exclusion never matches** — allow() excludes questions by testing `cur.kind === "permission"`, but AskUserQuestion prompts ARE raised as kind 'permission' (agent-runtime.service.ts:588-617 always passes kind: "permission", carrying the structured `questions` from the hook payload) - the question-ness lives in n.questions, which is what the card layout switches on (notification-card.component.ts:145-147) and why the card suppresses its own Accept row for question-bearing notifications (:104). _(added by verifier)_
- `src/app/notifications/question-stepper.component.ts:311-321` **Submitting a question with no option selected sends nothing but still marks the notification answered** — In the single-select branch pick is undefined when the user never clicked an option (the stepper has no required-selection gate and Submit is enabled whenever sending() is false, :165).
- `src/app/terminal.service.ts:424-426` **An agent that was never in the interest set gets no snapshot replay when its handle already exists, losing all scrollback** — attach() replays the backend snapshot only when the handle is brand new or explicitly stale.
- `src/app/commands/command-registry.service.ts:257-268` **Double-Shift detector has no e.repeat guard — merely holding Shift opens Search Everywhere** — Chromium (and therefore WebView2) dispatches auto-repeating keydown events for held modifier keys.
- `src/app/commands/find-in-files.component.ts:323-355` **Replace applies the CURRENT query/options to hit lines produced by the previous query** — `apply()` sends `query`, `caseSensitive`, `wholeWord`, `regex` read live from the signals, but the `files[].lines` come from `this.flat()`, which still holds the hits of the last completed search.
- `src/app/commands/find-in-files.component.ts:399-435` **Editing the replacement text clears the user's unticked-hit set (run() resets `excluded`)** — The debounced re-run effect tracks `replacement()` and `mode()`, and `run()` unconditionally does `this.excluded.set(new Set())`.
- `src/app/commands/find-in-files.component.ts:428-477` **run() has no generation guard: overlapping searches are not cancelled and an older search can win the searchId** — `cancelCurrent()` can only cancel a search whose id has already come back from `search_start`.
- `src/app/commands/find-in-files.component.ts:519-531` **Matches found in the project checkout are opened in a different worktree's copy of the file** — For a hit with no `agentId` (the project checkout root, bridge.ts:344-347) the click handler substitutes the scoping AGENT's id and opens the same relative path inside that agent's worktree.
- `src/app/commands/overlays.component.ts:214-222` **Recent Files never lists files opened from a project workspace tab** — A project tab's pane leaf carries the PROJECT id as its agentId (the v2 pseudo-agent, pseudo-agent.ts:14), so RecentFilesService records entries with `agentId === projectId`.
- `src/app/commands/search-everywhere.component.ts:322` **Search Everywhere keyboard selection jumps back to the top whenever any agent changes working/needsInput state** — `sel` is a linkedSignal whose source is `items()`, resetting to 0 on every new array.
- `src/app/tool-window/local-history-panel.component.ts:225-237` **Snapshot↔current diff is not refreshed after a restore, so the panel keeps showing the pre-restore differences** — `doRestoreAll()` / `restoreFile()` delegate to `store.restore()`, which reloads the snapshot LIST in its finally block (local-history.store.ts:62-63) but touches nothing else.
- `src/app/tool-window/tool-window.component.ts:143-147` **Dock height computed reads window.innerHeight but never re-runs on window resize** — `h()` is a computed whose only reactive dependency is `tw.height()`; `window.innerHeight` is read untracked.
- `src/app/loading/loading.component.ts:186-194` **12s splash backstop routes into the shell mid auto-update download; the later install/relaunch skips the updateResume capture the manual path performs** — `boot()` races `runWithFloor()` against a 12s timer that always resolves 'no-update', so at t=12s it navigates to /app while `UpdaterService.run()` is still awaiting `update.downloadAndInstall` (updater.service.ts:76-86).
- `src/app/projects/project-actions.service.ts:72` **Removing a project cascade-deletes its agents in the backend, but none of the frontend per-agent cleanup runs — orphan tabs, stale notification cards, leaked terminals and edit buffers** — project_remove cascades: the backend deletes the project's agents and emits agent://deleted for each (src-tauri/src/projects/commands.rs:76-86), which the facade turns into store.remove (entity-facade.ts:31). _(added by verifier)_
- `src/app/stores/tickets.store.ts:28-35` **A failed entity list is swallowed with no error surface and no retry; `loading` is exported but no component reads it, so the UI shows a confident empty state** — All three root stores run `listen(); load();` once at injection inside a bare `catch {}` (tickets.store.ts:28-35, projects.store.ts:27-34, agents.store.ts:73-80).
- `src/app/modals/add-project-modal.component.ts:361-372` **Add-project fires `detectGit` on every keystroke with no debounce, cancellation or catch — a stale response can flip the git state for the wrong path** — The effect re-runs on every `dir()` change, so typing `C:/code/orrery` issues ~14 backend `project_detect_git` invokes, each on the blocking pool.
- `src/app/modals/settings-modal.component.ts:575` **An auto-approve bypass set for a tool that is no longer detected becomes invisible and un-resettable while still applying at launch** — The Permissions section iterates `detectedTools()` only, so if a tool the user set to "Everything" is later uninstalled (or its detection fails — e.g. _(added by verifier)_
- `src/app/modals/settings-modal.component.ts:1174-1180` **Arming "Everything" for a second tool overwrites the single `confirm` slot, leaving the first tool on an unconfirmed, persisted bypass** — `confirm` is one slot (settings-modal.component.ts:1043).
- `src/app/projects/project-actions.service.ts:72-79` **`removeProject` swallows every error, so a failed "Remove project" gives no feedback at all** — Every other action in this service surfaces the backend message (`relocateProject` line 88, `initGitForProject` line 96, `addProject` line 64-68).
- `src/app/settings/settings.store.ts:121` **`anyDirty` is always true against a real backend, so the settings footer permanently offers "Reset all to defaults"** — `merge()` spreads the raw backend document (`...p`) over the defaults.
- `src/app/backlog/ticket-page.component.ts:557` **A failed ticket save silently discards the user's edits and exits edit mode** — In edit mode Save awaits ticketsStore.update; on any rejection (backend down, ticket removed in another tab, DB error) the catch body is empty and control falls through to `this.editing.set(false)`. _(added by verifier)_
- `src/app/overview/agent-card.component.ts:184-186` **Overview cards report "0 files +0 −0" for any agent whose worktree was never scanned, while the sidebar shows the real totals** — `ch()` reads `AgentWorkStore.changesFor(id)`, which returns the IDLE sentinel `{status:'idle', data:[]}` when no full detail scan has run (agent-work.store.ts:109-111, with the models.ts:101 note "`idle` = never requested (unknown, NOT empty)").
- `src/app/overview/timeline-view.component.ts:80-81` **Timeline/strip/dev-panel render `Agent.progress` as a percentage, but the field is hard-wired to 0 everywhere** — `progress` is written exactly once in the backend — `progress: 0.0` at spawn (src-tauri/src/agents/service.rs:162) — and nothing ever updates it (a repo-wide grep over src-tauri finds no other assignment); the frontend's optimistic row also uses 0 (agent-actions.service.ts:201, projects/pseudo-agent.ts:26).
- `scripts/release/bump.mjs:46-56` **bump.mjs has no branch guard — a release run from a feature branch tags and ships that branch to every user** — `pnpm release` (and release.yml's workflow_dispatch, which runs the same script at .github/workflows/release.yml:53-58) only checks that the tree is clean; it never checks that HEAD is on main.
- `scripts/release/changelog-json.mjs:97-104` **A malformed changelog.json is silently treated as empty, so the append rewrites the file with a single release** — `readJson` swallows every parse failure and returns `[]`.
- `src/app/shared/file-drop.service.ts:112-125` **File drops on a plain <input> miss the text-field branch and are pasted into an unrelated agent's PTY** — The routing selector is `input[type=text]`, a CSS *attribute* selector, so it only matches inputs that literally carry `type="text"`.
- `src/app/shared/rich-editor/rich-editor.component.ts:476` **Link popover has no dismiss path — the toolbar button does not toggle and there is no outside-click close** — Select text, press the link button (or ⌘K), then change your mind and click back into the document. _(added by verifier)_
- `src/app/shared/rich-editor/rich-editor.component.ts:482` **Link popover's URL input never receives focus (microtask runs before zoneless change detection renders it)** — `openLink()` sets `linkOpen` to true and then queues the focus call as a microtask.
- `tools/perf-smoke/assert.mjs:60-89` **perf-smoke's stale-row filtering is inert: the dev panel's capture JSON never emits a `stale` field** — Both stale guards read `row.stale`, but the only producer of these capture files — the Dev panel's Copy button — builds each row from an explicit key list that omits `stale` (src/app/dev-tools/dev-panel.component.ts:685-694: cmd, calls10s, avgRt, avgExec, overhead, p95Rt, maxRt, errPct), even though PerfRow carries `stale: boolean` (src/app/perf/perf.store.ts:46).
- `src/app/sidebar/files/file-tree.component.ts:52` **File-tree virtual scroll hard-codes itemSize 24 while the row height is density-scaled** — The viewport is `<cdk-virtual-scroll-viewport itemSize="24" ...>` but each row is `height:var(--sp-9)` (:63), and `--sp-9` is `round(calc(24px * var(--density)), 1px)` (styles.css:165) with `--density` 0.85 for compact and 1.18 for comfy (styles.css:266-274) — i.e. _(added by verifier)_
- `src/app/sidebar/files/file-tree.component.ts:273` **Deleting/renaming a DIRECTORY leaves buffers under it open and dirty, so the next Save All re-creates the deleted folder on disk** — Both handlers close only the buffer keyed at the node's own path; buffers for files INSIDE a deleted/renamed directory keep their old `agentId:dir/file` keys and stay dirty.
- `src/app/sidebar/files/file-tree.component.ts:364` **Dragging a file out of a `proj:` (main) root silently does nothing** — `onDragStart` publishes `agentId: this.workId()` (file-tree.component.ts:362-366), which for a `proj:` root is the PROJECT id (:151-153). _(added by verifier)_
- `src/app/top-bar/top-bar.component.ts:416` **zoneAt targets tabs hidden underneath the sticky Orchestrator tab, so a drop there silently tiles two tabs** — The Orchestrator tab is `position:sticky; left:0; z-index:2` with an opaque `var(--panel)` ground (template :90-94), so once the strip is scrolled, agent tabs slide UNDER it. _(added by verifier)_
- `src/app/top-bar/top-bar.component.ts:432` **Dropping a sidebar agent onto a ticket tab highlights a merge target and then does nothing but hijack the view; onto a project tab it tiles an agent the tab-drag path deliberately forbids** — `onDragOver` accepts any tab that is not orchestrator/backlog and paints the full `merge` highlight (:435 + template :107-109).
- `src/app/ui/ui.store.ts:306` **Deleting a worktree never drops its edit buffers, so every later Save All reports failure** — `closeTabsForAgent` removes the tabs/panes only, and the delete flow around it (agent-actions.service.ts:295-321: closeTabsForAgent → agentsStore.remove → runtime.dispose/conflicts.dispose/notifications.clearAgent) never touches EditsStore — `EditsStore.closeAgent` (stores/edits.store.ts:115) has no production caller at all (grep: only the store's own definition). _(added by verifier)_
- `src/app/ui/ui.store.ts:311` **Deleting an agent that was Alt+click-split into two panes of the same tab leaves a pane bound to the removed agent** — `openFileInSplitWorkspace` splits an agent's leaf and inserts ANOTHER leaf for the same agent (:512-513), so the tab's tree is `split(leaf(a1), leaf(a1))` and `treeAgentIds` returns `['a1','a1']`.
- `src/app/ui/ui.store.ts:421` **Ungroup / Detach rebuild panes as bare terminal leaves, discarding each agent's open file tabs, preview/pin state and diff view** — `leaf()` (pane-model.ts:36-38) returns `{type,id,agentId,view}` with no `files` / `activeFile` / `previewFile`.
- `src/app/workspace/diff-view.component.ts:507-509` **Rename/delete from the diff list silently drops unsaved edits and leaves a stale file tab that renders as an empty, savable file** — commit()/confirmDelete() call edits.close(id, from|path) directly — EditsStore.close removes the buffer whether or not it is dirty (edits.store.ts:105-112), so renaming or deleting a file that has unsaved edits open discards them with no prompt, unlike every other close path.
- `src/app/workspace/file-save.service.ts:51` **save()/saveAll() overwrite the file on disk with no disk-vs-baseText check — autosave silently clobbers an agent's concurrent write** — save() writes buf.text through file_write and then marks the buffer saved; it never compares what is on disk now with buf.baseText. _(added by verifier)_
- `src/app/workspace/file-view.component.ts:196` **Markdown preview renders the DISK text, so unsaved edits vanish when you switch a dirty .md file to Preview** — mdHtml is computed from content(), which is only ever set from agent_diff's `.new` (file-view.component.ts:359) — the working-tree text — never from the EditsStore buffer. _(added by verifier)_
- `src/app/workspace/file-view.component.ts:328-330` **"Keep mine" on the external-change banner does not stick — the banner returns on the next watcher scan** — keepMine() only clears the `conflict` signal; it records nothing.
- `src/app/workspace/git-action-bar.component.ts:103-105` **Discard in the git action bar wipes the whole worktree with no confirmation** — The Discard button under every diff calls discardAgent(id, []) straight from the click handler; [] means "all changes".
- `src/app/workspace/monaco-file-editor.component.ts:355-368` **The editor cap can turn a visible, editable file into a dead plain-text node that never recovers** — registerEditor demotes the OLDEST registration as soon as the 7th editor mounts (editor-cap.ts:13,30-33), and the cap is shared by file editors, unified diffs and side-by-side diffs.
- `src/app/workspace/pane-model.ts:233` **dropAgent cannot remove an agent that tiles twice in the same tab — a stale pane survives agent deletion** — treeAgentIds returns DUPLICATES (pane-model.ts:242-254 pushes one entry per leaf), so UiStore.closeTabsForAgent's `ids.length === 1` test (ui.store.ts:309-312) is false for a tab whose two panes both show agent A, and it calls dropAgent instead of closing the tab. _(added by verifier)_
- `src/app/workspace/pane-node.component.ts:138` **Closing a pane (or a center-drop replace) drops its file tabs with no dirty guard, orphaning buffers and scroll state** — Every other close path is guarded: closing a file tab goes through requestFilesClose (pane-node.component.ts:532-546) and closing a workspace tab through TabCloseGuardService.
- `src/app/workspace/tab-close-guard.service.ts:33-37` **Tab close guard claims every dirty buffer of every agent in the tree, so Discard destroys edits open in a different tab** — requestClose builds its "would be lost" set from treeAgentIds(root) x EditsStore.dirtyPaths(agentId) — i.e.
- `src/app/workspace/git/commit-diff-view.component.ts:215-221` **Commit/range/file-history file lists never reload after LRU eviction, leaving the view permanently empty** — The list-loading effects track only agent().id / sha() / shas() / path(); the store read is inside untracked().
- `src/app/workspace/git/conflict-view.component.ts:54-70` **Conflict parser mis-splits blocks whose content contains a line starting with ======= or >>>>>>>, so resolution stages corrupted text** — Inside a conflict block the parser switches to the theirs side on the first line that merely starts with "=======", and ends the block on any line starting with ">>>>>>>".
- `src/app/workspace/git/conflict-view.component.ts:124` **Session progress meter reaches 100% / "0 remaining" while Commit merge is still disabled, because the meter counts segment picks and the button counts staged files** — The meter's numerator/denominator are per-SEGMENT (totalResolved sums resolvedCount, which counts resMap picks; totalConflicts sums conflictCount, which counts parsed conflict blocks), but Commit merge is gated on allDone, which is per-FILE staging (`f.resolved`, set only by ConflictStore.resolve after conflict_resolve). _(added by verifier)_
- `src/app/workspace/git/conflict-view.component.ts:131-142` **Conflict progress re-parses every conflicted file's full text on every change detection pass** — conflictCount()/resolvedCount() are called from the template once per file row (and again inside the totalConflicts/totalResolved computeds), and conflictCount does a full merged.split("\n") + segment parse each call — nothing is memoised.
- `src/app/workspace/git/conflict-view.component.ts:539-549` **Conflict view accepts rebase/cherry-pick/revert sessions; its Abort resets to HEAD and leaves .git/rebase-merge behind** — The recovery effect accepts every session state except "none" — including "rebase", "cherrypick" and "revert" (SessionState.state, src-tauri/src/git/types.rs:106-113) — and populates the merge UI from the index's conflicted paths.
- `src/app/workspace/git/file-history-view.component.ts:370-388` **setTimeout(50) store polling never terminates when the store entry is evicted, and is not cancelled on destroy** — pollUntilReady re-arms a 50 ms timer until the keyed entry reports ready/error, and its only escape is a diffGen bump — there is no DestroyRef teardown.
- `src/app/workspace/review/review-comments.monaco.ts:316-347` **An open review composer is rebuilt (losing the typed note) on every in-place content refresh of the diff** — renderZones() unconditionally removes every zone and re-creates the composer with a fresh buildComposer(), and it is reached from `editor.onDidChangeModelContent(() => renderAll())`.
- `src/app/workspace/review/review-comments.monaco.ts:425` **Review comments never re-anchor after a diff refresh: out-of-range ones vanish from the editor but are still sent to the agent with stale line numbers** — A review comment is stored with absolute 1-based line numbers and nothing in the app ever mutates them (grep for fromLine: only reads outside saveComment). _(added by verifier)_
- `src/app/workspace/review/send-review.component.ts:197-202` **Review comments are cleared before delivery is known to have succeeded, and the store is memory-only** — send() clears the ReviewStore synchronously, but AgentReviewService.sendReview returns void immediately: if the agent process is not running it starts it and defers the PTY write behind a fixed setTimeout(1800), and the write itself only reports failures through `ui.flash`.
- `src-tauri/src/agents/adapters/mod.rs:403-408` **run_probe abandons its worker thread and never kills the child on timeout** — `run_probe` moves the `Command` into a detached thread that calls `cmd.output()`, then gives up after `recv_timeout(6s)` and returns `Err("timed out…")`.
- `src-tauri/src/agents/adapters/mod.rs:527` **Hook-config writes are non-atomic truncate-in-place on four user config files, every startup** — Both merge paths finish with `std::fs::write(path, …)` (mod.rs:527 for the three JSON files, codex.rs:125 for config.toml).
- `src-tauri/src/agents/adapters/mod.rs:536` **Ownership marker match is case-sensitive, so the dev binary's groups are never replaced by the installed build's (duplicate hook groups)** — The marker is the hook binary's file stem and the match is `s.contains(marker)`, which is byte-exact. _(added by verifier)_
- `src-tauri/src/agents/adapters/mod.rs:829-832` **A manual tool-path override that lands on the cmd.exe wrapper truncates a multi-line prompt at its first newline** — `windows_launch_prefix` returns `("cmd.exe", ["/c","call",<shim>])` for an unparseable `.cmd`/`.bat`.
- `src-tauri/src/agents/commands.rs:96-121` **agent_remove destroys local history, scrollback and the watch registration before the step that can abort, so a failed delete loses them anyway** — agent_remove kills the PTY, drops the scrollback ring, unwatches the worktree and calls HistoryService::purge(id) (which is `std::fs::remove_dir_all(agent_dir)`, src-tauri/src/history/mod.rs:257-259) BEFORE spawning the blocking svc.remove.
- `src-tauri/src/agents/fs_commands.rs:75` **rename_path's collision guard compares paths case-insensitively on every platform, so on Linux/case-sensitive volumes a rename silently overwrites a different existing file** — The `dst.exists()` guard is skipped whenever paths_equal_ci(src, dst) is true, and paths_equal_ci is an unconditional eq_ignore_ascii_case on the whole path string (96-98) — the exemption is only meant to allow a case-only rename on Windows' case-insensitive FS.
- `src-tauri/src/agents/service.rs:204-217` **On a directory clash spawn suffixes the folder but not the git worktree name, leaving a permanently stale registration that makes later spawns of the same slug fail silently** — When root/<slug> already exists (a folder left behind by a soft-removed agent, or an agent of another project with the same name), wt_path becomes `<slug>-<6hex>` but wt_name stays `<slug>`, and that unsuffixed name is what create_worktree registers under (.git/worktrees/<slug>, gix_backend.rs:2463-2468).
- `src-tauri/src/agents/service.rs:213` **Respawning an agent with a previously-used name silently reuses the old branch and ignores the requested base** — remove() never deletes refs/heads/<branch> — service.rs:400-448 only renames/deletes the folder and drops the .git/worktrees registration. _(added by verifier)_
- `src-tauri/src/agents/service.rs:476-495` **Startup trash sweep recursively deletes ANY directory under the worktree root whose name merely contains '.trash-'** — sweep_trash (called unconditionally at startup, src-tauri/src/lib.rs:94) read_dir's both worktree roots and hands every entry matching is_trash_dir to a background remove_dir_all_retry.
- `src-tauri/src/core/database.rs:16-18` **DB open and every CREATE TABLE unwrap inside setup(); no busy_timeout on a database the app expects a second instance to share** — `Database::get` unwraps `create_dir_all` and `Connection::open`, and each service's `init_schema` unwraps its `CREATE TABLE` while holding the mutex (projects/service.rs:65, tickets/service.rs:102 and :124, settings/service.rs:34, workspace/service.rs:31).
- `src-tauri/src/projects/commands.rs:75-86` **project_remove's cascade is partial and its failure is silent: tickets are never cascaded, agent-cascade errors are swallowed after the project row is already gone** — Two problems at the same call site.
- `src-tauri/src/projects/service.rs:236-259` **A failed post-clone step leaves the cloned folder on disk and permanently wedges the retry** — `create_from_git` guards the destination up front (:219-234: refuse if already registered, or if it exists and is not an empty directory), clones, then hands off to `create_local`.
- `src-tauri/src/tickets/service.rs:247` **tickets.agent_id is never cleared, so removing an agent strands its ticket in In Progress with no Dispatch action** — attach_agent is the only writer of tickets.agent_id and it also forces status='inprogress'. _(added by verifier)_
- `src-tauri/src/git/backend.rs:197` **clone_repo passes the user-supplied URL as a bare argument — no `--` end-of-options and no scheme check** — The URL reaches git as a positional argument with nothing separating it from the option list, and the caller only trims it and (in Root mode) derives a folder name: projects/service.rs:199-236 `let url = req.source_url.as_deref().unwrap_or("").trim().to_string(); ... _(added by verifier)_
- `src-tauri/src/git/gix_backend.rs:240-255` **EOL normalisation ignores .gitattributes, so counts/hunks/blame for a modified file in an eol=crlf repo cover the whole file** — In a repo with `*.x text eol=crlf` (or core.eol=crlf) and core.autocrlf unset, a file the agent edits is correctly detected as modified by gix status, but line_stats (:258-268), hunks_of (:272-290) and working_blame (:2252) compare the raw CRLF working bytes against the LF blob: the +N/-N badge counts every line, the editor gutter marks the whole file changed, and the annotate gutter attributes every line to 'Uncommitted'.
- `src-tauri/src/git/gix_backend.rs:441-460` **occupancy() misses the main checkout when called from a linked worktree, so agent_checkout can take a branch the project holds** — `occupancy` seeds the map with the CURRENT repo's HEAD labelled `MAIN_CHECKOUT` and then iterates `repo.worktrees()`, which (gix `Repository::worktrees`) lists only the LINKED worktrees under `<common>/worktrees` — the main checkout is never among them.
- `src-tauri/src/git/gix_backend.rs:1233-1244` **edit_config truncates .git/config before writing and takes no lock** — `edit_config` reads `<common>/config`, mutates it in memory, then `std::fs::File::create(&path)` — which TRUNCATES the real config — and streams the new content into it.
- `src-tauri/src/git/gix_backend.rs:1490-1563` **commit() ignores an in-progress merge: single-parent commit, MERGE_HEAD left behind** — `commit` never looks at MERGE_HEAD or at conflicted index stages (git refuses to commit with unmerged paths).
- `src-tauri/src/git/gix_backend.rs:1706` **Non-UTF-8 files read as empty on the 'new' side: the diff pane shows a full deletion and a conflicted binary file can never be resolved** — read_to_string().unwrap_or_default() at :1706 and :2315 yields "" for any non-UTF-8 file.
- `src-tauri/src/git/gix_backend.rs:1750-1761` **merge_abort hard-resets even when no merge is in progress** — `merge_abort` never checks for MERGE_HEAD: it unconditionally calls `reset_to_tree(head_tree)`, which (see rust-git-01) restores every dirty tracked path from HEAD.
- `src-tauri/src/git/gix_backend.rs:2245-2248` **working_blame swallows every blame failure and reports the whole file as uncommitted** — Any error from `blame` — a path that gix's blame cannot handle, a corrupt/missing object, a rename in history — is discarded and turned into a plain `Ok` whose 'old' side is an EMPTY blame and whose 'new' side marks every line 'Uncommitted' (:621-628).
- `src-tauri/src/git/gix_backend.rs:2432` **create_worktree never consults occupancy(), so two working trees can be registered on one branch** — create_worktree checks only that the target dir is empty (:2452-2462) and that the registration name is free (:2463-2468); it never asks occupancy() (:441-460) whether some checkout already holds `branch`, which is the check branch_rename (:1296-1298), branch_delete (:1330-1332) and checkout_branch (:1438-1446) all perform and the one `git worktree add` enforces itself. _(added by verifier)_
- `src-tauri/src/hooks/mod.rs:285` **A Notification's message never reaches the activity feed for Claude — transcript content wins and is then deduped away** — Notification shares the arm that prefers transcript content (`content.clone().or_else(|| event.activity_detail())`, mod.rs:287-290), and the notification's own text is only in activity_detail's `non_empty(message)` (protocol.rs:369). _(added by verifier)_
- `src-tauri/src/hooks/mod.rs:287-345` **UserPromptSubmit publishes the previous assistant message (or nothing) instead of the user's prompt** — For every non-permission event the activity line is `content.clone().or_else(|| event.activity_detail())` — transcript content always wins.
- `src-tauri/src/hooks/mod.rs:287` **A resumed agent is marked "working" at SessionStart and stays there until a turn ends** — SessionStart maps to emit_status("working").
- `src-tauri/src/hooks/mod.rs:462-469` **Dedup maps are never reset per agent run, so the first "working" after restarting a killed agent is swallowed** — `last_status` / `last_activity` are keyed by agentId and live for the whole app process; nothing clears them when an agent's process stops or restarts.
- `src-tauri/src/lib.rs:159-164` **If the loopback bind fails the bridge is left unmanaged, but agent_start still demands it as Tauri state — no agent can be launched** — `HookBridge::start` failure is logged and swallowed, so `app.manage(bridge)` never runs and startup continues as if hooks were merely unavailable.
- `src-tauri/src/core/emit.rs:399-403` **prune() deletes the raw-trace file that is still open and being written** — prune() enumerates every regular file in the telemetry dir and deletes oldest-first while the running total exceeds DISK_CAP_BYTES (50 MB).
- `src-tauri/src/runtime/digest.rs:27` **fold_lines seeds the cursor column from String::len (bytes) while everything else treats it as chars** — `fold_lines` is called once per batcher chunk with a persisted `lines` buffer (output_mux.rs:157, heuristics.rs:211), and it resumes the cursor with `lines.last().map(String::len)` — the BYTE length.
- `src-tauri/src/runtime/digest.rs:34-80` **Escape-sequence parsing in fold_lines carries no state across chunks, so a split CSI/OSC leaks as literal text** — `fold_lines` parses ESC sequences purely inside one chunk.
- `src-tauri/src/runtime/mod.rs:416-458` **Stop-then-restart: the OLD wait thread removes the NEW run's proc entry, orphaning it and completing its ticket** — `stop()` removes the map entry immediately (mod.rs:468) and only then signals the child, so `is_running(id)` is false while the old process is still dying.
- `src-tauri/src/runtime/mod.rs:425-428` **agent://exit is gated on PTY EOF; a surviving grandchild blocks the wait thread forever (the ordinary Windows stop path)** — The wait thread emits the tail frame and `agent://exit` only after `batcher.join()` returns (mod.rs:425).
- `src-tauri/src/runtime/output_mux.rs:350` **agent://digest re-ships the whole uncapped fold line every second, breaking the module's own ~300B/s bound** — digest_loop ships `tail_lines(&d.lines, DIGEST_LINES)` for every dirty digest agent once per second. _(added by verifier)_
- `src-tauri/src/runtime/scrollback.rs:87-112` **Ring lifecycle races the dying run's batcher: reset() can be undone and remove() resurrects the ring** — Ring writes come from the batcher thread, which outlives `stop()` by however long the reader takes to hit EOF and final-flush (mod.rs:119 in output_batcher, mod.rs:361).
- `src-tauri/src/fs/mod.rs:56-80` **NODE_LIMIT truncation is silent and yields children: Some([]), so a cut-off folder renders as loaded-and-empty and can never be lazily expanded** — scan shares one global counter and only breaks the CURRENT loop when it hits 10_000 (:58-60).
- `src-tauri/src/history/mod.rs:135-142` **History blobs are written non-atomically and never verified on read, so a crash mid-write poisons that content hash permanently and restore writes the truncated bytes into the worktree** — snapshot writes a blob with a plain std::fs::write to its final content-addressed path and gates future writes purely on blob.exists().
- `src-tauri/src/history/mod.rs:241-252` **restore is not all-or-nothing: one unreadable blob aborts it mid-way, and its own guard snapshot can prune away the snapshot being restored** — restore writes files one at a time and propagates the first error with `?` (:244, :250), so a failure part-way leaves the worktree half old / half new with no rollback and no partial-result report — the command just returns Err.
- `src-tauri/src/search/mod.rs:188-190` **Whole-word search and whole-word replace use different word-boundary semantics, so Replace All silently replaces nothing for patterns that start or end with a non-word character** — Search uses grep-regex's RegexMatcherBuilder::word(true) (:168), whose WordMatcher wraps the pattern as `(?:^|\W)(pat)(?:$|\W)` precisely so patterns beginning/ending with punctuation still match.
- `src-tauri/src/search/mod.rs:276-285` **apply_replace's temp-file+rename drops the target's permission bits and turns a symlinked file into a regular file** — apply_replace never opens the original for writing; it writes a brand-new sibling file and renames it over the target.
- `src-tauri/src/watch/mod.rs:336` **ProjectWatcher.roots is never pruned on unwatch, so a re-created worktree at a reused path is never re-registered** — watch_with_emit appends every successfully registered root to pw.roots and skips any later root already covered by that list (:336-343). _(added by verifier)_
- `src-tauri/src/watch/mod.rs:412-416` **rescan blocks on the AgentRun mutex while holding the projects and project-state locks, stalling the notify OS callback thread for a whole git-status scan** — rescan takes self.projects (:408) and pw.shared.state (:412) and then does reg.run.lock() (:414).
- `src-tauri/src/watch/mod.rs:512-526` **unwatch does not cancel an already-dequeued scan, so a post-purge emit recreates the agent's deleted local-history directory** — project_loop takes the due agents' Arc<Mutex<AgentRun>> under the state lock and then scans them one at a time OUTSIDE that lock (watch/mod.rs:478-526).
- `src-tauri/src/watch/mod.rs:521` **Local-history snapshots are gated on the scan fingerprint, so content-only edits are never captured** — history.snapshot is called ONLY from the watch emit closure (the only production caller — `grep -rn 'history.snapshot' src-tauri/src` gives watch/mod.rs:224 and the internal guard at history/mod.rs:241), and that closure runs only when the scan fingerprint changed. _(added by verifier)_

## Low (137)

One line each; full detail is in `.audit-work/pass1-merged.json`.

- `src/app/agents/agent-work.store.ts:242-252` **expandDir has no rejection handler, unlike every other load in the store** — Every other invoke in AgentWorkStore/GitInspectStore ends in .catch(); expandDir chains only .then().
- `src/app/agents/agent-work.store.ts:323-325` **dispose() deletes the generation counters, so a stale in-flight response can pass the guard of a post-eviction reload** — The generation guards are only monotonic while the key lives.
- `src/app/terminal-output-scheduler.ts:178-183` **Stale comments: backlog drop does not keep the newest tail, mux fast-path comments describe removed behaviour, LRU comment says 5 vs MAX_AGENTS 6** — Three comments actively mislead a maintainer.
- `src/app/terminal.service.ts:133-135` **Dead code: TerminalService.onTitle/titleCb, tail(), and the scheduler's onBacklogDropped hook have no callers** — onTitle() is never called outside specs, so titleCb is always undefined and the onTitleChange subscription at :230 (still commented '// live agent state') is a no-op - title parsing moved to Rust.
- `src/app/commands/command-registry.service.ts:197` **'Delete Worktree' is enabled for a project pseudo-agent and opens a dialog that instantly self-closes** — activeAgent() returns the synthesized project pseudo-agent whose id IS the project id when a project workspace tab is focused (agent-runtime.service.ts:94-96). _(added by verifier)_
- `src/app/commands/command-registry.service.ts:262` **Double-Shift ignores the keymap: rebinding Search Everywhere leaves the ⇧⇧ chord live and unlisted** — The dispatcher hard-codes `this.open("search")` for the double-Shift gesture before the command loop runs, and the loop explicitly filters the "Shift Shift" sentinel out of the matched bindings (:279). _(added by verifier)_
- `src/app/commands/find-in-files.component.ts:376-386` **Event unsubscribers are pushed after onDestroy may have run — listeners leak if the overlay/panel closes before listen() resolves** — `bridge.on()` returns a Promise (Tauri `listen`); the unsubscriber is only appended to `unsubs` when it resolves, while `destroy.onDestroy` iterates `unsubs` at destroy time.
- `src/app/commands/fuzzy.ts:61` **A recorded chord whose key is '+' produces a binding string that can never match again** — bindingFromEvent joins parts with '+' (fuzzy.ts:75-80), so recording Ctrl+NumpadAdd (or Ctrl+Shift+= on a US layout, where e.key is '+') stores the string "Ctrl++". _(added by verifier)_
- `src/app/commands/overlays.component.ts:303-319` **Go to Line input strips non-digits in the model but the field keeps showing them** — `onInput` writes the sanitised value back into `v`, but the template binds `[value]="v()"`.
- `src/app/commands/overlays.component.ts:352` **Dead 'all' Search-Everywhere tab sentinel and its leftover Exclude<TabKey,'all'> type** — The overlay host passes `'all'` as the default initial tab, but TABS (search-everywhere.component.ts:52-58) has no 'all' entry, so the linkedSignal computation always falls through to 'commands' (:167-170).
- `src/app/commands/recent-files.service.ts:79-81` **RecentFilesService.clearAgent is dead code and would not persist anyway** — Nothing in src/app calls `RecentFilesService.clearAgent` (grep for `clearAgent` finds only the unrelated NotificationStore/ScrollStateService methods and this definition), so entries for a deleted worktree are never purged.
- `src/app/commands/search-everywhere.component.ts:217-232` **Search Everywhere file corpus is re-allocated on every agent/ticket/command edge while the overlay is open** — `corpus()` builds `files` — one object plus a closure per path across up to 8 worktrees, each capped at 20 000 entries by the Rust `search_files` (src-tauri/src/search/commands.rs:306-309) — inside the same computed as agents/tickets/commands/refs.
- `src/app/tool-window/graph-strip.component.ts:84` **Graph strip hard-codes the Ctrl+Shift+G hint, ignoring keymap overrides and macOS glyphs** — Both the tooltip (:23) and the kbd chips (:84) are string literals.
- `src/app/tool-window/tool-window.component.ts:169-173` **Tool-window project/worktree scope is component state and is lost every time the dock is closed** — `explicit` lives in the component, and the shell renders `<app-tool-window>` only while `toolWindow.panel()` is non-null (shell.component.ts:78-83), destroying it on close.
- `src/app/app.component.ts:21-24` **The window-icon effect tracks the whole tweaks object, so density/motion/viz changes also re-invoke set_window_icon** — The effect reads `this.ui.tweaks()` — one signal holding `{theme, density, defaultViz, motion}` (models.ts:280-285) — and derives `dark` from it, so it re-runs on ANY tweak change, not only a theme toggle.
- `src/app/data-source/bridge.ts:306` **Events.AgentAsk is a dead constant: neither emitted by the backend nor subscribed by the frontend** — Grep for `AgentAsk` and for the literal `agent://ask` across src/ and src-tauri/ hits only this one line.
- `src/app/data-source/bridge.ts:325` **comment://created is emitted by the backend but has no frontend subscriber (dead event constant; comments are pull-only)** — The stated scenario is not reachable: comments can only be created through the `comment_add` Tauri command (src-tauri/src/tickets/commands.rs:82-93), whose sole caller is ticket-page.component.ts:627, which awaits loadComments(id) right after.
- `src/app/data.ts:4` **Hardcoded fake org name "northwind" from the mock-data file is rendered in the shipping UI** — `ORG` lives in data.ts, whose header comment calls the file "Orrery mock data", and is re-exported live as `UiStore.org` (ui.store.ts:160).
- `src/app/data.ts:133-241` **Dead mock exports PROJECTS and STREAM (~110 lines) plus the types that exist only to serve them** — `PROJECTS` (three fabricated northwind repos with fake paths, shas and file lists) and `STREAM` (fake agent log lines) are exported but imported nowhere — grep for both names over src/ hits only their declarations in data.ts.
- `src/app/loading/loading.component.ts:183-194` **The splash has no rejection path: Promise.race adopts a rejection, so the documented "safety timeout" does not guarantee moving on** — `Promise.race` settles on the FIRST settlement, including a rejection, so a rejection from `runWithFloor()` (i.e.
- `src/app/models.ts:94-98` **Stale comment: Project.branches is labelled mock/demo data but is backend-supplied and depended on by five call sites** — The `// --- ui-only extras (mock/demo data) ---` banner covers `org`, `repo`, `branches` and `files`.
- `src/app/state/entity-facade.ts:20-37` **listen() is registered before load(), and load()'s setAll wholesale-replaces the map — an event arriving mid-list is discarded** — `load()` calls `store.setAll(await bridge.invoke(listCommand))`, and `setAll` rebuilds both `entities` and `ids` from scratch (entity-store.ts:35-40), so any `<entity>://created|updated` upsert applied while the list command was in flight is thrown away.
- `src/app/state/entity-store.ts:34-47` **Half of EntityStore's public API is unused by every root store, and byId allocates a fresh computed per call** — `entities`, `ids`, `active`, `byId`, `update`, `upsertMany`, `setActive` and `reset` are consumed only by entity-store.spec.ts / tickets.store.spec.ts — the three root stores surface just `all` and `loading` (projects.store.ts:12-13, tickets.store.ts:12-13, agents.store.ts:40-41).
- `src/app/stores/notifications.store.ts:11-17` **The notification feed grows without bound over a session and re-sorts the whole list on every read** — `list` only ever shrinks through `clearResolved()` (a manual "Clear read" button, top-bar/notification-center.component.ts:42) or `clearAgent()` when an agent is removed (agent-actions.service.ts:320).
- `src/app/stores/workspace.store.ts:104-110` **flush() does not await an in-flight debounced write, so two concurrent workspace_set calls can commit out of order** — `write()` is fire-and-forget from the debounce timer (:83-85 `void this.write(doc)`), and `flush()` only clears a *pending* timer — it has no handle on a write already awaiting its IPC.
- `src/app/stores/workspace.store.ts:120-128` **localStorage->SQLite migration deletes the source copies before the first workspace_set has succeeded, and write() swallows the failure** — On the first backend load of an install that used the pre-SQLite localStorage layout, `workspace_get` returns null, the doc is read from localStorage, and both keys are removed immediately.
- `src/app/utils.ts:90-194` **Dead production code: appendPtyTail (~95 lines of VT folding) and isPermissionPrompt have no callers outside their own spec** — Grep across src/ finds `appendPtyTail` only in utils.ts and utils.spec.ts (which also carries a ~90-line duplicate reference implementation plus differential and perf test suites), and `isPermissionPrompt`/`PERMISSION_RE` likewise only in utils.ts and utils.spec.ts.
- `src/app/modals/runtime-row.component.ts:43` **Duplicate `kjVariant` attribute on the same button (and a malformed CSS custom property) — one of the two values is dead** — `<kj-button kjVariant="ghost" class="set-rt-link" kjVariant="quiet" …>` sets the same static input twice; only one binding can win, so the intent of the other is silently dropped and the button's variant depends on parser order rather than the markup.
- `src/app/modals/spawn-modal.component.ts:370-372` **Worktree name preview omits the backend's 60-char truncation (and its empty-name fallback)** — src-tauri/src/agents/service.rs:47-68 lowercases non-alphanumerics to spaces, joins with `_`, then `slug.truncate(60)` and trims `_`, falling back to `agent_<id[..6]>` when empty.
- `src/app/projects/project-actions.service.ts:43` **Every agent commit fires one `project_commits` git-log per project, and the result is thrown away** — agent-actions.service.ts:99 runs `void this.projects.refreshCommits(this.projects.all().map((p) => p.id))` after every successful agent commit. _(added by verifier)_
- `src/app/projects/project-actions.service.ts:47` **Dead commit feed: `ProjectActionsService.commits` is written but never read (its Infinity comparator is unreachable)** — `this.commits` (project-actions.service.ts:31) is set only at :47 and has no consumer anywhere in src/app — a grep for `projects.commits`/`.commits()` finds only `ProjectsStore.commits`, `AgentWorkStore.commitsFor` and `ChangelogRelease.commits`.
- `src/app/settings/settings.store.ts:305` **`install()` does not await the settings flush it performs "because the installer may exit the process"** — `flush()` is declared `flush(): void` and dispatches `void this.bridge.invoke(Commands.SettingsSet, …)` without returning the promise (settings.store.ts:256-263), so the `this.flush()` at :305 is fire-and-forget despite its comment ("the installer may exit the process — persist edits first"). _(added by verifier)_
- `src/app/settings/settings.store.ts:334` **Manual "Check now" / "Install & relaunch" bypass the dev-build guard, so a `tauri dev` session installs a release installer over itself** — `TauriUpdater.isAvailable()` returns false under `isDevMode()` precisely so a dev build never self-updates (tauri-updater.ts:15-21), and `UpdaterService` honours that for auto-install only (updater.service.ts:60).
- `src/app/updater/changelog.service.ts:20-28` **`cmpVer` turns any pre-release segment into 0, so beta releases are ordered wrongly (and dropped from "What's new")** — `'0.9.4-beta.1'.split('.')` yields `['0','9','4-beta','1']`; `Number('4-beta')` is NaN and `(NaN || 0)` collapses it to 0, so the tag compares as 0.9.0.1.
- `src/app/updater/update-watcher.service.ts:60-67` **`UpdateWatcherService.check()` sets its in-flight flag after an await, so two polls can run concurrently** — update-watcher.service.ts:63-67 — both `inFlight = true` and `lastCheck = Date.now()` are set after `await this.settings.ready()`.
- `src/app/updater/updater.service.ts:16` **Auto-update loop guard's 60 s cooldown is shorter than a real download+install+relaunch cycle, so it cannot stop the loop it exists for** — The marker is written before the download starts (line 74), so the measured interval spans download + installer + process exit + relaunch + splash + the next `update_check`.
- `src/app/backlog/backlog.component.ts:47-52` **Backlog project-filter popover has no outside-click/Escape dismissal — it only closes on a click in the board body** — The dropdown's open state is a bare signal toggled by its trigger; the only close paths are `setFilter()` and the board body's `(click)="filterOpen.set(false)"` at :100.
- `src/app/backlog/ticket-page.component.ts:68-70` **`shortId` is a no-op on real ticket ids — cards and breadcrumb print the full 36-char UUID** — `shortId` strips a leading "t", which only ever matched the old mock ids ("t1", still used in the specs).
- `src/app/backlog/ticket-page.component.ts:580-589` **Delete ticket removes the ticket and all its comments on a single click, with no confirmation and no undo** — The header "Delete" button (:218) calls deleteTicket() directly; it invokes ticket_remove, which does `PRAGMA foreign_keys = ON` + DELETE so comments cascade away (src-tauri/src/tickets/service.rs:220-228), then closes the tab.
- `src/app/dev-tools/dev-panel.component.ts:621-629` **Dev console's 1s heartbeat runs for the entire app lifetime, even though the panel is almost always closed** — `<app-dev-panel />` is mounted unconditionally by the shell (shell.component.ts:99) and the interval is created in the constructor, not when `open()` flips.
- `src/app/dev-tools/dev-panel.component.ts:738` **Dev console sort comparators never return 0 for ties, so sorting is inconsistent** — Every string/enum comparator in the panel is `x < y ? -dir : dir` — for two equal keys it returns `dir` for BOTH compare(a,b) and compare(b,a), which violates the total-order contract Array.prototype.sort requires. _(added by verifier)_
- `src/app/overview/mini-term.component.ts:109-111` **Dead `source` computed in MiniTerm (never read by template or code)** — The private computed is declared with a comment admitting it is "Debug aid only — never rendered".
- `src/app/overview/mini-term.component.ts:128-141` **Every overview card logs a console.debug on every hook-activity update, in production builds too** — The second constructor effect is a pure debug side effect with no isDevMode() gate.
- `src/app/perf/perf.store.ts:73-82` **Every Tauri invoke (including each terminal keystroke) forces a full perf-table recompute via the always-mounted status bar** — `record()` bumps `rev`, which `rows()` depends on (:104).
- `landing/dl-button.js:88` **Baked fallback version is v0.4.0 while the app is at 0.22.2 — shown to any visitor whose API lookup fails** — The CTA's version sub is baked as `v0.4.0`, as are landing/index.html:469 and landing/changelog.html:142,:162 (console-mock.js:185 bakes v0.4.1), while package.json/src-tauri/tauri.conf.json are at 0.22.2.
- `landing/dl-button.js:121-124` **update() can leave the CTA pointing at the previous release's installer while the label shows the new tag** — `update()` returns early when the fresh release has no `-setup.exe` asset, deliberately leaving the CTA as-is.
- `landing/home.js:49` **host._stop is never called — the two hero epicycle animations loop for the lifetime of the tab** — `loop()` schedules a 2.4s rAF animation, then `setTimeout(loop, HOLD)` at :44, forever.
- `landing/version.js:61-109` **The 6h cache gates only the paint, not the two GitHub API calls — every page view burns 2 unauthenticated requests** — The header states the results are 'cached in localStorage (6h) to avoid hammering the API on every visit', but the cache read at :51-58 only pre-paints; both `fetch` calls at :61 and :90 are unconditional top-level statements with no freshness check.
- `scripts/landing/gen-changelog.mjs:135-186` **gen-changelog.mjs main() (plus its GitHub API layer) is dead code that throws on the first line it reaches** — `main()` rewrites a `RELEASES:START/END` region inside landing/changelog.html, but those markers no longer exist anywhere in landing/ (grep for RELEASES:START / RELEASES:END / `const RELEASES =` in landing/ returns nothing — the page now fetches changelog.json at runtime, landing/changelog.html:213).
- `scripts/release/changelog-json.mjs:53-55` **upsertRelease always prepends, so appending a non-newest tag silently reorders the published history** — The entry is unconditionally placed at index 0 with no date/semver ordering, even though the docstring claims 'Newest-first is preserved'.
- `src/app/shared/file-drop.service.ts:64-70` **FileDropService.stop() is never called, and would leak the Tauri listener if it were** — `stop()` has no caller anywhere in src/ (the service is started once from src/app/shell/shell.component.ts:136 and never torn down) and no test exercises it.
- `src/app/shared/git/git-action-button.component.ts:256` **The confirm-above arm (`confirming`) is never cleared, so a stale confirmation lets a later single click spend** — runVariant() arms `confirming` with the variant key and returns, expecting the user's second click to be the confirmation. _(added by verifier)_
- `src/app/shared/rich-editor/rich-editor.component.ts:243-246` **Placeholder disappears for good once the user types and then deletes everything** — Both placeholder selectors require an empty node: `.rte-content:empty` or a single empty `<p>`.
- `src/app/shared/rich-editor/rich-editor.component.ts:350-359` **valueChange re-serializes the whole document on every editor update, including selection-only changes** — Lexical commits (and therefore fires update listeners) for selection changes as well as content changes, and this listener has no dirty check: every caret move, click or arrow key runs `$generateHtmlFromNodes` over the entire document and emits the result.
- `tools/perf-smoke/assert.mjs:73` **A null metric passes any budget (`null <= limit` is true) and prints as 'n/a  PASS'** — The pass test guards only against `undefined`.
- `tools/perf-smoke/noise.mjs:80-81` **--bytes-per-sec is a per-tick cap, not a rate: in `lines` mode the generator emits ~20x less than asked** — Each 50ms tick builds one chunk and writes `chunk.slice(0, bytesPerTick)`.
- `src/app/sidebar/files/file-tree.component.ts:37` **Stale doc comments claim file CRUD/drag/open are 'agent roots only' when they operate on the project's main checkout** — Both comments tell a maintainer that the destructive operations are gated to agent worktrees.
- `src/app/sidebar/files/file-tree.component.ts:304` **Folder openMap survives a root switch; only gitignored dirs then render open-but-empty** — `rootKey` is an input on a component the `@if (open && root)` block does not re-create when root changes from one truthy key to another (sidebar-files.component.ts:118-120), so `openMap` (file-tree.component.ts:284) carries over and is never pruned.
- `src/app/top-bar/top-bar.component.ts:299` **Shortcut hints in the chrome are hard-coded and ignore the Settings keymap overrides** — The command registry lets a user rebind any command via `settings().keymap`, which replaces `kbd`/`kbdAlt` at command-registry.service.ts:227-228.
- `src/app/top-bar/top-bar.component.ts:330` **The 'drag a tab onto another to tile them' hint counts the pinned Backlog tab, so it appears with a single agent tab** — `agentTabCount()` counts every tab whose kind is not 'orchestrator', and the Backlog tab is a pinned singleton that always exists (ui.store.ts:64-67).
- `src/app/top-bar/top-bar.component.ts:382` **suppressClick is a latch with no reset path — a tab drag that ends without a follow-up tab click swallows the user's next tab click** — `suppressClick` is set on every live pointer drop and cleared ONLY inside `onTabClick`.
- `src/app/top-bar/top-bar.component.ts:452` **Right-clicking a project tab opens an empty context menu** — A project tab's pane leaf carries the PROJECT id as its agentId (ui.store.ts:277), so `tabAgentIds` returns [projectId] and `onTabContext` takes the single-agent branch: `this.ui.openMenu(e, this.agentActions.agentMenu(ids[0]))` (:450-453). _(added by verifier)_
- `src/app/ui/ui.store.ts:175` **restoreWorkspace adopts persisted tabs wholesale without re-asserting the pinned Orchestrator/Backlog tabs** — `tabs.set(ws.tabs)` (ui.store.ts:175) replaces the seeded pinned pair with no check that an 'orchestrator'/'backlog' entry survived, and no other code path re-creates them (every other writer is openAgent/openProject/openTicket/closeTab/mergeTabs/reorderTab/ungroupTab/detachAgent — none inserts a pinned tab).
- `src/app/ui/ui.store.ts:293` **Per-tab / per-agent maps are never pruned on close, so the persisted workspace document grows without bound** — `closeTab` removes the tab and its pane root only; `gitViews`, `diffSelections`, `diffDirOpen` (keyed by agentId) and `filesRootOverride` (keyed by tabId) keep their entries, and `closeTabsForAgent` prunes nothing either.
- `src/app/stores/edits.store.ts:115-123` **EditsStore.closeAgent has no callers — deleting an agent leaks its buffers and its persisted scroll state** — closeAgent is documented as the "agent removed / worktree gone" cleanup but grep over src/ finds no call site (only notifications.store.ts and recent-files.service.ts have their own clearAgent).
- `src/app/workspace/editor-cap.ts:2-9` **Post-Monaco-migration comments still describe CodeMirror and files that no longer exist** — A maintainer reading these headers is told the wrong architecture.
- `src/app/workspace/file-view.component.ts:233-245` **FileView's onScan subscription leaks when the component is destroyed before the listen promise resolves** — AgentsStore.onScan is async (agents.store.ts:197-204 -> bridge.on, a Tauri listen round-trip).
- `src/app/workspace/file-view.component.ts:392-393` **Open image/PDF tabs re-read the entire file over IPC on every watcher scan and hash it into an O(size) string** — The onScan handler reloads unconditionally (it never consults p.changes to see whether THIS path changed), so for a media tab every agent://changed push — about one per second per working agent (src-tauri/src/watch/mod.rs:16-19 SETTLE 200ms / MAX_BURST 1s) — runs loadMedia: a full `file_read_binary` of the file, base64-encoded, across the Tauri IPC boundary.
- `src/app/workspace/monaco-loader.ts:80-81` **Monaco diff theme colours are silently dropped because the tokens they read are color-mix() values** — put() resolves a CSS custom property through tokenHex, which only accepts `#rrggbb` or `rgb()/rgba()` (:50-58).
- `src/app/workspace/pane-node.component.ts:549-551` **Save in the dirty-tab dialog silently does nothing when a save for that file is already in flight** — FileSaveService.save returns false WITHOUT flashing anything when the key is already in the `saving` set (file-save.service.ts:56).
- `src/app/workspace/pane-node.component.ts:670-692` **Divider/resizer drags leak their pointer listeners on pointercancel or mid-drag teardown** — Both drag helpers only unhook on `pointerup`.
- `src/app/workspace/git/conflict-view.component.ts:256-266` **The conflict result editor's "done" button re-opens the editor instead of closing it** — The textarea commits on focusout, and editResult() sets `editing` to null.
- `src/app/workspace/git/conflict-view.component.ts:546` **Recovered merge sessions are labelled "incoming" instead of the merged branch** — The recovery path only has SessionState.ours (the HEAD shorthand) and hardcodes the theirs label.
- `src/app/workspace/git/conflict-view.component.ts:550-561` **Per-segment resolution map is reset only when the agent id changes, not when the session is replaced** — resMap is keyed by file path and segment index, and the only reset is the `id !== lastId` effect.
- `src/app/workspace/git/diff-or-blame.component.ts:60-72` **A still-loading per-file diff renders as "no textual diff"** — commit-diff-view/range-diff-view pass `selDiff()`, which is the loadable's `data` and is null for a freshly-keyed entry while the IPC is in flight (git-inspect.store.ts:102-103 seeds `prev.data` = null for a new key).
- `src/app/workspace/git/diff-or-blame.component.ts:94-95` **Dead surfaces: unwired openCommit output, uncalled openComposer API, unreachable UnifiedCode view="file" branch** — Three non-trivial dead paths, all confirmed by grepping every non-spec template/usage: (1) DiffOrBlameComponent.openCommit is declared "Reserved … not wired yet", never emitted and never bound by a parent; (2) MonacoReviewApi.openComposer (review-comments.monaco.ts:45,434-436) has no caller — the composer only opens through the internal drag path; (3) UnifiedCodeComponent's view="file" branch (unified-code.component.ts:274-282, plus the side:"file" arm at :319) is unreachable because the component's only instantiation is diff-view.component.ts:247 with view="diff" — the writable file surface uses MonacoFileEditorComponent instead.
- `src/app/workspace/review/annotate-blame.component.ts:298-304` **Blame find highlighting is O(rows × matches) per change-detection pass** — isHit(i) does a linear `matches().includes(i)` and isActiveHit(i) recomputes the match array, and both are evaluated for every rendered row (template line 159).
- `src/app/workspace/review/review-comments.monaco.ts:388-398` **Review-comment drag leaves window listeners behind and touches a disposed editor if the surface is torn down mid-drag** — startDrag registers window mousemove/mouseup handlers that are only removed by its own `up`.
- `src-tauri/src/agents/adapters/codex.rs:114-123` **Unguarded toml_edit indexing panics if the user's config.toml has `hooks` as a non-table, aborting app startup** — `doc["hooks"]["pre_tool_use"] = value(pre)` relies on toml_edit's `IndexMut for Item`, which creates an implicit dotted table only when the item is `Item::None` and otherwise matches on `Item::Table` / `Item::Value(InlineTable)`, panicking on anything else.
- `src-tauri/src/agents/adapters/mod.rs:3-4` **Two stale doc comments describe mechanisms that do not exist (decision JSON; a cursor PTY fallback)** — (a) The module header says the trait hides 'the JSON it expects back for an allow/deny decision'.
- `src-tauri/src/agents/adapters/mod.rs:79` **ToolStatus::error hardcodes shim:false, hiding the "native installer" hint exactly when it matters** — `ToolStatus::ok` computes `shim = is_shim_path(&path)` (line 75); `ToolStatus::error` sets `shim: false` unconditionally even though it also carries a resolved `path`.
- `src-tauri/src/agents/adapters/mod.rs:107-109` **Dead trait surface: is_installed / which have no callers; supports_hooks and prompt_args are never overridden** — `AgentAdapter::is_installed` (107-109) is called by nothing; its only body calls `pub fn which` (554-556), which is likewise called by nothing else in src-tauri (grep over the crate finds only the definition and this one use).
- `src-tauri/src/agents/adapters/mod.rs:145-147` **Gemini's "Continue" button is indistinguishable from "Resume" — no session is restored, and the flash claims otherwise** — GeminiAdapter has no resume_argv override so it inherits None (mod.rs:145-147); tool_command's resume branch (runtime/mod.rs:676-681) is skipped and the fresh-launch branch runs, producing a bare `gemini --model <m>` in the worktree.
- `src-tauri/src/agents/adapters/mod.rs:438-452` **parse_semver returns the first digit-led dotted token anywhere in stdout+stderr, so a banner beats the real version** — `run_probe` concatenates stdout, a newline, then stderr (lines 412-416) and `parse_semver` returns the FIRST token that starts with a digit and contains a dot.
- `src-tauri/src/agents/adapters/mod.rs:516-524` **No prune pass: orrery hook groups for events dropped from the managed list stay in the user's config forever** — `merge_json_hooks` only visits the events in the `events` slice: for each it filters out prior orrery groups and appends a fresh one.
- `src-tauri/src/agents/adapters/mod.rs:527` **The merge rewrites the user's JSON with alphabetically reordered keys on every app start** — `serde_json` is pulled in without the `preserve_order` feature (src-tauri/Cargo.toml:30 `serde_json = "1"`; Cargo.lock:5279-5289 lists deps itoa/memchr/serde/serde_core/zmij with no `indexmap`), so `serde_json::Map` is a BTreeMap.
- `src-tauri/src/agents/adapters/mod.rs:699-716` **Shim parsing can return a non-PE `node` path as a "directly spawnable" program, bypassing the wrapper fallback** — `resolve_shim_argv` is documented (mod.rs:588-594) as returning a 'directly-spawnable argv', and `windows_launch_prefix:812-814` returns early on `Some(argv)`, skipping the `cmd.exe`/PowerShell wrapper fallback entirely.
- `src-tauri/src/agents/adapters/mod.rs:858-871` **Detection's PATH lookup uses a different extension set/order than the launcher, so Settings can report a different binary than the one spawned** — which_path (mod.rs:858-871) tries ['', '.exe', '.cmd', '.bat'] per PATH dir while find_program (runtime/mod.rs:868-878) tries exe>com>cmd>bat>ps1 plus PATHEXT with extensionless as a global last resort, so the path/version shown in Settings can name a different file than the launcher spawns.
- `src-tauri/src/agents/adapters/mod.rs:868` **which_path accepts any file on PATH with no executable-bit check, so a non-executable shadow makes detection report the tool as broken** — On Unix the candidate test is `cand.is_file()` only — no mode check — and the search stops at the first matching file in PATH order. _(added by verifier)_
- `src-tauri/src/agents/commands.rs:307-366` **agent_start runs DB reads + the PTY spawn on the async runtime rather than the blocking pool (perf risk, not a convention break)** — Every genuinely blocking command in this file hands its work to spawn_blocking (agent_spawn, agent_remove, agent_tree, agent_commit, …), but agent_start is a sync fn marked `#[tauri::command(async)]`, so Tauri runs its body as a task on the shared async runtime.
- `src-tauri/src/agents/fs_commands.rs:17-33` **safe_join's 'can never escape the worktree' guarantee is lexical only — no symlink resolution (no reachable escape found)** — safe_join only inspects the lexical components of the relative path; it never canonicalizes and never checks that the result stays under workdir.
- `src-tauri/src/agents/fs_commands.rs:85-94` **safe_join accepts ".", so file_delete with that path empties the whole worktree (or project checkout)** — Component::CurDir is explicitly allowed (line 28), so safe_join(workdir, ".") returns `workdir/.`.
- `src-tauri/src/agents/fs_commands.rs:322` **safe_join unit test asserts a Windows path is rejected, which is false on unix — the Rust test suite fails outside Windows** — `safe_join_rejects_escapes` includes "C:\\Windows\\x" in the must-reject list.
- `src-tauri/src/agents/service.rs:74-96` **effective_worktree_root creates a directory as a side effect of merely reading the setting** — effective_worktree_root calls std::fs::create_dir_all on the configured path to decide whether it is 'usable'.
- `src-tauri/src/agents/service.rs:406-443` **remove() would rename a project's main checkout aside for a project id — currently unreachable, blocked only by the delete modal's null-agent self-close** — record() falls back to project_pseudo_record for any id that is not an agent row (service.rs:733, 743-777), returning worktree = the project's MAIN checkout.
- `src-tauri/src/agents/service.rs:554-571` **running_ids/reset_running match statuses ('blocked', 'waiting') that no code ever persists** — Both queries filter `status IN ('running','blocked','waiting')`, but the only statuses ever written to the agents table are 'running' (commands.rs:370-378), 'idle' (commands.rs:424-432 and the frontend's exit handler, src/app/agents/agent-runtime.service.ts:563) and whatever agent_update is handed.
- `src-tauri/src/core/errors.rs:8-11` **Dead error variants and an unused public type alias in the core data layer** — `DbError::NotFound` and `DbError::InsertError` are never constructed anywhere in src-tauri (a grep for `DbError::NotFound` / `DbError::InsertError` matches only their declaration): every not-found case goes through the per-domain `ProjectError::NotFound` / `TicketError::NotFound` / `AgentError::NotFound`, and every sqlite failure through `DbError::Sqlite`.
- `src-tauri/src/lib.rs:126-130` **Telemetry auto-disable does a whole-document read-modify-write of settings from a background callback — last writer wins** — The raw-trace notifier registered at startup reacts to an auto-disable by re-reading the entire settings document, flipping one bool, and writing the whole document back.
- `src-tauri/src/projects/commands.rs:11` **project_list/project_update run per-project disk+git enrichment on the async runtime, breaking the file's own spawn_blocking convention** — Every other project command that touches disk is `#[tauri::command] pub async` + spawn_blocking with an explicit "Blocking pool" doc comment (project_create :17-18, project_init_git :49-50, project_detect_git :98-99, project_tree :107-110, project_commits :141-142). _(added by verifier)_
- `src-tauri/src/tickets/service.rs:107-110` **Ad-hoc ALTER migrations discard their errors and there is no schema version, so a genuine migration failure is indistinguishable from a no-op** — There is no migration table or schema-version row anywhere in the backend; each service's constructor runs `CREATE TABLE IF NOT EXISTS` plus ad-hoc ALTERs whose result is thrown away with `let _ =`.
- `src-tauri/src/git/gix_backend.rs:258-290` **Line counts use Myers while hunks use Histogram, so the counters and the gutter can disagree** — `line_stats` (the +N/−N shown per file in the change list and in file history) computes with `Algorithm::Myers`, while `hunks_of` — the same old/new byte pair, used for the editor's change markers and for `revert_hunk` — computes with `Algorithm::Histogram` and additionally runs `postprocess_lines`.
- `src-tauri/src/git/gix_backend.rs:708-728` **The status cache key walks and stats the whole worktree on every call, cache hit included** — `status_key` is computed before the cache lookup (:673) and always calls `crate::search::worktree_fingerprint(&workdir)` (search/mod.rs:368-391), an `ignore`-crate walk that stats every non-ignored file in the worktree.
- `src-tauri/src/git/gix_backend.rs:1721-1725` **Frontend-supplied `rel` is joined to the worktree without validation, so an absolute or ../ path escapes it** — `conflict_resolve` takes `rel` straight from the `agent_conflict_resolve` command (agents/commands.rs:871-890), joins it to the workdir, `create_dir_all`s the parent and writes the content.
- `src-tauri/src/git/service.rs:96-98` **GitService::from_backend is dead code (the second-implementation seam it documents has no callers)** — `from_backend` — and with it the only reason `GitService` wraps an `Arc<dyn GitBackend>` rather than a `GixBackend` — has no callers anywhere in the crate, tests included (`grep -rn from_backend src-tauri` matches only its definition and the module doc that advertises it).
- `src-tauri/src/cli/hook.rs:92-94` **The CLI's only diagnostic log line can never be emitted — no logger exists in the CLI path** — main.rs runs `cli::invoked_as_cli()` / `cli::run()` and returns before `orrery_lib::run()`, and the only logger in the crate is tauri-plugin-log installed inside the Tauri builder (src-tauri/src/core/logger.rs).
- `src-tauri/src/cli/hook.rs:107-113` **Non-JSON stdin is spliced into the envelope verbatim, silently discarding the whole event** — The envelope is assembled by string formatting with the raw stdin bytes pasted into the `payload` slot; only the three scalar fields go through `quote()`.
- `src-tauri/src/cli/hook.rs:123-129` **CLI doc comment, 595s read timeout and the integration test all describe a decision round-trip the bridge never performs** — The module doc (hook.rs:1-5) and `post`'s comment claim the CLI "waits, and prints the decision JSON the agent then obeys" and that "the bridge holds the connection while the user decides", justifying a 595-second read timeout.
- `src-tauri/src/cli/mod.rs:34-41` **invoked_as_cli() panics on a non-UTF-8 argv before anything else runs** — `std::env::args()` panics during iteration if any argument is not valid Unicode (`args_os()` is the non-panicking form).
- `src-tauri/src/hooks/mod.rs:120` **Accept loop busy-spins forever on a persistent accept() error** — `listener.incoming()` never yields None, and `.flatten()` silently discards every `Err`.
- `src-tauri/src/hooks/protocol.rs:326-337` **Event-taxonomy arms with no installed hook: Compact and Error are unreachable in production** — `parse()` maps "PreCompact"/"PostCompact"/"Compact" and "Error", and `AgentEvent` carries `Compact{phase}` and `Error{kind,message}` with their own `activity_detail()`/`kind()` arms plus a dedicated no-emit branch in handle() (mod.rs:307-310).
- `src-tauri/src/hooks/protocol.rs:603` **A JSON-null `error` field in a tool response is treated as a tool failure** — `resp.get("error").is_some()` is true whenever the key is *present*, including `"error": null` — a very common shape for successful results in JSON tool protocols.
- `src-tauri/src/hooks/protocol.rs:719-720` **Unauthenticated, unbounded body allocation and no read timeout on accepted connections** — `read_request` parses the client-supplied Content-Length and immediately does `vec![0u8; len]` — before `handle()` ever compares the bearer token (mod.rs:177-181).
- `src-tauri/src/core/emit.rs:268` **A failing trace-file open is retried, and logged, on every single emit** — In trace_append, `rotate` is derived from the guard being None (emit.rs:264-267). _(added by verifier)_
- `src-tauri/src/core/emit.rs:290-293` **Trace NDJSON lines are built with Rust Debug formatting, which is not JSON** — The trace line is assembled by hand with `{:?}` for the event name and key.
- `src-tauri/src/core/emit.rs:416-420` **No flush on shutdown: buffered trace lines are dropped on every normal exit** — The only flush points are the detached 60 s thread (emit.rs:416-420) and set_raw_trace(false)/auto_disable (emit.rs:313, 338).
- `src-tauri/src/core/emit.rs:438-450` **Day summary file is written from a never-reset process-cumulative registry** — flush_summary computes `today()` fresh on every 60 s flush but writes `snapshot()` of the global REG, which is process-cumulative and never reset at a day boundary (record() at emit.rs:95-127 only ever adds).
- `src-tauri/src/defender.rs:127` **UAC-cancel detection matches an English-only substring, so a declined prompt is recorded as a failure on localized Windows** — The launcher distinguishes 'the user dismissed UAC' from 'the launch broke' by regex-matching the .NET exception message against the literal `'cancel'`.
- `src-tauri/src/metrics/commands.rs:20-30` **system_metrics' doc comment is attached to process_tree, and lib.rs still documents a 3s metrics cadence** — Two doc blocks sit back to back at commands.rs:20-30 with no item between them, so the paragraph written for `system_metrics` ('Optional initial value so the UI can paint before the first push… a single scoped refresh on the SAME warm sampler') becomes part of `process_tree`'s rustdoc, describing behaviour process_tree does not have (it never serves a cache and can force a full sweep).
- `src-tauri/src/metrics/commands.rs:58-61` **process_tree's scoped refresh never includes the job-object / WebView2 backstop pids it then reports on** — process_tree builds the refresh root list from `app_pid` plus the agent pids only (commands.rs:58-60) and hands it to refresh_and_procs, which on a non-`discover` poll calls refresh_scoped (metrics/mod.rs:364-370).
- `src-tauri/src/metrics/mod.rs:125` **scoped_streak is shared by two callers with different cadences, so full machine sweeps run ~3x more often than documented while the perf panel is open** — `scoped_streak` lives on the single shared MetricsSampler and is bumped by EVERY scoped refresh (mod.rs:125), whichever caller made it: the 5 s metrics push loop via refresh_and_sample (mod.rs:341, lib.rs:193) and the pull-based process_tree via refresh_and_procs (mod.rs:369). _(added by verifier)_
- `src-tauri/src/update.rs:143-153` **update://progress is emitted once per download chunk with no throttling** — The `on_chunk` closure passed to `update.download` emits an `update://progress` event on every chunk the HTTP body yields.
- `src-tauri/updater-stub/src/banner.html:188-191` **Dead UI entry point: window.__prepare is never invoked by the stub** — `window.__prepare` sets the title to 'Updating Orrery' and the status to 'preparing…', but the Rust side never evaluates it: the only scripts main.rs ever runs are `__msi`/`__nsis` (main.rs:187-190), `__progress`, `__restart`, `__fail` and `__close`.
- `src-tauri/updater-stub/src/main.rs:196` **Failed updates abandon the full installer package in %TEMP% (one per version); the stub log never rotates** — The downloaded package (an MSI or NSIS setup, tens to hundreds of MB, written at update.rs:216-218) is removed only on the success branch `Ui::Done` (main.rs:196).
- `src-tauri/updater-stub/src/main.rs:205-211` **Updater stub's failure branch has no Rust-side terminal path — every exit depends on banner.html's JS** — The stub's window ignores CloseRequested (main.rs:184) and is always-on-top and undecorated (main.rs:149-152), so the only ways it ever terminates are (a) `Ui::Quit`, which is only scheduled from the success branch, and (b) the IPC message `relaunch`, which only banner.html's JS can send — either from the 5 s countdown in `__fail` (banner.html:216-224) or the 'Relaunch now' button.
- `src-tauri/src/agents/commands.rs:407-412` **Project-shell scrollback rings are never freed** — `shell_start` runs the shell through the same pipeline keyed by the PROJECT uuid, so it creates a scrollback ring (mod.rs:361 → scrollback.rs:87-92) capped at RING_CAP = 1MB.
- `src-tauri/src/runtime/digest.rs:77-79` **Two-char escape branch consumes only one char, so 3-byte escapes leak their final byte** — After ESC, anything that is not '[' or ']' is treated as a two-character escape and exactly one following char is dropped.
- `src-tauri/src/runtime/heuristics.rs:112-131` **Permission needles are broad substrings matched over the whole 5-line output tail** — `is_awaiting_input` / `is_permission_prompt` run over `tail()` — the last five non-empty folded lines of ordinary agent output, not a prompt region.
- `src-tauri/src/runtime/mod.rs:373` **The PTY reader treats any read error as EOF, so one transient error kills the agent's output stream while the process keeps running** — The reader loop collapses `Ok(0)` and every `Err(_)` into the same `break`, with no distinction for retryable errors (`ErrorKind::Interrupted`, `WouldBlock`). _(added by verifier)_
- `src-tauri/src/runtime/mod.rs:505-507` **RuntimeService::interest_mode is dead — every caller uses the free function** — The public method just forwards to the module-level `interest_mode` (mod.rs:36-38).
- `src-tauri/src/runtime/mod.rs:576-583` **Unix stop() is SIGKILL to the whole group with no SIGTERM grace, so the agent CLI cannot flush its session** — `kill_proc` sends SIGKILL directly to the child's process group and then kills the child; there is no SIGTERM/grace phase anywhere.
- `src-tauri/src/runtime/mod.rs:576-581` **getpgid/killpg run on a pid the wait thread may already have reaped** — The wait thread reaps the child at mod.rs:406 (`child.wait()`) and only removes the map entry afterwards at mod.rs:416.
- `src-tauri/src/fs/mod.rs:47-51` **list_dir joins the caller-supplied relative path onto the workdir with no containment check, unlike the equivalent guard in apply_replace** — scan builds the absolute path as workdir.join(rel) with rel taken verbatim from the command argument.
- `src-tauri/src/history/commands.rs:34` **history_file renders any read failure of the current file as empty text, so the diff claims the file's content was deleted** — The 'new' side of the history diff is std::fs::read_to_string(...).unwrap_or_default().
- `src-tauri/src/history/mod.rs:154-184` **Every settled fs burst parses index.jsonl twice and re-runs full prune accounting under a global lock, on the debounce thread, ahead of the agent://changed emit** — The watch emit closure calls history.snapshot for every settled burst of every agent BEFORE emit_keyed (watch/mod.rs:222-231), and snapshot holds the service-wide write_lock (:117) across the whole thing.
- `src-tauri/src/history/mod.rs:286-290` **prune's entry_count_hint parameter and its early-return branch are dead — both outcomes of the condition do nothing** — The only work prune performs after computing `keep` lives inside `if changed { ...
- `src-tauri/src/search/mod.rs:199` **Replace preview of a long line is windowed from the start while the match text is windowed around the match, so the preview shows no replacement** — For a line longer than MAX_LINE_BYTES the two sides window differently: window_line centres on the FIRST match (search/mod.rs:307-318 — it seeks to `first.saturating_sub(80)` and emits a leading '…'), while preview_line always truncates the replaced line at byte 0..500 and appends a trailing '…' (:200-206). _(added by verifier)_
- `src-tauri/src/search/mod.rs:466-473` **Per-file match cap is invisible to the UI despite the comment claiming otherwise, and the streamed `files` count is files walked, not files searched** — The sink stops a file at MAX_PER_FILE with `Ok(per_file < MAX_PER_FILE && total < max_results)`; unlike the overall cap (:489-492) this sets no flag and the true per-file count is never computed or sent, so a file with 5000 hits reports exactly 200 and the UI has no way to say so.

## Rejected on verification (6)

Listed so the reader can see what was considered and why it was dropped.

- `src/app/agents/agent-actions.service.ts:100` Overlay masks nothing - Agent.commits is a permanently-zero backend field — **refuted**: The premise that the overlay masks a backend value is wrong: Agent.commits is a hardcoded transient - AgentService::enrich sets `commits: 0` for every record (src-tauri/src/agents/service.rs:160) and nothing in src-tauri ever writes it (grep for `commits` finds only the list command, not a field write), so list()/get()/agent://updated always carry 0. The overlay therefore shadows nothing; it is the only thing that ever makes ag.commits non-zero. The real defect is the inverse and is filed as fe-agents-terminal-missed-01.
- `src/app/metrics/telemetry.store.ts:42-43` TelemetryStore priming clobbers a trace-state push that lands during the await — the recording indicator can read "off" while the trace records — **refuted**: The described window is not reachable. core::emit::init (src-tauri/src/lib.rs:107-138) arms the trace from persisted settings inside Tauri setup(), long before the webview exists, so the telemetry_trace_state snapshot the frontend takes is already correct and the priming write at telemetry.store.ts:43 cannot clobber anything. The finder's trigger — settings load re-applying the toggle via settings_set — does not exist: SettingsStore.load only reads (settings.store.ts:161-164) and set_raw_trace returns early when unchanged (src-tauri/src/core/emit.rs:329-332), so no telemetry://trace event fires at startup. The only other transition source is the 30min/200MB cap, which cannot fire in the ms-wide await. traceReason staying null is not a defect either — TraceState (emit.rs:348-355) carries no reason field to prime from.
- `src/app/overview/graph-view.component.ts:69` Graph view dereferences `project.name[0]` with no guard — **refuted**: The finder's own analysis shows there is no reachable failure, and I confirmed it: create_local rejects an empty trimmed name (src-tauri/src/projects/service.rs:151-154), the clone path derives a non-empty fallback (:239-249), and update() ignores an empty name (:265-269); project-actions.service.ts passes the user's name straight through with no optimistic empty-name row. A name of "  X" survives but name[0] is then ' ', whose .toUpperCase() is fine. An unguarded deref with no path to reach it is not a defect to report.
- `src-tauri/src/hooks/mod.rs:246` The transcript tail is read on every hook, including the four event classes that discard it — **refuted**: The premise about the hot path is wrong. mod.rs:282-298 — the arms for ToolStart/ToolEnd/AgentMessage/Notification/UserPrompt/SessionStart and TurnEnd — all consume `content`, and those are exactly the PreToolUse/PostToolUse/Notification events the finding calls hot. The only arms that discard it are PermissionRequest (mod.rs:262-277), SessionEnd (:301-304), Error and Compact/Unknown — and Error/Compact are never installed by any adapter, while PermissionRequest/SessionEnd are low-volume. So the unconditional call at mod.rs:246 wastes a 64 KB tail read only on rare events; there is no per-tool-call waste to reclaim.
- `src-tauri/src/update.rs:241-242` update_install returns Ok after app.exit(0), letting the auto-update path relaunch a second instance into the installer — **refuted**: The premise that the Ok response can be delivered before the exit is wrong: both are posted to the SAME main-thread message queue, and the exit is posted first. `app.exit(0)` (update.rs:241) → `request_exit` → `proxy.send_event(Message::RequestExit(code))` (tauri-runtime-wry-2.11.2/src/lib.rs:2751-2758), handled at :4361-4372 by setting `ControlFlow::Exit`. The command's Ok is returned afterwards and reaches the webview only via wry's responder, which when called off the main thread does `Self::dispatch_handler(hwnd, handler)` → `PostMessageW(hwnd, EXEC_MSG_ID, …)` (wry-0.55.1/src/webview2/mod.rs:994-1017, 1153-1177); the postMessage IPC variant goes through responder_eval → send_user_message, also queued behind it. FIFO ordering means the loop exits before the reply is dispatched, so updater.service.ts:87-88 never runs — the comment at tauri-updater.ts:63-64 is accurate, not merely assumed.
- `src-tauri/updater-stub/src/main.rs:212-219` Ui::Quit blocks the event-loop thread for 600 ms right after requesting the close animation — **refuted**: The 600 ms sleep at main.rs:212-219 is real, but the claimed consequence is not established. WebView2 in windowed hosting renders and runs script in the browser/renderer processes; ExecuteScript is dispatched over WebView2's IPC during the call, so a blocked host message pump does not stop the `.closing` CSS transition from painting — that decoupling is precisely why a hung WebView2 host app still shows live web content. Even in the worst case the only effect is a missing fade before a window that is already committed to closing; no defect in behaviour is demonstrated. The finder's own confidence note concedes this could not be run.
