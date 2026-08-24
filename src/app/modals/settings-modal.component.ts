import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  EventEmitter,
  inject,
  Input,
  Output,
  signal,
  ViewEncapsulation,
} from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { BRIDGE } from "../data-source/bridge";
import { AutoApprovePolicy, CostRate, SettingsEvents } from "../models";
import { COST_FEATURES_ENABLED } from "../cost/cost-flags";
import { DEFAULT_RATES } from "../cost/estimate.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AppCommand, CommandRegistryService } from "../commands/command-registry.service";
import { bindingFromEvent, kbdLabel } from "../commands/fuzzy";
import { DiagnosticsService } from "../shared/diagnostics.service";
import {
  effectiveEffort,
  effectiveModel,
  settingsDefaults,
  SettingsSection,
  SettingsStore,
  SOUND_OPTIONS,
} from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { SelectComponent } from "../shared/select.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { RuntimeRowComponent } from "./runtime-row.component";
import { NotificationAlertService } from "../notifications/notification-alert.service";
import { VersionService } from "../shared/version.service";
import { RELEASES_URL } from "../shared/links";
import {
  KjBadgeComponent,
  KjButtonComponent,
  KjComboboxComponent,
  KjComboboxEmptyComponent,
  KjComboboxOptionComponent,
  KjKbdComponent,
  KjNumberInputComponent,
  KjProgressBarComponent,
  KjSliderComponent,
  KjTabComponent,
  KjTabListComponent,
  KjTabsComponent,
  KjListComponent,
  KjListItemComponent,
  KjSpinnerComponent,
  KjToggleComponent,
} from "@kouji-ui/components";

// ─────────────────────────────────────────────────────────────────────────────
// ORCHESTRA settings surface — faithful Angular port of the KJ design bundle v2
// settings.jsx FINAL state (no allowlist editor / stepper / nav footer; footer =
// reset-all + "changes apply instantly" + Cancel + Done). Instant-apply via
// SettingsStore; per-row reset pills compare against the backend defaults.
//
// Controls are kouji primitives (kj-toggle / kj-button / kj-combobox /
// kj-number-input / kj-slider / app-select); only SetRowComponent remains a
// local primitive. It uses classic @Input/@Output (not signal inputs) ON
// PURPOSE: the vitest JIT compiler can't wire signal inputs (NG0950), and it's
// a component the modal's DOM specs render for real.
// ─────────────────────────────────────────────────────────────────────────────

interface SegOption {
  value: string;
  label: string;
}

/** One settings row: label (+ reset pill when dirty), help, and the projected
 *  control. `wide` stacks the control under the help (full-width controls). */
