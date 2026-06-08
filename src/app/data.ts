// Orrery mock data — multi-project, multi-agent git orchestration
import { AgentTool, Commit, Diff, LogLine, Project } from "./models";

export const ORG = "northwind";
export const WORKTREE_ROOT = "~/.orrery/worktrees";

// ---------- agent tools (CLI coding agents) ----------
export const AGENT_TOOLS: AgentTool[] = [
  {
    id: "claude",
    name: "Claude Code",
    short: "claude",
    accent: "#d98a5b",
    // aliases (not dated IDs) so they always resolve to the latest — opus is 4.8 today
    models: ["opus", "sonnet", "haiku"],
    effort: false,
  },
  {
    id: "codex",
    name: "Codex",
    short: "codex",
    accent: "#10a37f",
    models: ["gpt-5.1-codex", "gpt-5.1-codex-mini"],
    effort: ["low", "medium", "high"],
  },
  {
    id: "cursor",
    name: "Cursor",
    short: "cursor",
    accent: "#6e9bff",
    models: ["composer-1", "auto"],
    effort: false,
  },
  {
    id: "gemini",
    name: "Gemini",
    short: "gemini",
    accent: "#8a7cff",
    models: ["gemini-2.5-pro", "gemini-2.5-flash"],
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
    color: "#a855f7",
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
    color: "#22d3ee",
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
    color: "#34e0a1",
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


// terminal log lines per agent
export const LOGS: Record<string, LogLine[]> = {
  a1: [
    { t: "sys", s: "worktree mounted → worktrees/agent-a1 (claude · sonnet-4.6)" },
    { t: "cmd", s: "git checkout -b agent/stripe-retry a3f91c2" },
    { t: "out", s: "Switched to a new branch 'agent/stripe-retry'" },
    { t: "cmd", s: "pnpm install --frozen-lockfile" },
    { t: "ok", s: "Packages: +0  done in 1.2s" },
    { t: "cmd", s: "pnpm vitest run test/webhooks/retry.test.ts" },
    { t: "out", s: " ✓ retry.test.ts (6 tests) 412ms" },
    { t: "ok", s: "Test Files  1 passed (1)   Tests  6 passed (6)" },
    { t: "cmd", s: "pnpm tsc --noEmit" },
  ],
  c3: [
    { t: "sys", s: "worktree mounted → worktrees/agent-c3 (codex · gpt-5.1-codex high)" },
    { t: "cmd", s: "git checkout -b agent/reconcile-refactor a3f91c2" },
    { t: "cmd", s: "rg -n 'reconcile' src/jobs" },
    { t: "out", s: "src/jobs/reconcile.ts:14: export async function reconcile()" },
    { t: "warn", s: "function reconcile() is 240 lines — splitting into steps/" },
    { t: "cmd", s: "pnpm vitest run src/jobs" },
  ],
  b2: [
    { t: "sys", s: "worktree mounted → worktrees/agent-b2 (claude · opus-4.6)" },
    { t: "cmd", s: "pnpm add jose @types/ms" },
    { t: "ok", s: "+ jose 5.2.0   + @types/ms 0.7.34" },
    { t: "cmd", s: "pnpm vitest run src/auth" },
    { t: "out", s: " ✓ jwt.test.ts (11)   ✗ refresh.test.ts (2 failed)" },
    { t: "err", s: "AssertionError: refresh token store not configured" },
    { t: "sys", s: "⏸  paused — awaiting human decision (storage backend)" },
  ],
  d4: [
    { t: "sys", s: "worktree mounted → worktrees/agent-d4 (claude · sonnet-4.6)" },
    { t: "cmd", s: "pnpm vitest run test/integration/checkout.test.ts" },
    { t: "out", s: " ✓ checkout.test.ts (14 tests) 1.8s" },
    { t: "ok", s: "Test Files 1 passed   Tests 14 passed" },
    { t: "cmd", s: "git commit -am 'test: checkout capture integration suite'" },
    { t: "ok", s: "[agent/checkout-tests 9b21e07] 3 files changed, 236 insertions(+)" },
    { t: "sys", s: "✓ task complete — ready to merge into main" },
  ],
  e5: [
    { t: "sys", s: "worktree mounted → worktrees/agent-e5 (gemini · 2.5-flash)" },
    { t: "cmd", s: "node -v && echo 'target: v22'" },
    { t: "out", s: "v20.11.0" },
    { t: "cmd", s: "gh workflow run ci.yml --ref agent/node22" },
    { t: "warn", s: "3 checks queued: lint · typecheck · test-matrix" },
  ],
  f6: [{ t: "sys", s: "queued — awaiting worktree allocation" }],
  g7: [
    { t: "sys", s: "worktree mounted → worktrees/agent-g7 (claude · sonnet-4.6)" },
    { t: "cmd", s: "pnpm dev --filter web-dashboard" },
    { t: "out", s: "VITE v5.4.0  ready in 612 ms" },
    { t: "cmd", s: "applying new design tokens to Settings.tsx" },
    { t: "out", s: "rebuilt 3 components" },
  ],
  h8: [
    { t: "sys", s: "worktree mounted → worktrees/agent-h8 (codex · gpt-5.1-codex medium)" },
    { t: "cmd", s: "pnpm axe ./src --tags wcag2aa" },
    { t: "warn", s: "14 contrast violations · 6 missing focus styles" },
    { t: "sys", s: "⏸ awaiting review of focus-trap approach" },
  ],
  i9: [
    { t: "sys", s: "worktree mounted → worktrees/agent-i9 (gemini · 2.5-pro)" },
    { t: "cmd", s: "terraform fmt -recursive && terraform validate" },
    { t: "ok", s: "Success! The configuration is valid." },
    { t: "cmd", s: "git commit -am 'refactor: split into modules'" },
    { t: "ok", s: "[agent/tf-modules 4cf0a2b] 5 files changed" },
    { t: "sys", s: "✓ task complete — ready to merge into main" },
  ],
};

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

export const DIFFS: Record<string, Diff> = {
  a1: {
    file: "src/webhooks/retry.ts",
    lang: "typescript",
    hunks: [
      {
        meta: "@@ -0,0 +1,21 @@ new file",
        lines: [
          { k: "+", n: 1, s: "import { setTimeout as sleep } from 'node:timers/promises';" },
          { k: "+", n: 2, s: "" },
          { k: "+", n: 3, s: "const SCHEDULE = [1_000, 2_000, 4_000, 8_000, 16_000] as const;" },
          { k: "+", n: 4, s: "" },
          { k: "+", n: 5, s: "export async function withRetry<T>(" },
          { k: "+", n: 6, s: "  fn: () => Promise<T>," },
          { k: "+", n: 7, s: "  opts: { maxAttempts?: number } = {}," },
          { k: "+", n: 8, s: "): Promise<T> {" },
          { k: "+", n: 9, s: "  const max = opts.maxAttempts ?? SCHEDULE.length;" },
          { k: "+", n: 10, s: "  let lastErr: unknown;" },
          { k: "+", n: 11, s: "  for (let attempt = 0; attempt < max; attempt++) {" },
          { k: "+", n: 12, s: "    try { return await fn(); }" },
          { k: "+", n: 13, s: "    catch (err) {" },
          { k: "+", n: 14, s: "      if (!isRetryable(err)) throw err;" },
          { k: "+", n: 15, s: "      await sleep(SCHEDULE[attempt] ?? 16_000);" },
          { k: "+", n: 16, s: "    }" },
          { k: "+", n: 17, s: "  }" },
          { k: "+", n: 18, s: "  throw lastErr;" },
          { k: "+", n: 19, s: "}" },
        ],
      },
    ],
  },
  b2: {
    file: "src/auth/refresh.ts",
    lang: "typescript",
    hunks: [
      {
        meta: "@@ -12,7 +12,14 @@ export class RefreshStore",
        lines: [
          { k: " ", n: 12, s: "export class RefreshStore {" },
          { k: "-", n: 13, s: "  private tokens = new Map<string, Token>();" },
          { k: "+", n: 13, s: "  // TODO(human): pick a backend — Redis or Postgres?" },
          { k: "+", n: 14, s: "  constructor(private store: TokenBackend) {}" },
          { k: " ", n: 15, s: "" },
          { k: "+", n: 16, s: "  async rotate(old: string): Promise<Token> {" },
          { k: "+", n: 17, s: "    await this.store.revoke(old);" },
          { k: "+", n: 18, s: "    return this.store.issue();" },
          { k: "+", n: 19, s: "  }" },
          { k: " ", n: 20, s: "}" },
        ],
      },
    ],
  },
  c3: {
    file: "src/jobs/reconcile.ts",
    lang: "typescript",
    hunks: [
      {
        meta: "@@ -14,80 +14,9 @@ export async function reconcile()",
        lines: [
          { k: " ", n: 14, s: "export async function reconcile(date: Date) {" },
          { k: "-", n: 15, s: "  const ledger = await db.ledger.forDate(date);" },
          { k: "-", n: 16, s: "  // ...238 lines of nested logic..." },
          { k: "+", n: 15, s: "  const ctx = createContext(date);" },
          { k: "+", n: 16, s: "  for (const step of STEPS) {" },
          { k: "+", n: 17, s: "    await step.run(ctx);   // idempotent" },
          { k: "+", n: 18, s: "  }" },
          { k: "+", n: 19, s: "  return ctx.report;" },
          { k: " ", n: 20, s: "}" },
        ],
      },
    ],
  },
  d4: {
    file: "test/integration/checkout.test.ts",
    lang: "typescript",
    hunks: [
      {
        meta: "@@ -0,0 +1,8 @@ new file",
        lines: [
          { k: "+", n: 1, s: "describe('checkout → capture', () => {" },
          { k: "+", n: 2, s: "  it('captures an authorized payment', async () => {" },
          { k: "+", n: 3, s: "    const intent = await api.createIntent({ amount: 4200 });" },
          { k: "+", n: 4, s: "    await api.confirm(intent.id, cards.visa);" },
          { k: "+", n: 5, s: "    const res = await api.capture(intent.id);" },
          { k: "+", n: 6, s: "    expect(res.status).toBe('succeeded');" },
          { k: "+", n: 7, s: "  });" },
          { k: "+", n: 8, s: "});" },
        ],
      },
    ],
  },
  e5: {
    file: "package.json",
    lang: "json",
    hunks: [
      {
        meta: "@@ -2,7 +2,7 @@",
        lines: [
          { k: " ", n: 2, s: '  "name": "payments-service",' },
          { k: "-", n: 3, s: '  "engines": { "node": ">=20" },' },
          { k: "+", n: 3, s: '  "engines": { "node": ">=22" },' },
          { k: " ", n: 4, s: '  "dependencies": {' },
          { k: "-", n: 5, s: '    "fastify": "4.26.0",' },
          { k: "+", n: 5, s: '    "fastify": "5.1.0",' },
        ],
      },
    ],
  },
  g7: {
    file: "src/pages/Settings.tsx",
    lang: "tsx",
    hunks: [
      {
        meta: "@@ -8,10 +8,14 @@ export function Settings()",
        lines: [
          { k: " ", n: 8, s: "export function Settings() {" },
          { k: "-", n: 9, s: '  return <div className="settings legacy">' },
          { k: "+", n: 9, s: '  return <div className="settings" data-theme={theme}>' },
          { k: "+", n: 10, s: '    <Toggle label="Dark mode" bind={prefs.dark} />' },
          { k: "+", n: 11, s: '    <Toggle label="Compact rows" bind={prefs.compact} />' },
          { k: " ", n: 12, s: "  </div>;" },
        ],
      },
    ],
  },
  h8: {
    file: "src/components/Nav.tsx",
    lang: "tsx",
    hunks: [
      {
        meta: "@@ -22,6 +22,9 @@",
        lines: [
          { k: " ", n: 22, s: "  <a href={item.href}" },
          { k: "-", n: 23, s: '     className="nav-link">' },
          { k: "+", n: 23, s: '     className="nav-link"' },
          { k: "+", n: 24, s: "     aria-current={active ? 'page' : undefined}" },
          { k: "+", n: 25, s: "     tabIndex={0}>" },
        ],
      },
    ],
  },
  i9: {
    file: "modules/network/main.tf",
    lang: "hcl",
    hunks: [
      {
        meta: "@@ -0,0 +1,9 @@ new file",
        lines: [
          { k: "+", n: 1, s: 'resource "aws_vpc" "main" {' },
          { k: "+", n: 2, s: "  cidr_block = var.vpc_cidr" },
          { k: "+", n: 3, s: '  tags = { Name = "${var.env}-vpc" }' },
          { k: "+", n: 4, s: "}" },
          { k: "+", n: 5, s: "" },
          { k: "+", n: 6, s: 'resource "aws_subnet" "public" {' },
          { k: "+", n: 7, s: "  count      = length(var.azs)" },
          { k: "+", n: 8, s: "  vpc_id     = aws_vpc.main.id" },
          { k: "+", n: 9, s: "}" },
        ],
      },
    ],
  },
};

export const COMMITS: Commit[] = [
  { agent: "i9", projectId: "p_infra", sha: "4cf0a2b", msg: "refactor: split into network/db/cache modules", when: "12m", files: 5 },
  { agent: "d4", projectId: "p_pay", sha: "9b21e07", msg: "test: checkout capture integration suite", when: "2m", files: 3 },
  { agent: "b2", projectId: "p_pay", sha: "7c4d11a", msg: "feat(auth): rotating refresh token store", when: "5m", files: 2 },
  { agent: "a1", projectId: "p_pay", sha: "1f8c2a9", msg: "feat: retry stripe webhooks w/ backoff", when: "6m", files: 4 },
  { agent: "g7", projectId: "p_web", sha: "e22a7f1", msg: "feat(settings): adopt new design tokens", when: "8m", files: 3 },
  { agent: "c3", projectId: "p_pay", sha: "b2a7c80", msg: "refactor: extract reconcile context", when: "18m", files: 1 },
];

// spawn helpers
export const PALETTES: Record<string, [string, string]> = {
  Nebula: ["#a855f7", "#22d3ee"],
  Plasma: ["#ff4d8d", "#a855f7"],
  Reactor: ["#00e5c7", "#3b82f6"],
  Ember: ["#ff6b35", "#ffd23f"],
};
export const PROJECT_COLORS = ["#a855f7", "#22d3ee", "#34e0a1", "#ff6b35", "#ff4d8d", "#3b82f6", "#f5c451"];
export const PROJECT_ICONS = ["box", "globe", "server", "database", "grid", "layers", "terminal", "spark"];
export const SPAWN_NAMES = ["refund-flow", "rate-limiter", "audit-log", "webhook-dlq", "fee-engine", "ledger-sync", "fraud-rules", "currency-fx"];
