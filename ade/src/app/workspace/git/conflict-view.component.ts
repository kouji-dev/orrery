import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  inject,
  input,
  output,
  signal,
  viewChild,
} from "@angular/core";
import { Agent, ConflictFile } from "../../models";
import { AgentActionsService } from "../../agents/agent-actions.service";
import { ConflictStore } from "../../agents/conflict.store";
import { AgentsStore } from "../../stores/agents.store";
import { IconComponent } from "../../shared/icon.component";
import {
  GitActionButtonComponent,
  GitActionAiEvent,
} from "../../shared/git/git-action-button.component";
import { UiStore } from "../../ui/ui.store";
import { fileDir, fileName } from "../../utils";
import { KjBadgeComponent, KjButtonComponent, KjTextareaComponent } from "@kouji-ui/components";

// ---- diff3 conflict-marker parsing ------------------------------------------
// The backend checks the merge out with conflict_style_diff3, so `merged` is
//   <<<<<<< ours-label / ours… / ||||||| base-label / base… / ======= /
//   theirs… / >>>>>>> theirs-label
// (the ||||||| base section is absent for add/add conflicts).

export type ConflictSegment =
  | { type: "ctx"; lines: string[] }
  | { type: "conflict"; ours: string[]; base: string[]; theirs: string[] };

export function parseConflictSegments(merged: string): ConflictSegment[] {
  const out: ConflictSegment[] = [];
  let ctx: string[] = [];
  const lines = merged.split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("<<<<<<<")) {
      if (ctx.length) {
        out.push({ type: "ctx", lines: ctx });
        ctx = [];
      }
      const ours: string[] = [];
      const base: string[] = [];
      const theirs: string[] = [];
      i++;
      let mode: "ours" | "base" | "theirs" = "ours";
      let closed = false;
      for (; i < lines.length; i++) {
        const l = lines[i];
        if (l.startsWith("|||||||")) {
          mode = "base";
        } else if (l.startsWith("=======") && mode !== "theirs") {
          mode = "theirs";
        } else if (l.startsWith(">>>>>>>")) {
          i++;
          closed = true;
          break;
        } else {
          (mode === "ours" ? ours : mode === "base" ? base : theirs).push(l);
        }
      }
      // An unterminated block (shouldn't happen) degrades to context.
      if (closed) out.push({ type: "conflict", ours, base, theirs });
      else ctx.push(...ours, ...base, ...theirs);
    } else {
      ctx.push(line);
      i++;
    }
  }
  if (ctx.length) out.push({ type: "ctx", lines: ctx });
  return out;
}

export type SideChoice = "ours" | "theirs" | "both";
interface SegResolution {
  res: SideChoice | null;
  custom: string | null;
}

/** Chosen result lines for a resolved conflict segment. */
function resultLines(seg: Extract<ConflictSegment, { type: "conflict" }>, r: SideChoice): string[] {
  if (r === "ours") return seg.ours;
  if (r === "theirs") return seg.theirs;
  return [...seg.ours, ...seg.theirs];
}

/**
 * B4.2 — the flagship 3-way conflict view. Renders the A3.6 session model:
 * conflicted-file list with progress, per-file base/ours/theirs conflict
 * blocks with accept-side / accept-both / edit-result actions, per-file AI
 * resolve (dropdown with its estimate, per A4), and Commit-merge / Abort
 * session actions. High-fidelity port of design repo-conflict.jsx onto the
 * app's CSS conventions.
 */
