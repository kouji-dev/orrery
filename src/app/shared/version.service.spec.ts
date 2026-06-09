import { isDevMode } from "@angular/core";
import { getVersion } from "@tauri-apps/api/app";
import { afterEach, describe, expect, it, vi } from "vitest";
import { VersionService } from "./version.service";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));
// Override only isDevMode; keep the rest of @angular/core real.
vi.mock("@angular/core", async (orig) => ({ ...(await orig<object>()), isDevMode: vi.fn() }));

afterEach(() => {
  vi.mocked(isDevMode).mockReset();
  vi.mocked(getVersion).mockReset();
});

describe("VersionService", () => {
  it("dev build → DEV tag in amber", () => {
    vi.mocked(isDevMode).mockReturnValue(true);
    vi.mocked(getVersion).mockResolvedValue("0.1.1");
    const s = new VersionService();
    expect(s.isDev).toBe(true);
    expect(s.label).toBe("DEV");
    expect(s.color).toBe("#f5c451");
  });

  it("production build → BETA tag in accent-2", () => {
    vi.mocked(isDevMode).mockReturnValue(false);
    vi.mocked(getVersion).mockResolvedValue("0.1.1");
    const s = new VersionService();
    expect(s.isDev).toBe(false);
    expect(s.label).toBe("BETA");
    expect(s.color).toBe("var(--accent-2)");
  });

  it("populates the version from Tauri getVersion", async () => {
    vi.mocked(isDevMode).mockReturnValue(false);
    vi.mocked(getVersion).mockResolvedValue("9.9.9");
    const s = new VersionService();
    await vi.waitFor(() => expect(s.version()).toBe("9.9.9"));
  });

  it("leaves the version empty when Tauri is unavailable", async () => {
    vi.mocked(isDevMode).mockReturnValue(true);
    vi.mocked(getVersion).mockRejectedValue(new Error("no tauri"));
    const s = new VersionService();
    await Promise.resolve();
    await Promise.resolve();
    expect(s.version()).toBe("");
  });
});
