import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { Agent } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";

@Component({
  selector: "app-spawn-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ToolBadgeComponent],
  template: `
    @let proj = project();
    @let tool = currentTool();
    <div
      (click)="ui.closeSpawn()"
      style="position:fixed;inset:0;z-index:60;display:grid;place-items:center;padding:24px;background:rgba(0,0,0,0.5);backdrop-filter:blur(3px)"
    >
      <div
        class="surface rise"
        (click)="$event.stopPropagation()"
        style="width:540px;max-height:90vh;display:flex;flex-direction:column;padding:0;overflow:hidden;box-shadow:var(--shadow)"
      >
        <div style="padding:14px 18px;border-bottom:1px solid var(--hair);display:flex;align-items:center;gap:9px;flex:none">
          <app-icon name="agent" color="var(--accent)" />
          <span class="disp" style="font-size:14px;font-weight:600;white-space:nowrap">Spawn agent</span>
          <span class="chip" style="margin-left:auto;font-size:9.5px">new git worktree + branch</span>
        </div>

        <div class="scroll-y" style="padding:18px;display:flex;flex-direction:column;gap:16px;flex:1">
          <!-- project + branch -->
          <div style="display:flex;gap:14px">
            <div style="flex:1">
              <label class="field-label">Project</label>
              <select class="osel" [value]="projectId()" (change)="setProject($any($event.target).value)">
                @for (p of projects.all(); track p.id) { <option [value]="p.id" [selected]="p.id === projectId()">{{ p.name }}</option> }
              </select>
              <div style="font-size:9.5px;color:var(--ink-4);margin-top:6px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ proj.path }}</div>
            </div>
            <div style="flex:1">
              <label class="field-label">Source branch</label>
              <select class="osel" [value]="branch()" (change)="branch.set($any($event.target).value)">
                @for (b of proj.branches; track b) { <option [value]="b" [selected]="b === branch()">{{ b }}</option> }
              </select>
              @if (!proj.branches?.length) {
                <div style="font-size:9.5px;color:var(--st-blocked);margin-top:6px">no branch found — project git is not initialized</div>
              } @else {
                <div style="font-size:9.5px;color:var(--ink-4);margin-top:6px">base · {{ proj.head }}</div>
              }
            </div>
          </div>

          <!-- name (drives the worktree, unique per project) -->
          <div>
            <label class="field-label">Name</label>
            <div style="display:flex;align-items:center;gap:8px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 10px">
              <app-icon name="agent" size="sm" color="var(--ink-4)" />
              <input
                [value]="name()"
                (input)="name.set($any($event.target).value)"
                placeholder="e.g. fix-login-bug"
                style="flex:1;min-width:0;background:transparent;border:none;outline:none;padding:10px 0;color:var(--ink);font-family:var(--font-mono);font-size:12.5px"
              />
            </div>
            <div style="font-size:9.5px;color:var(--ink-4);margin-top:6px">unique per project · becomes the worktree → <span style="color:var(--ink-3)">{{ worktreePreview() }}</span></div>
          </div>

          <!-- agent tool -->
          <div>
            <label class="field-label">Agent</label>
            <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:7px">
              @for (tl of tools; track tl.id) {
                @let on = toolId() === tl.id;
                <button
                  class="btn"
                  (click)="setTool(tl.id)"
                  [style.border]="'1px solid ' + (on ? mix(tl.accent, 45) : 'var(--hair)')"
                  [style.background]="on ? mix(tl.accent, 88) : 'var(--panel-2)'"
                  style="flex-direction:column;gap:6px;padding:10px 6px;border-radius:var(--r-md)"
                >
                  <app-tool-badge [tool]="tl.id" [size]="20" />
                  <span [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'" style="font-size:10.5px">{{ tl.name }}</span>
                  @if (!runtime.toolAvailable(tl.id)) {
                    <span class="tnum" style="font-size:8px;color:var(--st-blocked)">not found</span>
                  }
                </button>
              }
            </div>
          </div>

          <!-- model + effort -->
          <div style="display:flex;gap:14px">
            <div style="flex:1">
              <label class="field-label">Model</label>
              <select class="osel" [value]="model()" (change)="model.set($any($event.target).value)">
                @for (m of tool.models; track m) { <option [value]="m" [selected]="m === model()">{{ m }}</option> }
              </select>
            </div>
            @if (tool.effort) {
              <div style="flex:1">
                <label class="field-label">Reasoning effort</label>
                <div style="display:flex;gap:6px">
                  @for (ef of tool.effort; track ef) {
                    <button
                      class="btn ghost-hair"
                      (click)="effort.set(ef)"
                      [style.border-color]="effort() === ef ? 'var(--accent)' : 'var(--hair)'"
                      [style.color]="effort() === ef ? 'var(--ink)' : 'var(--ink-3)'"
                      [style.background]="effort() === ef ? mix('var(--accent)', 90) : 'transparent'"
                      style="flex:1;justify-content:center;font-size:11px;text-transform:capitalize"
                    >{{ ef }}</button>
                  }
                </div>
              </div>
            }
          </div>

          <!-- initial prompt -->
          <div>
            <label class="field-label">Initial prompt</label>
            <textarea
              #promptEl
              [value]="prompt()"
              (input)="prompt.set($any($event.target).value)"
              rows="3"
              placeholder="Describe what this agent should do…"
              class="spawn-textarea"
              style="width:100%;resize:none;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:10px 12px;color:var(--ink);font-family:var(--font-mono);font-size:12.5px;line-height:1.5;outline:none"
            ></textarea>
          </div>
        </div>

        <div style="padding:12px 18px;border-top:1px solid var(--hair);display:flex;align-items:center;gap:8px;flex:none">
          <span style="font-size:10px;color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">→ {{ ui.worktreeRoot }}/{{ proj.id }}-…</span>
          <div style="margin-left:auto;display:flex;gap:8px;flex:none">
            <button class="btn ghost-hair" (click)="ui.closeSpawn()">Cancel</button>
            <button class="btn ghost-hair" [disabled]="!name().trim() || !branch()" (click)="submit(false)"><app-icon name="plus" size="sm" />Create</button>
            <button class="btn primary" [disabled]="!name().trim() || !branch()" (click)="submit(true)"><app-icon name="bolt" size="sm" />Spawn</button>
          </div>
        </div>
      </div>
    </div>
  `,
  styles: [`.spawn-textarea:focus { border-color: var(--accent) !important; }`],
})
export class SpawnModalComponent implements AfterViewInit {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly runtime = inject(AgentRuntimeService);
  readonly agentActions = inject(AgentActionsService);
  readonly tools = AGENT_TOOLS;
  readonly mix = mix;

