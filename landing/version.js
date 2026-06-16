/* Orrery landing — keep every version label in sync with the latest release.
 *
 * The HTML ships a baked version (the fallback shown instantly and when offline
 * / rate-limited). At runtime we replace every `.js-ver` element with the real
 * latest tag from the GitHub API — the SAME source the "Download" buttons link
 * to (/releases/latest) — so the label can never drift from the actual download.
 * Result cached in localStorage (6h) to avoid hammering the API on every visit.
 * Dependency-free; safe to load with `defer` on any page. */
(function () {
  "use strict";
  var KEY = "orrery:latest-ver";
  var TTL = 6 * 60 * 60 * 1000; // 6 hours
  var API = "https://api.github.com/repos/kouji-dev/orrery-releases/releases/latest";

  function apply(tag) {
    if (!tag) return;
    var nodes = document.querySelectorAll(".js-ver");
    for (var i = 0; i < nodes.length; i++) nodes[i].textContent = tag;
  }

  // 1) Instant: a fresh cached tag beats the baked fallback (no flash on revisit).
  try {
    var cached = JSON.parse(localStorage.getItem(KEY) || "null");
    if (cached && cached.tag && Date.now() - cached.t < TTL) apply(cached.tag);
  } catch (e) {
    /* private mode / corrupt cache — ignore, the baked value stands */
  }

  // 2) Refresh from the API and re-cache.
  fetch(API, { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (j) {
      if (!j || !j.tag_name) return;
      apply(j.tag_name);
      try {
        localStorage.setItem(KEY, JSON.stringify({ tag: j.tag_name, t: Date.now() }));
      } catch (e) {
        /* storage full / unavailable — the live value still applied */
      }
    })
    .catch(function () {
      /* offline or rate-limited — the baked/cached fallback stays in place */
    });
})();
