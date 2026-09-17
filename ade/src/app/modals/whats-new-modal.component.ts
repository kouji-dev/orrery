import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, inject } from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { SettingsStore } from "../settings/settings.store";
import { VersionService } from "../shared/version.service";
import { ChangelogService, ChangelogCommit } from "../updater/changelog.service";
import { RELEASES_URL } from "../shared/links";
import { KjBadgeComponent, KjButtonComponent, KjDialogComponent, KjSkeletonComponent } from "@kouji-ui/components";
import { KjDialog } from "@kouji-ui/core";

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
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjBadgeComponent, KjButtonComponent, KjDialogComponent, KjSkeletonComponent],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "What's new" },
  template: `
    @if (store.whatsNewOpen()) {
      <kj-dialog-shell>
        <div class="wn-modal">
          <div class="wn-hero">
            <kj-button kjSize="icon" class="wn-x" kjAriaLabel="Close" (click)="store.closeWhatsNew()"><app-icon name="x" size="sm" /></kj-button>
            <div class="up wn-eyebrow"><span class="d"></span>{{ store.updateKnown() ? 'Update available' : "What's new" }}</div>
            <div class="wn-h1">What's new in Orrery</div>
            <div class="wn-verline">
              <span class="wn-ver tnum">{{ headTag() }}</span>
              <kj-badge class="wn-badge" variant="outline">{{ channel() }}</kj-badge>
              <span class="wn-from tnum">from v{{ version.version() || '—' }}{{ fromSuffix() }}</span>
            </div>
          </div>

          <div class="wn-body scroll-y">
            @if (changelog.loading() && !releases().length) {
              <div class="wn-state" aria-label="Loading changelog"><kj-skeleton kjSkeletonShape="text-block" [kjLines]="4" /></div>
            } @else if (!releases().length) {
              <div class="wn-state">
                Couldn't load the changelog{{ changelog.error() ? ' — check your connection' : '' }}.
                Open the full changelog for the complete release history.
              </div>
            } @else {
              @for (rel of releases(); track rel.tag) {
                <section class="wn-rel">
                  <div class="wn-rel-head">
                    <kj-badge class="wn-rel-tag tnum" size="sm" variant="outline">{{ rel.tag }}</kj-badge>
                    @if (rel.channel) { <kj-badge class="wn-badge" variant="outline">{{ rel.channel === 'beta' ? 'BETA' : rel.channel === 'stable' ? 'STABLE' : 'DEV' }}</kj-badge> }
                    @if (rel.date) { <span class="wn-rel-date tnum">{{ rel.date }}</span> }
                  </div>
                  @if (rel.summary) { <p class="wn-rel-sum">{{ rel.summary }}</p> }
                  @for (c of sorted(rel.commits); track $index) {
                    <div class="wn-commit">
                      <span class="up wn-type" [class]="'wn-type ' + typeClass(c.type)">{{ c.type }}</span>
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
            <kj-button kjVariant="ghost" class="wn-link" kjVariant="quiet" (click)="openChangelog()">
              <app-icon name="file" size="sm" />View full changelog<app-icon name="ext" size="sm" />
            </kj-button>
            <kj-button class="wn-continue" kjVariant="default" (click)="store.closeWhatsNew()"><app-icon name="check" size="sm" />Continue</kj-button>
          </div>
        </div>
      </kj-dialog-shell>
    }
  `,
  styles: [
    `
      /* The panel box, not a page-level surface: KjDialog centers this
         component's host and paints the scrim behind it. */
      .wn-modal {
        --feat: var(--sem-change); --fix: var(--sem-add); --perf: var(--lane-1); --refactor: var(--ui-ink); --chore: var(--ink-3);
        width: round(calc(544px * var(--density)), 1px); max-width: calc(100vw - 48px); max-height: 86vh; display: flex; flex-direction: column;
        background: var(--panel); border: 1px solid var(--hair-2); border-radius: var(--r-lg); overflow: hidden;
        box-shadow: var(--shadow); color: var(--ink); font-family: var(--font-ui);
      }
      .wn-hero {
        position: relative; flex: none; padding: var(--sp-7) var(--sp-7) var(--sp-6);
        border-bottom: 1px solid var(--hair); overflow: hidden;
      }
      .wn-hero::before {
        content: ""; position: absolute; inset: 0; pointer-events: none;
        background: radial-gradient(120% 92% at 16% -16%, var(--ui-sel), transparent 66%);
      }
      .wn-hero::after {
        content: ""; position: absolute; left: 0; right: 0; top: 0; height: 2px;
        background: linear-gradient(90deg, var(--brand-1), var(--brand-2), var(--brand-3)); opacity: 0.85;
      }
      .wn-x ::ng-deep .kj-button {
        position: absolute; top: var(--sp-5); right: var(--sp-5); width: 28px; height: 28px; padding: 0; border-radius: var(--r-sm);
        border: 1px solid transparent; background: transparent; box-shadow: none; color: var(--ink-3); cursor: pointer; display: grid; place-items: center;
      }
      .wn-x ::ng-deep .kj-button:hover { background: var(--panel-3); color: var(--ink); border-color: var(--hair); }
      .wn-eyebrow {
        position: relative; display: inline-flex; align-items: center; gap: var(--sp-3); font-size: var(--fs-meta);
        color: var(--ink-3);
      }
      .wn-eyebrow .d { width: 6px; height: 6px; border-radius: 50%; background: var(--brand-2); }
      .wn-h1 { position: relative; font-family: var(--font-disp); font-weight: var(--fw-medium); font-size: var(--fs-xl); letter-spacing: -0.02em; margin-top: var(--sp-3); }
      .wn-verline { position: relative; display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap; margin-top: var(--sp-4); }
      .wn-ver { font-family: var(--font-disp); font-weight: var(--fw-medium); font-size: var(--fs-md); color: var(--ui-ink); }
      .wn-badge ::ng-deep .kj-badge {
        font-size: var(--fs-badge); font-weight: var(--fw-strong); letter-spacing: 0.12em; padding: 2px 7px; border-radius: 999px;
        color: var(--sem-attn); border: 1px solid color-mix(in oklch, var(--sem-attn), transparent 56%);
        background: color-mix(in oklch, var(--sem-attn), transparent 88%);
      }
      .wn-from {  color: var(--ink-4); }
      .wn-body { flex: 1; min-height: 0; overflow-y: auto; padding: 0 var(--sp-7) var(--sp-5); }
      .wn-state { font-size: var(--fs-meta); color: var(--ink-4); padding: var(--sp-7) 0; line-height: 1.6; }

      .wn-rel + .wn-rel { border-top: 1px solid var(--hair); }
      .wn-rel-head {
        position: sticky; top: 0; z-index: 1; display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap;
        padding: var(--sp-5) 0 var(--sp-3); background: linear-gradient(var(--panel) 78%, transparent);
      }
      .wn-rel-tag ::ng-deep .kj-badge { font: 600 var(--fs-md) / 1.2 var(--font-disp); color: var(--ink);
        background: transparent; border-color: transparent; padding: 0; }
      .wn-rel-date {  color: var(--ink-4); margin-left: auto; }
      .wn-rel-sum { line-height: 1.55; color: var(--ink-2); padding: 0 0 var(--sp-3); }

      .wn-commit {
        display: grid; grid-template-columns: auto 1fr; gap: var(--sp-4); align-items: baseline;
        padding: var(--sp-3) 0; border-bottom: 1px solid color-mix(in oklch, var(--hair), transparent 35%);
      }
      .wn-commit:last-child { border-bottom: none; }
      .wn-type {
        font-weight: var(--fw-medium);
        padding: 3px 7px; border-radius: 5px; white-space: nowrap; align-self: start; margin-top: 1px;
        color: var(--chore); background: color-mix(in oklch, var(--chore), transparent 84%);
      }
      .wn-type.feat { color: var(--feat); background: color-mix(in oklch, var(--feat), transparent 86%); }
      .wn-type.fix { color: var(--fix); background: color-mix(in oklch, var(--fix), transparent 86%); }
      .wn-type.perf { color: var(--perf); background: color-mix(in oklch, var(--perf), transparent 84%); }
      .wn-type.refactor { color: var(--refactor); background: color-mix(in oklch, var(--refactor), transparent 84%); }
      .wn-type.chore { color: var(--chore); background: color-mix(in oklch, var(--chore), transparent 84%); }
      .wn-msg { line-height: 1.5; color: var(--ink); }
      .wn-msg .scope { color: var(--ui-ink); }
      .wn-msg .by { color: var(--ink-4); margin-left: var(--sp-3);  }

      .wn-foot {
        flex: none; display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-5) var(--sp-7);
        border-top: 1px solid var(--hair); background: var(--panel-2);
      }
      /* .kj-quiet is the bare-label recipe; the link ink and the tighter icon
         gap are all that is per-instance. */
      .wn-link ::ng-deep .kj-button {
        --kj-button-fg: var(--ink-3); --kj-button-gap: var(--sp-2); --kj-button-font-size: var(--fs-meta);
      }
      .wn-link ::ng-deep .kj-button:hover:not([aria-disabled="true"]) { --kj-button-fg: var(--ui-link); }
      .wn-link ::ng-deep .kj-button svg { width: var(--sp-5); height: var(--sp-5); }
      .wn-continue ::ng-deep .kj-button { margin-left: auto; }
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
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.store.closeWhatsNew());
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

  /** Open the full changelog in the user's browser (window.open is blocked in
   *  the Tauri webview, so route through the opener plugin). */
  openChangelog(): void {
    import("@tauri-apps/plugin-opener")
      .then((m) => m.openUrl(RELEASES_URL))
      .catch(() => window.open(RELEASES_URL, "_blank"));
  }
}