  private defaultProject = this.ui.spawning()?.project ?? null;

  readonly projectId = signal<string>(this.defaultProject || this.projects.all()[0].id);
  readonly toolId = signal<Agent["tool"]>("claude");
  readonly name = signal("");
  readonly prompt = signal("");

  readonly currentTool = computed(() => AGENT_TOOLS.find((t) => t.id === this.toolId())!);
  readonly project = computed(
    () => this.projects.all().find((p) => p.id === this.projectId()) || this.projects.all()[0],
  );
  // mirror the backend slug so the user sees the worktree name they'll get
  readonly worktreePreview = computed(
    () => this.name().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "—",
  );

  readonly model = signal<string>(this.currentTool().models[0]);
  readonly effort = signal<string | null>(this.currentTool().effort ? "high" : null);
  // the backend guarantees a git project has ≥1 branch ("main"); take the first.
  readonly branch = signal<string>(this.project().branches?.[0] ?? "");

  private promptEl = viewChild<ElementRef<HTMLTextAreaElement>>("promptEl");

  ngAfterViewInit() {
    this.promptEl()?.nativeElement.focus();
  }

  setProject(id: string) {
    this.projectId.set(id);
    this.branch.set(this.project().branches?.[0] ?? "");
  }
  setTool(id: Agent["tool"]) {
    this.toolId.set(id);
    const tool = this.currentTool();
    this.model.set(tool.models[0]);
    this.effort.set(tool.effort ? "high" : null);
  }
  submit(start: boolean) {
    if (!this.name().trim() || !this.branch()) return;
    this.agentActions.spawn({
      projectId: this.projectId(),
      branch: this.branch(),
      toolId: this.toolId(),
      model: this.model(),
      effort: this.effort(),
      name: this.name().trim(),
      prompt: this.prompt().trim(),
      start,
    });
  }
}
