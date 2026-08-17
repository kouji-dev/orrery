import { ChangeDetectionStrategy, Component, ElementRef, HostListener, inject, signal } from "@angular/core";
import { VizMode } from "../models";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";

@Component({
  selector: "app-tweaks-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let t = ui.tweaks();
    <!-- launcher -->
    <button class="tweak-fab" [class.on]="open()" (click)="open.set(!open())" title="Tweaks">
      <app-icon name="spark" />
    </button>

    @if (open()) {
      <section class="tweak-panel" aria-label="Tweaks">
        <header class="tweak-head">
          <span class="tweak-brand"><app-icon name="spark" size="sm" [color]="'var(--ui-ink)'" /></span>
          <span class="disp" style="font-size:var(--fs-md);font-weight:600">Tweaks</span>
          <span style="flex:1"></span>
          <button class="tweak-x" (click)="open.set(false)" title="Close"><app-icon name="x" size="sm" /></button>
        </header>
        <div class="tweak-body">

        <!-- Theme -->
        <div class="tweak-section">Theme</div>
        <div class="tweak-row">
          <span class="tweak-label">Mode</span>
          <div class="seg">
            @for (m of ['dark', 'light']; track m) {
              <button [class.on]="t.theme === m" (click)="ui.setTweak('theme', $any(m))">{{ m }}</button>
            }
          </div>
        </div>

        <!-- Layout -->
        <div class="tweak-section">Layout</div>
        <div class="tweak-row">
          <span class="tweak-label">Density</span>
          <div class="seg">
            @for (d of ['compact', 'regular', 'comfy']; track d) {
              <button [class.on]="t.density === d" (click)="ui.setTweak('density', $any(d))">{{ d }}</button>
            }
          </div>
        </div>
        <div class="tweak-row">
          <span class="tweak-label">Right panel</span>
          <button class="toggle" [class.on]="t.rightPanel" (click)="ui.setTweak('rightPanel', !t.rightPanel)"><span></span></button>
        </div>

        <!-- Orchestrator -->
        <div class="tweak-section">Orchestrator</div>
        <div class="tweak-row">
          <span class="tweak-label">Agent visualization</span>
          <select class="osel" style="width:120px" [value]="t.defaultViz" (change)="setViz($event)">
            @for (v of vizOptions; track v) { <option [value]="v">{{ v }}</option> }
          </select>
        </div>
        <div class="tweak-row">
          <span class="tweak-label">Live motion</span>
          <button class="toggle" [class.on]="t.motion" (click)="ui.setTweak('motion', !t.motion)"><span></span></button>
        </div>
        </div>
      </section>
    }
  `,
  styles: [
    `
      /* matches the DevConsole FAB (.dvc-fab); positioned by the shell's
         .anchor-rail flex container, not individually — so it drops to the corner
         when the Perf FAB is hidden in PRD. */
      .tweak-fab {
        position: relative;
        /* square launcher: width MUST track height so it stays square across
           densities (sized to the top-bar height) */
        width: var(--topbar-h);
        height: var(--topbar-h);
        border-radius: 13px;
        display: grid;
        place-items: center;
        cursor: pointer;
        border: 1px solid var(--hair-2);
        background: linear-gradient(180deg, var(--panel-3), var(--panel));
        color: var(--ink-2);
        box-shadow: var(--shadow);
        transition: transform 0.16s, color 0.16s, border-color 0.16s, box-shadow 0.16s;
      }
      .tweak-fab:hover {
        transform: translateY(-2px);
        color: var(--ink);
        border-color: var(--ui-line);
      }
      .tweak-fab.on {
        color: var(--ui-ink);
        border-color: var(--ui-line);
        box-shadow: var(--shadow);
      }
      .tweak-fab svg {
        width: 19px;
        height: 19px;
      }
      /* popup chrome mirrors the DevConsole (.dvcon) — opens above its FAB */
      .tweak-panel {
        position: fixed;
        right: 18px;
        bottom: 148px;
        z-index: 70;
        width: 288px;
        overflow: hidden;
        background: var(--panel);
        border: 1px solid var(--hair-2);
        border-radius: 14px;
        box-shadow: var(--shadow);
        font-family: var(--font-ui);
        transform-origin: bottom right;
        animation: tweakin 0.22s cubic-bezier(0.2, 0.7, 0.2, 1);
      }
      @keyframes tweakin {
        from { opacity: 0; transform: translateY(8px) scale(0.985); }
        to { opacity: 1; transform: none; }
      }
      .tweak-head {
        display: flex;
        align-items: center;
        gap: var(--sp-5);
        padding: var(--sp-4) var(--sp-5) var(--sp-4) var(--sp-6);
        border-bottom: 1px solid var(--hair);
        background: linear-gradient(180deg, var(--panel-3), var(--panel));
      }
      .tweak-brand {
        display: flex;
        align-items: center;
        color: var(--ui-ink);
      }
      .tweak-x {
        border: none;
        padding: var(--sp-2);
        color: var(--ink-3);
        background: transparent;
        border-radius: var(--r-sm);
        cursor: pointer;
        display: inline-flex;
      }
      .tweak-x:hover {
        color: var(--ink);
        background: var(--panel-3);
      }
      .tweak-body {
        padding: var(--sp-6) var(--sp-7) var(--sp-6);
        display: flex;
        flex-direction: column;
        gap: var(--sp-2);
      }
      .tweak-section {
        font-size: var(--fs-2xs);
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--ink-3);
        margin: var(--sp-5) 0 var(--sp-2);
      }
      .tweak-row {
        display: flex;
        align-items: center;
        gap: var(--sp-5);
        padding: var(--sp-2) 0;
      }
      .tweak-label {
        font-size: var(--fs-sm);
        color: var(--ink-2);
        flex: 1;
      }
      .seg {
        display: flex;
        gap: var(--sp-1);
        padding: var(--sp-1);
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
      }
      .seg button {
        font-family: var(--font-ui);
        font-size: var(--fs-xs);
        text-transform: capitalize;
        padding: var(--sp-1) var(--sp-4);
        border-radius: 4px;
        border: none;
        cursor: pointer;
        background: transparent;
        color: var(--ink-3);
      }
      .seg button.on {
        background: var(--panel-3);
        color: var(--ink);
        box-shadow: 0 0 0 1px var(--hair-2);
      }
      .toggle {
        width: 34px;
        height: 19px;
        border-radius: 999px;
        border: 1px solid var(--hair-2);
        background: var(--panel-2);
        cursor: pointer;
        position: relative;
        transition: background 0.15s;
        padding: 0;
      }
      .toggle span {
        position: absolute;
        top: 2px;
        left: 2px;
        /* knob: fixed 13x13 circle to match the fixed-geometry track
           (34x19 + 2px inset + 15px travel). Scaling only height made it oval. */
        width: 13px;
        height: 13px;
        border-radius: 50%;
        background: var(--ink-3);
        transition:
          transform 0.15s,
          background 0.15s;
      }
      .toggle.on {
        background: var(--ui-fill);
        border-color: var(--ui-focus);
      }
      .toggle.on span {
        transform: translateX(15px);
        background: var(--ui-on-fill);
      }
    `,
  ],
})
export class TweaksPanelComponent {
  readonly ui = inject(UiStore);
  readonly open = signal(false);
  private readonly host = inject(ElementRef<HTMLElement>);

  // close on click outside the FAB + panel (both live under this host element)
  @HostListener("document:mousedown", ["$event"]) onDocDown(e: MouseEvent) {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node)) this.open.set(false);
  }

  readonly vizOptions: VizMode[] = ["grid", "kanban", "graph", "timeline"];

  setViz(e: Event) {
    this.ui.setTweak("defaultViz", (e.target as HTMLSelectElement).value as VizMode);
  }
}
