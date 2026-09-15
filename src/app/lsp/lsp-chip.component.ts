import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, inject, signal, viewChild } from "@angular/core";
import { KjBadgeComponent, KjButtonComponent, KjSpinnerComponent } from "@kouji-ui/components";
import { KjButton, KjPopoverContent, KjPopoverTrigger } from "@kouji-ui/core";
import { ExtensionsStore } from "../extensions/extensions.store";
import { LspServer } from "../models";
import { IconComponent } from "../shared/icon.component";
import { fmtDur, fmtMem } from "../utils";
import { isError, isSyncable, LspStatusStore } from "./lsp-status.store";

/** Design LiveBadge tone per backend state. */
export function liveOf(s: LspServer): { tone: "live" | "idle" | "err"; label: string; spin: boolean } {
  switch (s.state) {
    case "starting":
      return { tone: "live", label: "starting", spin: true };
    case "idle":
      return { tone: "idle", label: "idle", spin: false };
    case "crashed":
      return { tone: "err", label: "error", spin: false };
    case "missing":
      return { tone: "err", label: "not found", spin: false };
    case "stopped":
      return { tone: "idle", label: "stopped", spin: false };
    default:
      return { tone: "live", label: "running", spin: false };
  }
}

/** Footer/popover name for a server. The pack label is a product name
 *  ("TypeScript language server") — too long for a chip that also carries a
 *  figure — so known packs show their tool name; anything else keeps its
 *  label when short, else the pack id without the "server." prefix. */
const SHORT_LABEL: Record<string, string> = {
  "server.typescript-language-server": "ts-ls",
  "server.jdtls": "jdtls",
  "server.gopls": "gopls",
  "server.rust-analyzer": "rust-analyzer",
  "server.pyright": "pyright",
};
export function shortLabel(s: Pick<LspServer, "extId" | "label">): string {
  const known = SHORT_LABEL[s.extId];
  if (known) return known;
  return s.label.length <= 14 ? s.label : s.extId.replace(/^server\./, "");
}

/** Memory figure of the chip and its rows: two decimals (user, 2026-09-15). */
export function fmtLspMem(bytes: number): string {
  return fmtMem(bytes, 2);
}

/** "12m 5s" since `startedAt`, or "—" before the process is up. */
export function uptimeOf(s: LspServer, now: number): string {
  if (!s.startedAt || s.state === "crashed") return "—";
  return fmtDur(Math.max(0, Math.round((now - s.startedAt) / 1000)));
}

/**
 * Footer chip for the language servers (design orrery-v2 `LspChip` +
 * `LspPopover`): ONE aggregate chip across servers and projects. The chip
 * itself is just the icon and a word — "servers" while any instance has a
 * process, "no servers" otherwise (user, 2026-09-15: no name, no memory, no
 * counts in the footer; the figures live in the popover). State still shows
 * through its tint: a spinner while any starts, the blocked tint when any
 * crashed or never launched (`missing`), dimmed when every instance idles.
 * ALWAYS RENDERED (user, 2026-09-12): a marker that
 * vanishes when nothing runs hides exactly the states worth seeing — a crash, a
 * server that never came up — and reads as a missing feature. With nothing live
 * it says "no servers" and its popover explains how one starts. Click → the
 * anchored popover (notification-center pattern): rows grouped by project
 * with state · memory · cpu · uptime and hover-revealed Stop/Restart, crashed
 * rows expanding their last error, Stop all in the header, Open extensions
 * in the footer. Recipes `.lsp-*` in styles.css (shared chrome).
 */
