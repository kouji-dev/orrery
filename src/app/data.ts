// Orrery mock data — multi-project, multi-agent git orchestration
import { AgentTool, LogLine, Project } from "./models";

export const ORG = "northwind";
export const WORKTREE_ROOT = "~/.orrery/worktrees";

// ---------- agent tools (CLI coding agents) ----------
export const AGENT_TOOLS: AgentTool[] = [
  {
    id: "claude",
    name: "Claude Code",
    short: "claude",
    accent: "var(--tool-claude)",
    // aliases (not dated IDs) so they always resolve to the latest — fable is
    // Fable 5 (claude-fable-5), opus is 4.8 today. First entry = spawn default.
    models: ["fable", "opus", "sonnet", "haiku"],
    effort: false,
  },
  {
    id: "codex",
    name: "Codex",
    short: "codex",
    accent: "var(--tool-codex)",
    // gpt-5.6 tier (Sol = flagship/CLI default, Terra = balanced, Luna = fast)
    // GA July 2026; gpt-5.5 kept for pinned workflows
    models: ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5"],
    effort: ["low", "medium", "high", "xhigh"],
  },
  {
    id: "cursor",
    name: "Cursor",
    short: "cursor",
    accent: "var(--tool-cursor)",
    models: ["composer-2.5", "composer-2", "auto"],
    effort: false,
  },
  {
    id: "gemini",
    name: "Gemini",
    short: "gemini",
    accent: "var(--tool-gemini)",
    models: ["gemini-3-pro", "gemini-3-flash", "gemini-3.1-pro-preview", "gemini-2.5-flash"],
    effort: false,
  },
];

// ---------- projects (each = one git repo) ----------
export const PROJECTS: Project[] = [
  {
    id: "p_pay",
    name: "payments-service",
    org: "northwind",
    path: "~/code/northwind/payments-service",
    repo: "github.com/northwind/payments-service",
    branch: "main",
    head: "a3f91c2",
    color: "var(--id-1)",
    icon: "box",
    hasGit: true,
    folderExists: true,
    branches: ["main", "develop", "release/2.4", "hotfix/refund-rounding"],
    files: [
      "src/index.ts",
      "src/server.ts",
      "src/webhooks/stripe.ts",
      "src/webhooks/retry.ts",
      "src/auth/jwt.ts",
      "src/auth/refresh.ts",
      "src/middleware/session.ts",
      "src/jobs/reconcile.ts",
      "src/config/queue.ts",
      "test/webhooks/retry.test.ts",
      "test/integration/checkout.test.ts",
      "package.json",
      "tsconfig.json",
      "README.md",
    ],
  },
  {
    id: "p_web",
    name: "web-dashboard",
    org: "northwind",
    path: "~/code/northwind/web-dashboard",
    repo: "github.com/northwind/web-dashboard",
    branch: "main",
    head: "7d10b4e",
    color: "var(--id-2)",
    icon: "globe",
    hasGit: true,
    folderExists: true,
    branches: ["main", "develop", "feat/new-settings"],
    files: [
      "src/main.tsx",
      "src/App.tsx",
      "src/pages/Dashboard.tsx",
      "src/pages/Settings.tsx",
      "src/components/Nav.tsx",
      "src/components/Table.tsx",
      "src/styles/theme.css",
      "package.json",
      "vite.config.ts",
      "README.md",
    ],
  },
  {
    id: "p_infra",
    name: "infra-terraform",
    org: "northwind",
    path: "~/code/northwind/infra-terraform",
    repo: "github.com/northwind/infra-terraform",
    branch: "main",
    head: "f02ce91",
    color: "var(--id-3)",
    icon: "server",
    hasGit: true,
    folderExists: true,
    branches: ["main", "staging"],
    files: [
      "main.tf",
      "variables.tf",
      "outputs.tf",
      "modules/network/main.tf",
      "modules/db/main.tf",
      "modules/cache/main.tf",
      "README.md",
    ],
  },
];

export const STREAM: Record<string, LogLine[]> = {
  a1: [
    { t: "out", s: "src/webhooks/stripe.ts ... type-checking" },
    { t: "ok", s: "No errors. Clean exit 0" },
    { t: "cmd", s: "git add -A && git commit -m 'feat: retry stripe webhooks'" },
    { t: "ok", s: "[agent/stripe-retry 1f8c2a9] 4 files changed, 188 insertions(+)" },
    { t: "cmd", s: "pnpm eslint --max-warnings 0 src/webhooks" },
    { t: "ok", s: "✓ no lint problems" },
    { t: "sys", s: "verifying retry backoff schedule: 1s · 2s · 4s · 8s · 16s" },
    { t: "ok", s: "all acceptance criteria met — preparing summary" },
  ],
  c3: [
    { t: "out", s: "extracting step 1/5 → steps/fetch-ledger.ts" },
    { t: "out", s: "extracting step 2/5 → steps/match-transactions.ts" },
    { t: "out", s: "extracting step 3/5 → steps/flag-discrepancies.ts" },
    { t: "warn", s: "step 4 shares state with step 2 — threading context" },
    { t: "out", s: "extracting step 5/5 → steps/emit-report.ts" },
    { t: "cmd", s: "pnpm vitest run src/jobs/steps" },
    { t: "out", s: " ✓ 5 step modules (22 tests) 980ms" },
  ],
  g7: [
    { t: "out", s: "Settings.tsx: replacing legacy color vars" },
    { t: "out", s: "wired <Toggle/> to preferences store" },
    { t: "cmd", s: "pnpm test src/pages/Settings.test.tsx" },
    { t: "ok", s: " ✓ 9 tests passed" },
  ],
};

// spawn helpers
export const PROJECT_COLORS = ["var(--id-1)", "var(--id-2)", "var(--id-3)", "var(--id-4)", "var(--id-5)", "var(--id-6)", "var(--id-7)"];
export const PROJECT_ICONS = ["box", "globe", "server", "database", "grid", "layers", "terminal", "spark"];
