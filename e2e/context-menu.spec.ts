import { expect, test } from "@playwright/test";

/**
 * E2E for the shared context-menu chrome (MenuPanelComponent + global
 * .menu-panel/.menu-item classes): items must visibly highlight on hover.
 * Items carry kouji's kjDropdownMenuItem now, so they expose role="menuitem".
 * Hover uses --elev-hover — on dark, --elev sits ABOVE --panel-3, so the old
 * panel-3 hover computed to a near-invisible darkening (the reported bug).
 */

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

test("menu items highlight on hover; danger gets its own tint; Escape closes", async ({ page }) => {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(
    ui(`.openMenu(
      { clientX: 120, clientY: 120, preventDefault() {}, stopPropagation() {} },
      [
        { label: "Alpha", icon: "file", onClick() {} },
        { label: "Beta", icon: "file", onClick() {} },
        { sep: true },
        { label: "Delete", icon: "trash", danger: true, onClick() {} },
      ],
    )`),
  );
  const menu = page.locator(".menu-panel");
  await expect(menu).toBeVisible();

  // Alpha takes roving focus when the menu opens (and paints :focus-visible),
  // so the idle/hover pair is read off Beta, which nothing has touched
  const beta = menu.getByRole("menuitem", { name: "Beta" });
  const idle = await beta.evaluate((el) => getComputedStyle(el).backgroundColor);
  await beta.hover();
  const hovered = await beta.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(idle).toBe("rgba(0, 0, 0, 0)");
  expect(hovered).toBe("rgb(55, 57, 61)"); // --elev-hover (dark default theme)

  // danger hover tints from the danger color, not the neutral hover
  const del = menu.getByRole("menuitem", { name: "Delete" });
  await del.hover();
  const dangerHover = await del.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(dangerHover).not.toBe("rgba(0, 0, 0, 0)");
  expect(dangerHover).not.toBe(hovered);

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});
