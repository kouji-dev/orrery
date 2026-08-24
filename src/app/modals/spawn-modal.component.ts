import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { AGENT_TOOLS } from "../data";
import { Agent, AgentTool, Project, Ticket } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { effectiveEffort, effectiveModel, SettingsStore } from "../settings/settings.store";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { SelectComponent } from "../shared/select.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";
import {
  KjBadgeComponent,
  KjButtonComponent,
  KjDialogComponent,
  KjFieldComponent,
  KjFieldHelpComponent,
  KjFieldLabelComponent,
  KjInputComponent,
  KjInputGroupAddonComponent,
  KjInputGroupComponent,
  KjTextareaComponent,
} from "@kouji-ui/components";
import { KjDialog } from "@kouji-ui/core";

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

/**
 * Opened through `KjDialog` by the shell, so this component IS the overlay
 * panel: the backdrop, focus trap, scroll lock, Esc and outside-click all come
 * from the kj overlay and the markup below is only the panel body.
 */
@Component({
  selector: "app-spawn-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    SelectComponent,
    ToolBadgeComponent,
    KjBadgeComponent,
    KjButtonComponent,
    KjDialogComponent,
    KjFieldComponent,
    KjFieldHelpComponent,
    KjFieldLabelComponent,
    KjInputComponent,
    KjInputGroupAddonComponent,
    KjInputGroupComponent,
    KjTextareaComponent,
  ],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Spawn agent" },
  template: `
    @let proj = project();
    @let tool = currentTool();
    @let linked = !!ticketId();
    <kj-dialog-shell>
      <div class="kj-dialog rise">
        <div class="pane-head" style="padding:var(--sp-6) var(--sp-7)">
          <app-icon name="agent" color="var(--ui-ink)" />
          <h1 style="white-space:nowrap">Spawn agent</h1>
          <kj-badge style="margin-left:auto;font-size:var(--fs-meta)">new git worktree + branch</kj-badge>
        </div>

        <div class="scroll-y" style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-7);flex:1">
          <!-- project + branch -->
          <div style="display:flex;gap:var(--sp-6)">
            <div style="flex:1">
              <label class="field-label">Project</label>
              <app-select [value]="projectId()" [options]="projectOptions()" (valueChange)="setProject($event)" />
              <div class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-3)">{{ proj.path }}</div>
            </div>
            <div style="flex:1">
              <label class="field-label">Source branch</label>
              <app-select [value]="branch()" [options]="proj.branches ?? []" (valueChange)="branch.set($event)" />
              @if (!proj.branches?.length) {
                <div style="font-size:var(--fs-meta);color:var(--st-blocked);margin-top:var(--sp-3)">no branch found — project git is not initialized</div>
              } @else {
                <div style="font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-3)">base · {{ proj.head }}</div>
              }
            </div>
          </div>

          <!-- ticket (optional) — prefills + links Name and Initial prompt.
               Stays a native select: it needs <optgroup> (To do / In progress),
               which app-select / kj-select cannot express. -->
          <div>
            <label class="field-label">Ticket</label>
            <select
              class="osel"
              [value]="ticketId()"
              (change)="applyTicket($any($event.target).value)"
              [style.border-color]="linked ? 'var(--ui-line)' : 'var(--hair)'"
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
              <div style="display:flex;align-items:center;gap:var(--sp-2);margin-top:var(--sp-3);color:var(--ink-2)">
                <app-icon name="link" size="sm" />Name linked · the ticket is prepended to the prompt on spawn
              </div>
            } @else {
              <div style="font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-3)">optional · attach a ticket to base the agent on it</div>
            }
          </div>

          <!-- name (drives the worktree, unique per project) -->
          <kj-field class="spawn-field">
            <kj-field-label>Name</kj-field-label>
            <kj-input-group class="spawn-name" [style.--kj-input-group-border-color]="linked ? 'var(--ui-focus)' : null">
              <kj-input-group-addon><app-icon name="agent" size="sm" color="var(--ink-4)" /></kj-input-group-addon>
              <kj-input
                [value]="name()"
                (input)="onNameInput($any($event.target).value)"
                placeholder="e.g. fix-login-bug"
              />
            </kj-input-group>
            <kj-field-help>unique per project · becomes the worktree → <span style="color:var(--ink-3)">{{ worktreePreview() }}</span></kj-field-help>
          </kj-field>

          <!-- agent tool -->
          <kj-field class="spawn-field">
            <kj-field-label>Agent</kj-field-label>
            <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:var(--sp-3);width:100%">
              @for (tl of tools; track tl.id) {
                @let on = toolId() === tl.id;
                <kj-button kjVariant="ghost" (click)="setTool(tl.id)" [style.--kj-button-border-color]="on ? mix(tl.accent, 45) : 'var(--hair)'" [style.--kj-button-bg]="on ? mix(tl.accent, 88) : 'var(--panel-2)'">
                  <app-tool-badge [tool]="tl.id" [size]="20" />
                  <span [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'">{{ tl.name }}</span>
                  @if (!runtime.toolAvailable(tl.id)) {
                    <span class="tnum" style="font-size:var(--fs-badge);color:var(--st-blocked)">not found</span>
                  }
                </kj-button>
              }
            </div>
          </kj-field>

          <!-- model + effort -->
          <div style="display:flex;gap:var(--sp-6)">
            <kj-field class="spawn-field" style="flex:1">
              <kj-field-label>Model</kj-field-label>
              <app-select [value]="model()" [options]="tool.models" (valueChange)="model.set($event)" />
            </kj-field>
            @if (tool.effort) {
              <kj-field class="spawn-field" style="flex:1">
                <kj-field-label>Reasoning effort</kj-field-label>
                <div style="display:flex;gap:var(--sp-3);width:100%">
                  @for (ef of tool.effort; track ef) {
                    <kj-button kjVariant="outline" (click)="effort.set(ef)" [style.--kj-button-border-color]="effort() === ef ? 'var(--ui-focus)' : 'var(--hair)'" [style.--kj-button-fg]="effort() === ef ? 'var(--ink)' : 'var(--ink-3)'" [style.--kj-button-bg]="effort() === ef ? 'var(--ui-sel)' : 'transparent'" class="kj-center" style="--kj-button-font-size: var(--fs-meta)">{{ ef }}</kj-button>
                  }
                </div>
              </kj-field>
            }
          </div>

          <!-- initial prompt — the agent's own instructions. NOT prefilled from
               the ticket; when a ticket is linked its content is prepended
               ("Implement …") at spawn time (see composePrompt). -->
          <kj-field class="spawn-field">
            <kj-field-label>Initial prompt</kj-field-label>
            <kj-textarea
              #promptEl
              class="spawn-textarea"
              [kjValue]="prompt()"
              (input)="onPromptInput($any($event.target).value)"
              kjRows="3"
              kjResize="none"
              [kjPlaceholder]="linked ? 'Add extra instructions — the ticket is included automatically…' : 'Describe what this agent should do…'"
            />
          </kj-field>
        </div>

        <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;align-items:center;gap:var(--sp-4);flex:none">
          <span class="trunc" style="color:var(--ink-4)">→ {{ ui.worktreeRoot }}/{{ proj.id }}-…</span>
          <kj-button class="spawn-cancel" kjVariant="outline" (click)="ui.closeSpawn()">Cancel</kj-button>
          <kj-button kjVariant="outline" [kjDisabled]="!name().trim() || !branch()" (click)="submit(false)"><app-icon name="plus" size="sm" />Create</kj-button>
          <kj-button kjVariant="default" [kjDisabled]="!name().trim() || !branch()" (click)="submit(true)"><app-icon name="bolt" size="sm" />Spawn</kj-button>
        </div>
      </div>
    </kj-dialog-shell>
  `,
  styles: [
    `
      /* The panel box is the shared .kj-overlay-wrapper .kj-dialog recipe in
         styles.css; only this modal's width and height cap are per-instance. */
      .kj-dialog {
        width: round(calc(540px * var(--density)), 1px);
        max-height: 90vh;
      }
      /* kj-field label/help mapped onto the app's micro-label vocabulary */
      .spawn-field { --kj-field-gap: var(--sp-3); }
      /* the field label is the same micro-label role as .up; kouji owns the
         element so the recipe is restated here rather than classed */
      .spawn-field ::ng-deep .kj-field-label {
        font: var(--fw-normal) var(--fs-badge) / 1.4 var(--font-ui);
        color: var(--ink-3);
        text-transform: uppercase;
        letter-spacing: 0.12em;
      }
      .spawn-field ::ng-deep .kj-field-help { font-size: var(--fs-meta); color: var(--ink-4); }
      /* name input group: one panel-2 box, addon + input share the (linkable) border */
      .spawn-name { width: 100%; border-radius: var(--r-md); }
      .spawn-name ::ng-deep .kj-input-group__addon {
        background: var(--panel-2);
        border-color: var(--kj-input-group-border-color, var(--hair));
        border-right: none;
        color: var(--ink-4);
      }
      .spawn-name ::ng-deep .kj-input {
        background: var(--panel-2);
        border-color: var(--kj-input-group-border-color, var(--hair));
        border-left: none;
        box-shadow: none;
        color: var(--ink);
        font-family: var(--font-mono);
        padding: var(--sp-5) var(--sp-5) var(--sp-5) 0;
      }
      .spawn-name ::ng-deep .kj-input:focus-visible { outline: none; }
      /* kj-textarea restyled onto the app tokens (the old .spawn-textarea box) */
      .spawn-textarea ::ng-deep textarea {
        width: 100%;
        resize: none;
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-md);
        padding: var(--sp-5) var(--sp-6);
        color: var(--ink);
        font-family: var(--font-mono);
        line-height: 1.5;
        outline: none;
        box-shadow: none;
      }
      .spawn-textarea ::ng-deep textarea:focus {
        border-color: var(--ui-focus);
      }
      /* footer: the path label ellipsizes, the first button pushes the trio right */
      .spawn-cancel ::ng-deep .kj-button { margin-left: auto; }
    `,
  ],
})
export class SpawnModalComponent {
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
  /** Projects as app-select options (id → display name). */
  readonly projectOptions = computed(() => this.projects.all().map((p) => ({ value: p.id, label: p.name })));
  // mirror the backend slug so the user sees the worktree name they'll get
  readonly worktreePreview = computed(
    () => this.name().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "—",
  );

