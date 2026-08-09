import { Injector, runInInjectionContext } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BRIDGE, Commands } from "../data-source/bridge";
import { UiStore } from "../ui/ui.store";
import { LocalHistoryStore } from "./local-history.store";

describe("LocalHistoryStore", () => {
  let invoke: ReturnType<typeof vi.fn>;
  let flash: ReturnType<typeof vi.fn>;
  let store: LocalHistoryStore;

  const SNAPS = [
    { id: "s2", ts: 2000, trigger: "watch", files: [{ path: "a.ts", hash: "h2", size: 10 }] },
    { id: "s1", ts: 1000, trigger: "watch", files: [{ path: "a.ts", hash: "h1", size: 9 }] },
  ];

  beforeEach(() => {
    invoke = vi.fn((cmd: string) => {
      if (cmd === Commands.HistoryList) return Promise.resolve(SNAPS);
      if (cmd === Commands.HistoryRestore) return Promise.resolve(["a.ts"]);
      if (cmd === Commands.HistoryFile)
        return Promise.resolve({ old: "v1", new: "v2", lang: "javascript" });
      return Promise.resolve(undefined);
    });
    flash = vi.fn();
    const injector = Injector.create({
      providers: [
        { provide: BRIDGE, useValue: { invoke } },
        { provide: UiStore, useValue: { flash } },
      ],
    });
    store = runInInjectionContext(injector, () => new LocalHistoryStore());
  });

  it("load fills the timeline for the agent", async () => {
    await store.load("a1");
    expect(store.snapshots()).toEqual(SNAPS);
    expect(store.loadedFor()).toBe("a1");
  });

  it("restore invokes the command, flashes, and reloads", async () => {
    const ok = await store.restore("a1", "s1", ["a.ts"]);
    expect(ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith(Commands.HistoryRestore, {
      id: "a1",
      snap: "s1",
      paths: ["a.ts"],
    });
    expect(flash).toHaveBeenCalledWith("Restored 1 file");
    expect(invoke).toHaveBeenCalledWith(Commands.HistoryList, { id: "a1" });
  });

  it("a failed restore surfaces the backend error", async () => {
    invoke.mockImplementationOnce(() => Promise.reject(new Error("snapshot not found")));
    const ok = await store.restore("a1", "gone");
    expect(ok).toBe(false);
    expect(flash).toHaveBeenCalledWith("snapshot not found");
  });

  it("fileDiff returns the FileDiff shape for the diff surface", async () => {
    const d = await store.fileDiff("a1", "s1", "a.ts");
    expect(d).toEqual({ old: "v1", new: "v2", lang: "javascript" });
  });
});
