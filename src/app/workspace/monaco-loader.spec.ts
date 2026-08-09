import { describe, expect, it, vi } from "vitest";

import { applyMonacoTheme, monacoLanguage, type MonacoApi } from "./monaco-loader";

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
