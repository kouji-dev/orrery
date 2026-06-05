import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { Agent } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { LogoComponent } from "./logo.component";
import { NotificationCenterComponent } from "./notification-center.component";

@Component({
  selector: "app-top-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [LogoComponent, IconComponent, StatusDotComponent, NotificationCenterComponent],
  template: `
    <header
      style="display:flex;align-items:stretch;background:var(--panel);border-bottom:1px solid var(--hair);height:44px;position:relative;z-index:5"
    >
      <!-- brand -->
      <div style="display:flex;align-items:center;gap:10px;padding:0 14px;flex:none">
        <app-logo />
        <div style="display:flex;flex-direction:column;line-height:1.15">
          <span class="disp" style="font-size:13px;font-weight:600;letter-spacing:0.02em">ORCHESTRA</span>
          <span style="font-size:9.5px;color:var(--ink-3);letter-spacing:0.04em">
            {{ projects.all().length }} projects · {{ runtime.agents().length }} agents
          </span>
        </div>
      </div>

      <div class="vdiv"></div>

      <!-- tabs -->
      <div style="display:flex;align-items:stretch;flex:1;min-width:0;overflow-x:auto">
        @for (tab of ui.tabs(); track tab.id) {
          @let ag = agentFor(tab.id);
          @let active = ui.activeTab() === tab.id;
          @let proj = ag ? projects.projectOf(ag.projectId) : null;
          @let isOrch = tab.id === 'orchestrator';
          <div
            (click)="ui.selectTab(tab.id)"
            (contextmenu)="onTabContext($event, tab.id)"
            [style.background]="active ? 'var(--panel-2)' : (isOrch ? 'var(--panel)' : 'transparent')"
            [style.color]="active ? 'var(--ink)' : 'var(--ink-3)'"
            [style.position]="isOrch ? 'sticky' : null"
            [style.left]="isOrch ? '0' : null"
            [style.z-index]="isOrch ? 2 : null"
            style="display:flex;align-items:center;gap:8px;padding:0 13px;cursor:pointer;white-space:nowrap;position:relative;flex:none;border-right:1px solid var(--hair)"
          >
            @if (active) {
              <span style="position:absolute;left:0;right:0;top:0;height:2px;background:linear-gradient(90deg,var(--accent),var(--accent-2))"></span>
            }
            @if (tab.id === 'orchestrator') {
              <app-icon name="layers" size="sm" [color]="active ? 'var(--accent)' : null" />
            } @else {
              <app-status-dot [status]="ag ? ag.status : 'idle'" />
            }
            @if (proj) {
              <span [style.background]="proj.color" [title]="proj.name" style="width:6px;height:6px;border-radius:2px;flex:none"></span>
            }
            <span style="font-size:12px">{{ tab.id === 'orchestrator' ? 'Orchestrator' : ag ? ag.name : tab.id }}</span>
            @if (tab.id !== 'orchestrator') {
              <button
                (click)="closeTab($event, tab.id)"
                class="tab-x"
                style="background:transparent;border:none;color:var(--ink-4);cursor:pointer;display:flex;padding:1px;border-radius:3px"
              >
                <app-icon name="x" size="sm" />
              </button>
            }
          </div>
        }
      </div>

      <div class="vdiv"></div>

      <!-- actions -->
      <div style="display:flex;align-items:center;gap:8px;padding:0 12px;flex:none">
        <app-notification-center />
        <button [class]="'btn ' + (ui.running() ? 'ghost-hair' : 'primary')" (click)="ui.toggleRunAll()">
          <app-icon [name]="ui.running() ? 'pause' : 'play'" size="sm" />
          {{ ui.running() ? 'Pause all' : 'Run all' }}
        </button>
        <button class="btn ghost-hair" (click)="ui.toggleTheme()" title="Toggle theme" style="padding:5px 8px">
          <app-icon [name]="ui.tweaks().theme === 'dark' ? 'sun' : 'moon'" size="sm" />
        </button>
      </div>
    </header>
  `,
  styles: [`.tab-x:hover { color: var(--ink) !important; }`],
})
export class TopBarComponent {
  readonly ui = inject(UiStore);
  readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);

  agentFor(id: string): Agent | null {
    return id === "orchestrator" ? null : (this.runtime.agents().find((a) => a.id === id) ?? null);
  }
  onTabContext(e: MouseEvent, id: string) {
    if (id !== "orchestrator") this.ui.openMenu(e, this.agentActions.agentMenu(id));
  }
  closeTab(e: MouseEvent, id: string) {
    e.stopPropagation();
    this.ui.closeTab(id);
  }
}
