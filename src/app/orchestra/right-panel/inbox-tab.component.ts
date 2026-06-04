import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent, PendingItem } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { EmptyStateComponent } from "./empty-state.component";
import { PendingCardComponent } from "./pending-card.component";

interface InboxEntry {
  agent: Agent;
  item: PendingItem;
}

@Component({
  selector: "app-inbox-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PendingCardComponent, EmptyStateComponent],
  template: `
    @if (items().length) {
      <div class="scroll-y" style="flex:1;padding-bottom:8px">
        @if (!scopeAgent()) {
          <div class="up" style="font-size:9px;color:var(--ink-3);padding:10px 14px 2px">All projects · {{ items().length }} pending</div>
        }
        @for (e of items(); track $index) {
          <app-pending-card [agent]="e.agent" [item]="e.item" (resolve)="store.handleInbox(e.agent.id, e.item, $event)" />
        }
      </div>
    } @else {
      <app-empty-state icon="bell" [text]="emptyText()" />
    }
  `,
})
export class InboxTabComponent {
  readonly store = inject(OrchestraStore);
  readonly scopeAgent = input<Agent | null>(null);

  readonly items = computed<InboxEntry[]>(() => {
    const list = this.scopeAgent() ? [this.scopeAgent()!] : this.store.agents();
    const out: InboxEntry[] = [];
    list.forEach((a) => (a.pending || []).forEach((p) => out.push({ agent: a, item: p })));
    return out;
  });
  readonly emptyText = computed(() => {
    const sa = this.scopeAgent();
    return sa ? "No pending actions for " + sa.name : "Inbox zero — no pending actions";
  });
}