@Component({
  selector: "app-lsp-chip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjBadgeComponent, KjButtonComponent, KjSpinnerComponent, KjButton, KjPopoverTrigger, KjPopoverContent],
  host: { style: "display:contents" },
  template: `
  <button kjButton type="button" kjPopoverTrigger #lt="kjPopoverTrigger"
      class="lsp-chip" [class.starting]="store.aggregate() === 'starting'" [class.running]="store.aggregate() === 'running'"
      [class.idle]="store.aggregate() === 'idle'" [class.error]="store.aggregate() === 'error'"
      [class.none]="store.aggregate() === 'none'" [class.open]="lt.controller.isOpen()"
      [title]="title()" data-testid="lsp-chip">
      @if (store.aggregate() === 'starting') {
        <kj-spinner kjSize="xs" kjAriaLabel="A language server is starting" />
      } @else {
        <app-icon [name]="one() ? 'server' : 'layers'" size="sm" />
      }
      <span class="mono">{{ label() }}</span>
    </button>

    <kj-popover-content [kjFor]="lt" kjSide="top" kjAlign="end" style="--kj-popover-padding-x:0;--kj-popover-padding-y:0">
      <div class="lsp-pop static" role="dialog" aria-label="Language servers" data-testid="lsp-popover">
        <div class="lsp-pop-h">
          <app-icon name="layers" size="sm" />
          <span class="nm">Language servers</span>
          <span class="tot">{{ store.running().length }}@if (anyLive()) { · {{ mem() }}}</span>
          @if (anyLive()) {
            <kj-button kjVariant="outline" [kjDisabled]="store.busy().has('*')" (click)="stopAll(lt)"><app-icon name="stop" size="sm" />Stop all</kj-button>
          }
        </div>
        <div class="lsp-list">
          @if (!store.any()) {
            @let e = emptyState();
            <div class="lsp-empty" data-testid="lsp-empty">
              <span class="h">{{ e.head }}</span>
              <span>{{ e.body }}</span>
              @if (e.cta) {
                <kj-button class="ext-link" kjVariant="link" (click)="openExtensions(lt)">{{ e.cta }}</kj-button>
              }
            </div>
          }
          @for (g of store.byProject(); track g.projectId) {
            <div class="lsp-g" [attr.data-project]="g.projectId"><app-icon name="box" size="sm" />{{ g.projectName }}<span class="n">{{ g.rows.length }}</span></div>
            @for (s of g.rows; track s.id) {
              @let live = liveOf(s);
              <div class="lsp-row" [class.dim]="s.state === 'idle' || s.state === 'stopped'" [class.err]="isError(s)" [attr.data-server]="s.id">
                <div class="lsp-row-m" [title]="rowTitle(s)">
                  <app-icon name="server" size="sm" />
                  <span class="nm">{{ shortLabel(s) }}</span>
                  <kj-badge class="ext-badge sm" [class.live]="live.tone === 'live'" [class.idle]="live.tone === 'idle'" [class.err]="live.tone === 'err'" variant="outline" [dot]="!live.spin">
                    @if (live.spin) { <kj-spinner kjSize="xs" kjAriaLabel="starting" /> }{{ live.label }}
                  </kj-badge>
                  <span class="fig">{{ s.memBytes ? fmtLspMem(s.memBytes) : '—' }}</span>
                  <span class="fig sub">{{ s.cpu != null ? s.cpu.toFixed(2) + '%' : '—' }} · {{ uptime(s) }}</span>
                  <span class="acts">
                    @if (isSyncable(s)) {
                      <kj-button kjSize="icon" kjVariant="ghost" title="Stop" kjAriaLabel="Stop" [kjDisabled]="store.busy().has(s.id)" (click)="store.stop(s)"><app-icon name="stop" size="sm" /></kj-button>
                    }
                    <kj-button kjSize="icon" kjVariant="ghost" title="Restart" kjAriaLabel="Restart" [kjDisabled]="store.busy().has(s.id)" (click)="store.restart(s)"><app-icon name="refresh" size="sm" /></kj-button>
                  </span>
                </div>
                @if (!isSyncable(s) && s.lastError) { <div class="lsp-err">{{ s.lastError }}</div> }
              </div>
            }
          }
        </div>
        <div class="lsp-pop-f">
          <kj-button class="ext-link" kjVariant="link" (click)="openExtensions(lt)">Open extensions</kj-button>
        </div>
      </div>
    </kj-popover-content>
  `,
})
export class LspChipComponent {
  readonly store = inject(LspStatusStore);
  private readonly extensions = inject(ExtensionsStore);
  readonly fmtLspMem = fmtLspMem;
  readonly liveOf = liveOf;
  readonly shortLabel = shortLabel;
  readonly isError = isError;
  readonly isSyncable = isSyncable;

  private readonly trig = viewChild(KjPopoverTrigger);
  /** Wall clock, ticking once a second only while the popover is open — the
   *  uptime column reads it. */
  private readonly now = signal(Date.now());
  private timer: ReturnType<typeof setInterval> | null = null;

  readonly one = computed(() => this.store.running().length === 1);
  /** Something has (or is getting) a process — the memory figure and "Stop
   *  all" only make sense then; a parked row alone shows neither. */
  readonly anyLive = computed(() => this.store.running().length > 0);
  readonly label = computed(() => (this.store.servers().some(isSyncable) ? "servers" : "no servers"));
  readonly mem = computed(() => fmtLspMem(this.store.totalMem()));

  /**
   * What the popover says when nothing runs. "No language server is running"
   * is true but useless on its own — the reason is almost always one step
   * earlier (nothing installed, or installed but switched off), so name that
   * step and offer the way out of it.
   */
  readonly emptyState = computed<{ head: string; body: string; cta: string | null }>(() => {
    const servers = this.extensions.servers();
    const installed = servers.filter((p) => p.installed);
    const enabled = installed.filter((p) => p.enabled);
    if (!installed.length) {
      return {
        head: "No language server installed",
        body: servers.length
          ? "Install one from Extensions for exact definitions, references and hovers. The symbol index answers meanwhile."
          : "No server packs are listed yet — the registry has not been read.",
        cta: "Open extensions",
      };
    }
    if (!enabled.length) {
      return {
        head: "No language server enabled",
        body: `${installed.length} installed but switched off. Enable one to let it start.`,
        cta: "Open extensions",
      };
    }
    return {
      head: "No language server is running",
      body: `${enabled.length} enabled. One starts when you open a file in its language (or on the first Ctrl+click / hover), and stops again when idle.`,
      cta: null,
    };
  });
  readonly title = computed(() => this.store.running().map((s) => `${s.label} · ${s.projectName}`).join("\n"));

  constructor() {
    effect(() => {
      const open = this.trig()?.controller.isOpen() ?? false;
      if (this.timer) clearInterval(this.timer);
      this.timer = null;
      if (!open) return;
      this.now.set(Date.now());
      this.timer = setInterval(() => this.now.set(Date.now()), 1000);
    });
    inject(DestroyRef).onDestroy(() => {
      if (this.timer) clearInterval(this.timer);
    });
  }

  uptime(s: LspServer): string {
    return uptimeOf(s, this.now());
  }

  rowTitle(s: LspServer): string {
    const parts = [`${s.label} · ${s.projectName}`, `cpu ${(s.cpu ?? 0).toFixed(2)}%`, `up ${this.uptime(s)}`];
    if (s.pid) parts.push(`pid ${s.pid}`);
    if (s.restarts) parts.push(`${s.restarts} restart${s.restarts > 1 ? "s" : ""}`);
    return parts.join(" · ");
  }

  stopAll(lt: KjPopoverTrigger): void {
    lt.controller.close();
    void this.store.stopAll();
  }

  openExtensions(lt: KjPopoverTrigger): void {
    lt.controller.close();
    this.extensions.openModal("servers");
  }
}
