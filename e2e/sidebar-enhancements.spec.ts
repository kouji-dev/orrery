import { expect, Page, test } from "@playwright/test";

/**
 * E2E for the sidebar enhancements from design/orrery-v2.html:
 *   - a collapse/expand-ALL projects button in the Projects header strip
 *   - "Add project" / "Agent" moved ABOVE the list (the files section owns the
 *     sidebar's bottom edge now)
 *   - a PROJECT-FIRST filter: a query naming a project keeps all of its agents,
 *     otherwise the query falls through to agent name/task.
 *
 * Backend-free: projects/agents are seeded straight into their stores.
 */

const seedProject = (id: string, name: string) => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.projects["projectsStore"]["store"].upsert({
    id: "${id}", name: "${name}", path: "C:/e2e/${name}", icon: "box", color: "#22d3ee",
    folderExists: true, hasGit: true, branch: "main",
  });
})()`;

const seedAgent = (projectId: string, id: string, name: string, task = "") => `(() => {
  const bar = window.ng.getComponent(document.querySelector("app-top-bar"));
  bar.agentActions["agentsStore"]["store"].upsert({
    id: "${id}", projectId: "${projectId}", tool: "claude", model: "m", name: "${name}",
    task: "${task}", status: "idle", branch: "agent/${name}", worktree: "C:/wt/${name}", base: "main",
    commits: 0, elapsed: 0, progress: 0, pending: [],
  });
})()`;

/** Two projects: "orrery" (2 agents) and "kouji" (1 agent that names orrery). */
async function boot(page: Page): Promise<void> {
  await page.goto("/");
  await page.waitForSelector("app-top-bar");
  await page.evaluate(seedProject("p-orrery", "orrery"));
  await page.evaluate(seedProject("p-kouji", "kouji"));
  await page.evaluate(seedAgent("p-orrery", "e2e-sbe1", "alpha", "wire the sidebar"));
  await page.evaluate(seedAgent("p-orrery", "e2e-sbe2", "beta", "ship the release"));
  await page.evaluate(seedAgent("p-kouji", "e2e-sbe3", "gamma", "port to orrery"));
}

const filter = (page: Page) => page.locator("app-sidebar kj-input input");
const groups = (page: Page) => page.locator("app-sidebar app-project-group");
const rows = (page: Page) => page.locator("app-sidebar app-agent-row");

test("collapse-all folds every project away, and the button flips back", async ({ page }) => {
  await boot(page);
  await expect(rows(page)).toHaveCount(3);

  const all = page.locator("app-sidebar kj-button[title='Collapse all projects']");
  await all.click();
  // headers stay, agent rows are gone
  await expect(groups(page)).toHaveCount(2);
  await expect(rows(page)).toHaveCount(0);

  const expand = page.locator("app-sidebar kj-button[title='Expand all projects']");
  await expect(expand).toBeVisible();
  await expand.click();
  await expect(rows(page)).toHaveCount(3);
});

test("collapsing every project one at a time flips the button to expand-all", async ({ page }) => {
  await boot(page);
  // the button only reads "expand all" once NOTHING is left open
  await groups(page).first().getByText("orrery", { exact: true }).click();
  await expect(page.locator("app-sidebar kj-button[title='Collapse all projects']")).toBeVisible();
  await groups(page).last().getByText("kouji", { exact: true }).click();
  await expect(page.locator("app-sidebar kj-button[title='Expand all projects']")).toBeVisible();
});

test("panel actions sit above the list, not in a footer", async ({ page }) => {
  await boot(page);
  const actions = page.locator("app-sidebar .sb-actions");
  await expect(actions).toBeVisible();
  await expect(actions.getByText("Add project")).toBeVisible();
  await expect(actions.getByText("Agent")).toBeVisible();

  // above the scrolling group list, and above the files section
  const actionsBox = (await actions.boundingBox())!;
  const listBox = (await groups(page).first().boundingBox())!;
  const filesBox = (await page.locator("app-sidebar-files").boundingBox())!;
  expect(actionsBox.y).toBeLessThan(listBox.y);
  expect(actionsBox.y).toBeLessThan(filesBox.y);
});

test("a project-name match keeps ALL of that project's agents", async ({ page }) => {
  await boot(page);
  await filter(page).fill("orrery");

  // orrery matched by NAME -> alpha + beta both survive, even though neither
  // agent's own name contains "orrery"
  await expect(groups(page)).toHaveCount(2);
  await expect(rows(page)).toHaveCount(3);
  await expect(page.locator("app-sidebar")).toContainText("alpha");
  await expect(page.locator("app-sidebar")).toContainText("beta");
  // kouji did NOT match by name, so it keeps only its matching agent
  await expect(page.locator("app-sidebar")).toContainText("gamma");
});

test("no project matches: the query falls through to agent name and task", async ({ page }) => {
  await boot(page);
  await filter(page).fill("alpha");
  await expect(groups(page)).toHaveCount(1);
  await expect(rows(page)).toHaveCount(1);
  await expect(page.locator("app-sidebar")).toContainText("alpha");

  // task text matches too
  await filter(page).fill("release");
  await expect(rows(page)).toHaveCount(1);
  await expect(page.locator("app-sidebar")).toContainText("beta");

  // nothing at all
  await filter(page).fill("zzzz");
  await expect(groups(page)).toHaveCount(0);
});

test("a named project with no matching agents still shows, empty", async ({ page }) => {
  await boot(page);
  await page.evaluate(seedProject("p-empty", "solaris"));
  await filter(page).fill("solaris");
  await expect(groups(page)).toHaveCount(1);
  await expect(page.locator("app-sidebar")).toContainText("solaris");
  await expect(rows(page)).toHaveCount(0);
});

test("clearing the filter brings every project back", async ({ page }) => {
  await boot(page);
  await filter(page).fill("alpha");
  await expect(groups(page)).toHaveCount(1);
  await page.locator("app-sidebar kj-input-group-addon app-icon[name='x']").click();
  await expect(groups(page)).toHaveCount(2);
  await expect(rows(page)).toHaveCount(3);
});
