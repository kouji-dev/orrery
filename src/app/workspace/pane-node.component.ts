import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  HostListener,
  inject,
  input,
  signal,
  viewChild,
} from "@angular/core";
import { Agent, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { DragService } from "../shared/drag.service";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { STATUS_META } from "../utils";
import { DiffViewComponent } from "./diff-view.component";
import { DropSide, PaneCtx, PaneLeaf, PaneNode, PaneSplit } from "./pane-model";
import { TerminalComponent } from "./terminal.component";

/**
 * One node of a workspace pane tree, rendered recursively. A `leaf` is a single
 * pane (agent picker + terminal/diff view + split/close controls); a `split` lays
 * its two children side-by-side (dir "v") or stacked (dir "h") with a draggable
 * divider. All mutations go through the injected `ctx` (the PaneManager).
 */
@Component({
  selector: "app-pane-node",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ToolBadgeComponent, TerminalComponent, DiffViewComponent],
  template: `
    @if (asLeaf(); as lf) {
      @let ag = agent();
      <div
        (mousedown)="ctx().onFocus(lf.id)"
        (dragover)="onDragOver($event, lf.id)"
        (dragleave)="onDragLeave($event)"
        (drop)="onDrop($event, lf.id)"
        class="pane-leaf"
        [class.focused]="ctx().focusId() === lf.id"
        style="flex:1;min-width:0;min-height:0;display:flex;flex-direction:column;background:var(--panel-2);position:relative;border-radius:var(--r-md);overflow:hidden"
      >
        <!-- pane header -->
        <div style="display:flex;align-items:center;gap:6px;padding:5px 6px 5px 8px;background:var(--panel);border-bottom:1px solid var(--hair);position:relative;flex:none">
          @if (proj(); as p) { <span [style.background]="p.color" [title]="p.name" style="width:6px;height:6px;border-radius:2px;flex:none"></span> }
          <button
            (click)="$event.stopPropagation(); pickOpen.set(!pickOpen())"
            style="display:flex;align-items:center;gap:6px;background:transparent;border:none;cursor:pointer;color:var(--ink);font-family:var(--font-mono);font-size:11.5px;padding:2px 4px;border-radius:5px;min-width:0"
          >
            @if (ag) {
              <span [style.background]="dot(ag.status)" style="width:7px;height:7px;border-radius:50%;flex:none"></span>
            } @else {
              <app-icon name="agent" size="sm" color="var(--ink-4)" />
            }
            <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:130px">{{ ag ? ag.name : 'Assign agent' }}</span>
            <app-icon name="chevronD" size="sm" [px]="11" color="var(--ink-4)" />
          </button>

          @if (pickOpen()) {
            <div #picker class="rise" style="position:absolute;top:calc(100% + 4px);left:6px;z-index:40;width:230px;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:var(--shadow);padding:5px;max-height:320px;overflow-y:auto">
              @for (p of ctx().projects(); track p.id) {
                @let pa = agentsOf(p.id);
                @if (pa.length) {
                  <div style="display:flex;align-items:center;gap:6px;padding:5px 8px 3px">
                    <span [style.background]="p.color" style="width:6px;height:6px;border-radius:2px"></span>
                    <span class="up" style="font-size:8.5px;color:var(--ink-3)">{{ p.name }}</span>
                  </div>
                  @for (a of pa; track a.id) {
                    <button
                      (click)="ctx().onAgent(lf.id, a.id); pickOpen.set(false)"
                      [style.background]="lf.agentId === a.id ? 'var(--panel-3)' : 'transparent'"
                      style="display:flex;align-items:center;gap:8px;width:100%;text-align:left;padding:5px 9px;border-radius:6px;border:none;cursor:pointer;font-family:var(--font-mono);font-size:11.5px;color:var(--ink)"
                    >
                      <span [style.background]="dot(a.status)" style="width:7px;height:7px;border-radius:50%;flex:none"></span>
                      <span style="flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ a.name }}</span>
                      <app-tool-badge [tool]="a.tool" [size]="13" />
                    </button>
                  }
                }
              }
            </div>
          }

          @if (ag) {
            <div style="display:flex;gap:1px;margin-left:4px;padding:2px;background:var(--panel-2);border:1px solid var(--hair);border-radius:5px">
              @for (v of views; track v.k) {
                <button
                  (click)="$event.stopPropagation(); ctx().onView(lf.id, v.k)"
                  [title]="v.k"
                  [style.background]="lf.view === v.k ? 'var(--panel-3)' : 'transparent'"
                  [style.color]="lf.view === v.k ? 'var(--accent)' : 'var(--ink-3)'"
                  style="display:flex;padding:2px 5px;border-radius:3px;border:none;cursor:pointer"
                ><app-icon [name]="v.icon" size="sm" [px]="12" /></button>
              }
            </div>
          }

          <div style="flex:1"></div>
          @if (ag) {
            <button
              class="pane-btn primary"
              (click)="$event.stopPropagation(); toggleRun(ag)"
              [disabled]="ag.status === 'done'"
              [title]="runTitle(ag)"
            ><app-icon [name]="ag.status === 'running' ? 'pause' : 'play'" size="sm" [px]="13" /></button>
          }
          <button class="pane-btn" (click)="$event.stopPropagation(); ctx().onSplit(lf.id, 'v')" title="Split right"><app-icon name="splitCol" size="sm" [px]="13" /></button>
          <button class="pane-btn" (click)="$event.stopPropagation(); ctx().onSplit(lf.id, 'h')" title="Split down"><app-icon name="splitRow" size="sm" [px]="13" /></button>
          @if (ctx().canClose()) {
            <button class="pane-btn" (click)="$event.stopPropagation(); ctx().onClose(lf.id)" title="Close pane"><app-icon name="x" size="sm" [px]="13" /></button>
          }
        </div>

        <!-- pane body -->
        <div style="flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden">
          @if (!ag) {
            <div style="flex:1;display:grid;place-items:center;color:var(--ink-4)">
              <button class="btn ghost-hair" (click)="$event.stopPropagation(); pickOpen.set(true)"><app-icon name="plus" size="sm" />Assign an agent</button>
            </div>
          } @else if (lf.view === 'terminal') {
            <app-terminal [agent]="ag" />
          } @else {
            <app-diff-view [agent]="ag" />
          }
        </div>

        <!-- drag drop preview -->
        @if (dropSide(); as side) {
          <div style="position:absolute;inset:0;z-index:20;pointer-events:none">
            <div
              class="drop-zone"
              [style.left]="zone()[0]"
              [style.right]="zone()[1]"
              [style.top]="zone()[2]"
              [style.bottom]="zone()[3]"
              [style.width]="zone()[4]"
              [style.height]="zone()[5]"
            >
              <div class="drop-label">
                <app-icon [name]="side === 'center' ? 'swap' : (side === 'left' || side === 'right') ? 'splitCol' : 'splitRow'" size="sm" [px]="12" color="#06070b" />
                {{ side === 'center' ? 'Replace' : 'Split ' + side }}
              </div>
            </div>
          </div>
        }
      </div>
    } @else if (asSplit(); as sp) {
      <div #splitEl [style.flex-direction]="sp.dir === 'v' ? 'row' : 'column'" style="flex:1;min-width:0;min-height:0;display:flex">
        <div [style.flex-grow]="sp.ratio" style="flex-shrink:1;flex-basis:0;min-width:0;min-height:0;display:flex">
          <app-pane-node [node]="sp.a" [ctx]="ctx()" />
        </div>
        <div
          class="pane-divider"
          [class.v]="sp.dir === 'v'"
          [class.h]="sp.dir !== 'v'"
          [class.on]="dragging()"
          (pointerdown)="startDrag($event, sp)"
          [style.cursor]="sp.dir === 'v' ? 'col-resize' : 'row-resize'"
          [style.width]="sp.dir === 'v' ? '8px' : '100%'"
          [style.height]="sp.dir === 'v' ? '100%' : '8px'"
          style="flex:none;align-self:stretch;display:flex;align-items:center;justify-content:center;touch-action:none;position:relative;z-index:3"
        >
          <span class="grip-handle" [style.width]="sp.dir === 'v' ? '6px' : '100%'" [style.height]="sp.dir === 'v' ? '100%' : '6px'"></span>
        </div>
        <div [style.flex-grow]="1 - sp.ratio" style="flex-shrink:1;flex-basis:0;min-width:0;min-height:0;display:flex">
          <app-pane-node [node]="sp.b" [ctx]="ctx()" />
        </div>
      </div>
    }
  `,
  styles: [
    `
      .pane-leaf {
        border: 1px solid var(--hair);
      }
      .pane-leaf.focused {
        border-color: color-mix(in oklch, var(--accent), transparent 55%);
        box-shadow: 0 0 0 1px color-mix(in oklch, var(--accent), transparent 70%);
      }
      .pane-btn {
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 3px;
        background: transparent;
        border: none;
        border-radius: 4px;
        cursor: pointer;
        color: var(--ink-3);
        flex: none;
      }
      .pane-btn:hover:not(:disabled) {
        background: var(--panel-3);
        color: var(--ink);
      }
      .pane-btn:disabled {
        opacity: 0.4;
        cursor: default;
      }
      /* play/pause adopts the merge button's primary look so it's more visible */
      .pane-btn.primary {
        background: linear-gradient(180deg, var(--accent), color-mix(in oklch, var(--accent), #000 14%));
        color: #06070b;
        padding: 3px 8px;
        box-shadow: 0 0 16px -5px rgba(var(--accent-rgb), 0.85);
      }
      .pane-btn.primary:hover:not(:disabled) {
        filter: brightness(1.08);
        background: linear-gradient(180deg, var(--accent), color-mix(in oklch, var(--accent), #000 14%));
        color: #06070b;
      }
      [data-theme="light"] .pane-btn.primary {
        color: #fff;
      }
      .pane-divider {
        position: relative;
        background: transparent;
      }
      .pane-divider .grip-handle {
        background: color-mix(in oklch, var(--accent), transparent 45%);
        border-radius: 999px;
        transition: background 0.12s, box-shadow 0.12s;
      }
      .pane-divider:hover .grip-handle,
      .pane-divider.on .grip-handle {
        background: var(--accent);
        box-shadow: 0 0 14px 0 rgba(var(--accent-rgb), 0.9);
      }
      .drop-zone {
        position: absolute;
        background: color-mix(in oklch, var(--accent), transparent 78%);
        border: 2px solid var(--accent);
        border-radius: var(--r-md);
        box-shadow: 0 0 24px -4px rgba(var(--accent-rgb), 0.6) inset;
        transition: all 0.08s ease;
      }
      .drop-label {
        position: absolute;
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 9px;
        border-radius: 999px;
        background: var(--accent);
        color: #06070b;
        font-size: 10.5px;
        font-weight: 600;
        white-space: nowrap;
      }
    `,
  ],
})
export class PaneNodeComponent {
  readonly node = input.required<PaneNode>();
  readonly ctx = input.required<PaneCtx>();

