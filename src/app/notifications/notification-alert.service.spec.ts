import { Injector, runInInjectionContext, signal } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, MockInstance, vi } from "vitest";
import { Settings } from "../models";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { NotificationStore } from "../stores/notifications.store";
import { UiStore } from "../ui/ui.store";
import { NotificationAlertService, RaiseInput } from "./notification-alert.service";

// ---- module mocks (hoisted) ----
const plugin = vi.hoisted(() => ({
  sendNotification: vi.fn(),
  isPermissionGranted: vi.fn(async () => true),
  requestPermission: vi.fn(async () => "granted" as NotificationPermission),
  onAction: vi.fn(async (_cb: (n: { extra?: Record<string, unknown> }) => void) => ({})),
}));
vi.mock("@tauri-apps/plugin-notification", () => plugin);

const sound = vi.hoisted(() => ({ playNotificationSound: vi.fn() }));
vi.mock("./notification-sound", () => ({
  playNotificationSound: sound.playNotificationSound,
  resetNotificationAudio: vi.fn(),
  primeNotificationAudioOnGesture: vi.fn(),
}));

const win = vi.hoisted(() => ({
  unminimize: vi.fn(async () => {}),
  setFocus: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => win }));

// ---- helpers ----
let hasFocus: MockInstance;
let ui: { openAgent: ReturnType<typeof vi.fn>; flash: ReturnType<typeof vi.fn> };

function setup(overrides: Partial<Settings> = {}): {
  svc: NotificationAlertService;
  store: NotificationStore;
} {
  const settings = signal<Settings>({ ...settingsDefaults(), ...overrides });
  ui = { openAgent: vi.fn(), flash: vi.fn() };
  const store = new NotificationStore(); // real in-memory feed — gating asserts on it
  const injector = Injector.create({
    providers: [
      { provide: SettingsStore, useValue: { settings } },
      { provide: UiStore, useValue: ui },
      { provide: NotificationStore, useValue: store },
    ],
  });
  const svc = runInInjectionContext(injector, () => new NotificationAlertService());
  return { svc, store };
}

function input(kind: "done" | "question" | "permission", agentId = "a1"): RaiseInput {
  return { agentId, agentName: "ag", kind, title: `${kind} title`, detail: "detail" };
}

/** Let the service's dynamic-import + permission promise chain settle. A few
 *  macrotask turns — vitest's module runner can resolve a second dynamic
 *  import of the same mocked module on a later tick. */
const flush = async () => {
  for (let i = 0; i < 5; i++) await new Promise<void>((r) => setTimeout(r, 0));
};

beforeEach(() => {
  vi.clearAllMocks();
  plugin.isPermissionGranted.mockResolvedValue(true);
  hasFocus = vi.spyOn(document, "hasFocus").mockReturnValue(false); // default: app unfocused
});
afterEach(() => {
  hasFocus.mockRestore();
});

describe("NotificationAlertService — per-event gating", () => {
  it.each([
    ["done", "finished"],
    ["question", "question"],
    ["permission", "permission"],
  ] as const)("events.%s toggle OFF drops a %s raise entirely (not even in-app)", async (kind, key) => {
    const { svc, store } = setup({ events: { ...settingsDefaults().events, [key]: false } });
    expect(svc.raise(input(kind))).toBeNull();
    expect(store.all()).toHaveLength(0);
    await flush();
    expect(plugin.sendNotification).not.toHaveBeenCalled();
    expect(sound.playNotificationSound).not.toHaveBeenCalled();
  });

  it("toggle ON raises in-app (and only gates its own kind)", () => {
    const { svc, store } = setup({ events: { ...settingsDefaults().events, finished: false } });
    expect(svc.raise(input("question"))).not.toBeNull();
    expect(svc.raise(input("done"))).toBeNull();
    expect(store.all()).toHaveLength(1);
    expect(store.all()[0].kind).toBe("question");
  });

  it("de-duped raise (same agent+kind pending) returns null and re-fires nothing", async () => {
    const { svc } = setup();
    expect(svc.raise(input("permission"))).not.toBeNull();
    expect(svc.raise(input("permission"))).toBeNull();
    await flush();
    expect(plugin.sendNotification).toHaveBeenCalledTimes(1);
    expect(sound.playNotificationSound).toHaveBeenCalledTimes(1);
  });
});

