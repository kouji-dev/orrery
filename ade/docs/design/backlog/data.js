/* global window */
// ORCHESTRA mock data — multi-project, multi-agent git orchestration

// ---------- agent tools (CLI coding agents) ----------
const AGENT_TOOLS = [
  { id: "claude", name: "Claude Code", short: "claude", accent: "#d98a5b",
    models: ["opus-4.6", "sonnet-4.6", "haiku-4.6"], effort: false },
  { id: "codex", name: "Codex", short: "codex", accent: "#10a37f",
    models: ["gpt-5.1-codex", "gpt-5.1-codex-mini"], effort: ["low", "medium", "high"] },
  { id: "cursor", name: "Cursor", short: "cursor", accent: "#6e9bff",
    models: ["composer-1", "auto"], effort: false },
  { id: "gemini", name: "Gemini", short: "gemini", accent: "#8a7cff",
    models: ["gemini-2.5-pro", "gemini-2.5-flash"], effort: false },
];

// ---------- projects (each = one git repo) ----------
const PROJECTS = [
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
    branches: ["main", "develop", "release/2.4", "hotfix/refund-rounding"],
    files: [
      "src/index.ts", "src/server.ts",
      "src/webhooks/stripe.ts", "src/webhooks/retry.ts",
      "src/auth/jwt.ts", "src/auth/refresh.ts", "src/middleware/session.ts",
      "src/jobs/reconcile.ts", "src/config/queue.ts",
      "test/webhooks/retry.test.ts", "test/integration/checkout.test.ts",
      "package.json", "tsconfig.json", "README.md",
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
    branches: ["main", "develop", "feat/new-settings"],
    files: [
      "src/main.tsx", "src/App.tsx",
      "src/pages/Dashboard.tsx", "src/pages/Settings.tsx",
      "src/components/Nav.tsx", "src/components/Table.tsx",
      "src/styles/theme.css",
      "package.json", "vite.config.ts", "README.md",
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
    branches: ["main", "staging"],
    files: [
      "main.tf", "variables.tf", "outputs.tf",
      "modules/network/main.tf", "modules/db/main.tf", "modules/cache/main.tf",
      "README.md",
    ],
  },
];

// status: running | blocked | waiting | done | idle | queued
const AGENTS = [
  {
    id: "a1", projectId: "p_pay", tool: "claude", model: "sonnet-4.6",
    name: "stripe-retry",
    task: "Add exponential-backoff retry to Stripe webhook handler",
    status: "running", branch: "agent/stripe-retry", worktree: "agent-a1",
    base: "a3f91c2", commits: 2, elapsed: 412, progress: 0.68,
    files: [
      { path: "src/webhooks/stripe.ts", add: 34, del: 6, state: "M" },
      { path: "src/webhooks/retry.ts", add: 88, del: 0, state: "A" },
      { path: "src/config/queue.ts", add: 5, del: 2, state: "M" },
      { path: "test/webhooks/retry.test.ts", add: 61, del: 0, state: "A" },
    ],
    pending: [
      { id: "pm1", kind: "permission", title: "Run shell command", cmd: "pnpm add p-retry@5", when: "now" },
    ],
  },
  {
    id: "b2", projectId: "p_pay", tool: "claude", model: "opus-4.6",
    name: "jwt-refresh",
    task: "Migrate auth to short-lived JWT + rotating refresh tokens",
    status: "blocked", branch: "agent/jwt-refresh", worktree: "agent-b2",
    base: "a3f91c2", commits: 4, elapsed: 1284, progress: 0.52,
    blockReason: "Needs decision: store refresh tokens in Redis or Postgres?",
    files: [
      { path: "src/auth/jwt.ts", add: 120, del: 44, state: "M" },
      { path: "src/auth/refresh.ts", add: 96, del: 0, state: "A" },
      { path: "src/middleware/session.ts", add: 18, del: 31, state: "M" },
      { path: "migrations/0042_refresh_tokens.sql", add: 22, del: 0, state: "A" },
    ],
    pending: [
      { id: "pm2", kind: "decision", title: "Storage backend decision", cmd: "Where should refresh tokens live? Redis is fast but volatile; Postgres is durable and auditable.", when: "4m", options: ["Redis", "Postgres"] },
    ],
  },
  {
    id: "c3", projectId: "p_pay", tool: "codex", model: "gpt-5.1-codex", effort: "high",
    name: "reconcile-refactor",
    task: "Refactor nightly reconciliation job into idempotent steps",
    status: "running", branch: "agent/reconcile-refactor", worktree: "agent-c3",
    base: "a3f91c2", commits: 1, elapsed: 196, progress: 0.34,
    files: [
      { path: "src/jobs/reconcile.ts", add: 47, del: 80, state: "M" },
      { path: "src/jobs/steps/", add: 210, del: 0, state: "A" },
    ],
    pending: [],
  },
  {
    id: "d4", projectId: "p_pay", tool: "claude", model: "sonnet-4.6",
    name: "checkout-tests",
    task: "Write integration tests for the checkout → capture flow",
    status: "done", branch: "agent/checkout-tests", worktree: "agent-d4",
    base: "a3f91c2", commits: 3, elapsed: 642, progress: 1,
    files: [
      { path: "test/integration/checkout.test.ts", add: 184, del: 0, state: "A" },
      { path: "test/fixtures/cards.ts", add: 40, del: 0, state: "A" },
      { path: "test/helpers/server.ts", add: 12, del: 4, state: "M" },
    ],
    pending: [
      { id: "pm3", kind: "review", title: "Ready to merge", cmd: "agent/checkout-tests → main", when: "2m" },
    ],
  },
  {
    id: "e5", projectId: "p_pay", tool: "gemini", model: "gemini-2.5-flash",
    name: "node22",
    task: "Upgrade runtime to Node 22 and bump all dependencies",
    status: "waiting", branch: "agent/node22", worktree: "agent-e5",
    base: "a3f91c2", commits: 0, elapsed: 38, progress: 0.08,
    waitReason: "Waiting on CI: 3 checks queued",
    files: [
      { path: "package.json", add: 28, del: 28, state: "M" },
      { path: ".nvmrc", add: 1, del: 1, state: "M" },
    ],
    pending: [
      { id: "pm4", kind: "permission", title: "Push branch to origin", cmd: "git push -u origin agent/node22", when: "1m" },
    ],
  },
  {
    id: "f6", projectId: "p_pay", tool: "cursor", model: "composer-1",
    name: "idempotency-keys",
    task: "Add idempotency keys to the public payments API",
    status: "queued", branch: "agent/idempotency-keys", worktree: "agent-f6",
    base: "a3f91c2", commits: 0, elapsed: 0, progress: 0,
    files: [], pending: [],
  },
  {
    id: "g7", projectId: "p_web", tool: "claude", model: "sonnet-4.6",
    name: "settings-redesign",
    task: "Rebuild the account settings page with the new design tokens",
    status: "running", branch: "agent/settings-redesign", worktree: "agent-g7",
    base: "7d10b4e", commits: 2, elapsed: 308, progress: 0.46,
    files: [
      { path: "src/pages/Settings.tsx", add: 142, del: 96, state: "M" },
      { path: "src/styles/theme.css", add: 38, del: 4, state: "M" },
      { path: "src/components/Toggle.tsx", add: 54, del: 0, state: "A" },
    ],
    pending: [],
  },
  {
    id: "h8", projectId: "p_web", tool: "codex", model: "gpt-5.1-codex", effort: "medium",
    name: "a11y-audit",
    task: "Fix WCAG AA contrast + keyboard-nav issues across the dashboard",
    status: "waiting", branch: "agent/a11y-audit", worktree: "agent-h8",
    base: "7d10b4e", commits: 1, elapsed: 122, progress: 0.21,
    waitReason: "Waiting on review of focus-trap approach",
    files: [
      { path: "src/components/Nav.tsx", add: 22, del: 14, state: "M" },
      { path: "src/components/Table.tsx", add: 31, del: 9, state: "M" },
    ],
    pending: [
      { id: "pm5", kind: "decision", title: "Focus-trap approach", cmd: "Keep focus inside the nav drawer, or release it only on Escape?", when: "3m", options: ["Trap in drawer", "Release on Esc"] },
    ],
  },
  {
    id: "i9", projectId: "p_infra", tool: "gemini", model: "gemini-2.5-pro",
    name: "tf-modules",
    task: "Split monolithic main.tf into reusable network/db/cache modules",
    status: "done", branch: "agent/tf-modules", worktree: "agent-i9",
    base: "f02ce91", commits: 5, elapsed: 904, progress: 1,
    files: [
      { path: "main.tf", add: 18, del: 240, state: "M" },
      { path: "modules/network/main.tf", add: 96, del: 0, state: "A" },
      { path: "modules/db/main.tf", add: 88, del: 0, state: "A" },
      { path: "modules/cache/main.tf", add: 44, del: 0, state: "A" },
    ],
    pending: [
      { id: "pm6", kind: "review", title: "Ready to merge", cmd: "agent/tf-modules → main", when: "12m" },
    ],
  },
];

// terminal log lines per agent  {t: 'cmd'|'out'|'ok'|'warn'|'err'|'sys', s}
const LOGS = {
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

const STREAM = {
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

const DIFFS = {
  a1: { file: "src/webhooks/retry.ts", lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,21 @@ new file", lines: [
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
    ]},
  ]},
  b2: { file: "src/auth/refresh.ts", lang: "typescript", hunks: [
    { meta: "@@ -12,7 +12,14 @@ export class RefreshStore", lines: [
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
    ]},
  ]},
  c3: { file: "src/jobs/reconcile.ts", lang: "typescript", hunks: [
    { meta: "@@ -14,80 +14,9 @@ export async function reconcile()", lines: [
      { k: " ", n: 14, s: "export async function reconcile(date: Date) {" },
      { k: "-", n: 15, s: "  const ledger = await db.ledger.forDate(date);" },
      { k: "-", n: 16, s: "  // ...238 lines of nested logic..." },
      { k: "+", n: 15, s: "  const ctx = createContext(date);" },
      { k: "+", n: 16, s: "  for (const step of STEPS) {" },
      { k: "+", n: 17, s: "    await step.run(ctx);   // idempotent" },
      { k: "+", n: 18, s: "  }" },
      { k: "+", n: 19, s: "  return ctx.report;" },
      { k: " ", n: 20, s: "}" },
    ]},
  ]},
  d4: { file: "test/integration/checkout.test.ts", lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,8 @@ new file", lines: [
      { k: "+", n: 1, s: "describe('checkout → capture', () => {" },
      { k: "+", n: 2, s: "  it('captures an authorized payment', async () => {" },
      { k: "+", n: 3, s: "    const intent = await api.createIntent({ amount: 4200 });" },
      { k: "+", n: 4, s: "    await api.confirm(intent.id, cards.visa);" },
      { k: "+", n: 5, s: "    const res = await api.capture(intent.id);" },
      { k: "+", n: 6, s: "    expect(res.status).toBe('succeeded');" },
      { k: "+", n: 7, s: "  });" },
      { k: "+", n: 8, s: "});" },
    ]},
  ]},
  e5: { file: "package.json", lang: "json", hunks: [
    { meta: "@@ -2,7 +2,7 @@", lines: [
      { k: " ", n: 2, s: '  "name": "payments-service",' },
      { k: "-", n: 3, s: '  "engines": { "node": ">=20" },' },
      { k: "+", n: 3, s: '  "engines": { "node": ">=22" },' },
      { k: " ", n: 4, s: '  "dependencies": {' },
      { k: "-", n: 5, s: '    "fastify": "4.26.0",' },
      { k: "+", n: 5, s: '    "fastify": "5.1.0",' },
    ]},
  ]},
  g7: { file: "src/pages/Settings.tsx", lang: "tsx", hunks: [
    { meta: "@@ -8,10 +8,14 @@ export function Settings()", lines: [
      { k: " ", n: 8, s: "export function Settings() {" },
      { k: "-", n: 9, s: "  return <div className=\"settings legacy\">" },
      { k: "+", n: 9, s: "  return <div className=\"settings\" data-theme={theme}>" },
      { k: "+", n: 10, s: "    <Toggle label=\"Dark mode\" bind={prefs.dark} />" },
      { k: "+", n: 11, s: "    <Toggle label=\"Compact rows\" bind={prefs.compact} />" },
      { k: " ", n: 12, s: "  </div>;" },
    ]},
  ]},
  h8: { file: "src/components/Nav.tsx", lang: "tsx", hunks: [
    { meta: "@@ -22,6 +22,9 @@", lines: [
      { k: " ", n: 22, s: "  <a href={item.href}" },
      { k: "-", n: 23, s: "     className=\"nav-link\">" },
      { k: "+", n: 23, s: "     className=\"nav-link\"" },
      { k: "+", n: 24, s: "     aria-current={active ? 'page' : undefined}" },
      { k: "+", n: 25, s: "     tabIndex={0}>" },
    ]},
  ]},
  i9: { file: "modules/network/main.tf", lang: "hcl", hunks: [
    { meta: "@@ -0,0 +1,9 @@ new file", lines: [
      { k: "+", n: 1, s: "resource \"aws_vpc\" \"main\" {" },
      { k: "+", n: 2, s: "  cidr_block = var.vpc_cidr" },
      { k: "+", n: 3, s: "  tags = { Name = \"${var.env}-vpc\" }" },
      { k: "+", n: 4, s: "}" },
      { k: "+", n: 5, s: "" },
      { k: "+", n: 6, s: "resource \"aws_subnet\" \"public\" {" },
      { k: "+", n: 7, s: "  count      = length(var.azs)" },
      { k: "+", n: 8, s: "  vpc_id     = aws_vpc.main.id" },
      { k: "+", n: 9, s: "}" },
    ]},
  ]},
};