@Component({
  selector: "app-conflict-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, GitActionButtonComponent, KjButtonComponent, KjBadgeComponent, KjTextareaComponent],
  template: `
    @if (session(); as s) {
      <div class="split-pane cf-grid">
        <!-- ── conflicted file list + session actions ─────────────────── -->
        <div class="split-pane-side cf-side">
          <div class="pane-head cf-side-head" style="display:block">
            <div style="display:flex;align-items:center;gap:var(--sp-3)">
              <app-icon name="branch" size="sm" color="var(--vcs-conflicted)" />
              <span style="font-weight:var(--fw-medium);color:var(--ink)">Merge {{ s.theirs }} → {{ s.ours }}</span>
            </div>
            <div class="tnum" style="display:flex;align-items:center;gap:var(--sp-4);margin-top:var(--sp-4);font-size:var(--fs-meta);color:var(--ink-4)">
              <span style="display:flex;align-items:center;gap:var(--sp-2)"><span class="sq" style="background:var(--ink-4)"></span>ours · {{ s.ours }}</span>
              <span style="display:flex;align-items:center;gap:var(--sp-2)"><span class="sq" style="background:var(--ink-2)"></span>theirs · {{ s.theirs }}</span>
            </div>
            <div style="margin-top:var(--sp-5)">
              <div class="tnum" style="display:flex;justify-content:space-between;font-size:var(--fs-meta);color:var(--ink-3);margin-bottom:var(--sp-2)">
                <span>{{ totalResolved() }} resolved</span><span>{{ totalConflicts() - totalResolved() }} remaining</span>
              </div>
              <div class="meter">
                <i [style.width.%]="totalConflicts() ? (totalResolved() / totalConflicts()) * 100 : 0"
                   [style.background]="allDone() ? 'var(--st-done)' : 'var(--ui-ind)'"></i>
              </div>
            </div>
          </div>

          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (f of s.files; track f.path) {
              @let rc = resolvedCount(f);
              @let done = rc === conflictCount(f);
              <div class="cf-file list-row" [class.sel]="f.path === activePath()" (click)="selectFile(f.path)">
                <span class="cf-dot" [class.done]="done">
                  @if (done) { <app-icon size="sm" name="check" color="var(--ui-on-fill)" /> } @else { <span class="pip"></span> }
                </span>
                <div style="flex:1;min-width:0">
                  <div class="trunc" style="color:var(--ink)">{{ fname(f.path) }}</div>
                  <div class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4)">{{ fdir(f.path) }}</div>
                </div>
                <span class="tnum" style="flex:none;font-size:var(--fs-meta)" [style.color]="done ? 'var(--st-done)' : 'var(--vcs-conflicted)'">{{ rc }}/{{ conflictCount(f) }}</span>
              </div>
            }
          </div>

          <div class="cf-actions">
            <kj-button class="kj-center" [kjVariant]="allDone() ? 'default' : 'outline'" [kjDisabled]="!allDone() || busy()"
              (click)="commitMerge()">
              <app-icon name="commit" size="sm" />Commit merge
            </kj-button>
            <kj-button class="kj-center" kjVariant="danger" [kjDisabled]="busy()" (click)="abort()">
              <app-icon name="discard" size="sm" />Abort
            </kj-button>
          </div>
        </div>

        <!-- ── per-file 3-way merge surface ───────────────────────────── -->
        @if (activeFile(); as f) {
          <div class="split-pane-main cf-main">
            <div class="pane-head cf-main-head">
              <app-icon size="lg" name="merge" color="var(--vcs-conflicted)" />
              <span style="color:var(--ink-4)">{{ fdir(f.path) }}</span><span style="margin-left:calc(-1 * var(--sp-2))">{{ fname(f.path) }}</span>
              <kj-badge class="tnum" style="font-size:var(--fs-badge);padding:1px var(--sp-3);color:var(--vcs-conflicted);border-color:color-mix(in oklch, var(--vcs-conflicted), transparent 55%)">3-way merge</kj-badge>
              <div style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-3)">
                <kj-button kjVariant="outline" (click)="acceptAll('ours')">Accept all ours</kj-button>
                <kj-button kjVariant="outline" (click)="acceptAll('theirs')">theirs</kj-button>
                <!-- per-file AI resolve: native = accept both everywhere;
                     dropdown = the agent drives the resolution (estimate on the row) -->
                <app-git-action-button
                  label="Auto-resolve"
                  icon="merge"
                  [small]="true"
                  title="Accept both sides for every conflict · native"
                  [estimateInput]="fileEstimateInput()"
                  [variants]="aiVariants"
                  (native)="acceptAll('both')"
                  (ai)="resolveWithAi($event)"
                  style="flex:none"
                />
                @if (f.resolved) {
                  <kj-badge class="tnum" style="font-size:var(--fs-badge);color:var(--st-done);border-color:color-mix(in oklch, var(--st-done), transparent 55%)">staged</kj-badge>
                } @else {
                  <kj-button kjVariant="outline" [kjDisabled]="resolvedCount(f) !== conflictCount(f) || busy()" title="Write the chosen result and stage this file" (click)="stageFile(f)">
                    <app-icon size="md" name="check" />Stage file
                  </kj-button>
                }
                <div class="cf-nav">
                  <button kjButton class="pane-btn" (click)="gotoConf(-1)" title="Previous conflict"><app-icon size="lg" name="chevron" style="transform:rotate(180deg)" /></button>
                  <span class="tnum" style="font-size:var(--fs-meta);color:var(--ink-3);padding:0 var(--sp-2)">{{ cursor() + 1 }}/{{ confIdxs().length }}</span>
                  <button kjButton class="pane-btn" (click)="gotoConf(1)" title="Next conflict"><app-icon size="lg" name="chevron" /></button>
                </div>
              </div>
            </div>

            <div #scrollBody class="scroll-y" style="flex:1;padding:var(--sp-2) 0 var(--sp-8);min-height:0">
              @for (seg of segments(); track $index) {
                @if (seg.type === 'ctx') {
                  <pre class="cf-ctx">@for (l of seg.lines; track $index) {<div class="ln"><span class="g"></span><code>{{ l }}</code></div>}</pre>
                } @else {
                  @let idx = $index;
                  @let res = resolutionOf(idx);
                  @let done = !!res.res;
                  <div class="cf-block" [attr.data-conf]="idx" [class.done]="done">
                    <div class="cf-block-head" [class.done]="done">
                      <span class="st-dot" [style.background]="done ? 'var(--st-done)' : 'var(--vcs-conflicted)'"></span>
                      <span style="font-weight:var(--fw-medium);color:var(--ink)">Conflict {{ confNum(idx) }}</span>
                      <span style="font-size:var(--fs-meta)" [style.color]="done ? 'var(--st-done)' : 'var(--st-blocked)'">
                        {{ done ? 'resolved · ' + resLabel(res) : 'unresolved' }}
                      </span>
                      <div style="margin-left:auto;display:flex;gap:var(--sp-3)">
                        <kj-button kjVariant="outline" (click)="resolveSeg(idx, 'both')">Both</kj-button>
                        <kj-button kjVariant="outline" [style.--kj-button-fg]="baseOpen()[idx] ? 'var(--ink)' : 'var(--ink-3)'" (click)="toggleBase(idx)">
                          <app-icon size="md" [name]="baseOpen()[idx] ? 'chevronD' : 'chevron'" />Base
                        </kj-button>
                      </div>
                    </div>

                    @if (baseOpen()[idx]) {
                      <div style="padding:var(--sp-2) 0;border-bottom:1px solid var(--hair);background:var(--bg)">
                        <div class="up" style="color:var(--ink-4);padding:0 var(--sp-6) var(--sp-2)">base · common ancestor</div>
                        <pre class="cf-base">{{ seg.base.length ? seg.base.join('\n') : '(no common ancestor — both sides added this)' }}</pre>
                      </div>
                    }

                    <div style="display:flex;gap:var(--sp-3);padding:var(--sp-4)">
                      <div class="side" [class.chosen]="res.res === 'ours'" style="--side:var(--ink-4)">
                        <div class="side-head">
                          <span class="sq" style="background:var(--ink-4)"></span>
                          <span style="font-weight:var(--fw-medium);color:var(--ink)">Ours</span>
                          <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-4)">{{ s.ours }}</span>
                          <kj-button kjVariant="outline" [kjPressed]="res.res === 'ours'" (click)="resolveSeg(idx, 'ours')">
                            @if (res.res === 'ours') { <app-icon size="md" name="check" />accepted } @else { accept }
                          </kj-button>
                        </div>
                        <pre class="side-code">@for (l of seg.ours; track $index) {<div class="ln"><span class="g">{{ res.res === 'ours' ? '✓' : '·' }}</span><code>{{ l }}</code></div>}</pre>
                      </div>
                      <div class="side" [class.chosen]="res.res === 'theirs'" style="--side:var(--ink-2)">
                        <div class="side-head">
                          <span class="sq" style="background:var(--ink-2)"></span>
                          <span style="font-weight:var(--fw-medium);color:var(--ink)">Theirs</span>
                          <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-4)">{{ s.theirs }}</span>
                          <kj-button kjVariant="outline" [kjPressed]="res.res === 'theirs'" (click)="resolveSeg(idx, 'theirs')">
                            @if (res.res === 'theirs') { <app-icon size="md" name="check" />accepted } @else { accept }
                          </kj-button>
                        </div>
                        <pre class="side-code">@for (l of seg.theirs; track $index) {<div class="ln"><span class="g">{{ res.res === 'theirs' ? '✓' : '·' }}</span><code>{{ l }}</code></div>}</pre>
                      </div>
                    </div>

                    <div style="border-top:1px solid var(--hair);background:var(--bg)">
                      <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-5)">
                        <app-icon size="md" name="enter" [color]="done ? 'var(--st-done)' : 'var(--ink-4)'" />
                        <span class="up" style="color:var(--ink-4)">Result</span>
                        @if (done) {
                          <kj-button kjVariant="ghost" [style.--kj-button-fg]="editing() === idx ? 'var(--ui-ink)' : 'var(--ink-3)'" (click)="editing.set(editing() === idx ? null : idx)">
                            <app-icon size="md" name="rename" />{{ editing() === idx ? 'done' : 'edit' }}
                          </kj-button>
                        }
                      </div>
                      @if (done) {
                        @if (editing() === idx) {
                          <!-- focusout, not blur: the kj-textarea host is display:contents,
                               so only bubbling events reach a host listener -->
                          <kj-textarea class="cf-edit" [kjValue]="resultText(seg, res)"
                            (focusout)="editResult(idx, $any($event.target).value)" />
                        } @else {
                          <pre class="cf-result">@for (l of resultText(seg, res).split('\n'); track $index) {<div class="ln"><span class="g">+</span><code>{{ l }}</code></div>}</pre>
                        }
                      } @else {
                        <div style="padding:var(--sp-4) var(--sp-6);color:var(--ink-4);border-top:1px dashed var(--hair-2);display:flex;align-items:center;gap:var(--sp-3)">
                          <app-icon size="md" name="flag" color="var(--st-blocked)" />pick a side above, or accept both, to resolve
                        </div>
                      }
                    </div>
                  </div>
                }
              }
            </div>
          </div>
        } @else {
          <div class="pane-empty">no conflicted files</div>
        }
      </div>
    } @else if (loading()) {
      <div class="pane-empty">reading merge session…</div>
    } @else {
      <div class="pane-empty">no merge in progress</div>
    }
  `,
  styles: [
    `
      /* geometry: .split-pane / .split-pane-side / .split-pane-main + the
         .pane-head header rows; the host fill is the shared app-conflict-view
         rule in styles.css. Only the column width and this view's own grounds
         are left here. */
      .cf-grid {
        --split-w: 292px;
      }
      .sq {
        width: 7px;
        height: 7px;
        border-radius: 2px;
        flex: none;
        display: inline-block;
      }
      /* box comes from the shared .meter; the fill animation is local */
      .meter > i {
        transition: width 0.2s ease;
      }
      /* margin / radius / cursor / hover / selected all come from .list-row */
      .cf-file {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-3) var(--sp-5);
      }
      .cf-dot {
        flex: none;
        width: 15px;
        height: 15px;
        border-radius: 50%;
        display: grid;
        place-items: center;
        background: color-mix(in oklch, var(--vcs-conflicted), transparent 80%);
        border: 1px solid color-mix(in oklch, var(--vcs-conflicted), transparent 50%);
      }
      .cf-dot.done {
        background: var(--st-done);
        border: none;
      }
      .cf-dot .pip {
        width: 5px;
        height: 5px;
        border-radius: 50%;
        background: var(--vcs-conflicted);
      }
      .cf-actions {
        padding: var(--sp-4);
        border-top: 1px solid var(--hair);
        display: grid;
        gap: var(--sp-3);
        flex: none;
      }
      .cf-main {
        background: var(--bg);
      }
      .cf-main-head {
        gap: var(--sp-3);
        padding: var(--sp-3) var(--sp-5);
        background: var(--panel);
        }
      .cf-nav {
        display: flex;
        align-items: center;
        gap: var(--sp-1);
        padding: var(--sp-1);
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
      }
      /* context + code lines share the mono ln/g/code layout */
      pre {
        margin: 0;
        font-family: var(--font-mono);
        line-height: 1.65;
      }
      .ln {
        display: flex;
      }
      .ln .g {
        width: 14px;
        flex: none;
        text-align: center;
        user-select: none;
        opacity: 0.7;
      }
      .ln code {
        flex: 1;
        white-space: pre-wrap;
        word-break: break-word;
        padding-right: var(--sp-4);
      }
      .cf-ctx {
        padding: var(--sp-1) 0;
      }
      .cf-ctx code {
        color: var(--ink-3);
        padding-left: var(--sp-6);
      }
      .cf-block {
        margin: var(--sp-4) var(--sp-6);
        border: 1px solid color-mix(in oklch, var(--vcs-conflicted), transparent 55%);
        border-radius: var(--r-md);
        overflow: hidden;
        background: var(--panel);
      }
      .cf-block.done {
        border-color: color-mix(in oklch, var(--st-done), transparent 60%);
      }
      .cf-block-head {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-3) var(--sp-4);
        background: color-mix(in oklch, var(--vcs-conflicted), transparent 91%);
      }
      .cf-block-head.done {
        background: color-mix(in oklch, var(--st-done), transparent 90%);
      }
      .st-dot {
        width: 8px;
        height: 8px;
        border-radius: 50%;
        flex: none;
      }
      .cf-base {
        color: var(--ink-3);
        padding: 0 var(--sp-6);
        line-height: 1.6;
      }
      .side {
        flex: 1;
        min-width: 0;
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
        overflow: hidden;
        background: var(--panel-2);
      }
      .side.chosen {
        border-color: var(--side);
        background: color-mix(in oklch, var(--side), transparent 92%);
      }
      .side-head {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-4);
        border-bottom: 1px solid var(--hair);
        background: color-mix(in oklch, var(--side), transparent 88%);
      }
      .side-code {
        padding: var(--sp-2) 0;
      }
      .side-code .g {
        color: var(--side);
      }
      .side-code code {
        color: var(--ink-2);
      }
      .cf-result {
        padding: var(--sp-1) var(--sp-5) var(--sp-3) var(--sp-6);
        background: var(--code-add-bg);
      }
      .cf-result .g,
      .cf-result code {
        color: var(--code-add-ink);
      }
      /* Result editor — kouji declares the --kj-textarea-* knobs ON the inner
         .kj-textarea (layered), so this unlayered ::ng-deep rule retargets them
         there; host-level custom properties would lose to those declarations. */
      /* only the DELTAS from the app-wide kj-textarea defaults in styles.css */
      :host ::ng-deep kj-textarea.cf-edit .kj-textarea {
        --kj-textarea-fg: var(--code-add-ink);
        --kj-textarea-border-color: var(--ui-focus);
        --kj-textarea-radius: 0;
        --kj-textarea-line-height: 1.65;
        --kj-textarea-padding-y: var(--sp-2);
        --kj-textarea-min-height: 70px;
        padding-left: var(--sp-6);
      }
    `,
  ],
})
export class ConflictViewComponent {
  private readonly store = inject(ConflictStore);
  private readonly agents = inject(AgentsStore);
  private readonly actions = inject(AgentActionsService);
  private readonly ui = inject(UiStore);

