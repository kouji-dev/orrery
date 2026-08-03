/* global React, Icon, ToolBadge */
// ORCHESTRA — Settings surface (Orrery preferences).
// A dedicated modal opened from the gear in the top bar. Instant-apply,
// per-row "reset to default", Esc / × closes. Built entirely on ORCHESTRA
// tokens + the existing control vocabulary so it reads as a sibling of the
// dev console, not a generic preferences page.
//
// Exports (to window): SettingsModal, SETTINGS_DEFAULTS, SETTINGS_DETECTED.

const { useState: useSS, useEffect: useSE, useRef: useSR } = React;

// ── defaults / fixtures ─────────────────────────────────────────────────────
const SETTINGS_DEFAULTS = {
  // Updates
  channel: "stable",            // stable | beta
  updatePolicy: "notify",       // auto | notify | manual
  // Agent defaults
  defaultTool: "claude",        // claude | codex | cursor | gemini
  toolModel: { claude: "sonnet-4.6", codex: "gpt-5.1-codex", cursor: "composer-1", gemini: "gemini-2.5-pro" },
  toolEffort: { codex: "high" },
  branchTemplate: "agent/{name}",
  worktreeRoot: "~/.orchestra/worktrees",
  autoResume: true,
  // Permissions & safety
  autoApprove: { claude: "allowlist", codex: "off", cursor: "off" }, // off | allowlist | everything
  remoteApproval: true,
  // Notifications
  osNotifications: true,
  events: { finished: true, question: true, permission: true, error: true },
  sound: true,
  soundName: "Ping",
  volume: 70,
  // Editor & keymap
  keymap: "intellij",           // intellij | vscode
  autosave: false,
  autosaveDelay: 2,             // seconds after typing stops
  structureView: true,
  gutterMarkers: true,
  // Memory & sleep
  autoSleep: true,
  sleepExitedAfter: 10,         // minutes
  sleepIdleAfter: 30,           // minutes
  webglVisibleOnly: true,
  ringSize: "1 MB",
  // Per-project overrides { [projectId]: {model, effort, autoApprove, budgetUsd, autosave, lsp} }
  perProject: {},
};

// Per-tool runtime detection. Three outcomes:
//   ok      — binary found on PATH and runs; we know its resolved path + version
//   error   — binary found, but launching it failed (perm / arch / wrapper) → user must point us at a working path
//   missing — nothing found anywhere → user can locate it manually
const SETTINGS_DETECTED = {
  claude: { status: "ok",      path: "/opt/homebrew/bin/claude",        version: "1.4.2",  source: "path" },
  codex:  { status: "ok",      path: "/Users/ada/.local/bin/codex",     version: "0.31.0", source: "path" },
  cursor: { status: "error",   path: "/usr/local/bin/cursor-agent",     version: null,     source: "path",
            reason: "found, but exited 126 — file is not executable" },
  gemini: { status: "missing", path: null,                              version: null,     source: null },
};
const isRunnable = (det, id) => (det[id] || {}).status === "ok";
const SOUND_OPTIONS = ["Ping", "Chime", "Pop", "Glass", "Submarine"];
// available remote update (mock)
const SETTINGS_UPDATE = { version: "0.9.4", current: "0.9.2", size: "18.4 MB", date: "Jun 18, 2026" };
// where Orrery writes its rolling diagnostics log on disk
const SETTINGS_LOGFILE = "~/.orchestra/logs/orrery.log";

const SECTIONS = [
  { id: "updates", label: "Updates", icon: "refresh" },
  { id: "agent", label: "Agent defaults", icon: "agent" },
  { id: "editor", label: "Editor & keymap", icon: "file" },
  { id: "memory", label: "Memory & sleep", icon: "cpu" },
  { id: "project", label: "Per-project", icon: "box" },
  { id: "perms", label: "Permissions & safety", icon: "lock" },
  { id: "notif", label: "Notifications", icon: "bell" },
];
const SECTIONS_OLD = [
  { id: "updates", label: "Updates", icon: "refresh" },
  { id: "agent", label: "Agent defaults", icon: "agent" },
  { id: "perms", label: "Permissions & safety", icon: "lock" },
  { id: "notif", label: "Notifications", icon: "bell" },
];

const sClone = (x) => JSON.parse(JSON.stringify(x));
const sEq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