const CHATS = {
  a1: [
    { role: "user", s: "Add retry with exponential backoff to the Stripe webhook handler. Cap at 5 attempts.", time: "−6m" },
    { role: "agent", s: "On it. I'll add a `withRetry` helper with a 1→16s schedule, mark only 429/5xx as retryable, and honor `Retry-After`.", time: "−6m" },
    { role: "agent", s: "Helper + tests in place, 6/6 passing. Type-check is running — I'll commit once it's green.", time: "now" },
  ],
  b2: [
    { role: "user", s: "Move auth to short-lived JWTs with rotating refresh tokens.", time: "−21m" },
    { role: "agent", s: "JWT issuing + rotation logic is done. One open decision blocks the store:", time: "−4m" },
    { role: "agent", s: "Where should refresh tokens live — Redis (fast, volatile) or Postgres (durable, auditable)? This affects the migration I scaffold next.", time: "−4m", decision: true },
  ],
  c3: [
    { role: "user", s: "Break the 240-line reconcile() into idempotent steps.", time: "−3m" },
    { role: "agent", s: "Extracting into 5 step modules under jobs/steps/. Each takes a shared context and is safe to re-run. 3 of 5 done.", time: "now" },
  ],
  d4: [
    { role: "user", s: "Integration tests for checkout → capture.", time: "−11m" },
    { role: "agent", s: "14 tests covering auth, confirm, capture, partial capture, and decline paths. All green, committed. Ready to merge.", time: "−2m" },
  ],
  e5: [
    { role: "user", s: "Upgrade to Node 22, bump deps.", time: "−1m" },
    { role: "agent", s: "Bumped engines + Fastify 5 + 11 other deps. Triggered CI on the branch — waiting on lint, typecheck, and the test matrix.", time: "now" },
  ],
  f6: [{ role: "sys", s: "Agent queued. Will start when a worktree frees up (max 5 concurrent).", time: "queued" }],
  g7: [
    { role: "user", s: "Rebuild the account settings page with the new design tokens.", time: "−5m" },
    { role: "agent", s: "Swapping legacy color vars for the token set and adding a reusable <Toggle/>. 9 component tests green so far.", time: "now" },
  ],
  h8: [
    { role: "user", s: "Fix WCAG AA contrast + keyboard nav across the dashboard.", time: "−2m" },
    { role: "agent", s: "Found 14 contrast + 6 focus issues. Before I wire the focus-trap in the nav drawer — keep it within the drawer or release focus on Escape only?", time: "−2m", decision: true },
  ],
  i9: [
    { role: "user", s: "Split the monolithic main.tf into modules.", time: "−16m" },
    { role: "agent", s: "Extracted network/db/cache modules, terraform validate passes. Ready to merge.", time: "−12m" },
  ],
};

