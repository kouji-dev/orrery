import { ApplicationRef, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Commands, BRIDGE } from "../data-source/bridge";
import { SettingsStore } from "../settings/settings.store";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { AUTOSAVE_DELAY_MS, FileSaveService } from "./file-save.service";

const A = "agent-1";
const P = "src/lib.rs";

describe("FileSaveService", () => {
  let invoke: ReturnType<typeof vi.fn>;
  let svc: FileSaveService;
  let edits: EditsStore;
  let flash: ReturnType<typeof vi.fn>;

  // the injected SettingsStore issues its own settings_get through the shared
  // bridge mock — count only the write path
  const fileWrites = () => invoke.mock.calls.filter((c) => c[0] === Commands.FileWrite);

  beforeEach(() => {
    invoke = vi.fn().mockResolvedValue(undefined);
    flash = vi.fn();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: BRIDGE, useValue: { invoke } },
        { provide: UiStore, useValue: { flash } },
      ],
    });
    svc = TestBed.inject(FileSaveService);
    edits = TestBed.inject(EditsStore);
  });

  it("writes the dirty buffer and marks it saved (echo-tolerance)", async () => {
    edits.open(A, P, "one");
    edits.update(A, P, "one edited");
    await expect(svc.save(A, P)).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledWith(Commands.FileWrite, {
      id: A,
      path: P,
      content: "one edited",
    });
    expect(edits.get(A, P)).toEqual({ baseText: "one edited", text: "one edited", dirty: false });
    expect(flash).toHaveBeenCalledWith("Saved lib.rs");
  });

  it("clean or unknown buffers save as a silent no-op", async () => {
    await expect(svc.save(A, P)).resolves.toBe(true); // no buffer at all
    edits.open(A, P, "one");
    await expect(svc.save(A, P)).resolves.toBe(true); // clean buffer
    expect(fileWrites()).toHaveLength(0);
    expect(flash).not.toHaveBeenCalled();
  });

  it("a failed write keeps the buffer dirty and reports the error", async () => {
    invoke.mockRejectedValueOnce(new Error("disk on fire"));
    edits.open(A, P, "one");
    edits.update(A, P, "two");
    await expect(svc.save(A, P)).resolves.toBe(false);
    expect(edits.isDirty(A, P)).toBe(true);
    expect(flash).toHaveBeenCalledWith("Save failed: disk on fire");
  });

  it("saveAll writes every dirty buffer across agents and flashes once", async () => {
    edits.open(A, P, "one");
    edits.update(A, P, "one!");
    edits.open("agent-2", "src/other.ts", "two");
    edits.update("agent-2", "src/other.ts", "two!");
    edits.open(A, "src/clean.ts", "untouched"); // clean — must not be written
    await expect(svc.saveAll()).resolves.toBe(true);
    expect(fileWrites()).toHaveLength(2);
    expect(invoke).toHaveBeenCalledWith(Commands.FileWrite, { id: A, path: P, content: "one!" });
    expect(invoke).toHaveBeenCalledWith(Commands.FileWrite, { id: "agent-2", path: "src/other.ts", content: "two!" });
    expect(edits.dirtyKeys().size).toBe(0);
    expect(flash).toHaveBeenCalledTimes(1);
    expect(flash).toHaveBeenCalledWith("Saved 2 files");
  });

  it("saveAll with a single dirty file flashes its name; clean workspace is silent", async () => {
    await expect(svc.saveAll()).resolves.toBe(true);
    expect(flash).not.toHaveBeenCalled();
    edits.open(A, P, "one");
    edits.update(A, P, "one!");
    await expect(svc.saveAll()).resolves.toBe(true);
    expect(flash).toHaveBeenCalledWith("Saved lib.rs");
  });

  it("a save already in flight is not doubled", async () => {
    let release!: () => void;
    invoke.mockImplementation(
      () => new Promise<void>((r) => (release = () => r())),
    );
    edits.open(A, P, "one");
    edits.update(A, P, "two");
    const first = svc.save(A, P);
    await expect(svc.save(A, P)).resolves.toBe(false); // rejected as duplicate
    release();
    await expect(first).resolves.toBe(true);
    expect(fileWrites()).toHaveLength(1);
  });

  // ----- B1.2 autosave (opt-in setting) -----

  describe("autosave", () => {
    afterEach(() => vi.useRealTimers());

    const flushEffects = () => TestBed.inject(ApplicationRef).tick();

    it("writes dirty buffers quietly after the idle delay; typing re-arms it", async () => {
      vi.useFakeTimers();
      TestBed.inject(SettingsStore).set({ autosave: true });
      edits.open(A, P, "one");
      edits.update(A, P, "one!");
      flushEffects();
      expect(fileWrites()).toHaveLength(0); // still inside the debounce

      // typing again before the deadline re-arms the timer
      await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS - 500);
      edits.update(A, P, "one!!");
      flushEffects();
      await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS - 500);
      expect(fileWrites()).toHaveLength(0);

      await vi.advanceTimersByTimeAsync(600);
      expect(fileWrites()).toHaveLength(1);
      expect(fileWrites()[0][1]).toEqual({ id: A, path: P, content: "one!!" });
      expect(edits.isDirty(A, P)).toBe(false);
      expect(flash).not.toHaveBeenCalled(); // autosave never toasts on success
    });

    it("does nothing while the setting is off (the default)", async () => {
      vi.useFakeTimers();
      edits.open(A, P, "one");
      edits.update(A, P, "one!");
      flushEffects();
      await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS * 3);
      expect(fileWrites()).toHaveLength(0);
      expect(edits.isDirty(A, P)).toBe(true);
    });
  });
});
