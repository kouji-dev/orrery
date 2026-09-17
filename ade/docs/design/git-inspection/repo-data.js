/* global window */
// Orrery — human-driven git inspection mock data, scoped to the project repo
// (independent of agents). Covers: commit history + per-commit diffs, range
// diff, blame, 3-way conflict session, file history, and working-tree hunks.

// ---------- commit authors (humans + merged agent branches) ----------
const GIT_AUTHORS = {
  mara:  { name: "Mara Kjell",    handle: "mara",    short: "MK", color: "#a855f7", kind: "human" },
  devin: { name: "Devin Osei",    handle: "devin",   short: "DO", color: "#22d3ee", kind: "human" },
  priya: { name: "Priya Nair",    handle: "priya",   short: "PN", color: "#34e0a1", kind: "human" },
  tom:   { name: "Tom Becker",    handle: "tom",     short: "TB", color: "#ff6b35", kind: "human" },
  ci:    { name: "northwind-ci",  handle: "ci-bot",  short: "CI", color: "#6b7488", kind: "bot" },
  // agent-authored branches that were merged back into main
  ag_retry:    { name: "stripe-retry",    handle: "agent", short: "✳", color: "#d98a5b", kind: "agent", tool: "claude" },
  ag_jwt:      { name: "jwt-refresh",     handle: "agent", short: "✳", color: "#d98a5b", kind: "agent", tool: "claude" },
  ag_checkout: { name: "checkout-tests",  handle: "agent", short: "✳", color: "#d98a5b", kind: "agent", tool: "claude" },
  ag_recon:    { name: "reconcile-refactor", handle: "agent", short: "◆", color: "#10a37f", kind: "agent", tool: "codex" },
};

