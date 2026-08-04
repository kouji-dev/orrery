import { Component, provideZonelessChangeDetection } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { Settings, ToolDetection } from "../models";
import { RuntimeRowComponent } from "./runtime-row.component";
import { SettingsStore } from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { VersionService } from "../shared/version.service";
import { UiStore } from "../ui/ui.store";
import {
  ModelComboComponent,
  SetRowComponent,
  SetSegComponent,
  SettingsModalComponent,
} from "./settings-modal.component";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

afterEach(() => TestBed.resetTestingModule());

// The shared icon/badge components use signal inputs, which raw vitest JIT
// cannot wire (NG0950) — same-selector stubs keep the modal's own templates
// (rows, segs, toggles, combo) fully exercised.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}
@Component({ selector: "app-tool-badge", template: "", inputs: ["tool", "size"] })
class ToolBadgeStub {}

interface Setup {
  fixture: ComponentFixture<SettingsModalComponent>;
  el: HTMLElement;
  store: SettingsStore;
  pickDirectory: ReturnType<typeof vi.fn>;
}

// claude/codex/cursor detected; gemini missing — mirrors the design fixture.
const DETECTIONS: Record<string, ToolDetection> = {
  claude: { id: "claude", status: "ok", available: true, path: "/usr/local/bin/claude", version: "1.4.2", source: "path", reason: null, shim: false },
  codex: { id: "codex", status: "ok", available: true, path: "/usr/local/bin/codex", version: "0.31.0", source: "path", reason: null, shim: false },
  cursor: { id: "cursor", status: "ok", available: true, path: "/usr/local/bin/cursor-agent", version: null, source: "path", reason: null, shim: false },
  gemini: { id: "gemini", status: "missing", available: false, path: null, version: null, source: null, reason: null, shim: false },
};

async function setup(stored: Partial<Settings> = {}): Promise<Setup> {
  const pickDirectory = vi.fn(async () => "C:/picked/worktrees");
  const bridge: Bridge = {
    invoke: vi.fn(async (cmd: string) => (cmd === "settings_get" ? stored : null)) as Bridge["invoke"],
    on: async () => () => {},
    pickDirectory,
    pickFile: async () => null,
  };
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash: vi.fn() } },
      {
        provide: AgentRuntimeService,
        useValue: {
          toolAvailable: (id: string) => id !== "gemini",
          detection: (id: string) => DETECTIONS[id] ?? null,
          refreshDetections: async () => {},
          verifyToolPath: vi.fn(),
          setDetection: vi.fn(),
        },
      },
      { provide: VersionService, useValue: { version: () => "0.9.2" } },
    ],
  });
  TestBed.overrideComponent(SettingsModalComponent, {
    remove: { imports: [IconComponent, ToolBadgeComponent] },
    add: { imports: [IconStub, ToolBadgeStub] },
  });
  TestBed.overrideComponent(RuntimeRowComponent, {
    remove: { imports: [IconComponent, ToolBadgeComponent] },
    add: { imports: [IconStub, ToolBadgeStub] },
  });
  for (const cmp of [SetSegComponent, ModelComboComponent, SetRowComponent]) {
    TestBed.overrideComponent(cmp, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
  }
  const store = TestBed.inject(SettingsStore);
  await store.ready(); // settle the settings_get load before interacting
  store.openModal();
  const fixture = TestBed.createComponent(SettingsModalComponent);
  fixture.detectChanges();
  return { fixture, el: fixture.nativeElement as HTMLElement, store, pickDirectory };
}

const click = (fixture: ComponentFixture<unknown>, target: Element | null) => {
  (target as HTMLElement).click();
  fixture.detectChanges();
};
const navTo = (s: Setup, label: string) =>
  click(s.fixture, Array.from(s.el.querySelectorAll(".set-nav-item")).find((b) => b.textContent?.includes(label)) ?? null);
const byText = (root: HTMLElement, selector: string, text: string) =>
  Array.from(root.querySelectorAll(selector)).find((b) => b.textContent?.includes(text)) ?? null;

