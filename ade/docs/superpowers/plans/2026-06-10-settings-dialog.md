# Settings dialog (from KJ design bundle v2, settings.jsx final state)

Implement ONLY what the trimmed dialog shows. Design source:
`%TEMP%\orchestra-design-v2\kj\project\settings.jsx` (+ topbar gear). User
removed: allowlist editor, background sub-toggle, unanswered-prompt row,
auto-dismiss, max-concurrent, nav footer; footer has Cancel + Done.

## Schema (persisted as ONE JSON row, serde defaults for forward-compat)

```
channel: "stable"|"beta"          updatePolicy: "auto"|"notify"|"manual"
defaultTool: string               toolModel: Record<tool,string>
toolEffort: Record<tool,string>   branchTemplate: "agent/{name}"
worktreeRoot: string (default = current root)        autoResume: bool
autoApprove: Record<tool,"off"|"allowlist"|"everything">
remoteApproval: bool
osNotifications: bool   events: {finished,question,permission,error}: bool
sound: bool   soundName: "Ping"|"Chime"|"Pop"|"Glass"|"Submarine"   volume: 0-100
```

Curated model/effort lists are CODE (single source: the spawn modal's TOOLS
moved/shared), with real ids — not the design's fictional ones. Detected
tools from existing detect_tools.

## T1 — backend (settings core + consumers + updater channel)

- `settings` table (key TEXT PK, value TEXT) in the existing DB; `Settings`
  struct serde camelCase w/ Default; commands `settings_get` / `settings_set`
  (perf::timed, register in lib.rs).
- service.rs create(): branch from template ({name}→wt_name, {tool}, {date}
  MMDD), worktree under settings.worktreeRoot (fallback ctor root). Sanitize.
- Adapters: spawn args per autoApprove policy — claude "everything" →
  `--dangerously-skip-permissions`; codex → `--dangerously-bypass-approvals-and-sandbox`
  (verify exact flag in adapter); cursor → `--force`; "off"/"allowlist" =
  tool default (documented honestly). gemini: none.
- `agents_interrupted` command: ids that were running before reset_running
  at launch (one-shot, for auto-resume).
- Updater channel: Rust command `update_check(channel)` →
  `{version,date,notes}|null` and `update_install(channel)` using
  tauri_plugin_updater UpdaterBuilder with endpoint override: stable =
  current latest.json URL; beta = same repo `latest-beta.json`. NEVER touch
  productName/upgradeCode. Release-pipeline beta manifest = follow-up.

## T2 — frontend settings UI

- `settings.store.ts`: load-on-init, instant-apply (debounced settings_set),
  per-key signals or one settings signal + helpers; Settings type in models.
- `modals/settings-modal.component.ts`: faithful Angular port of final
  settings.jsx (nav, rows, Seg/Toggle/Combo/Select/slider primitives, reset
  pills, reset-all, danger-confirm flow, beta warn chip, Cancel/Done).
  Disabled (not hidden) rows when gated. Esc closes (combo first).
- Top-bar gear (left of theme/window controls) opens it; UiStore signal or
  store-level open flag.
- Updates section wiring: VersionService current version; Check now →
  update_check(channel) w/ spinner + "checked just now"; update-available
  card only when a check returned one (version/date/notes link → releases
  page; size omitted if unknown); Install & relaunch → update_install;
  Later dismisses card. updatePolicy consumed by updater.service at startup:
  auto = today's behavior; notify = check + surface card/notification, no
  install; manual = no startup check. Nav dot on Updates only when an update
  is known available.

## T3 — frontend consumers (after T2 lands)

- Spawn modal prefills: defaultTool (if detected), toolModel/toolEffort per
  tool, falls back to current hardcoded defaults.
- Auto-resume: on startup when autoResume, agents_interrupted() →
  startProcess(id,{resume:true}) for agents with a session.
- Notification gating in the raise path: per-event toggles (finished/
  question/permission/error map to existing kinds; off = not raised at all
  in-app feed still records? NO — design copy says OS-only gating for master
  toggle; events toggles gate raising the alert entirely). osNotifications
  master: on = also fire native toast (plugin-notification; request
  permission once); off = in-app only. remoteApproval: permission prompts
  fire a native toast even when app unfocused; clicking focuses app + opens
  agent terminal (action buttons are a platform minefield — out of scope).
- Sound: WebAudio-synthesized cues (5 presets), volume %, played when a
  notification fires and sound on (and master on).

## Tests

Per piece: Rust settings round-trip + branch template + interrupted ids;
vitest: store instant-apply, modal renders sections + danger confirm + dirty
reset, spawn prefill, notification gating + sound trigger (Audio mocked),
updater policy branching. tsc + full suites green per batch.