describe("NotificationAlertService — master osNotifications + native toasts", () => {
  it("master OFF: in-app only — no toast, no sound", async () => {
    const { svc, store } = setup({ osNotifications: false });
    expect(svc.raise(input("done"))).not.toBeNull();
    expect(store.pending()).toHaveLength(1);
    await flush();
    expect(plugin.sendNotification).not.toHaveBeenCalled();
    expect(sound.playNotificationSound).not.toHaveBeenCalled();
  });

  it("master ON + unfocused: fires the native toast (title/body/agent extra)", async () => {
    const { svc } = setup();
    svc.raise(input("done", "a9"));
    await flush();
    expect(plugin.sendNotification).toHaveBeenCalledWith({
      title: "done title",
      body: "detail",
      extra: { agentId: "a9" },
    });
  });

  it("focused window: skips the toast for non-permission events (in-app feed suffices)", async () => {
    hasFocus.mockReturnValue(true);
    const { svc, store } = setup();
    svc.raise(input("done"));
    await flush();
    expect(plugin.sendNotification).not.toHaveBeenCalled();
    expect(store.pending()).toHaveLength(1); // still raised in-app
  });

  it("focused + permission + remoteApproval ON: toast fires anyway", async () => {
    hasFocus.mockReturnValue(true);
    const { svc } = setup({ remoteApproval: true });
    svc.raise(input("permission"));
    await flush();
    expect(plugin.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("focused + permission + remoteApproval OFF: toast skipped like other events", async () => {
    hasFocus.mockReturnValue(true);
    const { svc } = setup({ remoteApproval: false });
    svc.raise(input("permission"));
    await flush();
    expect(plugin.sendNotification).not.toHaveBeenCalled();
  });

  it("requests OS permission lazily, ONCE, and caches the answer", async () => {
    plugin.isPermissionGranted.mockResolvedValue(false);
    const { svc } = setup();
    svc.raise(input("done"));
    svc.raise(input("question"));
    await flush();
    expect(plugin.requestPermission).toHaveBeenCalledTimes(1);
    expect(plugin.sendNotification).toHaveBeenCalledTimes(2);
  });

  it("permission denied: no toast is sent", async () => {
    plugin.isPermissionGranted.mockResolvedValue(false);
    plugin.requestPermission.mockResolvedValue("denied");
    const { svc } = setup();
    svc.raise(input("done"));
    await flush();
    expect(plugin.sendNotification).not.toHaveBeenCalled();
  });

  it("toast action (where the plugin delivers it) focuses the window and opens the agent terminal", async () => {
    const { svc } = setup();
    svc.raise(input("permission", "a3"));
    await flush();
    expect(plugin.onAction).toHaveBeenCalledTimes(1);
    const cb = plugin.onAction.mock.calls[0][0];
    cb({ extra: { agentId: "a3" } });
    await flush();
    expect(win.setFocus).toHaveBeenCalled();
    expect(ui.openAgent).toHaveBeenCalledWith("a3", "terminal");
  });
});

describe("NotificationAlertService — sound cues", () => {
  it("plays the configured cue at the configured volume when raised (sound + master on)", () => {
    const { svc } = setup({ soundName: "Glass", volume: 40 });
    svc.raise(input("done"));
    expect(sound.playNotificationSound).toHaveBeenCalledTimes(1);
    expect(sound.playNotificationSound).toHaveBeenCalledWith("Glass", 40);
  });

  it("sound OFF: cue not played (toast unaffected)", async () => {
    const { svc } = setup({ sound: false });
    svc.raise(input("done"));
    await flush();
    expect(sound.playNotificationSound).not.toHaveBeenCalled();
    expect(plugin.sendNotification).toHaveBeenCalledTimes(1);
  });

  it("master OFF gates the cue too", () => {
    const { svc } = setup({ osNotifications: false });
    svc.raise(input("done"));
    expect(sound.playNotificationSound).not.toHaveBeenCalled();
  });
});
