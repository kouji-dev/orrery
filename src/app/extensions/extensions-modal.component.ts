import { NgTemplateOutlet } from "@angular/common";
import { ChangeDetectionStrategy, Component, computed, DestroyRef, inject, signal, ViewEncapsulation } from "@angular/core";
import {
  KjBadgeComponent,
  KjButtonComponent,
  KjConfirmPopupActionComponent,
  KjConfirmPopupActionsComponent,
  KjConfirmPopupCancelComponent,
  KjConfirmPopupComponent,
  KjConfirmPopupContentComponent,
  KjConfirmPopupMessageComponent,
  KjConfirmPopupTriggerComponent,
  KjListComponent,
  KjListItemComponent,
  KjNumberInputComponent,
  KjProgressBarComponent,
  KjSpinnerComponent,
  KjToggleComponent,
} from "@kouji-ui/components";
import { KjButton } from "@kouji-ui/core";
import { liveOf, uptimeOf } from "../lsp/lsp-chip.component";
import { LspStatusStore } from "../lsp/lsp-status.store";
import { SetRowComponent } from "../modals/settings-modal.component";
import { ExtPack, LspServer } from "../models";
import { fmtMem } from "../utils";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { ExtSection, ExtensionsStore, isExtUpdate } from "./extensions.store";

// ─────────────────────────────────────────────────────────────────────────────
// Extensions modal — design/orrery-v2.html `ExtensionsPanel` (≈14278), ported
// 1:1: the Settings modal's shell (nav · head · body · foot, all `.set-*`
// recipes in styles.css), four sections, and the `.ext-row` card per pack.
// Every state a row can be in is derived from the backend's `ExtPack` — the
// modal never keeps its own copy of a pack (see ExtensionsStore).
// ─────────────────────────────────────────────────────────────────────────────

/** What the row shows; derived from the backend's `ExtPack` (see rowState). */
export type ExtRowState = "available" | "downloading" | "installed" | "update" | "pendingRestart" | "error" | "incompatible";

const SECTIONS: ReadonlyArray<{ id: ExtSection; label: string; icon: string; sub: string }> = [
  { id: "grammars", label: "Grammars", icon: "code", sub: "Tree-sitter packs for highlighting, folding and the symbol index" },
  { id: "servers", label: "Language servers", icon: "server", sub: "Definitions, references and hovers beyond the index" },
  { id: "updates", label: "Updates", icon: "refresh", sub: "Newer builds in the registry" },
];

const MB = 1024 * 1024;
/** One decimal below 100 MB ("2.1", "12.3", "40"), whole megabytes above. */
function fmtMb(bytes: number): string {
  const n = bytes / MB;
  return String(n >= 100 ? Math.round(n) : Math.round(n * 10) / 10);
}

