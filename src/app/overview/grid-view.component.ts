import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent, GridRange } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { AgentCardComponent } from "./agent-card.component";
import { KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";

const DAY = 86_400_000;
const RANGES: { key: GridRange; label: string }[] = [
  { key: "week", label: "This week" },
  { key: "month", label: "This month" },
  { key: "older", label: "Older" },
];

@Component({
  selector: "app-grid-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AgentCardComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
  template: `
    <div style="display:flex;flex-direction:column;min-width:0">
      <!-- a bucket picker IS a tab strip: the grid below is its panel. Same
           pills/tabs-xs shape the overview header and the pane chrome use. -->
      <div style="display:flex;padding:var(--sp-6) var(--sp-7) 0">
        <kj-tabs
          variant="pills"
          class="tabs-xs"
          style="margin-left:auto"
          [value]="range()"
          (valueChange)="ui.gridRange.set($any($event))"
        >
          <kj-tab-list aria-label="Recency">
            @for (r of ranges; track r.key) {
              @let n = buckets()[r.key].length;
              <!-- an empty bucket is unpickable rather than pickable-then-
                   corrected: the fallback below would bounce the selection
                   straight back and read as a broken tab -->
              <kj-tab [value]="r.key" [disabled]="n === 0">{{ r.label }} · {{ n }}</kj-tab>
            }
          </kj-tab-list>
        </kj-tabs>
      </div>

      <!-- top padding is the strip's gap, not the pane's — the strip already
           carries the pane inset above it -->
      <div style="display:grid;gap:var(--sp-6);padding:var(--sp-5) var(--sp-7) var(--sp-7);grid-template-columns:repeat(auto-fill,minmax(320px,1fr));align-content:start">
        @for (ag of shown(); track ag.id) {
          <app-agent-card [agent]="ag" [proj]="projects.projectOf(ag.projectId)" />
        }
      </div>
    </div>
  `,
})
export class GridViewComponent {
  readonly projects = inject(ProjectActionsService);
  readonly ui = inject(UiStore);
  readonly agents = input.required<Agent[]>();

  readonly ranges = RANGES;

  /** Every agent split by how recently it ran, each bucket newest-first.
   *  Agents with no `lastRunAt` (rows written before the column existed) fall
   *  into "older" — they are by definition not recent activity. */
  readonly buckets = computed<Record<GridRange, Agent[]>>(() => {
    const now = Date.now();
    const out: Record<GridRange, Agent[]> = { week: [], month: [], older: [] };
    for (const a of this.agents()) {
      const age = a.lastRunAt ? now - a.lastRunAt : Infinity;
      out[age < 7 * DAY ? "week" : age < 30 * DAY ? "month" : "older"].push(a);
    }
    for (const k of Object.keys(out) as GridRange[]) {
      out[k].sort((x, y) => (y.lastRunAt ?? 0) - (x.lastRunAt ?? 0));
    }
    return out;
  });

  /** The bucket actually rendered. The user's pick wins, but an empty pick
   *  falls through to the first non-empty bucket: someone who has not spawned
   *  anything in a month would otherwise open the orchestrator onto a blank
   *  grid and conclude their agents were gone. */
  readonly range = computed<GridRange>(() => {
    const b = this.buckets();
    const picked = this.ui.gridRange();
    if (b[picked].length) return picked;
    return RANGES.find((r) => b[r.key].length)?.key ?? picked;
  });

  readonly shown = computed(() => this.buckets()[this.range()]);
}
