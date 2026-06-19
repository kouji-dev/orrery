import { Injector, runInInjectionContext } from "@angular/core";
import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { ChangelogService, cmpVer, ChangelogRelease } from "./changelog.service";

describe("cmpVer", () => {
  it("compares semver-ish tags with/without a v prefix", () => {
    expect(cmpVer("v0.9.4", "0.9.3")).toBe(1);
    expect(cmpVer("0.9.3", "v0.9.4")).toBe(-1);
    expect(cmpVer("v1.0.0", "v1.0.0")).toBe(0);
    expect(cmpVer("v0.10.0", "v0.9.9")).toBe(1); // numeric, not lexical
  });
});

function rel(tag: string): ChangelogRelease {
  return { tag, channel: "beta", date: "d", summary: "s", commits: [] };
}

function make(): ChangelogService {
  const injector = Injector.create({ providers: [] });
  return runInInjectionContext(injector, () => new ChangelogService());
}

describe("ChangelogService.since", () => {
  it("returns releases strictly newer than current, newest-first", () => {
    const svc = make();
    svc.releases.set([rel("v0.9.4"), rel("v0.9.3"), rel("v0.9.2"), rel("v0.9.0")]);
    expect(svc.since("0.9.2").map((r) => r.tag)).toEqual(["v0.9.4", "v0.9.3"]);
  });

  it("falls back to the single newest release when up to date", () => {
    const svc = make();
    svc.releases.set([rel("v0.9.4"), rel("v0.9.3")]);
    expect(svc.since("0.9.4").map((r) => r.tag)).toEqual(["v0.9.4"]);
  });

  it("returns [] when nothing is loaded", () => {
    expect(make().since("0.9.0")).toEqual([]);
  });
});

describe("ChangelogService.load", () => {
  const realFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = realFetch;
  });

  it("caches a successful fetch and only requests once", async () => {
    const json = [rel("v0.9.4")];
    const spy = vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(json) });
    globalThis.fetch = spy as unknown as typeof fetch;
    const svc = make();
    await svc.load();
    await svc.load(); // cached — no second request
    expect(spy).toHaveBeenCalledTimes(1);
    expect(svc.releases()?.map((r) => r.tag)).toEqual(["v0.9.4"]);
    expect(svc.error()).toBe(false);
  });

  it("sets error on a failed fetch (and leaves releases unloaded)", async () => {
    globalThis.fetch = vi.fn().mockRejectedValue(new Error("offline")) as unknown as typeof fetch;
    const svc = make();
    await svc.load();
    expect(svc.error()).toBe(true);
    expect(svc.releases()).toBeNull();
  });
});
