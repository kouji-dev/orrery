import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  inject,
  signal,
} from "@angular/core";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { KjBadgeComponent, KjButtonComponent, KjDialogComponent } from "@kouji-ui/components";
import { KjDialog } from "@kouji-ui/core";

/**
 * Confirmation gate for "Delete worktree" — the one destructive, disk-touching
 * action in the agent menu. Explains the full blast radius (tabs close, a
 * running agent is stopped) before
 * {@link AgentActionsService.confirmRemoveAgent} runs it.
 *
 * Opened through `KjDialog` by the shell, so this component IS the overlay
 * panel: the backdrop, focus trap, scroll lock, Esc and outside-click all come
 * from the kj overlay and the markup below is only the panel body.
 *
 * The blast radius is not fixed: the "Hard delete" checkbox decides whether the
 * worktree FOLDER goes too. It defaults to off, and the body copy plus the
 * confirm button both restate which of the two deletes is about to happen — the
 * modal must never describe a deletion it is not going to perform.
 */
@Component({
  selector: "app-delete-worktree-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjBadgeComponent, KjButtonComponent, KjDialogComponent],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Delete worktree?" },
  template: `
    <kj-dialog-shell>
      <div class="kj-dialog rise">
        <div class="pane-head" style="padding:var(--sp-6) var(--sp-7)">
          <kj-badge class="dw-badge" size="sm"><app-icon name="trash" size="sm" color="var(--st-blocked)" /></kj-badge>
          <h1 style="white-space:nowrap">Delete worktree?</h1>
        </div>

        <div style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-5)">
          @if (agent(); as ag) {
            <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md);background:var(--panel-2);border:1px solid var(--hair)">
              <app-icon name="branch" size="sm" color="var(--ink-4)" />
              <span style="color:var(--ink);font-weight:var(--fw-medium)">{{ ag.name }}</span>
              <code style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-3)">{{ ag.branch }}</code>
            </div>
            <div style="color:var(--ink-2);line-height:1.55">
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
          <kj-button kjVariant="outline" (click)="ui.closeDeleteWorktree()">Cancel</kj-button>
          <kj-button kjVariant="danger" (click)="confirm()">
            <app-icon name="trash" size="sm" />{{ hard() ? "Delete worktree + folder" : "Delete worktree" }}
          </kj-button>
        </div>
      </div>
    </kj-dialog-shell>
  `,
  styles: [
    `
      /* The panel box is the shared .kj-overlay-wrapper .kj-dialog recipe in
         styles.css; only this modal's width is per-instance. */
      .kj-dialog {
        width: round(calc(440px * var(--density)), 1px);
      }
      /* icon bubble: kj-badge restyled to the square glyph chip the design uses */
      .dw-badge ::ng-deep .kj-badge {
        flex: none;
        width: var(--sp-9);
        height: var(--sp-9);
        padding: 0;
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
        font-size: var(--fs-body);
        font-weight: var(--fw-strong);
        color: var(--ink);
      }
      /* helper line under the label — the same role as .set-row-help */
      .dw-hard-h {
        font-size: var(--fs-meta);
        color: var(--ink-3);
        line-height: 1.5;
      }
    `,
  ],
})
export class DeleteWorktreeModalComponent {
  readonly ui = inject(UiStore);
  private readonly actions = inject(AgentActionsService);
  private readonly runtime = inject(AgentRuntimeService);

  /** The store owns which worktree is up for deletion — the overlay carries no
   *  input, so read it straight off the signal that opened this dialog. */
  readonly agentId = computed(() => this.ui.deletingWorktree());
  readonly agent = computed(() => this.runtime.agents().find((a) => a.id === this.agentId()) ?? null);

  /** "Also delete the folder". Off by default — the destructive half of a
   *  destructive action is always an explicit second choice. */
  readonly hard = signal(false);

  constructor() {
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.ui.closeDeleteWorktree());
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

  confirm() {
    const id = this.agentId();
    if (id) void this.actions.confirmRemoveAgent(id, this.hard());
  }
}
