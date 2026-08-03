/* global React, Icon, ToolBadge, StatusDot, STATUS_META, Spark, fmtDur, TableRowSkeleton */
// ORCHESTRA — Dev console popover. Perf = simulated Tauri-invoke metrics;
// Agents/Projects = live in-memory state inspectors (read from app props).

const { useState: useDS, useEffect: useDE, useRef: useDR, useReducer: useDRed } = React;

// ── scoped styles (all selectors under .dvcon; dv- prefixed to avoid collisions)
const DVC_STYLE = `
.dvcon,.dvc-fab{--lat-g:#35e0a1;--lat-a:#f5c451;--lat-r:#ff5d7a;
  --lat-g-bg:rgba(53,224,161,.08);--lat-a-bg:rgba(245,196,81,.13);--lat-r-bg:rgba(255,93,122,.18);
  --lat-r-ring:rgba(255,93,122,.4);--dvadd:#5ef0bb;--dvdel:#ff8ba0;}
[data-theme="light"] .dvcon,[data-theme="light"] .dvc-fab{--lat-g:#0a8f5e;--lat-a:#a9700f;--lat-r:#d6304e;
  --lat-g-bg:rgba(10,143,94,.09);--lat-a-bg:rgba(169,112,15,.13);--lat-r-bg:rgba(214,48,78,.14);
  --lat-r-ring:rgba(214,48,78,.34);--dvadd:#0a8f5e;--dvdel:#c23252;}

.dvc-fab{position:fixed;right:18px;bottom:18px;z-index:90;width:44px;height:44px;border-radius:13px;
  display:grid;place-items:center;cursor:pointer;border:1px solid var(--hair-2);
  background:linear-gradient(180deg,var(--panel-3),var(--panel));color:var(--ink-2);
  box-shadow:var(--shadow);transition:transform .16s,color .16s,border-color .16s,box-shadow .16s;}
.dvc-fab:hover{transform:translateY(-2px);color:var(--ink);border-color:rgba(var(--accent-rgb),.5);}
.dvc-fab.on{color:var(--accent);border-color:rgba(var(--accent-rgb),.6);box-shadow:var(--shadow),0 0 18px -6px rgba(var(--accent-rgb),.7);}
.dvc-fab svg{width:19px;height:19px;}
.dvc-fab .dvc-badge{position:absolute;top:-6px;right:-6px;min-width:17px;height:17px;padding:0 4px;border-radius:999px;background:var(--lat-r);color:#fff;font-size:10px;font-weight:700;line-height:1;display:grid;place-items:center;border:2px solid var(--panel);font-variant-numeric:tabular-nums;animation:dvcpulse 1.8s ease-in-out infinite;}
@keyframes dvcpulse{0%{box-shadow:0 0 0 0 rgba(255,93,122,.5);}70%{box-shadow:0 0 0 7px rgba(255,93,122,0);}100%{box-shadow:0 0 0 0 rgba(255,93,122,0);}}

.dvcon{position:fixed;right:18px;bottom:74px;z-index:91;width:880px;max-width:calc(100vw - 32px);
  max-height:calc(100vh - 130px);display:flex;flex-direction:column;overflow:hidden;
  background:var(--panel);border:1px solid var(--hair-2);border-radius:14px;
  box-shadow:var(--shadow),0 0 0 1px rgba(var(--accent-rgb),.04);font-family:var(--font-mono);
  transform-origin:bottom right;animation:dvcin .22s cubic-bezier(.2,.7,.2,1);}
@keyframes dvcin{from{opacity:0;transform:translateY(8px) scale(.985);}to{opacity:1;transform:none;}}

.dvc-head{flex:none;display:flex;align-items:center;flex-wrap:nowrap;gap:10px;padding:9px 11px 9px 13px;border-bottom:1px solid var(--hair);background:linear-gradient(180deg,var(--panel-3),var(--panel));overflow:hidden;}
.dvc-brand{display:flex;align-items:center;color:var(--accent);flex:none;}
.dvc-tabs{display:flex;gap:2px;padding:2px;background:var(--panel-2);border:1px solid var(--hair);border-radius:8px;min-width:0;flex:0 1 auto;overflow-x:auto;overflow-y:hidden;scrollbar-width:none;}
.dvc-tabs::-webkit-scrollbar{height:0;width:0;}
.dvc-tab{flex:none;}
.dvc-tab{display:inline-flex;align-items:center;gap:6px;height:28px;font-family:var(--font-mono);font-size:11px;color:var(--ink-3);background:transparent;border:none;border-radius:6px;padding:0 10px;cursor:pointer;transition:all .12s;}
.dvc-tab:hover{color:var(--ink-2);}
.dvc-tab.on{color:var(--ink);background:var(--panel-3);box-shadow:0 0 0 1px var(--hair-2);}
.dvc-tab.on svg{color:var(--accent);}
.dvc-tab svg{width:13px;height:13px;color:var(--ink-4);}
.dvc-cnt{display:inline-flex;align-items:center;justify-content:center;height:15px;font-size:9px;padding:0 5px;border-radius:999px;background:var(--panel);border:1px solid var(--hair);color:var(--ink-3);min-width:16px;}
.dvc-tab.on .dvc-cnt{color:var(--ink-2);border-color:var(--hair-2);}
.dvc-live{display:inline-flex;align-items:center;gap:6px;font-size:9.5px;color:var(--ink-3);flex:none;}
.dvc-ld{width:7px;height:7px;border-radius:50%;background:var(--lat-g);position:relative;}
.dvc-ld::after{content:"";position:absolute;inset:-3px;border-radius:50%;background:inherit;opacity:.4;filter:blur(2px);}
.dvc-live.on .dvc-ld{animation:dvcblink 1.5s ease-in-out infinite;}
@keyframes dvcblink{0%,100%{opacity:1;}50%{opacity:.35;}}
.dvc-sp{flex:1 1 0;min-width:0;}
.dvc-ic{flex:none;display:inline-flex;align-items:center;gap:6px;font-family:var(--font-mono);font-size:11px;color:var(--ink-2);background:transparent;border:1px solid var(--hair);border-radius:var(--r-sm);padding:5px 9px;cursor:pointer;transition:all .12s;white-space:nowrap;}
.dvc-ic:hover{color:var(--ink);background:var(--panel-3);border-color:var(--hair-2);}
.dvc-ic svg{width:13px;height:13px;}
.dvc-x{border:none;padding:5px;color:var(--ink-3);background:transparent;border-radius:var(--r-sm);cursor:pointer;display:inline-flex;}
.dvc-x:hover{color:var(--ink);background:var(--panel-3);}
.dvc-x svg{width:13px;height:13px;}

.dvc-body{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;}
.dvc-body::-webkit-scrollbar{width:9px;}
.dvc-body::-webkit-scrollbar-thumb{background:var(--hair-2);border-radius:6px;border:2px solid transparent;background-clip:padding-box;}
.dvc-body::-webkit-scrollbar-thumb:hover{background:var(--ink-4);background-clip:padding-box;}

.dvc-tbl{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums;}
.dvc-tbl thead th{position:sticky;top:0;z-index:2;background:var(--panel-2);border-bottom:1px solid var(--hair-2);font-weight:500;font-size:9.5px;letter-spacing:.06em;text-transform:uppercase;color:var(--ink-3);padding:8px 9px;text-align:right;white-space:nowrap;cursor:pointer;user-select:none;transition:color .12s,background .12s;}
.dvc-tbl thead th:first-child{text-align:left;padding-left:13px;}
.dvc-tbl thead th:last-child{padding-right:13px;}
.dvc-tbl thead th:hover{color:var(--ink-2);background:var(--panel-3);}
.dvc-tbl thead th.srt{color:var(--ink);}
.dvc-arr{display:inline-block;width:9px;margin-left:3px;color:var(--accent);font-size:8px;vertical-align:middle;}
.dvc-tbl tbody td{padding:0 9px;height:32px;border-bottom:1px solid var(--hair);text-align:right;white-space:nowrap;font-size:12px;color:var(--ink-2);}
.dvc-tbl tbody td:first-child{text-align:left;padding-left:13px;}
.dvc-tbl tbody td:last-child{padding-right:13px;}
.dvc-row{cursor:pointer;transition:background .1s;}
.dvc-row:hover{background:var(--panel-2);}
.dvc-row.open{background:var(--panel-2);}
.dvc-tw{width:11px;height:11px;flex:none;color:var(--ink-4);transition:transform .15s;}
.dvc-row.open .dvc-tw{transform:rotate(90deg);color:var(--accent);}
.dvc-lead{display:flex;align-items:center;gap:8px;color:var(--ink);font-weight:500;}
.dvc-nm{overflow:hidden;text-overflow:ellipsis;}
.dvc-id{color:var(--ink-4);font-weight:400;}

.dvc-lat .v{display:inline-block;padding:2px 6px;border-radius:5px;font-weight:500;}
.dvc-lat.g .v{color:var(--lat-g);}
.dvc-lat.a .v{color:var(--lat-a);background:var(--lat-a-bg);}
.dvc-lat.r .v{color:var(--lat-r);background:var(--lat-r-bg);font-weight:600;box-shadow:inset 0 0 0 1px var(--lat-r-ring);}
.dvc-lat.na .v{color:var(--ink-4);}
.dvc-dim{color:var(--ink-4);}
.dvc-err .v{display:inline-block;padding:2px 6px;border-radius:5px;}
.dvc-err.zero .v{color:var(--ink-4);}
.dvc-err.bad .v{color:var(--lat-r);background:var(--lat-r-bg);font-weight:600;box-shadow:inset 0 0 0 1px var(--lat-r-ring);}
.dvc-spk{text-align:right;padding-right:13px;}
.dvc-spk svg{display:inline-block;vertical-align:middle;}

.dvc-stat{display:inline-flex;align-items:center;gap:6px;justify-content:flex-end;}
.dvc-stat .lb{font-size:10px;letter-spacing:.06em;text-transform:uppercase;}
.dvc-needs{width:5px;height:5px;border-radius:50%;background:var(--st-blocked);flex:none;}
.dvc-tool{width:17px;height:17px;flex:none;border-radius:4px;display:inline-grid;place-items:center;}
.dvc-pchip{display:inline-flex;align-items:center;gap:6px;justify-content:flex-end;color:var(--ink-2);}
.dvc-pi{width:16px;height:16px;border-radius:4px;display:grid;place-items:center;flex:none;}
.dvc-pn{overflow:hidden;text-overflow:ellipsis;max-width:96px;}
.dvc-add{color:var(--dvadd);} .dvc-del{color:var(--dvdel);}
.dvc-mtr{display:inline-block;width:46px;height:4px;border-radius:3px;background:var(--hair);overflow:hidden;vertical-align:middle;margin-right:6px;}
.dvc-mtr>i{display:block;height:100%;border-radius:3px;}
.dvc-branch{color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;max-width:120px;display:inline-block;vertical-align:bottom;}

.dvc-detail>td{padding:0;border-bottom:1px solid var(--hair);background:var(--bg);}
.dvc-din{padding:12px 14px 14px 32px;display:flex;flex-direction:column;gap:11px;animation:dvcexp .22s ease;}
@keyframes dvcexp{from{opacity:0;transform:translateY(-3px);}to{opacity:1;transform:none;}}
.dvc-kv{display:grid;grid-template-columns:repeat(auto-fill,minmax(210px,1fr));gap:5px 22px;}
.dvc-kv .r{display:flex;gap:8px;font-size:11px;align-items:baseline;}
.dvc-kv .k{color:var(--ink-4);min-width:74px;}
.dvc-kv .vv{color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-kv .vv b{color:var(--ink);font-weight:600;}
.dvc-sl{font-size:9.5px;letter-spacing:.1em;text-transform:uppercase;color:var(--ink-3);margin-bottom:3px;}
.dvc-files{display:flex;flex-direction:column;gap:1px;}
.dvc-frow{display:grid;grid-template-columns:16px 1fr auto;gap:9px;align-items:center;font-size:11px;padding:2px 0;color:var(--ink-2);}
.dvc-fst{width:15px;height:15px;border-radius:4px;display:grid;place-items:center;font-size:9px;font-weight:700;flex:none;}
.dvc-fst.A{color:var(--dvadd);background:rgba(94,240,187,.12);}
.dvc-fst.M{color:var(--lat-a);background:var(--lat-a-bg);}
.dvc-fst.D{color:var(--dvdel);background:rgba(255,139,160,.12);}
.dvc-fp{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-fd{font-size:10px;display:flex;gap:6px;}
.dvc-task{font-size:11.5px;color:var(--ink-2);line-height:1.5;}
.dvc-attn{display:inline-flex;align-items:center;gap:6px;font-size:10.5px;padding:5px 9px;border-radius:6px;color:var(--dvdel);background:var(--lat-r-bg);border:1px solid var(--lat-r-ring);align-self:flex-start;}
.dvc-chip{display:inline-flex;align-items:center;gap:5px;font-size:10px;padding:2px 7px;border-radius:999px;border:1px solid var(--hair);color:var(--ink-2);background:var(--panel-2);}

.dvc-calls{display:flex;flex-direction:column;gap:1px;}
.dvc-cr{display:grid;grid-template-columns:84px 1fr auto 14px;align-items:center;gap:10px;font-size:11px;padding:3px 0;color:var(--ink-2);}
.dvc-cr .ts{color:var(--ink-4);} .dvc-cr .cm{color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-cr .du{text-align:right;font-weight:500;}
.dvc-cr .du.g{color:var(--lat-g);} .dvc-cr .du.a{color:var(--lat-a);} .dvc-cr .du.r{color:var(--lat-r);}
.dvc-dot{width:7px;height:7px;border-radius:50%;justify-self:center;}
.dvc-dot.ok{background:var(--lat-g);} .dvc-dot.er{background:var(--lat-r);box-shadow:0 0 0 2px var(--lat-r-bg);}
.dvc-dh{display:flex;align-items:center;gap:10px;font-size:10px;color:var(--ink-3);}
.dvc-dh .lbl{text-transform:uppercase;letter-spacing:.1em;}
.dvc-ds{display:flex;gap:14px;margin-left:auto;color:var(--ink-2);}
.dvc-ds b{color:var(--ink);font-weight:600;}

.dvc-feed{border-top:1px solid var(--hair-2);}
.dvc-fh{position:sticky;top:0;z-index:1;display:flex;align-items:center;gap:9px;padding:9px 13px;background:var(--panel-2);border-bottom:1px solid var(--hair);cursor:pointer;user-select:none;transition:background .12s;}
.dvc-fh:hover{background:var(--panel-3);}
.dvc-fh .lbl{font-size:9.5px;letter-spacing:.1em;text-transform:uppercase;color:var(--ink-3);}
.dvc-fh:hover .lbl{color:var(--ink-2);}
.dvc-fh .tw{width:11px;height:11px;flex:none;color:var(--ink-4);transition:transform .16s;}
.dvc-fh.open .tw{transform:rotate(90deg);color:var(--accent);}
.dvc-fh .ct{font-size:10px;color:var(--ink-4);margin-left:auto;}

/* ── resources (cpu/mem per process) ── */
.dvc-res-top{display:grid;grid-template-columns:1fr 1fr;gap:12px;padding:13px;border-bottom:1px solid var(--hair);}
.dvc-gauge{background:var(--panel-2);border:1px solid var(--hair);border-radius:10px;padding:11px 12px;display:flex;flex-direction:column;gap:7px;}
.dvc-gauge .g-top{display:flex;align-items:baseline;gap:8px;}
.dvc-gauge .g-lab{font-size:9.5px;letter-spacing:.12em;text-transform:uppercase;color:var(--ink-3);display:inline-flex;align-items:center;gap:6px;}
.dvc-gauge .g-lab svg{width:12px;height:12px;color:var(--ink-4);}
.dvc-gauge .g-val{font-family:var(--font-disp);font-size:21px;font-weight:600;color:var(--ink);letter-spacing:-.02em;margin-left:auto;}
.dvc-gauge .g-val small{font-size:11px;color:var(--ink-3);font-weight:500;margin-left:1px;}
.dvc-gbar{height:6px;border-radius:4px;background:var(--hair);overflow:hidden;}
.dvc-gbar>i{display:block;height:100%;border-radius:4px;transition:width .5s cubic-bezier(.3,.8,.3,1);}
.dvc-gauge .g-sub{font-size:9.5px;color:var(--ink-4);}
.dvc-kind{display:inline-grid;place-items:center;width:17px;height:17px;border-radius:4px;flex:none;}
.dvc-kind svg{width:11px;height:11px;}
.dvc-kind.core{color:var(--accent);background:color-mix(in oklch,var(--accent),transparent 84%);}
.dvc-kind.ui{color:var(--accent-2);background:color-mix(in oklch,var(--accent-2),transparent 84%);}
.dvc-kind.gpu{color:var(--ink-3);background:var(--panel-3);}
.dvc-pid{color:var(--ink-4);font-size:11px;}
.dvc-memmtr{display:inline-block;width:38px;height:4px;border-radius:3px;background:var(--hair);overflow:hidden;vertical-align:middle;margin-right:6px;}
.dvc-memmtr>i{display:block;height:100%;border-radius:3px;}
.dvc-exited{opacity:.55;}
.dvc-gridcmd{font-size:10.5px;color:var(--ink-3);font-family:var(--font-mono);background:var(--panel-2);border:1px solid var(--hair);border-radius:5px;padding:2px 7px;display:inline-block;}
.dvc-fl{padding:5px 13px 12px;}
.dvc-fr{display:grid;grid-template-columns:90px 16px 1fr auto 22px;align-items:center;gap:10px;font-size:11px;padding:4px 0;border-bottom:1px solid var(--hair);color:var(--ink-2);}
.dvc-fr:last-child{border-bottom:none;}
.dvc-fr.fresh{animation:dvcfresh .4s ease;}
@keyframes dvcfresh{from{background:rgba(var(--accent-2-rgb),.1);}to{background:transparent;}}
.dvc-fr .ts{color:var(--ink-4);} .dvc-fr .cm{color:var(--ink);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.dvc-fr .du{text-align:right;font-weight:500;}
.dvc-fr .du.g{color:var(--lat-g);} .dvc-fr .du.a{color:var(--lat-a);} .dvc-fr .du.r{color:var(--lat-r);}
.dvc-fr .ix{color:var(--ink-4);text-align:right;font-size:9px;}

.dvc-empty{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:13px;padding:54px 20px 60px;text-align:center;}
.dvc-ring{width:46px;height:46px;border-radius:50%;border:1.5px dashed var(--hair-2);display:grid;place-items:center;color:var(--ink-4);}
.dvc-empty h4{font-family:var(--font-disp);font-weight:600;font-size:14px;color:var(--ink-2);}
.dvc-empty p{font-size:11.5px;color:var(--ink-4);max-width:300px;line-height:1.55;}
.dvc-hint{display:inline-flex;align-items:center;gap:6px;font-size:10.5px;color:var(--ink-3);}
.dvc-hint .dvc-ld{animation:dvcblink 1.5s ease-in-out infinite;}

.dvc-foot{flex:none;display:flex;align-items:center;gap:14px;padding:8px 13px;border-top:1px solid var(--hair);background:var(--panel-2);font-size:10px;color:var(--ink-3);}
.dvc-leg{display:flex;align-items:center;gap:6px;}
.dvc-sw{width:9px;height:9px;border-radius:3px;}
.dvc-sw.g{background:var(--lat-g);} .dvc-sw.a{background:var(--lat-a);} .dvc-sw.r{background:var(--lat-r);}
`;