// ── scoped styles (all prefixed .set- ; ORCHESTRA tokens only) ───────────────
const SETTINGS_STYLE = `
.set-backdrop{position:fixed;inset:0;z-index:80;display:grid;place-items:center;padding:28px;
  background:rgba(4,5,9,.58);-webkit-backdrop-filter:blur(4px);backdrop-filter:blur(4px);}
[data-theme="light"] .set-backdrop{background:rgba(20,24,40,.34);}

.set-modal{--set-amber:#f5c451;--set-danger:#ff5d7a;
  width:760px;max-width:calc(100vw - 56px);height:600px;max-height:84vh;display:flex;
  background:var(--panel);border:1px solid var(--hair-2);border-radius:15px;overflow:hidden;
  box-shadow:var(--shadow),0 0 0 1px rgba(var(--accent-rgb),.05);
  font-family:var(--font-mono);color:var(--ink);
  transform-origin:center;animation:set-pop .22s cubic-bezier(.2,.7,.2,1);}
[data-theme="light"] .set-modal{--set-amber:#a9700f;--set-danger:#d6304e;}
/* transform-only entrance: if the frame is throttled and the animation freezes
   at 0%, content stays visible (just offset) instead of stuck at opacity:0 */
@keyframes set-pop{from{transform:translateY(10px) scale(.99)}to{transform:none}}
@media (prefers-reduced-motion:reduce){.set-modal{animation:none}}

/* ── left nav ── */
.set-nav{width:204px;flex:none;background:var(--panel-2);border-right:1px solid var(--hair);
  display:flex;flex-direction:column;padding:13px 11px;}
.set-brand{display:flex;align-items:center;gap:9px;padding:5px 8px 14px;}
.set-brand .gi{width:26px;height:26px;border-radius:8px;display:grid;place-items:center;flex:none;
  color:var(--accent);background:color-mix(in oklch,var(--accent),transparent 86%);
  box-shadow:inset 0 0 0 1px color-mix(in oklch,var(--accent),transparent 64%);}
.set-brand .bt{font-family:var(--font-disp);font-size:14px;font-weight:600;letter-spacing:-.01em;}
.set-brand .bs{font-size:9px;color:var(--ink-4);letter-spacing:.04em;}
.set-nav-list{display:flex;flex-direction:column;gap:2px;}
.set-nav-item{position:relative;display:flex;align-items:center;gap:10px;height:35px;padding:0 11px;
  border-radius:9px;color:var(--ink-3);cursor:pointer;font-size:12.5px;border:1px solid transparent;
  transition:background .12s,color .12s,border-color .12s;text-align:left;background:transparent;
  font-family:var(--font-mono);width:100%;}
.set-nav-item:hover{background:var(--panel-3);color:var(--ink-2);}
.set-nav-item.on{background:var(--panel-3);color:var(--ink);border-color:var(--hair-2);}
.set-nav-item svg{width:15px;height:15px;flex:none;color:var(--ink-4);transition:color .12s;}
.set-nav-item:hover svg,.set-nav-item.on svg{color:var(--accent);}
.set-nav-item .lb{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-nav-item.on::before{content:"";position:absolute;left:-11px;top:9px;bottom:9px;width:2.5px;
  border-radius:2px;background:linear-gradient(var(--accent),var(--accent-2));}
.set-nav-dot{width:6px;height:6px;border-radius:50%;background:var(--set-amber);flex:none;
  box-shadow:0 0 7px -1px var(--set-amber);}
.set-nav-foot{margin-top:auto;padding:10px 9px 4px;display:flex;align-items:center;gap:7px;
  font-size:9.5px;color:var(--ink-4);border-top:1px solid var(--hair);}

/* ── right column ── */
.set-main{flex:1;min-width:0;display:flex;flex-direction:column;background:var(--panel);}
.set-head{flex:none;display:flex;align-items:center;gap:11px;padding:15px 16px 14px;
  border-bottom:1px solid var(--hair);}
.set-head .ht{font-family:var(--font-disp);font-size:15.5px;font-weight:600;letter-spacing:-.01em;}
.set-head .hs{font-size:10px;color:var(--ink-4);margin-top:2px;}
.set-x{margin-left:auto;flex:none;width:28px;height:28px;border-radius:7px;border:1px solid transparent;
  background:transparent;color:var(--ink-3);cursor:pointer;display:grid;place-items:center;transition:all .12s;}
.set-x:hover{background:var(--panel-3);color:var(--ink);border-color:var(--hair);}

.set-body{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;padding:4px 18px 20px;}
.set-body::-webkit-scrollbar{width:9px;}
.set-body::-webkit-scrollbar-thumb{background:var(--hair-2);border-radius:6px;border:2px solid transparent;background-clip:padding-box;}
.set-body::-webkit-scrollbar-thumb:hover{background:var(--ink-4);background-clip:padding-box;}

.set-grp{padding:15px 0 16px;border-bottom:1px solid var(--hair);}
.set-grp:last-child{border-bottom:none;}
.set-grp-h{font-size:9.5px;letter-spacing:.13em;text-transform:uppercase;color:var(--ink-3);
  margin-bottom:6px;display:flex;align-items:center;gap:8px;}
.set-grp-h .ln{flex:1;height:1px;background:var(--hair);}

/* ── row ── */
.set-row{display:flex;align-items:flex-start;gap:18px;padding:10px 0;}
.set-row.wide{flex-direction:column;gap:11px;}
.set-row.dis{opacity:.45;pointer-events:none;}
.set-row-main{flex:1;min-width:0;display:flex;flex-direction:column;gap:3px;}
.set-row.wide .set-row-main{width:100%;}
.set-row-lbl{font-size:12.5px;color:var(--ink);display:flex;align-items:center;gap:8px;line-height:1.2;}
.set-row-help{font-size:10.5px;color:var(--ink-4);line-height:1.5;max-width:46ch;}
.set-row-help code{font-family:var(--font-mono);color:var(--ink-3);background:var(--panel-2);
  padding:0 4px;border-radius:4px;font-size:10px;}
.set-row-ctrl{flex:none;display:flex;align-items:center;gap:8px;padding-top:1px;}
.set-row.wide .set-row-ctrl{width:100%;padding-top:0;}

.set-reset{display:inline-flex;align-items:center;gap:4px;height:18px;padding:0 7px 0 5px;
  border-radius:999px;border:1px solid var(--hair);background:var(--panel-2);color:var(--ink-3);
  font-family:var(--font-mono);font-size:8.5px;letter-spacing:.06em;text-transform:uppercase;
  cursor:pointer;transition:all .12s;flex:none;}
.set-reset:hover{color:var(--accent);border-color:color-mix(in oklch,var(--accent),transparent 50%);
  background:color-mix(in oklch,var(--accent),transparent 90%);}
.set-reset svg{width:9px;height:9px;}

/* ── segmented control ── */
.set-seg{display:inline-flex;padding:2px;background:var(--panel-2);border:1px solid var(--hair);
  border-radius:8px;gap:2px;}
.set-seg button{position:relative;display:inline-flex;align-items:center;justify-content:center;gap:6px;
  height:26px;padding:0 12px;border:none;background:transparent;color:var(--ink-3);
  font-family:var(--font-mono);font-size:11.5px;cursor:pointer;border-radius:6px;white-space:nowrap;
  transition:all .12s;}
.set-seg button:hover{color:var(--ink-2);}
.set-seg button.on{color:var(--ink);background:var(--panel-3);box-shadow:0 0 0 1px var(--hair-2);}
.set-seg button.on svg{color:var(--accent);}
.set-seg button svg{width:13px;height:13px;}
.set-seg button.dgr.on{color:var(--set-danger);
  background:color-mix(in oklch,var(--set-danger),transparent 88%);
  box-shadow:0 0 0 1px color-mix(in oklch,var(--set-danger),transparent 52%);}
.set-seg button.dgr.on svg{color:var(--set-danger);}

/* ── toggle ── */
.set-tgl{position:relative;width:34px;height:19px;border:none;border-radius:999px;
  background:var(--hair-2);cursor:pointer;padding:0;transition:background .16s;flex:none;}
.set-tgl.on{background:var(--accent);box-shadow:0 0 12px -3px rgba(var(--accent-rgb),.8);}
.set-tgl i{position:absolute;top:2px;left:2px;width:15px;height:15px;border-radius:50%;
  background:#fff;box-shadow:0 1px 3px rgba(0,0,0,.35);transition:transform .16s;}
.set-tgl.on i{transform:translateX(15px);}

/* ── stepper ── */
.set-step{display:inline-flex;align-items:center;background:var(--panel-2);border:1px solid var(--hair);
  border-radius:8px;overflow:hidden;height:28px;}
.set-step button{width:28px;height:28px;display:grid;place-items:center;background:transparent;border:none;
  color:var(--ink-3);cursor:pointer;transition:all .12s;}
.set-step button:hover:not(:disabled){background:var(--panel-3);color:var(--ink);}
.set-step button:disabled{opacity:.32;cursor:default;}
.set-step .val{min-width:38px;text-align:center;font-variant-numeric:tabular-nums;font-size:13px;
  color:var(--ink);border-left:1px solid var(--hair);border-right:1px solid var(--hair);height:100%;
  display:grid;place-items:center;}

/* ── styled select ── */
.set-sel{appearance:none;height:28px;padding:0 28px 0 11px;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:7px;color:var(--ink);font-family:var(--font-mono);
  font-size:12px;cursor:pointer;outline:none;transition:border-color .12s;
  background-image:linear-gradient(45deg,transparent 50%,var(--ink-3) 50%),linear-gradient(135deg,var(--ink-3) 50%,transparent 50%);
  background-position:right 13px center,right 8px center;background-size:5px 5px,5px 5px;background-repeat:no-repeat;}
.set-sel:hover{border-color:var(--hair-2);}
.set-sel:focus{border-color:var(--accent);}

/* ── model combobox ── */
.set-combo{position:relative;}
.set-combo-btn{display:inline-flex;align-items:center;gap:10px;height:28px;padding:0 9px 0 11px;
  min-width:186px;justify-content:space-between;background:var(--panel-2);border:1px solid var(--hair);
  border-radius:7px;color:var(--ink);font-family:var(--font-mono);font-size:12px;cursor:pointer;transition:all .12s;}
.set-combo-btn:hover{border-color:var(--hair-2);}
.set-combo-btn.open{border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in oklch,var(--accent),transparent 86%);}
.set-combo-btn .cv{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-combo-btn .tag{font-size:8px;letter-spacing:.1em;text-transform:uppercase;color:var(--accent);
  border:1px solid color-mix(in oklch,var(--accent),transparent 60%);border-radius:4px;padding:1px 4px;flex:none;}
.set-combo-btn svg{width:13px;height:13px;color:var(--ink-3);flex:none;transition:transform .15s;}
.set-combo-btn.open svg{transform:rotate(180deg);color:var(--accent);}
.set-combo-pop{position:absolute;top:calc(100% + 6px);right:0;z-index:6;width:248px;
  background:var(--elev);border:1px solid var(--hair-2);border-radius:11px;box-shadow:var(--shadow);
  padding:6px;animation:set-pop .14s ease;}
.set-combo-lbl{font-size:8.5px;letter-spacing:.13em;text-transform:uppercase;color:var(--ink-4);
  padding:4px 8px 5px;}
.set-combo-opt{display:flex;align-items:center;gap:9px;height:31px;padding:0 9px;border-radius:7px;
  color:var(--ink-2);font-size:12px;cursor:pointer;transition:all .1s;}
.set-combo-opt:hover{background:var(--panel-3);color:var(--ink);}
.set-combo-opt.on{color:var(--ink);background:var(--panel-2);}
.set-combo-opt .ck{width:14px;flex:none;color:var(--accent);}
.set-combo-opt .cn{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-combo-opt .meta{font-size:9.5px;color:var(--ink-4);flex:none;}
.set-combo-custom{margin-top:5px;border-top:1px solid var(--hair);padding-top:7px;}
.set-combo-field{display:flex;align-items:center;gap:7px;height:30px;padding:0 9px;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:7px;}
.set-combo-field:focus-within{border-color:var(--accent);}
.set-combo-field span{color:var(--ink-4);font-size:11px;flex:none;}
.set-combo-field input{flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);font-size:12px;}
.set-combo-field kbd{font-size:8.5px;color:var(--ink-4);border:1px solid var(--hair-2);border-radius:4px;
  padding:1px 5px;flex:none;}

/* ── tool select grid (agent default tool) ── */
.set-tools{display:grid;grid-template-columns:repeat(4,1fr);gap:8px;width:100%;}
.set-tool{position:relative;display:flex;flex-direction:column;align-items:flex-start;gap:9px;
  padding:11px 11px 10px;border-radius:11px;border:1px solid var(--hair);background:var(--panel-2);
  color:var(--ink-3);cursor:pointer;transition:all .13s;text-align:left;font-family:var(--font-mono);}
.set-tool:hover:not(.off){border-color:var(--hair-2);color:var(--ink-2);transform:translateY(-1px);}
.set-tool.on{color:var(--ink);border-color:color-mix(in oklch,var(--accent),transparent 42%);
  background:color-mix(in oklch,var(--accent),transparent 88%);
  box-shadow:0 0 18px -8px rgba(var(--accent-rgb),.7);}
.set-tool.on.warn{border-color:color-mix(in oklch,var(--set-amber),transparent 44%);
  background:color-mix(in oklch,var(--set-amber),transparent 90%);
  box-shadow:0 0 18px -9px color-mix(in oklch,var(--set-amber),transparent 30%);}
.set-tool.on.warn .pick{background:var(--set-amber);}
[data-theme="light"] .set-tool.on.warn .pick{color:#fff;}
.set-tool.off{opacity:.58;}
.set-tool.off:hover{opacity:.85;border-color:var(--hair-2);color:var(--ink-2);}
.set-tool .tn{font-size:11.5px;font-weight:500;}
.set-tool .ts{font-size:9px;color:var(--ink-4);display:flex;align-items:center;gap:4px;}
.set-tool .pick{position:absolute;top:9px;right:9px;width:15px;height:15px;border-radius:50%;
  display:grid;place-items:center;background:var(--accent);color:#06070b;}
[data-theme="light"] .set-tool .pick{color:#fff;}
.set-tool .nf{position:absolute;top:10px;right:10px;font-size:8px;letter-spacing:.08em;text-transform:uppercase;
  color:var(--ink-4);border:1px solid var(--hair);border-radius:4px;padding:1px 4px;}
.set-tool .nf.amber{color:var(--set-amber);border-color:color-mix(in oklch,var(--set-amber),transparent 55%);}

/* ── CLI runtimes list (per-tool detection + path setup) ── */
.set-rt-intro{font-size:10.5px;color:var(--ink-4);line-height:1.5;margin:-2px 0 11px;max-width:54ch;}
.set-rt-intro code{font-family:var(--font-mono);color:var(--ink-3);background:var(--panel-2);padding:0 4px;border-radius:4px;font-size:10px;}
.set-rt{display:flex;flex-direction:column;gap:10px;padding:11px 12px;border-radius:11px;
  border:1px solid var(--hair);background:var(--panel-2);transition:border-color .12s,background .12s;}
.set-rt + .set-rt{margin-top:8px;}
.set-rt.warn{border-color:color-mix(in oklch,var(--set-amber),transparent 62%);
  background:color-mix(in oklch,var(--set-amber),transparent 93%);}
.set-rt-top{display:flex;align-items:center;gap:10px;min-width:0;}
.set-rt-id{flex:none;display:flex;flex-direction:column;gap:2px;max-width:200px;}
.set-rt-name{font-size:12.5px;color:var(--ink);font-weight:500;line-height:1.1;}
.set-rt-st{font-size:9.5px;display:flex;align-items:center;gap:5px;color:var(--ink-4);letter-spacing:.01em;white-space:nowrap;}
.set-rt-st .rd{width:6px;height:6px;border-radius:50%;flex:none;}
.set-rt-st.ok{color:var(--st-done);} .set-rt-st.ok .rd{background:var(--st-done);box-shadow:0 0 7px -1px var(--st-done);}
.set-rt-st.err{color:var(--set-amber);} .set-rt-st.err .rd{background:var(--set-amber);box-shadow:0 0 7px -1px var(--set-amber);}
.set-rt-st.miss{color:var(--ink-4);} .set-rt-st.miss .rd{background:var(--ink-4);box-shadow:none;
  border:1px solid var(--hair-2);background:transparent;}
.set-rt-st .vt{color:var(--ink-4);}
.set-rt-spacer{flex:1;}
.set-rt-pathchip{flex:1;min-width:0;max-width:300px;display:flex;align-items:center;gap:7px;height:30px;
  padding:0 11px;background:var(--panel);border:1px solid var(--hair);border-radius:8px;color:var(--ink-2);font-size:11.5px;}
.set-rt-pathchip svg{width:13px;height:13px;color:var(--ink-4);flex:none;}
.set-rt-pathchip .pt{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-rt-link{background:none;border:none;color:var(--ink-4);font-family:var(--font-mono);font-size:10px;
  cursor:pointer;padding:0;display:inline-flex;align-items:center;gap:4px;transition:color .12s;flex:none;}
.set-rt-link:hover{color:var(--accent);}
.set-rt-link svg{width:11px;height:11px;}
.set-rt-edit{display:flex;flex-direction:column;gap:8px;padding-left:34px;}
.set-rt-reason{display:flex;align-items:flex-start;gap:7px;font-size:10px;color:var(--set-amber);line-height:1.45;}
.set-rt-reason svg{width:12px;height:12px;flex:none;margin-top:1px;}
.set-rt-reason code{font-family:var(--font-mono);background:color-mix(in oklch,var(--set-amber),transparent 86%);
  padding:0 4px;border-radius:4px;}
.set-rt-field{display:flex;align-items:center;gap:8px;}
.set-rt-input{flex:1;min-width:0;display:flex;align-items:center;gap:8px;height:32px;padding:0 11px;
  background:var(--panel);border:1px solid var(--hair);border-radius:8px;transition:border-color .12s,box-shadow .12s;}
.set-rt-input:focus-within{border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in oklch,var(--accent),transparent 88%);}
.set-rt-input svg{width:13px;height:13px;color:var(--ink-4);flex:none;}
.set-rt-input input{flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);font-size:12px;}
.set-rt-input input::placeholder{color:var(--ink-4);}
.set-rt-act{display:flex;align-items:center;gap:7px;flex:none;}
.set-rt-fail{display:flex;align-items:center;gap:7px;font-size:10px;color:var(--set-danger);}
.set-rt-fail svg{width:12px;height:12px;flex:none;}
.set-rt-cancel{background:none;border:none;color:var(--ink-4);font-family:var(--font-mono);font-size:10.5px;
  cursor:pointer;padding:0;transition:color .12s;}
.set-rt-cancel:hover{color:var(--ink-2);}

/* ── chips / amber / fields ── */
.set-warn{display:inline-flex;align-items:center;gap:8px;padding:7px 11px;border-radius:8px;font-size:10.5px;
  color:var(--set-amber);background:color-mix(in oklch,var(--set-amber),transparent 88%);
  border:1px solid color-mix(in oklch,var(--set-amber),transparent 60%);line-height:1.4;}
.set-warn svg{width:13px;height:13px;flex:none;}
.set-vchip{display:inline-flex;align-items:center;gap:6px;height:22px;padding:0 9px;border-radius:999px;
  border:1px solid var(--hair);background:var(--panel-2);color:var(--ink-2);font-size:10.5px;
  font-variant-numeric:tabular-nums;}
.set-vchip b{color:var(--ink);font-weight:600;}
.set-muted{font-size:11px;color:var(--ink-4);display:inline-flex;align-items:center;gap:7px;}

.set-text{display:flex;align-items:center;gap:8px;height:30px;padding:0 11px;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:8px;min-width:0;transition:border-color .12s;}
.set-text:focus-within{border-color:var(--accent);}
.set-text input{flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);font-size:12.5px;}
.set-text svg{width:13px;height:13px;color:var(--ink-4);flex:none;}
.set-preview{display:flex;align-items:center;gap:8px;font-size:11px;color:var(--ink-4);margin-top:2px;}
.set-preview b{color:var(--accent-2);font-weight:500;}
.set-preview .arr{color:var(--ink-4);}

.set-path{flex:1;min-width:0;display:flex;align-items:center;gap:8px;height:30px;padding:0 11px;
  background:var(--panel-2);border:1px solid var(--hair);border-radius:8px;color:var(--ink-2);
  font-size:12px;overflow:hidden;}
.set-path .pt{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-path svg{width:13px;height:13px;color:var(--ink-4);flex:none;}

/* ── update available card ── */
.set-upd{display:flex;flex-direction:column;gap:13px;padding:15px;border-radius:12px;width:100%;
  background:color-mix(in oklch,var(--accent),transparent 92%);
  border:1px solid color-mix(in oklch,var(--accent),transparent 70%);}
.set-upd-top{display:flex;align-items:center;gap:11px;}
.set-upd-ic{width:32px;height:32px;border-radius:9px;flex:none;display:grid;place-items:center;color:var(--accent);
  background:color-mix(in oklch,var(--accent),transparent 84%);
  box-shadow:inset 0 0 0 1px color-mix(in oklch,var(--accent),transparent 58%);}
.set-upd-tt{flex:1;min-width:0;}
.set-upd-tt .u1{font-size:12.5px;color:var(--ink);font-weight:500;display:flex;align-items:center;gap:8px;}
.set-upd-tt .u2{font-size:10px;color:var(--ink-3);margin-top:3px;font-variant-numeric:tabular-nums;}
.set-upd-ver{font-family:var(--font-disp);font-size:15px;font-weight:600;color:var(--accent);letter-spacing:-.01em;}
.set-upd-notes{display:inline-flex;align-items:center;gap:5px;font-size:10.5px;color:var(--accent-2);
  text-decoration:none;border-bottom:1px solid color-mix(in oklch,var(--accent-2),transparent 70%);
  padding-bottom:1px;align-self:flex-start;}
.set-upd-notes:hover{border-color:var(--accent-2);}
.set-upd-notes svg{width:11px;height:11px;}
.set-upd-act{display:flex;gap:8px;}

/* ── allowlist editor ── */
.set-allow{display:flex;flex-wrap:wrap;gap:7px;padding:11px;width:100%;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:10px;}
.set-allow-chip{display:inline-flex;align-items:center;gap:4px;height:25px;padding:0 4px 0 9px;
  border-radius:7px;background:var(--panel-3);border:1px solid var(--hair-2);font-size:11px;color:var(--ink-2);}
.set-allow-chip code{font-family:var(--font-mono);}
.set-allow-chip button{width:16px;height:16px;border-radius:4px;border:none;background:transparent;
  color:var(--ink-4);cursor:pointer;display:grid;place-items:center;transition:all .1s;}
.set-allow-chip button:hover{background:color-mix(in oklch,var(--set-danger),transparent 82%);color:var(--set-danger);}
.set-allow-chip button svg{width:11px;height:11px;}
.set-allow-add{display:inline-flex;align-items:center;gap:6px;height:25px;padding:0 10px;border-radius:7px;
  border:1px dashed var(--hair-2);background:transparent;}
.set-allow-add:focus-within{border-color:var(--accent);border-style:solid;}
.set-allow-add span{color:var(--ink-4);}
.set-allow-add input{width:108px;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);font-size:11px;}
.set-allow-add input::placeholder{color:var(--ink-4);}

/* ── danger confirm ── */
.set-danger{margin-top:12px;width:100%;padding:14px;border-radius:11px;display:flex;flex-direction:column;gap:11px;
  border:1px solid var(--set-danger);background:color-mix(in oklch,var(--set-danger),transparent 90%);
  box-shadow:0 0 0 3px color-mix(in oklch,var(--set-danger),transparent 88%);animation:set-pop .16s ease;}
.set-danger-h{display:flex;align-items:center;gap:9px;color:var(--set-danger);font-size:12.5px;font-weight:600;}
.set-danger-h svg{width:15px;height:15px;flex:none;}
.set-danger-b{font-size:11px;color:var(--ink-2);line-height:1.55;}
.set-danger-b b{color:var(--ink);font-weight:600;}
.set-danger-act{display:flex;gap:8px;justify-content:flex-end;}
.set-btn-danger{display:inline-flex;align-items:center;gap:6px;height:30px;padding:0 13px;border-radius:7px;
  border:none;background:var(--set-danger);color:#fff;font-family:var(--font-mono);font-size:11.5px;
  font-weight:600;cursor:pointer;transition:filter .12s;}
.set-btn-danger:hover{filter:brightness(1.08);}
.set-btn-danger svg{width:13px;height:13px;}

/* ── slider (volume) ── */
.set-slider{appearance:none;-webkit-appearance:none;width:128px;height:4px;border-radius:999px;
  background:var(--hair-2);outline:none;cursor:pointer;}
.set-slider::-webkit-slider-thumb{-webkit-appearance:none;appearance:none;width:14px;height:14px;border-radius:50%;
  background:var(--accent);border:2px solid var(--panel);box-shadow:0 0 0 1px var(--hair-2);cursor:pointer;}
.set-slider::-moz-range-thumb{width:14px;height:14px;border-radius:50%;background:var(--accent);
  border:2px solid var(--panel);box-shadow:0 0 0 1px var(--hair-2);cursor:pointer;}

/* ── footer ── */
.set-foot{flex:none;display:flex;align-items:center;gap:10px;padding:11px 16px;border-top:1px solid var(--hair);
  background:var(--panel-2);font-size:10.5px;color:var(--ink-3);}
.set-foot .fl{display:inline-flex;align-items:center;gap:7px;}
.set-foot .fd{width:6px;height:6px;border-radius:50%;background:var(--st-done);flex:none;}
.set-foot .reset-all{display:inline-flex;align-items:center;gap:5px;background:transparent;border:none;
  color:var(--ink-3);font-family:var(--font-mono);font-size:10.5px;cursor:pointer;padding:0;}
.set-foot .reset-all:hover{color:var(--accent);}
.set-foot .reset-all svg{width:11px;height:11px;}
.set-foot .sp{flex:1;}
.set-kbd{font-size:9px;padding:1px 5px;border-radius:4px;border:1px solid var(--hair-2);color:var(--ink-3);
  background:var(--panel);}
.set-done{display:inline-flex;align-items:center;gap:6px;height:28px;padding:0 14px;border-radius:7px;
  border:none;font-family:var(--font-mono);font-size:11.5px;font-weight:600;cursor:pointer;color:#06070b;
  background:linear-gradient(180deg,var(--accent),color-mix(in oklch,var(--accent),#000 14%));
  box-shadow:0 0 18px -7px rgba(var(--accent-rgb),.7);}
[data-theme="light"] .set-done{color:#fff;}
.set-done:hover{filter:brightness(1.07);}
`;

