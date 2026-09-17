import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  inject,
  signal,
  untracked,
} from "@angular/core";
import { AGENT_TOOLS, defaultEffortFor, effortLevelsFor } from "../data";
import { Agent, AgentTool } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ModelCatalogService } from "../agents/model-catalog.service";
import { IconComponent } from "../shared/icon.component";
import {
  AgentToolControlsComponent,
  modelChoicesFor,
} from "../shared/agent-tool-controls.component";
import { UiStore } from "../ui/ui.store";
import { toolMeta } from "../utils";
import { KjBadgeComponent, KjButtonComponent, KjDialogComponent } from "@kouji-ui/components";
import { KjDialog } from "@kouji-ui/core";

/** The tool the agent runs on, as a tile id. "shell" (the v2 project
 *  pseudo-agent) has no tile and never reaches this dialog, but a record
 *  written by an older build can carry a tool id we no longer ship — fall back
 *  rather than render a picker with nothing selected. */
function toolIdOf(ag: Agent): AgentTool["id"] {
  return AGENT_TOOLS.some((t) => t.id === ag.tool) ? (ag.tool as AgentTool["id"]) : "claude";
}

/**
 * Change an existing agent's provider, model and reasoning effort.
 *
 * Until this existed every one of the three was frozen at spawn: the agent
 * record carried them, and every launch AND resume forwarded them, so resuming
 * an agent re-pinned whatever model it was created with and there was no way to
 * move it. The edit lands on the record, so the next launch picks it up.
 *
 * Opened through `KjDialog` by the shell, so this component IS the overlay
 * panel: the backdrop, focus trap, scroll lock, Esc and outside-click all come
 * from the kj overlay and the markup below is only the panel body.
 *
 * The confirm for a provider switch on a RUNNING agent is a second STEP of this
 * dialog rather than a second overlay: a dialog stacked on a dialog fights the
 * kj focus trap, and the delete-worktree pattern (its own store-flagged modal)
 * only works because nothing is mid-edit behind it. Cancelling that step falls
 * back into the form with nothing applied.
 */
