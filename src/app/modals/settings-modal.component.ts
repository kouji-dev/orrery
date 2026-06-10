import {
  ChangeDetectionStrategy,
  Component,
  computed,
  EventEmitter,
  HostListener,
  inject,
  Input,
  Output,
  signal,
  ViewEncapsulation,
} from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { BRIDGE } from "../data-source/bridge";
import { AutoApprovePolicy, SettingsEvents } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import {
  effectiveEffort,
  effectiveModel,
  settingsDefaults,
  SettingsSection,
  SettingsStore,
  SOUND_OPTIONS,
} from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { VersionService } from "../shared/version.service";

// ─────────────────────────────────────────────────────────────────────────────
// ORCHESTRA settings surface — faithful Angular port of the KJ design bundle v2
// settings.jsx FINAL state (no allowlist editor / stepper / nav footer; footer =
// reset-all + "changes apply instantly" + Cancel + Done). Instant-apply via
// SettingsStore; per-row reset pills compare against the backend defaults.
//
// The primitives below use classic @Input/@Output (not signal inputs) ON PURPOSE:
// the vitest JIT compiler can't wire signal inputs (NG0950), and these are the
// components the modal's DOM specs render for real.
// ─────────────────────────────────────────────────────────────────────────────

interface SegOption {
  value: string;
  label: string;
}

@Component({
  selector: "app-set-seg",
  changeDetection: ChangeDetectionStrategy.OnPush,
  // every settings primitive ships its slice of the design stylesheet with
  // encapsulation OFF (classes are .set--namespaced; the CSS styles svg glyphs
  // inside the icon component) — and splitting keeps each chunk well under the
  // per-component style budget.
  encapsulation: ViewEncapsulation.None,
  imports: [IconComponent],
  styles: [
    `
.set-seg{display:inline-flex;padding:2px;background:var(--panel-2);border:1px solid var(--hair);
  border-radius:8px;gap:2px;}
.set-seg button{position:relative;display:inline-flex;align-items:center;justify-content:center;gap:6px;
  height:26px;padding:0 12px;border:none;background:transparent;color:var(--ink-3);
  font-family:var(--font-mono);font-size:11.5px;cursor:pointer;border-radius:6px;white-space:nowrap;
  transition:all .12s;}
.set-seg button:hover{color:var(--ink-2);}
.set-seg button.on{color:var(--ink);background:var(--panel-3);box-shadow:0 0 0 1px var(--hair-2);}
.set-seg button.on svg{color:var(--accent);}
.set-seg button svg{width:13px;height:13px;}
.set-seg button.dgr.on{color:var(--set-danger);
  background:color-mix(in oklch,var(--set-danger),transparent 88%);
  box-shadow:0 0 0 1px color-mix(in oklch,var(--set-danger),transparent 52%);}
.set-seg button.dgr.on svg{color:var(--set-danger);}
    `,
  ],
  template: `
    <div class="set-seg" role="radiogroup">
      @for (o of opts; track o.value) {
        <button
          type="button"
          role="radio"
          [attr.aria-checked]="o.value === value"
          [class.on]="o.value === value"
          [class.dgr]="danger !== null && o.value === danger"
          (click)="changed.emit(o.value)"
        >
          @if (danger !== null && o.value === danger) { <app-icon name="flag" size="sm" /> }
          {{ o.label }}
        </button>
      }
    </div>
  `,
})
export class SetSegComponent {
  @Input() value = "";
  @Input() set options(v: ReadonlyArray<string | SegOption>) {
    this.opts = v.map((o) => (typeof o === "string" ? { value: o, label: o } : o));
  }
  /** A value rendered in the danger style (flag icon + red when selected). */
  @Input() danger: string | null = null;
  @Output() readonly changed = new EventEmitter<string>();
  opts: SegOption[] = [];
}

@Component({
  selector: "app-set-tgl",
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  styles: [
    `
.set-tgl{position:relative;width:34px;height:19px;border:none;border-radius:999px;
  background:var(--hair-2);cursor:pointer;padding:0;transition:background .16s;flex:none;}
.set-tgl.on{background:var(--accent);box-shadow:0 0 12px -3px rgba(var(--accent-rgb),.8);}
.set-tgl i{position:absolute;top:2px;left:2px;width:15px;height:15px;border-radius:50%;
  background:#fff;box-shadow:0 1px 3px rgba(0,0,0,.35);transition:transform .16s;}
.set-tgl.on i{transform:translateX(15px);}
    `,
  ],
  template: `
    <button
      type="button"
      class="set-tgl"
      [class.on]="value"
      role="switch"
      [attr.aria-checked]="value"
      [disabled]="disabled"
      (click)="changed.emit(!value)"
    ><i></i></button>
  `,
})
export class SetToggleComponent {
  @Input() value = false;
  @Input() disabled = false;
  @Output() readonly changed = new EventEmitter<boolean>();
}

@Component({
  selector: "app-set-select",
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  styles: [
    `
.set-sel{appearance:none;height:28px;padding:0 28px 0 11px;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:7px;color:var(--ink);font-family:var(--font-mono);
  font-size:12px;cursor:pointer;outline:none;transition:border-color .12s;
  background-image:linear-gradient(45deg,transparent 50%,var(--ink-3) 50%),linear-gradient(135deg,var(--ink-3) 50%,transparent 50%);
  background-position:right 13px center,right 8px center;background-size:5px 5px,5px 5px;background-repeat:no-repeat;}
.set-sel:hover{border-color:var(--hair-2);}
.set-sel:focus{border-color:var(--accent);}
    `,
  ],
  template: `
    <select class="set-sel" [style.width.px]="width" (change)="changed.emit($any($event.target).value)">
      @for (o of opts; track o.value) {
        <option [value]="o.value" [selected]="o.value === value">{{ o.label }}</option>
      }
    </select>
  `,
})
export class SetSelectComponent {
  @Input() value = "";
  @Input() set options(v: ReadonlyArray<string | SegOption>) {
    this.opts = v.map((o) => (typeof o === "string" ? { value: o, label: o } : o));
  }
  @Input() width: number | null = null;
  @Output() readonly changed = new EventEmitter<string>();
  opts: SegOption[] = [];
}

