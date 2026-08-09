/**
 * Mermaid rendering for the markdown preview (B1.4 follow-up).
 *
 * marked renders ```mermaid fences as `<pre><code class="language-mermaid">`.
 * Angular's [innerHTML] sanitizer would strip mermaid's SVG output (style
 * elements, foreignObject), so diagrams are rendered by post-processing the
 * live preview DOM: each fence is replaced by a `.mmd` container holding the
 * generated SVG. The diagram source rides along in a data attribute so a theme
 * toggle can re-render in place without re-parsing the markdown.
 *
 * mermaid (~2.5MB) loads as a lazy chunk on the first document that needs it —
 * same pattern as the Monaco loader, including retry after a failed load.
 */

type MermaidApi = typeof import("mermaid").default;

let mermaidLoad: Promise<MermaidApi> | null = null;

function loadMermaid(): Promise<MermaidApi> {
  mermaidLoad ??= import("mermaid").then(
    (m) => m.default,
    (e) => {
      mermaidLoad = null; // allow a retry on the next render pass
      throw e;
    },
  );
  return mermaidLoad;
}

let seq = 0;

/**
 * Render every mermaid block inside `host` for the given theme: fresh
 * ```mermaid fences from marked, plus already-rendered diagrams whose theme no
 * longer matches (theme toggle). Invalid diagrams keep their code fence.
 */
export async function renderMermaidBlocks(host: HTMLElement, theme: "dark" | "light"): Promise<void> {
  const fresh = Array.from(host.querySelectorAll<HTMLElement>("pre > code.language-mermaid"));
  const stale = Array.from(host.querySelectorAll<HTMLElement>(".mmd")).filter(
    (el) => el.dataset["mmdTheme"] !== theme,
  );
  if (fresh.length === 0 && stale.length === 0) return;

  const mermaid = await loadMermaid();
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: theme === "dark" ? "dark" : "neutral",
  });

  const jobs = [
    ...fresh.map((code) => ({ target: code.parentElement as HTMLElement, src: code.textContent ?? "" })),
    ...stale.map((el) => ({ target: el, src: el.dataset["mmdSrc"] ?? "" })),
  ];
  for (const { target, src } of jobs) {
    if (!target?.isConnected || !src.trim()) continue;
    const id = `mmd-svg-${++seq}`;
    try {
      const { svg } = await mermaid.render(id, src);
      const box = document.createElement("div");
      box.className = "mmd";
      box.dataset["mmdSrc"] = src;
      box.dataset["mmdTheme"] = theme;
      box.innerHTML = svg;
      target.replaceWith(box);
    } catch {
      // mermaid can leave its scratch nodes in <body> on a parse error
      document.getElementById(id)?.remove();
      document.getElementById("d" + id)?.remove();
      // stamp the theme so a failing block isn't retried on every pass
      if (target.classList.contains("mmd")) target.dataset["mmdTheme"] = theme;
    }
  }
}