@Component({
  selector: "app-set-row",
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [IconComponent, KjButtonComponent],
  styles: [
    `
.set-row{display:flex;align-items:flex-start;gap:var(--sp-7);padding:var(--sp-5) 0;}
/* the control is a flex SIBLING of main (not nested as in the JSX) — gap sp-1
   keeps the wide control exactly where the design put it (3px under the help) */
.set-row.wide{flex-direction:column;gap:var(--sp-1);}
.set-row.dis{opacity:.45;pointer-events:none;}
.set-row-main{flex:1;min-width:0;display:flex;flex-direction:column;gap:var(--sp-1);}
.set-row.wide .set-row-main{width:100%;}
.set-row-lbl{color:var(--ink);display:flex;align-items:center;gap:var(--sp-4);line-height:1.2;}
/* only the measure is Settings-specific; the rest is the global <small> */
.set-row-help{max-width:46ch;}
.set-row-help code{color:var(--ink-3);background:var(--panel-2);
  padding:0 var(--sp-2);border-radius:4px;}
.set-row-ctrl{flex:none;display:flex;align-items:center;gap:var(--sp-4);padding-top:1px;}
.set-row.wide .set-row-ctrl{width:100%;padding-top:0;}

.set-reset .kj-button{display:inline-flex;align-items:center;gap:var(--sp-2);height:var(--sp-7);padding:0 var(--sp-3) 0 var(--sp-2);
  border-radius:999px;border:1px solid var(--hair);background:var(--panel-2);color:var(--ink-3);
  font-family:var(--font-mono);font-size:var(--fs-badge);letter-spacing:.06em;text-transform:uppercase;
  cursor:pointer;transition:all .12s;flex:none;box-shadow:none;}
.set-reset .kj-button:hover{color:var(--ui-ink);border-color:var(--ui-line);
  background:var(--ui-sel);}
.set-reset .kj-button svg{width:var(--sp-4);height:var(--sp-4);}
    `,
  ],
  template: `
    <div class="set-row" [class.wide]="wide" [class.dis]="disabled">
      <div class="set-row-main">
        <div class="set-row-lbl">
          <ng-content select="[row-label]" />
          @if (dirty && !disabled) {
            <kj-button kjVariant="ghost" class="set-reset" title="Reset to default" (click)="reset.emit()">
              <app-icon name="refresh" size="sm" />reset
            </kj-button>
          }
        </div>
        <small class="set-row-help"><ng-content select="[row-help]" /></small>
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
  { id: "keymap", label: "Keymap", icon: "bolt" },
  { id: "perms", label: "Permissions & safety", icon: "lock" },
  { id: "notif", label: "Notifications", icon: "bell" },
];

const SUBS: Record<SettingsSection, string> = {
  updates: "Channel, version & install behavior",
  agent: "Defaults for newly spawned agents",
  keymap: "Keyboard shortcuts per command",
  perms: "Auto-approval & remote control",
  notif: "OS alerts, events & sound",
};

const EVENTS: ReadonlyArray<{ k: keyof SettingsEvents; label: string; help: string }> = [
  { k: "finished", label: "Finished", help: "An agent completed its task." },
  { k: "question", label: "Question", help: "An agent asked a decision question." },
  { k: "permission", label: "Permission", help: "An agent needs approval to run a command." },
  { k: "error", label: "Error", help: "An agent hit an error or crashed." },
];

@Component({
  selector: "app-settings-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  // The design ships one flat `.set-`-prefixed stylesheet that also styles svg
  // glyphs INSIDE the icon/badge child components — encapsulation off keeps the
  // pixel-level CSS working verbatim (everything is namespaced under .set-).
  encapsulation: ViewEncapsulation.None,
  imports: [
    IconComponent,
    ToolBadgeComponent,
    SelectComponent,
    SetRowComponent,
    RuntimeRowComponent,
    KjBadgeComponent,
    KjButtonComponent,
    KjComboboxComponent,
    KjComboboxEmptyComponent,
    KjComboboxOptionComponent,
    KjKbdComponent,
    KjNumberInputComponent,
    KjProgressBarComponent,
    KjSliderComponent,
    KjTabsComponent,
    KjTabListComponent,
    KjTabComponent,
    KjListComponent,
    KjListItemComponent,
    KjSpinnerComponent,
    KjToggleComponent,
  ],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Settings" },
  template: `
    @let s = store.settings();
    <div class="set-modal">
      <!-- nav -->
      <nav class="set-nav">
        <div class="set-brand">
          <span class="gi glyph-plate"><app-icon name="settings" size="sm" /></span>
          <div style="min-width:0">
            <h1 class="bt">Settings</h1>
            <p class="bs">Orrery preferences</p>
          </div>
        </div>
        <!-- kouji's list is built for exactly this row ("Sidebar nav with
             active row" in its docs): it carries the list semantics, the
             current-row state and arrow-key navigation, none of which the old
             [class.on] buttons gave assistive tech. -->
        <kj-list
          class="set-nav-list"
          as="ul"
          [arrowNavigation]="true"
          [hoverable]="true"
          ariaLabel="Settings sections"
        >
          @for (sec of sections; track sec.id) {
            <kj-list-item
              class="set-nav-item"
              [active]="section() === sec.id"
              (click)="selectSection(sec.id)"
            >
              <app-icon [name]="sec.icon" size="sm" />
              <span class="lb trunc">{{ sec.label }}</span>
              @if (sec.id === 'updates' && store.updateKnown()) { <span class="set-nav-dot" title="Update available"></span> }
            </kj-list-item>
          }
        </kj-list>
      </nav>

      <!-- main -->
      <div class="set-main">
        <div class="set-head">
          <div>
            <h2 class="ht">{{ current().label }}</h2>
            <p class="hs">{{ subs[section()] }}</p>
          </div>
          <kj-button kjSize="icon" class="set-x" kjAriaLabel="Close settings" (click)="close()"><app-icon name="x" size="sm" /></kj-button>
        </div>

        <div class="set-body">
          @switch (section()) {
            <!-- ── Updates ─────────────────────────────────────────────── -->
            @case ("updates") {
              <div class="set-grp">
                <h3 class="up set-grp-h">Release channel</h3>
                <app-set-row [dirty]="s.channel !== D.channel" (reset)="store.set({ channel: D.channel })">
                  <ng-container row-label>Channel</ng-container>
                  <ng-container row-help>Beta receives pre-release builds first.</ng-container>
                  <kj-tabs variant="pills" class="set-seg" [value]="s.channel" (valueChange)="store.set({ channel: $any($event) })">
                    <kj-tab-list aria-label="Release channel">
                      @for (o of channelOptions; track o.value) {
                        <kj-tab [value]="o.value">{{ o.label }}</kj-tab>
                      }
                    </kj-tab-list>
                  </kj-tabs>
                </app-set-row>
                @if (s.channel === 'beta') {
                  <div class="set-warn" style="margin-top:var(--sp-1)">
                    <app-icon name="flag" size="sm" />
                    Pre-release builds — may be unstable or break worktrees. Roll back from this panel anytime.
                  </div>
                }
                <app-set-row [dirty]="s.updatePolicy !== D.updatePolicy" (reset)="store.set({ updatePolicy: D.updatePolicy })">
                  <ng-container row-label>Install policy</ng-container>
                  <ng-container row-help>What Orrery does when a new build is available.</ng-container>
                  <kj-tabs variant="pills" class="set-seg" [value]="s.updatePolicy" (valueChange)="store.set({ updatePolicy: $any($event) })">
                    <kj-tab-list aria-label="Install policy">
                      @for (o of policyOptions; track o.value) {
                        <kj-tab [value]="o.value">{{ o.label }}</kj-tab>
                      }
                    </kj-tab-list>
                  </kj-tabs>
                </app-set-row>
              </div>

              <div class="set-grp">
                <h3 class="up set-grp-h">Version</h3>
                <app-set-row>
                  <ng-container row-label>Current build</ng-container>
                  <ng-container row-help>
                    Orrery <code>v{{ version.version() || '—' }}</code> · {{ s.channel }} channel · checked
                    {{ store.checking() ? 'now…' : lastChecked() }}
                  </ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-4)">
                    <kj-badge class="set-vchip tnum" variant="outline">v{{ version.version() || '—' }} · {{ s.channel === 'beta' ? 'BETA' : 'STABLE' }}</kj-badge>
                    <kj-button kjVariant="outline" [kjDisabled]="store.checking()" (click)="store.checkNow()">
                      @if (store.checking()) {
                        <kj-spinner kjAriaLabel="Checking for updates" />
                      } @else {
                        <app-icon name="refresh" size="sm" />
                      }
                      {{ store.checking() ? 'Checking…' : 'Check now' }}
                    </kj-button>
                  </div>
                </app-set-row>

                @if (store.updateCard(); as upd) {
                  <app-set-row [wide]="true">
                    <ng-container row-label>Update available</ng-container>
                    <ng-container row-help>A newer build is ready to install.</ng-container>
                    <div class="set-upd">
                      <div class="set-upd-top">
                        <span class="set-upd-ic glyph-plate"><app-icon name="stage" /></span>
                        <div class="set-upd-tt">
                          <div class="u1">Orrery <span class="set-upd-ver">v{{ upd.version }}</span></div>
                          <div class="u2">@if (upd.date) { released {{ upd.date }} · } upgrades from v{{ version.version() || '—' }}</div>
                        </div>
                      </div>
                      <a class="set-upd-notes" [href]="releasesUrl" (click)="openWhatsNew($event)">
                        <app-icon name="file" size="sm" />Read release notes<app-icon name="ext" size="sm" />
                      </a>
                      <div class="set-upd-act">
                        <kj-button kjVariant="default" [kjDisabled]="store.installing()" (click)="store.install()">
                          <app-icon name="stage" size="sm" />
                          {{ !store.installing() ? 'Install & relaunch' : store.installPhase() === 'installing' ? 'Installing…' : 'Downloading ' + installPct() + '%' }}
                        </kj-button>
                        <kj-button kjVariant="outline" (click)="store.dismissUpdate()">Later</kj-button>
                      </div>
                      @if (store.installing()) {
                        <kj-progress-bar class="set-upd-bar" [kjValue]="$any(installBarValue())" kjAriaLabel="Update download progress" />
                        <div class="set-upd-stage">
                          {{ store.installPhase() === 'installing'
                            ? 'Handing off to the installer — Orrery restarts when it finishes'
                            : 'Downloading v' + upd.version + ' · ' + installPct() + '%' }}
                        </div>
                      }
                    </div>
                  </app-set-row>
                }
              </div>

              <div class="set-grp">
                <h3 class="up set-grp-h">Diagnostics</h3>
                <app-set-row>
                  <ng-container row-label>Log file</ng-container>
                  <ng-container row-help>
                    Orrery appends a rolling diagnostics log — orchestrator, updater, git and IPC events. Open it when something needs a closer look.
                  </ng-container>
                  <kj-button kjVariant="outline" (click)="openLog()">
                    <app-icon name="ext" size="sm" />Open log file
                  </kj-button>
                </app-set-row>
              </div>
            }

            <!-- ── Agent defaults ──────────────────────────────────────── -->
            @case ("agent") {
              <div class="set-grp">
                <h3 class="up set-grp-h">Worktrees</h3>
                <app-set-row [wide]="true" [dirty]="s.branchTemplate !== D.branchTemplate" (reset)="store.set({ branchTemplate: D.branchTemplate })">
                  <ng-container row-label>Branch template</ng-container>
                  <ng-container row-help>Tokens: <code>{{ '{name}' }}</code> <code>{{ '{tool}' }}</code> <code>{{ '{date}' }}</code></ng-container>
                  <div style="display:flex;flex-direction:column;gap:0;width:100%">
                    <div class="set-text" style="max-width: round(calc(320px * var(--density)), 1px)">
                      <app-icon name="branch" size="sm" />
                      <input [value]="s.branchTemplate" spellcheck="false" (input)="store.set({ branchTemplate: $any($event.target).value })" />
                    </div>
                    <div class="set-preview">
                      <span class="arr">preview</span><app-icon size="md" name="chevron" /><b>{{ branchPreview() }}</b>
                    </div>
                  </div>
                </app-set-row>

                <app-set-row [dirty]="s.worktreeRoot !== D.worktreeRoot" (reset)="store.set({ worktreeRoot: D.worktreeRoot })">
                  <ng-container row-label>Worktree root</ng-container>
                  <ng-container row-help>Where new agent worktrees are created on disk.</ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-4);width: round(calc(300px * var(--density)), 1px)">
                    <div class="set-path"><app-icon name="folder" size="sm" /><span class="pt trunc">{{ s.worktreeRoot || 'app data · worktrees' }}</span></div>
                    <kj-button kjVariant="outline" (click)="browse()"><app-icon name="folderOpen" size="sm" />Browse</kj-button>
                  </div>
                </app-set-row>

                <app-set-row [dirty]="s.autoResume !== D.autoResume" (reset)="store.set({ autoResume: D.autoResume })">
                  <ng-container row-label>Auto-resume on restart</ng-container>
                  <ng-container row-help>Re-attach to running agent sessions when Orrery relaunches.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Auto-resume on restart" [pressed]="s.autoResume" (pressedChange)="store.set({ autoResume: $event })" />
                </app-set-row>

                <app-set-row [dirty]="s.autosave !== D.autosave" (reset)="store.set({ autosave: D.autosave })">
                  <ng-container row-label>Autosave edits</ng-container>
                  <ng-container row-help>Write unsaved editor buffers 2s after you stop typing. Ctrl+S still saves on demand.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Autosave edits" [pressed]="s.autosave" (pressedChange)="store.set({ autosave: $event })" />
                </app-set-row>
              </div>

              <div class="set-grp">
                <h3 class="up set-grp-h">Projects</h3>
                <app-set-row [dirty]="s.projectsRoot !== D.projectsRoot" (reset)="store.set({ projectsRoot: D.projectsRoot })">
                  <ng-container row-label>Projects folder</ng-container>
                  <ng-container row-help>Where the folder picker opens when adding a project.</ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-4);width: round(calc(300px * var(--density)), 1px)">
                    <div class="set-path"><app-icon name="folder" size="sm" /><span class="pt trunc">{{ s.projectsRoot || 'OS default' }}</span></div>
                    <kj-button kjVariant="outline" (click)="browseProjects()"><app-icon name="folderOpen" size="sm" />Browse</kj-button>
                  </div>
                </app-set-row>
              </div>

              <!-- ── AI cost & budget (A4.3 / A4.4) — hidden while the cost
                   kill switch is off ─────────────────────────────────────── -->
              @if (costEnabled) {
              <div class="set-grp">
                <h3 class="up set-grp-h">AI cost &amp; budget</h3>
                <app-set-row [dirty]="s.budgetCapUsd !== D.budgetCapUsd" (reset)="store.set({ budgetCapUsd: D.budgetCapUsd })">
                  <ng-container row-label>Budget cap</ng-container>
                  <ng-container row-help>USD ceiling for AI git actions. At the cap, AI variants disable — native actions stay fully usable. 0 = no cap.</ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-3)">
                    <span style="color:var(--ink-4)">$</span>
                    <kj-number-input class="set-num-cap" [kjMin]="0" [kjStep]="1" [kjAllowDecimals]="true"
                      [kjValue]="s.budgetCapUsd" (kjValueChange)="setBudget('budgetCapUsd', $event)" kjAriaLabel="Budget cap in USD" />
                  </div>
                </app-set-row>
                <app-set-row [dirty]="s.confirmAboveUsd !== D.confirmAboveUsd" (reset)="store.set({ confirmAboveUsd: D.confirmAboveUsd })">
                  <ng-container row-label>Confirm above</ng-container>
                  <ng-container row-help>AI actions estimated above this amount need a confirming second click. 0 = never confirm.</ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-3)">
                    <span style="color:var(--ink-4)">$</span>
                    <kj-number-input class="set-num-cap" [kjMin]="0" [kjStep]="0.1" [kjAllowDecimals]="true"
                      [kjValue]="s.confirmAboveUsd" (kjValueChange)="setBudget('confirmAboveUsd', $event)" kjAriaLabel="Confirm above USD" />
                  </div>
                </app-set-row>
              </div>
              }

              <div class="set-grp">
                <h3 class="up set-grp-h">Default agent</h3>
                <app-set-row [wide]="true" [dirty]="s.defaultTool !== D.defaultTool" (reset)="store.set({ defaultTool: D.defaultTool })">
                  <ng-container row-label>Default tool</ng-container>
                  <ng-container row-help>Used when you spawn without picking one. Pick an agent to configure its model, effort and executable path below.</ng-container>
                  <div class="set-tools">
                    @for (tl of tools; track tl.id) {
                      @let e = runtime.detection(tl.id);
                      @let runnable = e?.status === 'ok';
                      @let on = s.defaultTool === tl.id;
                      <kj-button kjVariant="ghost" class="set-tool"
                        [class.on]="on" [class.warn]="on && !runnable" [class.off]="!runnable && !on"
                        [title]="runnable ? e?.path : (e?.status === 'error' ? 'Found but can’t run — set its path below' : 'Not installed — locate it below')"
                        (click)="store.set({ defaultTool: tl.id })">
                        <app-tool-badge [tool]="tl.id" [size]="22" />
                        <div class="tn">{{ tl.name }}</div>
                        <div class="ts">
                          @if (runnable) {
                            <span class="dot done" style="width:var(--sp-2);height:var(--sp-2)"></span>detected{{ e?.version ? ' · v' + e?.version : '' }}
                          } @else if (e?.status === 'error') {
                            <span style="color:var(--set-amber)">can’t run</span>
                          } @else { not installed }
                        </div>
                        @if (on) { <span class="pick"><app-icon size="md" name="check" /></span> }
                        @if (!runnable && !on) {
                          <span class="nf" [class.amber]="e?.status === 'error'">{{ e?.status === 'error' ? 'needs path' : 'not found' }}</span>
                        }
                      </kj-button>
                    }
                  </div>
                </app-set-row>

                <app-set-row [wide]="true" [dirty]="s.toolPath[modelTool().id] !== undefined" (reset)="resetToolPath(modelTool().id)">
                  <ng-container row-label>Executable <span style="color:var(--ink-4);font-weight:var(--fw-normal)">· {{ modelTool().name }}</span></ng-container>
                  <ng-container row-help>Where Orrery launches <b style="color:var(--ink-3);font-weight:var(--fw-medium)">{{ modelTool().name }}</b> from — detected on your <code>PATH</code> at startup. Override it if the binary lives elsewhere or couldn’t run.</ng-container>
                  <app-runtime-row [toolId]="modelTool().id" [toolName]="modelTool().name" />
                </app-set-row>

                <app-set-row [dirty]="s.toolModel[modelTool().id] !== undefined" (reset)="store.setMap('toolModel', modelTool().id, null)">
                  <ng-container row-label>Model <span style="color:var(--ink-4);font-weight:var(--fw-normal)">· {{ modelTool().name }}</span></ng-container>
                  <ng-container row-help>Curated per tool, with a free-text override — Enter to apply a custom id.</ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-4)">
                    @if (isCustomModel()) { <kj-badge variant="outline">custom</kj-badge> }
                    <kj-combobox class="set-model-combo" [freeText]="true" placeholder="model-id…"
                      [value]="effModel()" (valueChange)="onModelChange($event)">
                      @for (m of modelTool().models; track m) {
                        <kj-combobox-option [value]="m">{{ m }}</kj-combobox-option>
                      }
                      <kj-combobox-empty>Custom — CLIs don’t expose a list. Enter applies the typed id.</kj-combobox-empty>
                    </kj-combobox>
                  </div>
                </app-set-row>

                <app-set-row [dirty]="s.toolEffort[modelTool().id] !== undefined" (reset)="store.setMap('toolEffort', modelTool().id, null)">
                  <ng-container row-label>Reasoning effort</ng-container>
                  <ng-container row-help>
                    @if (effortOptions(); as eo) { How hard the model thinks before acting. } @else { {{ modelTool().name }} doesn’t expose an effort setting. }
                  </ng-container>
                  @if (effortOptions(); as eo) {
                    <kj-tabs variant="pills" class="set-seg" [value]="effEffort()" (valueChange)="store.setMap('toolEffort', modelTool().id, $any($event))">
                      <kj-tab-list aria-label="Reasoning effort">
                        @for (o of eo; track o) {
                          <kj-tab [value]="o">{{ o }}</kj-tab>
                        }
                      </kj-tab-list>
                    </kj-tabs>
                  } @else {
                    <span class="set-muted"><app-icon name="dots" size="sm" />not supported</span>
                  }
                </app-set-row>

                <!-- per-model AI rates, scoped to the SELECTED agent's model
                     list (common budget caps stay in the group above) —
                     hidden while the cost kill switch is off -->
                @for (m of costEnabled ? toolRateModels() : []; track m) {
                  <app-set-row [dirty]="rateDirty(m)" (reset)="resetRate(m)">
                    <ng-container row-label>
                      <span style="display:inline-flex;align-items:center;gap:var(--sp-3)">
                        <span style="color:var(--ink-4);font-weight:var(--fw-normal)">rate ·</span>
                        <code>{{ m }}</code>
                      </span>
                    </ng-container>
                    <ng-container row-help>$ per million tokens — in / out. Feeds the AI-action estimates for {{ modelTool().name }}.</ng-container>
                    <div style="display:flex;align-items:center;gap:var(--sp-3)">
                      <kj-number-input class="set-num-rate" [kjMin]="0" [kjStep]="0.1" [kjAllowDecimals]="true"
                        [kjValue]="rateOf(m).in" (kjValueChange)="setRate(m, 'in', $event)" kjAriaLabel="input $/Mtok" />
                      <span style="color:var(--ink-4)">/</span>
                      <kj-number-input class="set-num-rate" [kjMin]="0" [kjStep]="0.1" [kjAllowDecimals]="true"
                        [kjValue]="rateOf(m).out" (kjValueChange)="setRate(m, 'out', $event)" kjAriaLabel="output $/Mtok" />
                    </div>
                  </app-set-row>
                }
              </div>
            }

            <!-- ── Keymap (B6.2) ───────────────────────────────────────── -->
            @case ("keymap") {
              @for (grp of keymapGroups(); track grp.name) {
                <div class="set-grp">
                  <h3 class="up set-grp-h">{{ grp.name }}</h3>
                  @for (cmd of grp.commands; track cmd.id) {
                    <app-set-row [dirty]="!!s.keymap[cmd.id]" (reset)="store.setKeymapEntry(cmd.id, null)">
                      <ng-container row-label>{{ cmd.label }}</ng-container>
                      <ng-container row-help>
                        @if (capturing() === cmd.id) {
                          press the new chord — Esc cancels, Backspace unbinds
                        } @else if (conflictOf(cmd)) {
                          also bound to "{{ conflictOf(cmd) }}"
                        }
                      </ng-container>
                      <kj-button kjVariant="outline" class="set-kbd"
                        (click)="startCapture(cmd.id)"
                        [style.--kj-button-border-color]="capturing() === cmd.id ? 'var(--ui-focus)' : conflictOf(cmd) ? 'var(--sem-del)' : null"
                        [style.--kj-button-fg]="cmd.kbd ? 'var(--ink)' : 'var(--ink-4)'"
                        [title]="'Click, then press the new shortcut'"
                      ><kj-kbd>{{ capturing() === cmd.id ? 'recording…' : cmd.kbd ? kbdChip(cmd.kbd) : 'unassigned' }}</kj-kbd></kj-button>
                    </app-set-row>
                  }
                </div>
              }
            }

            <!-- ── Permissions & safety ────────────────────────────────── -->
            @case ("perms") {
              <div class="set-grp">
                <h3 class="up set-grp-h">Auto-approve policy</h3>
                @for (tl of detectedTools(); track tl.id) {
                  @let val = approveOf(tl.id);
                  <app-set-row [dirty]="val !== 'off'" (reset)="resetApprove(tl.id)">
                    <ng-container row-label>
                      <span style="display:inline-flex;align-items:center;gap:var(--sp-4)"><app-tool-badge [tool]="tl.id" [size]="16" />{{ tl.name }}</span>
                    </ng-container>
                    <ng-container row-help>{{ approveHelp(val) }}</ng-container>
                    <kj-tabs variant="pills" class="set-seg" [value]="val" (valueChange)="pickPolicy(tl.id, $any($event))">
                      <kj-tab-list aria-label="Auto-approve policy">
                        @for (o of approveOptions; track o.value) {
                          <kj-tab [value]="o.value" [class.dgr]="o.value === 'everything'">
                            @if (o.value === 'everything') { <app-icon name="flag" size="sm" /> }
                            {{ o.label }}
                          </kj-tab>
                        }
                      </kj-tab-list>
                    </kj-tabs>
                  </app-set-row>
                  @if (confirm()?.tool === tl.id) {
                    <div class="set-danger">
                      <div class="set-danger-h"><app-icon name="flag" />Allow {{ tl.name }} to run <b style="color:inherit">everything</b>?</div>
                      <div class="set-danger-b">
                        Auto-approving <b>every</b> command lets {{ tl.name }} run destructive operations —
                        <code>rm&nbsp;-rf</code>, force-push, network calls — with no prompt.
                        Recommended only for fully sandboxed worktrees.
                      </div>
                      <div class="set-danger-act">
                        <kj-button kjVariant="outline" (click)="cancelEverything(tl.id)">Cancel</kj-button>
                        <kj-button kjVariant="danger" (click)="confirm.set(null)"><app-icon name="flag" size="sm" />Enable “Everything”</kj-button>
                      </div>
                    </div>
                  }
                }
              </div>

              <div class="set-grp">
                <h3 class="up set-grp-h">Remote approval</h3>
                <app-set-row [dirty]="s.remoteApproval !== D.remoteApproval" (reset)="store.set({ remoteApproval: D.remoteApproval })">
                  <ng-container row-label>Approve from notifications</ng-container>
                  <ng-container row-help>Answer permission prompts straight from OS notifications.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Approve from notifications" [pressed]="s.remoteApproval" (pressedChange)="store.set({ remoteApproval: $event })" />
                </app-set-row>
              </div>

              <!-- ── Diagnostics (A0.7 raw emit trace) ──────────────────── -->
              <div class="set-grp">
                <h3 class="up set-grp-h">Diagnostics</h3>
                <app-set-row [dirty]="s.telemetryRawTrace !== D.telemetryRawTrace" (reset)="store.set({ telemetryRawTrace: D.telemetryRawTrace })">
                  <ng-container row-label>Raw emit trace</ng-container>
                  <ng-container row-help>Records one line per backend event (timestamp, name, byte count — never contents) to app-data/telemetry. Auto-stops after 30 min or 200 MB; a status-bar chip shows while it records.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Raw emit trace" [pressed]="s.telemetryRawTrace" (pressedChange)="setRawTrace($event)" />
                </app-set-row>
              </div>
            }

            <!-- ── Notifications ───────────────────────────────────────── -->
            @case ("notif") {
              @let off = !s.osNotifications;
              <div class="set-grp">
                <h3 class="up set-grp-h">Delivery</h3>
                <app-set-row [dirty]="s.osNotifications !== D.osNotifications" (reset)="store.set({ osNotifications: D.osNotifications })">
                  <ng-container row-label>Native OS notifications</ng-container>
                  <ng-container row-help>Off keeps all alerts inside the app only.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Native OS notifications" [pressed]="s.osNotifications" (pressedChange)="store.set({ osNotifications: $event })" />
                </app-set-row>
              </div>

              <div class="set-grp">
                <h3 class="up set-grp-h">Events</h3>
                @for (ev of events; track ev.k) {
                  <app-set-row [disabled]="off" [dirty]="s.events[ev.k] !== D.events[ev.k]" (reset)="store.setEvent(ev.k, D.events[ev.k])">
                    <ng-container row-label>{{ ev.label }}</ng-container>
                    <ng-container row-help>{{ ev.help }}</ng-container>
                    <kj-toggle class="set-tgl" appearance="switch" size="sm" [ariaLabel]="ev.label" [pressed]="s.events[ev.k]" [disabled]="off" (pressedChange)="store.setEvent(ev.k, $event)" />
                  </app-set-row>
                }
              </div>

              <div class="set-grp">
                <h3 class="up set-grp-h">Sound</h3>
                <app-set-row [disabled]="off" [dirty]="s.sound !== D.sound" (reset)="store.set({ sound: D.sound })">
                  <ng-container row-label>Play sound</ng-container>
                  <ng-container row-help>A short cue when a notification fires.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Play sound" [pressed]="s.sound" [disabled]="off" (pressedChange)="store.set({ sound: $event })" />
                </app-set-row>
                <app-set-row
                  [disabled]="off || !s.sound"
                  [dirty]="s.soundName !== D.soundName || s.volume !== D.volume"
                  (reset)="store.set({ soundName: D.soundName, volume: D.volume })"
                >
                  <ng-container row-label>Cue &amp; volume</ng-container>
                  <ng-container row-help>Notification tone and loudness.</ng-container>
                  <div style="display:flex;align-items:center;gap:var(--sp-5)">
                    <app-select [value]="s.soundName" [options]="soundOptions" (valueChange)="store.set({ soundName: $event })" />
                    <kj-button kjVariant="ghost" class="set-play" title="Send a test notification" kjAriaLabel="Send a test notification" (click)="previewCue()">
                      <svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.5v13l11-6.5z" /></svg>
                    </kj-button>
                    <app-icon name="volume" size="sm" color="var(--ink-4)" />
                    <kj-slider class="set-slider" [kjMin]="0" [kjMax]="100" [kjValue]="s.volume" (kjValueChange)="store.set({ volume: $event })" kjAriaLabel="Notification volume" />
                    <span class="tnum" style="font-size:var(--fs-meta);color:var(--ink-3);width:30px;text-align:right">{{ s.volume }}%</span>
                  </div>
                </app-set-row>
              </div>
            }
          }
        </div>

        <div class="set-foot">
          @if (store.anyDirty()) {
            <kj-button class="reset-all" kjVariant="quiet" (click)="resetAll()"><app-icon name="refresh" size="sm" />Reset all to defaults</kj-button>
          } @else {
            <span class="fl"><span class="fd"></span>Changes apply instantly</span>
          }
          <kj-button class="set-foot-cancel" kjVariant="outline" (click)="close()">Cancel</kj-button>
          <kj-button kjVariant="default" (click)="close()"><app-icon name="check" size="sm" />Done</kj-button>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
