import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import {
  BRIDGE,
  Commands,
  Events,
  FileReplaceResult,
  SearchDonePayload,
  SearchMatchEntry,
  SearchResultsPayload,
} from "../data-source/bridge";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName } from "../utils";
import { CommandRegistryService } from "./command-registry.service";
import { OverlayFooterComponent, OverlayShellComponent } from "./overlay-shell.component";

type Scope = "worktree" | "project" | "all";
const SCOPES: { k: Scope; label: string }[] = [
  { k: "worktree", label: "This worktree" },
  { k: "project", label: "This project" },
  { k: "all", label: "All worktrees" },
];

interface Group {
  key: string;
  path: string;
  root: string | null;
  agentId: string | null;
  items: SearchMatchEntry[];
  count: number;
}

/** Highlight segments of a match line, leading whitespace trimmed (the design
 *  trims and shifts the ranges the same way). */
function lineSegments(m: SearchMatchEntry): { t: string; hit: boolean }[] {
  const trim = m.text.length - m.text.replace(/^\s+/, "").length;
  const text = m.text.slice(trim);
  const ranges = m.ranges
    .map(([s, e]) => [Math.max(0, s - trim), Math.max(0, e - trim)] as [number, number])
    .filter(([s, e]) => e > s);
  if (!ranges.length) return [{ t: text, hit: false }];
  const out: { t: string; hit: boolean }[] = [];
  let at = 0;
  for (const [s, e] of ranges) {
    if (s > at) out.push({ t: text.slice(at, s), hit: false });
    out.push({ t: text.slice(s, Math.min(e, text.length)), hit: true });
    at = Math.min(e, text.length);
  }
  if (at < text.length) out.push({ t: text.slice(at), hit: false });
  return out;
}

/**
 * Find in Files (roadmap B3.1, Ctrl+Shift+F) — streams matches from the Rust
 * grep engine as they arrive (`search://results` batches, `search://done`),
 * cancellable, scoped to this worktree / this project / all worktrees.
 * High-fidelity port of design/orrery/project/search-panel.jsx (find mode;
 * replace lands with B3.2), design px mapped to the app's --sp/--fs tokens.
 */
