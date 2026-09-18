/**
 * Task 6: every `SurfaceKind` variant has a component.
 *
 * The discriminants come from the generated schema that ships inside
 * `@orrery/protocol`, not from a list retyped here — a list retyped here would
 * pass forever while the protocol grew a thirteenth kind.
 *
 * The compile-time half lives in `src/surfaces/index.tsx`: `COMPONENTS` is a
 * mapped type over the same union and the switch's default arm is typed
 * `never`, so a new variant is a TypeScript error before it is a test failure.
 */

import { render } from "ink-testing-library";
import { describe, expect, it } from "vitest";

import schema from "@orrery/protocol/protocol.schema.json" with { type: "json" };

import { COMPONENTS, SurfaceNode } from "../src/surfaces/index.js";

/** Every `t` the generated schema lists for `SurfaceKind`. */
function discriminants(): string[] {
  const kinds = (schema as {
    definitions: { SurfaceKind: { oneOf: Array<{ properties?: { t?: { enum?: string[] } } }> } };
  }).definitions.SurfaceKind.oneOf;
  return kinds.map((variant) => {
    const tag = variant.properties?.t?.enum?.[0];
    if (!tag) throw new Error("a SurfaceKind variant with no `t`");
    return tag;
  });
}

describe("the surface switch", () => {
  it("surfaces.every_kind_has_a_component", () => {
    const tags = discriminants();
    expect(tags.length).toBeGreaterThanOrEqual(12);
    for (const tag of tags) {
      expect(Object.keys(COMPONENTS), `no component for \`${tag}\``).toContain(tag);
    }
    // And nothing registered that the protocol does not have.
    expect(Object.keys(COMPONENTS).sort()).toEqual([...tags].sort());
  });

  it("draws something for a kind it has never seen", () => {
    // A kernel one version ahead sends a variant this build has no component
    // for. The store keeps it; the switch must not throw over it.
    const node = { id: "x", status: null, kind: { t: "hologram", value: "…" } };
    const ui = render(<SurfaceNode node={node as never} width={40} />);
    expect(ui.lastFrame()).toContain("unknown surface");
    ui.unmount();
  });
});