/* ── ORCHESTRA settings — scoped by the .set- prefix (encapsulation: None) ── */
/* No backdrop here: KjDialog centers this component's host and paints the
   scrim (see the kouji overlay chrome block in styles.css). .set-modal is the panel box. */
.set-modal{--set-amber:var(--sem-attn);--set-danger:var(--sem-del);
  width: round(calc(760px * var(--density)), 1px);max-width:calc(100vw - 56px);height:600px;max-height:84vh;display:flex;
  background:var(--panel);border:1px solid var(--hair-2);border-radius:15px;overflow:hidden;
  box-shadow:var(--shadow);
  font-family:var(--font-ui);color:var(--ink);
  transform-origin:center;animation:set-pop .22s cubic-bezier(.2,.7,.2,1);}
/* transform-only entrance: if the frame is throttled and the animation freezes
   at 0%, content stays visible (just offset) instead of stuck at opacity:0 */
@keyframes set-pop{from{transform:translateY(10px) scale(.99)}to{transform:none}}
@media (prefers-reduced-motion:reduce){.set-modal{animation:none}}

/* ── left nav ── */
.set-nav{width: round(calc(204px * var(--density)), 1px);flex:none;background:var(--panel-2);border-right:1px solid var(--hair);
  display:flex;flex-direction:column;padding:var(--sp-6) var(--sp-5);}
