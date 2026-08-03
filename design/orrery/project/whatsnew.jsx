/* global React, Icon, Logo */
// ORCHESTRA — post-update "What's new" digest + the update simulation surface.
// Built on ORCHESTRA tokens, but echoes the marketing changelog rail: epicycle
// mark, version tag + channel badge, conventional-commit type chips.
//
// Exports (to window): WhatsNewModal, UpdateToast, UpdaterBanner,
//   wnSeenVersion, wnMarkSeen.

const { useState: useWN, useEffect: useWE, useRef: useWR } = React;

// ── seen-tracking (auto-show the digest once per version) ────────────────────
const WN_KEY = "orrery.whatsnew.seen";
function wnSeenVersion() { try { return localStorage.getItem(WN_KEY) || null; } catch (e) { return null; } }
function wnMarkSeen(v) { try { localStorage.setItem(WN_KEY, v); } catch (e) {} }

// ── scoped styles (all .wn- ; ORCHESTRA tokens only) ─────────────────────────
const WN_STYLE = `
.wn-backdrop{position:fixed;inset:0;z-index:85;display:grid;place-items:center;padding:28px;
  background:rgba(4,5,9,.6);-webkit-backdrop-filter:blur(5px);backdrop-filter:blur(5px);}
[data-theme="light"] .wn-backdrop{background:rgba(20,24,40,.36);}

.wn-modal{--feat:#22d3ee;--fix:#34e0a1;--perf:#c084fc;--refactor:#a855f7;--chore:#8b94a8;
  width:544px;max-width:calc(100vw - 48px);max-height:86vh;display:flex;flex-direction:column;
  background:var(--panel);border:1px solid var(--hair-2);border-radius:16px;overflow:hidden;
  box-shadow:var(--shadow),0 0 0 1px rgba(var(--accent-rgb),.05);
  font-family:var(--font-mono);color:var(--ink);transform-origin:center;
  animation:wn-pop .26s cubic-bezier(.2,.7,.2,1);}
/* transform-only entrance: if the frame is throttled and the animation freezes
   at 0%, the modal stays fully visible (just offset) instead of stuck transparent */
@keyframes wn-pop{from{transform:translateY(12px) scale(.985)}to{transform:none}}
@media (prefers-reduced-motion:reduce){.wn-modal,.wn-backdrop{animation:none}}

/* hero */
.wn-hero{position:relative;flex:none;padding:24px 26px 20px;border-bottom:1px solid var(--hair);overflow:hidden;}
.wn-hero::before{content:"";position:absolute;inset:0;pointer-events:none;
  background:radial-gradient(120% 92% at 16% -16%,color-mix(in oklch,var(--accent),transparent 80%),transparent 66%);}
.wn-hero::after{content:"";position:absolute;left:0;right:0;top:0;height:2px;
  background:linear-gradient(90deg,#ff5d9e,var(--accent),var(--accent-2));opacity:.85;}
.wn-hero-row{position:relative;display:flex;align-items:flex-start;gap:16px;}
.wn-mark{flex:none;filter:drop-shadow(0 0 18px rgba(var(--accent-rgb),.42));margin-top:2px;}
.wn-eyebrow{display:inline-flex;align-items:center;gap:7px;font-size:9.5px;letter-spacing:.18em;
  text-transform:uppercase;color:var(--ink-3);}
.wn-eyebrow .d{width:6px;height:6px;border-radius:50%;background:var(--accent-2);
  box-shadow:0 0 8px 1px color-mix(in oklch,var(--accent-2),transparent 30%);}
.wn-h1{font-family:var(--font-disp);font-weight:600;font-size:23px;letter-spacing:-.02em;
  line-height:1.05;margin-top:9px;}
.wn-verline{display:flex;align-items:center;gap:9px;flex-wrap:wrap;margin-top:11px;}
.wn-ver{font-family:var(--font-disp);font-weight:600;font-size:15px;letter-spacing:-.01em;
  color:var(--accent);font-variant-numeric:tabular-nums;}
.wn-badge{font-size:8.5px;font-weight:700;letter-spacing:.12em;padding:2px 7px;border-radius:999px;}
.wn-badge.beta{color:var(--accent-2);border:1px solid color-mix(in oklch,var(--accent-2),transparent 56%);
  background:color-mix(in oklch,var(--accent-2),transparent 88%);}
.wn-badge.dev{color:#f5c451;border:1px solid color-mix(in oklch,#f5c451,transparent 56%);
  background:color-mix(in oklch,#f5c451,transparent 88%);}
.wn-from{font-size:10.5px;color:var(--ink-4);font-variant-numeric:tabular-nums;}
.wn-x{position:absolute;top:0;right:0;width:28px;height:28px;border-radius:7px;border:1px solid transparent;
  background:transparent;color:var(--ink-3);cursor:pointer;display:grid;place-items:center;transition:all .12s;}
.wn-x:hover{background:var(--panel-3);color:var(--ink);border-color:var(--hair);}
.wn-summary{position:relative;font-size:13px;line-height:1.6;color:var(--ink-2);margin-top:16px;
  text-wrap:pretty;max-width:44ch;}

/* commit list */
.wn-body{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;padding:6px 20px 14px;}
.wn-body::-webkit-scrollbar{width:9px;}
.wn-body::-webkit-scrollbar-thumb{background:var(--hair-2);border-radius:6px;border:2px solid transparent;background-clip:padding-box;}
.wn-body::-webkit-scrollbar-thumb:hover{background:var(--ink-4);background-clip:padding-box;}
.wn-grp{font-size:9px;letter-spacing:.13em;text-transform:uppercase;color:var(--ink-4);
  padding:14px 6px 5px;}
.wn-commit{display:grid;grid-template-columns:auto 1fr;gap:11px;align-items:baseline;
  padding:9px 6px;border-bottom:1px solid rgba(255,255,255,.045);}
[data-theme="light"] .wn-commit{border-bottom-color:rgba(0,0,0,.06);}
.wn-commit:last-child{border-bottom:none;}
.wn-type{font-size:9px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;
  padding:3px 7px;border-radius:5px;white-space:nowrap;align-self:start;margin-top:1px;font-variant-numeric:tabular-nums;}
.wn-type.feat{color:var(--feat);background:color-mix(in oklch,var(--feat),transparent 86%);}
.wn-type.fix{color:var(--fix);background:color-mix(in oklch,var(--fix),transparent 86%);}
.wn-type.perf{color:var(--perf);background:color-mix(in oklch,var(--perf),transparent 84%);}
.wn-type.refactor{color:var(--refactor);background:color-mix(in oklch,var(--refactor),transparent 84%);}
.wn-type.chore{color:var(--chore);background:color-mix(in oklch,var(--chore),transparent 84%);}
.wn-msg{font-size:13px;line-height:1.5;color:var(--ink);text-wrap:pretty;}
.wn-msg .scope{color:var(--accent);}
.wn-msg .by{color:var(--ink-4);margin-left:6px;font-size:11px;}

/* multi-release sections */
.wn-rel + .wn-rel{border-top:1px solid var(--hair);}
.wn-rel-head{position:sticky;top:0;z-index:1;display:flex;align-items:center;gap:9px;flex-wrap:wrap;
  padding:13px 6px 9px;background:linear-gradient(var(--panel) 78%,transparent);}
.wn-rel-tag{font-family:var(--font-disp);font-weight:600;font-size:16px;letter-spacing:-.01em;
  color:var(--ink);font-variant-numeric:tabular-nums;}
.wn-rel-date{font-size:10.5px;color:var(--ink-4);margin-left:auto;font-variant-numeric:tabular-nums;}
.wn-rel-sum{font-size:12px;line-height:1.55;color:var(--ink-2);padding:0 6px 6px;text-wrap:pretty;}

/* footer */
.wn-foot{flex:none;display:flex;align-items:center;gap:12px;padding:13px 20px;
  border-top:1px solid var(--hair);background:var(--panel-2);}
.wn-link{display:inline-flex;align-items:center;gap:6px;font-size:11.5px;color:var(--ink-3);
  text-decoration:none;transition:color .12s;}
.wn-link:hover{color:var(--accent-2);}
.wn-link svg{width:13px;height:13px;}
.wn-sp{flex:1;}
.wn-done{display:inline-flex;align-items:center;gap:7px;height:32px;padding:0 18px;border-radius:8px;
  border:none;font-family:var(--font-mono);font-size:12px;font-weight:600;cursor:pointer;color:#06070b;
  background:linear-gradient(180deg,var(--accent),color-mix(in oklch,var(--accent),#000 14%));
  box-shadow:0 0 18px -7px rgba(var(--accent-rgb),.7);transition:filter .12s;}
[data-theme="light"] .wn-done{color:#fff;}
.wn-done:hover{filter:brightness(1.07);}

/* ── update toast (available) ── */
.wn-toast{position:fixed;left:50%;bottom:46px;transform:translateX(-50%);z-index:84;
  display:flex;align-items:center;gap:14px;width:380px;max-width:calc(100vw - 32px);
  padding:13px 14px 13px 16px;border-radius:13px;font-family:var(--font-mono);color:var(--ink);
  background:var(--panel);border:1px solid var(--hair-2);
  box-shadow:var(--shadow),0 0 34px -12px rgba(var(--accent-rgb),.4),0 0 0 1px rgba(var(--accent-rgb),.05);
  animation:wn-rise .4s cubic-bezier(.2,.8,.2,1);}
@keyframes wn-rise{from{transform:translate(-50%,14px)}to{transform:translate(-50%,0)}}
.wn-toast .ic{flex:none;width:34px;height:34px;border-radius:9px;display:grid;place-items:center;color:var(--accent);
  background:color-mix(in oklch,var(--accent),transparent 86%);
  box-shadow:inset 0 0 0 1px color-mix(in oklch,var(--accent),transparent 58%);}
.wn-toast .col{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px;}
.wn-toast .t1{font-size:12.5px;color:var(--ink);font-weight:500;display:flex;align-items:center;gap:7px;}
.wn-toast .t1 b{color:var(--accent);font-weight:600;font-variant-numeric:tabular-nums;}
.wn-toast .t2{font-size:10px;color:var(--ink-4);font-variant-numeric:tabular-nums;}
.wn-toast .act{flex:none;display:flex;align-items:center;gap:7px;}
.wn-tbtn{display:inline-flex;align-items:center;gap:6px;height:28px;padding:0 12px;border-radius:7px;
  font-family:var(--font-mono);font-size:11px;font-weight:600;cursor:pointer;border:none;color:#06070b;
  background:linear-gradient(180deg,var(--accent),color-mix(in oklch,var(--accent),#000 14%));
  box-shadow:0 0 16px -7px rgba(var(--accent-rgb),.8);transition:filter .12s;}
[data-theme="light"] .wn-tbtn{color:#fff;}
.wn-tbtn:hover{filter:brightness(1.08);}
.wn-tbtn svg{width:12px;height:12px;}
.wn-tlater{background:transparent;border:none;color:var(--ink-4);font-family:var(--font-mono);
  font-size:10.5px;cursor:pointer;padding:4px 6px;transition:color .12s;}
.wn-tlater:hover{color:var(--ink-2);}

/* ── installer banner (frameless mini-window) ── */
.wn-inst-wrap{position:fixed;inset:0;z-index:86;display:grid;place-items:center;
  background:rgba(4,5,9,.55);-webkit-backdrop-filter:blur(3px);backdrop-filter:blur(3px);}
.wn-inst{width:344px;border-radius:14px;position:relative;overflow:hidden;padding:18px 20px;
  display:flex;align-items:center;gap:15px;background:#0b0d14;
  background-image:radial-gradient(70% 120% at 14% 0%,rgba(var(--accent-rgb),.18),transparent 70%);
  border:1px solid rgba(255,255,255,.08);
  box-shadow:0 24px 60px -16px rgba(0,0,0,.85),0 0 36px -10px rgba(var(--accent-rgb),.34);
  animation:wn-pop .3s cubic-bezier(.2,.7,.2,1);}
.wn-inst.closing{opacity:0;transform:scale(.97) translateY(-4px);transition:opacity .5s ease,transform .5s ease;}
.wn-inst .lg{flex:none;filter:drop-shadow(0 0 12px rgba(var(--accent-rgb),.42));}
.wn-inst .col{flex:1;min-width:0;display:flex;flex-direction:column;gap:9px;}
.wn-inst .ttl{font-family:var(--font-disp);font-weight:600;font-size:14.5px;color:#e8ebf2;line-height:1.1;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.wn-inst .srow{display:flex;align-items:baseline;justify-content:space-between;gap:10px;min-height:13px;}
.wn-inst .st{font-size:10px;letter-spacing:.14em;text-transform:uppercase;color:#6b7488;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis;}
.wn-inst .pct{font-size:10.5px;font-weight:500;color:#e8ebf2;font-variant-numeric:tabular-nums;flex:none;}
.wn-bar{height:3px;border-radius:3px;background:rgba(255,255,255,.08);overflow:hidden;position:relative;}
.wn-bar>i{display:block;height:100%;border-radius:3px;width:0;
  background:linear-gradient(90deg,#ff5d9e,#a855f7,#22d3ee);transition:width .25s linear;
  box-shadow:0 0 10px -1px rgba(168,85,247,.6);}
.wn-bar.shim>i{width:100%;background:rgba(168,85,247,.16);box-shadow:none;}
.wn-bar.shim::after{content:"";position:absolute;top:0;bottom:0;left:0;width:55%;border-radius:3px;
  background:linear-gradient(90deg,transparent,rgba(168,85,247,.55),rgba(34,211,238,.45),transparent);
  animation:wn-shim 1.4s ease-in-out infinite;}
@keyframes wn-shim{0%{transform:translateX(-100%)}100%{transform:translateX(282%)}}
.wn-bar.pulse>i{width:100%;animation:wn-bp 1s ease-in-out infinite;}
@keyframes wn-bp{0%,100%{filter:brightness(1)}50%{filter:brightness(1.5) saturate(1.2)}}
.wn-inst .pulsing{transform-box:fill-box;transform-origin:50% 50%;animation:wn-corepulse 2.2s ease-in-out infinite;}
@keyframes wn-corepulse{0%,100%{opacity:1}50%{opacity:.6}}
`;

