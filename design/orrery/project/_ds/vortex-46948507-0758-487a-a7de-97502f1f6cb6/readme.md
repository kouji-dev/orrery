# Vortex — Design System

**Vortex is the AI portal your team actually wants to use.** One chat for every model, grounded in your knowledge, aware of your memory, bounded by your guardrails — self-hostable end to end. This design system captures Vortex's brand and product surfaces so agents can generate on-brand interfaces, decks, and marketing without re-deriving the look each time.

> **Namespace:** components are exposed at `window.Vortex_469485`. In `@dsCard` HTML: `const { Button, PrismLogo } = window.Vortex_469485`.

---

## What Vortex is

An enterprise **AI Portal** (internal product name "AI Portal", public brand **Vortex**). Core product surfaces:

- **Chat** — streaming conversations against any model (Anthropic, OpenAI, Google, Mistral, open-source), with live tool visibility (memory, knowledge-base search, web search).
- **Knowledge bases** — upload docs, hybrid BM25 + pgvector + rerank retrieval, cited answers.
- **Memories** — auto-extracted user context (preferences / context / tools), editable, injected per thread.
- **Models** — a catalog with per-user entitlements, routing/fallback chains, cost + token metering.
- **Governance & guardrails** — PII redaction, prompt-injection & secrets detection, custom rules, audit log.
- **Observability** — every turn is a trace tree: prompt, retrieval, tool calls, cost.

Built on FastAPI + TanStack Start, Postgres/pgvector + Redis, deployable via one Docker Compose. Open-source at the core, self-hostable.

### Sources this system was derived from

Two Vortex/Claude design projects were the ground truth (stored here in case the reader has access):

- **Vortex app + marketing** — project `aed9902b-1388-47f9-ae14-3ab4ebd93828`. Files read: `styles.css` (app tokens), `Landing.html` (marketing site — canonical brand treatment), `src/*.jsx` (app shell, primitives, screens), and `uploads/` design specs — notably `2026-04-11-logo-design.md` (the Prism mark spec) and `2026-04-15-vortex-landing-page-design.md`.
- **Orrery / ORCHESTRA** — project `b7cdbad2-2b62-434b-8d46-d523572055df`, a futuristic multi-agent IDE. Used purely as **aesthetic inspiration** (dark stage, grid texture, radial glows, glowing primary buttons, mono-forward technical UI). Vortex's own identity — the Prism mark, the pink→violet→blue spectrum, IBM Plex type — is preserved; Orrery contributed the treatment, not the identity.

The two directions fuse cleanly: Vortex's real marketing brand is already dark and spectrum-glowing, so this system is **dark-first**, futuristic, and technical, with a light theme available for embedded/enterprise contexts.

---

## Content fundamentals (voice & tone)

How Vortex writes. Match this in any copy you generate.

- **Confident, plain, technical.** Short declaratives. "Streaming from turn one." "Policies that block." "Your infra, your data." No hype words, no exclamation marks.
- **Second person to the user, first person plural for the company.** "Ask anything." / "We believe every team should be able to talk to their work."
- **Product truth over marketing fluff.** Copy names real mechanisms: "hybrid BM25 + pgvector + rerank", "12-month default term and quarterly true-ups", "first byte under a second". It reads like it was written by people who ran this in production.
- **Three-beat headlines.** The hero is a triad: "Ask anything. / Know everything. / Ship faster." Section heads pair a plain clause with a gradient emphasis: "One portal. **Every model.**"
- **Mono eyebrows.** Sections open with an uppercase mono kicker: `AI PORTAL · BUILT FOR TEAMS`, `01 · COMPOSE`, `HOW IT WORKS`.
- **Casing:** Title case for the wordmark ("Vortex") and buttons ("Start for free"); sentence case for body; UPPERCASE (letterspaced) only for mono labels.
- **No emoji** in product UI. (One green 📚 grounding glyph appears historically, but prefer the line `book` icon.) Unicode `◆`, `·`, `→` are used as inline typographic marks.
- **Numbers are first-class.** Stats read as `10+`, `∞`, `100%`, `<1s`. Token cost and latency are shown, not hidden.

---

## Visual foundations

