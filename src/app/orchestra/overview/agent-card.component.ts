import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { RingComponent } from "../shared/ring.component";
import { StatusPillComponent } from "../shared/status-pill.component";
import { fmtDur, mix, STATUS_META } from "../utils";
import { MiniTermComponent } from "./mini-term.component";

@Component({
  selector: "app-agent-card",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RingComponent, StatusPillComponent, IconComponent, MiniTermComponent],
  template: `
    @let ag = agent();
    <div
      class="surface rise agent-card"
      (click)="ui.openAgent(ag.id)"
      style="padding:14px;display:flex;flex-direction:column;gap:11px;cursor:pointer;transition:border-color 0.15s,transform 0.15s"
    >
      <div style="display:flex;align-items:flex-start;gap:10px">
        <div style="position:relative;flex:none" [class.working]="ag.working">
          <app-ring [value]="ringValue()" [size]="36" [stroke]="3" [color]="meta().color" />
          <span class="tnum" style="position:absolute;inset:0;display:grid;place-items:center;font-size:9px;color:var(--ink-2)">{{ centerLabel() }}</span>
        </div>
        <div style="flex:1;min-width:0">
          <div style="display:flex;align-items:center;gap:7px;min-width:0">
            <span class="disp" style="font-size:14px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;flex:0 1 auto">{{ ag.name }}</span>
            <span style="flex:none"><app-status-pill [status]="ag.status" [filled]="true" /></span>
          </div>
          <div style="font-size:11px;color:var(--ink-3);margin-top:2px;display:flex;gap:6px;align-items:center;min-width:0">
            @if (proj()) {
              <span [style.color]="proj()!.color" style="display:flex;align-items:center;gap:4px;min-width:0;overflow:hidden">
                <app-icon [name]="proj()!.icon" size="sm" [px]="11" />
                <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ proj()!.name }}</span>
              </span>
            }
            <span style="color:var(--ink-4);flex:none">·</span>
            <app-icon name="branch" size="sm" [px]="11" />
            <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch.replace('agent/', '') }}</span>
          </div>
        </div>
      </div>

      <p style="font-size:12px;color:var(--ink-2);line-height:1.5;text-wrap:pretty;min-height:34px">{{ ag.task }}</p>

      @if (ag.status === 'blocked') {
        <div
          [style.background]="mix('var(--st-blocked)', 90)"
          [style.border]="'1px solid ' + mix('var(--st-blocked)', 70)"
          style="display:flex;gap:7px;padding:7px 9px;border-radius:var(--r-sm)"
        >
          <app-icon name="flag" size="sm" color="var(--st-blocked)" style="flex:none;margin-top:1px" />
          <span style="font-size:11px;color:var(--code-del-ink);line-height:1.45">{{ ag.blockReason }}</span>
        </div>
      }

      <app-mini-term [agentId]="ag.id" />

      <div class="tnum" style="display:flex;align-items:center;gap:10px;font-size:10.5px;color:var(--ink-3)">
        <span style="display:flex;gap:4px"><app-icon name="file" size="sm" [px]="11" />{{ (ag.git_changes?.files?.length ?? 0) }}</span>
        <span style="color:var(--code-add-ink)">+{{ totAdd() }}</span>
        <span style="color:var(--code-del-ink)">−{{ totDel() }}</span>
        <span style="display:flex;gap:4px"><app-icon name="commit" size="sm" [px]="11" />{{ ag.commits }}</span>
        <span style="margin-left:auto;display:flex;gap:4px;color:var(--ink-4)">
          <app-icon name="clock" size="sm" [px]="11" />{{ ag.elapsed ? fmt(ag.elapsed) : '—' }}
        </span>
      </div>

      <div style="display:flex;gap:6px" (click)="$event.stopPropagation()">
        @switch (ag.status) {
          @case ('done') {
            <button class="btn primary" style="flex:1;justify-content:center" (click)="agentActions.act(ag.id, 'merge')"><app-icon name="merge" size="sm" />Merge</button>
          }
          @case ('blocked') {
            <button class="btn primary" style="flex:1;justify-content:center" (click)="ui.openAgent(ag.id)"><app-icon name="chat" size="sm" />Answer</button>
          }
          @case ('queued') {
            <button class="btn ghost-hair" style="flex:1;justify-content:center" (click)="agentActions.act(ag.id, 'start')"><app-icon name="play" size="sm" />Start now</button>
          }
          @default {
            <button class="btn ghost-hair" style="flex:1;justify-content:center" (click)="agentActions.act(ag.id, ag.status === 'running' ? 'pause' : ag.started ? 'resume' : 'start')">
              <app-icon [name]="ag.status === 'running' ? 'pause' : 'play'" size="sm" />{{ ag.status === 'running' ? 'Pause' : ag.started ? 'Resume' : 'Start' }}
            </button>
          }
        }
        <button class="btn ghost-hair" (click)="ui.openAgent(ag.id)" style="padding:5px 9px"><app-icon name="terminal" size="sm" />Open</button>
      </div>
    </div>
  `,
  styles: [
    `
      .agent-card:hover {
        border-color: var(--hair-2) !important;
        transform: translateY(-2px);
      }
      /* breathing pulse on the liveness ring while the agent is actively working */
      .working {
        animation: livePulse 1.4s ease-in-out infinite;
      }
      @keyframes livePulse {
        0%,
        100% {
          opacity: 1;
        }
        50% {
          opacity: 0.5;
        }
      }
    `,
  ],
})
export class AgentCardComponent {
  readonly ui = inject(UiStore);
  readonly agentActions = inject(AgentActionsService);
  readonly agent = input.required<Agent>();
  readonly proj = input<Project | undefined>(undefined);

  readonly fmt = fmtDur;
  readonly mix = mix;
  readonly meta = computed(() => STATUS_META[this.agent().status]);
  // ring + center now reflect REAL liveness (not a fabricated %): a full status
  // ring, with the center showing live/idle/done rather than a fake percentage.
  readonly ringValue = computed(() => {
    switch (this.agent().status) {
      case "done":
      case "running":
      case "blocked":
        return 1;
      case "waiting":
        return 0.66;
      default:
        return 0.12; // idle / queued
    }
  });
  readonly centerLabel = computed(() => {
    const a = this.agent();
    switch (a.status) {
      case "done":
        return "✓";
      case "blocked":
        return "!";
      case "waiting":
        return "wait";
      case "queued":
        return "q";
      case "idle":
        return "—";
      default:
        return a.working ? "live" : a.needsInput ? "input" : "idle"; // running
    }
  });
  readonly totAdd = computed(() => (this.agent().git_changes?.files ?? []).reduce((s, f) => s + f.add, 0));
  readonly totDel = computed(() => (this.agent().git_changes?.files ?? []).reduce((s, f) => s + f.del, 0));
}
