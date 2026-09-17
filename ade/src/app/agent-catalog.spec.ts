import { describe, expect, it } from "vitest";
import { DEFAULT_RATES } from "./cost/estimate.service";
import { AGENT_TOOLS, defaultEffortFor, effortLevelsFor, modelOption } from "./data";
import { TOOL_ICON } from "./shared/tool-icons";
import { TOOL_GLYPH, toolMeta } from "./utils";

/** The agent-tool catalog is the single source the spawn modal, the settings
 *  modal and the tool badge all read — an entry is "registered" only when every
 *  one of those lookups resolves. These cover the `pi` entry (and keep the
 *  invariants honest for every other tool). */
describe("agent tool catalog", () => {
  const pi = AGENT_TOOLS.find((t) => t.id === "pi");

  it("registers pi alongside the other agent tools", () => {
    expect(AGENT_TOOLS.map((t) => t.id)).toEqual(["claude", "codex", "cursor", "gemini", "pi"]);
    expect(pi).toBeDefined();
    expect(pi!.name).toBe("Pi");
    expect(pi!.short).toBe("pi");
    expect(pi!.accent).toBe("var(--tool-pi)");
  });

  it("ships NO hardcoded models for pi — its CLI enumerates them", () => {
    // pi is BYOK across ~20 providers: a curated list would be invented, and
    // there is nothing to fall back to. The picker is fed by `pi --list-models`
    // through ModelCatalogService, and is free-text besides.
    expect(pi!.models).toEqual([]);
    expect(pi!.dynamicModels).toBe(true);
  });

  it("only pi and cursor are probe-backed; cursor keeps a curated fallback", () => {
    // `cursor-agent models` reports the signed-in account's pool, but it exits 0
    // with "No models available for this account." when signed out — so cursor
    // MUST keep a curated list for the picker to fall back to.
    const probeBacked = AGENT_TOOLS.filter((t) => t.dynamicModels).map((t) => t.id);
    expect(probeBacked.sort()).toEqual(["cursor", "pi"]);
    expect(AGENT_TOOLS.find((t) => t.id === "cursor")!.models.length).toBeGreaterThan(0);
    // claude / codex / gemini have no listing command at all — curated only.
    for (const t of AGENT_TOOLS.filter((x) => !x.dynamicModels)) {
      expect(t.models.length, `${t.id} curated models`).toBeGreaterThan(0);
    }
  });

  it("resolves pi's badge: drawn π glyph on a neutral (non-brand) accent", () => {
    expect(TOOL_ICON["pi"]).toMatch(/^M[\d.]/);
    expect(TOOL_GLYPH["pi"]).toBe("π");
    // pi ships no brand mark or colour, so the accent is the neutral ink token
    expect(toolMeta("pi").accent).toBe("var(--tool-pi)");
  });

  it("invents no pi rate rows — its ids are unknowable until the CLI reports", () => {
    for (const id of Object.keys(DEFAULT_RATES)) {
      expect(id.includes("/"), `no provider-prefixed rate key: ${id}`).toBe(false);
    }
  });

  it("keeps every tool's badge lookup resolvable", () => {
    for (const t of AGENT_TOOLS) {
      expect(TOOL_ICON[t.id], `icon for ${t.id}`).toBeTruthy();
      expect(toolMeta(t.id).name).toBe(t.name);
    }
  });
});
