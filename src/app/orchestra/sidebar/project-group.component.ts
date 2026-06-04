import { ChangeDetectionStrategy, Component, computed, inject, input, output } from "@angular/core";
import { Agent, AgentStatus, Project } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";
import { mix } from "../utils";
import { AgentRowComponent } from "./agent-row.component";

const STATUS_PRIORITY: Record<AgentStatus, number> = {
  blocked: 0,
  running: 1,
  waiting: 2,
  queued: 3,
  done: 4,
  idle: 5,
};

@Component({
  selector: "app-project-group",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, AgentRowComponent],
  template: `
    @let p = project();
    <div style="margin-bottom:2px">
      <div
        class="proj-row"
        (click)="toggle.emit(p.id)"
        (contextmenu)="store.openMenu($event, store.projectMenu(p.id))"
        style="display:flex;align-items:center;gap:8px;padding:7px 10px;cursor:pointer;position:relative;margin:0 6px;border-radius:var(--r-md)"
      >
        <app-icon [name]="collapsed() ? 'chevron' : 'chevronD'" size="sm" [px]="11" color="var(--ink-4)" />
        <span
          [style.background]="mix(p.color, 82)"
          [style.border]="'1px solid ' + mix(p.color, 62)"
          style="width:19px;height:19px;flex:none;border-radius:5px;display:grid;place-items:center"
        >
          <app-icon [name]="p.icon" size="sm" [px]="12" [color]="p.color" />
        </span>
        <span style="font-size:12.5px;font-weight:600;color:var(--ink);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ p.name }}</span>
        @if (needs() > 0) {
          <span class="tnum" style="font-size:9px;font-weight:700;color:var(--st-blocked)">{{ needs() }}!</span>
        }
        <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4);flex:none">{{ agents().length }}</span>
        <button
          class="proj-spawn"
          title="Spawn agent in this project"
          (click)="spawnHere($event)"
          style="background:transparent;border:none;color:var(--ink-3);cursor:pointer;display:flex;padding:2px;border-radius:4px;flex:none"
        >
          <app-icon name="plus" size="sm" />
        </button>
      </div>
      @if (!collapsed()) {
        <div style="position:relative">
          <span style="position:absolute;left:21px;top:0;bottom:4px;width:1px;background:var(--hair)"></span>
          @if (sorted().length) {
            @for (ag of sorted(); track ag.id) {
              <app-agent-row [agent]="ag" [active]="ag.id === activeAgent()" />
            }
          } @else {
            <div style="padding:4px 10px 6px 30px;font-size:10.5px;color:var(--ink-4)">no agents — spawn one</div>
          }
        </div>
      }
    </div>
  `,
  styles: [
    `
      .proj-spawn:hover {
        color: var(--accent) !important;
        background: var(--panel-3) !important;
      }
    `,
  ],
})
export class ProjectGroupComponent {
  readonly store = inject(OrchestraStore);
  readonly project = input.required<Project>();
  readonly agents = input.required<Agent[]>();
  readonly activeAgent = input<string | null>(null);
  readonly collapsed = input<boolean>(false);
  readonly toggle = output<string>();

  readonly mix = mix;
  readonly sorted = computed(() =>
    [...this.agents()].sort((a, b) => STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status]),
  );
  readonly needs = computed(() => this.agents().filter((a) => a.status === "blocked").length);

  spawnHere(e: MouseEvent) {
    e.stopPropagation();
    this.store.openSpawn(this.project().id);
  }
}