/** Model combobox — curated list + free-text custom override (Enter to apply). */
@Component({
  selector: "app-set-combo",
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [IconComponent],
  styles: [
    `
.set-combo{position:relative;}
.set-combo-btn{display:inline-flex;align-items:center;gap:10px;height:28px;padding:0 9px 0 11px;
  min-width:186px;justify-content:space-between;background:var(--panel-2);border:1px solid var(--hair);
  border-radius:7px;color:var(--ink);font-family:var(--font-mono);font-size:12px;cursor:pointer;transition:all .12s;}
.set-combo-btn:hover{border-color:var(--hair-2);}
.set-combo-btn.open{border-color:var(--accent);box-shadow:0 0 0 3px color-mix(in oklch,var(--accent),transparent 86%);}
.set-combo-btn .cv{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-combo-btn .tag{font-size:8px;letter-spacing:.1em;text-transform:uppercase;color:var(--accent);
  border:1px solid color-mix(in oklch,var(--accent),transparent 60%);border-radius:4px;padding:1px 4px;flex:none;}
.set-combo-btn svg{width:13px;height:13px;color:var(--ink-3);flex:none;transition:transform .15s;}
.set-combo-btn.open svg{transform:rotate(180deg);color:var(--accent);}
.set-combo-pop{position:absolute;top:calc(100% + 6px);right:0;z-index:6;width:248px;
  background:var(--elev);border:1px solid var(--hair-2);border-radius:11px;box-shadow:var(--shadow);
  padding:6px;animation:set-pop .14s ease;}
.set-combo-lbl{font-size:8.5px;letter-spacing:.13em;text-transform:uppercase;color:var(--ink-4);
  padding:4px 8px 5px;}
.set-combo-opt{display:flex;align-items:center;gap:9px;height:31px;padding:0 9px;border-radius:7px;
  color:var(--ink-2);font-size:12px;cursor:pointer;transition:all .1s;}
.set-combo-opt:hover{background:var(--panel-3);color:var(--ink);}
.set-combo-opt.on{color:var(--ink);background:var(--panel-2);}
.set-combo-opt .ck{width:14px;flex:none;color:var(--accent);}
.set-combo-opt .cn{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-combo-custom{margin-top:5px;border-top:1px solid var(--hair);padding-top:7px;}
.set-combo-field{display:flex;align-items:center;gap:7px;height:30px;padding:0 9px;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:7px;}
.set-combo-field:focus-within{border-color:var(--accent);}
.set-combo-field span{color:var(--ink-4);font-size:11px;flex:none;}
.set-combo-field input{flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);font-size:12px;}
.set-combo-field kbd{font-size:8.5px;color:var(--ink-4);border:1px solid var(--hair-2);border-radius:4px;
  padding:1px 5px;flex:none;}
    `,
  ],
  template: `
    <div class="set-combo">
      <button type="button" class="set-combo-btn" [class.open]="open" (click)="openChange.emit(!open)">
        <span class="cv">{{ value }}</span>
        <span style="display:flex;align-items:center;gap:8px;flex:none">
          @if (isCustom) { <span class="tag">custom</span> }
          <app-icon name="chevronD" size="sm" />
        </span>
      </button>
      @if (open) {
        <div class="set-combo-pop">
          <div class="set-combo-lbl">Curated models</div>
          @for (m of options; track m) {
            <div class="set-combo-opt" [class.on]="m === value" (click)="pick(m)">
              <span class="ck">@if (m === value) { <app-icon name="check" size="sm" /> }</span>
              <span class="cn">{{ m }}</span>
            </div>
          }
          <div class="set-combo-custom">
            <div class="set-combo-lbl" style="padding-top:0">Custom — CLIs don’t expose a list</div>
            <div class="set-combo-field">
              <span>›</span>
              <input
                [value]="custom()"
                [placeholder]="isCustom ? value : 'model-id…'"
                (input)="custom.set($any($event.target).value)"
                (keydown.enter)="commit()"
              />
              <kbd>↵</kbd>
            </div>
          </div>
        </div>
      }
    </div>
  `,
})
export class ModelComboComponent {
  @Input() value = "";
  @Input() options: string[] = [];
  @Input() open = false;
  @Output() readonly openChange = new EventEmitter<boolean>();
  @Output() readonly changed = new EventEmitter<string>();
  readonly custom = signal("");

  get isCustom(): boolean {
    return !this.options.includes(this.value);
  }
  pick(m: string): void {
    this.changed.emit(m);
    this.openChange.emit(false);
  }
  commit(): void {
    const v = this.custom().trim();
    if (!v) return;
    this.changed.emit(v);
    this.custom.set("");
    this.openChange.emit(false);
  }
}

/** One settings row: label (+ reset pill when dirty), help, and the projected
 *  control. `wide` stacks the control under the help (full-width controls). */
@Component({
  selector: "app-set-row",
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [IconComponent],
  styles: [
    `
.set-row{display:flex;align-items:flex-start;gap:18px;padding:10px 0;}
/* the control is a flex SIBLING of main (not nested as in the JSX) — gap 3px
   keeps the wide control exactly where the design put it (3px under the help) */
.set-row.wide{flex-direction:column;gap:3px;}
.set-row.dis{opacity:.45;pointer-events:none;}
.set-row-main{flex:1;min-width:0;display:flex;flex-direction:column;gap:3px;}
.set-row.wide .set-row-main{width:100%;}
.set-row-lbl{font-size:12.5px;color:var(--ink);display:flex;align-items:center;gap:8px;line-height:1.2;}
.set-row-help{font-size:10.5px;color:var(--ink-4);line-height:1.5;max-width:46ch;}
.set-row-help code{font-family:var(--font-mono);color:var(--ink-3);background:var(--panel-2);
  padding:0 4px;border-radius:4px;font-size:10px;}
.set-row-ctrl{flex:none;display:flex;align-items:center;gap:8px;padding-top:1px;}
.set-row.wide .set-row-ctrl{width:100%;padding-top:0;}

.set-reset{display:inline-flex;align-items:center;gap:4px;height:18px;padding:0 7px 0 5px;
  border-radius:999px;border:1px solid var(--hair);background:var(--panel-2);color:var(--ink-3);
  font-family:var(--font-mono);font-size:8.5px;letter-spacing:.06em;text-transform:uppercase;
  cursor:pointer;transition:all .12s;flex:none;}
.set-reset:hover{color:var(--accent);border-color:color-mix(in oklch,var(--accent),transparent 50%);
  background:color-mix(in oklch,var(--accent),transparent 90%);}
.set-reset svg{width:9px;height:9px;}
    `,
  ],
  template: `
    <div class="set-row" [class.wide]="wide" [class.dis]="disabled">
      <div class="set-row-main">
        <div class="set-row-lbl">
          <ng-content select="[row-label]" />
          @if (dirty && !disabled) {
            <button type="button" class="set-reset" title="Reset to default" (click)="reset.emit()">
              <app-icon name="refresh" size="sm" />reset
            </button>
          }
        </div>
        <div class="set-row-help"><ng-content select="[row-help]" /></div>
      </div>
      <div class="set-row-ctrl"><ng-content /></div>
    </div>
  `,
})
export class SetRowComponent {
  @Input() wide = false;
  @Input() disabled = false;
  @Input() dirty = false;
  @Output() readonly reset = new EventEmitter<void>();
}

