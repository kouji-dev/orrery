import { Injector, runInInjectionContext } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BRIDGE, Commands } from "../data-source/bridge";
import { UiStore } from "../ui/ui.store";
import { BranchesStore } from "./branches.store";

describe("BranchesStore", () => {
  let invoke: ReturnType<typeof vi.fn>;
  let flash: ReturnType<typeof vi.fn>;
  let store: BranchesStore;

  const BRANCHES = [
    { name: "main", current: true, checkedOutIn: "", ahead: 0, behind: 0 },
    { name: "feature", current: false, ahead: 1, behind: 2, upstream: "origin/feature" },
  ];
  const REMOTES = [{ name: "origin", url: "git@example.com:x.git" }];

  beforeEach(() => {
    invoke = vi.fn((cmd: string) => {
      if (cmd === Commands.ProjectBranchesDetail) return Promise.resolve(BRANCHES);
      if (cmd === Commands.ProjectRemotes) return Promise.resolve(REMOTES);
      return Promise.resolve(undefined);
    });
    flash = vi.fn();
    const injector = Injector.create({
      providers: [
        { provide: BRIDGE, useValue: { invoke } },
        { provide: UiStore, useValue: { flash } },
      ],
    });
    store = runInInjectionContext(injector, () => new BranchesStore());
  });

  it("load fills branches + remotes and records the project", async () => {
    await store.load("p1");
    expect(store.branches()).toEqual(BRANCHES);
    expect(store.remotes()).toEqual(REMOTES);
    expect(store.loadedFor()).toBe("p1");
    expect(store.busy()).toBe(false);
  });

  it("a failed load falls back to empty lists without flashing", async () => {
    invoke.mockRejectedValueOnce(new Error("not a repo"));
    await store.load("p1");
    expect(store.branches()).toEqual([]);
    expect(store.loadedFor()).toBeNull();
    expect(flash).not.toHaveBeenCalled();
  });

  it("ops invoke their command, flash success, and reload", async () => {
    const ok = await store.create("p1", "feat-x", "main");
    expect(ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith(Commands.ProjectBranchCreate, {
      id: "p1",
      name: "feat-x",
      from: "main",
    });
    expect(flash).toHaveBeenCalledWith("Created feat-x");
    // reload happened
    expect(invoke).toHaveBeenCalledWith(Commands.ProjectBranchesDetail, { id: "p1" });
  });

  it("a failed op flashes the backend error and resolves false", async () => {
    invoke.mockImplementationOnce(() => Promise.reject(new Error("branch 'x' is checked out in worktree 'wt'")));
    const ok = await store.delete("p1", "x");
    expect(ok).toBe(false);
    expect(flash).toHaveBeenCalledWith("branch 'x' is checked out in worktree 'wt'");
  });

  it("checkout targets the agent worktree command", async () => {
    await store.checkoutAgent("p1", "a1", "feature");
    expect(invoke).toHaveBeenCalledWith(Commands.AgentCheckout, { id: "a1", branch: "feature" });
  });
});