// ---------- commit history on payments-service · main (newest → oldest) ----
// state: A added · M modified · D deleted · R renamed
const GIT_HISTORY = [
  {
    sha: "a3f91c2", author: "mara", rel: "2h", date: "Jun 16 · 16:04", merge: true,
    parents: ["9b21e07", "d4e1ab0"],
    msg: "merge: agent/checkout-tests → main",
    body: "Integration suite for the checkout → capture flow. 14 tests, all green.",
    files: [
      { path: "test/integration/checkout.test.ts", state: "A", add: 184, del: 0 },
      { path: "test/fixtures/cards.ts", state: "A", add: 40, del: 0 },
      { path: "test/helpers/server.ts", state: "M", add: 12, del: 4 },
    ],
  },
  {
    sha: "9b21e07", author: "ag_checkout", rel: "2h", date: "Jun 16 · 15:58",
    parents: ["7c4d11a"],
    msg: "test: checkout capture integration suite",
    body: "Covers auth, confirm, capture, partial capture and decline paths.",
    files: [
      { path: "test/integration/checkout.test.ts", state: "A", add: 184, del: 0 },
      { path: "test/fixtures/cards.ts", state: "A", add: 40, del: 0 },
      { path: "test/helpers/server.ts", state: "M", add: 12, del: 4 },
    ],
  },
  {
    sha: "7c4d11a", author: "ag_jwt", rel: "5h", date: "Jun 16 · 12:30",
    parents: ["1f8c2a9"],
    msg: "feat(auth): rotating refresh token store",
    body: "Short-lived access JWTs backed by a Postgres-persisted rotating refresh store.",
    files: [
      { path: "src/auth/refresh.ts", state: "A", add: 96, del: 0 },
      { path: "src/auth/jwt.ts", state: "M", add: 41, del: 18 },
      { path: "migrations/0042_refresh_tokens.sql", state: "A", add: 22, del: 0 },
    ],
  },
  {
    sha: "1f8c2a9", author: "ag_retry", rel: "6h", date: "Jun 16 · 11:12",
    parents: ["e3b7d20"],
    msg: "feat: retry stripe webhooks w/ backoff",
    body: "withRetry helper on a 1→16s schedule; only 429/5xx are retryable; honors Retry-After.",
    files: [
      { path: "src/webhooks/retry.ts", state: "A", add: 88, del: 0 },
      { path: "src/webhooks/stripe.ts", state: "M", add: 34, del: 6 },
      { path: "src/config/queue.ts", state: "M", add: 5, del: 2 },
      { path: "test/webhooks/retry.test.ts", state: "A", add: 61, del: 0 },
    ],
  },
  {
    sha: "e3b7d20", author: "mara", rel: "yesterday", date: "Jun 15 · 18:41",
    parents: ["c80f5a1"],
    msg: "fix(refund): banker's rounding on minor units",
    body: "JPY partial captures rounded a cent high — switch to half-even rounding.",
    files: [
      { path: "src/refunds/round.ts", state: "M", add: 14, del: 9 },
      { path: "test/refunds/round.test.ts", state: "A", add: 33, del: 0 },
    ],
  },
  {
    sha: "c80f5a1", author: "devin", rel: "yesterday", date: "Jun 15 · 14:07",
    parents: ["b2a7c80"],
    msg: "chore(deps): bump stripe 14.21, zod 3.23",
    files: [
      { path: "package.json", state: "M", add: 6, del: 6 },
      { path: "pnpm-lock.yaml", state: "M", add: 188, del: 142 },
    ],
  },
  {
    sha: "b2a7c80", author: "ag_recon", rel: "2d", date: "Jun 14 · 09:22",
    parents: ["5d9e413"],
    msg: "refactor: extract reconcile context",
    body: "Reduce the 240-line reconcile() to a context-threaded step pipeline.",
    files: [
      { path: "src/jobs/reconcile.ts", state: "M", add: 9, del: 80 },
      { path: "src/jobs/context.ts", state: "A", add: 31, del: 0 },
    ],
  },
  {
    sha: "5d9e413", author: "priya", rel: "3d", date: "Jun 13 · 16:50",
    parents: ["6a1c8f4"],
    msg: "feat(webhooks): verify Stripe signature + raw body",
    files: [
      { path: "src/webhooks/stripe.ts", state: "M", add: 19, del: 5 },
      { path: "src/server.ts", state: "M", add: 8, del: 1 },
    ],
  },
  {
    sha: "6a1c8f4", author: "tom", rel: "4d", date: "Jun 12 · 11:03",
    parents: ["4e07b9d"],
    msg: "perf(db): index balance_transactions(created_at)",
    files: [
      { path: "migrations/0041_bt_index.sql", state: "A", add: 7, del: 0 },
    ],
  },
  {
    sha: "4e07b9d", author: "mara", rel: "5d", date: "Jun 11 · 17:36",
    parents: ["0c5a772"],
    msg: "feat(auth): issue short-lived access JWTs",
    files: [
      { path: "src/auth/jwt.ts", state: "M", add: 58, del: 12 },
      { path: "src/middleware/session.ts", state: "M", add: 18, del: 31 },
    ],
  },
  {
    sha: "0c5a772", author: "devin", rel: "1w", date: "Jun 09 · 10:00",
    parents: [],
    msg: "chore: scaffold payments-service",
    body: "Initial Fastify app, config, and CI workflow.",
    files: [
      { path: "src/index.ts", state: "A", add: 24, del: 0 },
      { path: "src/server.ts", state: "A", add: 31, del: 0 },
      { path: "package.json", state: "A", add: 38, del: 0 },
      { path: "tsconfig.json", state: "A", add: 18, del: 0 },
    ],
  },
];

