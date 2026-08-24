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
import { KjProgressBarComponent } from "@kouji-ui/components";

/**
 * Compact in-card agent strip: shows tool badge, name, status pill+dot,
 * progress bar, branch/blockReason, and commit count.
 * Clicking opens the agent's workspace tab.
 */
@Component({
  selector: "app-agent-strip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, ToolBadgeComponent, KjProgressBarComponent],
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
        style="display:flex;flex-direction:column;gap:var(--sp-3);padding:var(--sp-4) var(--sp-4);cursor:pointer;border-radius:var(--r-md);transition:border-color .15s,box-shadow .15s"
      >
        <!-- row 1: badge + name + pill + enter icon -->
        <div style="display:flex;align-items:center;gap:var(--sp-3);min-width:0">
          <app-tool-badge [tool]="ag.tool" [size]="17" />
          <h4 class="trunc" style="flex:0 1 auto">{{ ag.name }}</h4>
          <span
            class="chip up"
            style="flex:none;font-size:var(--fs-meta);letter-spacing:.08em"
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
        <kj-progress-bar
          [kjValue]="progressPct()"
          kjAriaLabel="Agent progress"
          [style.--kj-progress-bar-fill]="ag.status === 'blocked' ? 'color-mix(in oklch,' + m.color + ',transparent 50%)' : m.color"
          style="--kj-progress-bar-track:var(--hair);--kj-progress-bar-radius:3px;--kj-progress-bar-height:var(--sp-1)"
        />

        <!-- row 3: branch/blockReason + commits -->
        <div class="tnum" style="display:flex;align-items:center;gap:var(--sp-3);color:var(--ink-3)">
          @if (ag.status === 'blocked' && ag.blockReason) {
            <span style="display:flex;align-items:center;gap:var(--sp-2);min-width:0;overflow:hidden" [style.color]="'var(--code-del-ink)'">
              <app-icon size="md" name="flag" style="flex:none" />
              <span class="trunc">{{ ag.blockReason }}</span>
            </span>
          } @else {
            <span class="trunc">{{ ag.branch.replace('agent/', '') }}</span>
          }
          <span style="margin-left:auto;display:flex;gap:var(--sp-2);flex:none;color:var(--ink-4)">
            <app-icon size="md" name="commit" />{{ ag.commits }}
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