const COMMITS = [
  { agent: "i9", projectId: "p_infra", sha: "4cf0a2b", msg: "refactor: split into network/db/cache modules", when: "12m", files: 5 },
  { agent: "d4", projectId: "p_pay", sha: "9b21e07", msg: "test: checkout capture integration suite", when: "2m", files: 3 },
  { agent: "b2", projectId: "p_pay", sha: "7c4d11a", msg: "feat(auth): rotating refresh token store", when: "5m", files: 2 },
  { agent: "a1", projectId: "p_pay", sha: "1f8c2a9", msg: "feat: retry stripe webhooks w/ backoff", when: "6m", files: 4 },
  { agent: "g7", projectId: "p_web", sha: "e22a7f1", msg: "feat(settings): adopt new design tokens", when: "8m", files: 3 },
  { agent: "c3", projectId: "p_pay", sha: "b2a7c80", msg: "refactor: extract reconcile context", when: "18m", files: 1 },
];

// full-file contents for the viewer (path → source). Files not listed fall
// back to diff-reconstructed or stubbed content.
const FILE_CONTENTS = {
  "package.json": `{
  "name": "payments-service",
  "version": "2.4.0",
  "private": true,
  "engines": { "node": ">=20" },
  "scripts": {
    "dev": "tsx watch src/index.ts",
    "build": "tsc -p tsconfig.json",
    "test": "vitest run",
    "lint": "eslint src --max-warnings 0"
  },
  "dependencies": {
    "fastify": "4.26.0",
    "stripe": "14.21.0",
    "zod": "3.23.8",
    "pg": "8.11.5"
  },
  "devDependencies": {
    "typescript": "5.5.4",
    "vitest": "1.6.0",
    "tsx": "4.16.2"
  }
}`,
  "tsconfig.json": `{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "outDir": "dist",
    "rootDir": "src",
    "skipLibCheck": true
  },
  "include": ["src"],
  "exclude": ["node_modules", "dist"]
}`,
  "src/index.ts": `import { buildServer } from './server';
import { loadConfig } from './config/env';

const config = loadConfig();
const app = buildServer(config);

app.listen({ port: config.port, host: '0.0.0.0' })
  .then((addr) => app.log.info(\`payments-service up on \${addr}\`))
  .catch((err) => {
    app.log.error(err);
    process.exit(1);
  });`,
  "src/server.ts": `import Fastify, { FastifyInstance } from 'fastify';
import { registerWebhooks } from './webhooks/stripe';
import { authPlugin } from './auth/jwt';
import type { Config } from './config/env';

export function buildServer(config: Config): FastifyInstance {
  const app = Fastify({ logger: true, trustProxy: true });

  app.register(authPlugin, { secret: config.jwtSecret });
  app.register(registerWebhooks, { prefix: '/webhooks' });

  app.get('/healthz', async () => ({ ok: true }));

  return app;
}`,
  "src/webhooks/stripe.ts": `import type { FastifyPluginAsync } from 'fastify';
import Stripe from 'stripe';
import { withRetry } from './retry';

export const registerWebhooks: FastifyPluginAsync = async (app) => {
  const stripe = new Stripe(process.env.STRIPE_KEY!);

  app.post('/stripe', async (req, reply) => {
    const event = stripe.webhooks.constructEvent(
      req.rawBody, req.headers['stripe-signature']!, process.env.WHSEC!,
    );

    // retry transient failures with exponential backoff
    await withRetry(() => handleEvent(event));
    return reply.code(200).send({ received: true });
  });
};`,
  "src/config/queue.ts": `import { Queue } from 'bullmq';
import { redis } from './redis';

export const webhookQueue = new Queue('webhooks', {
  connection: redis,
  defaultJobOptions: {
    attempts: 5,
    backoff: { type: 'exponential', delay: 1000 },
    removeOnComplete: 1000,
  },
});`,
  "README.md": `# payments-service

Money-movement API for northwind. Handles checkout, capture,
refunds, reconciliation, and Stripe webhooks.

## Develop
    pnpm install
    pnpm dev

## Test
    pnpm test

## Architecture
- \`src/webhooks\`  — inbound Stripe events (idempotent + retried)
- \`src/auth\`      — JWT issuing + rotating refresh tokens
- \`src/jobs\`      — nightly reconciliation
- \`src/config\`    — env, redis, queues`,
  "src/main.tsx": `import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './styles/theme.css';

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);`,
  "src/App.tsx": `import { Routes, Route } from 'react-router-dom';
import { Nav } from './components/Nav';
import { Dashboard } from './pages/Dashboard';
import { Settings } from './pages/Settings';

export function App() {
  return (
    <div className="shell">
      <Nav />
      <main>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </main>
    </div>
  );
}`,
  "vite.config.ts": `import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: { port: 5173 },
  build: { sourcemap: true, target: 'es2022' },
});`,
  "main.tf": `terraform {
  required_version = ">= 1.7"
  required_providers {
    aws = { source = "hashicorp/aws", version = "~> 5.0" }
  }
}

module "network" { source = "./modules/network" env = var.env }
module "db"      { source = "./modules/db"      env = var.env  vpc_id = module.network.vpc_id }
module "cache"   { source = "./modules/cache"   env = var.env  vpc_id = module.network.vpc_id }`,
  "variables.tf": `variable "env" {
  type        = string
  description = "Deployment environment (staging | prod)"
}

variable "vpc_cidr" {
  type    = string
  default = "10.0.0.0/16"
}

variable "azs" {
  type    = list(string)
  default = ["us-east-1a", "us-east-1b"]
}`,
};