  readonly agent = input.required<Agent>();
  readonly close = output<void>();

  readonly fname = fileName;
  readonly fdir = fileDir;

  private readonly agentId = computed(() => this.agent().id);
  readonly entry = computed(() => this.store.sessionFor(this.agentId()));
  readonly session = computed(() => this.entry().data);
  readonly loading = computed(() => this.entry().status === "loading");
  readonly busy = signal(false);

  readonly activePath = signal<string | null>(null);
  readonly activeFile = computed<ConflictFile | undefined>(() => {
    const s = this.session();
    if (!s) return undefined;
    return s.files.find((f) => f.path === this.activePath()) ?? s.files[0];
  });
  readonly segments = computed<ConflictSegment[]>(() => {
    const f = this.activeFile();
    return f ? parseConflictSegments(f.merged) : [];
  });
  readonly confIdxs = computed(() =>
    this.segments()
      .map((s, i) => (s.type === "conflict" ? i : -1))
      .filter((i) => i >= 0),
  );

  /** { path: { segIdx: {res, custom} } } — per-file choices, UI-local. */
  private readonly resMap = signal<Record<string, Record<number, SegResolution>>>({});
  readonly cursor = signal(0);
  readonly editing = signal<number | null>(null);
  readonly baseOpen = signal<Record<number, boolean>>({});