describe("SettingsModal sections render", () => {
  it("opens on Updates: nav, header, channel/policy segs, version chip", async () => {
    const s = await setup();
    expect(s.el.querySelectorAll(".set-nav-item")).toHaveLength(4);
    expect(s.el.querySelector(".set-head .ht")?.textContent).toContain("Updates");
    expect(s.el.textContent).toContain("Install policy");
    expect(s.el.querySelector(".set-vchip")?.textContent).toContain("v0.9.2");
    expect(s.el.querySelector(".set-vchip")?.textContent).toContain("STABLE");
    // no update known → no nav dot, no update card
    expect(s.el.querySelector(".set-nav-dot")).toBeNull();
    expect(s.el.querySelector(".set-upd")).toBeNull();
  });

  it("Agent defaults: tool grid shows version, undetected tool dimmed 'not found'", async () => {
    const s = await setup();
    navTo(s, "Agent defaults");
    expect(s.el.querySelector(".set-head .ht")?.textContent).toContain("Agent defaults");
    const tools = Array.from(s.el.querySelectorAll<HTMLButtonElement>(".set-tool"));
    expect(tools).toHaveLength(4);
    const gemini = tools[3];
    expect(gemini.classList.contains("off")).toBe(true);
    // tiles are now selectable even when not runnable (you pick one to fix its path)
    expect(gemini.textContent).toContain("not found");
    expect(tools[0].textContent).toContain("detected");
    expect(tools[0].textContent).toContain("v1.4.2"); // version surfaced
    // the Executable runtime row renders for the selected tool with its path chip
    const chip = s.el.querySelector(".set-rt-pathchip .pt");
    expect(chip?.textContent).toContain("/usr/local/bin/claude");
    // branch template live preview
    expect(s.el.querySelector(".set-preview b")?.textContent).toContain("agent/fix-login");
  });

  it("Permissions: one policy row per DETECTED tool + remote approval", async () => {
    const s = await setup();
    navTo(s, "Permissions");
    expect(s.el.querySelector(".set-head .ht")?.textContent).toContain("Permissions & safety");
    const segs = s.el.querySelectorAll(".set-seg");
    expect(segs).toHaveLength(3); // claude/codex/cursor — gemini is undetected
    expect(s.el.textContent).toContain("Approve from notifications");
  });

  it("Notifications: master toggle gates (disables, not hides) the event + sound rows", async () => {
    const s = await setup();
    navTo(s, "Notifications");
    expect(s.el.textContent).toContain("Native OS notifications");
    expect(s.el.querySelectorAll(".set-row.dis")).toHaveLength(0);

    click(s.fixture, s.el.querySelector(".set-tgl")); // master off
    expect(s.store.settings().osNotifications).toBe(false);
    // 4 event rows + Play sound + Cue & volume stay rendered but disabled
    expect(s.el.querySelectorAll(".set-row.dis")).toHaveLength(6);
  });
});

describe("SettingsModal dirty rows + footer", () => {
  it("a changed row grows a reset pill that restores the default", async () => {
    const s = await setup();
    expect(s.el.querySelector(".set-reset")).toBeNull();
    expect(s.el.textContent).toContain("Changes apply instantly");

    click(s.fixture, byText(s.el, ".set-seg button", "beta"));
    expect(s.store.settings().channel).toBe("beta");
    expect(s.el.querySelector(".set-reset")).not.toBeNull();
    expect(s.el.querySelector(".set-warn")).not.toBeNull(); // beta warn chip
    expect(byText(s.el, "button.reset-all", "Reset all to defaults")).not.toBeNull();

    click(s.fixture, s.el.querySelector(".set-reset"));
    expect(s.store.settings().channel).toBe("stable");
    expect(s.el.querySelector(".set-reset")).toBeNull();
    expect(s.el.querySelector(".set-warn")).toBeNull();
    expect(s.el.textContent).toContain("Changes apply instantly");
  });

  it("Reset all restores every default", async () => {
    const s = await setup({ channel: "beta", autoResume: true, volume: 10 });
    expect(s.store.anyDirty()).toBe(true);
    click(s.fixture, byText(s.el, "button.reset-all", "Reset all"));
    expect(s.store.anyDirty()).toBe(false);
    expect(s.store.settings().channel).toBe("stable");
    expect(s.store.settings().volume).toBe(70);
  });
});