const SECTIONS: ReadonlyArray<{ id: SettingsSection; label: string; icon: string }> = [
  { id: "updates", label: "Updates", icon: "refresh" },
  { id: "agent", label: "Agent defaults", icon: "agent" },
  { id: "perms", label: "Permissions & safety", icon: "lock" },
  { id: "notif", label: "Notifications", icon: "bell" },
];

const SUBS: Record<SettingsSection, string> = {
  updates: "Channel, version & install behavior",
  agent: "Defaults for newly spawned agents",
  perms: "Auto-approval & remote control",
  notif: "OS alerts, events & sound",
};

const EVENTS: ReadonlyArray<{ k: keyof SettingsEvents; label: string; help: string }> = [
  { k: "finished", label: "Finished", help: "An agent completed its task." },
  { k: "question", label: "Question", help: "An agent asked a decision question." },
  { k: "permission", label: "Permission", help: "An agent needs approval to run a command." },
  { k: "error", label: "Error", help: "An agent hit an error or crashed." },
];

const RELEASES_URL = "https://github.com/kouji-dev/orrery-releases/releases";

@Component({
  selector: "app-settings-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  // The design ships one flat `.set-`-prefixed stylesheet that also styles svg
  // glyphs INSIDE the icon/badge child components — encapsulation off keeps the
  // pixel-level CSS working verbatim (everything is namespaced under .set-).
  encapsulation: ViewEncapsulation.None,
  imports: [IconComponent, ToolBadgeComponent, SetSegComponent, SetToggleComponent, SetSelectComponent, ModelComboComponent, SetRowComponent],
  template: `
    @let s = store.settings();
    <div class="set-backdrop" (mousedown)="close()">
      <div class="set-modal" role="dialog" aria-label="Settings" (mousedown)="onModalDown($event)">
        <!-- nav -->
        <nav class="set-nav">
          <div class="set-brand">
            <span class="gi"><app-icon name="settings" size="sm" /></span>
            <div style="min-width:0">
              <div class="bt">Settings</div>
              <div class="bs">Orrery preferences</div>
            </div>
          </div>
          <div class="set-nav-list">
            @for (sec of sections; track sec.id) {
              <button type="button" class="set-nav-item" [class.on]="section() === sec.id" (click)="selectSection(sec.id)">
                <app-icon [name]="sec.icon" size="sm" />
                <span class="lb">{{ sec.label }}</span>
                @if (sec.id === 'updates' && store.updateKnown()) { <span class="set-nav-dot" title="Update available"></span> }
              </button>
            }
          </div>
        </nav>

        <!-- main -->
        <div class="set-main">
          <div class="set-head">
            <div>
              <div class="ht">{{ current().label }}</div>
              <div class="hs">{{ subs[section()] }}</div>
            </div>
            <button type="button" class="set-x" aria-label="Close settings" (click)="close()"><app-icon name="x" size="sm" /></button>
          </div>

          <div class="set-body">
            @switch (section()) {
              <!-- ── Updates ─────────────────────────────────────────────── -->
              @case ("updates") {
                <div class="set-grp">
                  <div class="set-grp-h">Release channel<span class="ln"></span></div>
                  <app-set-row [dirty]="s.channel !== D.channel" (reset)="store.set({ channel: D.channel })">
                    <ng-container row-label>Channel</ng-container>
                    <ng-container row-help>Beta receives pre-release builds first.</ng-container>
                    <app-set-seg [value]="s.channel" [options]="['stable', 'beta']" (changed)="store.set({ channel: $any($event) })" />
                  </app-set-row>
                  @if (s.channel === 'beta') {
                    <div class="set-warn" style="margin-top:2px">
                      <app-icon name="flag" size="sm" />
                      Pre-release builds — may be unstable or break worktrees. Roll back from this panel anytime.
                    </div>
                  }
                  <app-set-row [dirty]="s.updatePolicy !== D.updatePolicy" (reset)="store.set({ updatePolicy: D.updatePolicy })">
                    <ng-container row-label>Install policy</ng-container>
                    <ng-container row-help>What Orrery does when a new build is available.</ng-container>
                    <app-set-seg [value]="s.updatePolicy" [options]="policyOptions" (changed)="store.set({ updatePolicy: $any($event) })" />
                  </app-set-row>
                </div>

                <div class="set-grp">
                  <div class="set-grp-h">Version<span class="ln"></span></div>
                  <app-set-row>
                    <ng-container row-label>Current build</ng-container>
                    <ng-container row-help>
                      Orrery <code>v{{ version.version() || '—' }}</code> · {{ s.channel }} channel · checked
                      {{ store.checking() ? 'now…' : lastChecked() }}
                    </ng-container>
                    <div style="display:flex;align-items:center;gap:8px">
                      <span class="set-vchip tnum">v{{ version.version() || '—' }}<b>·</b>{{ s.channel === 'beta' ? 'BETA' : 'STABLE' }}</span>
                      <button class="btn ghost-hair" style="padding:5px 11px" [disabled]="store.checking()" (click)="store.checkNow()">
                        <app-icon name="refresh" size="sm" [class.set-spin]="store.checking()" />
                        {{ store.checking() ? 'Checking…' : 'Check now' }}
                      </button>
                    </div>
                  </app-set-row>

                  @if (store.updateCard(); as upd) {
                    <app-set-row [wide]="true">
                      <ng-container row-label>Update available</ng-container>
                      <ng-container row-help>A newer build is ready to install.</ng-container>
                      <div class="set-upd">
                        <div class="set-upd-top">
                          <span class="set-upd-ic"><app-icon name="stage" /></span>
                          <div class="set-upd-tt">
                            <div class="u1">Orrery <span class="set-upd-ver">v{{ upd.version }}</span></div>
                            <div class="u2">@if (upd.date) { released {{ upd.date }} · } upgrades from v{{ version.version() || '—' }}</div>
                          </div>
                        </div>
                        <a class="set-upd-notes" [href]="releasesUrl" (click)="openNotes($event)">
                          <app-icon name="file" size="sm" />Read release notes<app-icon name="ext" size="sm" />
                        </a>
                        <div class="set-upd-act">
                          <button class="btn primary" [disabled]="store.installing()" (click)="store.install()">
                            <app-icon name="stage" size="sm" />{{ store.installing() ? 'Installing…' : 'Install & relaunch' }}
                          </button>
                          <button class="btn ghost-hair" (click)="store.dismissUpdate()">Later</button>
                        </div>
                      </div>
                    </app-set-row>
                  }
                </div>
              }

              <!-- ── Agent defaults ──────────────────────────────────────── -->
              @case ("agent") {
                <div class="set-grp">
                  <div class="set-grp-h">Default agent<span class="ln"></span></div>
                  <app-set-row [wide]="true" [dirty]="s.defaultTool !== D.defaultTool" (reset)="store.set({ defaultTool: D.defaultTool })">
                    <ng-container row-label>Default tool</ng-container>
                    <ng-container row-help>Used when you spawn without picking one. Detected CLIs on your PATH are selectable.</ng-container>
                    <div class="set-tools">
                      @for (tl of tools; track tl.id) {
                        @let detected = runtime.toolAvailable(tl.id);
                        @let on = s.defaultTool === tl.id;
                        <button type="button" class="set-tool" [class.on]="on" [class.off]="!detected" [disabled]="!detected" (click)="store.set({ defaultTool: tl.id })">
                          <app-tool-badge [tool]="tl.id" [size]="22" />
                          <div class="tn">{{ tl.name }}</div>
                          <div class="ts">
                            @if (detected) { <span class="dot done" style="width:5px;height:5px"></span>detected } @else { binary missing }
                          </div>
                          @if (on) { <span class="pick"><app-icon name="check" size="sm" [px]="11" /></span> }
                          @if (!detected) { <span class="nf">not found</span> }
                        </button>
                      }
                    </div>
                  </app-set-row>

                  <app-set-row [dirty]="s.toolModel[modelTool().id] !== undefined" (reset)="store.setMap('toolModel', modelTool().id, null)">
                    <ng-container row-label>Model <span style="color:var(--ink-4);font-weight:400">· {{ modelTool().name }}</span></ng-container>
                    <ng-container row-help>Curated per tool, with a free-text override — Enter to apply a custom id.</ng-container>
                    <app-set-combo
                      [value]="effModel()"
                      [options]="modelTool().models"
                      [open]="modelOpen()"
                      (openChange)="modelOpen.set($event)"
                      (changed)="store.setMap('toolModel', modelTool().id, $event)"
                    />
                  </app-set-row>

                  <app-set-row [dirty]="s.toolEffort[modelTool().id] !== undefined" (reset)="store.setMap('toolEffort', modelTool().id, null)">
                    <ng-container row-label>Reasoning effort</ng-container>
                    <ng-container row-help>
                      @if (effortOptions(); as eo) { How hard the model thinks before acting. } @else { {{ modelTool().name }} doesn’t expose an effort setting. }
                    </ng-container>
                    @if (effortOptions(); as eo) {
                      <app-set-seg [value]="effEffort()" [options]="eo" (changed)="store.setMap('toolEffort', modelTool().id, $event)" />
                    } @else {
                      <span class="set-muted"><app-icon name="dots" size="sm" />not supported</span>
                    }
                  </app-set-row>
                </div>

                <div class="set-grp">
                  <div class="set-grp-h">Worktrees<span class="ln"></span></div>
                  <app-set-row [wide]="true" [dirty]="s.branchTemplate !== D.branchTemplate" (reset)="store.set({ branchTemplate: D.branchTemplate })">
                    <ng-container row-label>Branch template</ng-container>
                    <ng-container row-help>Tokens: <code>{{ '{name}' }}</code> <code>{{ '{tool}' }}</code> <code>{{ '{date}' }}</code></ng-container>
                    <div style="display:flex;flex-direction:column;gap:0;width:100%">
                      <div class="set-text" style="max-width:320px">
                        <app-icon name="branch" size="sm" />
                        <input [value]="s.branchTemplate" spellcheck="false" (input)="store.set({ branchTemplate: $any($event.target).value })" />
                      </div>
                      <div class="set-preview">
                        <span class="arr">preview</span><app-icon name="chevron" size="sm" [px]="11" /><b>{{ branchPreview() }}</b>
                      </div>
                    </div>
                  </app-set-row>

                  <app-set-row [dirty]="s.worktreeRoot !== D.worktreeRoot" (reset)="store.set({ worktreeRoot: D.worktreeRoot })">
                    <ng-container row-label>Worktree root</ng-container>
                    <ng-container row-help>Where new agent worktrees are created on disk.</ng-container>
                    <div style="display:flex;align-items:center;gap:8px;width:300px">
                      <div class="set-path"><app-icon name="folder" size="sm" /><span class="pt">{{ s.worktreeRoot || 'app data · worktrees' }}</span></div>
                      <button class="btn ghost-hair" style="flex:none;padding:5px 11px" (click)="browse()"><app-icon name="folderOpen" size="sm" />Browse</button>
                    </div>
                  </app-set-row>

                  <app-set-row [dirty]="s.autoResume !== D.autoResume" (reset)="store.set({ autoResume: D.autoResume })">
                    <ng-container row-label>Auto-resume on restart</ng-container>
                    <ng-container row-help>Re-attach to running agent sessions when Orrery relaunches.</ng-container>
                    <app-set-tgl [value]="s.autoResume" (changed)="store.set({ autoResume: $event })" />
                  </app-set-row>
                </div>
              }

              <!-- ── Permissions & safety ────────────────────────────────── -->
              @case ("perms") {
                <div class="set-grp">
                  <div class="set-grp-h">Auto-approve policy<span class="ln"></span></div>
                  @for (tl of detectedTools(); track tl.id) {
                    @let val = approveOf(tl.id);
                    <app-set-row [dirty]="val !== 'off'" (reset)="resetApprove(tl.id)">
                      <ng-container row-label>
                        <span style="display:inline-flex;align-items:center;gap:8px"><app-tool-badge [tool]="tl.id" [size]="16" />{{ tl.name }}</span>
                      </ng-container>
                      <ng-container row-help>{{ approveHelp(val) }}</ng-container>
                      <app-set-seg [value]="val" [options]="approveOptions" danger="everything" (changed)="pickPolicy(tl.id, $any($event))" />
                    </app-set-row>
                    @if (confirm()?.tool === tl.id) {
                      <div class="set-danger">
                        <div class="set-danger-h"><app-icon name="flag" />Allow {{ tl.name }} to run <b style="color:inherit">everything</b>?</div>
                        <div class="set-danger-b">
                          Auto-approving <b>every</b> command lets {{ tl.name }} run destructive operations —
                          <code style="font-family:var(--font-mono)">rm&nbsp;-rf</code>, force-push, network calls — with no prompt.
                          Recommended only for fully sandboxed worktrees.
                        </div>
                        <div class="set-danger-act">
                          <button class="btn ghost-hair" (click)="cancelEverything(tl.id)">Cancel</button>
                          <button type="button" class="set-btn-danger" (click)="confirm.set(null)"><app-icon name="flag" size="sm" />Enable “Everything”</button>
                        </div>
                      </div>
                    }
                  }
                </div>

                <div class="set-grp">
                  <div class="set-grp-h">Remote approval<span class="ln"></span></div>
                  <app-set-row [dirty]="s.remoteApproval !== D.remoteApproval" (reset)="store.set({ remoteApproval: D.remoteApproval })">
                    <ng-container row-label>Approve from notifications</ng-container>
                    <ng-container row-help>Answer permission prompts straight from OS notifications.</ng-container>
                    <app-set-tgl [value]="s.remoteApproval" (changed)="store.set({ remoteApproval: $event })" />
                  </app-set-row>
                </div>
              }

              <!-- ── Notifications ───────────────────────────────────────── -->
              @case ("notif") {
                @let off = !s.osNotifications;
                <div class="set-grp">
                  <div class="set-grp-h">Delivery<span class="ln"></span></div>
                  <app-set-row [dirty]="s.osNotifications !== D.osNotifications" (reset)="store.set({ osNotifications: D.osNotifications })">
                    <ng-container row-label>Native OS notifications</ng-container>
                    <ng-container row-help>Off keeps all alerts inside the app only.</ng-container>
                    <app-set-tgl [value]="s.osNotifications" (changed)="store.set({ osNotifications: $event })" />
                  </app-set-row>
                </div>

                <div class="set-grp">
                  <div class="set-grp-h">Events<span class="ln"></span></div>
                  @for (ev of events; track ev.k) {
                    <app-set-row [disabled]="off" [dirty]="s.events[ev.k] !== D.events[ev.k]" (reset)="store.setEvent(ev.k, D.events[ev.k])">
                      <ng-container row-label>{{ ev.label }}</ng-container>
                      <ng-container row-help>{{ ev.help }}</ng-container>
                      <app-set-tgl [value]="s.events[ev.k]" [disabled]="off" (changed)="store.setEvent(ev.k, $event)" />
                    </app-set-row>
                  }
                </div>

                <div class="set-grp">
                  <div class="set-grp-h">Sound<span class="ln"></span></div>
                  <app-set-row [disabled]="off" [dirty]="s.sound !== D.sound" (reset)="store.set({ sound: D.sound })">
                    <ng-container row-label>Play sound</ng-container>
                    <ng-container row-help>A short cue when a notification fires.</ng-container>
                    <app-set-tgl [value]="s.sound" [disabled]="off" (changed)="store.set({ sound: $event })" />
                  </app-set-row>
                  <app-set-row
                    [disabled]="off || !s.sound"
                    [dirty]="s.soundName !== D.soundName || s.volume !== D.volume"
                    (reset)="store.set({ soundName: D.soundName, volume: D.volume })"
                  >
                    <ng-container row-label>Cue &amp; volume</ng-container>
                    <ng-container row-help>Notification tone and loudness.</ng-container>
                    <div style="display:flex;align-items:center;gap:10px">
                      <app-set-select [value]="s.soundName" [options]="soundOptions" (changed)="store.set({ soundName: $event })" />
                      <app-icon name="volume" size="sm" color="var(--ink-4)" />
                      <input type="range" class="set-slider" min="0" max="100" [value]="s.volume" (input)="store.set({ volume: +$any($event.target).value })" />
                      <span class="tnum" style="font-size:11px;color:var(--ink-3);width:30px;text-align:right">{{ s.volume }}%</span>
                    </div>
                  </app-set-row>
                </div>
              }
            }
          </div>

          <div class="set-foot">
            @if (store.anyDirty()) {
              <button type="button" class="reset-all" (click)="resetAll()"><app-icon name="refresh" size="sm" />Reset all to defaults</button>
            } @else {
              <span class="fl"><span class="fd"></span>Changes apply instantly</span>
            }
            <span class="sp"></span>
            <button class="btn ghost-hair" style="padding:5px 14px" (click)="close()">Cancel</button>
            <button type="button" class="set-done" (click)="close()"><app-icon name="check" size="sm" />Done</button>
          </div>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
/* ── ORCHESTRA settings — scoped by the .set- prefix (encapsulation: None) ── */
.set-backdrop{position:fixed;inset:0;z-index:80;display:grid;place-items:center;padding:28px;
  background:rgba(4,5,9,.58);-webkit-backdrop-filter:blur(4px);backdrop-filter:blur(4px);}
[data-theme="light"] .set-backdrop{background:rgba(20,24,40,.34);}

.set-modal{--set-amber:#f5c451;--set-danger:#ff5d7a;
  width:760px;max-width:calc(100vw - 56px);height:600px;max-height:84vh;display:flex;
  background:var(--panel);border:1px solid var(--hair-2);border-radius:15px;overflow:hidden;
  box-shadow:var(--shadow),0 0 0 1px rgba(var(--accent-rgb),.05);
  font-family:var(--font-mono);color:var(--ink);
  transform-origin:center;animation:set-pop .22s cubic-bezier(.2,.7,.2,1);}
[data-theme="light"] .set-modal{--set-amber:#a9700f;--set-danger:#d6304e;}
/* transform-only entrance: if the frame is throttled and the animation freezes
   at 0%, content stays visible (just offset) instead of stuck at opacity:0 */
@keyframes set-pop{from{transform:translateY(10px) scale(.99)}to{transform:none}}
@media (prefers-reduced-motion:reduce){.set-modal{animation:none}}
.set-spin{animation:set-spin .9s linear infinite;}
@keyframes set-spin{to{transform:rotate(360deg)}}

/* ── left nav ── */
.set-nav{width:204px;flex:none;background:var(--panel-2);border-right:1px solid var(--hair);
  display:flex;flex-direction:column;padding:13px 11px;}
.set-brand{display:flex;align-items:center;gap:9px;padding:5px 8px 14px;}
.set-brand .gi{width:26px;height:26px;border-radius:8px;display:grid;place-items:center;flex:none;
  color:var(--accent);background:color-mix(in oklch,var(--accent),transparent 86%);
  box-shadow:inset 0 0 0 1px color-mix(in oklch,var(--accent),transparent 64%);}
.set-brand .bt{font-family:var(--font-disp);font-size:14px;font-weight:600;letter-spacing:-.01em;}
.set-brand .bs{font-size:9px;color:var(--ink-4);letter-spacing:.04em;}
.set-nav-list{display:flex;flex-direction:column;gap:2px;}
.set-nav-item{position:relative;display:flex;align-items:center;gap:10px;height:35px;padding:0 11px;
  border-radius:9px;color:var(--ink-3);cursor:pointer;font-size:12.5px;border:1px solid transparent;
  transition:background .12s,color .12s,border-color .12s;text-align:left;background:transparent;
  font-family:var(--font-mono);width:100%;}
.set-nav-item:hover{background:var(--panel-3);color:var(--ink-2);}
.set-nav-item.on{background:var(--panel-3);color:var(--ink);border-color:var(--hair-2);}
.set-nav-item svg{width:15px;height:15px;flex:none;color:var(--ink-4);transition:color .12s;}
.set-nav-item:hover svg,.set-nav-item.on svg{color:var(--accent);}
.set-nav-item .lb{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-nav-item.on::before{content:"";position:absolute;left:-11px;top:9px;bottom:9px;width:2.5px;
  border-radius:2px;background:linear-gradient(var(--accent),var(--accent-2));}
.set-nav-dot{width:6px;height:6px;border-radius:50%;background:var(--set-amber);flex:none;
  box-shadow:0 0 7px -1px var(--set-amber);}

/* ── right column ── */
.set-main{flex:1;min-width:0;display:flex;flex-direction:column;background:var(--panel);}
.set-head{flex:none;display:flex;align-items:center;gap:11px;padding:15px 16px 14px;
  border-bottom:1px solid var(--hair);}
.set-head .ht{font-family:var(--font-disp);font-size:15.5px;font-weight:600;letter-spacing:-.01em;}
.set-head .hs{font-size:10px;color:var(--ink-4);margin-top:2px;}
.set-x{margin-left:auto;flex:none;width:28px;height:28px;border-radius:7px;border:1px solid transparent;
  background:transparent;color:var(--ink-3);cursor:pointer;display:grid;place-items:center;transition:all .12s;}
.set-x:hover{background:var(--panel-3);color:var(--ink);border-color:var(--hair);}

.set-body{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;padding:4px 18px 20px;}
.set-body::-webkit-scrollbar{width:9px;}
.set-body::-webkit-scrollbar-thumb{background:var(--hair-2);border-radius:6px;border:2px solid transparent;background-clip:padding-box;}
.set-body::-webkit-scrollbar-thumb:hover{background:var(--ink-4);background-clip:padding-box;}

.set-grp{padding:15px 0 16px;border-bottom:1px solid var(--hair);}
.set-grp:last-child{border-bottom:none;}
.set-grp-h{font-size:9.5px;letter-spacing:.13em;text-transform:uppercase;color:var(--ink-3);
  margin-bottom:6px;display:flex;align-items:center;gap:8px;}
.set-grp-h .ln{flex:1;height:1px;background:var(--hair);}

/* (row / reset-pill / seg / toggle / select / combo styles live on their own
   sub-components below-in-file — same flat .set- vocabulary, encapsulation off) */

/* ── tool select grid (agent default tool) ── */
.set-tools{display:grid;grid-template-columns:repeat(4,1fr);gap:8px;width:100%;}
.set-tool{position:relative;display:flex;flex-direction:column;align-items:flex-start;gap:9px;
  padding:11px 11px 10px;border-radius:11px;border:1px solid var(--hair);background:var(--panel-2);
  color:var(--ink-3);cursor:pointer;transition:all .13s;text-align:left;font-family:var(--font-mono);}
.set-tool:hover:not(.off){border-color:var(--hair-2);color:var(--ink-2);transform:translateY(-1px);}
.set-tool.on{color:var(--ink);border-color:color-mix(in oklch,var(--accent),transparent 42%);
  background:color-mix(in oklch,var(--accent),transparent 88%);
  box-shadow:0 0 18px -8px rgba(var(--accent-rgb),.7);}
.set-tool.off{opacity:.5;cursor:not-allowed;}
.set-tool .tn{font-size:11.5px;font-weight:500;}
.set-tool .ts{font-size:9px;color:var(--ink-4);display:flex;align-items:center;gap:4px;}
.set-tool .pick{position:absolute;top:9px;right:9px;width:15px;height:15px;border-radius:50%;
  display:grid;place-items:center;background:var(--accent);color:#06070b;}
[data-theme="light"] .set-tool .pick{color:#fff;}
.set-tool .nf{position:absolute;top:10px;right:10px;font-size:8px;letter-spacing:.08em;text-transform:uppercase;
  color:var(--ink-4);border:1px solid var(--hair);border-radius:4px;padding:1px 4px;}

/* ── chips / amber / fields ── */
.set-warn{display:inline-flex;align-items:center;gap:8px;padding:7px 11px;border-radius:8px;font-size:10.5px;
  color:var(--set-amber);background:color-mix(in oklch,var(--set-amber),transparent 88%);
  border:1px solid color-mix(in oklch,var(--set-amber),transparent 60%);line-height:1.4;}
.set-warn svg{width:13px;height:13px;flex:none;}
.set-vchip{display:inline-flex;align-items:center;gap:6px;height:22px;padding:0 9px;border-radius:999px;
  border:1px solid var(--hair);background:var(--panel-2);color:var(--ink-2);font-size:10.5px;
  font-variant-numeric:tabular-nums;}
.set-vchip b{color:var(--ink);font-weight:600;}
.set-muted{font-size:11px;color:var(--ink-4);display:inline-flex;align-items:center;gap:7px;}

.set-text{display:flex;align-items:center;gap:8px;height:30px;padding:0 11px;background:var(--panel-2);
  border:1px solid var(--hair);border-radius:8px;min-width:0;transition:border-color .12s;}
.set-text:focus-within{border-color:var(--accent);}
.set-text input{flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);font-size:12.5px;}
.set-text svg{width:13px;height:13px;color:var(--ink-4);flex:none;}
.set-preview{display:flex;align-items:center;gap:8px;font-size:11px;color:var(--ink-4);margin-top:2px;}
.set-preview b{color:var(--accent-2);font-weight:500;}
.set-preview .arr{color:var(--ink-4);}

.set-path{flex:1;min-width:0;display:flex;align-items:center;gap:8px;height:30px;padding:0 11px;
  background:var(--panel-2);border:1px solid var(--hair);border-radius:8px;color:var(--ink-2);
  font-size:12px;overflow:hidden;}
.set-path .pt{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-path svg{width:13px;height:13px;color:var(--ink-4);flex:none;}

/* ── update available card ── */
.set-upd{display:flex;flex-direction:column;gap:13px;padding:15px;border-radius:12px;width:100%;
  background:color-mix(in oklch,var(--accent),transparent 92%);
  border:1px solid color-mix(in oklch,var(--accent),transparent 70%);}
.set-upd-top{display:flex;align-items:center;gap:11px;}
.set-upd-ic{width:32px;height:32px;border-radius:9px;flex:none;display:grid;place-items:center;color:var(--accent);
  background:color-mix(in oklch,var(--accent),transparent 84%);
  box-shadow:inset 0 0 0 1px color-mix(in oklch,var(--accent),transparent 58%);}
.set-upd-tt{flex:1;min-width:0;}
.set-upd-tt .u1{font-size:12.5px;color:var(--ink);font-weight:500;display:flex;align-items:center;gap:8px;}
.set-upd-tt .u2{font-size:10px;color:var(--ink-3);margin-top:3px;font-variant-numeric:tabular-nums;}
.set-upd-ver{font-family:var(--font-disp);font-size:15px;font-weight:600;color:var(--accent);letter-spacing:-.01em;}
.set-upd-notes{display:inline-flex;align-items:center;gap:5px;font-size:10.5px;color:var(--accent-2);
  text-decoration:none;border-bottom:1px solid color-mix(in oklch,var(--accent-2),transparent 70%);
  padding-bottom:1px;align-self:flex-start;}
.set-upd-notes:hover{border-color:var(--accent-2);}
.set-upd-notes svg{width:11px;height:11px;}
.set-upd-act{display:flex;gap:8px;}

/* ── danger confirm ── */
.set-danger{margin-top:12px;width:100%;padding:14px;border-radius:11px;display:flex;flex-direction:column;gap:11px;
  border:1px solid var(--set-danger);background:color-mix(in oklch,var(--set-danger),transparent 90%);
  box-shadow:0 0 0 3px color-mix(in oklch,var(--set-danger),transparent 88%);animation:set-pop .16s ease;}
.set-danger-h{display:flex;align-items:center;gap:9px;color:var(--set-danger);font-size:12.5px;font-weight:600;}
.set-danger-h svg{width:15px;height:15px;flex:none;}
.set-danger-b{font-size:11px;color:var(--ink-2);line-height:1.55;}
.set-danger-b b{color:var(--ink);font-weight:600;}
.set-danger-act{display:flex;gap:8px;justify-content:flex-end;}
.set-btn-danger{display:inline-flex;align-items:center;gap:6px;height:30px;padding:0 13px;border-radius:7px;
  border:none;background:var(--set-danger);color:#fff;font-family:var(--font-mono);font-size:11.5px;
  font-weight:600;cursor:pointer;transition:filter .12s;}
.set-btn-danger:hover{filter:brightness(1.08);}
.set-btn-danger svg{width:13px;height:13px;}

/* ── slider (volume) ── */
.set-slider{appearance:none;-webkit-appearance:none;width:128px;height:4px;border-radius:999px;
  background:var(--hair-2);outline:none;cursor:pointer;}
.set-slider::-webkit-slider-thumb{-webkit-appearance:none;appearance:none;width:14px;height:14px;border-radius:50%;
  background:var(--accent);border:2px solid var(--panel);box-shadow:0 0 0 1px var(--hair-2);cursor:pointer;}
.set-slider::-moz-range-thumb{width:14px;height:14px;border-radius:50%;background:var(--accent);
  border:2px solid var(--panel);box-shadow:0 0 0 1px var(--hair-2);cursor:pointer;}

/* ── footer ── */
.set-foot{flex:none;display:flex;align-items:center;gap:10px;padding:11px 16px;border-top:1px solid var(--hair);
  background:var(--panel-2);font-size:10.5px;color:var(--ink-3);}
.set-foot .fl{display:inline-flex;align-items:center;gap:7px;}
.set-foot .fd{width:6px;height:6px;border-radius:50%;background:var(--st-done);flex:none;}
.set-foot .reset-all{display:inline-flex;align-items:center;gap:5px;background:transparent;border:none;
  color:var(--ink-3);font-family:var(--font-mono);font-size:10.5px;cursor:pointer;padding:0;}
.set-foot .reset-all:hover{color:var(--accent);}
.set-foot .reset-all svg{width:11px;height:11px;}
.set-foot .sp{flex:1;}
.set-done{display:inline-flex;align-items:center;gap:6px;height:28px;padding:0 14px;border-radius:7px;
  border:none;font-family:var(--font-mono);font-size:11.5px;font-weight:600;cursor:pointer;color:#06070b;
  background:linear-gradient(180deg,var(--accent),color-mix(in oklch,var(--accent),#000 14%));
  box-shadow:0 0 18px -7px rgba(var(--accent-rgb),.7);}
[data-theme="light"] .set-done{color:#fff;}
.set-done:hover{filter:brightness(1.07);}
    `,
  ],
})
export class SettingsModalComponent {
  readonly store = inject(SettingsStore);
  readonly runtime = inject(AgentRuntimeService);
  readonly version = inject(VersionService);
  private readonly bridge = inject(BRIDGE);

