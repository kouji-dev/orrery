import {
  ChangeDetectionStrategy,
  Component,
  NgZone,
  OnDestroy,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** True only inside the Tauri webview (false under a plain `ng serve` browser),
 *  so the controls degrade to no-ops instead of throwing when there's no window. */
function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// Borderless-titlebar window controls — minimize, maximize/restore, close —
// driven straight off the Tauri window API (see capabilities/default.json for the
// granted permissions). Matches the ORCHESTRA design's WindowControls: 44px-wide
// `.winbtn` cells, the close button flagged `danger` (hover → red). HIDE is
// intentionally deferred for now (per current scope); the design also has an
// eye-off "Hide to tray" cell we can slot back in here later.
@Component({
  selector: "app-window-controls",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="display:flex;align-items:stretch;height:100%;flex:none">
      <button class="winbtn" title="Minimize" aria-label="Minimize" (click)="minimize()">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
          stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M6 12h12" />
        </svg>
      </button>

      <button
        class="winbtn"
        [title]="maximized() ? 'Restore' : 'Maximize'"
        [attr.aria-label]="maximized() ? 'Restore' : 'Maximize'"
        (click)="toggleMaximize()"
      >
        @if (maximized()) {
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <rect x="8.5" y="8.5" width="9.5" height="9.5" rx="1.5" />
            <path d="M6 15.5V6h9.5" />
          </svg>
        } @else {
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <rect x="6" y="6" width="12" height="12" rx="1.5" />
          </svg>
        }
      </button>

      <button class="winbtn danger" title="Close" aria-label="Close" (click)="close()">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
          stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      </button>
    </div>
  `,
})
export class WindowControlsComponent implements OnInit, OnDestroy {
  private readonly zone = inject(NgZone);
  /** Maximized vs normal — flips the middle button between the square (maximize)
   *  and the offset-squares (restore) glyph, mirroring the design. */
  readonly maximized = signal(false);
  private unlisten?: () => void;

  async ngOnInit() {
    if (!inTauri()) return;
    try {
      const w = getCurrentWindow();
      this.maximized.set(await w.isMaximized());
      // Tauri's resize event fires outside Angular's zone — hop back in so the
      // signal write schedules change detection and the glyph stays in sync.
      this.unlisten = await w.onResized(async () => {
        const m = await w.isMaximized();
        this.zone.run(() => this.maximized.set(m));
      });
    } catch {
      /* not in tauri / permission missing — controls stay inert */
    }
  }

  ngOnDestroy() {
    this.unlisten?.();
  }

  minimize() {
    if (inTauri()) void getCurrentWindow().minimize();
  }
  toggleMaximize() {
    if (inTauri()) void getCurrentWindow().toggleMaximize();
  }
  close() {
    if (inTauri()) void getCurrentWindow().close();
  }
}
