import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  HostListener,
  inject,
  signal,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { MetricsStore } from "../metrics/metrics.store";
import { CostStore } from "../metrics/cost.store";

@Component({
  selector: "app-status-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <footer style="display:flex;align-items:center;gap:14px;padding:0 14px;background:var(--panel);border-top:1px solid var(--hair);font-size:10.5px;color:var(--ink-3)">
      <span style="display:flex;gap:6px;align-items:center">
        <span class="dot running" style="background:var(--st-running)"></span>{{ running() }} running
      </span>
      @if (blocked() > 0) {
        <span style="display:flex;gap:6px;align-items:center;color:var(--st-blocked)">
          <span class="dot" style="background:var(--st-blocked)"></span>{{ blocked() }} need attention
        </span>
      }
      <span class="tnum">{{ projects.all().length }} projects · {{ runtime.agents().length }} agents</span>
      <span style="display:flex;gap:5px"><app-icon name="folder" size="sm" [px]="11" />{{ ui.worktreeRoot }}</span>
      <span class="tnum" style="margin-left:auto;display:flex;gap:5px;align-items:center">
        @if (ui.toast()) {
          <span class="grad-ink" style="font-weight:600">{{ ui.toast() }}</span>
          <span style="color:var(--ink-4)">·</span>
        }
        <app-icon name="link" size="sm" [px]="11" />orchestrator: healthy
      </span>

      <!-- total Claude cost (ccusage); hidden when ccusage is unavailable -->
      @if (cost.cost()?.available) {
        <span class="tnum" style="display:flex;gap:5px;align-items:center" title="Total Claude cost (ccusage)">
          <app-icon name="sparkles" size="sm" [px]="11" [color]="'var(--accent)'" />\${{ cost.cost()!.totalCost.toFixed(2) }}
        </span>
      }

      <!-- bottom-right cpu/memory gauge; click → popover anchored ABOVE the bar -->
      <span style="position:relative;display:flex">
        <button
          type="button"
          class="gauge"
          (click)="toggle($event)"
          [style.color]="open() ? 'var(--ink)' : 'var(--ink-3)'"
          style="display:flex;align-items:center;gap:7px;border:none;background:transparent;cursor:pointer;font-family:inherit;font-size:10.5px;padding:0"
        >
          <app-icon name="cpu" size="sm" [px]="11" [color]="'var(--accent)'" />
          <span class="tnum">CPU {{ cpuPct() }}% · MEM {{ totalMem() }}</span>
          <!-- mini bar reflecting total cpu (clamped 0–100) -->
          <span style="position:relative;width:30px;height:4px;border-radius:2px;background:var(--panel-2);overflow:hidden">
            <span
              [style.width.%]="cpuBar()"
              style="position:absolute;inset:0 auto 0 0;background:var(--accent);border-radius:2px"
            ></span>
          </span>
        </button>

        @if (open()) {
          <div
            class="rise"
            style="position:absolute;bottom:100%;right:0;margin-bottom:8px;z-index:90;width:max-content;min-width:268px;max-width:min(92vw,420px);background:var(--elev,var(--panel-2));border:1px solid var(--hair-2,var(--hair));border-radius:var(--r-md,8px);box-shadow:var(--shadow,0 8px 28px rgba(0,0,0,.4));overflow:hidden"
          >
            <div style="display:flex;justify-content:space-between;align-items:center;padding:9px 11px;border-bottom:1px solid var(--hair);font-size:11px;color:var(--ink-2)">
              <span style="display:flex;gap:6px;align-items:center">
                <app-icon name="cpu" size="sm" [px]="12" [color]="'var(--accent)'" />resources
              </span>
              <span class="tnum" style="color:var(--ink-3)">{{ cpuPct() }}% · {{ totalMem() }}</span>
            </div>
            <div style="max-height:228px;overflow:auto;padding:5px">
              @for (p of procs(); track p.id) {
                <div style="display:flex;align-items:center;gap:9px;padding:6px 7px;border-radius:6px">
                  <span
                    class="dot"
                    [style.background]="p.id === 'app' ? 'var(--accent-2,var(--accent))' : 'var(--st-running)'"
                  ></span>
                  <span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:11px;color:var(--ink-2)">{{ p.label }}</span>
                  <span class="tnum" style="color:var(--ink-3);font-size:10.5px;min-width:42px;text-align:right;white-space:nowrap">{{ fmtCpu(p.cpu) }}%</span>
                  <span class="tnum" style="color:var(--ink-4);font-size:10.5px;min-width:64px;text-align:right;white-space:nowrap">{{ fmtMem(p.memBytes) }}</span>
                </div>
              } @empty {
                <div style="padding:10px 8px;font-size:11px;color:var(--ink-4)">no metrics yet</div>
              }
            </div>
          </div>
        }
      </span>
    </footer>
  `,
  styles: [
    `
      .gauge:hover {
        color: var(--ink-2) !important;
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
  private host = inject(ElementRef<HTMLElement>);

  readonly running = computed(() => this.runtime.agents().filter((a) => a.status === "running").length);
  readonly blocked = computed(() => this.runtime.agents().filter((a) => a.status === "blocked").length);

  readonly open = signal(false);

  // ---- gauge readouts ----
  // total cpu% used by katrix + agents (machine-relative), to one decimal
  readonly cpuPct = computed(() => Math.round((this.metrics.metrics()?.totalCpu ?? 0) * 10) / 10);
  readonly cpuBar = computed(() => Math.min(100, Math.max(0, this.cpuPct())));
  // total memory used by katrix + agents (e.g. "432.3 MB")
  readonly totalMem = computed(() => this.fmtMem(this.metrics.metrics()?.totalMemBytes ?? 0));
  // app subtree first, then agents, both already in backend order
  readonly procs = computed(() => this.metrics.metrics()?.procs ?? []);

  toggle(e: MouseEvent) {
    e.stopPropagation();
    this.open.update((v) => !v);
  }

  fmtCpu(cpu: number): number {
    return Math.round(cpu * 10) / 10;
  }

  /** Human-readable bytes: B / KB / MB / GB, one decimal above KB. */
  fmtMem(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    const kb = bytes / 1024;
    if (kb < 1024) return `${Math.round(kb)} KB`;
    const mb = kb / 1024;
    if (mb < 1024) return `${mb.toFixed(1)} MB`;
    return `${(mb / 1024).toFixed(1)} GB`;
  }

  // close on outside click / Esc (mirrors context-menu.component.ts)
  @HostListener("document:mousedown", ["$event"])
  onDown(e: MouseEvent) {
    if (!this.open()) return;
    if (!this.host.nativeElement.contains(e.target as Node)) this.open.set(false);
  }

  @HostListener("document:keydown.escape")
  onEsc() {
    if (this.open()) this.open.set(false);
  }
}