@Component({
  selector: "app-edit-agent-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    AgentToolControlsComponent,
    KjBadgeComponent,
    KjButtonComponent,
    KjDialogComponent,
  ],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Edit agent" },
  template: `
    @if (agent(); as ag) {
      <kj-dialog-shell>
        <div class="kj-dialog rise">
          <div class="pane-head" style="padding:var(--sp-6) var(--sp-7)">
            <kj-badge class="ea-badge" size="sm"><app-icon name="rename" size="sm" color="var(--ui-ink)" /></kj-badge>
            <h1 style="white-space:nowrap">Edit agent</h1>
          </div>

          <div class="scroll-y" style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-7);flex:1">
            <!-- identity strip: the same panel-2 card the delete-worktree
                 confirm leads with, so both agent-scoped dialogs name their
                 target the same way -->
            <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md);background:var(--panel-2);border:1px solid var(--hair)">
              <app-icon name="agent" size="sm" color="var(--ink-4)" />
              <span style="color:var(--ink);font-weight:var(--fw-medium)">{{ ag.name }}</span>
              <code style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-3)">{{ ag.branch }}</code>
            </div>

            @if (confirming()) {
              <div class="ea-warn">
                <app-icon name="warn" size="sm" color="var(--sem-attn)" />
                <div>
                  Switching to <b style="color:var(--ink)">{{ nextToolName() }}</b> closes the
                  <b style="color:var(--ink)">running</b> {{ currentToolName() }} agent and starts
                  it again on the new provider. Its captured session is dropped with the old
                  provider, so the new run starts fresh — uncommitted work in the worktree is
                  untouched.
                </div>
              </div>
            } @else {
              <!-- the SAME control group the spawn dialog renders, prefilled
                   from the record instead of from the settings defaults -->
              <app-agent-tool-controls
                [tool]="toolId()"
                [model]="model()"
                [effort]="effort()"
                (toolChange)="setTool($event)"
                (modelChange)="setModel($event)"
                (effortChange)="effort.set($event)"
              />

              <!-- A model/effort change cannot reach a process that is already
                   running: the flags were spent on its command line at launch.
                   Saying so here is the difference between "nothing happened"
                   and "it lands next time". -->
              @if (ag.status === "running" && !toolChanged() && dirty()) {
                <div class="ea-note">
                  <app-icon name="clock" size="sm" color="var(--ink-4)" />
                  {{ ag.name }} is running — the new model applies the next time it starts.
                </div>
              }
            }
          </div>

          <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;justify-content:flex-end;gap:var(--sp-4)">
            @if (confirming()) {
              <kj-button kjVariant="outline" (click)="confirming.set(false)">Cancel</kj-button>
              <kj-button kjVariant="danger" (click)="save()">
                <app-icon name="refresh" size="sm" />Restart on {{ nextToolName() }}
              </kj-button>
            } @else {
              <kj-button kjVariant="outline" (click)="ui.closeEditAgent()">Cancel</kj-button>
              <kj-button kjVariant="default" [kjDisabled]="!dirty()" (click)="save()">
                <app-icon name="check" size="sm" />Save
              </kj-button>
            }
          </div>
        </div>
      </kj-dialog-shell>
    }
  `,
  styles: [
    `
      /* The panel box is the shared .kj-overlay-wrapper .kj-dialog recipe in
         styles.css; only this modal's width and height cap are per-instance —
         the density-scaled round(calc(Npx * --density)) is the app's own width
         convention (Spawn 600, Add project 480, Delete worktree 440). 560: it
         carries the same Model | Reasoning effort pair Spawn does, so it needs
         most of that width, but none of Spawn's project/branch or ticket/name
         rows. */
      .kj-dialog {
        width: round(calc(560px * var(--density)), 1px);
        max-height: 90vh;
      }
      /* icon bubble: kj-badge restyled to the square glyph chip the design uses
         (delete-worktree's, in the neutral accent — this dialog is not
         destructive until its confirm step) */
      .ea-badge ::ng-deep .kj-badge {
        flex: none;
        width: var(--sp-9);
        height: var(--sp-9);
        padding: 0;
        border-radius: 7px;
        display: grid;
        place-items: center;
        background: color-mix(in oklch, var(--ui-fill), transparent 88%);
        border: 1px solid color-mix(in oklch, var(--ui-fill), transparent 60%);
      }
      /* the confirm step's blast-radius copy — attention-tinted, not danger:
         the agent is restarted, not destroyed */
      .ea-warn {
        display: flex;
        align-items: flex-start;
        gap: var(--sp-4);
        padding: var(--sp-5) var(--sp-6);
        border-radius: var(--r-md);
        background: color-mix(in oklch, var(--sem-attn), transparent 92%);
        border: 1px solid color-mix(in oklch, var(--sem-attn), transparent 60%);
        color: var(--ink-2);
        line-height: 1.55;
      }
      .ea-warn app-icon {
        flex: none;
        margin-top: var(--sp-1);
      }
      /* the "applies next start" line: the same muted helper role as
         .set-row-help / kj-field-help elsewhere */
      .ea-note {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        font-size: var(--fs-meta);
        color: var(--ink-3);
      }
    `,
  ],
})
export class EditAgentModalComponent {
  readonly ui = inject(UiStore);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly actions = inject(AgentActionsService);
  private readonly catalog = inject(ModelCatalogService);

  /** The store owns which agent is being edited — the overlay carries no input,
   *  so read it straight off the signal that opened this dialog. */
  readonly agentId = computed(() => this.ui.editingAgent());
  readonly agent = computed(() => this.runtime.agents().find((a) => a.id === this.agentId()) ?? null);

  readonly toolId = signal<AgentTool["id"]>("claude");
  readonly model = signal<string>("");
  readonly effort = signal<string | null>(null);

  /** The provider switch is waiting on its confirm. Only ever true for a
   *  RUNNING agent whose tool changed — every other save applies straight away. */
  readonly confirming = signal(false);

  readonly currentTool = computed(
    () => AGENT_TOOLS.find((t) => t.id === this.toolId()) ?? AGENT_TOOLS[0],
  );
  readonly currentToolName = computed(() => toolMeta(this.agent()?.tool ?? "").name);
  readonly nextToolName = computed(() => this.currentTool().name);