.set-brand{display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-2) var(--sp-4) var(--sp-6);}
/* skin from the shared .glyph-plate; size + radius stay per-instance */
.set-brand .gi{width:var(--ctl-h);height:var(--ctl-h);border-radius:8px;flex:none;}
.set-brand .bt{font-size:var(--fs-body);}
.set-brand .bs{font-size:var(--fs-meta);color:var(--ink-4);letter-spacing:.04em;}
.set-nav-list{display:flex;flex-direction:column;gap:var(--sp-1);}
/* The rows are kj-list-items now, so the ground, hover and current-row state
   come from the component (--kj-list-* / [active]). What stays here is only
   what the list cannot know: the row's height and the accent bar that marks
   the current section on the panel's edge. */
.set-nav-list{--kj-list-row-padding:0 var(--sp-5);--kj-list-radius:9px;}
.set-nav-item{position:relative;height:35px;gap:var(--sp-5);cursor:pointer;}
.set-nav-item app-icon{color:var(--ink-4);transition:color .12s;}
.set-nav-item:hover app-icon,
.set-nav-item[data-active] app-icon{color:var(--ui-ink);}
.set-nav-item .lb{flex:1;}
.set-nav-item[data-active]::before{content:"";position:absolute;left:-11px;top:9px;bottom:9px;width:2.5px;
  border-radius:2px;background:var(--ui-ind);}
