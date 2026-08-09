import { provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BRIDGE } from "../data-source/bridge";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { TabCloseGuardService } from "./tab-close-guard.service";

describe("TabCloseGuardService", () => {
  let invoke: ReturnType<typeof vi.fn>;
  let guard: TabCloseGuardService;
  let ui: UiStore;
  let edits: EditsStore;
  let tabId: string;

  beforeEach(() => {
    invoke = vi.fn().mockResolvedValue(undefined);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: BRIDGE, useValue: { invoke } }],
    });
    guard = TestBed.inject(TabCloseGuardService);
    ui = TestBed.inject(UiStore);
    edits = TestBed.inject(EditsStore);
    ui.openAgent("a1");
    tabId = ui.activeTab();
  });

  const hasTab = () => ui.tabs().some((t) => t.id === tabId);

  it("closes a tab with no dirty buffers immediately, no dialog", () => {
    edits.open("a1", "src/clean.ts", "one"); // clean buffer is not a blocker
    guard.requestClose(tabId);
    expect(guard.pending()).toBeNull();
    expect(hasTab()).toBe(false);
  });

  it("a dirty buffer in the tab's pane tree blocks the close behind the dialog", () => {
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    guard.requestClose(tabId);
    expect(hasTab()).toBe(true);
    expect(guard.pending()).toEqual({ tabId, files: [{ agentId: "a1", path: "src/a.ts" }] });

    guard.cancel();
    expect(guard.pending()).toBeNull();
    expect(hasTab()).toBe(true);
  });

  it("Save all & close writes every dirty buffer, then closes the tab", async () => {
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    edits.open("a1", "src/b.ts", "two");
    edits.update("a1", "src/b.ts", "two!");
    guard.requestClose(tabId);
    await guard.saveAndClose();
    expect(invoke).toHaveBeenCalledWith("file_write", { id: "a1", path: "src/a.ts", content: "one!" });
    expect(invoke).toHaveBeenCalledWith("file_write", { id: "a1", path: "src/b.ts", content: "two!" });
    expect(hasTab()).toBe(false);
    expect(edits.dirtyKeys().size).toBe(0);
  });

  it("a failing save keeps the tab open", async () => {
    invoke.mockRejectedValueOnce(new Error("disk full"));
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    guard.requestClose(tabId);
    await guard.saveAndClose();
    expect(hasTab()).toBe(true);
    expect(edits.isDirty("a1", "src/a.ts")).toBe(true);
  });

  it("Discard drops the buffers and closes", () => {
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    guard.requestClose(tabId);
    guard.discardAndClose();
    expect(hasTab()).toBe(false);
    expect(edits.get("a1", "src/a.ts")).toBeUndefined();
    // no write went out (the injected SettingsStore's own settings_get doesn't count)
    expect(invoke.mock.calls.filter((c) => c[0] === "file_write")).toHaveLength(0);
  });
});