  private readonly scrollBody = viewChild<ElementRef<HTMLElement>>("scrollBody");

  readonly aiVariants = [
    { id: "file", label: "Resolve with AI (agent drives the merge)", icon: "sparkles" },
    { id: "file-v", label: "Resolve with AI (verbose)", verbose: true, icon: "sparkles" },
  ];
  readonly fileEstimateInput = computed(() => {
    const f = this.activeFile();
    const ag = this.agent();
    return {
      op: "conflict" as const,
      conflicts: this.confIdxs().length,
      files: 1,
      // Why bytes from both sides: the model must read ours AND theirs (plus
      // context) to resolve — merged length is the closest cheap proxy.
      diffBytes: f ? f.merged.length : 0,
      model: ag.model,
    };
  });

  private lastId: string | null = null;
  constructor() {
    // Recovery path: the view opened without a store session (e.g. reload
    // while a merge was in progress) → read session_state and re-list.
    effect(() => {
      const id = this.agentId();
      if (this.entry().status !== "idle") return;
      void this.agents
        .sessionState(id)
        .then((st) => {
          if (st.state === "none") return;
          this.store.load(id, st.ours, "incoming");
        })
        .catch(() => undefined);
    });
    // reset per-file UI state when switching agents
    effect(() => {
      const id = this.agentId();
      if (id !== this.lastId) {
        this.lastId = id;
        this.activePath.set(null);
        this.resMap.set({});
        this.cursor.set(0);
        this.editing.set(null);
        this.baseOpen.set({});
      }
    });
  }

