import { ChangeDetectionStrategy, Component, ElementRef, HostListener, inject, signal } from "@angular/core";
import { PALETTES } from "../data";
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
          <span class="tweak-brand"><app-icon name="spark" size="sm" [color]="'var(--accent)'" /></span>
          <span class="disp" style="font-size:13px;font-weight:600">Tweaks</span>
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
        <div class="tweak-row">
          <span class="tweak-label">Accent palette</span>
          <div style="display:flex;gap:7px">
            @for (p of palettes; track p.name) {
              <button
                class="swatch"
                [class.on]="t.palette[0] === p.value[0] && t.palette[1] === p.value[1]"
                [title]="p.name"
                (click)="ui.setTweak('palette', p.value)"
                [style.background]="'linear-gradient(135deg,' + p.value[0] + ',' + p.value[1] + ')'"
              ></button>
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
      /* matches the DevConsole FAB (.dvc-fab) — stacked directly above it */
      .tweak-fab {
        position: fixed;
        right: 18px;
        bottom: 92px;
        z-index: 90;
        width: 44px;
        height: 44px;
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
        border-color: rgba(var(--accent-rgb), 0.5);
      }
      .tweak-fab.on {
        color: var(--accent);
        border-color: rgba(var(--accent-rgb), 0.6);
        box-shadow: var(--shadow), 0 0 18px -6px rgba(var(--accent-rgb), 0.7);
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
        box-shadow: var(--shadow), 0 0 0 1px rgba(var(--accent-rgb), 0.04);
        font-family: var(--font-mono);
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
        gap: 10px;
        padding: 9px 11px 9px 13px;
        border-bottom: 1px solid var(--hair);
        background: linear-gradient(180deg, var(--panel-3), var(--panel));
      }
      .tweak-brand {
        display: flex;
        align-items: center;
        color: var(--accent);
      }
      .tweak-x {
        border: none;
        padding: 5px;
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
        padding: 12px 16px 14px;
        display: flex;
        flex-direction: column;
        gap: 4px;
      }
      .tweak-section {
        font-size: 9px;
        text-transform: uppercase;
        letter-spacing: 0.12em;
        color: var(--ink-3);
        margin: 10px 0 4px;
      }
      .tweak-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 5px 0;
      }
      .tweak-label {
        font-size: 11.5px;
        color: var(--ink-2);
        flex: 1;
      }
      .seg {
        display: flex;
        gap: 2px;
        padding: 2px;
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
      }
      .seg button {
        font-family: var(--font-mono);
        font-size: 10.5px;
        text-transform: capitalize;
        padding: 3px 8px;
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
      .swatch {
        width: 22px;
        height: 22px;
        border-radius: 50%;
        cursor: pointer;
        border: 2px solid transparent;
      }
      .swatch.on {
        border-color: var(--ink);
        box-shadow: 0 0 0 2px var(--elev);
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
        width: 13px;
        height: 13px;
        border-radius: 50%;
        background: var(--ink-3);
        transition:
          transform 0.15s,
          background 0.15s;
      }
      .toggle.on {
        background: var(--accent);
        border-color: var(--accent);
      }
      .toggle.on span {
        transform: translateX(15px);
        background: #06070b;
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

  readonly palettes = Object.entries(PALETTES).map(([name, value]) => ({ name, value }));
  readonly vizOptions: VizMode[] = ["grid", "kanban", "graph", "timeline"];

  setViz(e: Event) {
    this.ui.setTweak("defaultViz", (e.target as HTMLSelectElement).value as VizMode);
  }
}
