import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the backlog TICKET CARD actions: an Edit icon button that opens the
 * ticket already in edit mode, and Dispatch as the card's one filled primary —
 * it used to be an outline that only promoted itself on hover, which hid which
 * action was primary on a keyboard or touch pass.
 *
 * Backend-free: tickets are seeded straight into TicketsStore.
 */

const seedProject = `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "p-bk", name: "bk-proj", path: "C:/bk", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main",
  });
})()`;

const seedTicket = (id: string, title: string, status: string) => `(() => {
  const bl = window.ng.getComponent(document.querySelector("app-backlog"));
  bl.tickets["store"].upsert({
    id: "${id}", title: "${title}", notes: "", status: "${status}",
    projectId: "p-bk", agentId: null, tags: [], createdAt: Date.now(), updatedAt: Date.now(),
  });
})()`;

const ui = (expr: string) =>
  `window.ng.getComponent(document.querySelector("app-top-bar")).ui${expr}`;

async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject);
  await page.evaluate(ui(`.openBacklog()`));
  await page.waitForSelector("app-backlog");
  await page.evaluate(seedTicket("t-bk1", "Fix the fetch race", "todo"));
  await page.evaluate(seedTicket("t-bk2", "Ship the dock", "inprogress"));
  await page.waitForSelector("app-ticket-card");
}

test("Dispatch is the filled primary without hovering", async ({ page }) => {
  await boot(page);
  const card = page.locator("app-ticket-card", { hasText: "Fix the fetch race" });
  const dispatch = card.getByRole("button", { name: /Dispatch/ });
  // kouji mirrors the resolved variant onto the inner button
  await expect(dispatch).toHaveAttribute("data-variant", "default");
});

test("the Edit icon button opens the ticket already in edit mode", async ({ page }) => {
  await boot(page);
  const card = page.locator("app-ticket-card", { hasText: "Fix the fetch race" });

  const edit = card.getByRole("button", { name: "Edit ticket" });
  await expect(edit).toBeVisible();
  // icon-only: it carries a glyph, never a text label
  await expect(edit).toHaveText("");

  await edit.click();
  const page$ = page.locator("app-ticket-page");
  await expect(page$).toBeVisible();
  // edit mode = the title is an input, and Save/Cancel replace the action row
  await expect(page$.locator(`input[placeholder^="Ticket title"]`)).toBeVisible();
  await expect(page$.getByRole("button", { name: "Cancel" })).toBeVisible();
});

test("an in-progress card is editable too", async ({ page }) => {
  await boot(page);
  const card = page.locator("app-ticket-card", { hasText: "Ship the dock" });
  await expect(card.getByRole("button", { name: "Edit ticket" })).toBeVisible();
  // …but it is not dispatchable — that action belongs to a todo card
  await expect(card.getByRole("button", { name: /Dispatch/ })).toHaveCount(0);
});

test("clicking the card body still opens the ticket in READ mode", async ({ page }) => {
  await boot(page);
  await page.locator("app-ticket-card", { hasText: "Fix the fetch race" }).click();
  const page$ = page.locator("app-ticket-page");
  await expect(page$).toBeVisible();
  // no edit was requested, so the title stays a heading and Edit is still offered
  await expect(page$.locator(`input[placeholder^="Ticket title"]`)).toHaveCount(0);
  await expect(page$.getByRole("button", { name: /Edit/ })).toBeVisible();
});
