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
import {
  BRIDGE,
  Commands,
  Events,
  FileReplaceResult,
  SearchDonePayload,
  SearchMatchEntry,
  SearchResultsPayload,
} from "../data-source/bridge";
import { IconComponent } from "../shared/icon.component";
import { LookupScopeStore } from "../shared/scope";
import { ScopeBarComponent } from "../shared/scope-bar.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName } from "../utils";
import { CommandRegistryService } from "./command-registry.service";
import { OverlayFooterComponent, OverlayShellComponent } from "./overlay-shell.component";
import { KjBadgeComponent, KjButtonComponent, KjCheckboxComponent, KjInputComponent, KjTabComponent, KjTabListComponent, KjTabsComponent} from "@kouji-ui/components";

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
  imports: [IconComponent, OverlayShellComponent, OverlayFooterComponent, KjButtonComponent, KjBadgeComponent, KjCheckboxComponent, KjInputComponent, ScopeBarComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
  template: `
    <app-overlay-shell [width]="860" top="9vh" label="Find in files" (closed)="registry.close()">
      <!-- query row -->
      <div class="pane-head">
        <kj-tabs variant="pills" class="tabs-xs" style="flex:none"
                 [value]="mode()" (valueChange)="mode.set($any($event))">
          <kj-tab-list aria-label="Find or replace">
            <kj-tab value="find">Find</kj-tab>
            <kj-tab value="replace" title="Replace in Files">Replace</kj-tab>
          </kj-tab-list>
        </kj-tabs>
        <kj-input
          #inp
          [value]="q()"
          (input)="onInput($event)"
          (keydown)="onKeys($event)"
          placeholder="Search in files…"
          autocomplete="off"
          style="flex:1;min-width:0;--kj-input-bg:var(--panel-2);--kj-input-font:var(--font-mono);--kj-input-font-size:var(--fs-ui)"
        />
        <div style="display:flex;gap:var(--sp-2);flex:none">
          <kj-button kjVariant="outline" [kjPressed]="caseSensitive()" (click)="toggle('caseSensitive')" title="Match case">Aa</kj-button>
          <kj-button kjVariant="outline" [kjPressed]="word()" (click)="toggle('word')" title="Words">W</kj-button>
          <kj-button kjVariant="outline" [kjPressed]="regex()" (click)="toggle('regex')" title="Regular expression">.*</kj-button>
        </div>
        <app-scope-bar [scope]="scope" [showKind]="true" />
      </div>

      <!-- replacement row (replace mode) -->
      @if (mode() === 'replace') {
        <div class="pane-head" style="padding:var(--sp-3) var(--sp-6)">
          <span style="flex:none;color:var(--ink-3);width: round(calc(64px * var(--density)), 1px);text-align:right">replace →</span>
          <kj-input
            kjSize="sm"
            [value]="replacement()"
            (input)="replacement.set($any($event.target).value)"
            placeholder="Replacement ($1 for capture groups in regex mode)…"
            autocomplete="off"
            style="flex:1;min-width:0;--kj-input-bg:var(--panel-2);--kj-input-font:var(--font-mono);--kj-input-font-size:var(--fs-body)"
          />
          <kj-button kjVariant="default" [kjDisabled]="applying() || busy() || !selectedCount()" (click)="apply()">{{ applying() ? 'Replacing…' : 'Replace ' + selectedCount() + ' in ' + selectedFileCount() + ' file' + (selectedFileCount() === 1 ? '' : 's') }}</kj-button>
        </div>
      }

      <!-- status row -->
      <div class="pane-head" style="gap:var(--sp-5);padding:var(--sp-3) var(--sp-6);color:var(--ink-3)">
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
        <kj-badge style="margin-left:auto;font-size:var(--fs-badge)">grep · {{ scope.label() }}</kj-badge>
        @if (busy()) {
          <kj-button kjVariant="outline" (click)="stop()">
            <app-icon size="md" name="stop" />Stop
          </kj-button>
        }
      </div>

      <!-- results -->
      <div #list class="scroll-y" style="flex:1;min-height:180px;padding:var(--sp-2) 0">
        @if (!q()) {
          <div style="padding:var(--sp-7);font-size:var(--fs-meta);color:var(--ink-4)">results stream in as files are scanned — click a line to open it at that position</div>
        }
        @if (q() && !busy() && !groups().length && !error()) {
          <div style="padding:var(--sp-7);font-size:var(--fs-meta);color:var(--ink-4)">no matches for "{{ q() }}"</div>
        }
        @for (g of groups(); track g.key) {
          <div style="margin-bottom:var(--sp-1)">
            <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-6);position:sticky;top:0;background:var(--panel);z-index:1;border-bottom:1px solid var(--hair)">
              @if (mode() === 'replace') {
                <kj-checkbox size="sm" [checked]="fileIncluded(g)" (checkedChange)="toggleFile(g)" (click)="$event.stopPropagation()" title="Include / exclude this file" style="flex:none" />
              }
              <app-icon name="file" size="sm" color="var(--ink-3)" />
              <span>{{ fname(g.path) }}</span>
              <span class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4);flex:1">{{ fdir(g.path) }}</span>
              @if (g.root) {
                <kj-badge style="font-size:var(--fs-badge);padding:1px var(--sp-3);color:var(--ink-2)">{{ g.root }}</kj-badge>
              }
              <span class="tnum" style="font-size:var(--fs-meta);color:var(--ink-4)">{{ g.count }}</span>
            </div>
            @for (h of g.items; track h.line + ':' + $index) {
              @let i = flatIndex(g, h);
              <div
                [attr.data-idx]="i"
                (click)="open(i)"
                (mouseenter)="sel.set(i)"
                [style.background]="sel() === i ? 'var(--panel-3)' : 'transparent'"
                style="display:flex;align-items:flex-start;gap:var(--sp-4);padding:var(--sp-2) var(--sp-6) var(--sp-2) 26px;cursor:pointer;line-height:1.55"
              >
                @if (mode() === 'replace') {
                  <kj-checkbox size="sm" [checked]="included(h)" (checkedChange)="toggleHit(h)" (click)="$event.stopPropagation()" style="flex:none;margin-top:var(--sp-1)" />
                }
                <span class="tnum" style="flex:none;width:34px;text-align:right;color:var(--ink-4)">{{ h.line }}</span>
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
  private ui = inject(UiStore);

  readonly fname = fileName;
  readonly fdir = fileDir;
  readonly segs = lineSegments;

  readonly q = signal("");
  readonly caseSensitive = signal(false);
  readonly word = signal(false);
  readonly regex = signal(false);
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

  /** Where the grep runs. SHARED with the other lookup overlays on purpose:
   *  re-pointing the scope in Ctrl+E must survive reopening Ctrl+Shift+F. */
  readonly scope = inject(LookupScopeStore);

  /** The real agent behind the scope — the worktree select leads with the
   *  project's MAIN checkout pseudo-agent, which has no worktree of its own, so
   *  a "this worktree" search over it is really a project-checkout search. */
  private readonly searchScope = computed<{ kind: string; agentId: string | null; projectId: string | null }>(
    () => {
      const real = this.scope.realAgent();
      const kind = this.scope.kind();
      return {
        kind: kind === "worktree" && !real ? "project" : kind,
        agentId: real?.id ?? null,
        projectId: this.scope.project()?.id ?? null,
      };
    },
  );

  private inp = viewChild.required<KjInputComponent>("inp");
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
    const projectId = this.searchScope().projectId;
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
        this.inp().focus();
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
      // read every scope FIELD, not the store: `scope` is an injected object,
      // so touching it tracks nothing and the search stops re-running when the
      // user re-points it.
      this.scope.kind();
      this.scope.agent()?.id;
      this.scope.project()?.id;
      this.mode();
      this.replacement();
      if (this.debounce) clearTimeout(this.debounce);
      this.debounce = setTimeout(() => this.run(), 250);
    });
  }


  toggle(k: "caseSensitive" | "word" | "regex") {
    this[k].update((v) => !v);
  }

  onInput(e: Event) {
    this.q.set((e.target as HTMLInputElement).value);
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
    const { kind: scope, agentId, projectId } = this.searchScope();
    if (scope === "worktree" && !agentId) {
      this.error.set("open an agent to search its worktree");
      return;
    }
    if (scope !== "worktree" && !projectId) {
      this.error.set("no project to search");
      return;
    }
    this.busy.set(true);
    this.runAgentId = agentId;
    try {
      const id = await this.bridge.invoke<string>(Commands.SearchStart, {
        req: {
          query: q,
          caseSensitive: this.caseSensitive(),
          wholeWord: this.word(),
          regex: this.regex(),
          scope,
          agentId,
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
    const agentId = h.agentId ?? this.runAgentId ?? this.scope.realAgent()?.id ?? null;
    if (!agentId) {
      this.ui.flash("open an agent to view files");
      return;
    }
    this.registry.close();
    setTimeout(() => this.registry.openFileAt(agentId, h.path, h.line), 0);
  }
}
