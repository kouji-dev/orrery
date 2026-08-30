import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { CommandRegistryService } from "../commands/command-registry.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { MetricsStore } from "../metrics/metrics.store";
import { CostStore } from "../metrics/cost.store";
import { TelemetryStore } from "../metrics/telemetry.store";
import { DevPanelStore } from "../dev-tools/dev-panel.store";
import { VersionBadgeComponent } from "../shared/version-badge.component";
import { SettingsStore, worktreeRootLabel } from "../settings/settings.store";
import { DiagnosticsService } from "../shared/diagnostics.service";
import { ToolWindowStore } from "../tool-window/tool-window.store";
import { KjButton, KjTooltipContent, KjTooltipTrigger } from "@kouji-ui/core";

@Component({
  selector: "app-status-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, VersionBadgeComponent, KjButton, KjTooltipContent, KjTooltipTrigger],
  template: `
    <footer
      style="display:flex;align-items:center;gap:var(--sp-6);padding:0 var(--sp-6);background:var(--panel);border-top:1px solid var(--hair);color:var(--ink-3)"
    >
      <span style="display:flex;gap:var(--sp-3);align-items:center">
        <span class="dot running" style="background:var(--st-running)"></span
        >{{ running() }} running
      </span>
      @if (blocked() > 0) {
        <button
          type="button"
          class="sb-link"
          (click)="openQueue()"
          title="Work the unblock queue · N"
          style="color:var(--st-blocked)"
        >
          <span class="dot" style="background:var(--st-blocked)"></span
          >{{ blocked() }} need attention
        </button>
      }
      <span class="tnum"
        >{{ projects.all().length }} projects ·
        {{ runtime.agents().length }} agents</span
      >
      <span style="display:flex;gap:var(--sp-2)"
        ><app-icon size="md" name="folder" />{{
          worktreeRoot()
        }}</span
      >

      <!-- bottom tool-window trigger (design: the dock is command-driven; this
           is its one discoverable affordance — Branches/Local History stay a
           tab-switch away once the dock is open). Branch icon by request. -->
      <button kjButton
        type="button"
        class="sb-link"
        [style.color]="toolWindow.panel() ? 'var(--ui-ink)' : null"
        (click)="toolWindow.toggle('graph')"
        title="Git Graph (tool window)"
      >
        <app-icon size="md" name="branch" />Git Graph
      </button>
      <!-- right cluster: the first member that actually renders carries
           margin-left:auto — no always-rendered spacer span needed -->
      @if (ui.toast()) {
        <span class="tnum" style="margin-left:auto;display:flex;gap:var(--sp-2);align-items:center">
          <span class="grad-ink" style="font-weight:var(--fw-medium)">{{ ui.toast() }}</span>
          <span style="color:var(--ink-4)">·</span>
        </span>
      }

      <!-- A0.7 visible indicator: the raw emit trace is recording (opt-in,
           auto-off after 30min/200MB) — click opens the Emits tab -->
      @if (telemetry.traceActive()) {
        <button kjButton type="button" class="sb-link sb-rec" [style.margin-left]="ui.toast() ? null : 'auto'" (click)="devPanel.openEmits()" title="Raw emit trace is recording (auto-off after 30min or 200MB) — click to open the Emits panel">
          <span class="sb-recdot"></span>TRACE
        </button>
      }

      <!-- open the rolling diagnostics log file -->
      <button kjButton type="button" class="sb-link" [style.margin-left]="ui.toast() || telemetry.traceActive() ? null : 'auto'" (click)="diag.openLog()" title="Open log file">
        <app-icon size="md" name="file" />logs
      </button>

      <!-- total Claude cost (ccusage); hover → rich kouji tooltip. hidden when unavailable -->
      @if (cost.cost()?.available) {
        <span
          class="cost-readout tnum"
          kjTooltipTrigger
          #costTip="kjTooltipTrigger"
          style="display:flex;gap:var(--sp-2);align-items:center;cursor:default"
        >
          <app-icon size="md"
            name="sparkles"
            [color]="'var(--ui-ink)'"
          />\${{ cost.cost()!.totalCost.toFixed(2) }}
        </span>
        <kj-tooltip-content [kjFor]="costTip" kjSide="top" kjAlign="end">
          <span style="display:block;text-align:right">
            <span
              class="up"
              style="display:block;color:var(--ink-3)"
              >Total Claude cost</span
            >
            <span
              class="tnum"
              style="display:block;font-size:var(--fs-xl);font-weight:var(--fw-medium);color:var(--ink);line-height:1.35"
              >\${{ cost.cost()!.totalCost.toFixed(2) }}</span
            >
            <span style="display:block;font-size:var(--fs-meta);color:var(--ink-3)"
              >all usage · via ccusage</span
            >
          </span>
        </kj-tooltip-content>
      }

      <!-- bottom-right cpu/memory readout; the per-process breakdown lives in
           the dev console's Resources tab — clicking deep-links there -->
      <button kjButton
        type="button"
        class="gauge"
        (click)="devPanel.openResources()"
        title="Open Resources (dev console)"
        style="display:flex;align-items:center;gap:var(--sp-3);border:none;background:transparent;cursor:pointer;font-family:inherit;padding:0;color:var(--ink-3)"
      >
        <app-icon size="md" name="cpu" [color]="'var(--ui-ink)'" />
        <!-- A0.6 agents-only readout: what the footer answers is "what are the
             AGENTS costing me" — Orrery's own footprint (and the full recursive
             tree) lives in the Resources tab this deep-links to -->
        @if (hasAgentProcs()) {
          <span class="tnum">agents {{ agentsCpu() }}% · {{ agentsMem() }}</span>
        }
        <!-- mini bar reflecting agents cpu (clamped 0–100) -->
        <span
          style="position:relative;width:30px;height:var(--sp-2);border-radius:2px;background:var(--panel-2);overflow:hidden"
        >
          <span
            [style.width.%]="cpuBar()"
            style="position:absolute;inset:0 auto 0 0;background:var(--ui-meter);border-radius:2px"
          ></span>
        </span>
      </button>

      <!-- design orrery-v2 sb-dev-slot: the FAB rail is gone — Tweaks and the
           Dev console launch from HERE, between the mem/cpu readout and the
           version badge. data-dismiss-ignore keeps a chip's mousedown from
           light-dismissing the very panel it is about to toggle. -->
      <button kjButton
        type="button"
        class="sb-chip"
        data-dismiss-ignore
        [class.on]="ui.tweaksOpen()"
        (click)="ui.tweaksOpen.set(!ui.tweaksOpen())"
        title="Tweaks — theme, layout, motion"
      >
        <app-icon size="md" name="spark" />Tweaks
      </button>
      <button kjButton
        type="button"
        class="sb-chip"
        data-dismiss-ignore
        [class.on]="devPanel.open()"
        (click)="devPanel.toggle()"
        [title]="devPanel.alertCount() ? 'Dev console · ' + devPanel.alertCount() + ' perf alert' + (devPanel.alertCount() > 1 ? 's' : '') : 'Dev console'"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="width:round(calc(11px * var(--density)), 1px);height:round(calc(11px * var(--density)), 1px)"><path d="M3 12h3l2.5-6 4 13 3-9 1.5 2H21" /></svg>
        Dev
        @if (devPanel.alertCount()) {
          <span class="sb-alertct tnum">{{ devPanel.alertCount() }}</span>
        }
      </button>

      <!-- app version + channel tag (DEV / BETA) → opens the release changelog -->
      <button kjButton type="button" class="sb-link" (click)="settings.openWhatsNew()" title="View release changelog">
        <app-version-badge variant="chip" class="tnum" />
      </button>
    </footer>
  `,
  styles: [
    `
      .gauge {
        font-size: inherit;
      }
      .gauge:hover {
        color: var(--ink-2) !important;
      }
      /* dvc-chip (design orrery-v2): the footer launchers read as CONTROLS —
         a bordered pill on the sunken panel tone, not quiet footer text. */
      .sb-chip {
        display: inline-flex;
        align-items: center;
        gap: var(--sp-3);
        padding: round(calc(2px * var(--density)), 1px) round(calc(7px * var(--density)), 1px);
        border-radius: 999px;
        border: 1px solid var(--hair);
        background: var(--panel-2);
        color: var(--ink-2);
        cursor: pointer;
        flex: none;
        white-space: nowrap;
        font-family: inherit;
        font-size: var(--fs-badge);
        transition: color 0.12s, border-color 0.12s;
      }
      .sb-chip:hover {
        color: var(--ink);
        border-color: var(--hair-2);
      }
      .sb-chip.on {
        color: var(--ui-ink);
        border-color: var(--ui-line);
      }
      /* dvc-chipct (design orrery-v2): the Dev chip's red perf-alert pill */
      .sb-alertct {
        display: inline-grid;
        place-items: center;
        min-width: round(calc(15px * var(--density)), 1px);
        height: round(calc(14px * var(--density)), 1px);
        padding: 0 var(--sp-2);
        border-radius: 999px;
        background: var(--sem-del);
        color: var(--on-solid);
        font-size: var(--fs-micro);
        font-weight: var(--fw-strong);
        line-height: 1;
      }
      .sb-link {
        display: flex;
        align-items: center;
        gap: var(--sp-2);
        border: none;
        background: transparent;
        cursor: pointer;
        font-family: inherit;
        /* a bare <button> keeps the UA's 13.33px unless told otherwise — these
           carry the [kjButton] DIRECTIVE, which adds no .kj-button class, so
           none of the button knobs reach them */
        font-size: inherit;
        padding: 0;
        color: var(--ink-3);
      }
      .sb-link:hover {
        color: var(--ink-2);
      }
      .sb-rec {
        color: var(--st-blocked);
        font-weight: var(--fw-medium);
        letter-spacing: 0.06em;
      }
      .sb-recdot {
        width: var(--sp-3);
        height: var(--sp-3);
        border-radius: 50%;
        background: var(--st-blocked);
        animation: sbrec 1.5s ease-in-out infinite;
      }
      @keyframes sbrec {
        0%,
        100% {
          opacity: 1;
        }
        50% {
          opacity: 0.35;
        }
      }
    `,
  ],
})
export class StatusBarComponent {
  readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);
  readonly ui = inject(UiStore);
  readonly metrics = inject(MetricsStore);
  readonly cost = inject(CostStore);
  readonly devPanel = inject(DevPanelStore);
  readonly settings = inject(SettingsStore);
  private readonly registry = inject(CommandRegistryService);
  /** Effective worktree root (settings override, else the app-data default). */
  readonly worktreeRoot = computed(() => worktreeRootLabel(this.settings.settings()));

  /** "N need attention" → the v2 peek queue, never a tab switch. */
  openQueue(): void {
    this.registry.open("peek");
  }
  readonly diag = inject(DiagnosticsService);
  readonly telemetry = inject(TelemetryStore);
  readonly toolWindow = inject(ToolWindowStore);

  readonly running = computed(
    () => this.runtime.agents().filter((a) => a.status === "running").length,
  );
  readonly blocked = computed(
    () => this.runtime.agents().filter((a) => a.status === "blocked").length,
  );

  // ---- gauge readout: AGENTS only (fed from the same subtree rollups the
  // Resources tree drills into — every non-"app" row is an agent subtree) ----
  private readonly agentProcs = computed(() =>
    (this.metrics.metrics()?.procs ?? []).filter((p) => p.id !== "app"),
  );
  readonly hasAgentProcs = computed(() => this.agentProcs().length > 0);
  // machine-relative %, to one decimal
  readonly agentsCpu = computed(
    () => Math.round(this.agentProcs().reduce((a, p) => a + p.cpu, 0) * 10) / 10,
  );
  readonly cpuBar = computed(() => Math.min(100, Math.max(0, this.agentsCpu())));
  readonly agentsMemBytes = computed(() =>
    this.agentProcs().reduce((a, p) => a + p.memBytes, 0),
  );
  readonly agentsMem = computed(() => this.fmtMem(this.agentsMemBytes()));

  /** Human-readable bytes: B / KB / MB / GB, one decimal above KB. */
  fmtMem(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const kb = bytes / 1024;
    if (kb < 1024) return `${Math.round(kb)} KB`;
    const mb = kb / 1024;
    if (mb < 1024) return `${mb.toFixed(1)} MB`;
    return `${(mb / 1024).toFixed(1)} GB`;
  }
}