// ── perf metrics engine (simulated Tauri invokes) ──────────────────────────
const DVC_SEED = [
  { cmd:"agent_spawn",calls:3,rt:121,exec:109,p95:184,max:247,err:0.4 },
  { cmd:"git_status",calls:11,rt:96,exec:91,p95:142,max:210,err:1.2 },
  { cmd:"project_commits",calls:8,rt:43,exec:38,p95:71,max:96,err:0 },
  { cmd:"project_open",calls:2,rt:38,exec:33,p95:60,max:88,err:0 },
  { cmd:"agent_diff",calls:14,rt:22,exec:18,p95:35,max:52,err:0 },
  { cmd:"agent_logs",calls:22,rt:11,exec:8,p95:19,max:28,err:0 },
  { cmd:"file_read",calls:31,rt:7,exec:5,p95:12,max:19,err:0 },
  { cmd:"agent_list",calls:9,rt:5,exec:3,p95:9,max:14,err:0 },
  { cmd:"agent_input",calls:17,rt:4,exec:2,p95:7,max:12,err:0 },
  { cmd:"terminal_write",calls:58,rt:3,exec:2,p95:6,max:11,err:0 },
  { cmd:"settings_get",calls:6,rt:2,exec:1,p95:4,max:7,err:0 },
];
let DVC_SEQ = 0;
const dvcRnd = (a,b)=>a+Math.random()*(b-a);
const dvcJit = (v,p)=>Math.max(0,v*(1+dvcRnd(-p,p)));
function dvcHist(avg,p95){ const o=[]; for(let i=0;i<16;i++){ let v=dvcJit(avg,.35); if(Math.random()<.12) v=dvcRnd(avg,p95); o.push(v);} return o; }
function dvcClock(off){ const d=new Date(Date.now()-(off||0)),p=(n,l)=>String(n).padStart(l||2,"0"); return p(d.getHours())+":"+p(d.getMinutes())+":"+p(d.getSeconds())+"."+p(d.getMilliseconds(),3).slice(0,3); }
function dvcRecent(s,n){ const o=[]; for(let i=0;i<n;i++){ let dur=dvcJit(s.rt,.4); if(Math.random()<.15) dur=dvcRnd(s.rt,s.p95); const err=s.err>0&&Math.random()<(s.err/100)*3.2; o.push({ts:dvcClock(i*dvcRnd(400,1400)),dur:+dur.toFixed(1),err});} return o; }
function dvcBuildStats(pop){ return DVC_SEED.map(s=> pop?{...s,overhead:+(s.rt-s.exec).toFixed(1),hist:dvcHist(s.rt,s.p95),recent:dvcRecent(s,9)}:{cmd:s.cmd,calls:0,rt:null,exec:null,overhead:null,p95:null,max:null,err:0,hist:[],recent:[]}); }
function dvcBuildFeed(stats){ const f=[],pool=[]; stats.forEach(s=>{ if(s.calls>0){ const w=Math.max(1,Math.round(s.calls/3)); for(let i=0;i<w;i++) pool.push(s);} }); if(!pool.length) return []; let t=0; for(let i=0;i<30;i++){ const s=pool[Math.floor(Math.random()*pool.length)]; let dur=dvcJit(s.rt,.4); if(Math.random()<.14) dur=dvcRnd(s.rt,s.p95); const err=s.err>0&&Math.random()<(s.err/100)*3; t+=dvcRnd(120,900); f.push({id:++DVC_SEQ,ts:dvcClock(t),cmd:s.cmd,dur:+dur.toFixed(1),err,fresh:false}); } return f; }
function dvcPush(stats,feed){ const pool=[]; stats.forEach(s=>{ if(s.calls>0){ const w=Math.max(1,Math.round(s.calls/3)); for(let i=0;i<w;i++) pool.push(s);} }); if(!pool.length) return; const n=1+Math.floor(Math.random()*2); for(let i=0;i<n;i++){ const s=pool[Math.floor(Math.random()*pool.length)]; let dur=dvcJit(s.rt,.4); if(Math.random()<.14) dur=dvcRnd(s.rt,s.p95); const err=s.err>0&&Math.random()<(s.err/100)*3; feed.unshift({id:++DVC_SEQ,ts:dvcClock(0),cmd:s.cmd,dur:+dur.toFixed(1),err,fresh:true}); } if(feed.length>30) feed.length=30; }
function dvcTick(stats,feed){
  const allZero = stats.every(s=>s.calls===0);
  if(allZero){ // trickle back after a reset
    const wake=DVC_SEED[Math.floor(Math.random()*DVC_SEED.length)],tgt=stats.find(s=>s.cmd===wake.cmd);
    if(tgt&&tgt.calls===0){ Object.assign(tgt,{calls:Math.max(1,Math.round(dvcJit(wake.calls,.4))),rt:+dvcJit(wake.rt,.15).toFixed(1),exec:+dvcJit(wake.exec,.15).toFixed(1),p95:wake.p95,max:wake.max,err:wake.err,hist:dvcHist(wake.rt,wake.p95),recent:dvcRecent(wake,9)}); tgt.overhead=+(tgt.rt-tgt.exec).toFixed(1); }
    dvcPush(stats,feed); return;
  }
  stats.forEach(s=>{ if(s.calls===0) return; s.rt=+Math.max(.5,dvcJit(s.rt,.06)).toFixed(1); s.exec=+Math.max(.3,Math.min(s.rt-.2,dvcJit(s.exec,.06))).toFixed(1); s.overhead=+(s.rt-s.exec).toFixed(1); s.calls=Math.max(1,Math.round(dvcJit(s.calls,.12))); s.hist.push(s.rt); if(s.hist.length>16) s.hist.shift(); });
  dvcPush(stats,feed);
}
const dvcMs=(v)=> v==null?"—":v>=100?Math.round(v)+"ms":v>=10?v.toFixed(0)+"ms":v.toFixed(1)+"ms";
const dvcLat=(v)=> v==null?"na":v<=16?"g":v<=100?"a":"r";
const dvcLatColor=(c)=> c==="r"?"var(--lat-r)":c==="a"?"var(--lat-a)":"var(--lat-g)";

