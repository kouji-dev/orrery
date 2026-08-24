import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { SettingsStore } from "../settings/settings.store";
import { VersionService } from "../shared/version.service";
import { KjButtonComponent, KjMuted, KjToastComponent } from "@kouji-ui/components";

/**
 * Bottom-center "update available" toast — the prominent, global warning that an
 * update is ready, complementing the card buried in Settings → Updates. Visible
 * while an update is KNOWN and not yet dismissed (`updateCard`), and hidden while
 * the settings modal is open (the card shows the same thing there). "Later"
 * dismisses the toast but the nav dot (update known) stays"Install" runs the
 * same install flow as the card.
 *
 * Faithful port of the design's UpdateToast (design/app.html `.wn-toast`):
 * rocket tile · title/sub column · quiet "Later" + primary "Install". kj-toast
 * carries the toast semantics (role="status"); kj-button carries the two
 * actions — both repainted onto the design's exact geometry below.
 */
@Component({
  selector: "app-update-toast",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjButtonComponent, KjMuted, KjToastComponent],
  template: `
    @if (!store.open() && store.updateCard(); as upd) {
      <div class="ut">
        <kj-toast>
          <span class="ut-ic glyph-plate"><app-icon name="rocket" /></span>
          <div class="ut-col">
            <h2 class="ut-t1">Update available · <b>v{{ upd.version }}</b></h2>
            <!-- no size segment: the updater metadata (UpdateInfo / UpdateHandle)
                 carries no content-length before the download starts -->
            <p class="ut-t2" kjMuted>from v{{ version.version() || '—' }}</p>
          </div>
          <div class="ut-act">
            <kj-button kjVariant="quiet" (click)="store.openWhatsNew()" title="See what's new">What's new</kj-button>
            <kj-button kjVariant="quiet" (click)="store.dismissUpdate()">Later</kj-button>
            <kj-button kjVariant="default" [kjDisabled]="store.installing()" (click)="store.install()">
              <app-icon name="stage" size="sm" />
              {{ store.installing() ? (store.installPhase() === 'installing' ? 'Installing…' : 'Downloading ' + pct() + '%') : 'Install' }}
            </kj-button>
          </div>
        </kj-toast>
      </div>
    }
  `,
  styles: [
    `
      /* .ut hosts nothing visual — the kj-toast box IS the design's .wn-toast.
         Fixed bottom-center pinning + the rise animation live on the inner box
         so translate(-50%) composes with the entrance offset exactly as in the
         design keyframes. */
      .ut ::ng-deep .kj-toast {
        position: fixed; left: 50%; bottom: 46px; transform: translateX(-50%); z-index: 84;
        display: flex; align-items: center; gap: 14px;
        /* shrink-to-fit: the toast is one line of copy plus three
           actions, so a fixed width just pads it out */
        width: max-content; max-width: calc(100vw - 32px);
        padding: 13px 14px 13px 16px; border-radius: 13px;
        font-family: var(--font-ui); color: var(--ink);
        background: var(--panel); border: 1px solid var(--hair-2);
        box-shadow: var(--shadow);
        animation: ut-rise 0.4s cubic-bezier(0.2, 0.8, 0.2, 1);
      }
      @keyframes ut-rise {
        from { transform: translate(-50%, 14px); }
        to { transform: translate(-50%, 0); }
      }
      /* .glyph-plate paints the tile; only the size and the design's --ui-line
         ring (the toast plate is a notch stronger than the settings one) are
         per-instance. */
      .ut-ic {
        flex: none; width: 34px; height: 34px;
        box-shadow: inset 0 0 0 1px var(--ui-line);
      }
      .ut-col { flex: 0 1 auto; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
      .ut-t1 { margin: 0; color: var(--ink); font-weight: var(--fw-medium); display: flex; align-items: center; gap: 7px; }
      .ut-t1 b { color: var(--ui-ink); font-weight: var(--fw-medium); font-variant-numeric: tabular-nums; }
      .ut-t2 { margin: 0; font-size: var(--fs-micro); font-variant-numeric: tabular-nums; }
      .ut-act { flex: none; display: flex; align-items: center; gap: 7px; }
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