  readonly pickOpen = signal(false);
  readonly dragging = signal(false);
  readonly views: { k: "terminal" | "diff"; icon: string }[] = [
    { k: "terminal", icon: "terminal" },
    { k: "diff", icon: "diff" },
  ];

  private host = inject(ElementRef<HTMLElement>);
  private drag = inject(DragService);
  private agentActions = inject(AgentActionsService);
  private picker = viewChild<ElementRef<HTMLElement>>("picker");
  private splitEl = viewChild<ElementRef<HTMLElement>>("splitEl");

  /** which side this leaf would receive a drop on (null = not the drop target). */
  readonly dropSide = computed<DropSide | null>(() => {
    const lf = this.asLeaf();
    if (!lf) return null;
    const dt = this.ctx().dropTarget();
    return dt && dt.paneId === lf.id ? dt.side : null;
  });
  /** [left, right, top, bottom, width, height] for the highlighted drop region. */
  readonly zone = computed<string[]>(() => {
    switch (this.dropSide()) {
      case "left":
        return ["0", "", "0", "0", "50%", ""];
      case "right":
        return ["", "0", "0", "0", "50%", ""];
      case "top":
        return ["0", "0", "0", "", "", "50%"];
      case "bottom":
        return ["0", "0", "", "0", "", "50%"];
      default:
        return ["0", "0", "0", "0", "", ""]; // center → full pane
    }
  });

