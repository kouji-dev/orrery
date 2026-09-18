/**
 * Phase 4, in this client: three ported extensions, drawn.
 *
 * §8's acceptance criterion is that three ported extensions render with no
 * drawing code of their own. The frames come from
 * `clients/conformance/ported/*.jsonl`, which `cargo test -p orrery-ported`
 * writes by actually loading those extensions through `orrery-host` and
 * running them — so this suite draws the same bytes the ratatui one drew,
 * rather than a hand-written imitation of them.
 *
 * Nothing here talks to a network or a model, and no fixture was written by
 * hand.
 */

import { readdirSync } from "node:fs";
import { join } from "node:path";

import { render } from "ink-testing-library";
import { describe, expect, it } from "vitest";

import { conformance } from "@orrery/client";

import { Turn } from "../src/turn.js";

const dir = join(conformance.fixturesDir(), "ported");
const names = readdirSync(dir)
  .filter((f) => f.endsWith(".jsonl"))
  .sort();

describe("the ported extensions", () => {
  it("are three, and they come from the extensions rather than from here", () => {
    expect(names).toEqual([
      "ported-patch-review.jsonl",
      "ported-release-train.jsonl",
      "ported-workspace-census.jsonl",
    ]);
  });

  for (const file of names) {
    const scenario = conformance.load(join(dir, file));

    it(`${scenario.name}: the store agrees, then it draws`, () => {
      for (const { index, actual, expected } of conformance.checkpoints(scenario)) {
        expect(actual, `${scenario.name} checkpoint ${index}`).toEqual(expected);
      }

      const store = conformance.replay(scenario);
      const ui = render(
        <>
          {store.state().turns.map((turn) => (
            <Turn key={turn.id} turn={turn} width={72} />
          ))}
        </>,
      );
      const frame = ui.lastFrame() ?? "";
      expect(frame).not.toBe("");
      expect(frame).toMatchSnapshot();
      ui.unmount();
    });
  }

  /**
   * The custom surface has no renderer registered here either, so this client
   * draws the fallback — and the fallback has to be worth reading (§6.2).
   */
  it("the timeline falls back, informatively", () => {
    const scenario = conformance.load(join(dir, "ported-release-train.jsonl"));
    const store = conformance.replay(scenario);
    const ui = render(
      <>
        {store.state().turns.map((turn) => (
          <Turn key={turn.id} turn={turn} width={72} />
        ))}
      </>,
    );
    const frame = ui.lastFrame() ?? "";
    expect(frame).toContain("example-release-train.timeline (no renderer)");
    for (const stage of [
      "surface schema",
      "kernel differ",
      "ratatui client",
      "ink client",
      "ade client",
    ]) {
      expect(frame, `the fallback drops ${stage}`).toContain(stage);
    }
    expect(frame).toContain("stages shipped");
    ui.unmount();
  });
});
