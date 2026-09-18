/**
 * Task 8: the custom-renderer hook.
 *
 * The thing this client can do that the ratatui one will not — and the thing
 * it must still do correctly when nobody has registered anything, because one
 * deny rule on the `render` grant degrades every custom surface everywhere.
 */

import { Text } from "ink";
import { render } from "ink-testing-library";
import { afterEach, describe, expect, it } from "vitest";

import { clearRenderers, registerRenderer, registered, type CustomProps } from "../src/registry.js";
import { SurfaceNode } from "../src/surfaces/index.js";
import { surfaceFrom } from "./fake.js";

afterEach(() => {
  clearRenderers();
});

describe("a custom surface", () => {
  it("custom.falls_back_without_a_renderer", () => {
    const node = surfaceFrom("custom-with-fallback", "dag-1");
    const ui = render(<SurfaceNode node={node} width={60} />);
    const frame = ui.lastFrame() ?? "";
    // The fallback is mandatory on the wire so that this branch is never blank.
    expect(frame).toContain("proto -> agui -> kernel");
    expect(frame).toContain("buildgraph.dag (no renderer)");
    ui.unmount();
  });

  it("custom.uses_a_registered_renderer", () => {
    const seen: CustomProps[] = [];
    registerRenderer("buildgraph.dag", (props: CustomProps) => {
      seen.push(props);
      const nodes = (props.payload["nodes"] as string[]) ?? [];
      return <Text>dag of {nodes.length} nodes</Text>;
    });
    expect(registered()).toEqual(["buildgraph.dag"]);

    const node = surfaceFrom("custom-with-fallback", "dag-1");
    const ui = render(<SurfaceNode node={node} width={60} />);
    const frame = ui.lastFrame() ?? "";
    expect(frame).toContain("dag of 3 nodes");
    // The fallback is not drawn as well: one surface, one drawing.
    expect(frame).not.toContain("proto -> agui -> kernel");
    // The renderer gets the payload and the surface's id, nothing else.
    expect(seen[0]?.kind).toBe("buildgraph.dag");
    expect(seen[0]?.id).toBe("dag-1");
    expect(Object.keys(seen[0]?.payload ?? {}).sort()).toEqual(["edges", "nodes"]);
    ui.unmount();
  });

  it("a renderer registered for another kind does not claim this one", () => {
    registerRenderer("buildgraph.flamegraph", () => <Text>flames</Text>);
    const node = surfaceFrom("custom-with-fallback", "dag-1");
    const ui = render(<SurfaceNode node={node} width={60} />);
    expect(ui.lastFrame()).toContain("proto -> agui -> kernel");
    ui.unmount();
  });
});