// type → ordering weight for grouping the digest (features first)
const WN_ORDER = { feat: 0, fix: 1, perf: 2, refactor: 3, chore: 4 };

// semver-ish compare on "0.9.4" / "v0.9.4" → 1 if a>b, -1 if a<b, 0 equal
function wnCmpVer(a, b) {
  const pa = String(a).replace(/^v/, "").split(".").map(Number);
  const pb = String(b).replace(/^v/, "").split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d) return d > 0 ? 1 : -1;
  }
  return 0;
}
// every release strictly newer than fromVersion (newest first); never empty
function wnReleasesSince(fromVersion) {
  const rels = window.RELEASES || [];
  const missed = rels.filter((r) => wnCmpVer(r.tag, fromVersion) > 0);
  return missed.length ? missed : rels.slice(0, 1);
}

// ── What's new modal ─────────────────────────────────────────────────────────
// Shows every release strictly newer than `fromVersion` (newest first) so the
// user catches up on everything since their build — not just the latest tag.
function WhatsNewModal({ releases, fromVersion, onClose }) {
  useWE(() => {
    const onKey = (e) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  if (!releases || !releases.length) return null;
  const latest = releases[0];
  const lchan = latest.channel === "beta" ? "beta" : "dev";
  const multi = releases.length > 1;
  const sortC = (cs) => (cs || []).slice().sort((a, b) => (WN_ORDER[a.type] ?? 9) - (WN_ORDER[b.type] ?? 9));
  const commitRow = (c, k) => (
    <div className="wn-commit" key={k}>
      <span className={"wn-type " + c.type}>{c.type}</span>
      <span className="wn-msg">
        {c.scope && <span className="scope">{c.scope}: </span>}{c.msg}
        {c.by && <span className="by">{c.by}</span>}
      </span>
    </div>
  );

  return (
    <>
      <style>{WN_STYLE}</style>
      <div className="wn-backdrop" onMouseDown={onClose}>
        <div className="wn-modal" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="What's new">
          <div className="wn-hero">
            <button className="wn-x" onClick={onClose} aria-label="Close"><Icon name="x" size="sm" /></button>
            <div className="wn-hero-row">
              <span className="wn-mark"><Logo size={44} /></span>
              <div style={{ minWidth: 0 }}>
                <span className="wn-eyebrow"><span className="d" />Updated · just now</span>
                <div className="wn-h1">What’s new in Orrery</div>
                <div className="wn-verline">
                  <span className="wn-ver">{latest.tag}</span>
                  <span className={"wn-badge " + lchan}>{lchan === "beta" ? "BETA" : "DEV"}</span>
                  {fromVersion && <span className="wn-from">from v{fromVersion} · {multi ? releases.length + " releases" : latest.date}</span>}
                </div>
              </div>
            </div>
            {!multi && latest.summary && <p className="wn-summary">{latest.summary}</p>}
          </div>

          <div className="wn-body">
            {multi
              ? releases.map((rel) => {
                  const ch = rel.channel === "beta" ? "beta" : "dev";
                  return (
                    <section className="wn-rel" key={rel.tag}>
                      <div className="wn-rel-head">
                        <span className="wn-rel-tag">{rel.tag}</span>
                        <span className={"wn-badge " + ch}>{ch === "beta" ? "BETA" : "DEV"}</span>
                        <span className="wn-rel-date">{rel.date}</span>
                      </div>
                      {rel.summary && <p className="wn-rel-sum">{rel.summary}</p>}
                      {sortC(rel.commits).map(commitRow)}
                    </section>
                  );
                })
              : (
                <>
                  <div className="wn-grp">Highlights · {sortC(latest.commits).length} changes</div>
                  {sortC(latest.commits).map(commitRow)}
                </>
              )}
          </div>

          <div className="wn-foot">
            <a className="wn-link" href="changelog.html" target="_blank" rel="noopener">
              <Icon name="file" size="sm" />View full changelog<Icon name="ext" size="sm" />
            </a>
            <span className="wn-sp" />
            <button className="wn-done" onClick={onClose}><Icon name="check" size="sm" />Continue</button>
          </div>
        </div>
      </div>
    </>
  );
}

// ── update-available toast ────────────────────────────────────────────────────
function UpdateToast({ fromVersion, toVersion, size, onInstall, onLater }) {
  return (
    <>
      <style>{WN_STYLE}</style>
      <div className="wn-toast" role="status">
        <span className="ic"><Icon name="rocket" /></span>
        <div className="col">
          <div className="t1">Update available · <b>v{toVersion}</b></div>
          <div className="t2">from v{fromVersion}{size ? " · " + size : ""}</div>
        </div>
        <div className="act">
          <button className="wn-tlater" onClick={onLater}>Later</button>
          <button className="wn-tbtn" onClick={onInstall}><Icon name="stage" size="sm" />Install</button>
        </div>
      </div>
    </>
  );
}

// ── installer banner — preparing → installing 0→100 → restarting → onDone ─────
function UpdaterBanner({ toVersion, onDone }) {
  const [phase, setPhase] = useWN("prepare"); // prepare | install | restart | closing
  const [pct, setPct] = useWN(0);
  const [status, setStatus] = useWN("preparing…");
  const timers = useWR([]);

  useWE(() => {
    const T = (fn, ms) => { const id = setTimeout(fn, ms); timers.current.push(id); return id; };
    const reduce = window.matchMedia && matchMedia("(prefers-reduced-motion:reduce)").matches;
    if (reduce) {
      setPhase("install"); setStatus("copying files"); setPct(100);
      T(() => { setPhase("closing"); T(onDone, 600); }, 1100);
      return () => timers.current.forEach(clearTimeout);
    }
    // 1) preparing
    T(() => {
      setPhase("install");
      const steps = [[30, "copying files"], [62, "registering components"], [86, "updating shortcuts"], [100, "finishing up"]];
      const start = performance.now(), DUR = 3200; let last = 0, raf;
      const frame = (now) => {
        const p = Math.min(1, (now - start) / DUR);
        const v = Math.max(last, p * 100); last = v;
        setPct(v);
        const msg = (steps.find(([t]) => v <= t) || steps[steps.length - 1])[1];
        setStatus(msg);
        if (p < 1) raf = requestAnimationFrame(frame);
        else {
          setStatus("restarting Orrery…"); setPhase("restart");
          T(() => { setPhase("closing"); T(onDone, 600); }, 1150);
        }
      };
      raf = requestAnimationFrame(frame);
      timers.current.push(() => cancelAnimationFrame(raf));
    }, 900);
    return () => timers.current.forEach((t) => (typeof t === "function" ? t() : clearTimeout(t)));
  }, []);

  const barCls = phase === "prepare" ? "wn-bar shim" : phase === "restart" ? "wn-bar pulse" : "wn-bar";
  const title = phase === "prepare" ? "Updating Orrery" : "Updating to v" + toVersion;

  return (
    <>
      <style>{WN_STYLE}</style>
      <div className="wn-inst-wrap">
        <div className={"wn-inst" + (phase === "closing" ? " closing" : "")}>
          <span className="lg"><span className="pulsing" style={{ display: "inline-flex" }}><Logo size={44} /></span></span>
          <div className="col">
            <div className="ttl">{title}</div>
            <div className="srow">
              <span className="st">{status}</span>
              {phase === "install" && <span className="pct">{Math.round(pct)}%</span>}
            </div>
            <div className={barCls}><i style={{ width: phase === "install" ? pct + "%" : undefined }} /></div>
          </div>
        </div>
      </div>
    </>
  );
}

Object.assign(window, { WhatsNewModal, UpdateToast, UpdaterBanner, wnSeenVersion, wnMarkSeen, wnReleasesSince });