// ── resources engine (per-process CPU / MEM) ───────────────────────────────
const DVC_CORES = 10;
const DVC_SYSMEM = 16384; // MB
function dvcHash(s){ let h=0; for(let i=0;i<s.length;i++) h=(h*31+s.charCodeAt(i))|0; return Math.abs(h); }
function dvcClamp(v,lo,hi){ return Math.max(lo,Math.min(hi,v)); }
function dvcSeries(base,p,n){ const o=[]; for(let i=0;i<n;i++) o.push(dvcJit(base,p)); return o; }
// orrery's own processes
const DVC_ORRERY = [
  { key:"orrery:core", name:"Orrery", sub:"main · rust core", kind:"core", cmd:"orrery", cpu:[1,7], mem:[96,168], thr:[14,26] },
  { key:"orrery:webview", name:"Orrery WebView", sub:"ui · wkwebview", kind:"ui", cmd:"WebKitWebProcess", cpu:[2,14], mem:[300,560], thr:[28,58] },
  { key:"orrery:gpu", name:"Orrery GPU", sub:"gpu helper", kind:"gpu", cmd:"WebKit.GPU", cpu:[0,5], mem:[120,250], thr:[8,16] },
];
const DVC_ALIVE = ["running","waiting","queued","blocked"];
function dvcResReconcile(map, agents){
  DVC_ORRERY.forEach((p,i)=>{
    if(!map.has(p.key)){
      const cpu0=dvcRnd(p.cpu[0],p.cpu[1]), mem0=dvcRnd(p.mem[0],p.mem[1]);
      map.set(p.key, { key:p.key, name:p.name, sub:p.sub, kind:p.kind, cmd:p.cmd,
        pid:4800+i*23+Math.floor(Math.random()*9), cpu:cpu0, mem:mem0,
        threads:Math.round(dvcRnd(p.thr[0],p.thr[1])), start:Date.now()-dvcRnd(900,7200)*1000,
        cpuH:dvcSeries(cpu0,.35,20), memH:dvcSeries(mem0,.04,20), live:true });
    }
  });
  const alive=new Set();
  agents.forEach(a=>{
    if(!DVC_ALIVE.includes(a.status)) return;
    const key="agent:"+a.id; alive.add(key);
    if(!map.has(key)){
      const run=a.status==="running"; const cpu0=run?dvcRnd(18,62):dvcRnd(1,6); const mem0=dvcRnd(150,420);
      map.set(key, { key, name:a.name, sub:"agent · "+a.tool, kind:"agent", tool:a.tool, agentId:a.id, projectId:a.projectId,
        cmd:a.tool+(a.model?" "+a.model:""), pid:9000+dvcHash(a.id)%990, cpu:cpu0, mem:mem0,
        threads:Math.round(dvcRnd(8,28)), start:Date.now()-(a.elapsed||60)*1000,
        cpuH:dvcSeries(cpu0,.4,20), memH:dvcSeries(mem0,.05,20), live:true, status:a.status });
    } else { const v=map.get(key); v.status=a.status; v.name=a.name; v.tool=a.tool; }
  });
  for(const k of [...map.keys()]) if(k.startsWith("agent:") && !alive.has(k)) map.delete(k);
}
function dvcResTick(map, agents){
  dvcResReconcile(map, agents);
  for(const v of map.values()){
    if(v.kind==="agent" && v.status!=="running") v.cpu=dvcClamp(dvcJit(2.5,.6),0.2,6);
    else { const ceil=v.kind==="agent"?92:v.kind==="ui"?38:v.kind==="gpu"?12:18; v.cpu=dvcClamp(dvcJit(Math.max(0.5,v.cpu),.24),0.2,ceil); }
    const ceilM=v.kind==="ui"?640:v.kind==="agent"?620:280;
    v.mem=dvcClamp(dvcJit(v.mem,.035),v.kind==="agent"?110:80,ceilM);
    v.cpuH.push(v.cpu); if(v.cpuH.length>20) v.cpuH.shift();
    v.memH.push(v.mem); if(v.memH.length>20) v.memH.shift();
  }
}
const dvcCpuC=(v)=> v==null?"na":v<30?"g":v<70?"a":"r";
const dvcMemC=(v)=> v==null?"na":v<300?"g":v<600?"a":"r";
const dvcMem=(mb)=> mb>=1024?(mb/1024).toFixed(2)+" GB":Math.round(mb)+" MB";
const dvcUptime=(start)=>{ let s=Math.floor((Date.now()-start)/1000); const h=Math.floor(s/3600); s-=h*3600; const m=Math.floor(s/60); s-=m*60; return h>0?h+"h "+m+"m":m>0?m+"m "+s+"s":s+"s"; };

