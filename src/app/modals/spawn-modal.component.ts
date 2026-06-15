import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  OnInit,
  signal,
  viewChild,
} from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { Agent, AgentTool, Ticket } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { effectiveEffort, effectiveModel, SettingsStore } from "../settings/settings.store";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";

/** Strip HTML tags to plain text (no DOM dependency — regex-based). */
function stripHtml(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&nbsp;/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** Convert a title to a slug (mirrors the backend worktree slug logic). */
function slugName(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 50);
}

@Component({
  selector: "app-spawn-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ToolBadgeComponent],
  template: `
    @let proj = project();
    @let tool = currentTool();
    @let linked = !!ticketId();
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

          <!-- ticket (optional) — prefills + links Name and Initial prompt -->
          <div>
            <label class="field-label">Ticket</label>
            <select
              class="osel"
              [value]="ticketId()"
              (change)="applyTicket($any($event.target).value)"
              [style.border-color]="linked ? 'color-mix(in oklch, var(--accent), transparent 55%)' : 'var(--hair)'"
            >
              <option value="" [selected]="!ticketId()">None — start from scratch</option>
              @if (openTicketsTodo().length) {
                <optgroup label="To do">
                  @for (t of openTicketsTodo(); track t.id) {
                    <option [value]="t.id" [selected]="t.id === ticketId()">{{ t.title }}</option>
                  }
                </optgroup>
              }
              @if (openTicketsInProgress().length) {
                <optgroup label="In progress">
                  @for (t of openTicketsInProgress(); track t.id) {
                    <option [value]="t.id" [selected]="t.id === ticketId()">{{ t.title }}</option>
                  }
                </optgroup>
              }
            </select>
            @if (linked) {
              <div style="display:flex;align-items:center;gap:5px;margin-top:6px;font-size:10px;color:var(--accent-2)">
                <app-icon name="link" size="sm" />Name linked · the ticket is prepended to the prompt on spawn
              </div>
            } @else {
              <div style="font-size:9.5px;color:var(--ink-4);margin-top:6px">optional · attach a ticket to base the agent on it</div>
            }
          </div>

          <!-- name (drives the worktree, unique per project) -->
          <div>
            <label class="field-label">Name</label>
            <div
              style="display:flex;align-items:center;gap:8px;background:var(--panel-2);border-radius:var(--r-md);padding:0 10px"
              [style.border]="linked ? '1px solid var(--accent)' : '1px solid var(--hair)'"
            >
              <app-icon name="agent" size="sm" color="var(--ink-4)" />
              <input
                [value]="name()"
                (input)="onNameInput($any($event.target).value)"
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

          <!-- initial prompt — the agent's own instructions. NOT prefilled from
               the ticket; when a ticket is linked its content is prepended
               ("Implement …") at spawn time (see composePrompt). -->
          <div>
            <label class="field-label">Initial prompt</label>
            <textarea
              #promptEl
              [value]="prompt()"
              (input)="onPromptInput($any($event.target).value)"
              rows="3"
              [placeholder]="linked ? 'Add extra instructions — the ticket is included automatically…' : 'Describe what this agent should do…'"
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
export class SpawnModalComponent implements AfterViewInit, OnInit {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly runtime = inject(AgentRuntimeService);
  readonly agentActions = inject(AgentActionsService);
  private readonly settingsStore = inject(SettingsStore);
  private readonly ticketsStore = inject(TicketsStore);
  readonly tools = AGENT_TOOLS;
  readonly mix = mix;

  private defaultProject = this.ui.spawning()?.project ?? null;

  readonly projectId = signal<string>(this.defaultProject || this.projects.all()[0].id);
  readonly toolId = signal<Agent["tool"]>(this.initialTool());
  readonly name = signal("");
  readonly prompt = signal("");

  /** "" = None (no ticket linked) */
  readonly ticketId = signal<string>("");

  /** Track whether the user has manually typed in the Name field, so a ticket
   *  selection won't overwrite a name they've already customized. */
  private nameUserEdited = false;

  readonly currentTool = computed(() => AGENT_TOOLS.find((t) => t.id === this.toolId())!);
  readonly project = computed(
    () => this.projects.all().find((p) => p.id === this.projectId()) || this.projects.all()[0],
  );
  // mirror the backend slug so the user sees the worktree name they'll get
  readonly worktreePreview = computed(
    () => this.name().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "—",
  );

  readonly model = signal<string>(this.prefillModel(this.currentTool()));
  readonly effort = signal<string | null>(this.prefillEffort(this.currentTool()));
  // the backend guarantees a git project has ≥1 branch ("main"); take the first.
  readonly branch = signal<string>(this.project().branches?.[0] ?? "");

  /** Open tickets (todo + inprogress), same-project first, then rest. */
  private readonly openTickets = computed<Ticket[]>(() => {
    const all = this.ticketsStore.all();
    const open = all.filter((t) => t.status === "todo" || t.status === "inprogress");
    const pid = this.projectId();
    const sameProject = open.filter((t) => t.projectId === pid);
    const others = open.filter((t) => t.projectId !== pid);
    return [...sameProject, ...others];
  });

  readonly openTicketsTodo = computed<Ticket[]>(() =>
    this.openTickets().filter((t) => t.status === "todo"),
  );
  readonly openTicketsInProgress = computed<Ticket[]>(() =>
    this.openTickets().filter((t) => t.status === "inprogress"),
  );

  // ---- settings prefill (defaultTool / toolModel / toolEffort) ----
  /** The settings defaultTool when it names a DETECTED tool; the hardcoded
   *  default ("claude") otherwise — "" means nothing was ever saved. */
  private initialTool(): Agent["tool"] {
    const id = this.settingsStore.settings().defaultTool;
    const known = AGENT_TOOLS.some((t) => t.id === id);
    return known && this.runtime.toolAvailable(id) ? (id as Agent["tool"]) : "claude";
  }
  /** Per-tool settings model override while it's still in the curated list
   *  (a stale persisted id must not produce an unselectable <option>);
   *  otherwise the tool's first curated model — the old hardcoded default. */
  private prefillModel(tool: AgentTool): string {
    const m = effectiveModel(this.settingsStore.settings(), tool.id);
    return tool.models.includes(m) ? m : tool.models[0];
  }
  /** Per-tool settings effort override when valid; "high" (the old hardcoded
   *  default) otherwise; null for tools without effort levels. */
  private prefillEffort(tool: AgentTool): string | null {
    if (!tool.effort) return null;
    const e = effectiveEffort(this.settingsStore.settings(), tool.id);
    return tool.effort.includes(e) ? e : "high";
  }

  private promptEl = viewChild<ElementRef<HTMLTextAreaElement>>("promptEl");

  ngOnInit() {
    // Consume the dispatch ticket id (set by ui.dispatchTicket) and clear it
    // so it doesn't leak into subsequent manual spawns.
    const dispatched = this.ui.spawnTicketId();
    if (dispatched) {
      this.ui.clearSpawnTicket();
      this.applyTicket(dispatched);
    }
  }

  ngAfterViewInit() {
    // The Initial prompt starts empty (even when a ticket is linked), so focus
    // it so the user can add instructions immediately.
    this.promptEl()?.nativeElement.focus();
  }

  /** Called when the user manually edits the Name field. */
  onNameInput(value: string) {
    this.nameUserEdited = true;
    this.name.set(value);
  }

  /** Called when the user manually edits the Initial prompt field. */
  onPromptInput(value: string) {
    this.prompt.set(value);
  }

  /**
   * Apply a ticket selection: prefill the Name (unless the user already typed
   * one) and update the linked ticket id. The Initial prompt is intentionally
   * left untouched — the ticket's content is composed into the effective prompt
   * only at spawn time (see composePrompt). Passing "" clears the selection.
   */
  applyTicket(id: string) {
    this.ticketId.set(id);
    if (!id) return;

    const tk = this.ticketsStore.byId(id);
    if (!tk) return;

    // Switch to the ticket's project if it has one
    if (tk.projectId) {
      this.projectId.set(tk.projectId);
      this.branch.set(this.project().branches?.[0] ?? "");
    }

    // Prefill name only if the user hasn't manually typed anything
    if (!this.nameUserEdited || !this.name().trim()) {
      this.name.set(slugName(tk.title));
      this.nameUserEdited = false; // reset so next ticket selection can prefill again
    }
  }

  setProject(id: string) {
    this.projectId.set(id);
    this.branch.set(this.project().branches?.[0] ?? "");
  }
  setTool(id: Agent["tool"]) {
    this.toolId.set(id);
    const tool = this.currentTool();
    // switching tool applies THAT tool's settings defaults (or the curated ones)
    this.model.set(this.prefillModel(tool));
    this.effort.set(this.prefillEffort(tool));
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
      prompt: this.composePrompt(),
      ticketId: this.ticketId() || undefined,
      start,
    });
  }

  /**
   * The agent's effective initial prompt. With a linked ticket the ticket's
   * content leads — `Implement <ticket title + notes>` — and the user's own
   * prompt (if any) is appended after it: `Implement <ticket>, <user prompt>`.
   * Without a ticket it's just the user's prompt.
   */
  private composePrompt(): string {
    const userPrompt = this.prompt().trim();
    const id = this.ticketId();
    const tk = id ? this.ticketsStore.byId(id) : undefined;
    if (!tk) return userPrompt;
    const plain = stripHtml(tk.notes ?? "");
    const ticketPrompt = tk.title + (plain ? "\n\n" + plain : "");
    return userPrompt ? `Implement ${ticketPrompt}, ${userPrompt}` : `Implement ${ticketPrompt}`;
  }
}
