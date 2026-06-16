import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  signal,
} from "@angular/core";
import { Ticket, TicketStatus } from "../models";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { TicketCardComponent } from "./ticket-card.component";

type ColDef = { key: TicketStatus; label: string; color: string };

const COLS: ColDef[] = [
  { key: "todo", label: "To do", color: "var(--ink-3)" },
  { key: "inprogress", label: "In progress", color: "var(--accent)" },
  { key: "done", label: "Done", color: "var(--st-done)" },
];

/**
 * Center-pane backlog board: 3-column kanban for tickets with HTML5 DnD.
 */
@Component({
  selector: "app-backlog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, TicketCardComponent],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">
      <!-- header -->
      <div style="display:flex;align-items:center;gap:var(--sp-6);padding:var(--sp-6) var(--sp-7);flex:none;border-bottom:1px solid var(--hair);background:var(--panel)">
        <div>
          <h1 class="disp" style="font-size:var(--fs-lg);font-weight:600;letter-spacing:-0.02em">Backlog</h1>
          <span style="font-size:var(--fs-xs);color:var(--ink-3)">
            {{ tickets.all().length }} tickets · {{ openCount() }} open · {{ ui.org }}
          </span>
        </div>
        <div style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-4)">
          <!-- project filter dropdown -->
          <div style="position:relative">
            <button class="btn ghost-hair" style="font-size:var(--fs-sm);color:var(--ink-2)" (click)="filterOpen.set(!filterOpen())">
              <app-icon name="folder" size="sm" color="var(--ink-3)" />
              {{ filterLabel() }}
              <app-icon name="chevronD" size="sm" color="var(--ink-4)" />
            </button>
            @if (filterOpen()) {
              <div
                class="surface"
                style="position:absolute;top:100%;right:0;margin-top:var(--sp-3);z-index:20;padding:var(--sp-2);min-width:190px;box-shadow:var(--shadow)"
                (click)="$event.stopPropagation()"
              >
                @for (p of filterOptions(); track p.id) {
                  <div
                    (click)="setFilter(p.id)"
                    [style.background]="activeFilter() === p.id ? 'var(--panel-3)' : 'transparent'"
                    [style.color]="activeFilter() === p.id ? 'var(--ink)' : 'var(--ink-2)'"
                    style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-4);border-radius:var(--r-sm);cursor:pointer;font-size:var(--fs-ui)"
                  >
                    <app-icon [name]="p.icon" size="sm" [color]="p.color" />
                    {{ p.name }}
                  </div>
                }
              </div>
            }
          </div>
          <button class="btn primary" (click)="ui.openTicketDraft()">
            <app-icon name="plus" size="sm" />New ticket
          </button>
        </div>
      </div>

      <!-- board body -->
      <div class="scroll-y" style="flex:1" (click)="filterOpen.set(false)">
        @if (isEmpty()) {
          <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:var(--sp-6);min-height:100%;padding:var(--sp-11) var(--sp-8);text-align:center">
            <div style="width:52px;height:52px;border-radius:14px;display:grid;place-items:center;background:color-mix(in oklch,var(--accent),transparent 88%);border:1px solid color-mix(in oklch,var(--accent),transparent 60%)">
              <app-icon name="layers" size="lg" color="var(--accent)" />
            </div>
            <div>
              <div class="disp" style="font-size:var(--fs-lg);font-weight:600">No tickets yet</div>
              <div style="font-size:var(--fs-ui);color:var(--ink-3);margin-top:var(--sp-2);max-width:340px;line-height:1.5">
                Capture work to be done, then dispatch an agent straight from a ticket — the board tracks it to done.
              </div>
            </div>
            <button class="btn primary" (click)="ui.openTicketDraft()">
              <app-icon name="plus" size="sm" />New ticket
            </button>
          </div>
        } @else {
          <div style="display:grid;grid-template-columns:repeat(3,1fr);gap:var(--sp-6);padding:var(--sp-7);align-content:start">
            @for (col of cols; track col.key) {
              @let colTickets = filtered(col.key);
              @let isTarget = drag() !== null && drag()?.from !== col.key;
              <div
                (dragover)="onDragOver($event, col.key)"
                (dragleave)="onDragLeave()"
                (drop)="onDrop($event, col.key)"
                [style.outline]="over() === col.key && isTarget ? '1.5px dashed color-mix(in oklch,' + col.color + ',transparent 35%)' : '1.5px dashed transparent'"
                [style.outline-offset]="'4px'"
                [style.background]="over() === col.key && isTarget ? 'color-mix(in oklch,' + col.color + ',transparent 94%)' : 'transparent'"
                style="display:flex;flex-direction:column;gap:var(--sp-4);min-width:0;border-radius:var(--r-lg);transition:background .15s"
              >
                <!-- column header -->
                <div
                  [style.border-bottom]="'2px solid ' + col.color"
                  style="display:flex;align-items:center;gap:var(--sp-4);padding:0 var(--sp-1) var(--sp-3);flex:none"
                >
                  <span class="up" style="font-size:var(--fs-xs);color:var(--ink-2)">{{ col.label }}</span>
                  <span class="chip tnum" style="margin-left:auto;font-size:var(--fs-2xs);padding:0 var(--sp-3)">{{ colTickets.length }}</span>
                </div>

                <!-- cards -->
                @for (tk of colTickets; track tk.id) {
                  <app-ticket-card
                    [ticket]="tk"
                    [dragging]="drag()?.id === tk.id"
                    (dragStarted)="onCardDragStart($event, tk)"
                    (dragEnd)="onDragEnd()"
                  />
                }

                <!-- todo empty: show "New ticket" button -->
                @if (col.key === 'todo') {
                  <button
                    (click)="ui.openTicketDraft()"
                    style="display:flex;justify-content:center;align-items:center;gap:var(--sp-3);padding:var(--sp-4);border-radius:var(--r-md);border:1px dashed var(--hair-2);color:var(--ink-3);font-size:var(--fs-sm);background:transparent;cursor:pointer"
                  >
                    <app-icon name="plus" size="sm" />New ticket
                  </button>
                } @else if (!colTickets.length) {
                  <div style="padding:var(--sp-6);text-align:center;color:var(--ink-4);font-size:var(--fs-xs);border:1px dashed var(--hair);border-radius:var(--r-md)">empty</div>
                }
              </div>
            }
          </div>
        }
      </div>
    </div>
  `,
})
export class BacklogComponent {
  readonly tickets = inject(TicketsStore);
  readonly ui = inject(UiStore);
  private readonly projects = inject(ProjectActionsService);

  readonly cols = COLS;

  readonly activeFilter = signal<string>("all");
  readonly filterOpen = signal(false);
  readonly drag = signal<{ id: string; from: TicketStatus } | null>(null);
  readonly over = signal<TicketStatus | null>(null);

  readonly isEmpty = computed(() => this.tickets.all().length === 0);
  readonly openCount = computed(
    () => this.tickets.all().filter((t) => t.status !== "done").length,
  );

  readonly filterOptions = computed(() => [
    { id: "all", name: "All projects", icon: "layers", color: "var(--ink-3)" },
    ...this.projects.all().map((p) => ({
      id: p.id,
      name: p.name,
      icon: p.icon,
      color: p.color,
    })),
  ]);

  readonly filterLabel = computed(() => {
    const f = this.activeFilter();
    if (f === "all") return "All projects";
    return this.projects.projectOf(f)?.name ?? "All projects";
  });

  filtered(status: TicketStatus): Ticket[] {
    const f = this.activeFilter();
    return this.tickets
      .all()
      .filter(
        (t) =>
          t.status === status && (f === "all" || t.projectId === f),
      );
  }

  setFilter(id: string) {
    this.activeFilter.set(id);
    this.filterOpen.set(false);
  }

  // ---- drag-and-drop ----
  onCardDragStart(_e: DragEvent, tk: Ticket) {
    this.drag.set({ id: tk.id, from: tk.status });
  }

  onDragOver(e: DragEvent, status: TicketStatus) {
    if (!this.drag()) return;
    e.preventDefault();
    this.over.set(status);
  }

  onDragLeave() {
    this.over.set(null);
  }

  onDrop(e: DragEvent, status: TicketStatus) {
    e.preventDefault();
    const d = this.drag();
    if (d && d.from !== status) {
      void this.tickets.setStatus(d.id, status);
    }
    this.drag.set(null);
    this.over.set(null);
  }

  onDragEnd() {
    this.drag.set(null);
    this.over.set(null);
  }
}