  readonly model = signal<string>(this.prefillModel(this.currentTool()));
  readonly effort = signal<string | null>(this.prefillEffort(this.currentTool()));
  // Pre-select the repo's default branch (origin/HEAD → main/master → HEAD);
  // fall back to the first branch in the list when no default resolves.
  readonly branch = signal<string>(this.defaultBranchFor(this.project()));

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
  /** Per-tool settings effort override when valid"high" (the old hardcoded
   *  default) otherwise; null for tools without effort levels. */
  private prefillEffort(tool: AgentTool): string | null {
    if (!tool.effort) return null;
    const e = effectiveEffort(this.settingsStore.settings(), tool.id);
    return tool.effort.includes(e) ? e : "high";
  }

  private promptEl = viewChild("promptEl", { read: ElementRef });

  constructor() {
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.ui.closeSpawn());

    // Consume the dispatch ticket id (set by ui.dispatchTicket) and clear it
    // so it doesn't leak into subsequent manual spawns.
    const dispatched = this.ui.spawnTicketId();
    if (dispatched) {
      this.ui.clearSpawnTicket();
      this.applyTicket(dispatched);
    }

    // The Initial prompt starts empty (even when a ticket is linked), so focus
    // it so the user can add instructions immediately (focus-only, no hook).
    afterNextRender(() => {
      (this.promptEl()?.nativeElement as HTMLElement | undefined)?.querySelector("textarea")?.focus();
    });
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
      this.branch.set(this.defaultBranchFor(this.project()));
    }

    // Prefill name only if the user hasn't manually typed anything
    if (!this.nameUserEdited || !this.name().trim()) {
      this.name.set(slugName(tk.title));
      this.nameUserEdited = false; // reset so next ticket selection can prefill again
    }
  }

  setProject(id: string) {
    this.projectId.set(id);
    this.branch.set(this.defaultBranchFor(this.project()));
  }

  /** The pre-selected source branch for a project: its resolved default branch,
   *  else the first branch in the list, else "" (non-git project). */
  private defaultBranchFor(project: Project): string {
    return project.defaultBranch ?? project.branches?.[0] ?? "";
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