// ── small helpers ───────────────────────────────────────────────────────────
function dvcDelta(ag){ return (ag.files||[]).reduce((o,f)=>({a:o.a+(f.add||0),d:o.d+(f.del||0)}),{a:0,d:0}); }
function dvcAttn(ag){ return ag.status==="blocked" || (ag.pending||[]).some(p=>p.kind==="permission"||p.kind==="decision"); }
function dvcNote(ag){ if(ag.blockReason) return ag.blockReason; if(ag.waitReason) return ag.waitReason; const r=(ag.pending||[]).find(p=>p.kind==="review"); if(r) return r.title+" · "+r.cmd; const d=(ag.pending||[]).find(p=>p.kind==="decision"); if(d) return "Decision: "+d.title; return null; }

// ════════════════════════════════════════════════════════════════════════════
function DevConsole({ agents, projects, channel, loading, ctx }) {
  const [open, setOpen] = useDS(false);
  const [tab, setTab] = useDS("perf");
  const [, force] = useDRed((x)=>x+1, 0);
  const statsRef = useDR(null);
  const feedRef = useDR(null);
  if (statsRef.current === null) { statsRef.current = dvcBuildStats(true); feedRef.current = dvcBuildFeed(statsRef.current); }
  const [sort, setSort] = useDS({ key:"rt", dir:-1 });
  const [aSort, setASort] = useDS({ key:"status", dir:1 });
  const [pSort, setPSort] = useDS({ key:"name", dir:1 });
  const [openCmd, setOpenCmd] = useDS(null);
  const [openAg, setOpenAg] = useDS(null);
  const [openPr, setOpenPr] = useDS(null);
  const [openProc, setOpenProc] = useDS(null);
  const [feedOpen, setFeedOpen] = useDS(true);
  const [rSort, setRSort] = useDS({ key:"cpu", dir:-1 });
  const resRef = useDR(null);
  if (resRef.current === null) { resRef.current = new Map(); dvcResReconcile(resRef.current, agents); }
  const dev = channel !== "prod";

  // perf live sim — runs only while the panel is open
  useDE(() => {
    if (!open) return;
    const iv = setInterval(() => { dvcTick(statsRef.current, feedRef.current); force(); }, 1200);
    return () => clearInterval(iv);
  }, [open]);

  // resources live sim — ticks while panel open AND resources tab active
  useDE(() => {
    if (!open || tab !== "resources") { if(open) dvcResReconcile(resRef.current, agents); return; }
    const iv = setInterval(() => { dvcResTick(resRef.current, agents); force(); }, 1100);
    return () => clearInterval(iv);
  }, [open, tab, agents]);

  useDE(() => {
    const onKey = (e) => { if (e.key === "Escape") setOpen(false); };
    window.addEventListener("keydown", onKey);
    // opening from the command palette / status-bar memory split
    const onOpen = (e) => { setOpen(true); if (e.detail && e.detail.tab) setTab(e.detail.tab); };
    window.addEventListener("orrery:devpanel", onOpen);
    return () => { window.removeEventListener("keydown", onKey); window.removeEventListener("orrery:devpanel", onOpen); };
  }, []);

  // ctx for the roadmap performance surfaces (process tree · emits · budgets)
  const pctx = Object.assign({ flash: () => {}, isAsleep: () => false, visibleAgentIds: [] }, ctx || {}, { agents, projects });
  const perfBox = { height: "min(430px, 54vh)", display: "flex", flexDirection: "column", minHeight: 0 };

  const proj = (id) => projects.find((p) => p.id === id) || { name:id, color:"var(--ink-3)", icon:"box" };
  const agentsIn = (id) => agents.filter((a) => a.projectId === id);

  // ── PERF ──
  const sortStats = () => {
    const arr = statsRef.current.slice(), k = sort.key, d = sort.dir;
    arr.sort((a,b)=>{ if(k==="cmd"||k==="spark") return a.cmd<b.cmd?-1*d:a.cmd>b.cmd?1*d:0; const av=a[k]==null?-1:a[k],bv=b[k]==null?-1:b[k]; return (av-bv)*d; });
    return arr;
  };
  const clickPerfSort = (k) => setSort((s)=> s.key===k ? {key:k,dir:-s.dir} : {key:k,dir:(k==="cmd"?1:-1)});
  const PCOLS = [["cmd","command"],["calls","calls/10s"],["rt","avg RT"],["exec","avg exec"],["overhead","overhead"],["p95","p95"],["max","max"],["err","err%"],["spark","trend"]];
  const reset = () => { statsRef.current.forEach(s=>{ s.calls=0;s.rt=null;s.exec=null;s.overhead=null;s.p95=null;s.max=null;s.err=0;s.hist=[];s.recent=[]; }); feedRef.current=[]; setOpenCmd(null); force(); };
  const hasCalls = statsRef.current.some(s=>s.calls>0);

  // ── AGENTS ──
  const SPRIO = { blocked:0, running:1, waiting:2, queued:3, done:4, idle:5 };
  const sortAgents = () => {
    const arr = agents.slice(), k = aSort.key, d = aSort.dir;
    arr.sort((a,b)=>{
      if(k==="status") return (SPRIO[a.status]-SPRIO[b.status])*d;
      if(k==="delta") return (dvcDelta(a).a-dvcDelta(b).a)*d;
      if(k==="project") return a.projectId<b.projectId?-1*d:1*d;
      if(typeof a[k]==="number") return (a[k]-b[k])*d;
      return String(a[k])<String(b[k])?-1*d:1*d;
    });
    return arr;
  };
  const clickASort = (k) => setASort((s)=> s.key===k ? {key:k,dir:-s.dir} : {key:k,dir:1});
  const ACOLS = [["name","agent"],["status","status"],["tool","tool"],["project","project"],["branch","branch"],["commits","c"],["delta","Δ lines"],["elapsed","elapsed"],["progress","prog"]];

  // ── PROJECTS ──
  const sortProjects = () => {
    const arr = projects.slice(), k = pSort.key, d = pSort.dir;
    arr.sort((a,b)=>{
      if(k==="branches") return ((a.branches||[]).length-(b.branches||[]).length)*d;
      if(k==="agents") return (agentsIn(a.id).length-agentsIn(b.id).length)*d;
      if(k==="files") return ((a.files||[]).length-(b.files||[]).length)*d;
      return String(a[k])<String(b[k])?-1*d:1*d;
    });
    return arr;
  };
  const clickPSort = (k) => setPSort((s)=> s.key===k ? {key:k,dir:-s.dir} : {key:k,dir:1});
  const PRCOLS = [["name","project"],["id","id"],["head","head"],["branch","default"],["branches","branches"],["agents","agents"],["files","files"]];

  // ── RESOURCES ──
  const procList = () => Array.from(resRef.current.values());
  const sortProcs = () => {
    const arr = procList(), k = rSort.key, d = rSort.dir;
    arr.sort((a,b)=>{
      if(k==="uptime") return (a.start-b.start)*d;
      if(k==="name") return a.name<b.name?-1*d:1*d;
      if(typeof a[k]==="number") return (a[k]-b[k])*d;
      return String(a[k])<String(b[k])?-1*d:1*d;
    });
    return arr;
  };
  const clickRSort = (k) => setRSort((s)=> s.key===k ? {key:k,dir:-s.dir} : {key:k,dir:(k==="name"?1:-1)});
  const RCOLS = [["name","process"],["pid","pid"],["cpu","cpu %"],["cpuH","trend"],["mem","memory"],["threads","thr"],["uptime","uptime"]];
  const sumCpu = procList().reduce((a,p)=>a+(p.cpu||0),0);
  const sumMem = procList().reduce((a,p)=>a+(p.mem||0),0);
  const machineCpu = sumCpu / DVC_CORES; // % of total machine capacity
  const memPctSys = (sumMem / DVC_SYSMEM) * 100;
  const agentProcs = procList().filter(p=>p.kind==="agent").length;

  const statColor = (st) => (STATUS_META[st]||STATUS_META.idle).color;
  const cnt = (st) => agents.filter(a=>a.status===st).length;
  // alert badge on the anchor button: commands breaching budget (>100ms) or erroring
  const alertCount = statsRef.current.filter(s=>s.calls>0 && (s.err>0 || (s.rt!=null && s.rt>100))).length;

  return (
    <React.Fragment>
      <style>{DVC_STYLE}</style>
      <button className={"dvc-fab" + (open?" on":"")} title={alertCount?("Dev console · "+alertCount+" perf alert"+(alertCount>1?"s":"")):"Dev console"} aria-label="Dev console"
        onClick={() => setOpen((o)=>!o)}>
        {alertCount>0 && <span className="dvc-badge">{alertCount}</span>}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M3 12h3l2.5-6 4 13 3-9 1.5 2H21" /></svg>
      </button>

      {open && (
        <section className="dvcon" aria-label="Dev console">
          <header className="dvc-head">
            <span className="dvc-brand"><Icon name="cpu" size="sm" /></span>
            <div className="dvc-tabs">
              <button className={"dvc-tab"+(tab==="perf"?" on":"")} onClick={()=>setTab("perf")}><Icon name="spark" size="sm" />Perf</button>
              <button className={"dvc-tab"+(tab==="agents"?" on":"")} onClick={()=>setTab("agents")}><Icon name="agent" size="sm" />Agents<span className="dvc-cnt">{agents.length}</span></button>
              <button className={"dvc-tab"+(tab==="projects"?" on":"")} onClick={()=>setTab("projects")}><Icon name="box" size="sm" />Projects<span className="dvc-cnt">{projects.length}</span></button>
              <button className={"dvc-tab"+(tab==="resources"?" on":"")} onClick={()=>setTab("resources")}><Icon name="cpu" size="sm" />Resources<span className="dvc-cnt">{procList().length}</span></button>
              <button className={"dvc-tab"+(tab==="procs"?" on":"")} onClick={()=>setTab("procs")}><Icon name="graph" size="sm" />Tree</button>
              <button className={"dvc-tab"+(tab==="emits"?" on":"")} onClick={()=>setTab("emits")}><Icon name="link" size="sm" />Emits</button>
              <button className={"dvc-tab"+(tab==="budgets"?" on":"")} onClick={()=>setTab("budgets")}><Icon name="flag" size="sm" />Budgets</button>
            </div>
            <span className="dvc-live on"><span className="dvc-ld" />live</span>
            <span className="dvc-sp" />
            {tab==="perf" && <button className="dvc-ic" onClick={reset} title="Clear counters"><Icon name="refresh" size="sm" />Reset</button>}
            <button className="dvc-x" onClick={()=>setOpen(false)} title="Close"><Icon name="x" size="sm" /></button>
          </header>

          <div className="dvc-body">
            {/* ── PROCESS TREE / EMITS / BUDGETS (roadmap A7.7 · A0.7) ── */}
            {tab==="procs" && window.ProcessTree && (
              <div style={perfBox}>
                {window.SubscriptionStrip && <window.SubscriptionStrip ctx={pctx} />}
                <window.ProcessTree ctx={pctx} />
              </div>
            )}
            {tab==="emits" && window.EmitTelemetry && <div style={perfBox}><window.EmitTelemetry ctx={pctx} /></div>}
            {tab==="budgets" && window.BudgetsPanel && <div style={perfBox}><window.BudgetsPanel ctx={pctx} /></div>}

            {/* ── PERF ── */}
            {tab==="perf" && (
              <React.Fragment>
                {hasCalls ? (
                  <table className="dvc-tbl">
                    <thead><tr>{PCOLS.map(([k,l])=>(
                      <th key={k} className={sort.key===k?"srt":""} onClick={()=>clickPerfSort(k)}>{l}{sort.key===k && <span className="dvc-arr">{sort.dir<0?"▼":"▲"}</span>}</th>
                    ))}</tr></thead>
                    <tbody>
                      {sortStats().map((s)=>{
                        const rt=dvcLat(s.rt),ex=dvcLat(s.exec),p9=dvcLat(s.p95),mx=dvcLat(s.max),isOpen=openCmd===s.cmd;
                        return (
                          <React.Fragment key={s.cmd}>
                            <tr className={"dvc-row"+(isOpen?" open":"")} onClick={()=> dev && setOpenCmd(isOpen?null:s.cmd)}>
                              <td><span className="dvc-lead">{dev && <svg className="dvc-tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6"/></svg>}<span className="dvc-nm">{s.cmd}</span></span></td>
                              <td className="tnum" style={{color:"var(--ink-2)"}}>{s.calls}</td>
                              <td className={"dvc-lat "+rt+" tnum"}><span className="v">{dvcMs(s.rt)}</span></td>
                              <td className={"dvc-lat "+ex+" tnum"}><span className="v">{dvcMs(s.exec)}</span></td>
                              <td className="tnum" style={{color:"var(--ink-2)"}}>{s.overhead==null?"—":(s.overhead<10?s.overhead.toFixed(1):Math.round(s.overhead))+"ms"}</td>
                              <td className={"dvc-lat "+p9+" tnum"}><span className="v">{dvcMs(s.p95)}</span></td>
                              <td className={"dvc-lat "+mx+" tnum"}><span className="v">{dvcMs(s.max)}</span></td>
                              <td className={"dvc-err "+(s.err>0?"bad":"zero")+" tnum"}><span className="v">{s.err.toFixed(1)}%</span></td>
                              <td className="dvc-spk"><Spark data={s.hist} color={dvcLatColor(rt)} w={58} h={16} /></td>
                            </tr>
                            {isOpen && dev && (
                              <tr className="dvc-detail"><td colSpan={PCOLS.length}><div className="dvc-din">
                                <div className="dvc-dh"><span className="lbl">recent · {s.cmd}</span><span className="dvc-ds tnum"><span>p95 <b>{dvcMs(s.p95)}</b></span><span>max <b>{dvcMs(s.max)}</b></span><span>overhead <b>{(s.overhead<10?s.overhead.toFixed(1):Math.round(s.overhead))}ms</b></span></span></div>
                                <div className="dvc-calls">{s.recent.map((c,i)=>{ const cc=dvcLat(c.dur); return (<div className="dvc-cr" key={i}><span className="ts tnum">{c.ts}</span><span className="cm">{s.cmd}</span><span className={"du "+cc+" tnum"}>{dvcMs(c.dur)}</span><span className={"dvc-dot "+(c.err?"er":"ok")} /></div>); })}</div>
                              </div></td></tr>
                            )}
                          </React.Fragment>
                        );
                      })}
                    </tbody>
                  </table>
                ) : (
                  <div className="dvc-empty">
                    <div className="dvc-ring"><Icon name="spark" /></div>
                    <h4>No calls yet</h4>
                    <p>Invoke metrics populate as the frontend dispatches Tauri commands. Counters reset on a rolling 10-second window.</p>
                    <span className="dvc-hint"><span className="dvc-ld" />listening for invokes…</span>
                  </div>
                )}
                {hasCalls && dev && (
                  <div className="dvc-feed">
                    <div className={"dvc-fh"+(feedOpen?" open":"")} onClick={()=>setFeedOpen(o=>!o)} title={feedOpen?"Collapse":"Expand"}>
                      <svg className="tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6"/></svg>
                      <span className="lbl">Recent calls</span><Icon name="timeline" size="sm" style={{color:"var(--ink-4)"}} /><span className="ct">last {feedRef.current.length}</span>
                    </div>
                    {feedOpen && <div className="dvc-fl">{feedRef.current.map((f)=>{ const cc=dvcLat(f.dur); return (<div className={"dvc-fr"+(f.fresh?" fresh":"")} key={f.id}><span className="ts tnum">{f.ts}</span><span className={"dvc-dot "+(f.err?"er":"ok")} style={{justifySelf:"start"}} /><span className="cm">{f.cmd}</span><span className={"du "+cc+" tnum"}>{dvcMs(f.dur)}</span><span className="ix tnum">{f.err?"err":"ok"}</span></div>); })}</div>}
                  </div>
                )}
              </React.Fragment>
            )}

            {/* ── AGENTS ── */}
            {tab==="agents" && (
              <table className="dvc-tbl">
                <thead><tr>{ACOLS.map(([k,l])=>(
                  <th key={k} className={aSort.key===k?"srt":""} onClick={()=>clickASort(k)}>{l}{aSort.key===k && <span className="dvc-arr">{aSort.dir<0?"▼":"▲"}</span>}</th>
                ))}</tr></thead>
                <tbody>
                  {loading
                    ? Array.from({ length: 6 }).map((_, i) => <TableRowSkeleton key={i} lead cols={[ [120,90,138,104][i%4], 46, 17, 96, 90, 18, 56, 50, 44 ]} />)
                    : sortAgents().map((ag)=>{
                    const p=proj(ag.projectId),dl=dvcDelta(ag),isOpen=openAg===ag.id,attn=dvcAttn(ag),note=dvcNote(ag),sc=statColor(ag.status);
                    return (
                      <React.Fragment key={ag.id}>
                        <tr className={"dvc-row"+(isOpen?" open":"")} onClick={()=>setOpenAg(isOpen?null:ag.id)}>
                          <td><span className="dvc-lead"><svg className="dvc-tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6"/></svg><span className="dvc-id">{ag.id}</span><span className="dvc-nm">{ag.name}</span>{attn && <span className="dvc-needs" title="needs you" />}</span></td>
                          <td><span className="dvc-stat"><span className="lb" style={{color:sc}}>{ag.status}</span><StatusDot status={ag.status} /></span></td>
                          <td><span className="dvc-tool" title={ag.tool+" · "+ag.model} style={{verticalAlign:"middle"}}><ToolBadge tool={ag.tool} size={17} /></span></td>
                          <td><span className="dvc-pchip"><span className="dvc-pn">{p.name}</span><span className="dvc-pi" style={{background:"color-mix(in oklch,"+p.color+",transparent 82%)",boxShadow:"inset 0 0 0 1px color-mix(in oklch,"+p.color+",transparent 62%)"}}><Icon name={p.icon} size="sm" style={{width:11,height:11,color:p.color}} /></span></span></td>
                          <td><span className="dvc-branch">{ag.branch.replace("agent/","")}</span></td>
                          <td className="tnum">{ag.commits}</td>
                          <td className="tnum">{ag.files&&ag.files.length?(<span><span className="dvc-add">+{dl.a}</span> <span className="dvc-del">−{dl.d}</span></span>):<span className="dvc-dim">—</span>}</td>
                          <td className="tnum" style={{color:"var(--ink-3)"}}>{ag.elapsed?fmtDur(ag.elapsed):"—"}</td>
                          <td className="tnum"><span className="dvc-mtr"><i style={{width:Math.round(ag.progress*100)+"%",background:sc}} /></span>{Math.round(ag.progress*100)}%</td>
                        </tr>
                        {isOpen && (
                          <tr className="dvc-detail"><td colSpan={ACOLS.length}><div className="dvc-din">
                            <div className="dvc-task">{ag.task}</div>
                            {attn && note && <div className="dvc-attn"><Icon name="flag" size="sm" />{note}</div>}
                            {!attn && note && <div className="dvc-chip">{note}</div>}
                            <div className="dvc-kv">
                              <div className="r"><span className="k">id</span><span className="vv"><b>{ag.id}</b></span></div>
                              <div className="r"><span className="k">tool</span><span className="vv">{ag.tool} · {ag.model}{ag.effort?" · "+ag.effort:""}</span></div>
                              <div className="r"><span className="k">status</span><span className="vv">{ag.status}{ag.pending&&ag.pending.length?" · "+ag.pending.length+" pending":""}</span></div>
                              <div className="r"><span className="k">branch</span><span className="vv">{ag.branch}</span></div>
                              <div className="r"><span className="k">worktree</span><span className="vv">{ag.worktree}</span></div>
                              <div className="r"><span className="k">base</span><span className="vv">{ag.base}</span></div>
                              <div className="r"><span className="k">commits</span><span className="vv">{ag.commits}</span></div>
                              <div className="r"><span className="k">progress</span><span className="vv">{Math.round(ag.progress*100)}%</span></div>
                            </div>
                            <div><div className="dvc-sl">files · {(ag.files||[]).length}</div><div className="dvc-files">{(ag.files||[]).length?ag.files.map((f,i)=>(<div className="dvc-frow" key={i}><span className={"dvc-fst "+f.state}>{f.state}</span><span className="dvc-fp">{f.path}</span><span className="dvc-fd"><span className="dvc-add">+{f.add}</span><span className="dvc-del">−{f.del}</span></span></div>)):<div className="dvc-dim" style={{fontSize:11}}>no files touched</div>}</div></div>
                          </div></td></tr>
                        )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
            )}

            {/* ── PROJECTS ── */}
            {tab==="projects" && (
              <table className="dvc-tbl">
                <thead><tr>{PRCOLS.map(([k,l])=>(
                  <th key={k} className={pSort.key===k?"srt":""} onClick={()=>clickPSort(k)}>{l}{pSort.key===k && <span className="dvc-arr">{pSort.dir<0?"▼":"▲"}</span>}</th>
                ))}</tr></thead>
                <tbody>
                  {loading
                    ? Array.from({ length: 3 }).map((_, i) => <TableRowSkeleton key={i} lead cols={[ [120,96,138][i%3], 70, 40, 56, 36, 40, 36 ]} />)
                    : sortProjects().map((p)=>{
                    const ags=agentsIn(p.id),needs=ags.filter(a=>dvcAttn(a)).length,isOpen=openPr===p.id,branches=p.branches||[],files=p.files||[];
                    return (
                      <React.Fragment key={p.id}>
                        <tr className={"dvc-row"+(isOpen?" open":"")} onClick={()=>setOpenPr(isOpen?null:p.id)}>
                          <td><span className="dvc-lead"><svg className="dvc-tw" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6"/></svg><span className="dvc-pi" style={{width:18,height:18,borderRadius:5,background:"color-mix(in oklch,"+p.color+",transparent 82%)",boxShadow:"inset 0 0 0 1px color-mix(in oklch,"+p.color+",transparent 62%)"}}><Icon name={p.icon} size="sm" style={{width:12,height:12,color:p.color}} /></span><span className="dvc-nm" style={{color:p.color}}>{p.name}</span></span></td>
                          <td className="dvc-id tnum">{p.id}</td>
                          <td className="tnum" style={{color:"var(--ink-3)"}}>{p.head}</td>
                          <td style={{color:"var(--ink-3)"}}>{p.branch}</td>
                          <td className="tnum">{branches.length}</td>
                          <td className="tnum">{ags.length}{needs>0 && <span style={{color:"var(--st-blocked)",fontWeight:700}}> {needs}!</span>}</td>
                          <td className="tnum">{files.length}</td>
                        </tr>
                        {isOpen && (
                          <tr className="dvc-detail"><td colSpan={PRCOLS.length}><div className="dvc-din">
                            <div className="dvc-kv">
                              <div className="r"><span className="k">id</span><span className="vv"><b>{p.id}</b></span></div>
                              <div className="r"><span className="k">path</span><span className="vv">{p.path}</span></div>
                              <div className="r"><span className="k">repo</span><span className="vv">{p.repo||"—"}</span></div>
                              <div className="r"><span className="k">head</span><span className="vv">{p.head} ({p.branch})</span></div>
                              <div className="r"><span className="k">hasGit</span><span className="vv">{String(p.hasGit)}</span></div>
                              <div className="r"><span className="k">files</span><span className="vv">{files.length}</span></div>
                            </div>
                            <div><div className="dvc-sl">branches · {branches.length}</div><div style={{display:"flex",flexWrap:"wrap",gap:6}}>{branches.map((b)=>(<span className="dvc-chip" key={b}><Icon name="branch" size="sm" style={{width:11,height:11}} />{b}</span>))}</div></div>
                            <div><div className="dvc-sl">agents · {ags.length}</div><div className="dvc-files">{ags.length?ags.map((a)=>(<div className="dvc-frow" style={{gridTemplateColumns:"9px 1fr auto"}} key={a.id}><StatusDot status={a.status} /><span className="dvc-fp">{a.id} · {a.name}</span><span className="dvc-chip"><ToolBadge tool={a.tool} size={13} />{a.status}</span></div>)):<div className="dvc-dim" style={{fontSize:11}}>no agents</div>}</div></div>
                          </div></td></tr>
                        )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
            )}

            {/* ── RESOURCES ── */}
            {tab==="resources" && (
              <React.Fragment>
                <div className="dvc-res-top">
                  <div className="dvc-gauge">
                    <div className="g-top">
                      <span className="g-lab"><Icon name="cpu" size="sm" />CPU</span>
                      <span className="g-val tnum">{machineCpu.toFixed(1)}<small>%</small></span>
                    </div>
                    <div className="dvc-gbar"><i style={{width:dvcClamp(machineCpu,2,100)+"%",background:dvcLatColor(dvcCpuC(machineCpu>70?80:machineCpu<30?10:50))}} /></div>
                    <span className="g-sub tnum">{(sumCpu/100).toFixed(2)} of {DVC_CORES} cores · {procList().length} processes</span>
                  </div>
                  <div className="dvc-gauge">
                    <div className="g-top">
                      <span className="g-lab"><Icon name="database" size="sm" />Memory</span>
                      <span className="g-val tnum">{(sumMem/1024).toFixed(2)}<small> GB</small></span>
                    </div>
                    <div className="dvc-gbar"><i style={{width:dvcClamp(memPctSys,2,100)+"%",background:dvcLatColor(dvcMemC(memPctSys>50?700:memPctSys<25?200:450))}} /></div>
                    <span className="g-sub tnum">{memPctSys.toFixed(1)}% of {(DVC_SYSMEM/1024)} GB · {agentProcs} agent {agentProcs===1?"process":"processes"}</span>
                  </div>
                </div>
                <table className="dvc-tbl">
                  <thead><tr>{RCOLS.map(([k,l])=>(
                    <th key={k} className={rSort.key===k?"srt":""} onClick={()=>k!=="cpuH"&&clickRSort(k)} style={k==="cpuH"?{cursor:"default"}:null}>{l}{rSort.key===k && <span className="dvc-arr">{rSort.dir<0?"▼":"▲"}</span>}</th>
                  ))}</tr></thead>
                  <tbody>
                    {sortProcs().map((p)=>{
                      const cc=dvcCpuC(p.cpu), mc=dvcMemC(p.mem), idle=p.kind==="agent"&&p.status!=="running";
                      return (
                        <tr key={p.key} className={idle?"dvc-exited":""}>
                            <td><span className="dvc-lead" style={{paddingLeft:0}}>
                              {p.kind==="agent"
                                ? <span className="dvc-kind" style={{background:"transparent"}}><ToolBadge tool={p.tool} size={17} /></span>
                                : <span className={"dvc-kind "+p.kind}><Icon name={p.kind==="ui"?"globe":p.kind==="gpu"?"layers":"cpu"} size="sm" /></span>}
                              <span className="dvc-nm">{p.name}</span>
                              <span className="dvc-dim" style={{fontSize:10}}>{p.sub}</span>
                            </span></td>
                            <td className="dvc-pid tnum">{p.pid}</td>
                            <td className={"dvc-lat "+cc+" tnum"}><span className="v">{p.cpu.toFixed(1)}%</span></td>
                            <td className="dvc-spk"><Spark data={p.cpuH} color={dvcLatColor(cc)} w={54} h={16} /></td>
                            <td className={"dvc-lat "+mc+" tnum"}><span className="v">{dvcMem(p.mem)}</span></td>
                            <td className="tnum" style={{color:"var(--ink-3)"}}>{p.threads}</td>
                            <td className="tnum" style={{color:"var(--ink-3)"}}>{dvcUptime(p.start)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </React.Fragment>
            )}
          </div>

          <footer className="dvc-foot">
            {tab==="perf" && <React.Fragment>
              <span className="dvc-leg"><span className="dvc-sw g" />≤16ms</span>
              <span className="dvc-leg"><span className="dvc-sw a" />16–100ms</span>
              <span className="dvc-leg"><span className="dvc-sw r" />&gt;100ms · err</span>
              <span className="dvc-sp" />
              <span className="tnum">{hasCalls ? statsRef.current.filter(s=>s.calls>0).length+" commands · "+statsRef.current.reduce((a,s)=>a+(s.calls||0),0)+" calls/10s · "+statsRef.current.filter(s=>s.rt!=null&&s.rt>100).length+" >100ms" : "0 commands · 0 calls"}{!dev && " · prod (aggregates only)"}</span>
            </React.Fragment>}
            {tab==="agents" && <React.Fragment>
              <span className="tnum">{agents.length} agents · <span style={{color:"var(--st-running)"}}>{cnt("running")} running</span> · <span style={{color:"var(--st-blocked)"}}>{cnt("blocked")} need you</span> · {cnt("waiting")+cnt("queued")} waiting · {cnt("done")} done</span>
              <span className="dvc-sp" /><span className="tnum">live state</span>
            </React.Fragment>}
            {tab==="projects" && <React.Fragment>
              <span className="tnum">{projects.length} projects · {agents.length} worktrees</span>
              <span className="dvc-sp" /><span className="tnum">live state</span>
            </React.Fragment>}
            {tab==="resources" && <React.Fragment>
              <span className="dvc-leg"><span className="dvc-sw g" />idle</span>
              <span className="dvc-leg"><span className="dvc-sw a" />busy</span>
              <span className="dvc-leg"><span className="dvc-sw r" />hot</span>
              <span className="dvc-sp" />
              <span className="tnum">Orrery {(DVC_ORRERY.reduce((a,p)=>{const v=resRef.current.get(p.key);return a+(v?v.cpu:0);},0)).toFixed(1)}% · {procList().length} procs · {machineCpu.toFixed(1)}% / {DVC_CORES} cores · {(sumMem/1024).toFixed(2)} GB</span>
            </React.Fragment>}
          </footer>
        </section>
      )}
    </React.Fragment>
  );
}

Object.assign(window, { DevConsole });