.set-nav-dot{width:var(--sp-3);height:var(--sp-3);border-radius:50%;background:var(--set-amber);flex:none;
  box-shadow:0 0 7px -1px var(--set-amber);}

/* ── right column ── */
.set-main{flex:1;min-width:0;display:flex;flex-direction:column;background:var(--panel);}
.set-head{flex:none;display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-7) var(--sp-7) var(--sp-6);
  border-bottom:1px solid var(--hair);}
.set-head .ht{font-size:var(--fs-body);}
.set-head .hs{color:var(--ink-4);margin-top:var(--sp-1);}
.set-x .kj-button{margin-left:auto;flex:none;width:var(--ctl-h);height:var(--ctl-h);padding:0;border-radius:7px;border:1px solid transparent;
  background:transparent;color:var(--ink-3);cursor:pointer;display:grid;place-items:center;transition:all .12s;box-shadow:none;}
.set-x .kj-button:hover{background:var(--panel-3);color:var(--ink);border-color:var(--hair);}

.set-body{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;padding:var(--sp-2) var(--sp-7) var(--sp-8);}

.set-grp{padding:var(--sp-7) 0 var(--sp-7);border-bottom:1px solid var(--hair);}
.set-grp:last-child{border-bottom:none;}
.set-grp-h{color:var(--ink-3);
  margin-bottom:var(--sp-3);display:flex;align-items:center;gap:var(--sp-4);}