  readonly toolChanged = computed(() => {
    const ag = this.agent();
    return !!ag && this.toolId() !== toolIdOf(ag);
  });
  /** Anything to send at all. `?? null` on both sides: the record stores "no
   *  effort" as either undefined or null depending on which backend version
   *  wrote it, and an undefined/null mismatch would arm Save on open. */
  readonly dirty = computed(() => {
    const ag = this.agent();
    if (!ag) return false;
    return (
      this.toolChanged() ||
      this.model() !== ag.model ||
      (this.effort() ?? null) !== (ag.effort ?? null)
    );
  });

  constructor() {
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.ui.closeEditAgent());
    // The agent can vanish while the modal is up (removed elsewhere, backend
    // event) — an edit for a gone agent is meaningless, so self-close.
    effect(() => {
      if (!this.agent()) this.ui.closeEditAgent();
    });
    // Close-then-reopen inside a single change-detection pass leaves the @if
    // view — and this component instance — alive, with only agentId changing.
    // Without this the PREVIOUS agent's draft (and a half-armed confirm) would
    // carry into the next agent's dialog and be saved onto it. Re-prefill per
    // target; the agent id, not the record, is the trigger, so a status flip
    // under the open dialog must not wipe what the user has picked.
    effect(() => {
      const id = this.agentId();
      this.confirming.set(false);
      // untracked: agents() churns constantly (status flips, elapsed, working),
      // and tracking it here would re-prefill — discarding the user's picks —
      // every time the agent they are editing so much as ticks.
      const ag = untracked(() => this.runtime.agents().find((a) => a.id === id));
      if (ag) this.prefill(ag);
    });
  }

  /** Seed the three controls from the record. The stored effort is kept only
   *  while the stored MODEL still accepts it — a record can carry a level the
   *  model no longer offers (Opus 4.6 lost `xhigh`), and pills with nothing
   *  selected read as "this agent has no effort" rather than as a stale value. */
  private prefill(ag: Agent): void {
    const id = toolIdOf(ag);
    const tool = AGENT_TOOLS.find((t) => t.id === id)!;
    if (tool.dynamicModels) this.catalog.load(id);
    this.toolId.set(id);
    this.model.set(ag.model);
    const levels = effortLevelsFor(tool, ag.model);
    const stored = ag.effort ?? null;
    this.effort.set(
      !levels ? null : stored && levels.includes(stored) ? stored : defaultEffortFor(tool, ag.model),
    );
  }

  /** Switching tool takes THAT tool's default model + effort — a model id is
   *  vocabulary-specific and means nothing to another CLI. Switching BACK to
   *  the agent's own tool restores what it actually has, so a stray click
   *  through the tiles can't quietly rewrite an untouched agent. */
  setTool(id: AgentTool["id"]): void {
    const ag = this.agent();
    if (ag && toolIdOf(ag) === id) {
      this.prefill(ag);
      return;
    }
    const tool = AGENT_TOOLS.find((t) => t.id === id)!;
    this.toolId.set(id);
    const first = modelChoicesFor(tool, this.catalog)[0]?.id ?? (tool.dynamicModels ? "" : tool.models[0].id);
    this.model.set(first);
    this.effort.set(defaultEffortFor(tool, first));
  }

  /** Picking a model re-validates the effort against what IT accepts: keep the
   *  level when offered, else fall to the model's default (or none). */
  setModel(id: string): void {
    this.model.set(id);
    const tool = this.currentTool();
    const levels = effortLevelsFor(tool, id);
    const cur = this.effort();
    this.effort.set(!levels ? null : cur && levels.includes(cur) ? cur : defaultEffortFor(tool, id));
  }

  /**
   * Save, or arm the confirm first. The gate is narrow on purpose: only a
   * PROVIDER change on a RUNNING agent costs anything (its live CLI has to go),
   * so a model or effort edit — which cannot touch a running process either way
   * — must not be made to feel destructive.
   */
  save(): void {
    const ag = this.agent();
    if (!ag || !this.dirty()) return;
    if (this.toolChanged() && ag.status === "running" && !this.confirming()) {
      this.confirming.set(true);
      return;
    }
    // effort travels even when null: an omitted key means "leave alone" to the
    // backend, so a tool with no effort knob would inherit the old tool's level
    void this.actions.applyAgentEdit(
      ag.id,
      { tool: this.toolId(), model: this.model(), effort: this.effort() },
      this.confirming(),
    );
  }
}
