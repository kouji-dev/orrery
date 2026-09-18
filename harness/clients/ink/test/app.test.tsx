/**
 * Tasks 1 and 2: the app drains a stream, and settled turns go to `<Static>`.
 */

import { render } from "ink-testing-library";
import { describe, expect, it, vi } from "vitest";

import { App } from "../src/app.js";
import { FakeConnection, frames, offset, settle } from "./fake.js";

/** How many times each turn's component was rendered. */
const renders = new Map<string, number>();

vi.mock("../src/turn.js", async (importOriginal) => {
  const real = await importOriginal<typeof import("../src/turn.js")>();
  return {
    ...real,
    Turn: (props: Parameters<typeof real.Turn>[0]) => {
      // A settled turn is only ever drawn from inside `<Static>`; the live
      // one is drawn from the dynamic region. Counting them apart is what
      // makes "printed once" checkable.
      const key = `${props.turn.id}:${props.turn.settled ? "static" : "live"}`;
      renders.set(key, (renders.get(key) ?? 0) + 1);
      return real.Turn(props);
    },
  };
});

describe("the app", () => {
  it("connects_and_drains", async () => {
    const connection = new FakeConnection(frames("tool-call"));
    let drained: unknown = null;
    const ui = render(
      <App connection={connection} session="sess-1" onDrained={(s) => (drained = s.state())} />,
    );

    await settle(24);
    connection.close();
    await settle(4);

    expect(drained).not.toBeNull();
    expect((drained as { last_seq: number }).last_seq).toBe(10);
    expect((drained as { turns: unknown[] }).turns).toHaveLength(1);
    expect(ui.lastFrame()).toBeDefined();
    ui.unmount();
    // Unmounting must not throw and must not leave the pump running.
    await settle(2);
    ui.cleanup();
  });

  it("settled_turns_go_static", async () => {
    // Two turns in one stream: text-only settles, then tool-call runs.
    renders.clear();
    const first = frames("text-only");
    const second = offset(frames("tool-call"), first.length);
    const connection = new FakeConnection([...first, ...second]);
    const ui = render(<App connection={connection} session="sess-1" />);

    await settle(40);
    connection.close();
    await settle(4);

    // `<Static>` renders a settled turn once and never again, whatever else
    // happens on the screen; the live turn is redrawn on every frame.
    expect(renders.get("turn-1:static"), "turn-1 was re-printed").toBe(1);
    expect(renders.get("turn-2:static"), "turn-2 was re-printed").toBe(1);
    expect(renders.get("turn-2:live") ?? 0).toBeGreaterThan(1);
    // Both turns are on the screen, the settled one exactly once.
    const frame = ui.lastFrame() ?? "";
    expect(frame.split("── turn-1").length - 1).toBe(1);
    expect(frame).toContain("── turn-2");
    ui.unmount();
  });

  it("gap_triggers_reattach", async () => {
    const connection = new FakeConnection(frames("seq-gap"));
    const ui = render(<App connection={connection} session="sess-1" />);

    await settle(24);
    connection.close();
    await settle(4);

    const attaches = connection.of("attach");
    expect(attaches).toHaveLength(1);
    // `since` is the last seq this client actually saw, not the one it missed.
    expect(attaches[0]!.args).toEqual(["sess-1", 3]);
    ui.unmount();
  });
});