  readonly tools = AGENT_TOOLS;
  readonly sections = SECTIONS;
  readonly subs = SUBS;
  readonly events = EVENTS;
  readonly soundOptions = SOUND_OPTIONS;
  readonly releasesUrl = RELEASES_URL;
  readonly D = settingsDefaults();
  readonly policyOptions: SegOption[] = [
    { value: "auto", label: "Auto-install" },
    { value: "notify", label: "Notify only" },
    { value: "manual", label: "Manual" },
  ];
  readonly approveOptions: SegOption[] = [
    { value: "off", label: "Off" },
    { value: "allowlist", label: "Allowlist only" },
    { value: "everything", label: "Everything" },
  ];

  readonly section = signal<SettingsSection>(this.store.openSection());
  readonly modelOpen = signal(false);
  readonly confirm = signal<{ tool: string; prev: AutoApprovePolicy } | null>(null);

  readonly current = computed(() => SECTIONS.find((x) => x.id === this.section()) ?? SECTIONS[0]);
  /** The tool whose model/effort the Agent section edits — the picked default
   *  tool, falling back to the first curated one (design parity). */
  readonly modelTool = computed(
    () => AGENT_TOOLS.find((t) => t.id === this.store.settings().defaultTool) ?? AGENT_TOOLS[0],
  );
  readonly effModel = computed(() => effectiveModel(this.store.settings(), this.modelTool().id));
  readonly effEffort = computed(() => effectiveEffort(this.store.settings(), this.modelTool().id));
  readonly effortOptions = computed<string[] | null>(() => {
    const e = this.modelTool().effort;
    return e ? e : null;
  });
  readonly detectedTools = computed(() => AGENT_TOOLS.filter((t) => this.runtime.toolAvailable(t.id)));
  readonly branchPreview = computed(() => {
    const s = this.store.settings();
    const d = new Date();
    const mmdd = String(d.getMonth() + 1).padStart(2, "0") + String(d.getDate()).padStart(2, "0");
    const out = s.branchTemplate
      .replace(/\{name\}/g, "fix-login")
      .replace(/\{tool\}/g, s.defaultTool || "claude")
      .replace(/\{date\}/g, mmdd);
    return out || "—";
  });