@Component({
  selector: "app-extensions-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  // Same reason as the Settings modal: the `.set-*` / `.ext-*` recipes style
  // svg glyphs inside the icon/badge child components.
  encapsulation: ViewEncapsulation.None,
  imports: [
    NgTemplateOutlet,
    IconComponent,
    SetRowComponent,
    KjBadgeComponent,
    KjButtonComponent,
    KjConfirmPopupComponent,
    KjConfirmPopupTriggerComponent,
    KjConfirmPopupContentComponent,
    KjConfirmPopupMessageComponent,
    KjConfirmPopupActionsComponent,
    KjConfirmPopupActionComponent,
    KjConfirmPopupCancelComponent,
    KjListComponent,
    KjListItemComponent,
    KjNumberInputComponent,
    KjProgressBarComponent,
    KjSpinnerComponent,
    KjToggleComponent,
    KjButton,
  ],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Extensions" },
  template: `
    @let s = settings.settings();
    <div class="set-modal ext-modal">
      <!-- nav -->
      <nav class="set-nav">
        <div class="set-brand">
          <span class="gi glyph-plate"><app-icon name="puzzle" size="sm" /></span>
          <div style="min-width:0">
            <h1 class="bt">Extensions</h1>
            <p class="bs">Grammars and servers</p>
          </div>
        </div>
        <kj-list class="set-nav-list" as="ul" [arrowNavigation]="true" [hoverable]="true" ariaLabel="Extension sections">
          @for (sec of sections; track sec.id) {
            <kj-list-item class="set-nav-item" [active]="store.section() === sec.id" (click)="selectSection(sec.id)">
              <app-icon [name]="sec.icon" size="sm" />
              <span class="lb trunc">{{ sec.label }}</span>
              @if (sec.id === 'updates' && store.updateCount() > 0) { <span class="ext-count">{{ store.updateCount() }}</span> }
            </kj-list-item>
          }
        </kj-list>
        <div class="set-nav-foot"><app-icon name="server" size="sm" />{{ runningCount() }} running · {{ packCount() }} packs</div>
      </nav>

      <!-- main -->
      <div class="set-main">
        <div class="set-head">
          <div>
            <h2 class="ht">{{ current().label }}</h2>
            <p class="hs">{{ current().sub }}</p>
          </div>
          <kj-button kjSize="icon" class="set-x" kjAriaLabel="Close extensions" (click)="close()"><app-icon name="x" size="sm" /></kj-button>
        </div>

        <div class="set-body">
          @switch (store.section()) {
            <!-- ── Grammars ────────────────────────────────────────────── -->
            @case ("grammars") {
              @if (!store.grammars().length) {
                <div class="ext-empty">
                  <app-icon name="code" size="lg" />
                  <span class="h">No grammar packs installed</span>
                  <span>Files open as plain text until a grammar for their language is added.</span>
                  <kj-button kjVariant="outline" (click)="store.refresh()"><app-icon name="search" size="sm" />Browse registry</kj-button>
                </div>
              } @else {
                <div class="set-grp">
                  <h3 class="up set-grp-h">Installed · {{ grammarsInstalled().length }}</h3>
                  @for (p of grammarsInstalled(); track p.id) {
                    <ng-container *ngTemplateOutlet="packRow; context: { $implicit: p }" />
                  }
                </div>
                @if (grammarsAvailable().length) {
                  <div class="set-grp">
                    <h3 class="up set-grp-h">Available</h3>
                    @for (p of grammarsAvailable(); track p.id) {
                      <ng-container *ngTemplateOutlet="packRow; context: { $implicit: p }" />
                    }
                  </div>
                }
              }
            }

            <!-- ── Language servers ────────────────────────────────────── -->
            @case ("servers") {
              <div class="set-grp">
                <h3 class="up set-grp-h">Lifecycle</h3>
                <app-set-row [dirty]="s.lspIdleMinutes !== D.lspIdleMinutes" (reset)="setIdle(D.lspIdleMinutes)">
                  <ng-container row-label>Idle shutdown</ng-container>
                  <ng-container row-help>Stop a server that has had no request for this long. It restarts on the next request.</ng-container>
                  <kj-number-input class="set-num-idle" [kjMin]="1" [kjMax]="120" [kjStep]="1"
                    [kjValue]="s.lspIdleMinutes" (kjValueChange)="setIdle($event)" kjAriaLabel="Idle shutdown in minutes" />
                  <span class="ext-unit">min</span>
                </app-set-row>
                <app-set-row [dirty]="s.lspUseSystemServers !== D.lspUseSystemServers" (reset)="settings.set({ lspUseSystemServers: D.lspUseSystemServers })">
                  <ng-container row-label>Use system servers</ng-container>
                  <ng-container row-help>Fall back to servers found on PATH when no pack is installed.</ng-container>
                  <kj-toggle class="set-tgl" appearance="switch" size="sm" ariaLabel="Use system servers" [pressed]="s.lspUseSystemServers" (pressedChange)="settings.set({ lspUseSystemServers: $event })" />
                </app-set-row>
              </div>
              @if (!serversEnabled().length) {
                <div class="ext-empty">
                  <app-icon name="server" size="lg" />
                  <span class="h">No language servers enabled</span>
                  <span>The symbol index still answers go-to-definition; servers add references, hovers and diagnostics.</span>
                  <kj-button kjVariant="outline" (click)="store.refresh()"><app-icon name="search" size="sm" />Browse registry</kj-button>
                </div>
              } @else {
                <div class="set-grp">
                  <h3 class="up set-grp-h">Enabled · {{ serversEnabled().length }}</h3>
                  @for (p of serversEnabled(); track p.id) {
                    <ng-container *ngTemplateOutlet="serverRow; context: { $implicit: p }" />
                  }
                </div>
              }
              @if (serversAvailable().length) {
                <div class="set-grp">
                  <h3 class="up set-grp-h">Available</h3>
                  @for (p of serversAvailable(); track p.id) {
                    <ng-container *ngTemplateOutlet="serverRow; context: { $implicit: p }" />
                  }
                </div>
              }
              <!-- runtime packs (Node, Java) the servers ship with: auto-managed -->
              @if (store.runtimes().length) {
                <div class="set-grp" data-testid="ext-runtimes">
                  <h3 class="up set-grp-h">Runtimes</h3>
                  @for (p of store.runtimes(); track p.id) {
                    <ng-container *ngTemplateOutlet="packRow; context: { $implicit: p, runtime: true }" />
                  }
                </div>
              }
            }

            <!-- ── Updates ─────────────────────────────────────────────── -->
            @case ("updates") {
              @if (!store.updates().length) {
                <div class="ext-empty">
                  <app-icon name="check" size="lg" />
                  <span class="h">Everything is up to date</span>
                  <span>Registry checked {{ ago() }}.</span>
                </div>
              } @else {
                <div class="set-grp">
                  <h3 class="up set-grp-h">{{ store.updateCount() }} update{{ store.updateCount() > 1 ? 's' : '' }} available</h3>
                  @for (p of store.updates(); track p.id) {
                    <ng-container *ngTemplateOutlet="packRow; context: { $implicit: p, runtime: p.kind === 'runtime' }" />
                  }
                </div>
              }
            }
          }
        </div>

        <div class="set-foot">
          @switch (store.registryState()) {
            @case ("refreshing") {
              <span class="fl"><kj-spinner kjSize="xs" kjAriaLabel="Refreshing the registry" />registry · refreshing…</span>
            }
            @case ("offline") {
              <span class="fl ext-off"><app-icon name="warn" size="sm" />registry offline — showing the cached list</span>
              <kj-button class="reset-all" kjVariant="quiet" (click)="store.refresh()"><app-icon name="refresh" size="sm" />Retry</kj-button>
            }
            @default {
              <span class="fl" [title]="store.view()?.registryUrl ?? ''"><span class="fd"></span>{{ registryLabel() }} · updated {{ ago() }}</span>
              <kj-button class="reset-all" kjVariant="quiet" (click)="store.refresh()"><app-icon name="refresh" size="sm" />Refresh</kj-button>
            }
          }
          <span class="sp"></span>
          <kj-button kjVariant="default" (click)="close()"><app-icon name="check" size="sm" />Done</kj-button>
        </div>
      </div>
    </div>

    <!-- ── one grammar pack (also the Updates + Runtimes rows) — design ExtRow;
         runtime = auto-managed dependency pack: no Enabled toggle, Uninstall
         locked while an installed server requires it ──────────────────── -->
    <ng-template #packRow let-p let-runtime="runtime">
      @let st = rowState(p);
      <div class="ext-row" [class.dim]="(p.installed && !p.enabled && !runtime) || st === 'incompatible'" [class.err]="st === 'error'"
        [attr.data-ext-id]="p.id" [attr.data-state]="st">
        <div class="ext-top">
          <span class="ext-ic"><app-icon [name]="runtime ? 'cube' : 'code'" size="sm" /></span>
          <div class="ext-id">
            <div class="ext-name">{{ p.name }}
              @if ((st === 'installed' || st === 'pendingRestart') && p.installedVersion) {
                <kj-badge class="set-vchip tnum" variant="outline">v{{ p.installedVersion }}</kj-badge>
              }
              @if (st === 'update') { <span class="ext-up">{{ p.installedVersion }} → <b>{{ p.version }}</b></span> }
              @if (st === 'pendingRestart') { <kj-badge class="ext-badge attn" variant="outline">restart to activate</kj-badge> }
            </div>
            <div class="ext-meta">
              @if (runtime) {
                <span>{{ runtimeLine(p) }}</span>
              } @else {
                <ng-container *ngTemplateOutlet="langs; context: { $implicit: p.languages }" />
              }
              <span>·</span><span class="mono tnum">{{ sizeLabel(p.sizeBytes) }}</span>
              @if (st === 'incompatible') { <span>·</span><span>{{ incompatibleReason(p) }}</span> }
            </div>
          </div>
          <div class="ext-act">
            @switch (st) {
              @case ("available") {
                <kj-button kjVariant="default" [kjDisabled]="store.busy().has(p.id)" (click)="store.install(p.id)"><app-icon name="download" size="sm" />Install</kj-button>
              }
              @case ("installed") {
                @if (!runtime) {
                  <span class="ext-enabled">Enabled<kj-toggle class="set-tgl" appearance="switch" size="sm" [ariaLabel]="'Enable ' + p.name" [pressed]="p.enabled" [disabled]="store.busy().has(p.id)" (pressedChange)="store.setEnabled(p.id, $event)" /></span>
                }
                <ng-container *ngTemplateOutlet="uninstall; context: { $implicit: p }" />
              }
              @case ("update") {
                <kj-button kjVariant="default" [kjDisabled]="store.busy().has(p.id)" (click)="store.install(p.id)"><app-icon name="refresh" size="sm" />Update</kj-button>
              }
              @case ("error") {
                <kj-button kjVariant="outline" [kjDisabled]="store.busy().has(p.id)" (click)="store.install(p.id)"><app-icon name="refresh" size="sm" />Retry</kj-button>
              }
              @case ("pendingRestart") {
                <kj-button class="ext-link" kjVariant="quiet" (click)="store.restartNow()">Restart now</kj-button>
              }
            }
          </div>
        </div>
        <ng-container *ngTemplateOutlet="tail; context: { $implicit: p, st: st }" />
      </div>
    </ng-template>

    <!-- ── one language server — design ServerRow + ServerInstanceRow ──── -->
    <ng-template #serverRow let-p>
      @let st = rowState(p);
      @let inst = lsp.instancesOf(p.id);
      <div class="ext-row" [class.dim]="(p.installed && !p.enabled) || st === 'incompatible'" [class.err]="st === 'error' || hasCrash(inst)"
        [attr.data-ext-id]="p.id" [attr.data-state]="st">
        <div class="ext-top">
          <span class="ext-ic"><app-icon name="server" size="sm" /></span>
          <div class="ext-id">
            <div class="ext-name">{{ p.name }}
              @if (p.installed && p.installedVersion && st !== 'update') {
                <kj-badge class="set-vchip tnum" variant="outline">v{{ p.installedVersion }}</kj-badge>
              }
              @if (inst.length) {
                <span class="ext-up" [title]="instTitle(inst)">{{ inst.length }} instance{{ inst.length > 1 ? 's' : '' }}</span>
              }
              @if (st === 'update') { <span class="ext-up">{{ p.installedVersion }} → <b>{{ p.version }}</b></span> }
              @if (st === 'pendingRestart') { <kj-badge class="ext-badge attn" variant="outline">restart to activate</kj-badge> }
            </div>
            <div class="ext-meta">
              <ng-container *ngTemplateOutlet="langs; context: { $implicit: p.languages }" />
              @if (p.description) { <span>·</span><span class="trunc">{{ p.description }}</span> }
              @if (st === 'incompatible') { <span>·</span><span>{{ incompatibleReason(p) }}</span> }
            </div>
          </div>
          <div class="ext-act">
            @switch (st) {
              @case ("available") {
                <kj-button kjVariant="default" [kjDisabled]="store.busy().has(p.id)" (click)="store.install(p.id)"><app-icon name="download" size="sm" />{{ installLabel(p) }}</kj-button>
              }
              @case ("update") {
                <kj-button kjVariant="default" [kjDisabled]="store.busy().has(p.id)" (click)="store.install(p.id)"><app-icon name="refresh" size="sm" />Update</kj-button>
              }
              @case ("error") {
                <kj-button kjVariant="outline" [kjDisabled]="store.busy().has(p.id)" (click)="store.install(p.id)"><app-icon name="refresh" size="sm" />Retry</kj-button>
              }
              @case ("pendingRestart") {
                <kj-button class="ext-link" kjVariant="quiet" (click)="store.restartNow()">Restart now</kj-button>
              }
            }
            @if (p.installed && inst.length >= 2) {
              <kj-button kjVariant="outline" [kjDisabled]="lsp.busy().has('*')" (click)="lsp.stopPack(p.id)"><app-icon name="stop" size="sm" />Stop all</kj-button>
            }
            @if (p.installed && st !== 'downloading') {
              <span class="ext-enabled">Enabled<kj-toggle class="set-tgl" appearance="switch" size="sm" [ariaLabel]="'Enable ' + p.name" [pressed]="p.enabled" [disabled]="store.busy().has(p.id)" (pressedChange)="store.setEnabled(p.id, $event)" /></span>
              <ng-container *ngTemplateOutlet="uninstall; context: { $implicit: p }" />
            }
          </div>
        </div>
        <!-- state line: what an Install pulls in (before), where the server
             runs from (after). System copies only count with the toggle on. -->
        @if (!p.installed && st !== 'downloading' && st !== 'error') {
          <div class="ext-line ext-bundle">
            <app-icon name="cube" size="sm" /><span class="lb">bundled ·</span>
            @if (bundleIncludes(p); as deps) { <span>includes {{ deps }}</span><span>·</span> }
            <span class="mono tnum">{{ sizeLabel(bundledSize(p)) }}</span>
          </div>
        }
        @if (p.detection; as d) {
          @switch (detectionLine(p, d)) {
            @case ("bundled") {
              <div class="ext-line"><app-icon name="check" size="sm" /><span class="lb">bundled ·</span><span class="mono tnum" [title]="d.path ?? ''">v{{ p.installedVersion }}</span></div>
            }
            @case ("system") {
              <div class="ext-line" [class.warn]="d.status === 'missing'">
                <app-icon [name]="d.status === 'missing' ? 'warn' : d.status === 'configured' ? 'settings' : 'check'" size="sm" />
                @switch (d.status) {
                  @case ("found") { <span class="lb">found ·</span><span class="mono">{{ d.path }}</span> }
                  @case ("configured") { <span class="lb">configured ·</span><span class="mono">{{ d.path }}</span> }
                  @case ("missing") { <span>not found — install or <kj-button class="ext-link" kjVariant="quiet" (click)="store.locate(p.id)">Locate…</kj-button></span> }
                }
                <span class="sp"></span>
                <kj-button kjVariant="outline" (click)="store.locate(p.id)"><app-icon name="crosshair" size="sm" />Locate…</kj-button>
              </div>
            }
            @case ("ignored") {
              <div class="ext-line mute" [title]="d.path ?? ''"><app-icon name="lock" size="sm" /><span>system copy ignored — enable ‘Use system servers’</span></div>
            }
          }
          @if (d.hint && detectionLine(p, d) !== null) { <div class="ext-line warn"><app-icon name="warn" size="sm" /><span>{{ d.hint }}</span></div> }
        }
        <!-- live instances (M3): one sub-row per project; past three, a
             one-line summary that expands -->
        @if (inst.length) {
          <div class="ext-insts">
            @if (inst.length > 3 && !expanded().has(p.id)) {
              <button kjButton type="button" class="ext-inst sum" (click)="toggleExpanded(p.id)">
                <app-icon name="layers" size="sm" /><span class="pj">{{ instSummary(inst) }}</span><span class="sp"></span><span class="more">expand<app-icon name="chevronD" size="sm" /></span>
              </button>
            } @else {
              @for (i of inst; track i.id) {
                @let live = liveOf(i);
                <div class="ext-inst" [class.dim]="i.state === 'idle' || i.state === 'stopped'" [class.err]="i.state === 'crashed' || i.state === 'missing'" [attr.data-server]="i.id" [title]="instRowTitle(i)">
                  <app-icon name="server" size="sm" />
                  <span class="pj">{{ i.projectName }}</span>
                  <kj-badge class="ext-badge" [class.live]="live.tone === 'live'" [class.idle]="live.tone === 'idle'" [class.err]="live.tone === 'err'" variant="outline" [dot]="!live.spin">
                    @if (live.spin) { <kj-spinner kjSize="xs" kjAriaLabel="starting" /> }{{ live.label }}
                  </kj-badge>
                  <span class="fig">{{ i.memBytes ? fmtMem(i.memBytes) : '—' }}</span>
                  <span class="fig">up {{ uptime(i) }}</span>
                  <span class="sp"></span>
                  @if (i.state === 'starting' || i.state === 'ready' || i.state === 'idle') {
                    <kj-button kjVariant="outline" [kjDisabled]="lsp.busy().has(i.id)" (click)="lsp.stop(i)"><app-icon name="stop" size="sm" />Stop</kj-button>
                  }
                  <kj-button kjVariant="outline" [kjDisabled]="lsp.busy().has(i.id)" (click)="lsp.restart(i)"><app-icon name="refresh" size="sm" />Restart</kj-button>
                  @if ((i.state === 'crashed' || i.state === 'missing' || i.state === 'stopped') && i.lastError) { <div class="ext-line err ie"><app-icon name="warn" size="sm" /><span class="mono">{{ i.lastError }}</span></div> }
                </div>
              }
              @if (inst.length > 3) {
                <button kjButton type="button" class="ext-inst sum" (click)="toggleExpanded(p.id)">
                  <app-icon name="layers" size="sm" /><span class="pj">{{ inst.length }} instances</span><span class="sp"></span><span class="more">collapse<app-icon name="chevsUp" size="sm" /></span>
                </button>
              }
            }
          </div>
        }
        <ng-container *ngTemplateOutlet="tail; context: { $implicit: p, st: st }" />
      </div>
    </ng-template>

    <!-- download bar + error line, shared by both rows -->
    <ng-template #tail let-p let-st="st">
      @if (st === 'downloading') {
        <div class="ext-bar">
          <kj-progress-bar [kjValue]="$any(pct(p.id))" [kjAriaLabel]="'Downloading ' + p.name" />
          <span class="ext-fig">{{ fig(p.id) }}</span>
        </div>
      }
      @if (st === 'error' && p.error) { <div class="ext-line err"><app-icon name="warn" size="sm" /><span>{{ p.error }}</span></div> }
    </ng-template>

    <!-- language chips: first four, then "+n" -->
    <ng-template #langs let-list>
      <span class="ext-langs">
        @for (l of list.slice(0, 4); track l) { <span class="ext-lang">{{ l }}</span> }
        @if (list.length > 4) { <span class="ext-lang">+{{ list.length - 4 }}</span> }
      </span>
    </ng-template>

    <!-- Uninstall with kouji's confirm popup (design UninstallConfirm); a
         runtime an installed server still needs is locked with the reason -->
    <ng-template #uninstall let-p>
      @if (uninstallLock(p); as lock) {
        <span class="ext-lock" [title]="lock"><kj-button kjVariant="outline" [kjDisabled]="true"><app-icon name="trash" size="sm" />Uninstall</kj-button></span>
      } @else {
      <kj-confirm-popup [kjDestructive]="true">
        <kj-confirm-popup-trigger #unTrig="kjConfirmPopupTrigger">
          <kj-button kjVariant="outline" [kjDisabled]="store.busy().has(p.id)"><app-icon name="trash" size="sm" />Uninstall</kj-button>
        </kj-confirm-popup-trigger>
        <kj-confirm-popup-content [kjFor]="unTrig" kjPanelClass="ext-confirm">
          <kj-confirm-popup-message>
            <span class="h"><app-icon name="trash" size="sm" />Uninstall {{ p.name }}?</span>
            <span class="b">{{ uninstallBody(p) }}</span>
          </kj-confirm-popup-message>
          <kj-confirm-popup-actions>
            <kj-confirm-popup-cancel><kj-button kjVariant="outline">Keep</kj-button></kj-confirm-popup-cancel>
            <kj-confirm-popup-action><kj-button kjVariant="danger" (click)="store.uninstall(p.id)"><app-icon name="trash" size="sm" />Uninstall</kj-button></kj-confirm-popup-action>
          </kj-confirm-popup-actions>
        </kj-confirm-popup-content>
      </kj-confirm-popup>
      }
    </ng-template>
  `,
})
export class ExtensionsModalComponent {
  readonly store = inject(ExtensionsStore);
  readonly settings = inject(SettingsStore);
  readonly lsp = inject(LspStatusStore);
  readonly sections = SECTIONS;
  readonly D = settingsDefaults();
  readonly fmtMem = fmtMem;
  readonly liveOf = liveOf;
  /** Server rows whose instance list is expanded past the 3-row summary. */
  readonly expanded = signal<ReadonlySet<string>>(new Set());
  /** Design nav foot: instances with a process (crashed ones excluded). */
  readonly runningCount = computed(() => this.lsp.running().filter((s) => s.state !== "crashed").length);

