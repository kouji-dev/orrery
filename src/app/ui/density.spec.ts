import { beforeEach, describe, expect, it } from "vitest";
import { codeMetrics, rowHeight } from "./density";

/** jsdom has no @property registration, so tokens are set as plain resolved px
 *  here — which is exactly what the browser hands back once they ARE registered. */
function setTokens(v: Record<string, string>) {
  const r = document.documentElement;
  r.removeAttribute("style");
  for (const [k, val] of Object.entries(v)) r.style.setProperty(k, val);
}

describe("density bridge", () => {
  beforeEach(() => document.documentElement.removeAttribute("style"));

  it("reads the resolved code metrics", () => {
    setTokens({ "--fs-code": "14.5px", "--lh-code": "20px" });
    expect(codeMetrics()).toEqual({ fontSize: 14.5, lineHeight: 20 });
  });

  it("reads the resolved row height", () => {
    setTokens({ "--row-h": "35px" });
    expect(rowHeight()).toBe(35);
  });

  it("falls back to the regular-density values when a token is missing", () => {
    // Guards the failure mode that shipped silently before the tokens were
    // registered with @property: getPropertyValue returned the raw token stream,
    // parseFloat gave NaN, and Monaco/xterm quietly pinned to one size forever.
    expect(codeMetrics()).toEqual({ fontSize: 13, lineHeight: 18 });
    expect(rowHeight()).toBe(30);
  });

  it("falls back rather than yielding NaN from an unresolved token stream", () => {
    setTokens({
      "--fs-code": "round(calc(13px * var(--fs-scale)), 0.5px)",
      "--lh-code": "round(calc(18px * var(--fs-scale)), 1px)",
      "--row-h": "round(calc(30px * var(--density)), 1px)",
    });
    const m = codeMetrics();
    expect(Number.isNaN(m.fontSize)).toBe(false);
    expect(Number.isNaN(m.lineHeight)).toBe(false);
    expect(Number.isNaN(rowHeight())).toBe(false);
  });
});
