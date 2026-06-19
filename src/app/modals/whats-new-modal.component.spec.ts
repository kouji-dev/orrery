import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { WhatsNewModalComponent } from "./whats-new-modal.component";
import { SettingsStore } from "../settings/settings.store";
import { VersionService } from "../shared/version.service";
import { ChangelogService, ChangelogRelease } from "../updater/changelog.service";
import { IconComponent } from "../shared/icon.component";

// Stub IconComponent (signal input.required → NG0950 under JIT).
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

function makeStore() {
  return {
    whatsNewOpen: signal(false),
    updateKnown: signal(false),
    settings: signal({ channel: "stable" } as unknown as Record<string, unknown>),
    closeWhatsNew: vi.fn(),
  };
}

function makeChangelog() {
  const data = signal<ChangelogRelease[]>([]);
  return {
    data,
    loading: signal(false),
    error: signal(false),
    load: vi.fn(),
    since: (_cur: string | null | undefined) => data(), // reads the signal → computed reacts
  };
}

describe("WhatsNewModalComponent", () => {
  let store: ReturnType<typeof makeStore>;
  let changelog: ReturnType<typeof makeChangelog>;

  beforeEach(() => {
    store = makeStore();
    changelog = makeChangelog();
    TestBed.configureTestingModule({
      imports: [WhatsNewModalComponent],
      providers: [
        provideZonelessChangeDetection(),
        { provide: SettingsStore, useValue: store },
        { provide: ChangelogService, useValue: changelog },
        { provide: VersionService, useValue: { version: () => "0.4.1" } },
      ],
    });
    TestBed.overrideComponent(WhatsNewModalComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
  });
  afterEach(() => TestBed.resetTestingModule());

  it("renders nothing while closed and loads the changelog on open", () => {
    const f = TestBed.createComponent(WhatsNewModalComponent);
    f.detectChanges();
    expect(f.nativeElement.querySelector(".wn-modal")).toBeNull();
    expect(changelog.load).not.toHaveBeenCalled();

    store.whatsNewOpen.set(true);
    f.detectChanges();
    expect(changelog.load).toHaveBeenCalled();
  });

  it("renders every release section with typed-commit chips and the count", () => {
    const f = TestBed.createComponent(WhatsNewModalComponent);
    changelog.data.set([
      { tag: "v0.9.4", channel: "beta", date: "June 18, 2026", summary: "newer things",
        commits: [{ type: "feat", scope: "updater", msg: "what's-new digest" }, { type: "ci", scope: "x", msg: "pipeline tweak" }] },
      { tag: "v0.9.3", channel: "beta", date: "June 3, 2026", summary: "older things",
        commits: [{ type: "fix", msg: "clamp the bar" }] },
    ]);
    store.whatsNewOpen.set(true);
    f.detectChanges();
    const text = f.nativeElement.textContent as string;
    expect(text).toContain("v0.9.4");
    expect(text).toContain("v0.9.3");
    expect(text).toContain("what's-new digest");
    expect(text).toContain("clamp the bar");
    expect(text).toContain("feat");
    expect(text).toContain("fix");
    expect(text).toContain("ci"); // unknown type still rendered (neutral chip)
    expect(text).toContain("newer things");
    expect(text).toContain("from v0.4.1 · 2 releases");
    // two release sections
    expect(f.nativeElement.querySelectorAll("section.wn-rel").length).toBe(2);

    f.nativeElement.querySelector("button.primary")!.click(); // Continue
    expect(store.closeWhatsNew).toHaveBeenCalled();
  });

  it("shows a graceful state when the changelog fails to load", () => {
    const f = TestBed.createComponent(WhatsNewModalComponent);
    changelog.error.set(true);
    store.whatsNewOpen.set(true);
    f.detectChanges();
    expect(f.nativeElement.textContent).toContain("Couldn't load the changelog");
  });
});