describe("SettingsModal danger confirm (Everything)", () => {
  it("selecting Everything applies it and opens the confirm; Cancel reverts", async () => {
    const s = await setup();
    navTo(s, "Permissions");
    const firstSeg = s.el.querySelector(".set-seg")!; // claude's row
    click(s.fixture, byText(firstSeg as HTMLElement, "button", "Everything"));

    expect(s.store.settings().autoApprove["claude"]).toBe("everything"); // instant-apply
    expect(s.el.querySelector(".set-danger")).not.toBeNull();

    click(s.fixture, byText(s.el.querySelector(".set-danger") as HTMLElement, "button", "Cancel"));
    expect(s.store.settings().autoApprove["claude"]).toBeUndefined(); // back to implicit "off"
    expect(s.el.querySelector(".set-danger")).toBeNull();
  });

  it("Enable “Everything” confirms and keeps the policy", async () => {
    const s = await setup();
    navTo(s, "Permissions");
    click(s.fixture, byText(s.el.querySelector(".set-seg") as HTMLElement, "button", "Everything"));
    click(s.fixture, s.el.querySelector(".set-btn-danger"));
    expect(s.el.querySelector(".set-danger")).toBeNull();
    expect(s.store.settings().autoApprove["claude"]).toBe("everything");
  });
});

describe("SettingsModal updates card + nav dot", () => {
  it("shows the card and the amber dot only once an update is known; Later hides the card", async () => {
    const s = await setup();
    s.store.noteUpdate({ version: "9.9.9", date: "Jun 9, 2026" });
    s.fixture.detectChanges();

    expect(s.el.querySelector(".set-nav-dot")).not.toBeNull();
    const card = s.el.querySelector(".set-upd")!;
    expect(card.textContent).toContain("v9.9.9");
    expect(card.textContent).toContain("released Jun 9, 2026");
    expect(card.textContent).toContain("Install & relaunch");
    expect(card.textContent).not.toContain("MB"); // no size — by design

    click(s.fixture, byText(card as HTMLElement, "button", "Later"));
    expect(s.el.querySelector(".set-upd")).toBeNull();
    expect(s.el.querySelector(".set-nav-dot")).not.toBeNull(); // dot survives Later
  });
});

describe("SettingsModal combo + Esc + browse", () => {
  it("Esc closes the model combo first, then the modal; backdrop click closes", async () => {
    const s = await setup();
    navTo(s, "Agent defaults");
    click(s.fixture, s.el.querySelector(".set-combo-btn"));
    expect(s.el.querySelector(".set-combo-pop")).not.toBeNull();

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    s.fixture.detectChanges();
    expect(s.el.querySelector(".set-combo-pop")).toBeNull(); // combo closed…
    expect(s.store.open()).toBe(true); // …modal still open

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(s.store.open()).toBe(false);

    s.store.openModal();
    (s.el.querySelector(".set-backdrop") as HTMLElement).dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true }),
    );
    expect(s.store.open()).toBe(false);
  });

  it("custom model: typing + Enter applies the override and tags it custom", async () => {
    const s = await setup();
    navTo(s, "Agent defaults");
    click(s.fixture, s.el.querySelector(".set-combo-btn"));
    const input = s.el.querySelector<HTMLInputElement>(".set-combo-field input")!;
    input.value = "opus-secret-preview";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    s.fixture.detectChanges();

    expect(s.store.settings().toolModel["claude"]).toBe("opus-secret-preview");
    expect(s.el.querySelector(".set-combo-pop")).toBeNull(); // commit closes the popover
    expect(s.el.querySelector(".set-combo-btn .tag")?.textContent).toContain("custom");
  });

  it("Browse picks a directory into worktreeRoot", async () => {
    const s = await setup();
    navTo(s, "Agent defaults");
    click(s.fixture, byText(s.el, "button", "Browse"));
    await s.fixture.whenStable();
    expect(s.pickDirectory).toHaveBeenCalled();
    expect(s.store.settings().worktreeRoot).toBe("C:/picked/worktrees");
  });
});