.set-grp-h::after{content:"";flex:1;height:1px;background:var(--hair);}

/* ── segmented control ──
   The tray, the chip and the selected state are kouji's pills tabs now
   (--kj-tab-* knobs in styles.css). What is Settings-specific: its size step
   and the danger tint on "everything". */
.set-seg{--kj-tab-padding-x:var(--sp-6);--kj-tab-font-size:var(--fs-body);}
.set-seg .kj-tab svg{width:var(--sp-6);height:var(--sp-6);}
.set-seg .kj-tab[aria-selected="true"] svg{color:var(--ui-ink);}
.set-seg .kj-tab.dgr[aria-selected="true"]{color:var(--set-danger);
  background:color-mix(in oklch,var(--set-danger),transparent 88%);
  box-shadow:0 0 0 1px color-mix(in oklch,var(--set-danger),transparent 52%);}
.set-seg .kj-tab.dgr[aria-selected="true"] svg{color:var(--set-danger);}

/* ── switch toggle — 34×19 track + 15×15 thumb, the design's fixed geometry ── */
.set-tgl .kj-toggle--switch{--kj-switch-w:34px;--kj-switch-h:19px;--kj-switch-thumb:15px;flex:none;}

/* ── model combobox / numeric fields / volume slider widths ── */
.set-model-combo{min-width: round(calc(186px * var(--density)), 1px);}
.set-num-cap .kj-number-input{width: round(calc(120px * var(--density)), 1px);}
.set-num-rate .kj-number-input{width: round(calc(96px * var(--density)), 1px);}
.set-num-cap .kj-number-input__field,.set-num-rate .kj-number-input__field{width:100%;min-width:0;}
.set-slider .kj-slider{width: round(calc(128px * var(--density)), 1px);}
.set-upd-bar .kj-progress-bar{width:100%;}

/* ── tool select grid (agent default tool) ── */
/* auto-fit, NOT repeat(4,1fr): a grid item's min-width is auto, so a 1fr
   track cannot shrink below its content. At a larger --fs-scale the tool names
   outgrew their quarter-share, the tracks pushed the grid past .set-body — which
   is overflow-x:hidden — and the 4th tile was simply cut off with no scrollbar.
   Wrapping to fewer columns keeps every tile whole and readable; minmax(0,…) on
   the floor plus the truncation below stops it re-appearing at extreme scales. */
.set-tools{display:grid;gap:var(--sp-4);width:100%;
  /* the floor is in ch, so it grows with the TYPE: when the names no longer
     fit four across, auto-fit drops to three or two and every tile stays
     whole, instead of four cramped tiles truncating their labels. */
  grid-template-columns:repeat(auto-fit,minmax(14ch,1fr));}
.set-tool .kj-button{min-width:0;}
.set-tool .tn,.set-tool .ts{max-width:100%;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-tool .kj-button{position:relative;display:flex;flex-direction:column;align-items:flex-start;justify-content:flex-start;gap:var(--sp-4);
  height:auto;padding:var(--sp-5) var(--sp-5) var(--sp-5);border-radius:11px;border:1px solid var(--hair);background:var(--panel-2);
  color:var(--ink-3);cursor:pointer;transition:all .13s;text-align:left;box-shadow:none;width:100%;
  /* DELIBERATE EXCEPTION: the tool tiles stay mono. They read as machine
     identities (claude / codex / gemini + their binaries), not as chrome, so
     they keep the data typeface even though the rest of the dialog is UI. */
  font-family:var(--font-mono);}
.set-tool .kj-button:hover{border-color:var(--hair-2);color:var(--ink-2);transform:translateY(-1px);}
.set-tool.off .kj-button:hover{border-color:var(--hair);color:var(--ink-3);transform:none;}
.set-tool.on .kj-button{color:var(--ink);border-color:var(--ui-line);
  background:var(--ui-sel);}
.set-tool.off .kj-button{opacity:.5;}
.set-tool.warn .kj-button{color:var(--ink);border-color:color-mix(in oklch,var(--set-amber),transparent 50%);
  background:color-mix(in oklch,var(--set-amber),transparent 90%);}
.set-tool.warn .pick{background:var(--set-amber);}
.set-tool .tn{font-weight:var(--fw-medium);}
.set-tool .ts{font-size:var(--fs-meta);color:var(--ink-4);display:flex;align-items:center;gap:var(--sp-2);}
.set-tool .pick{position:absolute;top:9px;right:9px;width:var(--sp-7);height:var(--sp-7);border-radius:50%;
  display:grid;place-items:center;background:var(--ui-fill);color:var(--ui-on-fill);}
.set-tool .nf{position:absolute;top:10px;right:10px;font-size:var(--fs-micro);letter-spacing:.08em;text-transform:uppercase;
  color:var(--ink-4);border:1px solid var(--hair);border-radius:4px;padding:0 var(--sp-1);
  /* the tile is ~125px wide and this badge shares its top row with the tool
     glyph — at the badge step (12px) the uppercase label filled the tile and
     collided with the icon, so it takes the micro step (design uses 8px) */
  max-width:calc(100% - var(--sp-9));overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
.set-tool .nf.amber{color:var(--set-amber);border-color:color-mix(in oklch,var(--set-amber),transparent 55%);}

/* ── chips / amber / fields ── */
.set-warn{display:inline-flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-5);border-radius:8px;
  color:var(--set-amber);background:color-mix(in oklch,var(--set-amber),transparent 88%);
  border:1px solid color-mix(in oklch,var(--set-amber),transparent 60%);line-height:1.4;}
.set-warn svg{width:var(--sp-6);height:var(--sp-6);flex:none;}
.set-muted{font-size:var(--fs-meta);color:var(--ink-4);display:inline-flex;align-items:center;gap:var(--sp-3);}

.set-text{display:flex;align-items:center;gap:var(--sp-4);height:var(--row-h);padding:0 var(--sp-5);background:var(--panel-2);
  border:1px solid var(--hair);border-radius:8px;min-width:0;transition:border-color .12s;}
.set-text:focus-within{border-color:var(--ui-focus);}
.set-text input{flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);
  font-family:var(--font-mono);}
