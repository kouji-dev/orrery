/* Orrery landing — resolve the latest release and feed it to the page.
 *
 * One GitHub API call resolves the latest release; then:
 *   - every `.js-ver` span gets the real tag (footer + the <orrery-download>
 *     buttons' version sub — the HTML/component bake a fallback version);
 *   - every <orrery-download> gets update({tag, exe, msi, dmg}) to become a
 *     direct installer download for the visitor's OS (see dl-button.js).
 *   - `.js-dl-count` gets the total installer download count summed over ALL
 *     releases (same buckets as scripts/download-stats.mjs: exe + msi + dmg;
 *     latest.json update-check fetches are NOT counted) — hidden until known.
 * If the API fails, the baked releases-page fallback stays in place. Results
 * cached in localStorage (6h) to avoid hammering the API on every visit.
 * Dependency-free; load with `defer` AFTER dl-button.js. */
(function () {
  "use strict";
  var KEY = "orrery:latest-ver:2";
  var COUNT_KEY = "orrery:dl-count:1";
  var TTL = 6 * 60 * 60 * 1000; // 6 hours
  var API = "https://api.github.com/repos/kouji-dev/orrery-releases/releases/latest";
  var API_ALL = "https://api.github.com/repos/kouji-dev/orrery-releases/releases?per_page=100";

  function pickAsset(assets, re) {
    for (var i = 0; i < (assets || []).length; i++)
      if (re.test(assets[i].name)) return assets[i].browser_download_url;
    return null;
  }

  function apply(data) {
    if (!data || !data.tag) return;
    var vers = document.querySelectorAll(".js-ver");
    for (var i = 0; i < vers.length; i++) vers[i].textContent = data.tag;
    var btns = document.querySelectorAll("orrery-download");
    for (var j = 0; j < btns.length; j++)
      if (typeof btns[j].update === "function") btns[j].update(data);
  }

  var DL_ICON =
    '<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m0 0 5-5m-5 5-5-5M4 21h16"/></svg>';

  function applyCount(n) {
    if (!(n > 0)) return;
    var els = document.querySelectorAll(".js-dl-count");
    for (var i = 0; i < els.length; i++) {
      // n is a number — the only interpolated value in this fixed markup.
      els[i].innerHTML = '<span class="dl-badge">' + DL_ICON + "<b>" + n.toLocaleString("en-US") + "</b>&nbsp;downloads</span>";
      els[i].hidden = false;
    }
  }

  // 1) Instant: a fresh cached result beats the baked fallback (no flash on revisit).
  try {
    var cached = JSON.parse(localStorage.getItem(KEY) || "null");
    if (cached && cached.tag && Date.now() - cached.t < TTL) apply(cached);
    var cachedCount = JSON.parse(localStorage.getItem(COUNT_KEY) || "null");
    if (cachedCount && Date.now() - cachedCount.t < TTL) applyCount(cachedCount.n);
  } catch (e) {
    /* private mode / corrupt cache — ignore, the baked values stand */
  }

  // 2) Refresh from the API and re-cache.
  fetch(API, { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (j) {
      if (!j || !j.tag_name) {
        // rate-limited / bad payload — the download CTA degrades to the releases page
        if (window.track) window.track("release_lookup", { status: "error" });
        return;
      }
      if (window.track) window.track("release_lookup", { status: "ok", version: j.tag_name });
      var data = {
        tag: j.tag_name,
        exe: pickAsset(j.assets, /-setup\.exe$/i),
        msi: pickAsset(j.assets, /\.msi$/i),
        dmg: pickAsset(j.assets, /\.dmg$/i),
        t: Date.now()
      };
      apply(data);
      try {
        localStorage.setItem(KEY, JSON.stringify(data));
      } catch (e) {
        /* storage full / unavailable — the live values still applied */
      }
    })
    .catch(function () {
      /* offline or rate-limited — the baked/cached fallback stays in place */
      if (window.track) window.track("release_lookup", { status: "error" });
    });

  // 3) Total downloads across all releases (single page covers ~100 releases).
  fetch(API_ALL, { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (releases) {
      if (!releases || !releases.length) return;
      var n = 0;
      for (var i = 0; i < releases.length; i++) {
        var assets = releases[i].assets || [];
        for (var j = 0; j < assets.length; j++)
          if (/-setup\.exe$|\.msi$|\.dmg$/i.test(assets[j].name)) n += assets[j].download_count || 0;
      }
      applyCount(n);
      try {
        localStorage.setItem(COUNT_KEY, JSON.stringify({ n: n, t: Date.now() }));
      } catch (e) {
        /* storage unavailable — the live value still applied */
      }
    })
    .catch(function () {
      /* offline or rate-limited — the count line simply stays hidden */
    });
})();