  // ---- per-file derived counts ----
  conflictCount(f: ConflictFile): number {
    if (f.resolved) return this.segCountOf(f);
    return parseConflictSegments(f.merged).filter((s) => s.type === "conflict").length || 1;
  }
  private segCountOf(f: ConflictFile): number {
    const n = parseConflictSegments(f.merged).filter((s) => s.type === "conflict").length;
    return n || 1;
  }
  resolvedCount(f: ConflictFile): number {
    if (f.resolved) return this.conflictCount(f);
    const rm = this.resMap()[f.path] ?? {};
    return Object.values(rm).filter((r) => r.res).length;
  }
  readonly totalConflicts = computed(() => {
    const s = this.session();
    return s ? s.files.reduce((n, f) => n + this.conflictCount(f), 0) : 0;
  });
  readonly totalResolved = computed(() => {
    const s = this.session();
    // read resMap so the computed re-evaluates on per-segment choices
    this.resMap();
    return s ? s.files.reduce((n, f) => n + this.resolvedCount(f), 0) : 0;
  });
  /** Every file staged → the merge can be committed. */
  readonly allDone = computed(() => {
    const s = this.session();
    return !!s && s.files.length > 0 && s.files.every((f) => f.resolved);
  });

  resolutionOf(idx: number): SegResolution {
    const f = this.activeFile();
    if (!f) return { res: null, custom: null };
    return this.resMap()[f.path]?.[idx] ?? { res: null, custom: null };
  }
  resLabel(r: SegResolution): string {
    if (r.custom != null) return "edited";
    return r.res === "both" ? "both sides" : (r.res ?? "");
  }
  resultText(seg: Extract<ConflictSegment, { type: "conflict" }>, r: SegResolution): string {
    if (r.custom != null) return r.custom;
    return r.res ? resultLines(seg, r.res).join("\n") : "";
  }
  confNum(idx: number): number {
    return this.confIdxs().indexOf(idx) + 1;
  }

