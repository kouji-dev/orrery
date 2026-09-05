/* Orrery landing — <orrery-download>: the split download button + installer dropdown.
 *
 * Native custom element, light DOM (no shadow root) so the page's design tokens
 * and .ol-btn/.dl-* styles apply as-is. Used in the nav (size="sm"), the hero
 * and the bottom CTA band — one implementation, three placements.
 *
 * Baked state (no JS data yet / API failed): primary CTA links the GitHub
 * /releases/latest PAGE in a new tab; the dropdown holds the macOS
 * "coming soon" row and "All releases". version.js fetches the latest release
 * and calls update(data) on every instance with {tag, exe, msi, dmg} — the CTA
 * then becomes a direct download of the NSIS -setup.exe and the dropdown is
 * rebuilt with the .msi.
 *
 * macOS is NOT offered yet: the CTA is the Windows installer for every visitor
 * (a mac visitor gets the same button — there is nothing else to hand them),
 * and the dropdown carries a disabled "macOS · coming soon" row so the plan is
 * visible without a dead link. A .dmg asset in a release is ignored until the
 * mac build ships; to re-enable, restore the per-OS branch in update().
 *
 * Attributes: size="sm" — nav-sized button, dropdown right-aligned. */
(function () {
  "use strict";
  var LATEST_PAGE = "https://github.com/kouji-dev/orrery-releases/releases/latest";
  var RELEASES_PAGE = "https://github.com/kouji-dev/orrery-releases/releases";

  var ICONS = {
    win: '<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M3 5.5 10.2 4.5V11H3V5.5ZM10.2 12v6.5L3 17.5V12h7.2ZM11.4 4.3 21 3v8.5h-9.6V4.3ZM21 12.5V21l-9.6-1.3V12.5H21Z"/></svg>',
    mac: '<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M17.05 20.28c-.98.95-2.05.8-3.08.35-1.09-.46-2.09-.48-3.24 0-1.44.62-2.2.44-3.06-.35C2.79 15.25 3.51 7.59 9.05 7.31c1.35.07 2.29.74 3.08.8 1.18-.24 2.31-.93 3.57-.84 1.51.12 2.65.72 3.4 1.8-3.12 1.87-2.38 5.98.48 7.13-.57 1.5-1.31 2.99-2.53 4.09ZM12.03 7.25c-.15-2.23 1.66-4.07 3.74-4.25.29 2.58-2.34 4.5-3.74 4.25Z"/></svg>',
    gh: '<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.56 0-.27-.01-1.17-.02-2.12-3.2.7-3.87-1.36-3.87-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.18 1.76 1.18 1.03 1.76 2.69 1.25 3.35.96.1-.75.4-1.25.72-1.54-2.55-.29-5.23-1.28-5.23-5.68 0-1.26.45-2.28 1.18-3.09-.12-.29-.51-1.46.11-3.05 0 0 .96-.31 3.15 1.18.92-.26 1.9-.38 2.88-.39.98 0 1.96.13 2.88.39 2.19-1.49 3.15-1.18 3.15-1.18.62 1.59.23 2.76.11 3.05.73.81 1.18 1.83 1.18 3.09 0 4.42-2.69 5.39-5.25 5.67.41.36.78 1.06.78 2.14 0 1.54-.01 2.79-.01 3.17 0 .31.21.67.8.56A10.51 10.51 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z"/></svg>'
  };

  function menuItem(icon, label, href, opts) {
    var a = document.createElement("a");
    a.className = "dl-item";
    a.innerHTML = ICONS[icon]; // fixed markup — label/href attached safely below
    a.appendChild(document.createTextNode(label));
    if (opts && opts.sub) {
      var s = document.createElement("span");
      s.className = "dl-item-sub";
      s.textContent = opts.sub;
      a.appendChild(s);
    }
    a.href = href;
    if (opts && opts.blank) { a.target = "_blank"; a.rel = "noopener"; }
    return a;
  }

  /** The disabled macOS row: same layout as a .dl-item, but a <span> — no href,
   *  no click — flagged aria-disabled so assistive tech reads it as such. */
  function macSoonItem() {
    var s = document.createElement("span");
    s.className = "dl-item soon";
    s.setAttribute("aria-disabled", "true");
    s.innerHTML = ICONS.mac; // fixed markup — text attached safely below
    s.appendChild(document.createTextNode("macOS · Apple Silicon"));
    var sub = document.createElement("span");
    sub.className = "dl-item-sub";
    sub.textContent = "coming soon";
    s.appendChild(sub);
    return s;
  }

  function closeAll() {
    var open = document.querySelectorAll(".dl-split.open");
    for (var i = 0; i < open.length; i++) {
      open[i].classList.remove("open");
      var b = open[i].querySelector(".js-dl-arrow");
      if (b) b.setAttribute("aria-expanded", "false");
    }
  }
  document.addEventListener("click", closeAll);
  document.addEventListener("keydown", function (e) { if (e.key === "Escape") closeAll(); });

  // Windows is the only installer on offer (see the header note on macOS).
  var OS_ICON = ICONS.win;
  var OS_LABEL = "Windows";

  class OrreryDownload extends HTMLElement {
    connectedCallback() {
      if (this.dataset.rendered) return;
      this.dataset.rendered = "1";
      var sm = this.getAttribute("size") === "sm";

      var split = (this._split = document.createElement("div"));
      split.className = "dl-split" + (sm ? " sm" : "");
      split.innerHTML =
        '<a class="ol-btn primary js-dl' + (sm ? " sm" : "") + '" href="' + LATEST_PAGE + '" target="_blank" rel="noopener">' +
        OS_ICON + '<span class="js-dl-label">' + OS_LABEL + '</span><span class="ol-btn-sub js-ver">v0.4.0</span></a>' +
        '<button class="dl-arrow js-dl-arrow" type="button" aria-haspopup="true" aria-expanded="false" aria-label="More download options">' +
        '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg></button>' +
        '<div class="dl-menu js-dl-menu"></div>';
      var menu = split.querySelector(".js-dl-menu");
      menu.appendChild(macSoonItem());
      menu.appendChild(menuItem("gh", "All releases on GitHub", RELEASES_PAGE, { blank: true }));

      var arrow = split.querySelector(".js-dl-arrow");
      arrow.addEventListener("click", function (e) {
        e.stopPropagation();
        var willOpen = !split.classList.contains("open");
        closeAll();
        if (willOpen) {
          split.classList.add("open");
          arrow.setAttribute("aria-expanded", "true");
        }
      });
      this.appendChild(split);
    }

    /** data: {tag, exe, msi, dmg} — installer asset URLs from the latest release.
     *  `dmg` is deliberately unused while macOS is "coming soon" (header note). */
    update(data) {
      if (!data || !this._split) return;
      var split = this._split;

      var menu = split.querySelector(".js-dl-menu");
      menu.textContent = "";
      if (data.msi) menu.appendChild(menuItem("win", ".msi installer", data.msi, { sub: "per-machine" }));
      menu.appendChild(macSoonItem());
      menu.appendChild(menuItem("gh", "All releases on GitHub", RELEASES_PAGE, { blank: true }));

      if (!data.exe) return; // no installer asset — baked CTA fallback stands
      var cta = split.querySelector(".js-dl");
      cta.href = data.exe;
      cta.removeAttribute("target"); // direct download: the page stays, the file lands
    }
  }
  customElements.define("orrery-download", OrreryDownload);
})();