.set-text svg{width:var(--sp-6);height:var(--sp-6);color:var(--ink-4);flex:none;}
.set-preview{display:flex;align-items:center;gap:var(--sp-4);font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-1);}
.set-preview b{color:var(--ink);font-weight:var(--fw-medium);}
.set-preview .arr{color:var(--ink-4);}

.set-path{flex:1;min-width:0;display:flex;align-items:center;gap:var(--sp-4);height:var(--row-h);padding:0 var(--sp-5);
  background:var(--panel-2);border:1px solid var(--hair);border-radius:8px;color:var(--ink-2);
  overflow:hidden;}
.set-path svg{width:var(--sp-6);height:var(--sp-6);color:var(--ink-4);flex:none;}

/* ── update available card ── */
.set-upd{display:flex;flex-direction:column;gap:var(--sp-6);padding:var(--sp-7);border-radius:12px;width:100%;
  background:var(--ui-sel);
  border:1px solid var(--ui-sel-2);}
.set-upd-top{display:flex;align-items:center;gap:var(--sp-5);}
/* .glyph-plate + the update card's stronger --ui-line ring (design/app.html:10622) */
.set-upd-ic{width:var(--ctl-h-lg);height:var(--ctl-h-lg);flex:none;
  box-shadow:inset 0 0 0 1px var(--ui-line);}
.set-upd-tt{flex:1;min-width:0;}
.set-upd-tt .u1{color:var(--ink);font-weight:var(--fw-medium);display:flex;align-items:center;gap:var(--sp-4);}
.set-upd-tt .u2{color:var(--ink-3);margin-top:var(--sp-1);font-variant-numeric:tabular-nums;}
.set-upd-ver{font-family:var(--font-disp);font-size:var(--fs-lg);font-weight:var(--fw-medium);color:var(--ui-ink);letter-spacing:-.01em;}
.set-upd-notes{display:inline-flex;align-items:center;gap:var(--sp-2);color:var(--ui-link);
  text-decoration:none;border-bottom:1px solid color-mix(in oklch,var(--ui-link),transparent 70%);
  padding-bottom:1px;align-self:flex-start;}
.set-upd-notes:hover{border-color:var(--ui-link);}
.set-upd-notes svg{width:var(--sp-5);height:var(--sp-5);}
.set-upd-act{display:flex;gap:var(--sp-4);}
.set-upd-stage{color:var(--ink-3);font-variant-numeric:tabular-nums;}

/* ── danger confirm ── */
.set-danger{margin-top:var(--sp-6);width:100%;padding:var(--sp-6);border-radius:11px;display:flex;flex-direction:column;gap:var(--sp-5);
  border:1px solid var(--set-danger);background:color-mix(in oklch,var(--set-danger),transparent 90%);
  box-shadow:0 0 0 3px color-mix(in oklch,var(--set-danger),transparent 88%);animation:set-pop .16s ease;}
.set-danger-h{display:flex;align-items:center;gap:var(--sp-4);color:var(--set-danger);font-weight:var(--fw-medium);}
.set-danger-h svg{width:var(--sp-7);height:var(--sp-7);flex:none;}
.set-danger-b{color:var(--ink-2);line-height:1.55;}
.set-danger-b b{color:var(--ink);font-weight:var(--fw-medium);}
.set-danger-act{display:flex;gap:var(--sp-4);justify-content:flex-end;}

/* ── cue preview ── */
.set-play .kj-button{width:var(--ctl-h);height:var(--ctl-h);flex:none;display:grid;place-items:center;border-radius:7px;padding:0;
  border:1px solid var(--hair);background:var(--panel-2);color:var(--ink-3);cursor:pointer;transition:all .12s;box-shadow:none;}
.set-play .kj-button:hover{color:var(--ui-ink);border-color:var(--ui-line);
  background:var(--ui-sel);}
.set-play .kj-button svg{width:var(--sp-5);height:var(--sp-5);}

/* ── footer ── */
.set-foot{flex:none;display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-5) var(--sp-7);border-top:1px solid var(--hair);
  background:var(--panel-2);color:var(--ink-3);}
