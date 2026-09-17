import { beforeEach, describe, expect, it, vi } from "vitest";

import { fenceTag, renderCodeBlocks } from "./md-code";

const { loadMonaco, monacoLanguage, applyMonacoTheme, colorize } = vi.hoisted(() => {
  const colorize = vi.fn();
  return {
    colorize,
    loadMonaco: vi.fn(),
    monacoLanguage: vi.fn(),
    applyMonacoTheme: vi.fn(),
  };
});
vi.mock("../monaco-loader", () => ({ loadMonaco, monacoLanguage, applyMonacoTheme }));

/** marked-mermaid's output for a ```lang fence. */
const fence = (lang: string, body: string): string =>
  `<div class="md-box fence"><div class="fence-hd static"><span class="chip mono">${lang}</span></div>` +
  `<pre><code class="language-${lang}">${body}</code></pre></div>`;

function host(html: string): HTMLElement {
  const el = document.createElement("div");
  el.innerHTML = html;
  document.body.appendChild(el);
  return el;
}

describe("fenceTag", () => {
  it("maps the language names people type in a fence", () => {
    const cases: Record<string, string> = {
      typescript: "javascript", javascript: "javascript", rust: "rust", python: "python",
      bash: "shell", sh: "shell", shell: "shell", console: "shell",
      yaml: "yaml", json: "json", go: "go", sql: "sql", html: "html", css: "css",
      scss: "sass", java: "java", kotlin: "kotlin", ruby: "ruby", php: "php",
      c: "cpp", cpp: "cpp", csharp: "csharp", swift: "swift", dockerfile: "dockerfile",
      powershell: "powershell", toml: "toml", ini: "properties", xml: "xml", lua: "lua",
      r: "r", dart: "dart", scala: "scala", clojure: "clojure", julia: "julia", perl: "perl",
    };
    for (const [word, tag] of Object.entries(cases)) expect([word, fenceTag(word)]).toEqual([word, tag]);
  });

  it("falls back to langId() for extension-shaped words, and is case/space tolerant", () => {
    expect(fenceTag("ts")).toBe("javascript");
    expect(fenceTag("rs")).toBe("rust");
    expect(fenceTag("yml")).toBe("yaml");
    expect(fenceTag(" TypeScript ")).toBe("javascript");
    expect(fenceTag("PY")).toBe("python");
  });

  it("returns none for prose fences and for anything it does not know", () => {
    for (const w of ["", "text", "plain", "plaintext", "txt", "none", "diff", "patch", "wat-is-this", "42"]) {
      expect([w, fenceTag(w)]).toEqual([w, ""]);
    }
  });
});

describe("renderCodeBlocks", () => {
  beforeEach(() => {
    loadMonaco.mockReset();
    monacoLanguage.mockReset();
    applyMonacoTheme.mockReset();
    colorize.mockReset();
    loadMonaco.mockResolvedValue({ editor: { colorize } });
    monacoLanguage.mockImplementation(async (tag: string) => (tag === "javascript" ? "typescript" : tag));
    colorize.mockImplementation(async (src: string) => `<span class="mtk1">${src}</span><br/>`);
    document.body.innerHTML = "";
  });

  it("colorizes a fence in place and stores source + theme on the element", async () => {
    const el = host(`<h1>doc</h1>${fence("ts", "const x = 1;")}<p>after</p>`);
    await renderCodeBlocks(el, "dark");

    expect(applyMonacoTheme).toHaveBeenCalledWith(expect.anything(), "dark");
    expect(monacoLanguage).toHaveBeenCalledWith("javascript");
    expect(colorize).toHaveBeenCalledWith("const x = 1;", "typescript", { tabSize: 2 });

    const code = el.querySelector<HTMLElement>(".md-box.fence pre code")!;
    expect(code.querySelector("span.mtk1")?.textContent).toBe("const x = 1;");
    expect(code.innerHTML.endsWith("<br>")).toBe(false); // trailing line break dropped
    expect(code.dataset["mdcSrc"]).toBe("const x = 1;");
    expect(code.dataset["mdcTheme"]).toBe("dark");
    // the card around it, and the rest of the document, are untouched
    expect(code.classList.contains("language-ts")).toBe(true);
    expect(el.querySelector(".fence-hd .chip")?.textContent).toBe("ts");
    expect(el.querySelector("p")?.textContent).toBe("after");
  });

  it("loads nothing when the document holds no highlightable fence", async () => {
    const el = host(
      `<p>prose</p>${fence("text", "just words")}${fence("diff", "-a\n+b")}${fence("nonsense", "?")}` +
        `<div class="md-box fence"><pre><code>indented</code></pre></div>`,
    );
    await renderCodeBlocks(el, "dark");
    expect(loadMonaco).not.toHaveBeenCalled();
    expect(colorize).not.toHaveBeenCalled();
    expect(el.querySelector("code.language-text")?.textContent).toBe("just words");
  });

  it("skips a tag Monaco has no grammar for, without touching the markup", async () => {
    monacoLanguage.mockResolvedValue("plaintext");
    const el = host(fence("hs", "main = pure ()"));
    await renderCodeBlocks(el, "dark");
    expect(colorize).not.toHaveBeenCalled();
    const code = el.querySelector<HTMLElement>("code")!;
    expect(code.textContent).toBe("main = pure ()");
    expect(code.dataset["mdcTheme"]).toBeUndefined();
  });

  it("re-colours on a theme toggle from the stored source, and only then", async () => {
    const el = host(fence("py", "x = 1"));
    await renderCodeBlocks(el, "dark");
    expect(colorize).toHaveBeenCalledTimes(1);

    // same theme again → no work
    await renderCodeBlocks(el, "dark");
    expect(colorize).toHaveBeenCalledTimes(1);

    // toggle → recoloured from dataset, not from the span soup in the DOM
    await renderCodeBlocks(el, "light");
    expect(colorize).toHaveBeenCalledTimes(2);
    expect(colorize.mock.calls[1][0]).toBe("x = 1");
    expect(applyMonacoTheme).toHaveBeenLastCalledWith(expect.anything(), "light");
    expect(el.querySelector<HTMLElement>("code")!.dataset["mdcTheme"]).toBe("light");
  });

  it("leaves a block plain when colorize fails, and keeps colouring the others", async () => {
    colorize.mockRejectedValueOnce(new Error("tokenizer blew up"));
    const el = host(`${fence("rs", "fn main() {}")}${fence("go", "func main() {}")}`);
    await expect(renderCodeBlocks(el, "dark")).resolves.toBeUndefined();

    const [bad, good] = Array.from(el.querySelectorAll<HTMLElement>("code"));
    expect(bad.textContent).toBe("fn main() {}");
    expect(bad.querySelector("span")).toBeNull();
    expect(bad.dataset["mdcTheme"]).toBeUndefined();
    expect(good.querySelector("span.mtk1")).not.toBeNull();
  });

  it("leaves every fence plain when Monaco itself fails to load", async () => {
    loadMonaco.mockRejectedValue(new Error("chunk 404"));
    const el = host(fence("ts", "const x = 1;"));
    await expect(renderCodeBlocks(el, "dark")).resolves.toBeUndefined();
    expect(el.querySelector("code")?.textContent).toBe("const x = 1;");
    expect(applyMonacoTheme).not.toHaveBeenCalled();
  });
});
