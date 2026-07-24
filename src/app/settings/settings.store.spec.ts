import { Injector, runInInjectionContext } from "@angular/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AGENT_TOOLS } from "../data";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { Settings, UpdateInfo } from "../models";
import { UiStore } from "../ui/ui.store";
import { SETTINGS_SAVE_DEBOUNCE_MS, settingsDefaults, SettingsStore } from "./settings.store";

interface Made {
  store: SettingsStore;
  invoke: ReturnType<typeof vi.fn>;
  flash: ReturnType<typeof vi.fn>;
}

function make(opts: { stored?: Partial<Settings> | null; update?: UpdateInfo | string | null; failGet?: boolean; failCheck?: boolean } = {}): Made {
  const invoke = vi.fn(async (cmd: string) => {
    if (cmd === "settings_get") {
      if (opts.failGet) throw new Error("no backend");
      return opts.stored ?? {};
    }
    if (cmd === "update_check") {
      if (opts.failCheck) throw new Error("command not found");
      return opts.update ?? null;
    }
    return undefined;
  });
  const bridge: Bridge = {
    invoke: invoke as Bridge["invoke"],
    on: async () => () => {},
    pickDirectory: async () => null,
  };
  const flash = vi.fn();
  const injector = Injector.create({
    providers: [
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash } },
    ],
  });
  const store = runInInjectionContext(injector, () => new SettingsStore());
  return { store, invoke, flash };
}

const setCalls = (invoke: ReturnType<typeof vi.fn>) =>
  invoke.mock.calls.filter(([cmd]) => cmd === "settings_set");

afterEach(() => vi.useRealTimers());

describe("SettingsStore load", () => {
  it("merges the persisted document over the defaults on init", async () => {
    const { store } = make({ stored: { channel: "beta", volume: 25, toolModel: { claude: "opus-custom" } } });
    const s = await store.ready();
    expect(s.channel).toBe("beta");
    expect(s.volume).toBe(25);
    expect(s.toolModel["claude"]).toBe("opus-custom");
    // missing keys fall back to defaults
    expect(s.updatePolicy).toBe("notify");
    expect(s.events.finished).toBe(true);
  });

  it("keeps the defaults when the backend is unavailable", async () => {
    const { store } = make({ failGet: true });
    const s = await store.ready();
    expect(s).toEqual(settingsDefaults());
    expect(store.anyDirty()).toBe(false);
  });
});

describe("SettingsStore instant-apply + debounce", () => {
  it("applies edits to the signal instantly but persists once, debounced", async () => {
    vi.useFakeTimers();
    const { store, invoke } = make();
    await store.ready();

    store.set({ channel: "beta" });
    expect(store.settings().channel).toBe("beta"); // instant
    store.set({ volume: 30 });

    vi.advanceTimersByTime(SETTINGS_SAVE_DEBOUNCE_MS - 1);
    expect(setCalls(invoke)).toHaveLength(0); // still pending

    vi.advanceTimersByTime(1);
    const calls = setCalls(invoke);
    expect(calls).toHaveLength(1); // both edits coalesced into ONE write
    const sent = (calls[0][1] as { settings: Settings }).settings;
    expect(sent.channel).toBe("beta");
    expect(sent.volume).toBe(30);
  });

  it("restarts the debounce window on every edit", async () => {
    vi.useFakeTimers();
    const { store, invoke } = make();
    await store.ready();

    store.set({ sound: false });
    vi.advanceTimersByTime(SETTINGS_SAVE_DEBOUNCE_MS - 50);
    store.set({ soundName: "Pop" });
    vi.advanceTimersByTime(SETTINGS_SAVE_DEBOUNCE_MS - 50);
    expect(setCalls(invoke)).toHaveLength(0);
    vi.advanceTimersByTime(50);
    expect(setCalls(invoke)).toHaveLength(1);
  });
});

