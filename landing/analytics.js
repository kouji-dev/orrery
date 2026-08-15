/* Orrery landing — GA4 event tracking (same pattern as kouji.dev).
 *
 * window.track(name, params) no-ops when gtag is unavailable (blocked /
 * offline), so the page never depends on GA. One delegated capture-phase
 * click listener covers the static markup AND the links dl-button.js /
 * version.js rebuild at runtime:
 *   - installer links (.js-dl CTA / .dl-item) → file_download
 *     { file_name, platform: win|mac, link_text }
 *   - outbound links (GitHub, kouji.dev, …)   → click { outbound: true, … }
 *   - changelog navigation                    → select_content
 * version.js additionally reports release_lookup ok/error (CTA health).
 * Dependency-free; load with `defer` BEFORE the other landing scripts. */
(function () {
  "use strict";

  window.track = function (name, params) {
    if (typeof window.gtag === "function") window.gtag("event", name, params || {});
  };

  var INSTALLER = /-setup\.exe$|\.msi$|\.dmg$/i;

  document.addEventListener(
    "click",
    function (e) {
      var a = e.target && e.target.closest && e.target.closest("a[href]");
      if (!a) return;
      var href = a.getAttribute("href") || "";

      // direct installer downloads (CTA rewritten by version.js, or dropdown items)
      if (INSTALLER.test(a.pathname || href)) {
        var file = (a.pathname || href).split("/").pop();
        window.track("file_download", {
          file_name: file,
          platform: /\.dmg$/i.test(file) ? "mac" : "win",
          link_text: (a.textContent || "").trim().slice(0, 60)
        });
        return;
      }

      // outbound (GitHub repo/releases pages, kouji.dev, …)
      if (/^https?:/.test(a.href || "") && a.hostname && a.hostname !== location.hostname) {
        window.track("click", { outbound: true, link_url: a.href, link_domain: a.hostname });
        return;
      }

      // internal changelog navigation
      if (href.indexOf("changelog") >= 0) {
        window.track("select_content", { content_type: "page", content_id: "changelog" });
      }
    },
    true
  );
})();