@Component({
  selector: "app-find-in-files",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, OverlayShellComponent, OverlayFooterComponent],
  template: `
    <app-overlay-shell [width]="860" top="9vh" label="Find in files" (closed)="registry.close()">
      <!-- query row -->
      <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
        <div style="display:flex;gap:var(--sp-1);padding:var(--sp-1);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);flex:none">
          <button class="btn" (click)="mode.set('find')" style="padding:var(--sp-2) var(--sp-4);border-radius:4px;font-size:var(--fs-xs)"
            [style.background]="mode() === 'find' ? 'var(--panel-3)' : 'transparent'"
            [style.color]="mode() === 'find' ? 'var(--ink)' : 'var(--ink-3)'"
            [style.box-shadow]="mode() === 'find' ? '0 0 0 1px var(--hair-2)' : 'none'">Find</button>
          <button class="btn" (click)="mode.set('replace')" title="Replace in Files" style="padding:var(--sp-2) var(--sp-4);border-radius:4px;font-size:var(--fs-xs)"
            [style.background]="mode() === 'replace' ? 'var(--panel-3)' : 'transparent'"
            [style.color]="mode() === 'replace' ? 'var(--ink)' : 'var(--ink-3)'"
            [style.box-shadow]="mode() === 'replace' ? '0 0 0 1px var(--hair-2)' : 'none'">Replace</button>
        </div>
        <input
          #inp
          [value]="q()"
          (input)="onInput($event)"
          (keydown)="onKeys($event)"
          placeholder="Search in files…"
          spellcheck="false"
          autocomplete="off"
          [style.border-color]="focusedInput() ? 'var(--ui-focus)' : 'var(--hair)'"
          (focus)="focusedInput.set(true)"
          (blur)="focusedInput.set(false)"
          style="flex:1;min-width:0;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-3) var(--sp-4);color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-ui);outline:none"
        />
        <div style="display:flex;gap:var(--sp-2);flex:none">
          <button class="btn" (click)="toggle('caseSensitive')" title="Match case" [style]="optStyle(caseSensitive())">Aa</button>
          <button class="btn" (click)="toggle('word')" title="Words" [style]="optStyle(word())">W</button>
          <button class="btn" (click)="toggle('regex')" title="Regular expression" [style]="optStyle(regex())">.*</button>
        </div>
        <select
          class="osel"
          [value]="scope()"
          (change)="onScope($event)"
          style="width:150px;flex:none;padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)"
        >
          @for (s of scopes; track s.k) {
            <option [value]="s.k" [disabled]="s.k === 'worktree' && !scopeAgent()">{{ s.label }}</option>
          }
        </select>
      </div>

      <!-- replacement row (replace mode) -->
      @if (mode() === 'replace') {
        <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
          <span style="flex:none;font-size:var(--fs-xs);color:var(--ink-3);width:64px;text-align:right">replace →</span>
          <input
            [value]="replacement()"
            (input)="replacement.set($any($event.target).value)"
            placeholder="Replacement ($1 for capture groups in regex mode)…"
            spellcheck="false"
            autocomplete="off"
            style="flex:1;min-width:0;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-2) var(--sp-4);color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm);outline:none"
          />
          <button
            class="btn primary"
            style="padding:var(--sp-2) var(--sp-5);font-size:var(--fs-xs);flex:none"
            [disabled]="applying() || busy() || !selectedCount()"
            (click)="apply()"
          >{{ applying() ? 'Replacing…' : 'Replace ' + selectedCount() + ' in ' + selectedFileCount() + ' file' + (selectedFileCount() === 1 ? '' : 's') }}</button>
        </div>
      }

      <!-- status row -->
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair);flex:none;font-size:var(--fs-xs);color:var(--ink-3)">
        @if (error(); as err) {
          <span style="color:var(--st-blocked)">{{ err }}</span>
        } @else if (busy()) {
          <span style="display:flex;align-items:center;gap:var(--sp-3);color:var(--st-running)">
            <span class="dot running" style="background:var(--st-running)"></span>streaming results… {{ filesScanned() }} files
          </span>
        } @else {
          <span class="tnum">
            {{ q() ? total() + ' match' + (total() === 1 ? '' : 'es') + ' in ' + groups().length + ' file' + (groups().length === 1 ? '' : 's') + (truncated() ? ' · capped' : '') : 'type to search' }}
          </span>
        }
        <span class="chip" style="margin-left:auto;font-size:var(--fs-3xs)">grep · {{ scopeLabel() }}</span>
        @if (busy()) {
          <button class="btn ghost-hair" style="padding:var(--sp-1) var(--sp-4);font-size:var(--fs-2xs)" (click)="stop()">
            <app-icon name="stop" size="sm" [px]="11" />Stop
          </button>
        }
      </div>

      <!-- results -->
      <div #list class="scroll-y" style="flex:1;min-height:180px;padding:var(--sp-2) 0">
        @if (!q()) {
          <div style="padding:var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">results stream in as files are scanned — click a line to open it at that position</div>
        }
        @if (q() && !busy() && !groups().length && !error()) {
          <div style="padding:var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">no matches for "{{ q() }}"</div>
        }
        @for (g of groups(); track g.key) {
          <div style="margin-bottom:var(--sp-1)">
            <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-6);position:sticky;top:0;background:var(--panel);z-index:1;border-bottom:1px solid var(--hair)">
              @if (mode() === 'replace') {
                <input type="checkbox" [checked]="fileIncluded(g)" (click)="$event.stopPropagation(); toggleFile(g)" title="Include / exclude this file" style="accent-color:var(--ui-fill);flex:none" />
              }
              <app-icon name="file" size="sm" color="var(--ink-3)" />
              <span style="font-size:var(--fs-sm)">{{ fname(g.path) }}</span>
              <span style="font-size:var(--fs-2xs);color:var(--ink-4);flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fdir(g.path) }}</span>
              @if (g.root) {
                <span class="chip" style="font-size:var(--fs-3xs);padding:1px var(--sp-3);color:var(--ink-2)">{{ g.root }}</span>
              }
              <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)">{{ g.count }}</span>
            </div>
            @for (h of g.items; track h.line + ':' + $index) {
              @let i = flatIndex(g, h);
              <div
                [attr.data-idx]="i"
                (click)="open(i)"
                (mouseenter)="sel.set(i)"
                [style.background]="sel() === i ? 'var(--panel-3)' : 'transparent'"
                style="display:flex;align-items:flex-start;gap:var(--sp-4);padding:var(--sp-2) var(--sp-6) var(--sp-2) 26px;cursor:pointer;font-size:var(--fs-sm);line-height:1.55"
              >
                @if (mode() === 'replace') {
                  <input type="checkbox" [checked]="included(h)" (click)="$event.stopPropagation(); toggleHit(h)" style="accent-color:var(--ui-fill);flex:none;margin-top:var(--sp-1)" />
                }
                <span class="tnum" style="flex:none;width:34px;text-align:right;color:var(--ink-4);font-size:var(--fs-xs)">{{ h.line }}</span>
                <span style="flex:1;min-width:0;white-space:pre-wrap;word-break:break-word;color:var(--ink-2)">
                  @for (s of segs(h); track $index) {
                    @if (s.hit) {
                      <mark style="background:var(--ui-sel-2);color:var(--ink);border-radius:2px;padding:0 1px"
                        [style.text-decoration]="mode() === 'replace' ? 'line-through' : 'none'">{{ s.t }}</mark>
                    } @else { <span>{{ s.t }}</span> }
                  }
                  @if (mode() === 'replace' && h.preview !== undefined) {
                    <span style="color:var(--ink-4)"> → </span>
                    <span style="background:var(--code-add-bg);color:var(--code-add-ink);border-radius:2px;padding:0 var(--sp-1)">{{ previewTrim(h) }}</span>
                  }
                </span>
              </div>
            }
          </div>
        }
      </div>
      <app-overlay-footer [hints]="[['↑↓', 'navigate'], ['⏎', 'open at line'], ['esc', 'close']]" />
    </app-overlay-shell>
  `,
})
export class FindInFilesComponent {
  readonly registry = inject(CommandRegistryService);
  private bridge = inject(BRIDGE);
  private runtime = inject(AgentRuntimeService);
  private projects = inject(ProjectActionsService);
  private ui = inject(UiStore);

