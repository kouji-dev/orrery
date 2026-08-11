import { Injector, runInInjectionContext, signal } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { Settings } from "../models";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { Updater, UpdateHandle, UPDATER } from "./updater";
import { FOCUS_MIN_GAP_MS, UpdateWatcherService } from "./update-watcher.service";

const handle = (version: string): UpdateHandle => ({
  version,
  date: "2026-08-11",
  notes: "n",
  downloadAndInstall: async () => {},
});

interface Made {
  svc: UpdateWatcherService;
  note: ReturnType<typeof vi.fn>;
  checking: ReturnType<typeof signal<boolean>>;
  installing: ReturnType<typeof signal<boolean>>;
}

function make(updater: Partial<Updater>, settings: Partial<Settings> = {}): Made {
  const full: Updater = {
    isAvailable: () => true,
    check: async () => null,
    relaunch: async () => {},
    ...updater,
  };
  const note = vi.fn();
  const checking = signal(false);
  const installing = signal(false);
  const store = {
    ready: async () => ({ ...settingsDefaults(), ...settings }),
    noteBackgroundUpdate: note,
    checking,
    installing,
  } as unknown as SettingsStore;
  const injector = Injector.create({
    providers: [
      { provide: UPDATER, useValue: full },
      { provide: SettingsStore, useValue: store },
    ],
  });
  const svc = runInInjectionContext(injector, () => new UpdateWatcherService());
  return { svc, note, checking, installing };
}

describe("UpdateWatcherService", () => {
  it("reports a release found mid-session — the gap the startup-only check left", async () => {
    const { svc, note } = make({ check: async () => handle("0.15.1") });
    await svc.check();
    expect(note).toHaveBeenCalledWith({ version: "0.15.1", date: "2026-08-11", notes: "n" });
  });

  it("reports null when the offer is gone, so a stale toast clears itself", async () => {
    const { svc, note } = make({ check: async () => null });
    await svc.check();
    expect(note).toHaveBeenCalledWith(null);
  });

  // 'manual' opts out of update checking entirely — the background poll must
  // honour that exactly as the startup flow does.
  it("never checks under the manual policy", async () => {
    const check = vi.fn(async () => handle("0.15.1"));
    const { svc, note } = make({ check }, { updatePolicy: "manual" });
    await svc.check();
    expect(check).not.toHaveBeenCalled();
    expect(note).not.toHaveBeenCalled();
  });

  // A background poll must not race the user's own "Check now", nor move the
  // goalposts while the toast is rendering live download progress.
  it("stands down while a manual check or an install is in flight", async () => {
    const check = vi.fn(async () => handle("0.15.1"));
    const { svc, checking, installing } = make({ check });

    checking.set(true);
    await svc.check();
    expect(check).not.toHaveBeenCalled();

    checking.set(false);
    installing.set(true);
    await svc.check();
    expect(check).not.toHaveBeenCalled();

    installing.set(false);
    await svc.check();
    expect(check).toHaveBeenCalledTimes(1);
  });

  // A transient network failure is not something the user asked about — it must
  // stay silent and leave the known state alone, unlike an explicit "Check now".
  it("swallows a failed poll without touching the known update", async () => {
    const { svc, note } = make({
      check: async () => {
        throw new Error("offline");
      },
    });
    await expect(svc.check()).resolves.toBeUndefined();
    expect(note).not.toHaveBeenCalled();
  });

  // 'auto' installs at BOOT, where nothing is running. Mid-session it must only
  // notify: a relaunch would kill every live agent PTY without warning.
  it("never installs or relaunches, even under the auto policy", async () => {
    const relaunch = vi.fn(async () => {});
    const downloadAndInstall = vi.fn(async () => {});
    const { svc, note } = make(
      { check: async () => ({ ...handle("0.15.1"), downloadAndInstall }), relaunch },
      { updatePolicy: "auto" },
    );
    await svc.check();
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
    expect(note).toHaveBeenCalledTimes(1);
  });

  it("polls on window focus, throttled so alt-tabbing can't storm the endpoint", async () => {
    vi.useFakeTimers();
    try {
      const check = vi.fn(async () => handle("0.15.1"));
      const { svc } = make({ check });
      svc.start();

      globalThis.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(check).toHaveBeenCalledTimes(1);

      // straight back → inside the throttle window, ignored
      globalThis.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(check).toHaveBeenCalledTimes(1);

      // past the window → a real re-check
      vi.setSystemTime(Date.now() + FOCUS_MIN_GAP_MS + 1);
      globalThis.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(check).toHaveBeenCalledTimes(2);

      svc.stop();
      vi.setSystemTime(Date.now() + FOCUS_MIN_GAP_MS + 1);
      globalThis.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(check).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