  readonly asLeaf = computed<PaneLeaf | null>(() => {
    const n = this.node();
    return n.type === "leaf" ? n : null;
  });
  readonly asSplit = computed<PaneSplit | null>(() => {
    const n = this.node();
    return n.type === "split" ? n : null;
  });
  readonly agent = computed<Agent | undefined>(() => {
    const lf = this.asLeaf();
    return lf?.agentId ? this.ctx().agents().find((a) => a.id === lf.agentId) : undefined;
  });
  readonly proj = computed<Project | undefined>(() => {
    const a = this.agent();
    return a ? this.ctx().projects().find((p) => p.id === a.projectId) : undefined;
  });

  dot(status: string): string {
    return STATUS_META[status as keyof typeof STATUS_META]?.color ?? "var(--ink-3)";
  }
  toggleRun(ag: Agent) {
    this.agentActions.toggleRun(ag);
  }
  runTitle(ag: Agent): string {
    return ag.status === "running" ? "Pause agent" : ag.started ? "Resume agent" : "Start agent";
  }
  agentsOf(projectId: string): Agent[] {
    return this.ctx().agents().filter((a) => a.projectId === projectId);
  }

  // ----- drag-and-drop: a sidebar agent / tab dropped onto this pane -----
  onDragOver(e: DragEvent, paneId: string) {
    if (!this.drag.payload()?.agentId) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width;
    const py = (e.clientY - r.top) / r.height;
    const dl = px,
      dr = 1 - px,
      dt = py,
      db = 1 - py;
    const m = Math.min(dl, dr, dt, db);
    let side: DropSide = "center";
    if (m < 0.26) side = m === dl ? "left" : m === dr ? "right" : m === dt ? "top" : "bottom";
    this.ctx().onDropOver(paneId, side);
  }
  onDragLeave(e: DragEvent) {
    const ct = e.currentTarget as HTMLElement;
    if (!ct.contains(e.relatedTarget as Node)) this.ctx().onDropOver(null, "center");
  }
  onDrop(e: DragEvent, paneId: string) {
    e.preventDefault();
    this.ctx().onPaneDrop(paneId);
  }

  @HostListener("document:mousedown", ["$event"])
  onDocDown(e: MouseEvent) {
    if (!this.pickOpen()) return;
    const pk = this.picker()?.nativeElement;
    if (pk && !pk.contains(e.target as Node)) this.pickOpen.set(false);
  }

  startDrag(ev: PointerEvent, sp: PaneSplit) {
    ev.preventDefault();
    ev.stopPropagation();
    const container = this.splitEl()?.nativeElement;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const horiz = sp.dir === "v";
    this.dragging.set(true);
    const el = ev.target as HTMLElement;
    el.setPointerCapture?.(ev.pointerId);
    const move = (e: PointerEvent) => {
      const r = horiz ? (e.clientX - rect.left) / rect.width : (e.clientY - rect.top) / rect.height;
      this.ctx().onRatio(sp.id, Math.max(0.1, Math.min(0.9, r)));
    };
    const up = (e: PointerEvent) => {
      this.dragging.set(false);
      el.releasePointerCapture?.(e.pointerId);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
}
