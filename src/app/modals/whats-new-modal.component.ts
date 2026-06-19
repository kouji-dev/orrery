import { ChangeDetectionStrategy, Component, computed, effect, HostListener, inject } from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { SettingsStore } from "../settings/settings.store";
import { VersionService } from "../shared/version.service";
import { ChangelogService, ChangelogCommit } from "../updater/changelog.service";
import { RELEASES_URL } from "../shared/links";

/**
 * In-app "What's new" digest. Shows EVERY release strictly newer than the user's
 * build (newest first) so they catch up on everything since their version — not
 * just the latest tag — sourced from the single-source landing/changelog.json
 * via {@link ChangelogService}. Opened from Settings ("Read release notes"), the
 * update toast, and the version chips in the top bar / status bar.
 *
 * "View full changelog" opens the GitHub releases page in the browser (window.open
 * is blocked inside the Tauri webview). Falls back to that link if the fetch fails.
 */
@Component({
  selector: "app-whats-new-modal",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @if (store.whatsNewOpen()) {
      <div class="wn-backdrop" (mousedown)="store.closeWhatsNew()">
        <div class="wn-modal" (mousedown)="$event.stopPropagation()" role="dialog" aria-label="What's new">
          <div class="wn-hero">
            <button class="wn-x" (click)="store.closeWhatsNew()" aria-label="Close"><app-icon name="x" size="sm" /></button>
            <div class="wn-eyebrow"><span class="d"></span>{{ store.updateKnown() ? 'Update available' : "What's new" }}</div>
            <div class="wn-h1">What's new in Orrery</div>
            <div class="wn-verline">
              <span class="wn-ver tnum">{{ headTag() }}</span>
              <span class="wn-badge">{{ channel() }}</span>
              <span class="wn-from tnum">from v{{ version.version() || '—' }}{{ fromSuffix() }}</span>
            </div>
          </div>

          <div class="wn-body scroll-y">
            @if (changelog.loading() && !releases().length) {
              <div class="wn-state">Loading changelog…</div>
            } @else if (!releases().length) {
              <div class="wn-state">
                Couldn't load the changelog{{ changelog.error() ? ' — check your connection' : '' }}.
                Open the full changelog for the complete release history.
              </div>
            } @else {
              @for (rel of releases(); track rel.tag) {
                <section class="wn-rel">
                  <div class="wn-rel-head">
                    <span class="wn-rel-tag tnum">{{ rel.tag }}</span>
                    @if (rel.channel) { <span class="wn-badge">{{ rel.channel === 'beta' ? 'BETA' : rel.channel === 'stable' ? 'STABLE' : 'DEV' }}</span> }
                    @if (rel.date) { <span class="wn-rel-date tnum">{{ rel.date }}</span> }
                  </div>
                  @if (rel.summary) { <p class="wn-rel-sum">{{ rel.summary }}</p> }
                  @for (c of sorted(rel.commits); track $index) {
                    <div class="wn-commit">
                      <span class="wn-type" [class]="'wn-type ' + typeClass(c.type)">{{ c.type }}</span>
                      <span class="wn-msg">
                        @if (c.scope) { <span class="scope">{{ c.scope }}: </span> }{{ c.msg }}
                        @if (c.by) { <span class="by">{{ c.by }}</span> }
                      </span>
                    </div>
                  }
                </section>
              }
            }
          </div>

          <div class="wn-foot">
            <button class="wn-link" (click)="openChangelog()">
              <app-icon name="file" size="sm" />View full changelog<app-icon name="ext" size="sm" />
            </button>
            <span class="wn-sp"></span>
            <button class="btn primary" (click)="store.closeWhatsNew()"><app-icon name="check" size="sm" />Continue</button>
          </div>
        </div>
      </div>
    }
  `,
  styles: [
    `
      .wn-backdrop {
        position: fixed; inset: 0; z-index: 85; display: grid; place-items: center; padding: var(--sp-8);
        background: rgba(4, 5, 9, 0.6); backdrop-filter: blur(5px);
      }
      .wn-modal {
        --feat: #22d3ee; --fix: #34e0a1; --perf: #c084fc; --refactor: #a855f7; --chore: #8b94a8;
        width: 544px; max-width: calc(100vw - 48px); max-height: 86vh; display: flex; flex-direction: column;
        background: var(--panel); border: 1px solid var(--hair-2); border-radius: var(--r-lg); overflow: hidden;
        box-shadow: var(--shadow); color: var(--ink); font-family: var(--font-mono);
      }
      .wn-hero {
        position: relative; flex: none; padding: var(--sp-7) var(--sp-7) var(--sp-6);
        border-bottom: 1px solid var(--hair); overflow: hidden;
      }
      .wn-hero::before {
        content: ""; position: absolute; inset: 0; pointer-events: none;
        background: radial-gradient(120% 92% at 16% -16%, color-mix(in oklch, var(--accent), transparent 80%), transparent 66%);
      }
      .wn-hero::after {
        content: ""; position: absolute; left: 0; right: 0; top: 0; height: 2px;
        background: linear-gradient(90deg, var(--accent-2), var(--accent)); opacity: 0.85;
      }
      .wn-x {
        position: absolute; top: var(--sp-5); right: var(--sp-5); width: 28px; height: 28px; border-radius: var(--r-sm);
        border: 1px solid transparent; background: transparent; color: var(--ink-3); cursor: pointer; display: grid; place-items: center;
      }
      .wn-x:hover { background: var(--panel-3); color: var(--ink); border-color: var(--hair); }
      .wn-eyebrow {
        position: relative; display: inline-flex; align-items: center; gap: var(--sp-3); font-size: var(--fs-2xs);
        letter-spacing: 0.18em; text-transform: uppercase; color: var(--ink-3);
      }
      .wn-eyebrow .d { width: 6px; height: 6px; border-radius: 50%; background: var(--accent-2); }
      .wn-h1 { position: relative; font-family: var(--font-disp); font-weight: 600; font-size: var(--fs-xl); letter-spacing: -0.02em; margin-top: var(--sp-3); }
      .wn-verline { position: relative; display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap; margin-top: var(--sp-4); }
      .wn-ver { font-family: var(--font-disp); font-weight: 600; font-size: var(--fs-md); color: var(--accent); }
      .wn-badge {
        font-size: var(--fs-3xs); font-weight: 700; letter-spacing: 0.12em; padding: 2px 7px; border-radius: 999px;
        color: var(--accent-2); border: 1px solid color-mix(in oklch, var(--accent-2), transparent 56%);
        background: color-mix(in oklch, var(--accent-2), transparent 88%);
      }
      .wn-from { font-size: var(--fs-xs); color: var(--ink-4); }
      .wn-body { flex: 1; min-height: 0; overflow-y: auto; padding: 0 var(--sp-7) var(--sp-5); }
      .wn-state { font-size: var(--fs-sm); color: var(--ink-4); padding: var(--sp-7) 0; line-height: 1.6; }

      .wn-rel + .wn-rel { border-top: 1px solid var(--hair); }
      .wn-rel-head {
        position: sticky; top: 0; z-index: 1; display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap;
        padding: var(--sp-5) 0 var(--sp-3); background: linear-gradient(var(--panel) 78%, transparent);
      }
      .wn-rel-tag { font-family: var(--font-disp); font-weight: 600; font-size: var(--fs-md); color: var(--ink); }
      .wn-rel-date { font-size: var(--fs-xs); color: var(--ink-4); margin-left: auto; }
      .wn-rel-sum { font-size: var(--fs-sm); line-height: 1.55; color: var(--ink-2); padding: 0 0 var(--sp-3); }

      .wn-commit {
        display: grid; grid-template-columns: auto 1fr; gap: var(--sp-4); align-items: baseline;
        padding: var(--sp-3) 0; border-bottom: 1px solid color-mix(in oklch, var(--hair), transparent 35%);
      }
      .wn-commit:last-child { border-bottom: none; }
      .wn-type {
        font-size: var(--fs-3xs); font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase;
        padding: 3px 7px; border-radius: 5px; white-space: nowrap; align-self: start; margin-top: 1px;
        color: var(--chore); background: color-mix(in oklch, var(--chore), transparent 84%);
      }
      .wn-type.feat { color: var(--feat); background: color-mix(in oklch, var(--feat), transparent 86%); }
      .wn-type.fix { color: var(--fix); background: color-mix(in oklch, var(--fix), transparent 86%); }
      .wn-type.perf { color: var(--perf); background: color-mix(in oklch, var(--perf), transparent 84%); }
      .wn-type.refactor { color: var(--refactor); background: color-mix(in oklch, var(--refactor), transparent 84%); }
      .wn-type.chore { color: var(--chore); background: color-mix(in oklch, var(--chore), transparent 84%); }
      .wn-msg { font-size: var(--fs-sm); line-height: 1.5; color: var(--ink); }
      .wn-msg .scope { color: var(--accent); }
      .wn-msg .by { color: var(--ink-4); margin-left: var(--sp-3); font-size: var(--fs-xs); }

      .wn-foot {
        flex: none; display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-5) var(--sp-7);
        border-top: 1px solid var(--hair); background: var(--panel-2);
      }
      .wn-link {
        display: inline-flex; align-items: center; gap: var(--sp-2); font-size: var(--fs-sm); color: var(--ink-3);
        background: transparent; border: none; cursor: pointer;
      }
      .wn-link:hover { color: var(--accent-2); }
      .wn-link svg { width: var(--sp-5); height: var(--sp-5); }
      .wn-sp { flex: 1; }
    `,
  ],
})
export class WhatsNewModalComponent {
  readonly store = inject(SettingsStore);
  readonly version = inject(VersionService);
  readonly changelog = inject(ChangelogService);

  // features first within a release
  private static readonly ORDER: Record<string, number> = { feat: 0, fix: 1, perf: 2, refactor: 3, chore: 4 };
  private static readonly KNOWN = new Set(["feat", "fix", "perf", "refactor", "chore"]);

  /** Releases the user hasn't seen — everything newer than their build. */
  readonly releases = computed(() => this.changelog.since(this.version.version()));
  readonly headTag = computed(() => this.releases()[0]?.tag ?? `v${this.version.version() || "—"}`);
  readonly channel = computed(() => {
    const ch = this.releases()[0]?.channel ?? this.store.settings().channel;
    return ch === "beta" ? "BETA" : ch === "stable" ? "STABLE" : "DEV";
  });
  readonly fromSuffix = computed(() => {
    const n = this.releases().length;
    if (n > 1) return ` · ${n} releases`;
    const d = this.releases()[0]?.date;
    return d ? ` · ${d}` : "";
  });

  constructor() {
    // Load the changelog the first time the dialog opens (cached afterwards).
    effect(() => {
      if (this.store.whatsNewOpen()) void this.changelog.load();
    });
  }

  /** Features-first ordering within a release. */
  sorted(commits: ChangelogCommit[]): ChangelogCommit[] {
    return [...(commits ?? [])].sort(
      (a, b) => (WhatsNewModalComponent.ORDER[a.type] ?? 9) - (WhatsNewModalComponent.ORDER[b.type] ?? 9),
    );
  }

  /** Known conventional types get their own chip color; anything else is neutral. */
  typeClass(type: string): string {
    return WhatsNewModalComponent.KNOWN.has(type) ? type : "";
  }

  @HostListener("document:keydown.escape")
  onEsc(): void {
    if (this.store.whatsNewOpen()) this.store.closeWhatsNew();
  }

  /** Open the full changelog in the user's browser (window.open is blocked in
   *  the Tauri webview, so route through the opener plugin). */
  openChangelog(): void {
    import("@tauri-apps/plugin-opener")
      .then((m) => m.openUrl(RELEASES_URL))
      .catch(() => window.open(RELEASES_URL, "_blank"));
  }
}
