import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { MetricsStore } from "../metrics/metrics.store";
import { CostStore } from "../metrics/cost.store";
import { DevPanelStore } from "../dev-tools/dev-panel.store";
import { VersionBadgeComponent } from "../shared/version-badge.component";

@Component({
  selector: "app-status-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, VersionBadgeComponent],
  template: `
    <footer
      style="display:flex;align-items:center;gap:var(--sp-6);padding:0 var(--sp-6);background:var(--panel);border-top:1px solid var(--hair);font-size:var(--fs-xs);color:var(--ink-3)"
    >
      <span style="display:flex;gap:var(--sp-3);align-items:center">
        <span class="dot running" style="background:var(--st-running)"></span
        >{{ running() }} running
      </span>
      @if (blocked() > 0) {
        <span
          style="display:flex;gap:var(--sp-3);align-items:center;color:var(--st-blocked)"
        >
          <span class="dot" style="background:var(--st-blocked)"></span
          >{{ blocked() }} need attention
        </span>
      }
      <span class="tnum"
        >{{ projects.all().length }} projects ·
        {{ runtime.agents().length }} agents</span
      >
      <span style="display:flex;gap:var(--sp-2)"
        ><app-icon name="folder" size="sm" [px]="11" />{{
          ui.worktreeRoot
        }}</span
      >
      <span
        class="tnum"
        style="margin-left:auto;display:flex;gap:var(--sp-2);align-items:center"
      >
        @if (ui.toast()) {
          <span class="grad-ink" style="font-weight:600">{{ ui.toast() }}</span>
          <span style="color:var(--ink-4)">·</span>
        }
      </span>

      <!-- app version + channel tag (DEV / BETA) -->
      <app-version-badge variant="chip" class="tnum" />

      <!-- total Claude cost (ccusage); hover → bigger tooltip. hidden when unavailable -->
      @if (cost.cost()?.available) {
        <span
          class="cost-readout tnum"
          style="position:relative;display:flex;gap:var(--sp-2);align-items:center;cursor:default"
        >
          <app-icon
            name="sparkles"
            size="sm"
            [px]="11"
            [color]="'var(--accent)'"
          />\${{ cost.cost()!.totalCost.toFixed(2) }}
          <span
            class="cost-tip"
            style="position:absolute;bottom:calc(100% + 8px);right:0;z-index:90;width:max-content;background:var(--elev,var(--panel-2));border:1px solid var(--hair-2,var(--hair));border-radius:var(--r-md,8px);box-shadow:var(--shadow,0 8px 28px rgba(0,0,0,.4));padding:var(--sp-4) var(--sp-6);text-align:right"
          >
            <span
              style="display:block;font-size:var(--fs-2xs);color:var(--ink-3);text-transform:uppercase;letter-spacing:.05em"
              >Total Claude cost</span
            >
            <span
              class="tnum"
              style="display:block;font-size:var(--fs-xl);font-weight:600;color:var(--ink);line-height:1.35"
              >\${{ cost.cost()!.totalCost.toFixed(2) }}</span
            >
            <span style="display:block;font-size:var(--fs-2xs);color:var(--ink-3)"
              >all usage · via ccusage</span
            >
          </span>
        </span>
      }

      <!-- bottom-right cpu/memory readout; the per-process breakdown lives in
           the dev console's Resources tab — clicking deep-links there -->
      <button
        type="button"
        class="gauge"
        (click)="devPanel.openResources()"
        title="Open Resources (dev console)"
        style="display:flex;align-items:center;gap:var(--sp-3);border:none;background:transparent;cursor:pointer;font-family:inherit;font-size:var(--fs-xs);padding:0;color:var(--ink-3)"
      >
        <app-icon name="cpu" size="sm" [px]="11" [color]="'var(--accent)'" />
        <span class="tnum">CPU {{ cpuPct() }}% · MEM {{ totalMem() }}</span>
        <!-- mini bar reflecting total cpu (clamped 0–100) -->
        <span
          style="position:relative;width:30px;height:var(--sp-2);border-radius:2px;background:var(--panel-2);overflow:hidden"
        >
          <span
            [style.width.%]="cpuBar()"
            style="position:absolute;inset:0 auto 0 0;background:var(--accent);border-radius:2px"
          ></span>
        </span>
      </button>
    </footer>
  `,
  styles: [
    `
      .gauge:hover {
        color: var(--ink-2) !important;
      }
      .cost-tip {
        opacity: 0;
        visibility: hidden;
        transform: translateY(4px);
        transition:
          opacity 0.12s ease,
          transform 0.12s ease;
        pointer-events: none;
      }
      .cost-readout:hover .cost-tip {
        opacity: 1;
        visibility: visible;
        transform: translateY(0);
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

  readonly running = computed(
    () => this.runtime.agents().filter((a) => a.status === "running").length,
  );
  readonly blocked = computed(
    () => this.runtime.agents().filter((a) => a.status === "blocked").length,
  );

  // ---- gauge readouts ----
  // total cpu% used by orrery + agents (machine-relative), to one decimal
  readonly cpuPct = computed(
    () => Math.round((this.metrics.metrics()?.totalCpu ?? 0) * 10) / 10,
  );
  readonly cpuBar = computed(() => Math.min(100, Math.max(0, this.cpuPct())));
  // total memory used by orrery + agents (e.g. "432.3 MB")
  readonly totalMem = computed(() =>
    this.fmtMem(this.metrics.metrics()?.totalMemBytes ?? 0),
  );

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
