/* ============================================================
   Orrery landing — motion & the live orrery canvas
   Vanilla JS, zero deps. Respects prefers-reduced-motion.
   ============================================================ */
(() => {
  "use strict";
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];

  /* agents — the orbiting bodies */
  const AGENTS = [
    { name: "Claude", tag: "anthropic", color: "#d8865b", orbit: 0.46, speed: 0.34, r: 7,   phase: 0.4 },
    { name: "Codex",  tag: "openai",    color: "#3ecf8e", orbit: 0.62, speed: 0.24, r: 6,   phase: 2.1 },
    { name: "Cursor", tag: "editor",    color: "#e7e3d8", orbit: 0.78, speed: 0.17, r: 5.5, phase: 3.8 },
    { name: "Gemini", tag: "google",    color: "#6f9bff", orbit: 0.95, speed: 0.12, r: 6.5, phase: 5.3 },
  ];

  /* ── year ── */
  const yr = $("#year"); if (yr) yr.textContent = new Date().getFullYear();

  /* ── legend + marquee ── */
  const legend = $("#orreryLegend");
  if (legend) legend.innerHTML = AGENTS.map(a =>
    `<span><i style="background:${a.color};box-shadow:0 0 8px ${a.color}"></i>${a.name}</span>`).join("");

  const marquee = $("#marquee");
  if (marquee) {
    const one = AGENTS.map(a =>
      `<span class="m-item"><span class="m-dot" style="background:${a.color};color:${a.color}"></span>${a.name}<span class="m-tag">${a.tag}</span></span>`
    ).join("");
    marquee.innerHTML = one + one; // duplicate for seamless -50% loop
  }

  /* ── nav scroll state ── */
  const nav = $("#nav");
  const onScroll = () => nav && nav.classList.toggle("scrolled", window.scrollY > 24);
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  /* ── reveal on scroll ── */
  const io = new IntersectionObserver((entries) => {
    entries.forEach(e => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { threshold: 0.12, rootMargin: "0px 0px -8% 0px" });
  $$("[data-reveal]").forEach(el => io.observe(el));

  /* stagger the hero on load */
  const heroBits = [$(".eyebrow"), ...$$(".hero-title .line"), $(".hero-sub"), $(".hero-actions"), $(".hero-meta")].filter(Boolean);
  heroBits.forEach((el, i) => { el.style.transitionDelay = `${0.08 * i + 0.1}s`; });

  /* ── animated counters ── */
  const counters = $$("[data-count]");
  const cio = new IntersectionObserver((entries) => {
    entries.forEach(e => {
      if (!e.isIntersecting) return;
      const el = e.target, target = +el.dataset.count, suf = el.dataset.suffix || "";
      if (reduce) { el.textContent = target + suf; cio.unobserve(el); return; }
      const dur = 1100, t0 = performance.now();
      const tick = (t) => {
        const p = Math.min(1, (t - t0) / dur);
        const eased = 1 - Math.pow(1 - p, 3);
        el.textContent = Math.round(target * eased) + suf;
        if (p < 1) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
      cio.unobserve(el);
    });
  }, { threshold: 0.6 });
  counters.forEach(c => cio.observe(c));

  /* ── terminal typing loop ── */
  const termEl = $("#termType");
  if (termEl && !reduce) {
    const lines = ["spawn cursor --branch fix/auth", "review diff auth.ts", "approve · run tests", "ship 🛰"];
    let li = 0, ci = 0, deleting = false;
    const loop = () => {
      const full = lines[li];
      termEl.textContent = full.slice(0, ci);
      if (!deleting && ci < full.length) { ci++; setTimeout(loop, 55 + Math.random() * 50); }
      else if (!deleting && ci === full.length) { deleting = true; setTimeout(loop, 1400); }
      else if (deleting && ci > 0) { ci--; setTimeout(loop, 26); }
      else { deleting = false; li = (li + 1) % lines.length; setTimeout(loop, 320); }
    };
    loop();
  } else if (termEl) { termEl.textContent = "spawn cursor --branch fix/auth"; }

  /* ── cursor lens ── */
  const lens = $(".cursor-lens");
  if (lens && window.matchMedia("(pointer:fine)").matches) {
    document.body.classList.add("has-pointer");
    let lx = innerWidth / 2, ly = innerHeight / 2, tx = lx, ty = ly;
    addEventListener("pointermove", (e) => { tx = e.clientX; ty = e.clientY; }, { passive: true });
    const follow = () => { lx += (tx - lx) * .15; ly += (ty - ly) * .15; lens.style.transform = `translate(${lx}px,${ly}px)`; requestAnimationFrame(follow); };
    follow();
  }

  /* ── magnetic tilt ── */
  if (window.matchMedia("(pointer:fine)").matches && !reduce) {
    $$("[data-tilt]").forEach(card => {
      const max = card.classList.contains("hero-orrery") ? 10 : 6;
      card.addEventListener("pointermove", (e) => {
        const r = card.getBoundingClientRect();
        const px = (e.clientX - r.left) / r.width, py = (e.clientY - r.top) / r.height;
        card.style.transform = `perspective(900px) rotateX(${(0.5 - py) * max}deg) rotateY(${(px - 0.5) * max}deg) translateZ(0)`;
        card.style.setProperty("--mx", `${px * 100}%`);
        card.style.setProperty("--my", `${py * 100}%`);
      });
      card.addEventListener("pointerleave", () => { card.style.transform = ""; });
    });
  }

  /* ════════════════════════════════════════════
     The live orrery
     ════════════════════════════════════════════ */
  const canvas = $("#orrery");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  let W = 0, H = 0, cx = 0, cy = 0, R = 0, dpr = Math.min(2, window.devicePixelRatio || 1);
  const TILT = 0.42;            // y-squash → 3/4 perspective
  let stars = [];
  let mx = 0, my = 0;           // parallax target (-1..1)
  let pmx = 0, pmy = 0;

  function resize() {
    const rect = canvas.getBoundingClientRect();
    W = rect.width; H = rect.height;
    canvas.width = W * dpr; canvas.height = H * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    cx = W / 2; cy = H / 2; R = Math.min(W, H) / 2 - 12;
    stars = Array.from({ length: 90 }, () => ({
      x: Math.random() * W, y: Math.random() * H,
      z: Math.random() * 0.8 + 0.2, s: Math.random() * 1.3 + 0.2,
      tw: Math.random() * Math.PI * 2,
    }));
  }
  resize();
  addEventListener("resize", resize);

  const orreryWrap = $(".hero-orrery");
  if (orreryWrap && window.matchMedia("(pointer:fine)").matches) {
    addEventListener("pointermove", (e) => {
      mx = (e.clientX / innerWidth - 0.5) * 2;
      my = (e.clientY / innerHeight - 0.5) * 2;
    }, { passive: true });
  }

  // elliptical orbit point (with perspective tilt). returns {x,y, depth} depth in -1..1 (front=+)
  function orbitPoint(orbit, ang) {
    const rx = R * orbit, ry = R * orbit * TILT;
    return { x: cx + Math.cos(ang) * rx, y: cy + Math.sin(ang) * ry, depth: Math.sin(ang) };
  }

  function drawRing(orbit) {
    const rx = R * orbit, ry = R * orbit * TILT;
    ctx.beginPath();
    ctx.ellipse(cx, cy, rx, ry, 0, 0, Math.PI * 2);
    ctx.strokeStyle = "rgba(232,200,120,0.10)";
    ctx.lineWidth = 1;
    ctx.stroke();
  }

  function drawAgent(a, t) {
    const ang = a.phase + t * a.speed;
    const p = orbitPoint(a.orbit, ang);
    // scale a touch by depth for fake 3D
    const ds = 0.78 + (p.depth + 1) * 0.22;
    const rad = a.r * ds;

    // filament from core (flickers)
    const flick = 0.18 + 0.16 * (0.5 + 0.5 * Math.sin(t * 2.3 + a.phase * 3));
    const grad = ctx.createLinearGradient(cx, cy, p.x, p.y);
    grad.addColorStop(0, "rgba(232,200,120,0)");
    grad.addColorStop(1, hexA(a.color, flick));
    ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(p.x, p.y);
    ctx.strokeStyle = grad; ctx.lineWidth = 1; ctx.stroke();

    // trailing arc
    ctx.beginPath();
    const rx = R * a.orbit, ry = R * a.orbit * TILT;
    ctx.ellipse(cx, cy, rx, ry, 0, ang - 0.9, ang);
    const tg = ctx.createLinearGradient(p.x - 40, p.y, p.x, p.y);
    tg.addColorStop(0, hexA(a.color, 0));
    tg.addColorStop(1, hexA(a.color, 0.55));
    ctx.strokeStyle = tg; ctx.lineWidth = 1.6; ctx.stroke();

    // glow
    const g = ctx.createRadialGradient(p.x, p.y, 0, p.x, p.y, rad * 4.2);
    g.addColorStop(0, hexA(a.color, 0.55));
    g.addColorStop(1, hexA(a.color, 0));
    ctx.fillStyle = g;
    ctx.beginPath(); ctx.arc(p.x, p.y, rad * 4.2, 0, Math.PI * 2); ctx.fill();

    // body
    ctx.beginPath(); ctx.arc(p.x, p.y, rad, 0, Math.PI * 2);
    ctx.fillStyle = a.color; ctx.fill();
    ctx.lineWidth = 1; ctx.strokeStyle = "rgba(255,255,255,.5)"; ctx.stroke();

    return { p, ds, ang };
  }

  function drawCore(t) {
    const pulse = 1 + Math.sin(t * 1.4) * 0.06;
    const cr = R * 0.085 * pulse;
    // halo
    const halo = ctx.createRadialGradient(cx, cy, 0, cx, cy, cr * 7);
    halo.addColorStop(0, "rgba(246,207,118,0.5)");
    halo.addColorStop(0.4, "rgba(232,176,75,0.16)");
    halo.addColorStop(1, "rgba(232,176,75,0)");
    ctx.fillStyle = halo;
    ctx.beginPath(); ctx.arc(cx, cy, cr * 7, 0, Math.PI * 2); ctx.fill();
    // core
    const core = ctx.createRadialGradient(cx - cr * .3, cy - cr * .3, 0, cx, cy, cr);
    core.addColorStop(0, "#fff4d6");
    core.addColorStop(0.5, "#f6cf76");
    core.addColorStop(1, "#b9842c");
    ctx.fillStyle = core;
    ctx.beginPath(); ctx.arc(cx, cy, cr, 0, Math.PI * 2); ctx.fill();
  }

  function drawStars(t) {
    for (const s of stars) {
      const ox = pmx * 14 * s.z, oy = pmy * 14 * s.z;
      const tw = 0.4 + 0.6 * (0.5 + 0.5 * Math.sin(t * 1.5 + s.tw));
      ctx.globalAlpha = tw * s.z;
      ctx.fillStyle = s.z > 0.7 ? "rgba(232,200,150,1)" : "rgba(200,210,255,1)";
      ctx.beginPath(); ctx.arc(s.x + ox, s.y + oy, s.s, 0, Math.PI * 2); ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function hexA(hex, a) {
    const n = parseInt(hex.slice(1), 16);
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
  }

  let t0 = null;
  function frame(now) {
    if (t0 === null) t0 = now;
    const t = (now - t0) / 1000;
    pmx += (mx - pmx) * 0.05; pmy += (my - pmy) * 0.05;

    ctx.clearRect(0, 0, W, H);
    drawStars(t);
    AGENTS.forEach(a => drawRing(a.orbit));
    drawCore(t);

    // sort agents by depth so front ones draw last
    const order = AGENTS.map(a => ({ a, d: Math.sin(a.phase + t * a.speed) })).sort((p, q) => p.d - q.d);
    order.forEach(o => drawAgent(o.a, t));

    requestAnimationFrame(frame);
  }

  if (reduce) {
    // single static frame
    ctx.clearRect(0, 0, W, H);
    drawStars(0); AGENTS.forEach(a => drawRing(a.orbit)); drawCore(0);
    AGENTS.forEach(a => drawAgent(a, 0));
  } else {
    requestAnimationFrame(frame);
  }
})();
