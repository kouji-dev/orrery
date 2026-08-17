import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { SettingsStore } from "../settings/settings.store";
import { VersionService } from "../shared/version.service";

/**
 * Bottom-center "update available" toast — the prominent, global warning that an
 * update is ready, complementing the card buried in Settings → Updates. Visible
 * while an update is KNOWN and not yet dismissed (`updateCard`), and hidden while
 * the settings modal is open (the card shows the same thing there). "Later"
 * dismisses the toast but the nav dot (update known) stays; "Install" runs the
 * same install flow as the card; "What's new" opens the release-notes digest.
 */
@Component({
  selector: "app-update-toast",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @if (!store.open() && store.updateCard(); as upd) {
      <div class="ut" role="status">
        <span class="ut-ic"><app-icon name="stage" /></span>
        <div class="ut-col">
          <div class="ut-t1">Update available · <b class="tnum">v{{ upd.version }}</b></div>
          <div class="ut-t2 tnum">from v{{ version.version() || '—' }}{{ upd.date ? ' · ' + upd.date : '' }}</div>
        </div>
        <div class="ut-act">
          <button class="ut-notes" (click)="store.openWhatsNew()" title="See what's new">What's new</button>
          <button class="ut-later" (click)="store.dismissUpdate()">Later</button>
          <button class="ut-install" [disabled]="store.installing()" (click)="store.install()">
            <app-icon name="stage" size="sm" />
            {{ store.installing() ? (store.installPhase() === 'installing' ? 'Installing…' : 'Downloading ' + pct() + '%') : 'Install' }}
          </button>
        </div>
      </div>
    }
  `,
  styles: [
    `
      .ut {
        position: fixed; left: 50%; bottom: 46px; transform: translateX(-50%); z-index: 84;
        display: flex; align-items: center; gap: var(--sp-5); width: 392px; max-width: calc(100vw - 32px);
        padding: var(--sp-4) var(--sp-4) var(--sp-4) var(--sp-5); border-radius: var(--r-md);
        font-family: var(--font-ui); color: var(--ink); background: var(--panel); border: 1px solid var(--hair-2);
        box-shadow: var(--shadow);
      }
      .ut-ic {
        flex: none; width: 34px; height: 34px; border-radius: var(--r-sm); display: grid; place-items: center;
        color: var(--ui-ink); background: var(--ui-sel);
        box-shadow: inset 0 0 0 1px var(--ui-line);
      }
      .ut-ic svg { width: var(--sp-6); height: var(--sp-6); }
      .ut-col { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
      .ut-t1 { font-size: var(--fs-sm); color: var(--ink); }
      .ut-t1 b { color: var(--ui-ink); font-weight: 600; }
      .ut-t2 { font-size: var(--fs-2xs); color: var(--ink-4); }
      .ut-act { flex: none; display: flex; align-items: center; gap: var(--sp-3); }
      .ut-notes, .ut-later {
        background: transparent; border: none; color: var(--ink-4); font-family: var(--font-ui);
        font-size: var(--fs-xs); cursor: pointer; padding: var(--sp-2) var(--sp-3); border-radius: var(--r-sm);
      }
      .ut-notes:hover, .ut-later:hover { color: var(--ink-2); }
      .ut-install {
        display: inline-flex; align-items: center; gap: var(--sp-2); height: 28px; padding: 0 var(--sp-5);
        border-radius: var(--r-sm); font-family: var(--font-ui); font-size: var(--fs-xs); font-weight: 600;
        cursor: pointer; border: none; color: var(--ui-on-fill); background: var(--ui-fill);
      }
      .ut-install:hover:not(:disabled) { filter: brightness(1.08); }
      .ut-install:disabled { opacity: 0.7; cursor: default; }
      .ut-install svg { width: var(--sp-5); height: var(--sp-5); }
    `,
  ],
})
export class UpdateToastComponent {
  readonly store = inject(SettingsStore);
  readonly version = inject(VersionService);

  pct(): number {
    return Math.round(this.store.installProgress() * 100);
  }
}