.set-foot .fl{display:inline-flex;align-items:center;gap:var(--sp-3);}
.set-foot .fd{width:var(--sp-3);height:var(--sp-3);border-radius:50%;background:var(--st-done);flex:none;}
/* .kj-quiet is the bare-label recipe; only the type and hover ink differ */
.set-foot .reset-all .kj-button{--kj-button-gap:var(--sp-2);font-family:var(--font-mono);}
.set-foot .reset-all .kj-button:hover:not([aria-disabled="true"]){--kj-button-fg:var(--ui-ink);}
.set-foot .reset-all .kj-button svg{width:var(--sp-5);height:var(--sp-5);}
.set-foot-cancel .kj-button{margin-left:auto;}
    `,
  ],
})
export class SettingsModalComponent {
  /** Cost kill switch — hides the budget group + per-model rate rows. */
  readonly costEnabled = COST_FEATURES_ENABLED;
  readonly store = inject(SettingsStore);
  readonly runtime = inject(AgentRuntimeService);
  readonly version = inject(VersionService);
  private readonly bridge = inject(BRIDGE);
  private readonly alerts = inject(NotificationAlertService);
  private readonly diag = inject(DiagnosticsService);
  private readonly registry = inject(CommandRegistryService);

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      // Esc / outside-click dismiss the overlay, not the store — run the same
      // teardown (revert a pending danger confirm, stop a keymap recorder,
      // clear the store flag) whichever side closed it.
      this.close();
    });
  }

  // ---- Keymap (B6.2) ----
  /** Command id currently recording a new chord (null = not recording). */
  readonly capturing = signal<string | null>(null);
  readonly kbdChip = kbdLabel;

  /** Registry commands by group, in registry order — the effective kbd on each
   *  row already includes any override (the registry applies the keymap). */
  readonly keymapGroups = computed(() => {
    const groups = new Map<string, { name: string; commands: AppCommand[] }>();
    for (const c of this.registry.commands()) {
      if (!groups.has(c.group)) groups.set(c.group, { name: c.group, commands: [] });
      groups.get(c.group)!.commands.push(c);
    }
    return [...groups.values()];
  });

  /** Another command sharing this one's effective binding, for the warn hint. */
  conflictOf(cmd: AppCommand): string | null {
    if (!cmd.kbd) return null;
    const other = this.registry
      .commands()
      .find((c) => c.id !== cmd.id && (c.kbd === cmd.kbd || c.kbdAlt === cmd.kbd));
    return other?.label ?? null;
  }

  /** Record the next chord for `id`. The registry's dispatcher is gated off
   *  via `captureMode` for the duration, so the chord being recorded cannot
   *  simultaneously RUN the command it currently belongs to. */
  startCapture(id: string): void {
    if (this.capturing()) this.stopCapture();
    this.capturing.set(id);
    this.registry.captureMode.set(true);
    const h = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        this.stopCapture();
        return;
      }
      if (e.key === "Backspace" || e.key === "Delete") {
        this.store.setKeymapEntry(id, null);
        this.stopCapture();
        return;
      }
      const b = bindingFromEvent(e);
      if (!b) return; // modifier-only / unmodified key — keep recording
      this.store.setKeymapEntry(id, b);
      this.stopCapture();
    };
    this.captureHandler = h;
    window.addEventListener("keydown", h, true);
  }
  private captureHandler: ((e: KeyboardEvent) => void) | null = null;
  private stopCapture(): void {
    if (this.captureHandler) window.removeEventListener("keydown", this.captureHandler, true);
    this.captureHandler = null;
    this.capturing.set(null);
    this.registry.captureMode.set(false);
  }

  /** "Play" on the Cue & volume row: a full test notification (toast + cue)
   *  exactly as the current settings would deliver a real one. */
  previewCue(): void {
    this.alerts.preview();
  }

  readonly tools = AGENT_TOOLS;
  readonly sections = SECTIONS;
  readonly subs = SUBS;
  readonly events = EVENTS;
  readonly soundOptions: string[] = [...SOUND_OPTIONS];
  readonly releasesUrl = RELEASES_URL;
  readonly D = settingsDefaults();
  readonly channelOptions: SegOption[] = [
    { value: "stable", label: "stable" },
    { value: "beta", label: "beta" },
  ];
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
  /** The effective model isn't in the curated list — a free-text override. */
  readonly isCustomModel = computed(() => !this.modelTool().models.includes(this.effModel()));
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
  }
  close(): void {
    this.stopCapture(); // a dangling recorder would keep eating keystrokes
    // A pending danger confirm means "Everything" was applied optimistically
    // but never confirmed — closing (Esc / backdrop / Cancel / Done) must not
    // leave the bypass enabled silently. Same revert as the confirm's Cancel.
    const c = this.confirm();
    if (c) {
      this.store.setMap("autoApprove", c.tool, c.prev);
      this.confirm.set(null);
    }
    this.store.closeModal();
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
  /** Download fraction as a whole percent for the update card. */
  installPct(): number {
    return Math.round(this.store.installProgress() * 100);
  }
  /** Progress-bar value: null (indeterminate stripe) once the installer runs. */
  installBarValue(): number | null {
    return this.store.installPhase() === "installing" ? null : this.installPct();
  }
  /** "Read release notes": open the in-app What's New digest (it carries a
   *  "View full changelog" link to the releases page). */
  openWhatsNew(e: Event): void {
    e.preventDefault();
    this.store.openWhatsNew();
  }

  /** "Open log file": reveal the rolling diagnostics log in the OS handler. */
  openLog(): void {
    this.diag.openLog();
  }

  // ── agent defaults ──
  async browse(): Promise<void> {
    try {
      const dir = await this.bridge.pickDirectory(this.store.settings().worktreeRoot || undefined);
      if (dir) this.store.set({ worktreeRoot: dir });
    } catch {
      /* picker unavailable (plain browser) — ignore */
    }
  }

  /** Pick the default folder the add-project picker opens in. */
  async browseProjects(): Promise<void> {
    try {
      const dir = await this.bridge.pickDirectory(this.store.settings().projectsRoot || undefined);
      if (dir) this.store.set({ projectsRoot: dir });
    } catch {
      /* picker unavailable (plain browser) — ignore */
    }
  }

  /** Clear a tool's manual path override and re-detect it on PATH. */
  resetToolPath(id: string): void {
    this.store.setMap("toolPath", id, null);
    void this.runtime.refreshDetections();
  }

  /** Combobox commit (curated pick or free-text Enter) → per-tool override. */
  onModelChange(v: unknown): void {
    const m = String(v ?? "").trim();
    if (m) this.store.setMap("toolModel", this.modelTool().id, m);
  }

  // ── AI cost & budget (A4.3 / A4.4) ──
  /** The SELECTED tool's curated models (+ a custom effective model, so its
   *  rate stays editable) — the per-agent slice of the rate table. */
  readonly toolRateModels = computed(() => {
    const models = this.modelTool().models;
    const eff = this.effModel();
    return models.includes(eff) ? models : [...models, eff];
  });
  rateOf(model: string): CostRate {
    return this.store.settings().costRates[model] ?? DEFAULT_RATES[model] ?? DEFAULT_RATES["default"];
  }
  rateDirty(model: string): boolean {
    return this.store.settings().costRates[model] !== undefined;
  }
  setRate(model: string, field: "in" | "out", v: number): void {
    if (!isFinite(v) || v < 0) return;
    const next: CostRate = { ...this.rateOf(model), [field]: v };
    const def = DEFAULT_RATES[model];
    const rates = { ...this.store.settings().costRates };
    // back at the built-in default → drop the override (absent = default)
    if (def && def.in === next.in && def.out === next.out) delete rates[model];
    else rates[model] = next;
    this.store.set({ costRates: rates });
  }
  resetRate(model: string): void {
    const rates = { ...this.store.settings().costRates };
    delete rates[model];
    this.store.set({ costRates: rates });
  }
  setBudget(key: "budgetCapUsd" | "confirmAboveUsd", v: number): void {
    if (!isFinite(v) || v < 0) return;
    this.store.set({ [key]: v });
  }

  /** Raw emit trace toggle (A0.7): flush immediately — the backend arms the
   *  trace inside `settings_set`, so the 300ms debounce would delay recording. */
  setRawTrace(on: boolean): void {
    this.store.set({ telemetryRawTrace: on });
    this.store.flush();
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
