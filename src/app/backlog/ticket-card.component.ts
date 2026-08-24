import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { Ticket } from "../models";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { AgentStripComponent } from "./agent-strip.component";
import { TagChipComponent } from "./tag-chip.component";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { KjButtonComponent } from "@kouji-ui/components";

/** Strip HTML tags and collapse whitespace — for the notes preview. */
function plainText(html: string | null | undefined): string {
  if (!html) return "";
  return html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}

/**
 * Ticket card with three visual variants: todo / inprogress / done.
 * Emits dragstart/dragend for the parent column to wire up HTML5 DnD.
 */
@Component({
  selector: "app-ticket-card",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ToolBadgeComponent, AgentStripComponent, TagChipComponent, KjButtonComponent],
  template: `
    @let tk = ticket();
    @let st = tk.status;
    @let ag = resolvedAgent();

    <div
      draggable="true"
      (dragstart)="onDragStart($event)"
      (dragend)="dragEnd.emit()"
      (click)="ui.openTicket(tk.id)"
      (mouseenter)="hovered.set(true)"
      (mouseleave)="hovered.set(false)"
      [style.background]="st === 'done' ? 'var(--panel-2)' : 'var(--panel)'"
      [style.border]="cardBorder(st)"
      [style.border-left]="cardBorderLeft(st)"
      [style.transform]="hovered() ? 'translateY(-2px)' : 'none'"
      [style.box-shadow]="hovered() ? 'var(--shadow)' : 'none'"
      [style.opacity]="dragging() ? '0.4' : '1'"
      style="border-radius:var(--r-lg);padding:var(--sp-6);display:flex;flex-direction:column;gap:var(--sp-4);position:relative;cursor:pointer;transition:border-color .15s,transform .15s,box-shadow .15s"
    >
      <!-- title row -->
      <div style="display:flex;align-items:flex-start;gap:var(--sp-4)">
        @if (st === 'done') {
          <span style="flex:none;width:var(--sp-7);height:var(--sp-7);margin-top:1px;border-radius:50%;display:grid;place-items:center;background:color-mix(in oklch,var(--st-done),transparent 82%);border:1px solid color-mix(in oklch,var(--st-done),transparent 55%)">
            <app-icon size="md" name="check" color="var(--st-done)" />
          </span>
        }
        <h3
          [style.color]="st === 'done' ? 'var(--ink-2)' : null"
          style="line-height:1.3;flex:1;min-width:0;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden"
        >{{ tk.title }}</h3>
        <span class="tnum" style="flex:none;font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-1)">#{{ shortId(tk.id) }}</span>
      </div>

      <!-- notes preview (todo + inprogress only) -->
      @if (st !== 'done') {
        <p style="color:var(--ink-2);line-height:1.5;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden">
          {{ notes() }}
        </p>
      }

      <!-- tags (click a chip to toggle it into the board filter) -->
      @if (tk.tags && tk.tags.length > 0) {
        <div style="display:flex;flex-wrap:wrap;gap:var(--sp-3)">
          @for (t of tk.tags; track t) {
            <app-tag
              [name]="t"
              [clickable]="true"
              [active]="activeTags().includes(t)"
              (toggle)="toggleTag.emit($event)"
            />
          }
        </div>
      }

      <!-- inprogress: agent strip or "dispatch one" nudge -->
      @if (st === 'inprogress') {
        @if (tk.agentId) {
          <app-agent-strip [agentId]="tk.agentId" />
        } @else {
          <div
            (click)="dispatch($event, tk)"
            style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-4);border-radius:var(--r-md);cursor:pointer;border:1px dashed var(--ui-line);color:var(--ui-ink)"
          >
            <app-icon name="bolt" size="sm" />No agent yet — dispatch one
          </div>
        }
      }

      <!-- bottom row -->
      @if (st === 'todo') {
        <div style="display:flex;align-items:center;gap:var(--sp-4)">
          <span style="display:inline-flex;align-items:center;gap:var(--sp-2);min-width:0;overflow:hidden" [style.color]="chipColor()">
            <app-icon size="md" [name]="chipIcon()" style="flex:none" />
            <span class="trunc">{{ chipName() }}</span>
          </span>
          <!-- card actions ride the compact xs step, like the orchestrator
               cards; the card's hover promotes it from accent-tinted outline to
               the filled primary -->
          <kj-button
            [kjVariant]="hovered() ? 'default' : 'outline'"
            class="kj-push"
            (click)="dispatch($event, tk)"
            [style.--kj-button-fg]="hovered() ? null : 'var(--ui-ink)'"
            [style.--kj-button-border-color]="hovered() ? null : 'var(--ui-line)'"
            style="flex:none"
          >
            <app-icon name="bolt" size="sm" />Dispatch
          </kj-button>
        </div>
      }

      @if (st === 'inprogress') {
        <span style="display:inline-flex;align-items:center;gap:var(--sp-2);min-width:0;overflow:hidden" [style.color]="chipColor()">
          <app-icon size="md" [name]="chipIcon()" style="flex:none" />
          <span class="trunc">{{ chipName() }}</span>
        </span>
      }

      @if (st === 'done') {
        <div class="tnum" style="display:flex;align-items:center;gap:var(--sp-4);font-size:var(--fs-meta);color:var(--ink-3)">
          <span style="display:inline-flex;align-items:center;gap:var(--sp-2);min-width:0;overflow:hidden" [style.color]="'var(--ink-3)'">
            <app-icon size="md" [name]="chipIcon()" style="flex:none" />
            <span class="trunc">{{ chipName() }}</span>
          </span>
          @if (ag) {
            <span style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-3);flex:none">
              <app-tool-badge [tool]="ag.tool" [size]="14" />
              <span class="trunc" style="max-width: round(calc(92px * var(--density)), 1px)">{{ ag.name }}</span>
              <span style="color:var(--ink-4)">·</span>
              <span style="display:flex;align-items:center;gap:var(--sp-1);color:var(--st-done)">
                <app-icon size="md" name="commit" />{{ ag.commits }}
              </span>
            </span>
          }
        </div>
      }
    </div>
  `,
})
export class TicketCardComponent {
  readonly ticket = input.required<Ticket>();
  readonly dragging = input<boolean>(false);
  /** Tags currently active in the board filter — drives the chip's `active` look. */
  readonly activeTags = input<string[]>([]);
  readonly dragEnd = output<void>();
  readonly dragStarted = output<DragEvent>();
  readonly toggleTag = output<string>();