**Overall vibe:** two coherent surfaces from one brand. The **brand stage** (marketing, splash, brand moments) is a dark, futuristic control room — deep-space black, a faint 64px grid fading at the edges, three soft radial glows in the brand spectrum, the Prism refracting light. The **product console** (the primary working surface: chat, knowledge, models, governance) is its calm, high-legibility counterpart — **light by default**, neutral surfaces, hairline borders, a single violet accent, and semantic status doing the signalling. Same type, same Prism, same spectrum tokens; the difference is discipline — the console earns its restraint, the brand stage earns its glow.

- **Color.** The brand is three refracted rays — **pink `#f472b6` → violet `#a78bfa` → blue `#60a5fa`** — but they play two different roles. **Pink is brand-only**: it belongs to the Prism mark, the signature gradient, and the marketing stage — never to working console UI, where it reads consumer, not enterprise. **In product/admin surfaces the accent is a single trustworthy hue — violet** (`--vx-accent`), with blue and the semantic set (green `#22c55e`, amber `#f5b544`, red `#f2555a`) doing the real signalling. The gradient (`--vx-grad`) and the Prism glow are reserved for **brand moments** — the logo, the hero, the one marketing CTA — not data tables, KPI values, or policy rows. Neutrals are lavender-tinted on dark, cool-gray on light. Providers keep their own brand hues.
- **Theme.** Two stages, two jobs. **Light is the default for the product console** (Chat, Knowledge, Models, Governance, audit) — admins live in tables and forms all day and want a calm, high-legibility surface; set `data-theme="light"`. **Dark is the brand + marketing stage** (landing, splash, brand moments) — set `data-theme="dark"` and add `.vx-atmosphere` + `.vx-grid`. Chat surfaces and tool tints are theme-aware (`--vx-tool-*` tokens) so they hold up in both.
- **Type.** Three families: **Space Grotesk** (display — hero, section heads, KPI numbers; tracking −0.03em, gradient-ready), **IBM Plex Sans** (body & UI — Vortex's real product face, 13px base), **IBM Plex Mono** (labels, metrics, ids, chips, code). IBM Plex Serif is available for rare editorial quotes.
- **Backgrounds.** Never flat. Full-bleed dark stages get `.vx-atmosphere` (spectrum radial glows) + `.vx-grid` (masked grid lines). No photography in the product; marketing may use screenshots of the real app. No hand-drawn illustration — the Prism mark is the only illustrative element.
- **Borders & cards.** Hairline 1px borders (`--vx-line`, a barely-there `#161628`). Cards are panel-colored with `radius-lg` (12px). Corners are small and technical (3–8px on controls); only Tags and chips go pill.
- **Shadows & glows.** Elevation is dark-tuned (deep, soft, low-opacity black). The brand's real "shadow" is a **glow** — the Prism halo (`--vx-glow-violet/pink/blue`) — but it is a **brand-moment** device (logo, hero, marketing CTA), not a working-UI one. In the console, use hairlines + elevation, not glow. The `gradient` button variant is marketing-only; in product use the solid `accent` variant (violet fill, no glow).
- **Focus.** A violet ring: `border-color: accent` + `--vx-glow-ring` (3px soft violet). Consistent across every input.
- **Motion.** Quick and confident: `--vx-ease` cubic-bezier(.2,.8,.2,1), 120–200ms. Content animates *from* hidden via transform/opacity so it's never invisible if animation is skipped. Entrances rise 14px + fade. The Prism has a full lifecycle motion system (see below). Decorative loops are avoided except the Prism and live activity indicators. Everything respects `prefers-reduced-motion`.
- **Hover / press.** Hover lightens surface (`panel → panel-2`) or border (`line → line-3`); ghost controls gain a panel tint. Gradient CTAs brighten + lift. No aggressive scale on press.
- **Transparency & blur.** Sticky nav uses `backdrop-filter: blur(14px)` over `rgba(4,4,7,0.72)`. Tool chips use translucent tinted fills (e.g. `rgba(59,7,100,0.3)` for KB). Glows composite additively on the dark stage.
- **Layout.** App shell is a 44px topbar + 220px sidebar + fluid main, dense and information-rich (Linear-like). Marketing is a 1280px centered column with generous 120px section rhythm.

### The Prism mark

Vortex's logo is the **Prism**: a diamond (rotated square, vertices at 40,8 / 68,40 / 40,72 / 12,40) with three colored rays from the top vertex and a luminous lavender core — light entering a prism and refracting into a spectrum, i.e. many models unified through one interface. It has a **lifecycle motion system** driven by request state: `idle` (slow sway) · `loading` (360° spin) · `streaming` (pendulum + core pulse) · `thinking` (slow pendulum, amber palette) · `error` (shake, red) · `mono-white`/`mono-dark`. Shipped as the `PrismLogo` component and as static SVGs in `assets/`.

---

## Iconography

- **Custom line set.** Vortex uses a hand-built 16px line-icon set (1.25 stroke, round caps/joins, `currentColor`) — see `components/core/Icon.jsx`. It's the single source: nav glyphs (`chat`, `library`, `brain`, `cpu`, `key`, `gov`), actions (`search`, `plus`, `send`, `filter`, `copy`, `upload`, `paperclip`), and tool glyphs (`globe`, `wrench`, `sparkle`, `database`). Always use `Icon` rather than pulling in an external icon font.
- **Provider marks** are branded lettered squares (A/O/G/M) via `ProviderMark`, not logos.
- **No emoji** as UI icons. Inline typographic marks (`◆ · → ∞`) are used sparingly in mono contexts.
- **No CDN icon dependency** — the set is self-contained. If a glyph is missing, add it to `Icon.jsx` in the same 16px/1.25-stroke style rather than importing Lucide/Heroicons.
- Brand mark artwork lives in `assets/prism-logo.svg` (gradient) and `assets/prism-mono-white.svg`.

---

## Components

React primitives, exposed at `window.Vortex_469485`. Grouped by concern under `components/`.

**brand/** — `PrismLogo` (animated mark), `Wordmark`, `BrandLockup`.
**core/** — `Icon` (icon set), `Button`, `IconButton`, `Badge`, `Tag`, `Switch`, `Avatar`.
**forms/** — `Field`, `Input`, `Textarea`, `Select`, `Checkbox`.
**surfaces/** — `Card`, `Panel`, `Stat` (KPI), `Sparkline`.
**data/** — `ProviderMark`, `ProviderChip`, `ModelChip`, `DataTable`.
**feedback/** — `StatusPill`, `ToolChip`, `ThinkingDots`.
**chat/** — `ChatMessage`, `ThinkBlock`, `ToolCard`, `Composer`.
**navigation/** — `Tabs`, `FilterChip`, `SidebarItem`.

Each component directory has a `.jsx` implementation, a `.d.ts` props contract, a `.prompt.md` usage note, and a `@dsCard` HTML specimen. Styling is entirely via the CSS custom properties in `tokens/` — no CSS-in-JS libraries, React-only imports.

*Intentional additions* beyond the raw source: `Icon` wraps the product's inline SVG vocabulary as one component; `DataTable`, `Card`/`Panel`, `Field` generalize patterns the app repeats inline. Everything maps to something real in the Vortex codebase.

---

## Index / manifest

- `styles.css` — global entry point (import this one file). `@import`s everything below.
- `tokens/` — `fonts.css`, `colors.css`, `typography.css`, `spacing.css`, `effects.css`, `base.css`.
- `components/{brand,core,forms,surfaces,data,feedback,chat,navigation}/` — the primitives above.
- `foundations/*.card.html` — specimen cards (Colors, Type, Spacing, Brand) shown in the Design System tab.
- `ui_kits/app/` — the Vortex product shell (chat portal) recreation.
- `ui_kits/marketing/` — the Vortex marketing landing recreation.
- `assets/` — `prism-logo.svg`, `prism-mono-white.svg`.
- `SKILL.md` — Agent-Skills-compatible entry so this system can be used from Claude Code.

---

## Substitutions & notes

- **Fonts load from Google Fonts** (`tokens/fonts.css`), so no font binaries ship here and the compiler reports 0 `@font-face` fonts — that's expected. All three families (Space Grotesk, IBM Plex Sans/Mono/Serif) are exact matches to Vortex's real usage, not substitutes.
- **Dark is the brand stage; light is the console default.** Both themes are first-class. Use `[data-theme="light"]` for product/admin/governance surfaces (the default working environment) and `[data-theme="dark"]` for marketing, splash, and brand moments. Pink and the gradient/glow stay on the brand side of that line.
