/**
 * Task 3: one component per core surface, drawn from the conformance fixture
 * it belongs to, snapshotted wide and narrow.
 *
 * The narrow snapshot is the point of the second half: a surface that only
 * looks right at 100 columns is a surface that breaks in a split pane.
 */

import { render } from "ink-testing-library";
import { describe, expect, it } from "vitest";

import { SurfaceNode } from "../src/surfaces/index.js";
import { surfaceFrom } from "./fake.js";

/** Every core kind, and the fixture surface it is drawn from. */
const CASES: Array<[name: string, scenario: string, id: string]> = [
  ["text", "custom-with-fallback", "dag-1:fallback"],
  ["markdown", "text-only", "msg-1"],
  ["table", "table-then-resort", "tbl-1"],
  ["tree", "tree-surface", "tree-1"],
  ["diff", "diff-surface", "diff-1"],
  ["progress", "progress-surface", "prog-1"],
  ["stream", "stream-surface", "stream-1"],
  ["task", "task-surface", "task-1"],
  ["question", "question-surface", "q-1"],
  ["form", "form-surface", "form-1"],
  ["stack", "tool-call", "call-1"],
  ["custom", "custom-with-fallback", "dag-1"],
];

describe("the surface components", () => {
  for (const [name, scenario, id] of CASES) {
    it(`surfaces.${name}`, () => {
      const node = surfaceFrom(scenario, id);
      const ui = render(<SurfaceNode node={node} width={72} />);
      const frame = ui.lastFrame() ?? "";
      expect(frame).not.toContain("[no renderer for");
      expect(frame).toMatchSnapshot();
      ui.unmount();
    });

    it(`surfaces.${name}.narrow`, () => {
      const node = surfaceFrom(scenario, id);
      const ui = render(<SurfaceNode node={node} width={24} />);
      const frame = ui.lastFrame() ?? "";
      expect(frame).not.toContain("[no renderer for");
      // Degradation, not overflow: nothing may be wider than the budget.
      for (const line of frame.split("\n")) {
        expect(line.replace(/\[[0-9;]*m/g, "").length).toBeLessThanOrEqual(24);
      }
      expect(frame).toMatchSnapshot();
      ui.unmount();
    });
  }

  it("never echoes a secret field", () => {
    const node = surfaceFrom("form-surface", "form-1");
    const ui = render(<SurfaceNode node={node} width={72} />);
    const frame = ui.lastFrame() ?? "";
    expect(frame).toContain("Token");
    // The value slot for a secret shows the kind, never anything typed into it.
    expect(frame).not.toMatch(/Token\s+[^•\s]*[a-z]/);
    ui.unmount();
  });
});
