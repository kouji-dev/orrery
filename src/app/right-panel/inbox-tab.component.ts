import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent } from "../models";
import { NotificationService } from "../notifications/notification.service";
import { NotificationCardComponent } from "../notifications/notification-card.component";
import { IconComponent } from "../shared/icon.component";
import { EmptyStateComponent } from "./empty-state.component";

/** Full notification feed for the active agent (or all agents) — the same real
 *  feed the top-bar bell shows, scoped to this panel. Keeps history: resolved
 *  cards stay (dimmed) so past decisions remain visible; the badge upstream
 *  still tracks only the pending subset. */
@Component({
  selector: "app-inbox-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NotificationCardComponent, IconComponent, EmptyStateComponent],
  template: `
    @if (items().length) {
      <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-4) var(--sp-6) var(--sp-3);border-bottom:1px solid var(--hair)">
        <span class="up" style="font-size:var(--fs-2xs);letter-spacing:0.08em;color:var(--ink-3)">
          {{ scopeAgent() ? '' : 'All projects · ' }}{{ pendingCount() }} pending · {{ items().length }} total
        </span>
        @if (hasResolved()) {
          <button class="btn ghost-hair" style="margin-left:auto;padding:var(--sp-1) var(--sp-3);font-size:var(--fs-xs)" (click)="notifications.clearResolved()">
            <app-icon name="x" size="sm" />Clear read
          </button>
        }
      </div>
      <div class="scroll-y" style="flex:1;padding-bottom:var(--sp-4)">
        @for (n of items(); track n.id) {
          <app-notification-card [notification]="n" />
        }
      </div>
    } @else {
      <app-empty-state icon="bell" [text]="emptyText()" />
    }
  `,
})
export class InboxTabComponent {
  readonly notifications = inject(NotificationService);
  readonly scopeAgent = input<Agent | null>(null);

  /** Full feed (newest-first), scoped to the agent — history is preserved. */
  readonly items = computed(() => {
    const all = this.notifications.all();
    const sa = this.scopeAgent();
    return sa ? all.filter((n) => n.agentId === sa.id) : all;
  });
  readonly pendingCount = computed(() => this.items().filter((n) => n.status === "pending").length);
  readonly hasResolved = computed(() => this.items().some((n) => n.status !== "pending"));
  readonly emptyText = computed(() => {
    const sa = this.scopeAgent();
    return sa ? "No notifications for " + sa.name : "Inbox zero — no notifications";
  });
}
