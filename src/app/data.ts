// Orrery mock data — multi-project, multi-agent git orchestration
import { AgentTool, LogLine, ModelOption, Project } from "./models";

export const ORG = "northwind";

// ---------- agent tools (CLI coding agents) ----------
// Each tool's `--model` vocabulary, curated: ALIASES first where the CLI has
// them (they resolve to the tool's latest and never go stale — the first entry
// is the spawn default), then PINNED versions for workflows that must not
// drift under them. Effort: the tool-level list is what the CLI flag accepts
// (and the fallback for a custom id typed in Settings); a model narrows it
// when it takes fewer levels (Opus 4.6 has no `xhigh`) or none (Haiku).
const m = (id: string, label: string, group: string, extra: Partial<ModelOption> = {}): ModelOption => ({
  id,
  label,
  group,
  ...extra,
});

/** `claude --effort` levels (2.1.x); `xhigh` is Claude Code's own default. */
const CLAUDE_EFFORT = ["low", "medium", "high", "xhigh", "max"];
/** Opus 4.6 / Sonnet 4.6 predate `xhigh` (it arrived with Opus 4.7). */
const CLAUDE_EFFORT_46 = ["low", "medium", "high", "max"];
const CLAUDE_XHIGH = { defaultEffort: "xhigh" };
/** codex `--config model_reasoning_effort=…`; `max` is Sol-only. */
const CODEX_EFFORT = ["low", "medium", "high", "xhigh"];

export const AGENT_TOOLS: AgentTool[] = [
  {
    id: "claude",
    name: "Claude Code",
    short: "claude",
    accent: "var(--tool-claude)",
    models: [
      // `claude --model` aliases → the latest of each family
      m("fable", "Fable (latest)", "Latest", CLAUDE_XHIGH),
      m("opus", "Opus (latest)", "Latest", CLAUDE_XHIGH),
      m("sonnet", "Sonnet (latest)", "Latest", CLAUDE_XHIGH),
      m("haiku", "Haiku (latest)", "Latest", { effort: false }),
      // full names pin a version
      m("claude-fable-5-1", "Fable 5.1", "Pinned versions", CLAUDE_XHIGH),
      m("claude-fable-5", "Fable 5", "Pinned versions", CLAUDE_XHIGH),
      m("claude-opus-5", "Opus 5", "Pinned versions", CLAUDE_XHIGH),
      m("claude-opus-4-8", "Opus 4.8", "Pinned versions", CLAUDE_XHIGH),
      m("claude-opus-4-7", "Opus 4.7", "Pinned versions", CLAUDE_XHIGH),
      m("claude-opus-4-6", "Opus 4.6", "Pinned versions", { effort: CLAUDE_EFFORT_46 }),
      m("claude-sonnet-5", "Sonnet 5", "Pinned versions", CLAUDE_XHIGH),
      m("claude-sonnet-4-6", "Sonnet 4.6", "Pinned versions", { effort: CLAUDE_EFFORT_46 }),
      m("claude-haiku-4-5", "Haiku 4.5", "Pinned versions", { effort: false }),
    ],
    effort: CLAUDE_EFFORT,
  },
  {
    id: "codex",
    name: "Codex",
    short: "codex",
    accent: "var(--tool-codex)",
    models: [
      // gpt-5.6 tiers (GA July 2026): Sol = flagship / CLI default, Terra =
      // balanced, Luna = volume — each ships tuned to its own effort
      m("gpt-5.6-sol", "GPT-5.6 Sol", "GPT-5.6", { effort: [...CODEX_EFFORT, "max"], defaultEffort: "xhigh" }),
      m("gpt-5.6-terra", "GPT-5.6 Terra", "GPT-5.6", { defaultEffort: "high" }),
      m("gpt-5.6-luna", "GPT-5.6 Luna", "GPT-5.6", { defaultEffort: "medium" }),
      m("gpt-5.5", "GPT-5.5", "Pinned versions"),
      m("gpt-5.4", "GPT-5.4", "Pinned versions"),
      m("gpt-5.3-codex", "GPT-5.3 Codex", "Pinned versions"),
    ],
    effort: CODEX_EFFORT,
  },
  {
    id: "cursor",
    name: "Cursor",
    short: "cursor",
    accent: "var(--tool-cursor)",
    // cursor-agent takes any slug of the account's model pool; no effort flag
    models: [
      m("composer-2.5", "Composer 2.5", "Cursor"),
      m("composer-2.5-fast", "Composer 2.5 Fast", "Cursor"),
      m("composer-2", "Composer 2", "Cursor"),
      m("auto", "Auto", "Cursor"),
      m("claude-fable-5.1", "Claude Fable 5.1", "Frontier"),
      m("claude-opus-5", "Claude Opus 5", "Frontier"),
      m("claude-sonnet-5", "Claude Sonnet 5", "Frontier"),
      m("gpt-5.6-sol", "GPT-5.6 Sol", "Frontier"),
      m("gpt-5.5", "GPT-5.5", "Frontier"),
      m("gemini-3.1-pro", "Gemini 3.1 Pro", "Frontier"),
      m("grok-4.6", "Grok 4.6", "Frontier"),
    ],
    effort: false,
  },
  {
    id: "gemini",
    name: "Gemini",
    short: "gemini",
    accent: "var(--tool-gemini)",
    // gemini-cli `--model`; thinking level is not a CLI flag, so no effort
    models: [
      m("auto", "Auto (Gemini 3)", "Latest"),
      m("gemini-3.1-pro-preview", "Gemini 3.1 Pro (preview)", "Gemini 3"),
      m("gemini-3-pro-preview", "Gemini 3 Pro (preview)", "Gemini 3"),
      m("gemini-3-flash-preview", "Gemini 3 Flash (preview)", "Gemini 3"),
      m("gemini-2.5-pro", "Gemini 2.5 Pro", "Gemini 2.5"),
      m("gemini-2.5-flash", "Gemini 2.5 Flash", "Gemini 2.5"),
    ],
    effort: false,
  },
];

/** The curated entry for `id` on `tool`, if any (a custom id has none). */
export function modelOption(tool: AgentTool, id: string): ModelOption | undefined {
  return tool.models.find((x) => x.id === id);
}

/** Effort levels `model` accepts on `tool` — the model's own list when it
 *  narrows the tool's, else the tool's; `false` when there is no knob. A
 *  custom (non-curated) id gets the tool's list: the CLI still accepts it. */
export function effortLevelsFor(tool: AgentTool, model: string): string[] | false {
  return modelOption(tool, model)?.effort ?? tool.effort;
}

/** The level to pre-select for `model`: its declared default, else `high`
 *  (the historical spawn default) when offered, else the first level. `null`
 *  when the model takes no effort. */
export function defaultEffortFor(tool: AgentTool, model: string): string | null {
  const levels = effortLevelsFor(tool, model);
  if (!levels) return null;
  const preferred = modelOption(tool, model)?.defaultEffort;
  if (preferred && levels.includes(preferred)) return preferred;
  return levels.includes("high") ? "high" : levels[0];
}

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
