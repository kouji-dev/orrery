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
import { TagChipComponent } from "./tag-chip.component";
import { TagFilterComponent } from "./tag-filter.component";
import { allTagsOf } from "./tags.util";

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
  imports: [IconComponent, TicketCardComponent, TagChipComponent, TagFilterComponent],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">
      <!-- header -->
      <div style="display:flex;align-items:center;gap:14px;padding:14px 18px;flex:none;border-bottom:1px solid var(--hair);background:var(--panel)">
        <div>
          <h1 class="disp" style="font-size:16px;font-weight:600;letter-spacing:-0.02em">Backlog</h1>
          <span style="font-size:10.5px;color:var(--ink-3)">
            {{ tickets.all().length }} tickets · {{ openCount() }} open · {{ ui.org }}
          </span>
        </div>
        <div style="margin-left:auto;display:flex;align-items:center;gap:8px">
          <!-- project filter dropdown -->
          <div style="position:relative">
            <button class="btn ghost-hair" style="font-size:11.5px;color:var(--ink-2)" (click)="filterOpen.set(!filterOpen())">
              <app-icon name="folder" size="sm" color="var(--ink-3)" />
              {{ filterLabel() }}
              <app-icon name="chevronD" size="sm" color="var(--ink-4)" />
            </button>
            @if (filterOpen()) {
              <div
                class="surface"
                style="position:absolute;top:100%;right:0;margin-top:6px;z-index:20;padding:5px;min-width:190px;box-shadow:var(--shadow)"
                (click)="$event.stopPropagation()"
              >
                @for (p of filterOptions(); track p.id) {
                  <div
                    (click)="setFilter(p.id)"
                    [style.background]="activeFilter() === p.id ? 'var(--panel-3)' : 'transparent'"
                    [style.color]="activeFilter() === p.id ? 'var(--ink)' : 'var(--ink-2)'"
                    style="display:flex;align-items:center;gap:8px;padding:7px 9px;border-radius:var(--r-sm);cursor:pointer;font-size:12px"
                  >
                    <app-icon [name]="p.icon" size="sm" [color]="p.color" />
                    {{ p.name }}
                  </div>
                }
              </div>
            }
          </div>
          <app-tag-filter
            [allTags]="allTags()"
            [counts]="tagCounts()"
            [selected]="activeTagSel()"
            (selectedChange)="tagSel.set($event)"
          />
          <button class="btn primary" (click)="ui.openTicketDraft()">
            <app-icon name="plus" size="sm" />New ticket
          </button>
        </div>
      </div>

      <!-- active tag-filter summary -->
      @if (activeTagSel().length > 0) {
        <div style="display:flex;align-items:center;gap:8px;padding:9px 18px;flex:none;border-bottom:1px solid var(--hair);background:var(--panel)">
          <span class="up" style="font-size:9.5px;color:var(--ink-4);letter-spacing:.1em;flex:none">Filtered by</span>
          <div style="display:flex;flex-wrap:wrap;gap:6px">
            @for (t of activeTagSel(); track t) {
              <app-tag [name]="t" [active]="true" [removable]="true" (remove)="toggleTag($event)" />
            }
          </div>
          <span class="tnum" style="margin-left:auto;flex:none;font-size:10.5px;color:var(--ink-4)">
            {{ shownTotal() }} of {{ tickets.all().length }}
          </span>
        </div>
      }

      <!-- board body -->
      <div class="scroll-y" style="flex:1" (click)="filterOpen.set(false)">
        @if (isEmpty()) {
          <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:14px;min-height:100%;padding:40px 20px;text-align:center">
            <div style="width:52px;height:52px;border-radius:14px;display:grid;place-items:center;background:color-mix(in oklch,var(--accent),transparent 88%);border:1px solid color-mix(in oklch,var(--accent),transparent 60%)">
              <app-icon name="layers" size="lg" color="var(--accent)" />
            </div>
            <div>
              <div class="disp" style="font-size:16px;font-weight:600">No tickets yet</div>
              <div style="font-size:12px;color:var(--ink-3);margin-top:5px;max-width:340px;line-height:1.5">
                Capture work to be done, then dispatch an agent straight from a ticket — the board tracks it to done.
              </div>
            </div>
            <button class="btn primary" (click)="ui.openTicketDraft()">
              <app-icon name="plus" size="sm" />New ticket
            </button>
          </div>
        } @else {
          <div style="display:grid;grid-template-columns:repeat(3,1fr);gap:14px;padding:18px;align-content:start">
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
                style="display:flex;flex-direction:column;gap:9px;min-width:0;border-radius:var(--r-lg);transition:background .15s"
              >
                <!-- column header -->
                <div
                  [style.border-bottom]="'2px solid ' + col.color"
                  style="display:flex;align-items:center;gap:8px;padding:0 2px 6px;flex:none"
                >
                  <span class="up" style="font-size:10px;color:var(--ink-2)">{{ col.label }}</span>
                  <span class="chip tnum" style="margin-left:auto;font-size:9.5px;padding:0 6px">{{ colTickets.length }}</span>
                </div>

                <!-- cards -->
                @for (tk of colTickets; track tk.id) {
                  <app-ticket-card
                    [ticket]="tk"
                    [dragging]="drag()?.id === tk.id"
                    [activeTags]="activeTagSel()"
                    (toggleTag)="toggleTag($event)"
                    (dragStarted)="onCardDragStart($event, tk)"
                    (dragEnd)="onDragEnd()"
                  />
                }

                <!-- todo empty: show "New ticket" button -->
                @if (col.key === 'todo') {
                  <button
                    (click)="ui.openTicketDraft()"
                    style="display:flex;justify-content:center;align-items:center;gap:6px;padding:9px;border-radius:var(--r-md);border:1px dashed var(--hair-2);color:var(--ink-3);font-size:11.5px;background:transparent;cursor:pointer"
                  >
                    <app-icon name="plus" size="sm" />New ticket
                  </button>
                } @else if (!colTickets.length) {
                  <div style="padding:14px;text-align:center;color:var(--ink-4);font-size:10.5px;border:1px dashed var(--hair);border-radius:var(--r-md)">empty</div>
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
  readonly tagSel = signal<string[]>([]);
  readonly drag = signal<{ id: string; from: TicketStatus } | null>(null);
  readonly over = signal<TicketStatus | null>(null);

  readonly isEmpty = computed(() => this.tickets.all().length === 0);
  readonly openCount = computed(
    () => this.tickets.all().filter((t) => t.status !== "done").length,
  );

  // ---- tags ----
  readonly allTags = computed(() => allTagsOf(this.tickets.all()));
  readonly tagCounts = computed(() => {
    const counts: Record<string, number> = {};
    for (const t of this.tickets.all()) {
      for (const tg of t.tags ?? []) counts[tg] = (counts[tg] ?? 0) + 1;
    }
    return counts;
  });
  // drop selected tags that no longer exist on any ticket
  readonly activeTagSel = computed(() => {
    const tags = this.allTags();
    return this.tagSel().filter((t) => tags.includes(t));
  });
  // total tickets passing both filters (all statuses) — for the "N of M" summary
  readonly shownTotal = computed(
    () => this.tickets.all().filter((t) => this.passes(t)).length,
  );

  toggleTag(t: string) {
    this.tagSel.update((s) => (s.includes(t) ? s.filter((x) => x !== t) : [...s, t]));
  }

  /** Does a ticket pass the active project + tag filters? Tags use OR semantics. */
  private passes(t: Ticket): boolean {
    const f = this.activeFilter();
    const sel = this.activeTagSel();
    return (
      (f === "all" || t.projectId === f) &&
      (sel.length === 0 || (t.tags ?? []).some((tg) => sel.includes(tg)))
    );
  }

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
    return this.tickets.all().filter((t) => t.status === status && this.passes(t));
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