  constructor() {
    // Esc / outside-click dismiss the overlay, not the store — clear the flag
    // (and persist the idle-shutdown edit) whichever side closed it.
    inject(DestroyRef).onDestroy(() => this.close());
  }

  readonly current = computed(() => SECTIONS.find((x) => x.id === this.store.section()) ?? SECTIONS[0]);

  /** Design: everything but "available"/"incompatible" sits under Installed —
   *  a download, a failed download and a pending restart included. */
  readonly grammarsInstalled = computed(() => this.store.grammars().filter((p) => !this.isAvailable(p)));
  readonly grammarsAvailable = computed(() => this.store.grammars().filter((p) => this.isAvailable(p)));
  readonly serversEnabled = computed(() => this.store.servers().filter((p) => p.installed));
  readonly serversAvailable = computed(() => this.store.servers().filter((p) => !p.installed));
  /** Nav foot "M packs": grammars + runtimes on disk (servers are "running"). */
  readonly packCount = computed(() => this.grammarsInstalled().length + this.store.runtimes().filter((p) => !this.isAvailable(p)).length);

  private isAvailable(p: ExtPack): boolean {
    return !p.installed && (p.state === "available" || p.state === "incompatible");
  }

  // ── self-contained server packs: runtimes ride along ──
  /** Everything an Install downloads: the server plus its missing runtimes. */
  bundledSize(p: ExtPack): number {
    return p.bundledSizeBytes || p.sizeBytes;
  }
  /** "Install · 58 MB" on a server (the bundle), plain "Install" elsewhere. */
  installLabel(p: ExtPack): string {
    return p.kind === "server" && this.bundledSize(p) ? `Install · ${this.sizeLabel(this.bundledSize(p))}` : "Install";
  }
  /** "Node runtime" / "Node runtime, Java runtime" — the runtimes a server
   *  ships with, names without their version; null when it ships alone. */
  bundleIncludes(p: ExtPack): string | null {
    const deps = (p.requires ?? []).map((id) => this.store.nameOf(id).replace(/\s+\d[\w.]*$/, ""));
    return deps.length ? deps.join(", ") : null;
  }
  /** Which detection line a server row shows: its own bundled binary; a
   *  manually configured path always (an explicit user choice, the backend
   *  honours it regardless of the toggle); a found system copy only with the
   *  toggle on, "ignored" otherwise; nothing for a pack-less missing one. */
  detectionLine(p: ExtPack, d: NonNullable<ExtPack["detection"]>): "bundled" | "system" | "ignored" | null {
    if (d.status === "bundled") return p.installed ? "bundled" : null;
    if (d.status === "configured") return "system";
    if (this.settings.settings().lspUseSystemServers) return d.status === "missing" && !p.installed ? null : "system";
    return d.status === "found" ? "ignored" : p.installed ? "system" : null;
  }
  /** Runtime meta: "auto-managed · used by Pyright" / "auto-managed". */
  runtimeLine(p: ExtPack): string {
    const by = this.store.requiredBy(p.id);
    return by.length ? `auto-managed · used by ${by.join(", ")}` : "auto-managed";
  }
  /** Tooltip that locks a runtime's Uninstall while a server needs it. */
  uninstallLock(p: ExtPack): string | null {
    if (p.kind !== "runtime") return null;
    const by = this.store.requiredBy(p.id);
    return by.length ? `required by ${by.join(", ")}` : null;
  }