  // ---- interactions ----
  selectFile(path: string): void {
    this.activePath.set(path);
    this.cursor.set(0);
    this.editing.set(null);
    this.baseOpen.set({});
  }
  resolveSeg(idx: number, res: SideChoice): void {
    const f = this.activeFile();
    if (!f || f.resolved) return;
    this.resMap.update((m) => ({
      ...m,
      [f.path]: { ...(m[f.path] ?? {}), [idx]: { res, custom: null } },
    }));
  }
  editResult(idx: number, custom: string): void {
    const f = this.activeFile();
    if (!f) return;
    this.resMap.update((m) => ({
      ...m,
      [f.path]: { ...(m[f.path] ?? {}), [idx]: { ...(m[f.path]?.[idx] ?? { res: "both" }), custom } },
    }));
    this.editing.set(null);
  }
  acceptAll(side: SideChoice): void {
    const f = this.activeFile();
    if (!f || f.resolved) return;
    this.resMap.update((m) => {
      const fm = { ...(m[f.path] ?? {}) };
      for (const ci of this.confIdxs()) fm[ci] = { res: side, custom: null };
      return { ...m, [f.path]: fm };
    });
    this.ui.flash("accepted " + side + " for all conflicts in " + fileName(f.path));
  }
  toggleBase(idx: number): void {
    this.baseOpen.update((m) => ({ ...m, [idx]: !m[idx] }));
  }
  gotoConf(dir: number): void {
    const idxs = this.confIdxs();
    if (!idxs.length) return;
    const next = Math.max(0, Math.min(idxs.length - 1, this.cursor() + dir));
    this.cursor.set(next);
    const body = this.scrollBody()?.nativeElement;
    const el = body?.querySelector<HTMLElement>(`[data-conf="${idxs[next]}"]`);
    // Why -70: keep the block header visible under the sticky file header.
    if (el && body) body.scrollTop = el.offsetTop - 70;
  }