// ── primitives ──────────────────────────────────────────────────────────────
function Seg({ value, options, onChange, danger }) {
  return (
    <div className="set-seg" role="radiogroup">
      {options.map((o) => {
        const v = typeof o === "object" ? o.value : o;
        const l = typeof o === "object" ? o.label : o;
        const dgr = danger && v === danger;
        return (
          <button key={v} role="radio" aria-checked={v === value}
            className={(v === value ? "on " : "") + (dgr ? "dgr" : "")}
            onClick={() => onChange(v)}>
            {dgr && <Icon name="flag" size="sm" />}{l}
          </button>
        );
      })}
    </div>
  );
}

function Toggle({ value, onChange, disabled }) {
  return (
    <button type="button" className={"set-tgl" + (value ? " on" : "")} role="switch"
      aria-checked={!!value} disabled={disabled} onClick={() => onChange(!value)}><i /></button>
  );
}

function Stepper({ value, min = 1, max = 10, onChange }) {
  return (
    <div className="set-step">
      <button disabled={value <= min} onClick={() => onChange(Math.max(min, value - 1))} aria-label="Decrease">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round"><path d="M6 12h12" /></svg>
      </button>
      <span className="val tnum">{value}</span>
      <button disabled={value >= max} onClick={() => onChange(Math.min(max, value + 1))} aria-label="Increase">
        <Icon name="plus" size="sm" />
      </button>
    </div>
  );
}

