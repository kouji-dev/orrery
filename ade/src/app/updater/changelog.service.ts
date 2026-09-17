import { Injectable, signal } from "@angular/core";
import { CHANGELOG_JSON_URL } from "../shared/links";

export interface ChangelogCommit {
  type: string;
  scope?: string;
  msg: string;
  hash?: string;
  by?: string;
}
export interface ChangelogRelease {
  tag: string;
  channel?: string;
  date?: string;
  summary?: string;
  commits: ChangelogCommit[];
}

/** semver-ish compare on "v0.9.4" / "0.9.4": 1 if a>b, -1 if a<b, 0 equal. */
export function cmpVer(a: string, b: string): number {
  const pa = String(a).replace(/^v/, "").split(".").map(Number);
  const pb = String(b).replace(/^v/, "").split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d) return d > 0 ? 1 : -1;
  }
  return 0;
}

/**
 * Loads the single-source release history (the deployed landing/changelog.json)
 * and exposes the slice the user hasn't seen — every release strictly newer than
 * their build — for the in-app "What's new" digest. Fetched once and cached;
 * any failure degrades to the GitHub releases link in the dialog.
 */
@Injectable({ providedIn: "root" })
export class ChangelogService {
  /** null = not loaded yet; [] = loaded but empty/failed. */
  readonly releases = signal<ChangelogRelease[] | null>(null);
  readonly loading = signal(false);
  readonly error = signal(false);
  private loaded = false;

  /** Fetch changelog.json once (cached across opens). Best-effort. */
  async load(): Promise<void> {
    if (this.loaded || this.loading()) return;
    this.loading.set(true);
    this.error.set(false);
    try {
      const res = await fetch(CHANGELOG_JSON_URL, { headers: { Accept: "application/json" } });
      if (!res.ok) throw new Error(String(res.status));
      const data: unknown = await res.json();
      this.releases.set(Array.isArray(data) ? (data as ChangelogRelease[]) : []);
      this.loaded = true;
    } catch {
      this.error.set(true);
    } finally {
      this.loading.set(false);
    }
  }

  /** Releases strictly newer than `current` (newest-first). Falls back to the
   *  single newest release when none are newer, so the digest is never empty
   *  (e.g. opened from the footer version while up to date). */
  since(current: string | null | undefined): ChangelogRelease[] {
    const rels = this.releases() ?? [];
    if (!rels.length) return [];
    if (!current) return rels;
    const newer = rels.filter((r) => cmpVer(r.tag, current) > 0);
    return newer.length ? newer : rels.slice(0, 1);
  }
}
