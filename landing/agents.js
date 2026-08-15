/* Orrery landing — <orrery-agent>: an AI-tool mark + label chip.
 *
 * Native custom element, light DOM (display:contents) — the host element
 * (.node pill in the hero orbit, .toolstrip-item in the tool strip) keeps
 * providing layout/typography; this only emits the brand icon + label.
 *
 *   <orrery-agent name="claude" size="15"></orrery-agent>
 *
 * Attributes: name (registry key), size (icon px, default 14),
 * label (optional override of the registry label).
 *
 * Adding a new agent (OpenCode, Zia, …): add a <symbol id="i-<name>"> with its
 * brand mark to the sprite in index.html, then one AGENTS entry here. */
(function () {
  "use strict";
  var AGENTS = {
    claude: { icon: "i-claude", label: "Claude Code" },
    codex: { icon: "i-codex", label: "Codex" },
    cursor: { icon: "i-cursor", label: "Cursor" },
    gemini: { icon: "i-gemini", label: "Gemini" },
    more: { icon: "i-more", label: "and more" }
  };
  var SVG_NS = "http://www.w3.org/2000/svg";

  class OrreryAgent extends HTMLElement {
    connectedCallback() {
      if (this.dataset.rendered) return;
      this.dataset.rendered = "1";
      var agent = AGENTS[this.getAttribute("name")] || AGENTS.more;
      var size = +this.getAttribute("size") || 14;
      var svg = document.createElementNS(SVG_NS, "svg");
      svg.setAttribute("class", "ai-ico");
      svg.setAttribute("width", size);
      svg.setAttribute("height", size);
      svg.setAttribute("aria-hidden", "true");
      var use = document.createElementNS(SVG_NS, "use");
      use.setAttribute("href", "#" + agent.icon);
      svg.appendChild(use);
      this.appendChild(svg);
      this.appendChild(document.createTextNode(this.getAttribute("label") || agent.label));
    }
  }
  customElements.define("orrery-agent", OrreryAgent);
})();
