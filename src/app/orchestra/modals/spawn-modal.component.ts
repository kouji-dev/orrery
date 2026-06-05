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
import { OrchestraStore } from "../orchestra.store";
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
      (click)="store.closeSpawn()"
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
                @for (p of store.projects(); track p.id) { <option [value]="p.id">{{ p.name }}</option> }
              </select>
              <div style="font-size:9.5px;color:var(--ink-4);margin-top:6px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ proj.path }}</div>
            </div>
            <div style="flex:1">
              <label class="field-label">Source branch</label>
              <select class="osel" [value]="branch()" (change)="branch.set($any($event.target).value)">
                @for (b of (proj.branches ?? [proj.branch ?? 'main']); track b) { <option [value]="b">{{ b }}</option> }
              </select>
              <div style="font-size:9.5px;color:var(--ink-4);margin-top:6px">base · {{ proj.head }}</div>
            </div>
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
                </button>
              }
            </div>
          </div>

          <!-- model + effort -->
          <div style="display:flex;gap:14px">
            <div style="flex:1">
              <label class="field-label">Model</label>
              <select class="osel" [value]="model()" (change)="model.set($any($event.target).value)">
                @for (m of tool.models; track m) { <option [value]="m">{{ m }}</option> }
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
          <span style="font-size:10px;color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">→ {{ store.worktreeRoot }}/{{ proj.id }}-…</span>
          <div style="margin-left:auto;display:flex;gap:8px;flex:none">
            <button class="btn ghost-hair" (click)="store.closeSpawn()">Cancel</button>
            <button class="btn primary" (click)="submit()"><app-icon name="bolt" size="sm" />Spawn agent</button>
          </div>
        </div>
      </div>
    </div>
  `,
  styles: [`.spawn-textarea:focus { border-color: var(--accent) !important; }`],
})
export class SpawnModalComponent implements AfterViewInit {
  readonly store = inject(OrchestraStore);
  readonly tools = AGENT_TOOLS;
  readonly mix = mix;

  private defaultProject = this.store.spawning()?.project ?? null;

  readonly projectId = signal<string>(this.defaultProject || this.store.projects()[0].id);
  readonly toolId = signal<Agent["tool"]>("claude");
  readonly prompt = signal("");

  readonly currentTool = computed(() => AGENT_TOOLS.find((t) => t.id === this.toolId())!);
  readonly project = computed(
    () => this.store.projects().find((p) => p.id === this.projectId()) || this.store.projects()[0],
  );

  readonly model = signal<string>(this.currentTool().models[0]);
  readonly effort = signal<string | null>(this.currentTool().effort ? "high" : null);
  readonly branch = signal<string>(this.project().branch ?? "main");

  private promptEl = viewChild<ElementRef<HTMLTextAreaElement>>("promptEl");

  ngAfterViewInit() {
    this.promptEl()?.nativeElement.focus();
  }

  setProject(id: string) {
    this.projectId.set(id);
    this.branch.set(this.project().branch ?? "main");
  }
  setTool(id: Agent["tool"]) {
    this.toolId.set(id);
    const tool = this.currentTool();
    this.model.set(tool.models[0]);
    this.effort.set(tool.effort ? "high" : null);
  }
  submit() {
    this.store.spawn({
      projectId: this.projectId(),
      branch: this.branch(),
      toolId: this.toolId(),
      model: this.model(),
      effort: this.effort(),
      prompt: this.prompt().trim() || "Explore and improve the codebase",
    });
  }
}