function SetSelect({ value, options, onChange, width }) {
  return (
    <select className="set-sel" value={value} onChange={(e) => onChange(e.target.value)} style={width ? { width } : null}>
      {options.map((o) => {
        const v = typeof o === "object" ? o.value : o;
        const l = typeof o === "object" ? o.label : o;
        return <option key={v} value={v}>{l}</option>;
      })}
    </select>
  );
}

// Model combobox — curated list + free-text custom override.
function ModelCombo({ value, options, open, setOpen, onChange }) {
  const [custom, setCustom] = useSS("");
  const ref = useSR(null);
  useSE(() => {
    if (!open) return;
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);
  const isCustom = !options.includes(value);
  const commit = () => { const v = custom.trim(); if (v) { onChange(v); setCustom(""); setOpen(false); } };
  return (
    <div className="set-combo" ref={ref}>
      <button className={"set-combo-btn" + (open ? " open" : "")} onClick={() => setOpen(!open)}>
        <span className="cv">{value}</span>
        <span style={{ display: "flex", alignItems: "center", gap: 8, flex: "none" }}>
          {isCustom && <span className="tag">custom</span>}
          <Icon name="chevronD" size="sm" />
        </span>
      </button>
      {open && (
        <div className="set-combo-pop">
          <div className="set-combo-lbl">Curated models</div>
          {options.map((m) => (
            <div key={m} className={"set-combo-opt" + (m === value ? " on" : "")}
              onClick={() => { onChange(m); setOpen(false); }}>
              <span className="ck">{m === value && <Icon name="check" size="sm" />}</span>
              <span className="cn">{m}</span>
            </div>
          ))}
          <div className="set-combo-custom">
            <div className="set-combo-lbl" style={{ paddingTop: 0 }}>Custom — CLIs don’t expose a list</div>
            <div className="set-combo-field">
              <span>›</span>
              <input value={custom} placeholder={isCustom ? value : "model-id…"}
                onChange={(e) => setCustom(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") commit(); }} />
              <kbd>↵</kbd>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function ResetBtn({ onClick }) {
  return (
    <button className="set-reset" onClick={onClick} title="Reset to default">
      <Icon name="refresh" size="sm" />reset
    </button>
  );
}

function SetRow({ label, help, dirty, onReset, wide, disabled, children }) {
  return (
    <div className={"set-row" + (wide ? " wide" : "") + (disabled ? " dis" : "")}>
      <div className="set-row-main">
        <div className="set-row-lbl">{label}{dirty && !disabled && <ResetBtn onClick={onReset} />}</div>
        {help && <div className="set-row-help">{help}</div>}
        {wide && <div className="set-row-ctrl">{children}</div>}
      </div>
      {!wide && <div className="set-row-ctrl">{children}</div>}
    </div>
  );
}

function SetGroup({ title, children }) {
  return (
    <div className="set-grp">
      <div className="set-grp-h">{title}<span className="ln" /></div>
      {children}
    </div>
  );
}

// ── sections ─────────────────────────────────────────────────────────────────
function UpdatesSection({ s, set, resetTo, D, onOpenLog, onShowWhatsNew, onInstallUpdate }) {
  const [checking, setChecking] = useSS(false);
  const [lastChecked, setLastChecked] = useSS("2h ago");
  const check = () => { setChecking(true); setTimeout(() => { setChecking(false); setLastChecked("just now"); }, 1300); };
  return (
    <>
      <SetGroup title="Release channel">
        <SetRow label="Channel" help="Beta receives pre-release builds first."
          dirty={s.channel !== D.channel} onReset={() => resetTo({ channel: D.channel })}>
          <Seg value={s.channel} options={["stable", "beta"]} onChange={(v) => set({ channel: v })} />
        </SetRow>
        {s.channel === "beta" && (
          <div className="set-warn" style={{ marginTop: 2 }}>
            <Icon name="flag" size="sm" />
            Pre-release builds — may be unstable or break worktrees. Roll back from this panel anytime.
          </div>
        )}
        <SetRow label="Install policy" help="What Orrery does when a new build is available."
          dirty={s.updatePolicy !== D.updatePolicy} onReset={() => resetTo({ updatePolicy: D.updatePolicy })}>
          <Seg value={s.updatePolicy}
            options={[{ value: "auto", label: "Auto-install" }, { value: "notify", label: "Notify only" }, { value: "manual", label: "Manual" }]}
            onChange={(v) => set({ updatePolicy: v })} />
        </SetRow>
      </SetGroup>

      <SetGroup title="Version">
        <SetRow label="Current build"
          help={<>Orrery <code>v{SETTINGS_UPDATE.current}</code> · {s.channel === "beta" ? "beta" : "stable"} channel · checked {checking ? "now…" : lastChecked}</>}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span className="set-vchip tnum">v{SETTINGS_UPDATE.current}<b>·</b>{s.channel === "beta" ? "BETA" : "STABLE"}</span>
            <button className="btn ghost-hair" onClick={check} disabled={checking} style={{ padding: "5px 11px" }}>
              <Icon name="refresh" size="sm" style={checking ? { animation: "spin 0.9s linear infinite" } : null} />
              {checking ? "Checking…" : "Check now"}
            </button>
          </div>
        </SetRow>
        <SetRow wide label="Update available" help="A newer build is ready to install.">
          <div className="set-upd">
            <div className="set-upd-top">
              <span className="set-upd-ic"><Icon name="stage" /></span>
              <div className="set-upd-tt">
                <div className="u1">Orrery <span className="set-upd-ver">v{SETTINGS_UPDATE.version}</span><span className="set-vchip tnum" style={{ height: 19 }}>{SETTINGS_UPDATE.size}</span></div>
                <div className="u2">released {SETTINGS_UPDATE.date} · upgrades from v{SETTINGS_UPDATE.current}</div>
              </div>
            </div>
            <a className="set-upd-notes" href="#" onClick={(e) => { e.preventDefault(); onShowWhatsNew && onShowWhatsNew(); }}>
              <Icon name="file" size="sm" />Read release notes<Icon name="ext" size="sm" />
            </a>
            <div className="set-upd-act">
              <button className="btn primary" onClick={() => onInstallUpdate && onInstallUpdate()}><Icon name="stage" size="sm" />Install &amp; relaunch</button>
              <button className="btn ghost-hair">Later</button>
            </div>
          </div>
        </SetRow>
      </SetGroup>

      <SetGroup title="Diagnostics">
        <SetRow label="Log file" help={<>Orrery appends a rolling diagnostics log — orchestrator, updater, git and IPC events. Open it when something needs a closer look.</>}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, width: 320 }}>
            <div className="set-path" title={SETTINGS_LOGFILE}><Icon name="file" size="sm" /><span className="pt">{SETTINGS_LOGFILE}</span></div>
            <button className="btn ghost-hair" style={{ flex: "none", padding: "5px 11px" }} onClick={() => onOpenLog && onOpenLog()}><Icon name="ext" size="sm" />Open</button>
          </div>
        </SetRow>
      </SetGroup>
    </>
  );
}

// One row in the CLI-runtimes list: shows the resolved executable path for a
// detected tool, or an inline "locate the binary" editor when it can't run.
function RuntimeRow({ id, name, entry, orig, onResolve, onRevert }) {
  const ok = entry.status === "ok";
  const [editing, setEditing] = useSS(!ok);
  const [draft, setDraft] = useSS(entry.path || "");
  const [checking, setChecking] = useSS(false);
  const [fail, setFail] = useSS(null);

  const begin = () => { setDraft(entry.path || ""); setFail(null); setEditing(true); };
  const cancel = () => { setEditing(false); setFail(null); };

  const verify = () => {
    const v = draft.trim();
    if (!v) { setFail("Enter the full path to the executable."); return; }
    setFail(null); setChecking(true);
    setTimeout(() => {
      setChecking(false);
      // mock probe: re-pointing at the same known-bad binary still fails
      if (entry.status === "error" && v === entry.path) {
        setFail("Still exits 126 — point Orrery at a different binary or fix its permissions.");
        return;
      }
      onResolve(v);
      setEditing(false);
    }, 850);
  };

  const stCls = ok ? "ok" : entry.status === "error" ? "err" : "miss";
  const stLabel = ok
    ? <>detected<span className="vt">·</span>{entry.source === "manual" ? "manual path" : "on PATH"}{entry.version && <><span className="vt">·</span>v{entry.version}</>}</>
    : entry.status === "error" ? "found, can’t run" : "not installed";

  return (
    <div className={"set-rt" + (entry.status === "error" ? " warn" : "")}>
      <div className="set-rt-top">
        <ToolBadge tool={id} size={22} />
        <div className="set-rt-id">
          <div className="set-rt-name">{name}</div>
          <div className={"set-rt-st " + stCls}><span className="rd" />{stLabel}</div>
        </div>
        {ok && !editing ? (
          <>
            <div className="set-rt-pathchip" title={entry.path}><Icon name="terminal" size="sm" /><span className="pt">{entry.path}</span></div>
            <button className="set-rt-link" onClick={begin}><Icon name="rename" size="sm" />Change</button>
            {entry.source === "manual" && orig.status === "ok" &&
              <button className="set-rt-link" onClick={onRevert} title="Re-run auto-detection">revert</button>}
          </>
        ) : (
          <>
            <span className="set-rt-spacer" />
            {!editing && <button className="btn ghost-hair" style={{ padding: "5px 12px" }} onClick={begin}><Icon name="search" size="sm" />Locate binary</button>}
          </>
        )}
      </div>

      {editing && (
        <div className="set-rt-edit">
          {entry.status === "error" && entry.reason && (
            <div className="set-rt-reason"><Icon name="flag" size="sm" /><span>{entry.reason} Last tried <code>{entry.path}</code>.</span></div>
          )}
          <div className="set-rt-field">
            <div className="set-rt-input">
              <Icon name="terminal" size="sm" />
              <input value={draft} placeholder={"/path/to/" + id} spellCheck={false} autoFocus
                onChange={(e) => { setDraft(e.target.value); setFail(null); }}
                onKeyDown={(e) => { if (e.key === "Enter") verify(); if (e.key === "Escape" && ok) cancel(); }} />
            </div>
            <div className="set-rt-act">
              <button className="btn ghost-hair" style={{ padding: "6px 11px" }} onClick={() => setDraft("/usr/local/bin/" + id)}><Icon name="folderOpen" size="sm" />Browse</button>
              <button className="btn primary" style={{ padding: "6px 13px" }} disabled={checking} onClick={verify}>
                <Icon name={checking ? "refresh" : "check"} size="sm" style={checking ? { animation: "spin 0.9s linear infinite" } : null} />
                {checking ? "Verifying…" : ok ? "Save" : "Use this path"}
              </button>
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            {fail
              ? <span className="set-rt-fail"><Icon name="x" size="sm" />{fail}</span>
              : <span style={{ fontSize: 10, color: "var(--ink-4)" }}>Orrery runs <code style={{ fontFamily: "var(--font-mono)" }}>{id} --version</code> to confirm it launches.</span>}
            {ok && <button className="set-rt-cancel" onClick={cancel}>Cancel</button>}
          </div>
        </div>
      )}
    </div>
  );
}

function AgentSection({ s, set, setMap, resetTo, resetMap, D, modelOpen, setModelOpen, det, setDet }) {
  const TOOLS = window.AGENT_TOOLS || [];
  const tool = s.defaultTool;
  const meta = TOOLS.find((t) => t.id === tool) || TOOLS[0];
  const preview = s.branchTemplate.replace(/\{name\}/g, "fix-login").replace(/\{tool\}/g, tool).replace(/\{date\}/g, "0610");

  const resolvePath = (id, path) => setDet((prev) => ({ ...prev,
    [id]: { ...prev[id], status: "ok", path, source: "manual", reason: undefined } }));
  const revertPath = (id) => setDet((prev) => ({ ...prev, [id]: sClone(SETTINGS_DETECTED[id]) }));

  const sel = det[tool] || { status: "missing" };

  return (
    <>
      <SetGroup title="Default agent">
        <SetRow wide label="Default tool"
          help="Used when you spawn without picking one. Pick an agent to configure its model, effort and executable path below."
          dirty={s.defaultTool !== D.defaultTool} onReset={() => resetTo({ defaultTool: D.defaultTool })}>
          <div className="set-tools">
            {TOOLS.map((tl) => {
              const e = det[tl.id] || { status: "missing" };
              const runnable = e.status === "ok";
              const on = tool === tl.id;
              return (
                <button key={tl.id} className={"set-tool" + (on ? " on" : "") + (on && !runnable ? " warn" : "") + (!runnable && !on ? " off" : "")}
                  onClick={() => set({ defaultTool: tl.id })}
                  title={runnable ? e.path : e.status === "error" ? "Found but can’t run — set its path below" : "Not installed — locate it below"}>
                  <ToolBadge tool={tl.id} size={22} />
                  <div className="tn">{tl.name}</div>
                  <div className="ts">
                    {runnable
                      ? <><span className="dot done" style={{ width: 5, height: 5 }} />detected{e.version ? " · v" + e.version : ""}</>
                      : e.status === "error" ? <span style={{ color: "var(--set-amber)" }}>can’t run</span> : "not installed"}
                  </div>
                  {on && <span className="pick"><Icon name="check" size="sm" style={{ width: 11, height: 11 }} /></span>}
                  {!runnable && !on && <span className={"nf" + (e.status === "error" ? " amber" : "")}>{e.status === "error" ? "needs path" : "not found"}</span>}
                </button>
              );
            })}
          </div>
        </SetRow>

        <SetRow wide label={<>Executable <span style={{ color: "var(--ink-4)", fontWeight: 400 }}>· {meta.name}</span></>}
          help={<>Where Orrery launches <b style={{ color: "var(--ink-3)", fontWeight: 500 }}>{meta.name}</b> from — detected on your <code>PATH</code> at startup. Override it if the binary lives elsewhere or couldn’t run.</>}>
          <RuntimeRow key={tool} id={tool} name={meta.name} entry={sel}
            orig={SETTINGS_DETECTED[tool] || { status: "missing" }}
            onResolve={(p) => resolvePath(tool, p)} onRevert={() => revertPath(tool)} />
        </SetRow>

        <SetRow label={<>Model <span style={{ color: "var(--ink-4)", fontWeight: 400 }}>· {meta.name}</span></>}
          help="Curated per tool, with a free-text override — Enter to apply a custom id."
          dirty={s.toolModel[tool] !== D.toolModel[tool]} onReset={() => resetMap("toolModel", tool, D.toolModel[tool])}>
          <ModelCombo value={s.toolModel[tool]} options={meta.models} open={modelOpen} setOpen={setModelOpen}
            onChange={(v) => setMap("toolModel", tool, v)} />
        </SetRow>

        <SetRow label="Reasoning effort"
          help={meta.effort ? "How hard the model thinks before acting." : <>{meta.name} doesn’t expose an effort setting.</>}
          dirty={meta.effort && s.toolEffort[tool] !== D.toolEffort[tool]}
          onReset={() => resetMap("toolEffort", tool, D.toolEffort[tool])}>
          {meta.effort
            ? <Seg value={s.toolEffort[tool] || "medium"} options={meta.effort} onChange={(v) => setMap("toolEffort", tool, v)} />
            : <span className="set-muted"><Icon name="dots" size="sm" />not supported</span>}
        </SetRow>
      </SetGroup>

      <SetGroup title="Worktrees">
        <SetRow wide label="Branch template"
          help={<>Tokens: <code>{"{name}"}</code> <code>{"{tool}"}</code> <code>{"{date}"}</code></>}
          dirty={s.branchTemplate !== D.branchTemplate} onReset={() => resetTo({ branchTemplate: D.branchTemplate })}>
          <div style={{ display: "flex", flexDirection: "column", gap: 0, width: "100%" }}>
            <div className="set-text" style={{ maxWidth: 320 }}>
              <Icon name="branch" size="sm" />
              <input value={s.branchTemplate} onChange={(e) => set({ branchTemplate: e.target.value })} spellCheck={false} />
            </div>
            <div className="set-preview"><span className="arr">preview</span><Icon name="chevron" size="sm" style={{ width: 11, height: 11 }} /><b>{preview || "—"}</b></div>
          </div>
        </SetRow>
        <SetRow label="Worktree root" help="Where new agent worktrees are created on disk."
          dirty={s.worktreeRoot !== D.worktreeRoot} onReset={() => resetTo({ worktreeRoot: D.worktreeRoot })}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, width: 300 }}>
            <div className="set-path"><Icon name="folder" size="sm" /><span className="pt">{s.worktreeRoot}</span></div>
            <button className="btn ghost-hair" style={{ flex: "none", padding: "5px 11px" }}><Icon name="folderOpen" size="sm" />Browse</button>
          </div>
        </SetRow>
        <SetRow label="Auto-resume on restart" help="Re-attach to running agent sessions when Orrery relaunches."
          dirty={s.autoResume !== D.autoResume} onReset={() => resetTo({ autoResume: D.autoResume })}>
          <Toggle value={s.autoResume} onChange={(v) => set({ autoResume: v })} />
        </SetRow>
      </SetGroup>
    </>
  );
}

function PermissionsSection({ s, set, setMap, resetTo, resetMap, D, confirm, setConfirm, det }) {
  const TOOLS = window.AGENT_TOOLS || [];
  const detected = TOOLS.filter((t) => isRunnable(det, t.id));

  const POLICY = [
    { value: "off", label: "Off" },
    { value: "allowlist", label: "Allowlist only" },
    { value: "everything", label: "Everything" },
  ];
  const pickPolicy = (tid, v) => {
    if (v === "everything" && s.autoApprove[tid] !== "everything") {
      setConfirm({ tool: tid, prev: s.autoApprove[tid] });
      setMap("autoApprove", tid, "everything");
    } else {
      if (confirm && confirm.tool === tid) setConfirm(null);
      setMap("autoApprove", tid, v);
    }
  };

  return (
    <>
      <SetGroup title="Auto-approve policy">
        {detected.map((tl) => {
          const val = s.autoApprove[tl.id];
          const dirty = val !== D.autoApprove[tl.id];
          const showConfirm = confirm && confirm.tool === tl.id;
          return (
            <React.Fragment key={tl.id}>
              <SetRow
                label={<span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}><ToolBadge tool={tl.id} size={16} />{tl.name}</span>}
                help={val === "off" ? "Ask before every command." : val === "allowlist" ? "Auto-run allowlisted commands; ask for the rest." : "Run any command without asking."}
                dirty={dirty} onReset={() => { if (showConfirm) setConfirm(null); resetMap("autoApprove", tl.id, D.autoApprove[tl.id]); }}>
                <Seg value={val} options={POLICY} danger="everything" onChange={(v) => pickPolicy(tl.id, v)} />
              </SetRow>
              {showConfirm && (
                <div className="set-danger">
                  <div className="set-danger-h"><Icon name="flag" />Allow {tl.name} to run <b style={{ color: "inherit" }}>everything</b>?</div>
                  <div className="set-danger-b">
                    Auto-approving <b>every</b> command lets {tl.name} run destructive operations — <code style={{ fontFamily: "var(--font-mono)" }}>rm&nbsp;-rf</code>, force-push, network calls — with no prompt. Recommended only for fully sandboxed worktrees.
                  </div>
                  <div className="set-danger-act">
                    <button className="btn ghost-hair" onClick={() => { setMap("autoApprove", tl.id, confirm.prev); setConfirm(null); }}>Cancel</button>
                    <button className="set-btn-danger" onClick={() => setConfirm(null)}><Icon name="flag" size="sm" />Enable “Everything”</button>
                  </div>
                </div>
              )}
            </React.Fragment>
          );
        })}
      </SetGroup>

      <SetGroup title="Remote approval">
        <SetRow label="Approve from notifications" help="Answer permission prompts straight from OS notifications."
          dirty={s.remoteApproval !== D.remoteApproval} onReset={() => resetTo({ remoteApproval: D.remoteApproval })}>
          <Toggle value={s.remoteApproval} onChange={(v) => set({ remoteApproval: v })} />
        </SetRow>
      </SetGroup>
    </>
  );
}

function EditorSection({ s, set, resetTo, D, onOpenKeymap }) {
  return (
    <>
      <SetGroup title="Keymap">
        <SetRow label="Preset" help="IntelliJ is the default — ⇧⇧ Search Everywhere, ⌘⇧A actions, ⌘1 / ⌘2 views, ⌘[ / ⌘] navigation."
          dirty={s.keymap !== D.keymap} onReset={() => resetTo({ keymap: D.keymap })}>
          <Seg value={s.keymap} options={[{ value: "intellij", label: "IntelliJ IDEA" }, { value: "vscode", label: "VS Code" }]} onChange={(v) => set({ keymap: v })} />
        </SetRow>
        <SetRow label="Shortcut reference" help="Every action in the app is registered once; menus render from the same registry.">
          <button className="btn ghost-hair" onClick={onOpenKeymap} style={{ padding: "5px 11px" }}><Icon name="lock" size="sm" />Open keymap…</button>
        </SetRow>
      </SetGroup>

      <SetGroup title="Saving">
        <SetRow label="Autosave" help="Writes through the worktree after you stop typing. Off means ⌘S."
          dirty={s.autosave !== D.autosave} onReset={() => resetTo({ autosave: D.autosave })}>
          <Toggle value={s.autosave} onChange={(v) => set({ autosave: v })} />
        </SetRow>
        <SetRow label="Autosave delay" help="Seconds of idle typing before the buffer is written." disabled={!s.autosave}
          dirty={s.autosaveDelay !== D.autosaveDelay} onReset={() => resetTo({ autosaveDelay: D.autosaveDelay })}>
          <Stepper value={s.autosaveDelay} min={1} max={30} onChange={(v) => set({ autosaveDelay: v })} />
        </SetRow>
      </SetGroup>

      <SetGroup title="Gutter & structure">
        <SetRow label="Structure view" help="Symbol outline beside the editor, from the tree-sitter index."
          dirty={s.structureView !== D.structureView} onReset={() => resetTo({ structureView: D.structureView })}>
          <Toggle value={s.structureView} onChange={(v) => set({ structureView: v })} />
        </SetRow>
        <SetRow label="Change markers" help="Colored bars in the gutter, click to revert a hunk — IntelliJ-style."
          dirty={s.gutterMarkers !== D.gutterMarkers} onReset={() => resetTo({ gutterMarkers: D.gutterMarkers })}>
          <Toggle value={s.gutterMarkers} onChange={(v) => set({ gutterMarkers: v })} />
        </SetRow>
      </SetGroup>
    </>
  );
}

function MemorySection({ s, set, resetTo, D, onSleepIdle, agents }) {
  const alive = (agents || []).filter((a) => a.status !== "done").length;
  return (
    <>
      <SetGroup title="Sleep policy">
        <SetRow label="Auto-sleep" help="A slept agent disposes its xterm and WebGL context; the Rust scrollback ring keeps its output, so waking replays it."
          dirty={s.autoSleep !== D.autoSleep} onReset={() => resetTo({ autoSleep: D.autoSleep })}>
          <Toggle value={s.autoSleep} onChange={(v) => set({ autoSleep: v })} />
        </SetRow>
        <SetRow label="Sleep exited agents after" help="Minutes after the process exits." disabled={!s.autoSleep}
          dirty={s.sleepExitedAfter !== D.sleepExitedAfter} onReset={() => resetTo({ sleepExitedAfter: D.sleepExitedAfter })}>
          <Stepper value={s.sleepExitedAfter} min={1} max={120} onChange={(v) => set({ sleepExitedAfter: v })} />
        </SetRow>
        <SetRow label="Sleep idle agents after" help="Minutes with no output and no visible pane." disabled={!s.autoSleep}
          dirty={s.sleepIdleAfter !== D.sleepIdleAfter} onReset={() => resetTo({ sleepIdleAfter: D.sleepIdleAfter })}>
          <Stepper value={s.sleepIdleAfter} min={5} max={240} onChange={(v) => set({ sleepIdleAfter: v })} />
        </SetRow>
        <SetRow label="Sleep idle agents now" help={alive + " agents are alive in this session."}>
          <button className="btn ghost-hair" onClick={onSleepIdle} style={{ padding: "5px 11px" }}><Icon name="moon" size="sm" />Sleep idle now</button>
        </SetRow>
      </SetGroup>

      <SetGroup title="Renderer">
        <SetRow label="WebGL for visible terminals only" help="Browsers cap live contexts around 8–16; hidden panes fall back to the DOM renderer and repaint on show."
          dirty={s.webglVisibleOnly !== D.webglVisibleOnly} onReset={() => resetTo({ webglVisibleOnly: D.webglVisibleOnly })}>
          <Toggle value={s.webglVisibleOnly} onChange={(v) => set({ webglVisibleOnly: v })} />
        </SetRow>
        <SetRow label="Scrollback ring per agent" help="Rust-owned raw byte ring. Recovery source for reloads, sleep and the hidden-output cap."
          dirty={s.ringSize !== D.ringSize} onReset={() => resetTo({ ringSize: D.ringSize })}>
          <SetSelect value={s.ringSize} options={["512 KB", "1 MB", "2 MB", "4 MB"]} onChange={(v) => set({ ringSize: v })} width={120} />
        </SetRow>
      </SetGroup>
    </>
  );
}

function ProjectSection({ s, set, D, projects }) {
  const list = projects && projects.length ? projects : (window.PROJECTS || []);
  const [pid, setPid] = useSS(list[0] ? list[0].id : "");
  const proj = list.find((p) => p.id === pid) || list[0];
  const ov = (s.perProject || {})[pid] || {};
  const setOv = (patch) => set({ perProject: { ...(s.perProject || {}), [pid]: { ...ov, ...patch } } });
  const clear = () => { const nx = { ...(s.perProject || {}) }; delete nx[pid]; set({ perProject: nx }); };
  const inherited = (k, fallback) => ov[k] === undefined ? fallback : ov[k];
  const tool = s.defaultTool;
  const models = ((window.AGENT_TOOLS || []).find((t) => t.id === tool) || { models: [] }).models;
  if (!proj) return <SetGroup title="Projects"><SetRow label="No projects" help="Add a project first." /></SetGroup>;
  return (
    <>
      <SetGroup title="Project">
        <SetRow label="Overrides for" help={proj.path}>
          <SetSelect value={pid} options={list.map((p) => ({ value: p.id, label: p.name }))} onChange={setPid} width={200} />
        </SetRow>
      </SetGroup>

      <SetGroup title={"Overrides · " + proj.name}>
        <SetRow label="Model" help={"Inherits the global default (" + s.toolModel[tool] + ") unless set here."}
          dirty={ov.model !== undefined} onReset={() => setOv({ model: undefined })}>
          <SetSelect value={inherited("model", s.toolModel[tool])} options={models.length ? models : [s.toolModel[tool]]} onChange={(v) => setOv({ model: v })} width={170} />
        </SetRow>
        <SetRow label="Auto-approve policy" help="Per-project override of the global permission policy."
          dirty={ov.autoApprove !== undefined} onReset={() => setOv({ autoApprove: undefined })}>
          <Seg value={inherited("autoApprove", s.autoApprove[tool] || "off")}
            options={[{ value: "off", label: "Off" }, { value: "allowlist", label: "Allowlist" }, { value: "everything", label: "Everything" }]}
            onChange={(v) => setOv({ autoApprove: v })} danger={inherited("autoApprove", "off") === "everything"} />
        </SetRow>
        <SetRow label="Budget cap" help="AI git variants disable at the cap; native git stays fully usable."
          dirty={ov.budgetUsd !== undefined} onReset={() => setOv({ budgetUsd: undefined })}>
          <Seg value={String(inherited("budgetUsd", 25))} options={["10", "25", "50", "100"].map((v) => ({ value: v, label: "$" + v }))}
            onChange={(v) => { setOv({ budgetUsd: Number(v) }); if (window.COST_BUDGET) window.COST_BUDGET.capUsd = Number(v); }} />
        </SetRow>
        <SetRow label="Autosave" help="Override the global autosave behavior for files in this project."
          dirty={ov.autosave !== undefined} onReset={() => setOv({ autosave: undefined })}>
          <Toggle value={!!inherited("autosave", s.autosave)} onChange={(v) => setOv({ autosave: v })} />
        </SetRow>
        <SetRow label="Language server" help="Opt-in per project. A rust-analyzer or tsserver instance dwarfs Orrery itself — its RSS is shown in the status bar and shutdown is aggressive."
          dirty={ov.lsp !== undefined} onReset={() => setOv({ lsp: undefined })}>
          <Toggle value={!!inherited("lsp", false)} onChange={(v) => setOv({ lsp: v })} />
        </SetRow>
        <SetRow label="Reset project" help="Drop every override and inherit the globals again.">
          <button className="btn ghost-hair" onClick={clear} style={{ padding: "5px 11px" }}><Icon name="refresh" size="sm" />Clear overrides</button>
        </SetRow>
      </SetGroup>
    </>
  );
}

function NotificationsSection({ s, set, setMap, resetTo, resetMap, D }) {
  const off = !s.osNotifications;
  const EVENTS = [
    { k: "finished", label: "Finished", help: "An agent completed its task." },
    { k: "question", label: "Question", help: "An agent asked a decision question." },
    { k: "permission", label: "Permission", help: "An agent needs approval to run a command." },
    { k: "error", label: "Error", help: "An agent hit an error or crashed." },
  ];
  return (
    <>
      <SetGroup title="Delivery">
        <SetRow label="Native OS notifications" help="Off keeps all alerts inside the app only."
          dirty={s.osNotifications !== D.osNotifications} onReset={() => resetTo({ osNotifications: D.osNotifications })}>
          <Toggle value={s.osNotifications} onChange={(v) => set({ osNotifications: v })} />
        </SetRow>
      </SetGroup>

      <SetGroup title="Events">
        {EVENTS.map((ev) => (
          <SetRow key={ev.k} label={ev.label} help={ev.help} disabled={off}
            dirty={s.events[ev.k] !== D.events[ev.k]} onReset={() => resetMap("events", ev.k, D.events[ev.k])}>
            <Toggle value={s.events[ev.k]} onChange={(v) => setMap("events", ev.k, v)} disabled={off} />
          </SetRow>
        ))}
      </SetGroup>

      <SetGroup title="Sound">
        <SetRow label="Play sound" help="A short cue when a notification fires." disabled={off}
          dirty={s.sound !== D.sound} onReset={() => resetTo({ sound: D.sound })}>
          <Toggle value={s.sound} onChange={(v) => set({ sound: v })} disabled={off} />
        </SetRow>
        <SetRow label="Cue &amp; volume" help="Notification tone and loudness." disabled={off || !s.sound}
          dirty={s.soundName !== D.soundName || s.volume !== D.volume}
          onReset={() => resetTo({ soundName: D.soundName, volume: D.volume })}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <SetSelect value={s.soundName} options={SOUND_OPTIONS} onChange={(v) => set({ soundName: v })} />
            <Icon name="volume" size="sm" style={{ color: "var(--ink-4)" }} />
            <input type="range" className="set-slider" min={0} max={100} value={s.volume}
              onChange={(e) => set({ volume: Number(e.target.value) })} />
            <span className="tnum" style={{ fontSize: 11, color: "var(--ink-3)", width: 30, textAlign: "right" }}>{s.volume}%</span>
          </div>
        </SetRow>
      </SetGroup>
    </>
  );
}

// ── shell ────────────────────────────────────────────────────────────────────
function SettingsModal({ settings, setSettings, onClose, initialSection, initialModelOpen, initialConfirm, onOpenLog, onShowWhatsNew, onInstallUpdate, onOpenKeymap, onSleepIdle, projects, agents }) {
  const D = SETTINGS_DEFAULTS;
  const s = settings;
  const [section, setSection] = useSS(initialSection || "updates");
  const [modelOpen, setModelOpen] = useSS(!!initialModelOpen);
  const [confirm, setConfirm] = useSS(initialConfirm || null);
  const [det, setDet] = useSS(() => sClone(SETTINGS_DETECTED));

  const set = (patch) => setSettings((prev) => ({ ...prev, ...patch }));
  const setMap = (key, sub, val) => setSettings((prev) => ({ ...prev, [key]: { ...prev[key], [sub]: val } }));
  const resetTo = (patch) => setSettings((prev) => ({ ...prev, ...patch }));
  const resetMap = (key, sub, val) => setSettings((prev) => ({ ...prev, [key]: { ...prev[key], [sub]: val } }));
  const resetAll = () => { setSettings(sClone(D)); setConfirm(null); };
  const anyDirty = !sEq(s, D);

  useSE(() => {
    const onKey = (e) => { if (e.key === "Escape") { if (modelOpen) setModelOpen(false); else onClose(); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, modelOpen]);

  const cur = SECTIONS.find((x) => x.id === section) || SECTIONS[0];
  const SUBS = { updates: "Channel, version & install behavior", agent: "Defaults for newly spawned agents",
    editor: "Keymap preset, saving & gutter", memory: "Sleep policy, renderer & scrollback ring",
    project: "Overrides that apply to one repository",
    perms: "Auto-approval & remote control", notif: "OS alerts, events & sound" };

  return (
    <>
      <style>{SETTINGS_STYLE}</style>
      <div className="set-backdrop" onMouseDown={onClose}>
        <div className="set-modal" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Settings">
          {/* nav */}
          <nav className="set-nav">
            <div className="set-brand">
              <span className="gi"><Icon name="settings" size="sm" /></span>
              <div style={{ minWidth: 0 }}>
                <div className="bt">Settings</div>
                <div className="bs">Orrery preferences</div>
              </div>
            </div>
            <div className="set-nav-list">
              {SECTIONS.map((sec) => (
                <button key={sec.id} className={"set-nav-item" + (section === sec.id ? " on" : "")}
                  onClick={() => { setSection(sec.id); setModelOpen(false); }}>
                  <Icon name={sec.icon} size="sm" />
                  <span className="lb">{sec.label}</span>
                  {sec.id === "updates" && <span className="set-nav-dot" title="Update available" />}
                </button>
              ))}
            </div>
          </nav>

          {/* main */}
          <div className="set-main">
            <div className="set-head">
              <div>
                <div className="ht">{cur.label}</div>
                <div className="hs">{SUBS[section]}</div>
              </div>
              <button className="set-x" onClick={onClose} aria-label="Close settings"><Icon name="x" size="sm" /></button>
            </div>
            <div className="set-body" key={section}>
              {section === "updates" && <UpdatesSection s={s} set={set} resetTo={resetTo} D={D} onOpenLog={onOpenLog} onShowWhatsNew={onShowWhatsNew} onInstallUpdate={onInstallUpdate} />}
              {section === "agent" && <AgentSection s={s} set={set} setMap={setMap} resetTo={resetTo} resetMap={resetMap} D={D} modelOpen={modelOpen} setModelOpen={setModelOpen} det={det} setDet={setDet} />}
              {section === "editor" && <EditorSection s={s} set={set} resetTo={resetTo} D={D} onOpenKeymap={onOpenKeymap} />}
              {section === "memory" && <MemorySection s={s} set={set} resetTo={resetTo} D={D} onSleepIdle={onSleepIdle} agents={agents} />}
              {section === "project" && <ProjectSection s={s} set={set} D={D} projects={projects} />}
              {section === "perms" && <PermissionsSection s={s} set={set} setMap={setMap} resetTo={resetTo} resetMap={resetMap} D={D} confirm={confirm} setConfirm={setConfirm} det={det} />}
              {section === "notif" && <NotificationsSection s={s} set={set} setMap={setMap} resetTo={resetTo} resetMap={resetMap} D={D} />}
            </div>
            <div className="set-foot">
              {anyDirty
                ? <button className="reset-all" onClick={resetAll}><Icon name="refresh" size="sm" />Reset all to defaults</button>
                : <span className="fl"><span className="fd" />Changes apply instantly</span>}
              <span className="sp" />
              <button className="btn ghost-hair" onClick={onClose} style={{ padding: "5px 14px" }}>Cancel</button>
              <button className="set-done" onClick={onClose}><Icon name="check" size="sm" />Done</button>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

Object.assign(window, { SettingsModal, SETTINGS_DEFAULTS, SETTINGS_DETECTED });
