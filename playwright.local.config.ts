import { defineConfig } from "@playwright/test";

/**
 * Worktree-local Playwright config.
 *
 * The shared config pins port 1420 with `reuseExistingServer: true`. Several
 * Orrery worktrees exist side by side, so whichever one currently holds 1420
 * wins — and a run from THIS worktree silently tests THAT worktree's code.
 * That is a false pass (or a baffling false failure) with no visible cause.
 *
 * This config binds a port of its own so a run here always exercises the code
 * here, and never disturbs a dev server another worktree is using.
 */
const PORT = 4329;

export default defineConfig({
  testDir: "e2e",
  timeout: 60_000,
  use: { baseURL: `http://localhost:${PORT}` },
  webServer: {
    command: `npx ng serve --port ${PORT}`,
    url: `http://localhost:${PORT}`,
    // Never adopt a stranger's server — that is the bug this file exists to avoid.
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