  readonly scopes = SCOPES;
  readonly fname = fileName;
  readonly fdir = fileDir;
  readonly segs = lineSegments;

  readonly q = signal("");
  readonly caseSensitive = signal(false);
  readonly word = signal(false);
  readonly regex = signal(false);
  readonly focusedInput = signal(false);
  readonly sel = signal(0);

  // ----- replace mode (B3.2) -----
  readonly mode = signal<"find" | "replace">("find");
  readonly replacement = signal("");
  readonly applying = signal(false);
  /** Match keys the user UNticked (default = everything included). */
  readonly excluded = signal<ReadonlySet<string>>(new Set());

  readonly hits = signal<SearchMatchEntry[]>([]);
  readonly filesScanned = signal(0);
  readonly total = computed(() => this.hits().reduce((a, h) => a + Math.max(1, h.ranges.length), 0));
  readonly busy = signal(false);
  readonly truncated = signal(false);
  readonly error = signal<string | null>(null);

  /** Agent scoping the search (the focused one, else the first). */
  readonly scopeAgent = computed(() => this.runtime.activeAgent() ?? this.runtime.agents()[0] ?? null);
  readonly scope = signal<Scope>("worktree");
  readonly scopeLabel = computed(() => SCOPES.find((s) => s.k === this.scope())?.label.toLowerCase() ?? "");

  private inp = viewChild.required<ElementRef<HTMLInputElement>>("inp");
  private list = viewChild<ElementRef<HTMLElement>>("list");
  private focusedOnce = false;

  private searchId: string | null = null;
  /** Batches that raced ahead of the invoke's searchId resolution. */
  private earlyBatches: SearchResultsPayload[] = [];
  private debounce: ReturnType<typeof setTimeout> | null = null;
  private unsubs: (() => void)[] = [];
  /** Agent the running search was started for (opens project-root matches). */
  private runAgentId: string | null = null;

  readonly groups = computed<Group[]>(() => {
    const map = new Map<string, Group>();
    for (const h of this.hits()) {
      const key = (h.root ? h.root + "/" : "") + h.path;
      let g = map.get(key);
      if (!g) {
        g = { key, path: h.path, root: h.root ?? null, agentId: h.agentId ?? null, items: [], count: 0 };
        map.set(key, g);
      }
      g.items.push(h);
      g.count += Math.max(1, h.ranges.length);
    }
    return [...map.values()];
  });

  /** Flat item list in render order (keyboard selection index space). */
  readonly flat = computed(() => this.groups().flatMap((g) => g.items));
  private readonly flatIdx = computed(() => new Map(this.flat().map((h, i) => [h, i])));

