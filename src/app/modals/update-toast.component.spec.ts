import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { UpdateToastComponent } from "./update-toast.component";
import { SettingsStore } from "../settings/settings.store";
import { VersionService } from "../shared/version.service";
import { IconComponent } from "../shared/icon.component";
import { UpdateInfo } from "../models";

// Stub IconComponent (signal input.required → NG0950 under JIT).
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

function makeStore() {
  return {
    open: signal(false),
    updateCard: signal<UpdateInfo | null>(null),
    installing: signal(false),
    installPhase: signal<string | null>(null),
    installProgress: signal(0),
    install: vi.fn(),
    dismissUpdate: vi.fn(),
    openWhatsNew: vi.fn(),
  };
}

function btn(el: HTMLElement, text: string): HTMLButtonElement {
  return Array.from(el.querySelectorAll("button")).find((b) => (b.textContent ?? "").includes(text)) as HTMLButtonElement;
}

describe("UpdateToastComponent", () => {
  let store: ReturnType<typeof makeStore>;
  beforeEach(() => {
    store = makeStore();
    TestBed.configureTestingModule({
      imports: [UpdateToastComponent],
      providers: [
        provideZonelessChangeDetection(),
        { provide: SettingsStore, useValue: store },
        { provide: VersionService, useValue: { version: () => "0.4.1" } },
      ],
    });
    TestBed.overrideComponent(UpdateToastComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
  });
  afterEach(() => TestBed.resetTestingModule());

  it("renders nothing when no update is known", () => {
    const f = TestBed.createComponent(UpdateToastComponent);
    f.detectChanges();
    expect(f.nativeElement.querySelector(".ut")).toBeNull();
  });

  it("shows the version + from-version and wires the three actions", () => {
    const f = TestBed.createComponent(UpdateToastComponent);
    store.updateCard.set({ version: "0.9.4", date: "Jun 18, 2026" });
    f.detectChanges();
    const text = f.nativeElement.textContent as string;
    expect(text).toContain("Update available");
    expect(text).toContain("v0.9.4");
    expect(text).toContain("from v0.4.1");

    btn(f.nativeElement, "What's new").click();
    expect(store.openWhatsNew).toHaveBeenCalled();
    btn(f.nativeElement, "Later").click();
    expect(store.dismissUpdate).toHaveBeenCalled();
    btn(f.nativeElement, "Install").click();
    expect(store.install).toHaveBeenCalled();
  });

  it("hides while the settings modal is open (the card shows it there)", () => {
    const f = TestBed.createComponent(UpdateToastComponent);
    store.updateCard.set({ version: "0.9.4" });
    store.open.set(true);
    f.detectChanges();
    expect(f.nativeElement.querySelector(".ut")).toBeNull();
  });
});
