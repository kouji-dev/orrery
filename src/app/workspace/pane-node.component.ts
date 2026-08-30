import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  inject,
  Input,
  signal,
  viewChild,
} from "@angular/core";
import { Agent } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
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
import { KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";

/**
 * One node of a workspace pane tree, rendered recursively. A `leaf` is a single
 * pane (agent picker + terminal/diff view + split/close controls); a `split` lays
 * its two children side-by-side (dir "v") or stacked (dir "h") with a draggable
 * divider. All mutations go through the injected `ctx` (the PaneManager).
 */
@Component({
  selector: "app-pane-node",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "(document:mousedown)": "onDocDown($event)" },
  imports: [IconComponent, ToolBadgeComponent, TerminalComponent, DiffViewComponent, FileViewComponent, AgentGitViewComponent, KjButtonComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
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
          <kj-button kjVariant="ghost" kjSize="xs" style="--kj-button-fg: var(--ink)" (click)="$event.stopPropagation(); pickOpen.set(!pickOpen())">
            @if (ag) {
              <span [style.background]="dot(ag.status)" style="width:var(--sp-3);height:var(--sp-3);border-radius:50%;flex:none"></span>
            } @else {
              <app-icon name="agent" size="sm" color="var(--ink-4)" />
            }
            <span class="trunc" style="max-width: round(calc(130px * var(--density)), 1px)">{{ ag ? ag.name : 'Assign agent' }}</span>
            <app-icon size="md" name="chevronD" color="var(--ink-4)" />
          </kj-button>

          @if (pickOpen()) {
            <div #picker class="popover rise" style="position:absolute;top:calc(100% + 4px);left:6px;z-index:40;width: round(calc(230px * var(--density)), 1px);padding:var(--sp-2);max-height: round(calc(320px * var(--density)), 1px);overflow-y:auto">
              @for (p of ctx().projects(); track p.id) {
                @let pa = agentsOf(p.id);
                @if (pa.length) {
                  <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-4) var(--sp-1)">
                    <span [style.background]="p.color" style="width:var(--sp-3);height:var(--sp-3);border-radius:2px"></span>
                    <span class="up" style="color:var(--ink-3)">{{ p.name }}</span>
                  </div>
                  @for (a of pa; track a.id) {
                    <kj-button kjVariant="ghost" kjSize="xs" style="--kj-button-fg: var(--ink)" (click)="ctx().onAgent(lf.id, a.id); pickOpen.set(false)" [style.--kj-button-bg]="lf.agentId === a.id ? 'var(--panel-3)' : 'transparent'">
                      <span [style.background]="dot(a.status)" style="width:var(--sp-3);height:var(--sp-3);border-radius:50%;flex:none"></span>
                      <span class="trunc" style="flex:1">{{ a.name }}</span>
                      <app-tool-badge [tool]="a.tool" [size]="13" />
                    </kj-button>
                  }
                }
              }
            </div>
          }

          @if (ag) {
            <kj-tabs variant="pills" class="tabs-xs" style="margin-left:var(--sp-2)"
                     [value]="lf.view" (valueChange)="ctx().onView(lf.id, $any($event))" (click)="$event.stopPropagation()">
              <kj-tab-list aria-label="Pane view">
                @for (v of views; track v.k) {
                  <kj-tab [value]="v.k" [title]="v.k"><app-icon size="md" [name]="v.icon" [color]="lf.view === v.k ? 'var(--ui-ink)' : null" /></kj-tab>
                }
              </kj-tab-list>
            </kj-tabs>
          }

          <!-- open-file tabs, inline beside the view toggle; scrolls with an
               edge-fade when there are more than fit. Doubles as the flex spacer. -->
          @if (ag) {
            <div #fileStrip class="file-strip scroll-hide" (wheel)="onStripWheel($event)">
              @for (f of lf.files ?? []; track f) {
                @let onf = lf.view === 'file' && lf.activeFile === f;
                @let dirty = isDirty(lf, f);
                <div
                  class="file-tab"
                  [class.on]="onf"
                  [class.dirty]="dirty"
                  [class.preview]="lf.previewFile === f"
                  [title]="f + (dirty ? ' — unsaved changes' : '') + (lf.previewFile === f ? ' — preview (double-click to pin)' : '')"
                  [attr.data-path]="f"
                  (click)="$event.stopPropagation(); ctx().onFileSelect(lf.id, f)"
                  (dblclick)="$event.stopPropagation(); ctx().onFilePin(lf.id, f)"
                  (contextmenu)="onFileTabContext($event, lf, f)"
                >
                  <app-icon size="md" name="file" [color]="onf ? 'var(--ui-ink)' : 'var(--ink-4)'" />
                  <span class="fn trunc">{{ fname(f) }}</span>
                  <span class="fdot" aria-label="unsaved changes"></span>
                  <kj-button kjSize="icon" class="fx" kjVariant="ghost" title="Close file" (click)="$event.stopPropagation(); requestFileClose(lf, f)">
                    <app-icon size="sm" name="x" />
                  </kj-button>
                </div>
              }
            </div>
          }
          @if (ag) {
            @if (ag.worktree) {
              <kj-button kjSize="xs" kjVariant="ghost" (click)="$event.stopPropagation(); diagnostics.openWorktree(ag.worktree)" title="Open worktree folder"><app-icon size="lg" name="folderOpen" /></kj-button>
            }
            @if (ag.sessionId && ag.status !== 'running') {
              <kj-button kjSize="xs" kjVariant="ghost" (click)="$event.stopPropagation(); continueSession(ag.id)" [title]="'Continue last session · ' + ag.tool + ' (' + ag.sessionId + ')'"><app-icon size="lg" name="refresh" /></kj-button>
            }
            <kj-button kjSize="xs" kjVariant="default" (click)="$event.stopPropagation(); toggleRun(ag)" [kjDisabled]="ag.status === 'done'" [title]="runTitle(ag)"><app-icon size="lg" [name]="ag.status === 'running' ? 'pause' : 'play'" /></kj-button>
          }
          <kj-button kjSize="xs" kjVariant="ghost" style="margin-left:auto" (click)="$event.stopPropagation(); ctx().onSplit(lf.id, 'v')" title="Split right"><app-icon size="lg" name="splitCol" /></kj-button>
          <kj-button kjSize="xs" kjVariant="ghost" (click)="$event.stopPropagation(); ctx().onSplit(lf.id, 'h')" title="Split down"><app-icon size="lg" name="splitRow" /></kj-button>
          @if (ctx().canClose()) {
            <kj-button kjSize="xs" kjVariant="ghost" (click)="$event.stopPropagation(); ctx().onClose(lf.id)" title="Close pane"><app-icon size="lg" name="x" /></kj-button>
          }
        </div>

        <!-- pane body -->
        <div style="flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden">
          @if (!ag) {
            <div class="pane-empty">
              <kj-button kjVariant="outline" (click)="$event.stopPropagation(); pickOpen.set(true)"><app-icon name="plus" size="sm" />Assign an agent</kj-button>
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
          <div class="scrim cc-scrim" (mousedown)="$event.stopPropagation()">
            <div class="popover cc-card rise">
              <div class="cc-title">Unsaved changes</div>
              @for (p of cc.dirty; track p) {
                <div class="cc-path trunc">{{ p }}</div>
              }
              <small class="cc-note">Save before closing {{ cc.dirty.length === 1 ? 'this tab' : 'these tabs' }}?</small>
              <div class="cc-actions">
                <kj-button kjVariant="outline" (click)="confirmClose.set(null)">Cancel</kj-button>
                <kj-button kjVariant="danger" style="margin-left:auto" (click)="discardAndClose(cc)">Discard</kj-button>
                <kj-button kjVariant="default" (click)="saveAndClose(cc)">{{ cc.dirty.length === 1 ? 'Save' : 'Save all' }}</kj-button>
              </div>
            </div>
          </div>
        }

        <!-- drag drop preview -->
        @if (dropSide(); as side) {
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
              <app-icon size="md" [name]="side === 'center' ? 'swap' : (side === 'left' || side === 'right') ? 'splitCol' : 'splitRow'" color="var(--ui-on-fill)" />
              {{ side === 'center' ? 'Replace' : 'Split ' + side }}
            </div>
          </div>
        }
      </div>
    } @else if (asSplit(); as sp) {
      <div #splitEl [style.flex-direction]="sp.dir === 'v' ? 'row' : 'column'" style="flex:1;min-width:0;min-height:0;display:flex">
        <app-pane-node [node]="sp.a" [ctx]="ctx()" [style.flex-grow]="sp.ratio" style="flex-shrink:1;flex-basis:0;min-width:0;min-height:0;display:flex" />
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
        <app-pane-node [node]="sp.b" [ctx]="ctx()" [style.flex-grow]="1 - sp.ratio" style="flex-shrink:1;flex-basis:0;min-width:0;min-height:0;display:flex" />
      </div>
    }
  `,
  styles: [
    `
      .pane-leaf {
        border: 1px solid var(--hair);
      }
      .pane-leaf.focused {
        border-color: var(--ui-line);
        box-shadow: 0 0 0 1px var(--ui-sel-2);
      }
      /* open-file tabs live inline in the pane header, right after the view
         toggle. The strip is also the flex spacer (flex:1) and scrolls
         sideways when the tabs outgrow the space; a right edge-fade hints at
         the overflow and a vertical wheel pans it (see onStripWheel).
         Scrollbar hiding comes from the shared .scroll-hide. */
      .file-strip {
        display: flex;
        align-items: center;
        gap: var(--sp-1);
        flex: 1;
        min-width: 0;
        overflow-x: auto;
        -webkit-mask-image: linear-gradient(90deg, #000 calc(100% - 18px), transparent);
        mask-image: linear-gradient(90deg, #000 calc(100% - 18px), transparent);
      }
      /* The inline file strip is a CHIP row, not a tab row: design/app.html
         :5745-5750 draws each open file as a rounded 5px plate that fills with
         --panel-3 and gains a hairline when active. The 2px underline belongs
         to the pane/panel tab ROWS (design:5511-5518), which this is not. */
      .file-tab {
        flex: none;
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        max-width: round(calc(150px * var(--density)), 1px);
        padding: var(--sp-2) var(--sp-3) var(--sp-2) var(--sp-4);
        border: 1px solid transparent;
        border-radius: var(--r-sm);
        font-family: var(--font-mono);
        color: var(--ink-3);
        cursor: pointer;
        position: relative;
      }
      .file-tab:hover {
        color: var(--ink-2);
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
      /* v2 preview tab — italic until pinned (double-click / an edit) */
      .file-tab.preview .fn {
        font-style: italic;
      }
      /* close × — hidden until the tab is hovered or active */
      .file-tab .fx {
        flex: none;
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
        background: var(--ui-ind);
      }
      .file-tab.dirty:not(:hover) .fdot {
        display: block;
      }
      .file-tab.dirty:not(:hover) .fx {
        display: none;
      }
      /* close-confirm dialog for a dirty file tab — the shared .scrim/.popover
         supply the dim + the card surface; only the deltas live here. The
         override back to position:absolute is deliberate: this scrim dims the
         PANE it belongs to, not the whole window. */
      .cc-scrim {
        position: absolute;
        z-index: 30;
      }
      .cc-card {
        width: min(340px, 86%);
        padding: var(--sp-6);
      }
      .cc-title {
        color: var(--ink);
        font-weight: var(--fw-medium);
      }
      .cc-path {
        margin-top: var(--sp-2);
        color: var(--ink-3);
        font-family: var(--font-mono);
        
      }
      /* ink + size + line-height come from the global <small> rule */
      .cc-note {
        margin-top: var(--sp-3);
        }
      .cc-actions {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        margin-top: var(--sp-5);
      }
      .pane-divider {
        position: relative;
        background: transparent;
      }
      .pane-divider .grip-handle {
        background: var(--ui-line);
        border-radius: 999px;
        box-shadow: 0 0 0 1px var(--ui-sel);
        transition: background 0.12s, box-shadow 0.12s;
      }
      .pane-divider:hover .grip-handle,
      .pane-divider.on .grip-handle {
        background: var(--ui-fill);
      }
      .drop-zone {
        position: absolute;
        z-index: 20;
        pointer-events: none;
        background: var(--ui-sel);
        border: 2px solid var(--ui-focus);
        border-radius: var(--r-md);
        box-shadow: none;
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
        background: var(--ui-fill);
        color: var(--ui-on-fill);
        font-weight: var(--fw-medium);
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
  readonly fname = fileName;
  readonly views: { k: "terminal" | "diff"; icon: string }[] = [
    { k: "terminal", icon: "terminal" },
    { k: "diff", icon: "diff" },
  ];

  private host = inject(ElementRef<HTMLElement>);
  private drag = inject(DragService);
  private agentActions = inject(AgentActionsService);
  private runtime = inject(AgentRuntimeService);
  private edits = inject(EditsStore);
  private scroll = inject(ScrollStateService);
  private saver = inject(FileSaveService);
  readonly diagnostics = inject(DiagnosticsService);
  readonly ui = inject(UiStore);
  private picker = viewChild<ElementRef<HTMLElement>>("picker");
  private splitEl = viewChild<ElementRef<HTMLElement>>("splitEl");
  private fileStrip = viewChild<ElementRef<HTMLElement>>("fileStrip");

  constructor() {
    // v2 project tabs: auto-open the shell the FIRST time the project
    // terminal pane shows (shellState undefined = never launched). A shell
    // the user stopped (false) stays stopped until they press play again.
    effect(() => {
      const lf = this.asLeaf();
      const ag = this.agent();
      if (
        lf?.view === "terminal" &&
        ag?.tool === "shell" &&
        this.runtime.shellState(ag.id) === undefined
      ) {
        this.runtime.startShell(ag.id);
      }
    });
    // v2: an EDIT pins the preview tab — a modified buffer must never be
    // silently replaced by the next preview.
    effect(() => {
      const lf = this.asLeaf();
      const preview = lf?.previewFile;
      if (lf && preview && this.isDirty(lf, preview)) {
        this.ctx().onFilePin(lf.id, preview);
      }
    });
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
  dot(status: string): string {
    return STATUS_META[status as keyof typeof STATUS_META]?.color ?? "var(--ink-3)";
  }
  toggleRun(ag: Agent) {
    // v2 project pseudo-agent: play/pause drives the plain shell, not a tool
    if (ag.tool === "shell") {
      if (ag.status === "running") this.runtime.stopShell(ag.id);
      else this.runtime.startShell(ag.id);
      return;
    }
    this.agentActions.toggleRun(ag);
  }
  runTitle(ag: Agent): string {
    if (ag.tool === "shell") return ag.status === "running" ? "Stop shell" : "Open shell";
    return ag.status === "running" ? "Pause agent" : ag.started ? "Resume agent" : "Start agent";
  }
  // Continue the captured CLI session (claude --resume <id>, codex resume <id>, …)
  // — unlike play, which starts a fresh run without the previous session.
  continueSession(id: string) {
    this.agentActions.act(id, "continueSession");
  }
  agentsOf(projectId: string): Agent[] {
    // pseudo project-agents never appear in the assign picker — they belong to
    // their project tab, not to arbitrary panes
    return this.ctx().agents().filter((a) => a.projectId === projectId && a.tool !== "shell");
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
    const close = (mode: "one" | "left" | "right" | "all") => () =>
      this.requestFilesClose(lf, this.tabMenuTargets(lf, path, mode));
    this.ui.openMenu(e, [
      { label: "Close", icon: "x", onClick: close("one") },
      {
        label: "Close All to the Left",
        icon: "chevron",
        disabled: !this.tabMenuTargets(lf, path, "left").length,
        onClick: close("left"),
      },
      {
        label: "Close All to the Right",
        icon: "chevron",
        disabled: !this.tabMenuTargets(lf, path, "right").length,
        onClick: close("right"),
      },
      { label: "Close All", icon: "layers", onClick: close("all") },
    ]);
  }

  /** Tabs a menu action would close, in strip order, anchored at `path`. */
  tabMenuTargets(lf: PaneLeaf, path: string, mode: "one" | "left" | "right" | "all"): string[] {
    const files = lf.files ?? [];
    const i = files.indexOf(path);
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
