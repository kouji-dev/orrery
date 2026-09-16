import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * The two Orrery palettes used to ship inside @kouji-ui/themes; they are
 * app-specific and now live here. kouji-ui's own theme contract test no longer
 * covers them, so this spec carries it over: every token the shared layer
 * requires must be declared inside the theme's `[data-theme]` block, or
 * kouji components fall through to undefined vars and paint with no colour.
 */
const REQUIRED_SHARED_TOKENS = [
  "--kj-bg-body", "--kj-bg-surface", "--kj-bg-field",
  "--kj-bg-elevated", "--kj-bg-overlay", "--kj-bg-inverse", "--kj-bg-disabled",
  "--kj-bg-primary", "--kj-bg-primary-subtle",
  "--kj-bg-accent", "--kj-bg-accent-subtle",
  "--kj-bg-info", "--kj-bg-info-subtle",
  "--kj-bg-success", "--kj-bg-success-subtle",
  "--kj-bg-warning", "--kj-bg-warning-subtle",
  "--kj-bg-danger", "--kj-bg-danger-subtle",
  "--kj-fg-default", "--kj-fg-muted", "--kj-fg-subtle", "--kj-fg-disabled",
  "--kj-fg-on-primary", "--kj-fg-on-accent",
  "--kj-fg-on-info", "--kj-fg-on-success", "--kj-fg-on-warning", "--kj-fg-on-danger",
  "--kj-fg-on-inverse",
  "--kj-fg-primary", "--kj-fg-accent",
  "--kj-fg-info", "--kj-fg-success", "--kj-fg-warning", "--kj-fg-danger",
  "--kj-border-default", "--kj-border-muted", "--kj-border-strong",
  "--kj-border-focus", "--kj-border-disabled",
  "--kj-border-primary", "--kj-border-danger",
  "--kj-shadow-sm", "--kj-shadow-md", "--kj-shadow-lg", "--kj-shadow-focus",
  "--kj-radius-box", "--kj-radius-field", "--kj-radius-selector",
  "--kj-border", "--kj-depth",
  "--kj-transition",
] as const;

const THEMES = ["orrery", "orrery-light"] as const;
const themesDir = resolve(__dirname);

/** Every `--kj-*` property declared inside `[data-theme="<name>"] { … }`. */
function tokensInThemeBlock(css: string, name: string): Set<string> {
  const tokens = new Set<string>();
  const selector = `[data-theme="${name}"]`;
  let from = 0;
  for (;;) {
    const at = css.indexOf(selector, from);
    if (at < 0) break;
    const open = css.indexOf("{", at);
    let depth = 1;
    let i = open + 1;
    while (i < css.length && depth > 0) {
      if (css[i] === "{") depth++;
      else if (css[i] === "}") depth--;
      i++;
    }
    for (const m of css.slice(open + 1, i - 1).matchAll(/(--kj-[\w-]+)\s*:/g)) tokens.add(m[1]);
    from = i;
  }
  return tokens;
}

describe("orrery themes (moved out of @kouji-ui/themes)", () => {
  for (const name of THEMES) {
    describe(name, () => {
      const css = readFileSync(resolve(themesDir, `${name}.css`), "utf8");
      const defined = tokensInThemeBlock(css, name);

      it("declares its tokens in the kj.shared layer", () => {
        expect(css).toMatch(/@layer\s+kj\.shared\s*\{/);
      });

      for (const token of REQUIRED_SHARED_TOKENS) {
        it(`defines ${token}`, () => {
          expect(defined).toContain(token);
        });
      }
    });
  }

  it("angular.json loads both from src/themes after the kouji base and density layers", () => {
    const raw = readFileSync(resolve(__dirname, "../../angular.json"), "utf8");
    const cfg = JSON.parse(raw) as {
      projects: Record<string, { architect: { build: { options: { styles: unknown[] } } } }>;
    };
    const styles = cfg.projects["orrery"].architect.build.options.styles
      .map((s) => (typeof s === "string" ? s : (s as { input: string }).input));

    const base = styles.indexOf("node_modules/@kouji-ui/themes/src/base.css");
    const density = styles.indexOf("node_modules/@kouji-ui/themes/src/density.css");
    const dark = styles.indexOf("src/themes/orrery.css");
    const light = styles.indexOf("src/themes/orrery-light.css");

    expect(base).toBeGreaterThanOrEqual(0);
    expect(density).toBeGreaterThan(base);
    expect(dark).toBeGreaterThan(density);
    expect(light).toBeGreaterThan(dark);
    // Nothing may still point at the removed upstream files.
    expect(styles.filter((s) => /kouji-ui\/themes\/src\/themes\//.test(s))).toEqual([]);
  });
});
