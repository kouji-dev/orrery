import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  HostListener,
  inject,
  input,
  signal,
} from "@angular/core";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";

/**
 * Confirmation gate for "Delete worktree" — the one destructive, disk-touching
 * action in the agent menu. Explains the full blast radius (tabs close, a
 * running agent is stopped) before
 * {@link AgentActionsService.confirmRemoveAgent} runs it.
 *
 * The blast radius is not fixed: the "Hard delete" checkbox decides whether the
 * worktree FOLDER goes too. It defaults to off, and the body copy plus the
 * confirm button both restate which of the two deletes is about to happen — the
 * modal must never describe a deletion it is not going to perform.
 */
@Component({
  selector: "app-delete-worktree-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div
      (click)="ui.closeDeleteWorktree()"
      style="position:fixed;inset:0;z-index:60;display:grid;place-items:center;padding:var(--sp-9);background:var(--scrim);backdrop-filter:blur(3px)"
    >
      <div
        class="surface rise"
        (click)="$event.stopPropagation()"
        style="width:440px;padding:0;overflow:hidden;box-shadow:var(--shadow)"
      >
        <div style="padding:var(--sp-6) var(--sp-7);border-bottom:1px solid var(--hair);display:flex;align-items:center;gap:var(--sp-4)">
          <span class="dw-badge"><app-icon name="trash" size="sm" color="var(--st-blocked)" /></span>
          <span class="disp" style="font-size:var(--fs-lg);font-weight:600;white-space:nowrap">Delete worktree?</span>
        </div>

        <div style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-5)">
          @if (agent(); as ag) {
            <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md);background:var(--panel-2);border:1px solid var(--hair)">
              <app-icon name="branch" size="sm" color="var(--ink-4)" />
              <span style="font-size:var(--fs-sm);color:var(--ink);font-weight:600">{{ ag.name }}</span>
              <span style="margin-left:auto;font-family:var(--font-mono);font-size:var(--fs-2xs);color:var(--ink-3)">{{ ag.branch }}</span>
            </div>
            <div style="font-size:var(--fs-sm);color:var(--ink-2);line-height:1.55">
              This closes the worktree's tabs,
              @if (ag.status === "running") { stops the <b style="color:var(--ink)">running</b> agent, }
              and removes the agent.
              @if (hard()) {
                Its folder is <b style="color:var(--ink)">deleted from disk</b>, including any
                <b style="color:var(--ink)">uncommitted changes</b>.
              } @else {
                Its folder stays on disk — you can delete it yourself later.
              }
            </div>

            <label class="dw-hard" [class.on]="hard()">
              <input type="checkbox" [checked]="hard()" (change)="hard.set($any($event.target).checked)" />
              <span class="dw-hard-t">
                <span class="dw-hard-l">Hard delete</span>
                <span class="dw-hard-h">Also delete the worktree folder from disk. This cannot be undone.</span>
              </span>
            </label>
          }
        </div>

        <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;justify-content:flex-end;gap:var(--sp-4)">
          <button class="btn ghost-hair" (click)="ui.closeDeleteWorktree()">Cancel</button>
          <button class="dw-btn-danger" (click)="confirm()">
            <app-icon name="trash" size="sm" />{{ hard() ? "Delete worktree + folder" : "Delete worktree" }}
          </button>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
      .dw-badge {
        flex: none;
        width: var(--sp-9);
        height: var(--sp-9);
        border-radius: 7px;
        display: grid;
        place-items: center;
        background: color-mix(in oklch, var(--st-blocked), transparent 88%);
        border: 1px solid color-mix(in oklch, var(--st-blocked), transparent 60%);
      }
      .dw-hard {
        display: flex;
        align-items: flex-start;
        gap: var(--sp-4);
        padding: var(--sp-5) var(--sp-6);
        border-radius: var(--r-md);
        border: 1px solid var(--hair);
        background: var(--panel-2);
        cursor: pointer;
        transition:
          border-color 0.12s,
          background 0.12s;
      }
      .dw-hard:hover {
        border-color: var(--hair-2);
      }
      /* checked reads as armed: the row picks up the same red as the action */
      .dw-hard.on {
        border-color: color-mix(in oklch, var(--st-blocked), transparent 55%);
        background: color-mix(in oklch, var(--st-blocked), transparent 92%);
      }
      .dw-hard input {
        accent-color: var(--st-blocked);
        flex: none;
        margin: var(--sp-1) 0 0;
        cursor: pointer;
      }
      .dw-hard-t {
        display: flex;
        flex-direction: column;
        gap: var(--sp-1);
      }
      .dw-hard-l {
        font-size: var(--fs-sm);
        font-weight: 600;
        color: var(--ink);
      }
      .dw-hard-h {
        font-size: var(--fs-2xs);
        color: var(--ink-3);
        line-height: 1.5;
      }
      .dw-btn-danger {
        display: inline-flex;
        align-items: center;
        gap: var(--sp-3);
        height: var(--ctl-h);
        padding: 0 var(--sp-6);
        border-radius: 7px;
        border: none;
        background: var(--sem-del);
        color: var(--on-solid);
        font-family: var(--font-ui);
        font-size: var(--fs-sm);
        font-weight: 600;
        cursor: pointer;
        transition: filter 0.12s;
      }
      .dw-btn-danger:hover {
        filter: brightness(1.08);
      }
    `,
  ],
})
export class DeleteWorktreeModalComponent {
  readonly ui = inject(UiStore);
  private readonly actions = inject(AgentActionsService);
  private readonly runtime = inject(AgentRuntimeService);

  readonly agentId = input.required<string>();
  readonly agent = computed(() => this.runtime.agents().find((a) => a.id === this.agentId()) ?? null);

  /** "Also delete the folder". Off by default — the destructive half of a
   *  destructive action is always an explicit second choice. */
  readonly hard = signal(false);

  constructor() {
    // The agent can vanish while the modal is up (removed elsewhere, backend
    // event) — a confirm for a gone agent is meaningless, so self-close.
    effect(() => {
      if (!this.agent()) this.ui.closeDeleteWorktree();
    });
    // Close-then-reopen inside a single change-detection pass leaves the @if
    // view — and this component instance — alive, with only agentId changing.
    // Without this, an armed checkbox would carry into the NEXT agent's dialog
    // and hard-delete a worktree nobody asked to erase. Re-arm per target.
    effect(() => {
      this.agentId();
      this.hard.set(false);
    });
  }

  @HostListener("document:keydown.escape")
  onEscape() {
    this.ui.closeDeleteWorktree();
  }

  confirm() {
    void this.actions.confirmRemoveAgent(this.agentId(), this.hard());
  }
}
