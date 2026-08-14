import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  HostListener,
  inject,
  Input,
  signal,
  viewChild,
} from "@angular/core";
import { Agent, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { EditsStore } from "../stores/edits.store";
import { ScrollStateService } from "./scroll-state.service";
import { DiagnosticsService } from "../shared/diagnostics.service";
import { FileSaveService } from "./file-save.service";
import { DragService } from "../shared/drag.service";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { fileName, STATUS_META } from "../utils";
import { DiffViewComponent } from "./diff-view.component";
import { FileViewComponent } from "./file-view.component";
import { DropSide, PaneCtx, PaneLeaf, PaneNode, PaneSplit } from "./pane-model";
import { TerminalComponent } from "./terminal.component";
import { AgentGitViewComponent } from "./git/agent-git-view.component";
import { UiStore } from "../ui/ui.store";

/**
 * One node of a workspace pane tree, rendered recursively. A `leaf` is a single
 * pane (agent picker + terminal/diff view + split/close controls); a `split` lays
 * its two children side-by-side (dir "v") or stacked (dir "h") with a draggable
 * divider. All mutations go through the injected `ctx` (the PaneManager).
 */
@Component({
  selector: "app-pane-node",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ToolBadgeComponent, TerminalComponent, DiffViewComponent, FileViewComponent, AgentGitViewComponent],
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
        <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-3) var(--sp-2) var(--sp-4);background:var(--panel);border-bottom:1px solid var(--hair);position:relative;flex:none">
          @if (proj(); as p) { <span [style.background]="p.color" [title]="p.name" style="width:var(--sp-3);height:var(--sp-3);border-radius:2px;flex:none"></span> }
          <button
            (click)="$event.stopPropagation(); pickOpen.set(!pickOpen())"
            style="display:flex;align-items:center;gap:var(--sp-3);background:transparent;border:none;cursor:pointer;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm);padding:var(--sp-1) var(--sp-2);border-radius:5px;min-width:0"
          >
            @if (ag) {
              <span [style.background]="dot(ag.status)" style="width:var(--sp-3);height:var(--sp-3);border-radius:50%;flex:none"></span>
            } @else {
              <app-icon name="agent" size="sm" color="var(--ink-4)" />
            }
            <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:130px">{{ ag ? ag.name : 'Assign agent' }}</span>
            <app-icon name="chevronD" size="sm" [px]="11" color="var(--ink-4)" />
          </button>

          @if (pickOpen()) {
            <div #picker class="rise" style="position:absolute;top:calc(100% + 4px);left:6px;z-index:40;width:230px;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:var(--shadow);padding:var(--sp-2);max-height:320px;overflow-y:auto">
              @for (p of ctx().projects(); track p.id) {
                @let pa = agentsOf(p.id);
                @if (pa.length) {
                  <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-4) var(--sp-1)">
                    <span [style.background]="p.color" style="width:var(--sp-3);height:var(--sp-3);border-radius:2px"></span>
                    <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">{{ p.name }}</span>
                  </div>
                  @for (a of pa; track a.id) {
                    <button
                      (click)="ctx().onAgent(lf.id, a.id); pickOpen.set(false)"
                      [style.background]="lf.agentId === a.id ? 'var(--panel-3)' : 'transparent'"
                      style="display:flex;align-items:center;gap:var(--sp-4);width:100%;text-align:left;padding:var(--sp-2) var(--sp-4);border-radius:6px;border:none;cursor:pointer;font-family:var(--font-mono);font-size:var(--fs-sm);color:var(--ink)"
                    >
                      <span [style.background]="dot(a.status)" style="width:var(--sp-3);height:var(--sp-3);border-radius:50%;flex:none"></span>
                      <span style="flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ a.name }}</span>
                      <app-tool-badge [tool]="a.tool" [size]="13" />
                    </button>
                  }
                }
              }
            </div>
          }

          @if (ag) {
            <div style="display:flex;gap:1px;margin-left:var(--sp-2);padding:var(--sp-1);background:var(--panel-2);border:1px solid var(--hair);border-radius:5px">
              @for (v of views; track v.k) {
                <button
                  (click)="$event.stopPropagation(); ctx().onView(lf.id, v.k)"
                  [title]="v.k"
                  [style.background]="lf.view === v.k ? 'var(--panel-3)' : 'transparent'"
                  [style.color]="lf.view === v.k ? 'var(--accent)' : 'var(--ink-3)'"
                  style="display:flex;padding:var(--sp-1) var(--sp-2);border-radius:3px;border:none;cursor:pointer"
                ><app-icon [name]="v.icon" size="sm" [px]="12" /></button>
              }
            </div>
          }

          <!-- open-file tabs, inline beside the view toggle; scrolls with an
               edge-fade when there are more than fit. Doubles as the flex spacer. -->
          @if (ag) {
            <div #fileStrip class="file-strip" (wheel)="onStripWheel($event)">
              @for (f of lf.files ?? []; track f) {
                @let onf = lf.view === 'file' && lf.activeFile === f;
                @let dirty = isDirty(lf, f);
                <div
                  class="file-tab"
                  [class.on]="onf"
                  [class.dirty]="dirty"
                  [title]="f + (dirty ? ' — unsaved changes' : '')"
                  [attr.data-path]="f"
                  (click)="$event.stopPropagation(); ctx().onFileSelect(lf.id, f)"
                  (contextmenu)="onFileTabContext($event, lf, f)"
                >
                  <app-icon name="file" size="sm" [px]="11" [color]="onf ? 'var(--accent)' : 'var(--ink-4)'" />
                  <span class="fn">{{ fname(f) }}</span>
                  <span class="fdot" aria-label="unsaved changes"></span>
                  <button class="fx" title="Close file" (click)="$event.stopPropagation(); requestFileClose(lf, f)">
                    <app-icon name="x" size="sm" [px]="10" />
                  </button>
                </div>
              }
            </div>
          } @else {
            <div style="flex:1"></div>
          }
          @if (ag) {
            @if (ag.worktree) {
              <button
                class="pane-btn"
                (click)="$event.stopPropagation(); diagnostics.openWorktree(ag.worktree)"
                title="Open worktree folder"
              ><app-icon name="folderOpen" size="sm" [px]="13" /></button>
            }
            @if (ag.sessionId && ag.status !== 'running') {
              <button
                class="pane-btn"
                (click)="$event.stopPropagation(); continueSession(ag.id)"
                [title]="'Continue last session · ' + ag.tool + ' (' + ag.sessionId + ')'"
              ><app-icon name="refresh" size="sm" [px]="13" /></button>
            }
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
          } @else if (lf.view === 'file' && lf.activeFile) {
            <app-file-view [agent]="ag" [path]="lf.activeFile" />
          } @else if (lf.view === 'terminal') {
            <app-terminal [agent]="ag" />
          } @else if (ui.gitViewFor(ag!.id); as gv) {
            <app-agent-git-view [agent]="ag!" [gitView]="gv" (close)="ui.setGitView(ag!.id, null)" />
          } @else {
            <app-diff-view [agent]="ag" />
          }
        </div>

        <!-- close-confirm for a dirty file tab (B1.1) -->
        @if (confirmClose(); as cc) {
          <div class="cc-scrim" (mousedown)="$event.stopPropagation()">
            <div class="cc-card rise">
              <div class="cc-title">Unsaved changes</div>
              @for (p of cc.dirty; track p) {
                <div class="cc-path">{{ p }}</div>
              }
              <div class="cc-note">Save before closing {{ cc.dirty.length === 1 ? 'this tab' : 'these tabs' }}?</div>
              <div class="cc-actions">
                <button class="btn ghost-hair" (click)="confirmClose.set(null)">Cancel</button>
                <span style="margin-left:auto"></span>
                <button class="btn ghost-hair cc-danger" (click)="discardAndClose(cc)">Discard</button>
                <button class="btn primary" (click)="saveAndClose(cc)">{{ cc.dirty.length === 1 ? 'Save' : 'Save all' }}</button>
              </div>
            </div>
          </div>
        }

        <!-- file-tab context menu: single + bulk close -->
        @if (tabMenu(); as tm) {
          <div class="ftab-menu rise" [style.left.px]="tm.x" [style.top.px]="tm.y" (mousedown)="$event.stopPropagation()" (contextmenu)="$event.preventDefault()">
            <button class="ftab-mi" (click)="tabMenuClose(lf, 'one')"><app-icon name="x" size="sm" [px]="12" />Close</button>
            <button class="ftab-mi" [disabled]="!tabMenuTargets(lf, 'left').length" (click)="tabMenuClose(lf, 'left')"><app-icon name="chevron" size="sm" [px]="12" style="transform:rotate(180deg)" />Close All to the Left</button>
            <button class="ftab-mi" [disabled]="!tabMenuTargets(lf, 'right').length" (click)="tabMenuClose(lf, 'right')"><app-icon name="chevron" size="sm" [px]="12" />Close All to the Right</button>
            <button class="ftab-mi" (click)="tabMenuClose(lf, 'all')"><app-icon name="layers" size="sm" [px]="12" />Close All</button>
          </div>
        }

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
        padding: var(--sp-1);
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
        padding: var(--sp-1) var(--sp-4);
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
      /* open-file tabs live inline in the pane header, right after the view
         toggle. The strip is also the flex spacer (flex:1) and scrolls
         sideways when the tabs outgrow the space; a right edge-fade hints at
         the overflow and a vertical wheel pans it (see onStripWheel). */
      .file-strip {
        display: flex;
        align-items: center;
        gap: var(--sp-1);
        flex: 1;
        min-width: 0;
        overflow-x: auto;
        scrollbar-width: none;
        -webkit-mask-image: linear-gradient(90deg, #000 calc(100% - 18px), transparent);
        mask-image: linear-gradient(90deg, #000 calc(100% - 18px), transparent);
      }
      .file-strip::-webkit-scrollbar {
        display: none;
      }
      .file-tab {
        flex: none;
        display: flex;
        align-items: center;
        gap: var(--sp-2);
        max-width: 150px;
        padding: var(--sp-1) var(--sp-2) var(--sp-1) var(--sp-4);
        border: 1px solid transparent;
        border-radius: 5px;
        font-family: var(--font-mono);
        font-size: var(--fs-xs);
        color: var(--ink-3);
        cursor: pointer;
        position: relative;
      }
      .file-tab:hover {
        color: var(--ink-2);
        background: var(--panel-2);
      }
      .file-tab.on {
        color: var(--ink);
        background: var(--panel-3);
        border-color: var(--hair);
      }
      .file-tab .fn {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .file-tab .fx {
        display: flex;
        flex: none;
        padding: 1px;
        border: none;
        border-radius: 3px;
        background: transparent;
        color: var(--ink-4);
        cursor: pointer;
        opacity: 0;
      }
      .file-tab:hover .fx,
      .file-tab.on .fx {
        opacity: 1;
      }
      /* unsaved-changes dot — swaps places with the close × on hover */
      .file-tab .fdot {
        display: none;
        flex: none;
        width: var(--sp-2);
        height: var(--sp-2);
        border-radius: 50%;
        background: var(--accent);
      }
      .file-tab.dirty:not(:hover) .fdot {
        display: block;
      }
      .file-tab.dirty:not(:hover) .fx {
        display: none;
      }
      .file-tab .fx:hover {
        color: var(--ink);
        background: var(--panel-2);
      }
      /* close-confirm dialog for a dirty file tab */
      .cc-scrim {
        position: absolute;
        inset: 0;
        z-index: 30;
        display: grid;
        place-items: center;
        background: color-mix(in srgb, var(--bg), transparent 35%);
      }
      .cc-card {
        width: min(340px, 86%);
        background: var(--elev);
        border: 1px solid var(--hair-2);
        border-radius: var(--r-md);
        box-shadow: var(--shadow);
        padding: var(--sp-6);
      }
      .cc-title {
        color: var(--ink);
        font-size: var(--fs-ui);
        font-weight: 600;
      }
      .cc-path {
        margin-top: var(--sp-2);
        color: var(--ink-3);
        font-family: var(--font-mono);
        font-size: var(--fs-xs);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .cc-note {
        margin-top: var(--sp-3);
        color: var(--ink-2);
        font-size: var(--fs-sm);
      }
      .cc-actions {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        margin-top: var(--sp-5);
      }
      .cc-danger {
        color: var(--code-del-ink);
      }
      /* file-tab context menu */
      .ftab-menu {
        position: fixed;
        z-index: 60;
        min-width: 190px;
        background: var(--elev);
        border: 1px solid var(--hair-2);
        border-radius: var(--r-md);
        box-shadow: var(--shadow);
        padding: var(--sp-2);
      }
      .ftab-mi {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        width: 100%;
        text-align: left;
        padding: var(--sp-2) var(--sp-4);
        border: none;
        border-radius: 5px;
        background: transparent;
        color: var(--ink-2);
        font-size: var(--fs-sm);
        cursor: pointer;
      }
      .ftab-mi:hover:not(:disabled) {
        background: var(--panel-3);
        color: var(--ink);
      }
      .ftab-mi:disabled {
        color: var(--ink-4);
        cursor: default;
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
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-4);
        border-radius: 999px;
        background: var(--accent);
        color: #06070b;
        font-size: var(--fs-xs);
        font-weight: 600;
        white-space: nowrap;
      }
    `,
  ],
})
export class PaneNodeComponent {
  // Decorator inputs backed by signals (same pattern as RuntimeRowComponent /
  // ReviewCodeComponent) so the component is testable under vitest JIT —
  // signal `input.required` doesn't wire through `componentRef.setInput()` there.
  private readonly nodeSig = signal<PaneNode | undefined>(undefined);
  private readonly ctxSig = signal<PaneCtx | undefined>(undefined);
  @Input({ alias: "node", required: true }) set nodeInput(v: PaneNode) {
    this.nodeSig.set(v);
  }
  @Input({ alias: "ctx", required: true }) set ctxInput(v: PaneCtx) {
    this.ctxSig.set(v);
  }
  readonly node = computed(() => this.nodeSig()!);
  readonly ctx = computed(() => this.ctxSig()!);

  readonly pickOpen = signal(false);
  readonly dragging = signal(false);
  /** Pending dirty-tab close awaiting Save / Discard / Cancel. `paths` is the
   *  full close set (bulk actions include clean tabs); `dirty` the subset the
   *  dialog is really about. */
  readonly confirmClose = signal<{ leafId: string; agentId: string; paths: string[]; dirty: string[] } | null>(null);
  /** Right-click context menu on a file tab (screen coords + anchor path). */
  readonly tabMenu = signal<{ x: number; y: number; leafId: string; path: string } | null>(null);
  readonly fname = fileName;
  readonly views: { k: "terminal" | "diff"; icon: string }[] = [
    { k: "terminal", icon: "terminal" },
    { k: "diff", icon: "diff" },
  ];

  private host = inject(ElementRef<HTMLElement>);
  private drag = inject(DragService);
  private agentActions = inject(AgentActionsService);
  private edits = inject(EditsStore);
  private scroll = inject(ScrollStateService);
  private saver = inject(FileSaveService);
  readonly diagnostics = inject(DiagnosticsService);
  readonly ui = inject(UiStore);
  private picker = viewChild<ElementRef<HTMLElement>>("picker");
  private splitEl = viewChild<ElementRef<HTMLElement>>("splitEl");
  private fileStrip = viewChild<ElementRef<HTMLElement>>("fileStrip");

  constructor() {
    // keep the active file tab visible — a newly opened file lands at the END
    // of an overflowing strip, which would otherwise scroll out of sight
    effect(() => {
      const lf = this.asLeaf();
      const active = lf?.activeFile;
      const strip = this.fileStrip()?.nativeElement;
      if (!active || !strip) return;
      queueMicrotask(() => {
        strip
          .querySelector(`[data-path="${CSS.escape(active)}"]`)
          ?.scrollIntoView({ inline: "nearest", block: "nearest" });
      });
    });
  }

  /** Vertical wheel pans the file strip sideways (same UX as the top tab bar). */
  onStripWheel(e: WheelEvent) {
    if (e.ctrlKey) return;
    const el = e.currentTarget as HTMLElement;
    if (el.scrollWidth <= el.clientWidth) return;
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      el.scrollLeft += e.deltaY;
      e.preventDefault();
    }
  }

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
    const n = this.nodeSig();
    return n?.type === "leaf" ? n : null;
  });
  readonly asSplit = computed<PaneSplit | null>(() => {
    const n = this.nodeSig();
    return n?.type === "split" ? n : null;
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
  // Continue the captured CLI session (claude --resume <id>, codex resume <id>, …)
  // — unlike play, which starts a fresh run without the previous session.
  continueSession(id: string) {
    this.agentActions.act(id, "continueSession");
  }
  agentsOf(projectId: string): Agent[] {
    return this.ctx().agents().filter((a) => a.projectId === projectId);
  }

  // ----- dirty file tabs: indicator + guarded close (B1.1) -----

  isDirty(lf: PaneLeaf, path: string): boolean {
    return !!lf.agentId && this.edits.isDirty(lf.agentId, path);
  }

  /** Close a file tab; a dirty buffer detours through Save/Discard/Cancel. */
  requestFileClose(lf: PaneLeaf, path: string): void {
    this.requestFilesClose(lf, [path]);
  }

  /** Close several tabs at once (context-menu bulk actions); any dirty buffer
   *  in the set raises ONE dialog covering all of them. */
  requestFilesClose(lf: PaneLeaf, paths: string[]): void {
    if (paths.length === 0) return;
    const dirty = lf.agentId ? paths.filter((p) => this.edits.isDirty(lf.agentId!, p)) : [];
    if (dirty.length > 0 && lf.agentId) {
      this.confirmClose.set({ leafId: lf.id, agentId: lf.agentId, paths, dirty });
      return;
    }
    for (const p of paths) this.closeTab(lf.id, lf.agentId, p);
  }

  saveAndClose(cc: { leafId: string; agentId: string; paths: string[]; dirty: string[] }): void {
    void Promise.all(cc.dirty.map((p) => this.saver.save(cc.agentId, p, cc.dirty.length === 1))).then(
      (results) => {
        if (results.some((ok) => !ok)) return; // a save failed — keep tabs + dialog's edits
        if (cc.dirty.length > 1) this.ui.flash(`Saved ${cc.dirty.length} files`);
        this.confirmClose.set(null);
        for (const p of cc.paths) this.closeTab(cc.leafId, cc.agentId, p);
      },
    );
  }

  discardAndClose(cc: { leafId: string; agentId: string; paths: string[]; dirty: string[] }): void {
    for (const p of cc.dirty) {
      const buf = this.edits.get(cc.agentId, p);
      if (buf) this.edits.discard(cc.agentId, p, buf.baseText);
    }
    this.confirmClose.set(null);
    for (const p of cc.paths) this.closeTab(cc.leafId, cc.agentId, p);
  }

  // ----- file-tab context menu: single + bulk close -----

  onFileTabContext(e: MouseEvent, lf: PaneLeaf, path: string): void {
    e.preventDefault();
    e.stopPropagation();
    this.tabMenu.set({ x: e.clientX, y: e.clientY, leafId: lf.id, path });
  }

  /** Tabs a menu action would close, in strip order. */
  tabMenuTargets(lf: PaneLeaf, mode: "one" | "left" | "right" | "all"): string[] {
    const tm = this.tabMenu();
    const files = lf.files ?? [];
    const i = tm ? files.indexOf(tm.path) : -1;
    if (i === -1) return [];
    switch (mode) {
      case "one":
        return [files[i]];
      case "left":
        return files.slice(0, i);
      case "right":
        return files.slice(i + 1);
      case "all":
        return [...files];
    }
  }

  tabMenuClose(lf: PaneLeaf, mode: "one" | "left" | "right" | "all"): void {
    const targets = this.tabMenuTargets(lf, mode);
    this.tabMenu.set(null);
    this.requestFilesClose(lf, targets);
  }

  private closeTab(leafId: string, agentId: string | null, path: string): void {
    // Drop the buffer only when no other leaf still shows this file — the
    // EditsStore key (agent:path) is shared across panes.
    if (agentId && !this.fileOpenElsewhere(agentId, path, leafId)) {
      this.edits.close(agentId, path);
      this.scroll.clear(agentId, path);
    }
    this.ctx().onFileClose(leafId, path);
  }

  private fileOpenElsewhere(agentId: string, path: string, exceptLeafId: string): boolean {
    let found = false;
    const walk = (n: PaneNode): void => {
      if (found) return;
      if (n.type === "leaf") {
        if (n.id !== exceptLeafId && n.agentId === agentId && (n.files ?? []).includes(path)) {
          found = true;
        }
        return;
      }
      walk(n.a);
      walk(n.b);
    };
    for (const root of Object.values(this.ui.paneRoots())) walk(root);
    return found;
  }

  // ----- drag-and-drop: a sidebar agent / tab dropped onto this pane -----
  onDragOver(e: DragEvent, paneId: string) {
    const p = this.drag.payload();
    // file drags (B1.5) route through FileDropService, not the pane splitter
    if (!p?.agentId || p.kind === "file") return;
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
    if (this.drag.payload()?.kind === "file") return; // FileDropService routes it
    e.preventDefault();
    this.ctx().onPaneDrop(paneId);
  }

  @HostListener("document:mousedown", ["$event"])
  onDocDown(e: MouseEvent) {
    if (this.tabMenu()) this.tabMenu.set(null);
    if (!this.pickOpen()) return;
    const pk = this.picker()?.nativeElement;
    if (pk && !pk.contains(e.target as Node)) this.pickOpen.set(false);
  }

  @HostListener("document:keydown.escape")
  onDocEsc(): void {
    if (this.tabMenu()) this.tabMenu.set(null);
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
