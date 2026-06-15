import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  signal,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { STATUS_META } from "../utils";

/**
 * Compact in-card agent strip: shows tool badge, name, status pill+dot,
 * progress bar, branch/blockReason, and commit count.
 * Clicking opens the agent's workspace tab.
 */
@Component({
  selector: "app-agent-strip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, ToolBadgeComponent],
  template: `
    @if (agent(); as ag) {
      @let m = meta();
      <div
        (click)="open($event)"
        (mouseenter)="hovered.set(true)"
        (mouseleave)="hovered.set(false)"
        title="Open agent workspace"
        [style.background]="'var(--panel-2)'"
        [style.border]="'1px solid ' + (hovered() ? 'color-mix(in oklch, ' + m.color + ', transparent 55%)' : 'var(--hair)')"
        [style.box-shadow]="hovered() ? '0 0 0 1px color-mix(in oklch, ' + m.color + ', transparent 72%)' : 'none'"
        style="display:flex;flex-direction:column;gap:7px;padding:8px 9px;cursor:pointer;border-radius:var(--r-md);transition:border-color .15s,box-shadow .15s"
      >
        <!-- row 1: badge + name + pill + enter icon -->
        <div style="display:flex;align-items:center;gap:7px;min-width:0">
          <app-tool-badge [tool]="ag.tool" [size]="17" />
          <span
            class="disp"
            style="font-size:12px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:0 1 auto"
          >{{ ag.name }}</span>
          <span
            style="display:inline-flex;align-items:center;gap:5px;flex:none;font-size:9.5px;padding:2px 7px 2px 6px;border-radius:999px;letter-spacing:.08em;text-transform:uppercase"
            [style.color]="m.color"
            [style.border]="'1px solid color-mix(in oklch, ' + m.color + ', transparent 62%)'"
            [style.background]="'color-mix(in oklch, ' + m.color + ', transparent 88%)'"
          >
            <app-status-dot [status]="ag.status" />{{ m.label }}
          </span>
          <app-icon
            name="enter"
            size="sm"
            [color]="hovered() ? m.color : 'var(--ink-4)'"
            style="margin-left:auto;flex:none;transition:color .15s"
          />
        </div>

        <!-- progress bar -->
        <div style="height:3px;border-radius:3px;background:var(--hair);overflow:hidden;position:relative">
          <div
            [style.width]="progressPct() + '%'"
            [style.background]="m.color"
            [style.opacity]="ag.status === 'blocked' ? '0.5' : '1'"
            style="height:100%;border-radius:3px;transition:width .6s ease"
          ></div>
        </div>

        <!-- row 3: branch/blockReason + commits -->
        <div class="tnum" style="display:flex;align-items:center;gap:6px;font-size:10px;color:var(--ink-3)">
          @if (ag.status === 'blocked' && ag.blockReason) {
            <span style="display:flex;align-items:center;gap:5px;min-width:0;overflow:hidden" [style.color]="'var(--code-del-ink)'">
              <app-icon name="flag" size="sm" [px]="11" style="flex:none" />
              <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.blockReason }}</span>
            </span>
          } @else {
            <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch.replace('agent/', '') }}</span>
          }
          <span style="margin-left:auto;display:flex;gap:4px;flex:none;color:var(--ink-4)">
            <app-icon name="commit" size="sm" [px]="11" />{{ ag.commits }}
          </span>
        </div>
      </div>
    }
  `,
})
export class AgentStripComponent {
  readonly agentId = input.required<string>();

  private readonly runtime = inject(AgentRuntimeService);
  private readonly ui = inject(UiStore);

  readonly hovered = signal(false);

  readonly agent = computed(() =>
    this.runtime.agents().find((a) => a.id === this.agentId()),
  );

  readonly meta = computed(() => {
    const ag = this.agent();
    if (!ag) return STATUS_META["idle"];
    return STATUS_META[ag.status] ?? STATUS_META["idle"];
  });

  readonly progressPct = computed(() => {
    const ag = this.agent();
    return ag ? Math.max((ag.progress ?? 0) * 100, 4) : 4;
  });

  open(e: MouseEvent) {
    e.stopPropagation();
    this.ui.openAgent(this.agentId());
  }
}
