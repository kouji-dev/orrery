import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent } from "../models";
import { NotificationService } from "../notifications/notification.service";
import { NotificationCardComponent } from "../notifications/notification-card.component";
import { EmptyStateComponent } from "./empty-state.component";

/** Pending notifications for the active agent (or all agents) — the same real
 *  feed the top-bar bell shows, scoped to this panel. */
@Component({
  selector: "app-inbox-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NotificationCardComponent, EmptyStateComponent],
  template: `
    @if (items().length) {
      <div class="scroll-y" style="flex:1;padding-bottom:8px">
        @if (!scopeAgent()) {
          <div class="up" style="font-size:9px;color:var(--ink-3);padding:10px 14px 2px">All projects · {{ items().length }} pending</div>
        }
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
  private notifications = inject(NotificationService);
  readonly scopeAgent = input<Agent | null>(null);

  readonly items = computed(() => {
    const pending = this.notifications.pending();
    const sa = this.scopeAgent();
    return sa ? pending.filter((n) => n.agentId === sa.id) : pending;
  });
  readonly emptyText = computed(() => {
    const sa = this.scopeAgent();
    return sa ? "No pending actions for " + sa.name : "Inbox zero — no pending actions";
  });
}
