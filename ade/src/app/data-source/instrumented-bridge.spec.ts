import { describe, expect, it, vi } from "vitest";
import { Bridge } from "./bridge";
import { InstrumentedBridge } from "./instrumented-bridge";
import { PerfStore } from "../perf/perf.store";

function setup(inner: Partial<Bridge>) {
  const perf = new PerfStore();
  const base: Bridge = {
    invoke: async () => undefined as never,
    on: async () => () => {},
    pickDirectory: async () => null,
    ...inner,
  };
  return { bridge: new InstrumentedBridge(base, perf), perf };
}

describe("InstrumentedBridge", () => {
  it("records a successful round-trip (ok) and returns the result", async () => {
    const { bridge, perf } = setup({ invoke: async () => "result" as never });
    expect(await bridge.invoke("agent_list")).toBe("result");
    const r = perf.rows().find((x) => x.cmd === "agent_list")!;
    expect(r.calls10s).toBe(1);
    expect(r.errPct).toBe(0);
  });

  it("records a failed round-trip (err) and re-throws", async () => {
    const { bridge, perf } = setup({
      invoke: async () => {
        throw new Error("boom");
      },
    });
    await expect(bridge.invoke("agent_spawn")).rejects.toThrow("boom");
    const r = perf.rows().find((x) => x.cmd === "agent_spawn")!;
    expect(r.calls10s).toBe(1);
    expect(r.errPct).toBe(100);
  });

  it("delegates on() and pickDirectory() unchanged", async () => {
    const un = vi.fn();
    const on = vi.fn(async () => un);
    const pickDirectory = vi.fn(async () => "/tmp");
    const { bridge } = setup({ on, pickDirectory });
    const handler = () => {};
    expect(await bridge.on("e", handler)).toBe(un);
    expect(on).toHaveBeenCalledWith("e", handler);
    expect(await bridge.pickDirectory()).toBe("/tmp");
  });
});