  // ── shell ──
  selectSection(id: SettingsSection): void {
    this.section.set(id);
    this.modelOpen.set(false);
  }
  close(): void {
    this.store.closeModal();
  }
  /** Esc closes the model combo first, then the modal. */
  @HostListener("document:keydown.escape")
  onEsc(): void {
    if (this.modelOpen()) this.modelOpen.set(false);
    else this.close();
  }
  /** Clicks inside the modal don't reach the backdrop; a click outside an OPEN
   *  combo closes the popover (the design's document-level outside-click). */
  onModalDown(e: MouseEvent): void {
    e.stopPropagation();
    if (this.modelOpen() && !(e.target as HTMLElement | null)?.closest(".set-combo")) {
      this.modelOpen.set(false);
    }
  }

  // ── updates ──
  lastChecked(): string {
    const ts = this.store.lastCheckedAt();
    if (ts === null) return "—";
    const m = Math.round((Date.now() - ts) / 60_000);
    if (m < 1) return "just now";
    if (m < 60) return `${m}m ago`;
    return `${Math.round(m / 60)}h ago`;
  }
  openNotes(e: Event): void {
    e.preventDefault();
    // `notes` is the manifest's BODY TEXT (make-latest-json.mjs), not a URL —
    // only treat it as a link target when it actually is one.
    const notes = this.store.updateInfo()?.notes;
    const url = notes && /^https?:\/\//.test(notes) ? notes : this.releasesUrl;
    // window.open is blocked inside the Tauri webview — route through the opener.
    import("@tauri-apps/plugin-opener")
      .then((m) => m.openUrl(url))
      .catch(() => window.open(url, "_blank"));
  }

