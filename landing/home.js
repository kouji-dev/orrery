/* Orrery home — interactions, epicycle marks, console mount. Dependency-free. */
(function () {
  "use strict";
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const reduce = () => window.matchMedia && matchMedia("(prefers-reduced-motion:reduce)").matches;

  // ---- epicycle (rosette) geometry — shared with the app splash ----
  const A = 22, B = 13, K = -4, TAU = Math.PI * 2;
  const epi = (t) => [50 + A * Math.cos(t) + B * Math.cos(K * t), 50 + A * Math.sin(t) + B * Math.sin(K * t)];
  function epiPath(n) { let d = ""; for (let i = 0; i <= n; i++) { const p = epi((i / n) * TAU); d += (i ? "L" : "M") + p[0].toFixed(2) + " " + p[1].toFixed(2) + " "; } return d + "Z"; }
  const EPI_D = epiPath(260);
  let uid = 0;

  // big hero/band rosette; animated draws the stroke + orbiting arm once on view
  function epicycleLogo(host, { size = 64, animated = false, strokeW = 3, glow = true } = {}) {
    const id = ++uid;
    host.innerHTML =
      `<svg width="${size}" height="${size}" viewBox="0 0 100 100" aria-label="Orrery" style="display:block;${glow ? `filter:drop-shadow(0 0 ${size * 0.22}px rgba(168,85,247,.42))` : ""}">
        <defs>
          <linearGradient id="g${id}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/></linearGradient>
          <radialGradient id="c${id}"><stop offset="0" stop-color="#fff"/><stop offset=".42" stop-color="#a855f7"/><stop offset="1" stop-color="#a855f7" stop-opacity="0"/></radialGradient>
        </defs>
        <circle cx="50" cy="50" r="22" fill="none" stroke="#6b7488" stroke-width=".7" stroke-opacity=".3"/>
        <path class="p" d="${EPI_D}" fill="none" stroke="url(#g${id})" stroke-width="${strokeW}" stroke-linejoin="round" stroke-linecap="round"/>
        <line class="a" stroke="#22d3ee" stroke-width="1.2" stroke-opacity=".6" style="opacity:0"/>
        <circle cx="50" cy="50" r="8.5" fill="url(#c${id})" style="transform-box:fill-box;transform-origin:50% 50%;animation:ol-pulse 2.4s ease-in-out infinite"/>
        <circle class="n" r="4.2" fill="#22d3ee" cx="${epi(0)[0]}" cy="${epi(0)[1]}"/>
      </svg>`;
    if (!animated || reduce()) return;
    const svg = host.firstElementChild, path = svg.querySelector(".p"), arm = svg.querySelector(".a"), pen = svg.querySelector(".n");
    const L = path.getTotalLength(), ease = (x) => 1 - Math.pow(1 - x, 3);
    let raf, t1;
    const loop = () => {
      const DUR = 2400, HOLD = 1300, start = performance.now();
      path.style.strokeDasharray = L; path.style.strokeDashoffset = L; arm.style.transition = "none"; arm.style.opacity = 1;
      const frame = (now) => {
        const p = Math.min(1, (now - start) / DUR), e = ease(p);
        path.style.strokeDashoffset = L * (1 - e);
        const t = e * TAU, def = [50 + A * Math.cos(t), 50 + A * Math.sin(t)], pp = epi(t);
        pen.setAttribute("cx", pp[0]); pen.setAttribute("cy", pp[1]);
        arm.setAttribute("x1", def[0]); arm.setAttribute("y1", def[1]); arm.setAttribute("x2", pp[0]); arm.setAttribute("y2", pp[1]);
        if (p < 1) raf = requestAnimationFrame(frame);
        else { arm.style.transition = "opacity .35s"; arm.style.opacity = 0; const s = epi(0); pen.setAttribute("cx", s[0]); pen.setAttribute("cy", s[1]); t1 = setTimeout(loop, HOLD); }
      };
      raf = requestAnimationFrame(frame);
    };
    loop();
    host._stop = () => { cancelAnimationFrame(raf); clearTimeout(t1); };
  }

  // small flat mark for nav/footer
  function miniMark(host, size = 22, sw = 6) {
    const id = ++uid;
    host.innerHTML = `<svg width="${size}" height="${size}" viewBox="0 0 100 100"><defs><linearGradient id="m${id}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/></linearGradient><radialGradient id="mc${id}"><stop offset="0" stop-color="#fff"/><stop offset=".45" stop-color="#a855f7"/><stop offset="1" stop-color="#a855f7" stop-opacity="0"/></radialGradient></defs><path d="${EPI_D}" fill="none" stroke="url(#m${id})" stroke-width="${sw}" stroke-linejoin="round"/><circle cx="50" cy="50" r="13" fill="url(#mc${id})"/></svg>`;
  }

  document.addEventListener("DOMContentLoaded", () => {
    // marks
    $$("[data-minimark]").forEach((el) => miniMark(el, +el.dataset.minimark || 22));
    const hero = $("#heroLogo"); if (hero) epicycleLogo(hero, { size: 200, animated: true, strokeW: 2.6 });
    const band = $("#bandLogo"); if (band) epicycleLogo(band, { size: 64, animated: true });

    // nav scrolled state
    const nav = $("#nav");
    const onScroll = () => nav && nav.classList.toggle("scrolled", window.scrollY > 24);
    onScroll(); window.addEventListener("scroll", onScroll, { passive: true });

    // mobile menu
    const burger = $("#burger"), menu = $("#navMenu");
    if (burger && menu) {
      const toggle = (open) => { burger.classList.toggle("open", open); menu.style.display = open ? "flex" : "none"; };
      burger.addEventListener("click", () => toggle(menu.style.display !== "flex"));
      $$("a", menu).forEach((a) => a.addEventListener("click", () => toggle(false)));
    }

    // smooth in-page anchors
    $$('a[href^="#"]').forEach((a) => a.addEventListener("click", (e) => {
      const id = a.getAttribute("href"); if (id.length < 2) return;
      const t = $(id); if (!t) return;
      e.preventDefault(); t.scrollIntoView({ behavior: reduce() ? "auto" : "smooth", block: "start" });
    }));

    // reveal on scroll (transform-only; content is never hidden)
    const io = "IntersectionObserver" in window
      ? new IntersectionObserver((ents, o) => ents.forEach((en) => { if (en.isIntersecting) { en.target.classList.add("in"); o.unobserve(en.target); } }), { threshold: 0.12, rootMargin: "0px 0px -6% 0px" })
      : null;
    $$(".reveal").forEach((el) => {
      if (!io || el.getBoundingClientRect().top < (window.innerHeight || 800) * 0.96) { el.classList.add("in"); return; }
      io.observe(el);
    });

    // console mock — render + scale to fit
    const mount = $("#consoleMount");
    if (mount && window.renderOrreryConsole) {
      const inner = document.createElement("div"); inner.className = "rc-inner";
      const frame = document.createElement("div"); frame.className = "rc-frame";
      frame.appendChild(inner); mount.appendChild(frame);
      window.renderOrreryConsole(inner);
      // The mock is a fixed 1340x812. Size .rc-inner to match BEFORE scaling so
      // its overflow:hidden (the rounded-corner clip) never cuts the mock's right
      // edge — without this, .rc-inner defaults to the frame width (< 1340) and
      // clips the top-bar buttons + right column before the scale shrinks it.
      inner.style.width = "1340px";
      inner.style.height = "812px";
      const fit = () => {
        const host = mount.closest(".console-scroll") || mount.parentElement;
        const scale = Math.min(1, host.clientWidth / 1340);
        inner.style.transform = `scale(${scale})`;
        frame.style.width = 1340 * scale + "px"; frame.style.height = 812 * scale + "px";
      };
      fit();
      if ("ResizeObserver" in window) new ResizeObserver(fit).observe(mount.closest(".console-scroll") || mount);
      window.addEventListener("resize", fit);
    }
  });
})();
