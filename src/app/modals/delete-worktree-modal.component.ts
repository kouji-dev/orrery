import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  inject,
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
 * running agent is stopped, the worktree folder is deleted) before
 * {@link AgentActionsService.confirmRemoveAgent} runs it.
 *
 * Opened through `KjDialog` by the shell, so this component IS the overlay
 * panel: the backdrop, focus trap, scroll lock, Esc and outside-click all come
 * from the kj overlay and the markup below is only the panel body.
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
              and permanently deletes its folder — including any
              <b style="color:var(--ink)">uncommitted changes</b> — from disk.
            </div>
          }
        </div>

        <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;justify-content:flex-end;gap:var(--sp-4)">
          <kj-button kjVariant="outline" (click)="ui.closeDeleteWorktree()">Cancel</kj-button>
          <kj-button kjVariant="danger" (click)="confirm()"><app-icon name="trash" size="sm" />Delete worktree</kj-button>
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

  constructor() {
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.ui.closeDeleteWorktree());
    // The agent can vanish while the modal is up (removed elsewhere, backend
    // event) — a confirm for a gone agent is meaningless, so self-close.
    effect(() => {
      if (!this.agent()) this.ui.closeDeleteWorktree();
    });
  }

  confirm() {
    const id = this.agentId();
    if (id) void this.actions.confirmRemoveAgent(id);
  }
}
