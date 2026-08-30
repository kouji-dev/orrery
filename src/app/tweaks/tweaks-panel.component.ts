import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { VizMode } from "../models";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { KjButton } from "@kouji-ui/core";
import { KjToggleComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";
import { DismissDirective } from "../shared/dismiss.directive";
import { SelectComponent } from "../shared/select.component";

@Component({
  selector: "app-tweaks-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  hostDirectives: [DismissDirective],
  imports: [IconComponent, KjButton, KjToggleComponent, SelectComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
  template: `
    @let t = ui.tweaks();
    <!-- launcher -->
    <button kjButton class="fab tweak-fab" [class.on]="open()" (click)="open.set(!open())" title="Tweaks">
      <app-icon name="spark" />
    </button>

    @if (open()) {
      <section class="corner-panel tweak-panel" aria-label="Tweaks">
        <header class="pane-head tweak-head">
          <span class="tweak-brand"><app-icon name="spark" size="sm" [color]="'var(--ui-ink)'" /></span>
          <h2>Tweaks</h2>
          <button kjButton class="pane-btn kj-push" (click)="open.set(false)" title="Close"><app-icon name="x" size="sm" /></button>
        </header>
        <div class="tweak-body">

        <!-- Theme -->
        <div class="up tweak-section">Theme</div>
        <div class="tweak-row">
          <span class="tweak-label">Mode</span>
          <kj-tabs variant="pills" class="seg tabs-xs" [value]="t.theme" (valueChange)="ui.setTweak('theme', $any($event))">
            <kj-tab-list aria-label="Theme mode">
              @for (m of ['dark', 'light']; track m) {
                <kj-tab [value]="m">{{ m }}</kj-tab>
              }
            </kj-tab-list>
          </kj-tabs>
        </div>

        <!-- Layout -->
        <div class="up tweak-section">Layout</div>
        <div class="tweak-row">
          <span class="tweak-label">Density</span>
          <kj-tabs variant="pills" class="seg tabs-xs" [value]="t.density" (valueChange)="ui.setTweak('density', $any($event))">
            <kj-tab-list aria-label="Density">
              @for (d of ['compact', 'regular', 'comfy']; track d) {
                <kj-tab [value]="d">{{ d }}</kj-tab>
              }
            </kj-tab-list>
          </kj-tabs>
        </div>
        <!-- Orchestrator -->
        <div class="up tweak-section">Orchestrator</div>
        <div class="tweak-row">
          <span class="tweak-label">Agent visualization</span>
          <app-select style="display:inline-block;width: round(calc(120px * var(--density)), 1px)" [value]="t.defaultViz" [options]="vizOptions" (valueChange)="ui.setTweak('defaultViz', $any($event))" />
        </div>
        <div class="tweak-row">
          <span class="tweak-label">Live motion</span>
          <kj-toggle appearance="switch" size="sm" ariaLabel="Live motion" [pressed]="t.motion" (pressedChange)="ui.setTweak('motion', $event)" />
        </div>
        </div>
      </section>
    }
  `,
  styles: [
    `
      /* FAB skin + the corner-panel box and its pop-up entrance are the shared
         recipes in styles.css (.fab / .corner-panel) — this used to be a
         byte-for-byte copy of the dev console's, down to a second keyframes
         block under another name. Only where this panel sits on the rail and
         how wide it may grow are local. */
      .tweak-panel {
        bottom: 148px;
        z-index: 70;
        /* min-width, not width: the segmented controls below size from the type
           ramp, so a hard width clipped the third option ("comfy") outright once
           --fs-xs grew. Anchored bottom-right, so it grows leftward. */
        min-width: round(calc(288px * var(--density)), 1px);
        max-width: calc(100vw - 36px);
      }
      /* header chrome is .pane-head; the gradient + tighter right edge (the
         close button sits in it) are this panel's own. */
      .tweak-head {
        gap: var(--sp-5);
        padding: var(--sp-4) var(--sp-5) var(--sp-4) var(--sp-6);
        background: linear-gradient(180deg, var(--panel-3), var(--panel));
      }
      .tweak-brand {
        display: flex;
        align-items: center;
        color: var(--ui-ink);
      }
      .tweak-body {
        padding: var(--sp-6) var(--sp-7) var(--sp-6);
        display: flex;
        flex-direction: column;
        gap: var(--sp-2);
      }
      .tweak-section {
        color: var(--ink-3);
        margin: var(--sp-5) 0 var(--sp-2);
      }
      .tweak-row {
        display: flex;
        align-items: center;
        gap: var(--sp-5);
        padding: var(--sp-2) 0;
        /* a segment group that outgrows the row drops to its own line rather
           than overflowing into the panel's overflow:hidden and vanishing */
        flex-wrap: wrap;
      }
      .tweak-label {
        color: var(--ink-2);
        flex: 1;
      }
      /* tray, chip and selected state come from kouji's pills tabs; only the
         capitalised labels are this panel's own */
      .seg {
        text-transform: capitalize;
      }
    `,
  ],
})
export class TweaksPanelComponent {
  readonly ui = inject(UiStore);
  readonly open = signal(false);

  constructor() {
    // close on Escape / click outside the FAB + panel (both live under this
    // component's host, which carries the shared DismissDirective)
    inject(DismissDirective).appDismiss.subscribe(() => {
      if (this.open()) this.open.set(false);
    });
  }

  readonly vizOptions: VizMode[] = ["grid", "kanban", "graph", "timeline"];
}