  // ── agent defaults ──
  async browse(): Promise<void> {
    try {
      const dir = await this.bridge.pickDirectory();
      if (dir) this.store.set({ worktreeRoot: dir });
    } catch {
      /* picker unavailable (plain browser) — ignore */
    }
  }

  // ── permissions ──
  approveOf(tool: string): AutoApprovePolicy {
    return this.store.settings().autoApprove[tool] ?? "off";
  }
  approveHelp(val: AutoApprovePolicy): string {
    if (val === "off") return "Ask before every command.";
    if (val === "allowlist") return "Auto-run allowlisted commands; ask for the rest.";
    return "Run any command without asking.";
  }
  /** "Everything" applies immediately (instant-apply) but opens the danger
   *  confirm; Cancel reverts to the previous policy. */
  pickPolicy(tool: string, v: AutoApprovePolicy): void {
    if (v === "everything" && this.approveOf(tool) !== "everything") {
      this.confirm.set({ tool, prev: this.approveOf(tool) });
      this.store.setMap("autoApprove", tool, "everything");
      return;
    }
    if (this.confirm()?.tool === tool) this.confirm.set(null);
    this.store.setMap("autoApprove", tool, v);
  }
  cancelEverything(tool: string): void {
    const c = this.confirm();
    if (c?.tool === tool) this.store.setMap("autoApprove", tool, c.prev);
    this.confirm.set(null);
  }
  resetApprove(tool: string): void {
    if (this.confirm()?.tool === tool) this.confirm.set(null);
    this.store.setMap("autoApprove", tool, null);
  }

  // ── footer ──
  resetAll(): void {
    this.store.resetAll();
    this.confirm.set(null);
  }
}