  /** Rebuild the file from ctx + chosen lines and stage it via
   *  `conflict_resolve` (writes + stages backend-side). */
  stageFile(f: ConflictFile): void {
    const segs = parseConflictSegments(f.merged);
    const rm = this.resMap()[f.path] ?? {};
    const parts: string[] = [];
    for (let i = 0; i < segs.length; i++) {
      const seg = segs[i];
      if (seg.type === "ctx") {
        parts.push(...seg.lines);
      } else {
        const r = rm[i];
        if (!r?.res) return; // unresolved — button should be disabled anyway
        parts.push(...(r.custom != null ? r.custom.split("\n") : resultLines(seg, r.res)));
      }
    }
    this.busy.set(true);
    void this.store
      .resolve(this.agentId(), f.path, parts.join("\n"))
      .then(() => this.ui.flash("staged " + fileName(f.path)))
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "stage failed"))
      .finally(() => this.busy.set(false));
  }

  commitMerge(): void {
    const id = this.agentId();
    const s = this.session();
    this.busy.set(true);
    void this.store
      .commit(id)
      .then((sha) => {
        this.ui.flash("committed merge " + sha + (s ? " — " + s.theirs + " → " + s.ours : ""));
        this.close.emit();
      })
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "commit merge failed"))
      .finally(() => this.busy.set(false));
  }
  abort(): void {
    const id = this.agentId();
    this.busy.set(true);
    void this.store
      .abort(id)
      .then(() => {
        this.ui.flash("aborted merge — working tree restored");
        this.close.emit();
      })
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "abort failed"))
      .finally(() => this.busy.set(false));
  }

  /** AI path (A4.5): the dropdown branch is the existing PTY-driving
   *  aiAction("merge") verbatim — the agent resolves the session in its own
   *  terminal. The backend merge state stays; the agent picks it up. */
  resolveWithAi(_ev: GitActionAiEvent): void {
    this.actions.aiAction(this.agentId(), "merge");
  }
}
