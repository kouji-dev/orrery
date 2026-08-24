import { ChangeDetectionStrategy, Component, computed, inject, input, output } from "@angular/core";
import { Agent, AgentStatus, Project } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { mix } from "../utils";
import { AgentRowComponent } from "./agent-row.component";
import { KjButton } from "@kouji-ui/core";

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
  imports: [IconComponent, AgentRowComponent, KjButton],
  template: `
    @let p = project();
    <div style="margin-bottom:var(--sp-1)">
      <div
        class="proj-row"
        (click)="toggle.emit(p.id)"
        (contextmenu)="ui.openMenu($event, projects.projectMenu(p.id))"
        style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-5);cursor:pointer;position:relative;margin:0 var(--sp-3);border-radius:var(--r-md)"
      >
        <app-icon size="md" [name]="collapsed() ? 'chevron' : 'chevronD'" color="var(--ink-4)" />
        <span
          [style.background]="mix(p.color, 82)"
          [style.border]="'1px solid ' + mix(p.color, 62)"
          style="width:19px;height:19px;flex:none;border-radius:5px;display:grid;place-items:center"
        >
          <app-icon size="md" [name]="p.icon" [color]="p.color" />
        </span>
        <span
          [style.color]="p.folderExists ? 'var(--ink)' : 'var(--ink-3)'"
          class="trunc"
          style="font-weight:var(--fw-medium)"
        >{{ p.name }}</span>
        @if (!p.folderExists) {
          <app-icon size="md" name="flag" color="var(--st-blocked)" title="folder not found — right-click to relocate" />
        }
        @if (needs() > 0) {
          <span class="tnum" style="font-size:var(--fs-meta);font-weight:var(--fw-strong);color:var(--st-blocked)">{{ needs() }}!</span>
        }
        <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4);flex:none">{{ agents().length }}</span>
        <button kjButton
          class="proj-spawn"
          title="Spawn agent in this project"
          (click)="spawnHere($event)"
          style="background:transparent;border:none;color:var(--ink-3);cursor:pointer;display:flex;padding:var(--sp-1);border-radius:4px;flex:none"
        >
          <app-icon name="bolt" size="sm" />
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
            <div style="padding:var(--sp-2) var(--sp-5) var(--sp-3) var(--sp-10);font-size:var(--fs-meta);color:var(--ink-4)">no agents — spawn one</div>
          }
        </div>
      }
    </div>
  `,
  styles: [
    `
      .proj-spawn:hover {
        color: var(--ui-ink) !important;
        background: var(--panel-3) !important;
      }
    `,
  ],
})
export class ProjectGroupComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
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
    this.ui.openSpawn(this.project().id);
  }
}