  flatIndex(_g: Group, h: SearchMatchEntry): number {
    return this.flatIdx().get(h) ?? -1;
  }

  // ----- replace selection (line granularity — one key per match row) -----

  private hitKey(h: SearchMatchEntry): string {
    return `${h.agentId ?? ""}|${h.path}:${h.line}`;
  }
  included(h: SearchMatchEntry): boolean {
    return !this.excluded().has(this.hitKey(h));
  }
  toggleHit(h: SearchMatchEntry): void {
    const key = this.hitKey(h);
    this.excluded.update((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }
  fileIncluded(g: Group): boolean {
    return g.items.some((h) => this.included(h));
  }
  toggleFile(g: Group): void {
    const anyIn = this.fileIncluded(g);
    this.excluded.update((s) => {
      const next = new Set(s);
      for (const h of g.items) {
        if (anyIn) next.add(this.hitKey(h));
        else next.delete(this.hitKey(h));
      }
      return next;
    });
  }
  readonly selectedCount = computed(() => this.flat().filter((h) => this.included(h)).length);
  readonly selectedFileCount = computed(
    () => this.groups().filter((g) => g.items.some((h) => this.included(h))).length,
  );

  /** Preview with leading whitespace trimmed like the match text. */
  previewTrim(h: SearchMatchEntry): string {
    return (h.preview ?? "").replace(/^\s+/, "");
  }

  /** Apply the replacement to every included match, then re-run the search. */
  async apply(): Promise<void> {
    if (this.applying()) return;
    const agent = this.scopeAgent();
    const projectId = agent?.projectId ?? this.projects.all()[0]?.id ?? null;
    // group included hits per (agent, path) with the file's scan stamp
    const byFile = new Map<
      string,
      { agentId: string | null; path: string; mtime: number; size: number; lines: number[] }
    >();
    for (const h of this.flat()) {
      if (!this.included(h) || h.mtime === undefined || h.size === undefined) continue;
      const key = `${h.agentId ?? ""}|${h.path}`;
      let f = byFile.get(key);
      if (!f) {
        f = { agentId: h.agentId ?? null, path: h.path, mtime: h.mtime, size: h.size, lines: [] };
        byFile.set(key, f);
      }
      f.lines.push(h.line);
    }
    if (!byFile.size) return;
    this.applying.set(true);
    try {
      const results = await this.bridge.invoke<FileReplaceResult[]>(Commands.SearchReplaceApply, {
        req: {
          query: this.q(),
          caseSensitive: this.caseSensitive(),
          wholeWord: this.word(),
          regex: this.regex(),
          replacement: this.replacement(),
          projectId,
          files: [...byFile.values()],
        },
      });
      const replaced = results.reduce((a, r) => a + r.replaced, 0);
      const stale = results.filter((r) => r.stale).length;
      const errors = results.filter((r) => r.error).length;
      let msg = `Replaced ${replaced} line${replaced === 1 ? "" : "s"} in ${results.filter((r) => r.replaced > 0).length} file(s)`;
      if (stale) msg += ` · ${stale} skipped (changed since scan)`;
      if (errors) msg += ` · ${errors} failed`;
      this.ui.flash(msg);
      this.excluded.set(new Set());
      void this.run(); // fresh scan reflects the new content
    } catch (e) {
      this.ui.flash("Replace failed: " + ((e as { message?: string })?.message ?? e));
    } finally {
      this.applying.set(false);
    }
  }

  constructor() {
    if (!this.scopeAgent()) this.scope.set("project");

    const destroy = inject(DestroyRef);
    void this.bridge
      .on<SearchResultsPayload>(Events.SearchResults, (p) => this.onResults(p))
      .then((u) => this.unsubs.push(u));
    void this.bridge
      .on<SearchDonePayload>(Events.SearchDone, (p) => this.onDone(p))
      .then((u) => this.unsubs.push(u));
    destroy.onDestroy(() => {
      this.unsubs.forEach((u) => u());
      if (this.debounce) clearTimeout(this.debounce);
      this.cancelCurrent();
    });

    afterRenderEffect(() => {
      if (!this.focusedOnce) {
        this.focusedOnce = true;
        this.inp().nativeElement.focus();
      }
    });
    effect(() => {
      const i = this.sel();
      this.list()?.nativeElement.querySelector<HTMLElement>(`[data-idx="${i}"]`)?.scrollIntoView({ block: "nearest" });
    });
    // debounced re-run whenever the query or any option changes
    effect(() => {
      this.q();
      this.caseSensitive();
      this.word();
      this.regex();
      this.scope();
      this.mode();
      this.replacement();
      if (this.debounce) clearTimeout(this.debounce);
      this.debounce = setTimeout(() => this.run(), 250);
    });
  }

  optStyle(on: boolean): string {
    return (
      "padding:var(--sp-1) var(--sp-3);font-size:var(--fs-xs);border-radius:4px;min-width:26px;justify-content:center;" +
      `border:1px solid ${on ? "var(--ui-line)" : "var(--hair)"};` +
      `color:${on ? "var(--ink)" : "var(--ink-3)"};` +
      `background:${on ? "var(--ui-sel)" : "transparent"}`
    );
  }

  toggle(k: "caseSensitive" | "word" | "regex") {
    this[k].update((v) => !v);
  }

  onInput(e: Event) {
    this.q.set((e.target as HTMLInputElement).value);
  }

  onScope(e: Event) {
    this.scope.set((e.target as HTMLSelectElement).value as Scope);
  }

  private cancelCurrent() {
    if (this.searchId) {
      void this.bridge.invoke(Commands.SearchCancel, { id: this.searchId }).catch(() => {});
      this.searchId = null;
    }
  }

  private async run() {
    this.cancelCurrent();
    this.hits.set([]);
    this.filesScanned.set(0);
    this.truncated.set(false);
    this.error.set(null);
    this.sel.set(0);
    this.excluded.set(new Set());
    this.earlyBatches = [];
    const q = this.q();
    if (!q) {
      this.busy.set(false);
      return;
    }
    const agent = this.scopeAgent();
    const projectId = agent?.projectId ?? this.projects.all()[0]?.id ?? null;
    const scope = this.scope();
    if (scope === "worktree" && !agent) {
      this.error.set("open an agent to search its worktree");
      return;
    }
    if (scope !== "worktree" && !projectId) {
      this.error.set("no project to search");
      return;
    }
    this.busy.set(true);
    this.runAgentId = agent?.id ?? null;
    try {
      const id = await this.bridge.invoke<string>(Commands.SearchStart, {
        req: {
          query: q,
          caseSensitive: this.caseSensitive(),
          wholeWord: this.word(),
          regex: this.regex(),
          scope,
          agentId: agent?.id ?? null,
          projectId,
          replacement: this.mode() === "replace" ? this.replacement() : null,
        },
      });
      this.searchId = id;
      // flush result batches that arrived before the id resolved
      const early = this.earlyBatches;
      this.earlyBatches = [];
      early.forEach((p) => this.onResults(p));
    } catch (e) {
      this.busy.set(false);
      this.error.set((e as { message?: string })?.message ?? "search failed");
    }
  }

  private onResults(p: SearchResultsPayload) {
    if (!this.searchId) {
      this.earlyBatches.push(p);
      return;
    }
    if (p.searchId !== this.searchId) return;
    this.hits.update((h) => [...h, ...p.items]);
    this.filesScanned.set(p.files);
  }

  private onDone(p: SearchDonePayload) {
    if (p.searchId !== this.searchId) return;
    this.busy.set(false);
    this.filesScanned.set(p.files);
    this.truncated.set(p.truncated);
    this.searchId = null;
  }

  stop() {
    this.cancelCurrent();
    this.busy.set(false);
  }

  onKeys(e: KeyboardEvent) {
    const n = this.flat().length;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      this.sel.update((s) => (n ? (s + 1) % n : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      this.sel.update((s) => (n ? (s - 1 + n) % n : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (n) this.open(this.sel());
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.registry.close();
    }
  }

  open(i: number) {
    const h = this.flat()[i];
    if (!h) return;
    // matches from an agent worktree open there; project-checkout matches open
    // in the scoping agent's worktree (same path) — the file view is agent-bound
    const agentId = h.agentId ?? this.runAgentId ?? this.scopeAgent()?.id ?? null;
    if (!agentId) {
      this.ui.flash("open an agent to view files");
      return;
    }
    this.registry.close();
    setTimeout(() => this.registry.openFileAt(agentId, h.path, h.line), 0);
  }
}