// ---------- explicit per-commit diffs (key clickable files) -----------------
// keyed "sha/path" → { lang, hunks:[{ meta, lines:[{k,n,s}] }] }
// k: " " context · "+" add · "-" del   n: new-side line number
const COMMIT_DIFFS = {
  "a3f91c2/test/integration/checkout.test.ts": { lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,9 @@ new file", lines: [
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
  "a3f91c2/test/fixtures/cards.ts": { lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,6 @@ new file", lines: [
      { k: "+", n: 1, s: "export const cards = {" },
      { k: "+", n: 2, s: "  visa:    { number: '4242424242424242', cvc: '123' }," },
      { k: "+", n: 3, s: "  declined:{ number: '4000000000000002', cvc: '123' }," },
      { k: "+", n: 4, s: "  insufficient: { number: '4000000000009995', cvc: '123' }," },
      { k: "+", n: 5, s: "} as const;" },
    ]},
  ]},
  "a3f91c2/test/helpers/server.ts": { lang: "typescript", hunks: [
    { meta: "@@ -8,6 +8,9 @@ export async function testServer()", lines: [
      { k: " ", n: 8, s: "export async function testServer() {" },
      { k: "-", n: 9, s: "  const app = buildServer(testConfig);" },
      { k: "+", n: 9, s: "  const app = buildServer({ ...testConfig, logger: false });" },
      { k: "+", n: 10, s: "  await app.ready();" },
      { k: " ", n: 11, s: "  return app;" },
      { k: " ", n: 12, s: "}" },
    ]},
  ]},
  "1f8c2a9/src/webhooks/retry.ts": { lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,19 @@ new file", lines: [
      { k: "+", n: 1, s: "import { setTimeout as sleep } from 'node:timers/promises';" },
      { k: "+", n: 2, s: "" },
      { k: "+", n: 3, s: "const SCHEDULE = [1_000, 2_000, 4_000, 8_000, 16_000] as const;" },
      { k: "+", n: 4, s: "" },
      { k: "+", n: 5, s: "export async function withRetry<T>(" },
      { k: "+", n: 6, s: "  fn: () => Promise<T>," },
      { k: "+", n: 7, s: "  opts: { maxAttempts?: number } = {}," },
      { k: "+", n: 8, s: "): Promise<T> {" },
      { k: "+", n: 9, s: "  const max = opts.maxAttempts ?? SCHEDULE.length;" },
      { k: "+", n: 10, s: "  for (let attempt = 0; attempt < max; attempt++) {" },
      { k: "+", n: 11, s: "    try { return await fn(); }" },
      { k: "+", n: 12, s: "    catch (err) {" },
      { k: "+", n: 13, s: "      if (!isRetryable(err)) throw err;" },
      { k: "+", n: 14, s: "      await sleep(SCHEDULE[attempt] ?? 16_000);" },
      { k: "+", n: 15, s: "    }" },
      { k: "+", n: 16, s: "  }" },
      { k: "+", n: 17, s: "  throw new Error('retry attempts exhausted');" },
      { k: "+", n: 18, s: "}" },
    ]},
  ]},
  "1f8c2a9/src/webhooks/stripe.ts": { lang: "typescript", hunks: [
    { meta: "@@ -1,4 +1,5 @@", lines: [
      { k: " ", n: 1, s: "import type { FastifyPluginAsync } from 'fastify';" },
      { k: " ", n: 2, s: "import Stripe from 'stripe';" },
      { k: "+", n: 3, s: "import { withRetry } from './retry';" },
    ]},
    { meta: "@@ -12,7 +13,9 @@ app.post('/stripe')", lines: [
      { k: " ", n: 13, s: "    );" },
      { k: " ", n: 14, s: "" },
      { k: "-", n: 15, s: "    await handleEvent(event);" },
      { k: "+", n: 15, s: "    // retry transient failures with exponential backoff" },
      { k: "+", n: 16, s: "    await withRetry(() => handleEvent(event));" },
      { k: " ", n: 17, s: "    return reply.code(200).send({ received: true });" },
    ]},
  ]},
  "7c4d11a/src/auth/refresh.ts": { lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,12 @@ new file", lines: [
      { k: "+", n: 1, s: "import type { Pool } from 'pg';" },
      { k: "+", n: 2, s: "" },
      { k: "+", n: 3, s: "export class RefreshStore {" },
      { k: "+", n: 4, s: "  constructor(private db: Pool) {}" },
      { k: "+", n: 5, s: "" },
      { k: "+", n: 6, s: "  async rotate(old: string): Promise<Token> {" },
      { k: "+", n: 7, s: "    await this.revoke(old);" },
      { k: "+", n: 8, s: "    return this.issue();" },
      { k: "+", n: 9, s: "  }" },
      { k: "+", n: 10, s: "}" },
    ]},
  ]},
  "7c4d11a/src/auth/jwt.ts": { lang: "typescript", hunks: [
    { meta: "@@ -18,9 +18,12 @@ export function signAccess()", lines: [
      { k: " ", n: 18, s: "export function signAccess(sub: string) {" },
      { k: "-", n: 19, s: "  return jwt.sign({ sub }, secret, { expiresIn: '7d' });" },
      { k: "+", n: 19, s: "  return jwt.sign({ sub }, secret, { expiresIn: '10m' });" },
      { k: " ", n: 20, s: "}" },
      { k: "+", n: 21, s: "" },
      { k: "+", n: 22, s: "export function signRefresh(sub: string) {" },
      { k: "+", n: 23, s: "  return jwt.sign({ sub, t: 'r' }, secret, { expiresIn: '30d' });" },
      { k: "+", n: 24, s: "}" },
    ]},
  ]},
  "9b21e07/test/integration/checkout.test.ts": { lang: "typescript", hunks: [
    { meta: "@@ -0,0 +1,9 @@ new file", lines: [
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
  "e3b7d20/src/refunds/round.ts": { lang: "typescript", hunks: [
    { meta: "@@ -4,9 +4,14 @@ export function toMinorUnits()", lines: [
      { k: " ", n: 4, s: "export function toMinorUnits(amount: number, currency: string) {" },
      { k: " ", n: 5, s: "  const factor = MINOR[currency] ?? 100;" },
      { k: "-", n: 6, s: "  // rounds half-up — overshoots ¥ partial captures" },
      { k: "-", n: 7, s: "  return Math.round(amount * factor);" },
      { k: "+", n: 6, s: "  // banker's rounding (half-even) keeps reconciliation exact" },
      { k: "+", n: 7, s: "  const scaled = amount * factor;" },
      { k: "+", n: 8, s: "  const floor = Math.floor(scaled);" },
      { k: "+", n: 9, s: "  const diff = scaled - floor;" },
      { k: "+", n: 10, s: "  if (diff > 0.5) return floor + 1;" },
      { k: "+", n: 11, s: "  if (diff < 0.5) return floor;" },
      { k: "+", n: 12, s: "  return floor % 2 === 0 ? floor : floor + 1;" },
      { k: " ", n: 13, s: "}" },
    ]},
  ]},
  "5d9e413/src/webhooks/stripe.ts": { lang: "typescript", hunks: [
    { meta: "@@ -6,5 +6,9 @@ app.post('/stripe')", lines: [
      { k: " ", n: 6, s: "  app.post('/stripe', async (req, reply) => {" },
      { k: "-", n: 7, s: "    const event = JSON.parse(req.body);" },
      { k: "+", n: 7, s: "    const event = stripe.webhooks.constructEvent(" },
      { k: "+", n: 8, s: "      req.rawBody," },
      { k: "+", n: 9, s: "      req.headers['stripe-signature']!," },
      { k: "+", n: 10, s: "      process.env.WHSEC!," },
      { k: "+", n: 11, s: "    );" },
      { k: " ", n: 12, s: "    await handleEvent(event);" },
    ]},
  ]},
  "4e07b9d/src/auth/jwt.ts": { lang: "typescript", hunks: [
    { meta: "@@ -1,6 +1,9 @@", lines: [
      { k: " ", n: 1, s: "import jwt from 'jsonwebtoken';" },
      { k: "+", n: 2, s: "" },
      { k: "+", n: 3, s: "const secret = process.env.JWT_SECRET!;" },
      { k: " ", n: 4, s: "" },
      { k: "-", n: 5, s: "export function sign(sub: string) {" },
      { k: "-", n: 6, s: "  return jwt.sign({ sub }, 'dev-secret');" },
      { k: "+", n: 5, s: "export function signAccess(sub: string) {" },
      { k: "+", n: 6, s: "  return jwt.sign({ sub }, secret, { expiresIn: '7d' });" },
      { k: " ", n: 7, s: "}" },
    ]},
  ]},
};

// ---------- range diff (multi-commit) — combined across a selection ---------
// produced for the demo selection [1f8c2a9 .. 9b21e07]; the UI builds the
// changed-file list by unioning the commits' files.
const RANGE_DIFFS = {
  "src/webhooks/stripe.ts": { lang: "typescript", hunks: [
    { meta: "@@ -1,4 +1,5 @@  3 commits", lines: [
      { k: " ", n: 1, s: "import type { FastifyPluginAsync } from 'fastify';" },
      { k: " ", n: 2, s: "import Stripe from 'stripe';" },
      { k: "+", n: 3, s: "import { withRetry } from './retry';" },
    ]},
    { meta: "@@ -13,6 +14,9 @@ app.post('/stripe')", lines: [
      { k: "-", n: 14, s: "    await handleEvent(event);" },
      { k: "+", n: 14, s: "    // retry transient failures with exponential backoff" },
      { k: "+", n: 15, s: "    await withRetry(() => handleEvent(event));" },
      { k: " ", n: 16, s: "    return reply.code(200).send({ received: true });" },
    ]},
  ]},
};

// ---------- blame / annotate ------------------------------------------------
// per-line blame for src/webhooks/stripe.ts. age 0 (newest) → 1 (oldest)
// drives the IntelliJ-style aged shading; `first` marks a new author group.
const BLAME_FILE = { path: "src/webhooks/stripe.ts", lang: "typescript" };
const BLAME = [
  { n: 1,  s: "import type { FastifyPluginAsync } from 'fastify';", sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 2,  s: "import Stripe from 'stripe';",                       sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 3,  s: "import { withRetry } from './retry';",               sha: "1f8c2a9", author: "ag_retry", rel: "6h",  age: 0.1 },
  { n: 4,  s: "",                                                   sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 5,  s: "export const registerWebhooks: FastifyPluginAsync = async (app) => {", sha: "0c5a772", author: "devin", rel: "1w", age: 1.0 },
  { n: 6,  s: "  const stripe = new Stripe(process.env.STRIPE_KEY!);", sha: "0c5a772", author: "devin",  rel: "1w",  age: 1.0 },
  { n: 7,  s: "",                                                   sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 8,  s: "  app.post('/stripe', async (req, reply) => {",      sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 9,  s: "    const event = stripe.webhooks.constructEvent(",  sha: "5d9e413", author: "priya",    rel: "3d",  age: 0.55 },
  { n: 10, s: "      req.rawBody,",                                 sha: "5d9e413", author: "priya",    rel: "3d",  age: 0.55 },
  { n: 11, s: "      req.headers['stripe-signature']!,",            sha: "5d9e413", author: "priya",    rel: "3d",  age: 0.55 },
  { n: 12, s: "      process.env.WHSEC!,",                          sha: "5d9e413", author: "priya",    rel: "3d",  age: 0.55 },
  { n: 13, s: "    );",                                             sha: "5d9e413", author: "priya",    rel: "3d",  age: 0.55 },
  { n: 14, s: "",                                                   sha: "5d9e413", author: "priya",    rel: "3d",  age: 0.55 },
  { n: 15, s: "    // retry transient failures with exponential backoff", sha: "1f8c2a9", author: "ag_retry", rel: "6h", age: 0.1 },
  { n: 16, s: "    await withRetry(() => handleEvent(event));",     sha: "1f8c2a9", author: "ag_retry", rel: "6h",  age: 0.1 },
  { n: 17, s: "    return reply.code(200).send({ received: true });", sha: "0c5a772", author: "devin",   rel: "1w",  age: 1.0 },
  { n: 18, s: "  });",                                              sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 19, s: "",                                                   sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
  { n: 20, s: "  app.get('/stripe/health', async () => {",         sha: "6a1c8f4", author: "tom",      rel: "4d",  age: 0.45 },
  { n: 21, s: "    const lag = await measureWebhookLag();",         sha: "6a1c8f4", author: "tom",      rel: "4d",  age: 0.45 },
  { n: 22, s: "    return { ok: lag < 5_000, lag };",               sha: "6a1c8f4", author: "tom",      rel: "4d",  age: 0.45 },
  { n: 23, s: "  });",                                              sha: "6a1c8f4", author: "tom",      rel: "4d",  age: 0.45 },
  { n: 24, s: "};",                                                 sha: "0c5a772", author: "devin",    rel: "1w",  age: 1.0 },
];

// ---------- 3-way conflict session (merge develop → main) -------------------
// per file: ordered segments. ctx = unchanged · conflict has base/ours/theirs.
const CONFLICT_SESSION = {
  op: "merge",
  title: "Merge develop → main",
  ours: "main",
  theirs: "develop",
  base: "5d9e413",
  files: [
    {
      path: "src/auth/jwt.ts", lang: "typescript", conflicts: 2,
      segments: [
        { type: "ctx", lines: ["import jwt from 'jsonwebtoken';", "", "const secret = process.env.JWT_SECRET!;", ""] },
        { type: "conflict", res: null,
          base:   ["  return jwt.sign({ sub }, secret, { expiresIn: '7d' });"],
          ours:   ["  return jwt.sign({ sub }, secret, { expiresIn: '10m' });"],
          theirs: ["  return jwt.sign({ sub }, secret, { expiresIn: '15m', algorithm: 'HS512' });"] },
        { type: "ctx", lines: ["}", ""] },
        { type: "conflict", res: null,
          base:   ["export function verify(token: string) {", "  return jwt.verify(token, secret);", "}"],
          ours:   ["export function verify(token: string) {", "  return jwt.verify(token, secret, { clockTolerance: 5 });", "}"],
          theirs: ["export function verify(token: string) {", "  return jwt.verify(token, secret, { algorithms: ['HS512'] });", "}"] },
        { type: "ctx", lines: [""] },
      ],
    },
    {
      path: "src/config/queue.ts", lang: "typescript", conflicts: 1,
      segments: [
        { type: "ctx", lines: ["export const webhookQueue = new Queue('webhooks', {", "  connection: redis,", "  defaultJobOptions: {"] },
        { type: "conflict", res: null,
          base:   ["    attempts: 3,"],
          ours:   ["    attempts: 5,", "    backoff: { type: 'exponential', delay: 1000 },"],
          theirs: ["    attempts: 8,"] },
        { type: "ctx", lines: ["    removeOnComplete: 1000,", "  },", "});"] },
      ],
    },
    {
      path: "package.json", lang: "json", conflicts: 1,
      segments: [
        { type: "ctx", lines: ['  "dependencies": {', '    "fastify": "5.1.0",'] },
        { type: "conflict", res: null,
          base:   ['    "stripe": "14.21.0",'],
          ours:   ['    "stripe": "14.21.0",', '    "p-retry": "5.1.2",'],
          theirs: ['    "stripe": "15.0.0",'] },
        { type: "ctx", lines: ['    "zod": "3.23.8"', "  }"] },
      ],
    },
  ],
};

// ---------- file history (commits that touched a file) ----------------------
// for src/auth/jwt.ts — pick any two revisions to diff across them.
const FILE_HISTORY = {
  path: "src/auth/jwt.ts", lang: "typescript",
  revisions: [
    { sha: "7c4d11a", author: "ag_jwt", rel: "5h",        date: "Jun 16 · 12:30", msg: "feat(auth): rotating refresh token store", add: 41, del: 18 },
    { sha: "4e07b9d", author: "mara",   rel: "5d",        date: "Jun 11 · 17:36", msg: "feat(auth): issue short-lived access JWTs", add: 58, del: 12 },
    { sha: "0c5a772", author: "devin",  rel: "1w",        date: "Jun 09 · 10:00", msg: "chore: scaffold payments-service", add: 9, del: 0 },
  ],
  // diff across the two default-selected revisions (4e07b9d → 7c4d11a)
  diff: { lang: "typescript", hunks: [
    { meta: "@@ -17,6 +17,12 @@ export function signAccess()", lines: [
      { k: " ", n: 17, s: "export function signAccess(sub: string) {" },
      { k: "-", n: 18, s: "  return jwt.sign({ sub }, secret, { expiresIn: '7d' });" },
      { k: "+", n: 18, s: "  return jwt.sign({ sub }, secret, { expiresIn: '10m' });" },
      { k: " ", n: 19, s: "}" },
      { k: "+", n: 20, s: "" },
      { k: "+", n: 21, s: "export function signRefresh(sub: string) {" },
      { k: "+", n: 22, s: "  return jwt.sign({ sub, t: 'r' }, secret, { expiresIn: '30d' });" },
      { k: "+", n: 23, s: "}" },
    ]},
  ]},
};

// ---------- working tree (uncommitted) — for hunk/line-level review ---------
// human's local edits over the project repo; partial commit / partial discard.
const WORKING_TREE = [
  {
    path: "src/refunds/round.ts", lang: "typescript", state: "M", add: 8, del: 3,
    hunks: [
      { id: "h1", meta: "@@ -1,3 +1,4 @@", lines: [
        { k: " ", n: 1, s: "const MINOR: Record<string, number> = {" },
        { k: "+", n: 2, s: "  JPY: 1, KWD: 1000," },
        { k: " ", n: 3, s: "  USD: 100, EUR: 100," },
        { k: " ", n: 4, s: "};" },
      ]},
      { id: "h2", meta: "@@ -14,5 +15,8 @@ export function reconcile()", lines: [
        { k: " ", n: 15, s: "export function reconcile(rows: Row[]) {" },
        { k: "-", n: 16, s: "  return rows.reduce((a, r) => a + r.amount, 0);" },
        { k: "+", n: 16, s: "  // exclude reversed transactions from the total" },
        { k: "+", n: 17, s: "  return rows" },
        { k: "+", n: 18, s: "    .filter((r) => !r.reversed)" },
        { k: "+", n: 19, s: "    .reduce((a, r) => a + r.amount, 0);" },
        { k: " ", n: 20, s: "}" },
      ]},
    ],
  },
  {
    path: "src/webhooks/stripe.ts", lang: "typescript", state: "M", add: 3, del: 0,
    hunks: [
      { id: "h3", meta: "@@ -16,3 +16,6 @@ app.post('/stripe')", lines: [
        { k: " ", n: 16, s: "    await withRetry(() => handleEvent(event));" },
        { k: "+", n: 17, s: "    metrics.increment('webhook.handled', {" },
        { k: "+", n: 18, s: "      type: event.type," },
        { k: "+", n: 19, s: "    });" },
        { k: " ", n: 20, s: "    return reply.code(200).send({ received: true });" },
      ]},
    ],
  },
  {
    path: "README.md", lang: "markdown", state: "M", add: 4, del: 1,
    hunks: [
      { id: "h4", meta: "@@ -18,4 +18,7 @@ ## Architecture", lines: [
        { k: " ", n: 18, s: "- `src/auth`      — JWT issuing + rotating refresh tokens" },
        { k: "-", n: 19, s: "- `src/jobs`      — nightly reconciliation" },
        { k: "+", n: 19, s: "- `src/jobs`      — nightly reconciliation (idempotent steps)" },
        { k: "+", n: 20, s: "- `src/refunds`   — minor-unit rounding + reconciliation" },
        { k: "+", n: 21, s: "- `src/config`    — env, redis, queues" },
      ]},
    ],
  },
];

window.GIT = {
  authors: GIT_AUTHORS,
  history: GIT_HISTORY,
  commitDiffs: COMMIT_DIFFS,
  rangeDiffs: RANGE_DIFFS,
  blameFile: BLAME_FILE,
  blame: BLAME,
  conflict: CONFLICT_SESSION,
  fileHistory: FILE_HISTORY,
  workingTree: WORKING_TREE,
  repo: { projectId: "p_pay", name: "payments-service", branch: "main", head: "a3f91c2", ahead: 0, behind: 2 },
};