  readonly ui = inject(UiStore);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly projects = inject(ProjectActionsService);

  readonly hovered = signal(false);

  readonly resolvedAgent = computed(() => {
    const tk = this.ticket();
    if (!tk.agentId) return undefined;
    return this.runtime.agents().find((a) => a.id === tk.agentId);
  });

  readonly notes = computed(() => plainText(this.ticket().notes));

  shortId(id: string): string {
    return id.replace(/^t/, "");
  }

  cardBorder(st: string): string {
    if (this.hovered()) return "1px solid var(--hair-2)";
    return "1px solid var(--hair)";
  }

  cardBorderLeft(st: string): string {
    if (st === "inprogress")
      return "2px solid var(--ui-line)";
    return this.cardBorder(st);
  }

  private project() {
    const tk = this.ticket();
    return tk.projectId ? this.projects.projectOf(tk.projectId) : undefined;
  }

  chipColor(): string {
    const st = this.ticket().status;
    const p = this.project();
    if (!p) return "var(--ink-4)";
    return st === "done" ? "var(--ink-3)" : p.color;
  }

  chipIcon(): string {
    return this.project()?.icon ?? "folder";
  }

  chipName(): string {
    return this.project()?.name ?? "no project";
  }

  dispatch(e: MouseEvent, tk: Ticket) {
    e.stopPropagation();
    this.ui.dispatchTicket({ id: tk.id, projectId: tk.projectId });
  }

  onDragStart(e: DragEvent) {
    try {
      e.dataTransfer!.effectAllowed = "move";
      e.dataTransfer!.setData("text/plain", this.ticket().id);
    } catch {
      /* ignore */
    }
    this.dragStarted.emit(e);
  }
}