  /** The one state a row renders, from the backend's flags + transition. */
  rowState(p: ExtPack): ExtRowState {
    if (p.state === "downloading") return "downloading";
    if (p.state === "error") return "error";
    if (p.state === "pendingRestart") return "pendingRestart";
    if (p.state === "incompatible" || !p.compatible || !p.availableForTarget) return "incompatible";
    if (isExtUpdate(p)) return "update";
    return p.installed ? "installed" : "available";
  }

  incompatibleReason(p: ExtPack): string {
    if (!p.availableForTarget) return "no build for this platform";
    if (p.minAppVersion) return `needs Orrery ≥ ${p.minAppVersion}`;
    return p.error ?? "needs a newer Orrery";
  }

  sizeLabel(bytes: number): string {
    return `${fmtMb(bytes)} MB`;
  }

  /** Bar value 0..100 while bytes stream; null (indeterminate) for the
   *  verify/unpack/activate phases and before the first tick. */
  pct(id: string): number | null {
    const p = this.store.progress()[id];
    if (!p || p.phase !== "download" || !p.total) return null;
    return Math.min(100, Math.round((100 * p.downloaded) / p.total));
  }
  /** "12.3 / 40 MB" — or the phase past the download. A multi-step install
   *  (server + its runtimes) names the step: "1/2 · Node runtime 22 · 12.3 /
   *  32 MB", then "2/2 · TypeScript language server · …". */
  fig(id: string): string {
    const p = this.store.progress()[id];
    if (!p) return "starting…";
    let fig: string;
    if (p.phase === "verify") fig = "verifying…";
    else if (p.phase === "unpack") fig = "unpacking…";
    else if (p.phase === "activate") fig = "activating…";
    else fig = p.total ? `${fmtMb(p.downloaded)} / ${fmtMb(p.total)} MB` : `${fmtMb(p.downloaded)} MB`;
    if (!(p.stepCount > 1)) return fig;
    return `${p.stepIndex}/${p.stepCount} · ${this.store.nameOf(p.dependency ?? id)} · ${fig}`;
  }