// ---------- backlog tickets (link to AGENTS by agentId) ----------
// notes + comment bodies are rich HTML (rendered by the RichEditor / RichView)
const TICKETS = [
  // ── To do ──
  { id: "t1", status: "todo", projectId: "p_pay", created: "2h ago",
    title: "Refund rounding on partial captures",
    notes: "<p>JPY partial captures round a cent high. Reconcile against Stripe balance transactions and fix the rounding rule.</p><h3>Acceptance</h3><ul><li>Use banker's rounding (half-even) on the minor-unit conversion</li><li>Reconcile the nightly job against <code>balance_transactions</code></li><li>Add a regression test for the \u00a5 partial-capture case</li></ul>",
    comments: [] },
  { id: "t2", status: "todo", projectId: "p_pay", created: "yesterday",
    title: "Rate-limit the public payments API",
    notes: "<p>Add a <strong>token-bucket</strong> limiter keyed by API key. Return <code>429</code> + <code>Retry-After</code> on burst.</p><blockquote>Targets: 100 req/s sustained, burst 200. Config surface in <code>config/limits.ts</code>.</blockquote>",
    comments: [
      { id: "c1", author: "Mara Kjell", role: "human", when: "yesterday", body: "<p>Let's key the bucket on API key, not IP \u2014 most traffic is server-to-server behind a couple of NATs.</p>" },
      { id: "c2", author: "Devin Osei", role: "human", when: "3h ago", body: "<p>Agreed. Make sure <code>Retry-After</code> is in seconds, Stripe-style.</p>" },
    ] },
  { id: "t3", status: "todo", projectId: "p_web", created: "3d ago",
    title: "Dark-mode polish on Settings",
    notes: "<p>A few legacy color vars still leak through the redesigned Settings page. Audit and swap to design tokens.</p>",
    comments: [] },
  { id: "t4", status: "todo", projectId: null, created: "1w ago",
    title: "Document the worktree layout",
    notes: "<p>Short README on how agent worktrees map to branches, plus the cleanup command.</p>",
    comments: [] },

  // ── In progress (linked to live agents) ──
  { id: "t5", status: "inprogress", projectId: "p_pay", created: "4h ago", agentId: "a1",
    title: "Exponential-backoff for Stripe webhooks",
    notes: "<p>Retry transient webhook failures on a <code>1\u219216s</code> schedule, cap at 5 attempts, honor <code>Retry-After</code>.</p>",
    comments: [
      { id: "c3", author: "stripe-retry", role: "agent", tool: "claude", when: "6m ago", body: "<p>Added a <code>withRetry</code> helper with a 1\u219216s schedule and marked only 429/5xx as retryable. 6/6 tests passing \u2014 committing once typecheck is green.</p>" },
    ] },
  { id: "t6", status: "inprogress", projectId: "p_pay", created: "yesterday", agentId: "b2",
    title: "Migrate auth to rotating refresh tokens",
    notes: "<p>Short-lived JWTs with a rotating refresh-token store. Issuing + rotation are done; the storage backend is the open question.</p>",
    comments: [
      { id: "c4", author: "jwt-refresh", role: "agent", tool: "claude", when: "4m ago", body: "<p>Blocked on a decision: should refresh tokens live in <strong>Redis</strong> (fast, volatile) or <strong>Postgres</strong> (durable, auditable)? It changes the migration I scaffold next.</p>" },
      { id: "c5", author: "Mara Kjell", role: "human", when: "2m ago", body: "<p>Go <strong>Postgres</strong> \u2014 we need the audit trail for compliance.</p>" },
    ] },
  { id: "t7", status: "inprogress", projectId: "p_web", created: "5h ago", agentId: "g7",
    title: "Rebuild account settings with tokens",
    notes: "<p>Swap legacy color vars for the token set and add a reusable <code>Toggle</code> component.</p>",
    comments: [] },
  { id: "t8", status: "inprogress", projectId: "p_pay", created: "1h ago", agentId: "e5",
    title: "Upgrade runtime to Node 22",
    notes: "<p>Bump engines + Fastify 5 + 11 deps, then green the CI matrix before merge.</p>",
    comments: [] },

  // ── Done ──
  { id: "t9", status: "done", projectId: "p_pay", created: "2d ago", agentId: "d4",
    title: "Integration tests for checkout \u2192 capture",
    notes: "<p>14 tests across auth, confirm, capture, partial-capture and decline paths.</p>",
    comments: [
      { id: "c6", author: "checkout-tests", role: "agent", tool: "claude", when: "2m ago", body: "<p>All 14 green and committed. Ready to merge.</p>" },
    ] },
  { id: "t10", status: "done", projectId: "p_infra", created: "3d ago", agentId: "i9",
    title: "Split main.tf into reusable modules",
    notes: "<p>Extracted network / db / cache modules; <code>terraform validate</code> passes.</p>",
    comments: [] },
  { id: "t11", status: "done", projectId: "p_pay", created: "4d ago",
    title: "Idempotency keys on the payments API",
    notes: "<p>Idempotency-Key header on all mutating endpoints, de-duped for 24h.</p>",
    comments: [] },
];

Object.assign(window, { PROJECTS, AGENT_TOOLS, AGENTS, LOGS, STREAM, DIFFS, CHATS, COMMITS, FILE_CONTENTS, TICKETS, ORG: "northwind", WORKTREE_ROOT: "~/.orchestra/worktrees",
  BUILD: { version: "0.9.2", channel: "dev", commit: "a3f91c2", built: "2026-06-09" } });