describe("SettingsStore map overrides", () => {
  it("stores overrides and trims values equal to the tool default back out", async () => {
    const { store } = make();
    await store.ready();

    store.setMap("toolModel", "claude", "my-custom-model");
    expect(store.settings().toolModel["claude"]).toBe("my-custom-model");
    expect(store.anyDirty()).toBe(true);

    // setting the curated default (= models[0]) removes the key (absent = default)
    store.setMap("toolModel", "claude", AGENT_TOOLS[0].models[0]);
    expect(store.settings().toolModel["claude"]).toBeUndefined();

    store.setMap("autoApprove", "codex", "everything");
    expect(store.settings().autoApprove["codex"]).toBe("everything");
    store.setMap("autoApprove", "codex", "off"); // "off" is the implicit default
    expect(store.settings().autoApprove["codex"]).toBeUndefined();
    expect(store.anyDirty()).toBe(false);
  });

  it("setEvent toggles one event flag", async () => {
    const { store } = make();
    await store.ready();
    store.setEvent("error", false);
    expect(store.settings().events.error).toBe(false);
    expect(store.settings().events.finished).toBe(true);
  });
});

describe("SettingsStore resetAll", () => {
  it("restores the defaults and persists them", async () => {
    vi.useFakeTimers();
    const { store, invoke } = make({ stored: { channel: "beta", autoResume: true } });
    await store.ready();
    expect(store.anyDirty()).toBe(true);

    store.resetAll();
    expect(store.settings()).toEqual(settingsDefaults());
    expect(store.anyDirty()).toBe(false);

    vi.advanceTimersByTime(SETTINGS_SAVE_DEBOUNCE_MS);
    const calls = setCalls(invoke);
    expect((calls.at(-1)![1] as { settings: Settings }).settings).toEqual(settingsDefaults());
  });
});

describe("SettingsStore update check/install", () => {
  it("checkNow asks update_check on the configured channel and surfaces the result", async () => {
    const info: UpdateInfo = { version: "1.2.3", date: "Jun 9, 2026", notes: null };
    const { store, invoke } = make({ stored: { channel: "beta" }, update: info });
    await store.ready();

    await store.checkNow();
    expect(invoke).toHaveBeenCalledWith("update_check", { channel: "beta" });
    expect(store.updateInfo()).toEqual(info);
    expect(store.updateCard()).toEqual(info);
    expect(store.updateKnown()).toBe(true);
    expect(store.lastCheckedAt()).not.toBeNull();
    expect(store.checking()).toBe(false);
  });

  it("tolerates the legacy bare-string response", async () => {
    const { store } = make({ update: "0.9.4" });
    await store.ready();
    await store.checkNow();
    expect(store.updateInfo()).toEqual({ version: "0.9.4" });
  });

  it("records 'up to date' (null) and keeps the card hidden", async () => {
    const { store } = make({ update: null });
    await store.ready();
    await store.checkNow();
    expect(store.updateInfo()).toBeNull();
    expect(store.updateKnown()).toBe(false);
    expect(store.lastCheckedAt()).not.toBeNull();
  });

  it("flashes on a rejected check (command missing / offline) without crashing", async () => {
    const { store, flash } = make({ failCheck: true });
    await store.ready();
    await store.checkNow();
    expect(flash).toHaveBeenCalledWith("update check failed");
    expect(store.checking()).toBe(false);
    expect(store.updateInfo()).toBeNull();
  });

  it("'Later' hides the card but the update stays known (nav dot)", async () => {
    const { store } = make({ update: { version: "2.0.0" } });
    await store.ready();
    await store.checkNow();
    expect(store.updateCard()).toEqual({ version: "2.0.0" });
    store.dismissUpdate();
    expect(store.updateCard()).toBeNull();
    expect(store.updateKnown()).toBe(true); // the nav dot survives "Later"
    // a fresh positive check re-surfaces the card
    await store.checkNow();
    expect(store.updateCard()).toEqual({ version: "2.0.0" });
  });

  it("install invokes update_install with the channel and flashes on failure", async () => {
    const { store, invoke, flash } = make({ stored: { channel: "beta" } });
    await store.ready();
    invoke.mockImplementationOnce(async () => {
      throw new Error("not yet wired");
    });
    await store.install();
    expect(invoke).toHaveBeenCalledWith("update_install", { channel: "beta" });
    expect(flash).toHaveBeenCalledWith("update install failed");
    expect(store.installing()).toBe(false);
  });
});

describe("SettingsStore modal flags", () => {
  it("openModal sets the section and opens; closeModal closes", () => {
    const { store } = make();
    expect(store.open()).toBe(false);
    store.openModal("perms");
    expect(store.open()).toBe(true);
    expect(store.openSection()).toBe("perms");
    store.closeModal();
    expect(store.open()).toBe(false);
  });
});
