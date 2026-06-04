import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { PALETTES } from "../data";
import { VizMode } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";

@Component({
  selector: "app-tweaks-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let t = store.tweaks();
    <!-- launcher -->
    <button
      class="tweak-fab"
      (click)="open.set(!open())"
      title="Tweaks"
      style="position:fixed;right:18px;bottom:38px;z-index:70;width:42px;height:42px;border-radius:50%;display:grid;place-items:center;cursor:pointer;color:#06070b;border:none;background:linear-gradient(180deg,var(--accent),color-mix(in oklch,var(--accent),#000 14%));box-shadow:0 0 22px -4px rgba(var(--accent-rgb),0.8)"
    >
      <app-icon name="spark" [color]="'#06070b'" />
    </button>

    @if (open()) {
      <div
        class="rise"
        style="position:fixed;right:18px;bottom:90px;z-index:70;width:288px;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-lg);box-shadow:var(--shadow);padding:14px 16px;display:flex;flex-direction:column;gap:4px"
      >
        <div style="display:flex;align-items:center;gap:8px;margin-bottom:6px">
          <app-icon name="spark" size="sm" color="var(--accent)" />
          <span class="disp" style="font-size:13px;font-weight:600">Tweaks</span>
          <button class="btn" (click)="open.set(false)" style="margin-left:auto;padding:3px"><app-icon name="x" size="sm" /></button>
        </div>

        <!-- Theme -->
        <div class="tweak-section">Theme</div>
        <div class="tweak-row">
          <span class="tweak-label">Mode</span>
          <div class="seg">
            @for (m of ['dark', 'light']; track m) {
              <button [class.on]="t.theme === m" (click)="store.setTweak('theme', $any(m))">{{ m }}</button>
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
                (click)="store.setTweak('palette', p.value)"
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
              <button [class.on]="t.density === d" (click)="store.setTweak('density', $any(d))">{{ d }}</button>
            }
          </div>
        </div>
        <div class="tweak-row">
          <span class="tweak-label">Right panel</span>
          <button class="toggle" [class.on]="t.rightPanel" (click)="store.setTweak('rightPanel', !t.rightPanel)"><span></span></button>
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
          <button class="toggle" [class.on]="t.motion" (click)="store.setTweak('motion', !t.motion)"><span></span></button>
        </div>
      </div>
    }
  `,
  styles: [
    `
      .tweak-fab:hover {
        filter: brightness(1.08);
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
  readonly store = inject(OrchestraStore);
  readonly open = signal(false);

  readonly palettes = Object.entries(PALETTES).map(([name, value]) => ({ name, value }));
  readonly vizOptions: VizMode[] = ["grid", "kanban", "graph", "timeline"];

  setViz(e: Event) {
    this.store.setTweak("defaultViz", (e.target as HTMLSelectElement).value as VizMode);
  }
}