  uninstallBody(p: ExtPack): string {
    switch (p.kind) {
      case "server":
        return "Projects using this server fall back to the symbol index. Settings for the server are kept.";
      case "runtime":
        return "No installed server needs this runtime. A server that does downloads it again with its next install.";
      default:
        return "Files using this grammar fall back to plain text. Settings for the pack are kept.";
    }
  }

  // ── live instances (M3) ──
  hasCrash(inst: LspServer[]): boolean {
    return inst.some((i) => i.state === "crashed" || i.state === "missing");
  }
  uptime(i: LspServer): string {
    return uptimeOf(i, Date.now());
  }
  instTitle(inst: LspServer[]): string {
    const mem = inst.reduce((n, i) => n + (i.memBytes || 0), 0);
    return `${fmtMem(mem)} across ${inst.length} instance${inst.length > 1 ? "s" : ""}`;
  }
  instRowTitle(i: LspServer): string {
    const parts = [`${i.label} · ${i.projectName} · ${liveOf(i).label}`];
    if (i.memBytes) parts.push(fmtMem(i.memBytes));
    if (i.startedAt) parts.push(`up ${this.uptime(i)}`);
    if (i.pid) parts.push(`pid ${i.pid}`);
    return parts.join(" · ");
  }
  /** "2 running · 1 idle · 1 error · 2.1 GB" — the collapsed summary. */
  instSummary(inst: LspServer[]): string {
    const running = inst.filter((i) => i.state === "ready" || i.state === "starting").length;
    const idle = inst.filter((i) => i.state === "idle").length;
    const errors = inst.filter((i) => i.state === "crashed" || i.state === "missing").length;
    const mem = inst.reduce((n, i) => n + (i.memBytes || 0), 0);
    return `${running} running · ${idle} idle${errors ? ` · ${errors} error${errors > 1 ? "s" : ""}` : ""} · ${fmtMem(mem)}`;
  }
  toggleExpanded(id: string): void {
    this.expanded.update((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  /** "local registry (dist-ext)" for a file:// dev registry, else "registry". */
  registryLabel(): string {
    const u = this.store.view()?.registryUrl ?? "";
    return u.startsWith("file:") ? "local registry (dist-ext)" : "registry";
  }
  ago(): string {
    const t = this.store.view()?.fetchedAt ?? null;
    if (t === null) return "never";
    const m = Math.max(0, Math.round((Date.now() - t) / 60_000));
    if (m < 1) return "just now";
    if (m < 60) return `${m} min ago`;
    return `${Math.round(m / 60)} h ago`;
  }

  setIdle(n: number): void {
    const v = Math.max(1, Math.min(120, Math.round(Number(n) || this.D.lspIdleMinutes)));
    this.settings.set({ lspIdleMinutes: v });
  }

  // ── shell ──
  selectSection(id: ExtSection): void {
    this.store.section.set(id);
  }
  close(): void {
    this.store.closeModal();
    this.settings.flush();
  }
}
