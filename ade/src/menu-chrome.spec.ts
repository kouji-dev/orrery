import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(resolve(__dirname, "styles.css"), "utf8");
const kj = readFileSync(resolve(__dirname, "../node_modules/@kouji-ui/components/src/dropdown-menu/dropdown-menu.css"), "utf8");

/** The one menu recipe (styles.css "context menus") vs the kouji dropdown
 *  sheet it now shares the page with. */
describe("menu chrome", () => {
  it("kouji's dropdown-menu item still ships a touch-target floor in a cascade layer", () => {
    // if either of these stops holding, the override below may be dead weight — or insufficient
    expect(kj).toMatch(/^@layer kj\.component \{/);
    expect(kj).toMatch(/--kj-dropdown-menu-item-min-height:\s*2\.75rem/);
    expect(kj).toMatch(/min-height:\s*var\(--kj-dropdown-menu-item-min-height\)/);
  });

  it("a .menu-item row is sized by its padding alone — the kouji floor is zeroed on the row and its knob", () => {
    const block = css.match(/\n\.menu-item \{([^}]*)\}/)?.[1] ?? "";
    expect(block).toMatch(/min-height:\s*0;/);
    expect(block).toMatch(/--kj-dropdown-menu-item-min-height:\s*0;/);
    expect(block).toMatch(/padding:\s*var\(--sp-3\) var\(--sp-4\);/);
  });

  it("the kj-button menu rows (file menus, git split-button) zero the same knob", () => {
    const block = css.match(/\nkj-button\.menu-item,\n\.row kj-button,\n\.row \.kj-button \{([^}]*)\}/)?.[1] ?? "";
    expect(block).toMatch(/--kj-button-height:\s*auto;/);
    expect(block).toMatch(/--kj-dropdown-menu-item-min-height:\s*0;/);
  });

  it("the app-menu-panel host is out of flow — a menu never becomes a grid item in the panel it opens over", () => {
    // regression: <app-menu-panel> sits as a direct child of .diff-grid
    // (232px 6px 1fr). As an in-flow inline box it counted as a 4th grid item,
    // wrapping the diff body onto row 2 — the code view vanished below the fold.
    const block = css.match(/\napp-menu-panel \{([^}]*)\}/)?.[1] ?? "";
    expect(block).toMatch(/position:\s*fixed;/);
    expect(block).toMatch(/width:\s*0;/);
    expect(block).toMatch(/height:\s*0;/);
  });
});
