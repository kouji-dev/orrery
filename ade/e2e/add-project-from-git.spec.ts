import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the add-project "From Git URL" source: the modal gains a source
 * toggle; the git pane captures a repo URL + destination folder, a path mode
 * (root → path/<repo-name>, project → path IS the project, the "." clone) and
 * a shallow-clone toggle. Backend-free: everything here is pure modal UI —
 * submit wiring is covered by Rust tests on project_create.
 */

async function openModal(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(
    `window.ng.getComponent(document.querySelector("app-top-bar")).ui.openAddProject()`,
  );
  // the host tag has no box of its own (the overlay is position:fixed) — wait
  // for attachment, then for real content
  await page.waitForSelector("app-add-project-modal", { state: "attached" });
  await expect(page.getByRole("tab", { name: "From Git URL" })).toBeVisible();
}

const gitTab = (page: Page) => page.getByRole("tab", { name: "From Git URL" });

test("modal defaults to the local-folder source; the git pane is a toggle away", async ({ page }) => {
  await openModal(page);
  await expect(page.locator(`input[placeholder="~/code/my-repo"]`)).toBeVisible();
  await expect(page.locator(`input[placeholder="https://github.com/user/repo.git"]`)).toHaveCount(0);

  await gitTab(page).click();
  await expect(page.locator(`input[placeholder="https://github.com/user/repo.git"]`)).toBeVisible();
  await expect(page.getByText("Clone into", { exact: true })).toBeVisible();
  await expect(page.getByText("Use path as root")).toBeVisible();
  await expect(page.getByText("Use path as the project")).toBeVisible();
  await expect(page.getByText("Shallow clone (depth 1)")).toBeVisible();

  await page.getByRole("tab", { name: "Local folder" }).click();
  await expect(page.locator(`input[placeholder="~/code/my-repo"]`)).toBeVisible();
});

test("root mode previews path/<repo-name> and derives the project name from the url", async ({ page }) => {
  await openModal(page);
  await gitTab(page).click();

  await page.locator(`input[placeholder="https://github.com/user/repo.git"]`).fill("https://github.com/kouji/orrery.git");
  await page.locator(`input[placeholder="~/code"]`).fill("/home/me/code");

  // root mode (default): the clone lands in a repo-named subfolder
  await expect(page.getByText("clones to →")).toBeVisible();
  await expect(page.getByText("/home/me/code/orrery")).toBeVisible();
});

test("project mode uses the path itself as the clone destination", async ({ page }) => {
  await openModal(page);
  await gitTab(page).click();

  await page.locator(`input[placeholder="https://github.com/user/repo.git"]`).fill("https://github.com/kouji/orrery.git");
  await page.locator(`input[placeholder="~/code"]`).fill("/home/me/code/my-app");

  await page.getByText("Use path as the project").click();
  await expect(page.getByText("clones to →")).toBeVisible();
  await expect(page.getByText("/home/me/code/my-app", { exact: true })).toBeVisible();
  await expect(page.getByText("/home/me/code/my-app/orrery")).toHaveCount(0);
});

test("submit stays disabled until both url and destination are present", async ({ page }) => {
  await openModal(page);
  await gitTab(page).click();
  const submit = page
    .locator("app-add-project-modal")
    .getByRole("button", { name: "Add project" });

  await expect(submit).toBeDisabled();
  await page.locator(`input[placeholder="https://github.com/user/repo.git"]`).fill("https://github.com/kouji/orrery.git");
  await expect(submit).toBeDisabled();
  await page.locator(`input[placeholder="~/code"]`).fill("/home/me/code");
  await expect(submit).toBeEnabled();

  // clearing the url disables again
  await page.locator(`input[placeholder="https://github.com/user/repo.git"]`).fill("");
  await expect(submit).toBeDisabled();
});
