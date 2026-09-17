import { describe, expect, it, vi } from "vitest";

import {
  applyMonacoTheme,
  diffOverviewRulerOptions,
  monacoLanguage,
  type MonacoApi,
} from "./monaco-loader";

describe("monacoLanguage", () => {
  it("resolves unknown tags to plaintext without loading anything", async () => {
    await expect(monacoLanguage("haskell")).resolves.toBe("plaintext");
    await expect(monacoLanguage("nope")).resolves.toBe("plaintext");
    await expect(monacoLanguage("")).resolves.toBe("plaintext");
  });
});

describe("applyMonacoTheme", () => {
  function fakeMonaco(): {
    api: MonacoApi;
    defineTheme: ReturnType<typeof vi.fn>;
    setTheme: ReturnType<typeof vi.fn>;
  } {
    const defineTheme = vi.fn();
    const setTheme = vi.fn();
    const api = { editor: { defineTheme, setTheme } } as unknown as MonacoApi;
    return { api, defineTheme, setTheme };
  }

  it("defines + activates a theme built from resolved CSS tokens", () => {
    const root = document.documentElement;
    root.setAttribute("data-theme", "dark");
    root.style.setProperty("--panel", "#0e1018");
    root.style.setProperty("--ink", "#e8ebf2");
    root.style.setProperty("--code-add-bg", "rgba(52, 224, 161, 0.1)");
    const { api, defineTheme, setTheme } = fakeMonaco();

    applyMonacoTheme(api, "dark");

    expect(defineTheme).toHaveBeenCalledTimes(1);
    const [name, data] = defineTheme.mock.calls[0] as [
      string,
      { base: string; colors: Record<string, string> },
    ];
    expect(name).toBe("orrery-dark");
    expect(data.base).toBe("vs-dark");
    expect(data.colors["editor.background"]).toBe("#0e1018");
    expect(data.colors["editor.foreground"]).toBe("#e8ebf2");
    // rgba() tokens become #rrggbbaa
    expect(data.colors["diffEditor.insertedTextBackground"]).toBe("#34e0a11a");
    expect(setTheme).toHaveBeenCalledWith("orrery-dark");
  });

  it("uses the vs base for light mode and skips unresolvable tokens", () => {
    const root = document.documentElement;
    root.setAttribute("data-theme", "light");
    root.style.removeProperty("--panel");
    root.style.removeProperty("--ink");
    root.style.removeProperty("--code-add-bg");
    const { api, defineTheme } = fakeMonaco();

    applyMonacoTheme(api, "light");

    const [name, data] = defineTheme.mock.calls[0] as [
      string,
      { base: string; colors: Record<string, string> },
    ];
    expect(name).toBe("orrery-light");
    expect(data.base).toBe("vs");
    expect(data.colors["editor.background"]).toBeUndefined();
  });
});

describe("diff overview ruler", () => {
  function themeColors(appTheme: "dark" | "light"): Record<string, string> {
    const defineTheme = vi.fn();
    const api = {
      editor: { defineTheme, setTheme: vi.fn() },
    } as unknown as MonacoApi;
    applyMonacoTheme(api, appTheme);
    const [, data] = defineTheme.mock.calls[0] as [
      string,
      { colors: Record<string, string> },
    ];
    return data.colors;
  }

  it("asks Monaco for its own diff overview ruler instead of a hand-rolled overlay", () => {
    expect(diffOverviewRulerOptions()).toEqual({
      renderOverviewRuler: true,
      overviewRulerBorder: false,
    });
  });

  it("paints the ruler marks from the diff ink tokens, per theme", () => {
    const root = document.documentElement;
    const cases = [
      { theme: "dark", add: "#7fd7a5", del: "#ef8a95" },
      { theme: "light", add: "#146b33", del: "#a83234" },
    ] as const;

    for (const c of cases) {
      root.setAttribute("data-theme", c.theme);
      root.style.setProperty("--code-add-ink", c.add);
      root.style.setProperty("--code-del-ink", c.del);

      const colors = themeColors(c.theme);

      // solid ink, not the ~90%-transparent row tint — a 15px mark has to read
      expect(colors["diffEditorOverview.insertedForeground"]).toBe(c.add);
      expect(colors["diffEditorOverview.removedForeground"]).toBe(c.del);
    }
  });

  it("leaves the ruler colours to Monaco when the tokens are unresolvable", () => {
    const root = document.documentElement;
    root.setAttribute("data-theme", "dark");
    root.style.setProperty("--code-add-ink", "color-mix(in oklch, red, blue)");
    root.style.removeProperty("--code-del-ink");

    const colors = themeColors("dark");

    expect(colors["diffEditorOverview.insertedForeground"]).toBeUndefined();
    expect(colors["diffEditorOverview.removedForeground"]).toBeUndefined();
  });
});
