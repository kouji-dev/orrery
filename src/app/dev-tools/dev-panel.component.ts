import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ProjectsStore } from "../stores/projects.store";

/**
 * Live read-only view of the in-memory stores — a poor man's Redux devtools.
 * One simple table per entity. Reflects whatever the signals currently hold,
 * so it's the fastest way to see whether a round-trip actually landed in a store.
 */
@Component({
  selector: "app-dev-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <!-- launcher -->
    <button
      class="dbg-fab"
      (click)="open.set(!open())"
      title="Store inspector"
      style="position:fixed;right:18px;bottom:88px;z-index:70;width:42px;height:42px;border-radius:50%;display:grid;place-items:center;cursor:pointer;color:var(--ink);border:1px solid var(--hair-2);background:var(--elev);box-shadow:var(--shadow)"
    >
      <app-icon name="database" />
    </button>

    @if (open()) {
      <div
        class="rise"
        style="position:fixed;right:18px;bottom:140px;z-index:70;width:min(720px,92vw);max-height:76vh;overflow:auto;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-lg);box-shadow:var(--shadow);padding:14px 16px"
      >
        <div style="display:flex;align-items:center;gap:8px;margin-bottom:8px;position:sticky;top:0;background:var(--elev);padding-bottom:4px">
          <app-icon name="database" size="sm" color="var(--accent)" />
          <span class="disp" style="font-size:13px;font-weight:600">Store inspector</span>
          <button class="btn" (click)="open.set(false)" style="margin-left:auto;padding:3px"><app-icon name="x" size="sm" /></button>
        </div>

        <!-- Projects -->
        <div class="dbg-section">
          <span>ProjectsStore.projects</span>
          <span class="dbg-meta">{{ projects.all().length }} rows · loading={{ projects.loading() }}</span>
        </div>
        <div class="dbg-scroll"><table class="dbg">
          <thead><tr><th>id</th><th>name</th><th>path</th><th>hasGit</th><th>folderExists</th><th>branch</th><th>head</th></tr></thead>
          <tbody>
            @for (p of projects.all(); track p.id) {
              <tr>
                <td [title]="p.id">{{ p.id.slice(0, 8) }}</td>
                <td>{{ p.name }}</td>
                <td class="wrap" [title]="p.path">{{ p.path }}</td>
                <td>{{ bool(p.hasGit) }}</td>
                <td [style.color]="p.folderExists ? 'var(--st-done)' : 'var(--st-blocked)'">{{ bool(p.folderExists) }}</td>
                <td>{{ p.branch ?? "—" }}</td>
                <td>{{ p.head ?? "—" }}</td>
              </tr>
            } @empty {
              <tr><td colspan="7" class="dbg-empty">empty — nothing in the store</td></tr>
            }
          </tbody>
        </table></div>

        <!-- Agents -->
        <div class="dbg-section">
          <span>AgentRuntimeService.agents</span>
          <span class="dbg-meta">{{ runtime.agents().length }} rows</span>
        </div>
        <div class="dbg-scroll"><table class="dbg">
          <thead><tr><th>id</th><th>projectId</th><th>name</th><th>status</th><th>tool</th><th>branch</th><th>commits</th></tr></thead>
          <tbody>
            @for (a of runtime.agents(); track a.id) {
              <tr>
                <td [title]="a.id">{{ a.id }}</td>
                <td [title]="a.projectId">{{ a.projectId }}</td>
                <td>{{ a.name }}</td>
                <td>{{ a.status }}</td>
                <td>{{ a.tool }}</td>
                <td>{{ a.branch }}</td>
                <td>{{ a.commits }}</td>
              </tr>
            } @empty {
              <tr><td colspan="7" class="dbg-empty">empty</td></tr>
            }
          </tbody>
        </table></div>

        <!-- Tabs -->
        <div class="dbg-section">
          <span>UiStore.tabs</span>
          <span class="dbg-meta">active={{ ui.activeTab() }}</span>
        </div>
        <div class="dbg-scroll"><table class="dbg">
          <thead><tr><th>id</th><th>active</th></tr></thead>
          <tbody>
            @for (t of ui.tabs(); track t.id) {
              <tr><td>{{ t.id }}</td><td>{{ bool(t.id === ui.activeTab()) }}</td></tr>
            } @empty {
              <tr><td colspan="2" class="dbg-empty">empty</td></tr>
            }
          </tbody>
        </table></div>

        <!-- Commits -->
        <div class="dbg-section">
          <span>ProjectActionsService.commits</span>
          <span class="dbg-meta">{{ projectActions.commits().length }} rows</span>
        </div>
        <div class="dbg-scroll"><table class="dbg">
          <thead><tr><th>sha</th><th>agent</th><th>projectId</th><th>msg</th><th>when</th></tr></thead>
          <tbody>
            @for (c of projectActions.commits().slice(0, 12); track c.sha) {
              <tr>
                <td>{{ c.sha }}</td>
                <td>{{ c.agent }}</td>
                <td>{{ c.projectId ?? "—" }}</td>
                <td class="wrap" [title]="c.msg">{{ c.msg }}</td>
                <td>{{ c.when }}</td>
              </tr>
            } @empty {
              <tr><td colspan="5" class="dbg-empty">empty</td></tr>
            }
          </tbody>
        </table></div>
      </div>
    }
  `,
  styles: [
    `
      .dbg-fab:hover {
        border-color: var(--accent);
        color: var(--accent);
      }
      .dbg-section {
        display: flex;
        align-items: baseline;
        gap: 8px;
        margin: 14px 0 4px;
        font-family: var(--font-mono);
        font-size: 10.5px;
        color: var(--ink-2);
      }
      .dbg-section .dbg-meta {
        margin-left: auto;
        font-size: 9.5px;
        color: var(--ink-4);
      }
      table.dbg {
        width: 100%;
        border-collapse: collapse;
        font-family: var(--font-mono);
        font-size: 10.5px;
        table-layout: fixed;
      }
      table.dbg th,
      table.dbg td {
        text-align: left;
        padding: 3px 6px;
        border-bottom: 1px solid var(--hair);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
      }
      .dbg-scroll {
        max-height: 168px; /* ~7 rows + sticky header */
        overflow-y: auto;
      }
      table.dbg th {
        position: sticky;
        top: 0;
        background: var(--elev);
        color: var(--ink-4);
        font-weight: 600;
        border-bottom: 1px solid var(--hair-2);
      }
      table.dbg td.wrap {
        max-width: 220px;
      }
      table.dbg .dbg-empty {
        color: var(--ink-4);
        font-style: italic;
      }
    `,
  ],
})
export class DevPanelComponent {
  readonly runtime = inject(AgentRuntimeService);
  readonly ui = inject(UiStore);
  readonly projectActions = inject(ProjectActionsService);
  readonly projects = inject(ProjectsStore);
  readonly open = signal(false);

  bool(v: boolean): string {
    return v ? "✓" : "✗";
  }
}
