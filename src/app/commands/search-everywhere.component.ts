import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  input,
  linkedSignal,
  signal,
  untracked,
  viewChild,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { LookupScopeStore } from "../shared/scope";
import { ScopeBarComponent } from "../shared/scope-bar.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { TicketsStore } from "../stores/tickets.store";
import { ToolWindowStore } from "../tool-window/tool-window.store";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName, fmtN, langTag } from "../utils";
import { AgentStatus } from "../models";
import { CommandRegistryService, WorkspaceFilesService } from "./command-registry.service";
import { fzMatch, kbdLabel } from "./fuzzy";
import { fzSegments, OverlayFooterComponent, OverlayShellComponent } from "./overlay-shell.component";
import { SymbolSearchService } from "./symbol-search.service";
import { IndexStatusStore } from "../symbols/index-status.store";
import { symbolKindIcon } from "../symbols/symbol-kinds";
import { NavProvidersService } from "../workspace/nav-providers.service";
import { KjBadgeComponent, KjButtonComponent } from "@kouji-ui/components";

/** One Search-Everywhere corpus row. "symbol" rows come from the tree-sitter
 *  index through `SymbolSearchService` (M2). */
interface SeItem {
  type: "file" | "symbol" | "agent" | "ticket" | "command" | "ref";
  key: string;
  /** Ranked/displayed primary text. */
  label: string;
  /** Extra searchable text (branch, task, group…). */
  text: string;
  sub: string;
  meta: string;
  icon: string;
  /** Owning project id — half of the grouping key for file results.
   *  null/absent = outside any project (or a non-file corpus). */
  projectId?: string | null;
  /** Owning agent (worktree). File results group per WORKTREE, so the same
   *  path shows once per worktree that contains it — deliberately no dedup. */
  agentId?: string | null;
  /** Symbol rows (design SymbolRow): enclosing scope + root label + path:line. */
  container?: string | null;
  root?: string | null;
  status?: AgentStatus;
  danger?: boolean;
  open: () => void;
}

/** Design commands.jsx: Actions first (and the default tab) — no "All". */
const TABS = [
  { k: "commands", label: "Actions" },
  { k: "files", label: "Files" },
  { k: "symbols", label: "Symbols" },
  { k: "agents", label: "Agents" },
  { k: "tickets", label: "Tickets" },
  { k: "git", label: "Git" },
] as const;
type TabKey = (typeof TABS)[number]["k"];
/** Tabs whose corpus is too large to be useful empty — blank until you type,
 *  and their results group by project (design SE_LAZY / SE_GROUPED). Symbols
 *  are lazy for a harder reason: their corpus is a backend query, one per
 *  keystroke. */
const LAZY: Partial<Record<TabKey, true>> = { files: true, symbols: true };

/** Worktrees indexed for the Files corpus in one open. One `search_files`
 *  invoke each (a full gitignore-filtered tree walk), so it stays capped even
 *  when the scope says "all". */
const MAX_FILE_ROOTS = 8;

/**
 * Search Everywhere (roadmap B2.1, double-Shift): one ranked fuzzy overlay
 * over files, symbols, agents, tickets, commands and git branches. Keyboard-only:
 * ↑↓ navigate, ⏎ opens, Tab cycles the type tabs, Esc closes.
 */
@Component({
  selector: "app-search-everywhere",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    StatusDotComponent,
    OverlayShellComponent,
    OverlayFooterComponent,
    ScopeBarComponent,
    KjButtonComponent,
    KjBadgeComponent,
  ],
  template: `
    <app-overlay-shell [width]="720" top="9vh" label="Search everywhere" (closed)="registry.close()">
      <div class="pane-head" style="gap:var(--sp-5);padding:var(--sp-6) var(--sp-7)">
        <app-icon name="search" color="var(--ui-ink)" />
        <input
          #inp
          [value]="q()"
          (input)="onInput($event)"
          (keydown)="onKeys($event)"
          placeholder="Search files, symbols, agents, tickets, actions, branches…"
          spellcheck="false"
          autocomplete="off"
          style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-md)"
        />
      </div>

      <!-- scope: WHICH project/worktree the file + symbol corpora read from.
           Shared root store, so the choice survives closing the overlay. -->
      <div class="pane-head" style="gap:var(--sp-4);padding:var(--sp-3) var(--sp-7)">
        <app-scope-bar [scope]="scope" [showKind]="true" style="flex:1;min-width:0" />
      </div>

      <!-- type tabs -->
      <div class="kj-tab-strip" style="flex:none">
        @for (t of tabs; track t.k) {
          @let on = tab() === t.k;
          <kj-button kjVariant="ghost" (click)="tab.set(t.k)" [style.--kj-button-fg]="on ? 'var(--ink)' : 'var(--ink-3)'">
            @if (on) {
              <span class="tab-ind"></span>
            }
            {{ t.label }}
            <!-- lazy tabs hide their count until a query exists (design SE_LAZY) -->
            @if (!lazy[t.k] || q()) { <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-4)">{{ countOf(t.k) }}</span> }
          </kj-button>
        }
      </div>

      <!-- status row: what is still running, and the way to stop it -->
      @if (statusText(); as st) {
        <div class="pane-head" style="gap:var(--sp-4);padding:var(--sp-2) var(--sp-7);color:var(--ink-3)">
          @if (symbols.error(); as err) {
            <span style="color:var(--st-blocked)">{{ err }}</span>
          } @else {
            <span style="display:flex;align-items:center;gap:var(--sp-3);color:var(--st-running)">
              <span class="dot running" style="background:var(--st-running)"></span>{{ st }}
            </span>
          }
          @if (busy()) {
            <kj-button kjVariant="outline" style="margin-left:auto" (click)="stop()">
              <app-icon size="md" name="stop" />Stop
            </kj-button>
          }
        </div>
      }

      <div #list class="scroll-y" style="flex:1;padding:var(--sp-2) 0;min-height:120px">
        @if (!items().length && emptyText()) {
          <div style="padding:var(--sp-8) var(--sp-7);font-size:var(--fs-meta);color:var(--ink-4)">
            {{ emptyText() }}
          </div>
        }
        @for (row of rows(); track row.r.it.key) {
          @if (row.head) {
            <!-- sticky project header (design: grouped file results) -->
            <div
              style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-7) var(--sp-2);position:sticky;top:0;z-index:1;background:var(--panel)"
              [style.border-top]="row.first ? 'none' : '1px solid var(--hair)'"
            >
              <app-icon size="md" [name]="row.head.icon" [color]="row.head.color" />
              <span class="up" style="color:var(--ink-3)">{{ row.head.name }}</span>
              @if (row.head.path) {
                <span class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4);flex:1">{{ row.head.path }}</span>
              }
              <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-4);margin-left:auto;flex:none">{{ row.head.count }}</span>
            </div>
          }
          @let i = row.i;
          @let r = row.r;
          @if (r.it.type === 'symbol') {
            <!-- design SymbolRow: kind glyph · name (matched chars lit) · container
                 · root label (worktree hits) · path:line mono, right -->
            <div
              class="sym-row"
              [class.on]="sel() === i"
              [attr.data-idx]="i"
              (click)="open(i)"
              (mouseenter)="sel.set(i)"
            >
              <app-icon [name]="r.it.icon" size="sm" />
              <span class="nm">
                @for (s of r.segs; track $index) {
                  @if (s.hit) { <b style="color:var(--ui-ink);font-weight:var(--fw-medium)">{{ s.t }}</b> } @else { <span>{{ s.t }}</span> }
                }
              </span>
              <span class="ct">{{ r.it.container || r.it.meta }}</span>
              @if (r.it.root) { <span class="root">{{ r.it.root }}</span> }
              <span class="pth">{{ r.it.sub }}</span>
            </div>
          } @else {
          <div
            [attr.data-idx]="i"
            (click)="open(i)"
            (mouseenter)="sel.set(i)"
            [style.background]="sel() === i ? 'var(--panel-3)' : 'transparent'"
            [style.border-left]="'2px solid ' + (sel() === i ? 'var(--ui-focus)' : 'transparent')"
            [style.color]="r.it.danger ? 'var(--st-blocked)' : 'inherit'"
            style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-3) var(--sp-7);cursor:pointer"
          >
            <app-icon [name]="r.it.icon" size="sm" [color]="sel() === i ? 'var(--ui-ink)' : 'var(--ink-3)'" />
            <span class="trunc" style="flex:none;max-width:52%">
              @for (s of r.segs; track $index) {
                @if (s.hit) { <b style="color:var(--ui-ink);font-weight:var(--fw-medium)">{{ s.t }}</b> } @else { <span>{{ s.t }}</span> }
              }
            </span>
            @if (r.it.sub) {
              <span class="trunc" style="color:var(--ink-4);flex:1">{{ r.it.sub }}</span>
            }
            @if (r.it.status) { <app-status-dot [status]="r.it.status!" /> }
            @if (r.it.meta) { <kj-badge style="--kj-badge-font-size:var(--fs-badge)">{{ r.it.meta }}</kj-badge> }
          </div>
          }
        }
      </div>
      <app-overlay-footer [hints]="[['↑↓', 'navigate'], ['⏎', 'open'], ['⇥', 'next tab'], ['esc', 'close']]" />
    </app-overlay-shell>
  `,
})
export class SearchEverywhereComponent {
  readonly registry = inject(CommandRegistryService);
  readonly scope = inject(LookupScopeStore);
  readonly symbols = inject(SymbolSearchService);
  readonly index = inject(IndexStatusStore);
  private nav = inject(NavProvidersService);
  private runtime = inject(AgentRuntimeService);
  private projects = inject(ProjectActionsService);
  private tickets = inject(TicketsStore);
  private files = inject(WorkspaceFilesService);
  private ui = inject(UiStore);
  private toolWindow = inject(ToolWindowStore);

  readonly initialTab = input<string>("commands");
  readonly tabs = TABS;
  /** Active tab — seeded from (and re-seeded by) the requested initial tab,
   *  while staying locally writable for the tab strip / Tab key. */
  readonly tab = linkedSignal<string, TabKey>({
    source: this.initialTab,
    computation: (t) => (TABS.some((x) => x.k === t) ? (t as TabKey) : "commands"),
  });
  readonly lazy = LAZY;
  readonly q = signal("");
  /** File corpora, one per worktree the scope resolves to. */
  private readonly fileList = signal<{ agentId: string; projectId: string; paths: string[] }[]>([]);
  /** Worktrees still being walked (0 = idle) — drives "indexing N worktrees…". */
  readonly filesBusy = signal(0);
  /** Bumped per corpus load so a Stop (or a scope change) abandons the walks
   *  already in flight instead of letting them land late. */
  private fileGen = 0;

  private inp = viewChild.required<ElementRef<HTMLInputElement>>("inp");
  private list = viewChild<ElementRef<HTMLElement>>("list");
  private focused = false;
  private symDebounce: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    afterRenderEffect(() => {
      if (!this.focused) {
        this.focused = true;
        this.inp().nativeElement.focus();
      }
    });
    effect(() => {
      const i = this.sel();
      this.list()?.nativeElement.querySelector<HTMLElement>(`[data-idx="${i}"]`)?.scrollIntoView({ block: "nearest" });
    });
    // The Files corpus FOLLOWS the scope bar: re-index whenever the scope (or
    // the worktree list behind it) changes, instead of blindly walking every
    // worktree in the app on open.
    effect(() => {
      // keyed on the ROOT IDS, not the array: `fileRoots()` rebuilds its array
      // on every unrelated agent-status tick, and re-walking 8 trees for that
      // would be a full re-index per heartbeat
      this.fileRootsKey();
      untracked(() => this.loadFiles(this.fileRoots()));
    });
    // Symbols are one index lookup per query — debounced just enough to
    // coalesce a burst of keystrokes, and only while their tab is visible.
    // The query also re-runs when the index FINISHES: a first lookup on a
    // never-indexed root answers from an empty index while the walk runs, so
    // the rows fill in without retyping once it is done.
    effect(() => {
      const active = this.tab() === "symbols";
      const q = this.q();
      const kind = this.scope.kind();
      const agentId = this.scope.realAgent()?.id ?? null;
      const projectId = this.scope.project()?.id ?? null;
      const indexing = this.anyIndexing();
      untracked(() => {
        if (this.symDebounce) clearTimeout(this.symDebounce);
        if (!active) {
          this.symbols.cancel();
          return;
        }
        if (indexing && !q.trim()) return; // nothing to (re)ask yet
        this.symDebounce = setTimeout(() => this.symbols.search(q, { kind, agentId, projectId }), 60);
      });
    });
    // Opening the Symbols tab warms the index of every root in scope: the
    // editor does that per opened file, but "Ctrl+T on a fresh project"
    // must not answer from nothing forever (idempotent on the backend).
    effect(() => {
      if (this.tab() !== "symbols") return;
      const ids = this.symbolRootIds();
      untracked(() => {
        for (const id of ids) this.nav.startIndex(id);
      });
    });
    inject(DestroyRef).onDestroy(() => {
      if (this.symDebounce) clearTimeout(this.symDebounce);
      this.fileGen++;
      this.symbols.cancel();
    });
  }

  // ------------------------------------------------------------ files corpus

  /** Worktrees the Files corpus should cover for the current scope.
   *  worktree = the scoped one; project = its project's worktrees; all = every
   *  worktree — each capped, one tree walk per root. */
  private readonly fileRoots = computed<{ id: string; projectId: string }[]>(() => {
    const kind = this.scope.kind();
    const picks: { id: string; projectId: string }[] = [];
    const seen = new Set<string>();
    // the "main checkout" pseudo-agent has the PROJECT's id and no worktree of
    // its own — `search_files` takes a real agent id, so it never goes in
    const push = (a: { id: string; projectId: string } | null | undefined) => {
      if (!a || a.id === a.projectId || seen.has(a.id)) return;
      seen.add(a.id);
      picks.push({ id: a.id, projectId: a.projectId });
    };
    push(this.scope.realAgent());
    if (kind === "project") for (const a of this.scope.projAgents()) push(a);
    else if (kind === "all") for (const a of this.runtime.agents()) push(a);
    // worktree scope sitting on the MAIN checkout has no worktree to walk —
    // fall back to the project's worktrees so Go to File is not simply blank
    if (!picks.length) for (const a of this.scope.projAgents()) push(a);
    return picks.slice(0, MAX_FILE_ROOTS);
  });

  /** Identity of the current root set — the effect's actual dependency. */
  private readonly fileRootsKey = computed(() => this.fileRoots().map((r) => r.id).join(","));

  /** A boolean edge, not the progress stream: the query effect must re-run
   *  when indexing starts or ends, not on every "2,341 / 10,020" tick. */
  private readonly anyIndexing = computed(() => this.index.indexing().length > 0);

  /** Roots whose symbol index the Symbols tab needs: the worktrees the Files
   *  corpus walks, plus the project checkout itself for a project / all
   *  scope (its pseudo-agent id IS the project id). */
  private readonly symbolRootIds = computed<string[]>(() => {
    const ids = this.fileRoots().map((r) => r.id);
    const kind = this.scope.kind();
    const project = this.scope.project()?.id;
    if (project && kind !== "worktree") ids.push(project);
    return Array.from(new Set(ids));
  });

  private loadFiles(roots: { id: string; projectId: string }[]): void {
    const gen = ++this.fileGen;
    this.fileList.set([]);
    this.filesBusy.set(roots.length);
    for (const p of roots) {
      void this.files.filesFor(p.id).then((paths) => {
        if (gen !== this.fileGen) return; // scope changed / stopped mid-walk
        this.fileList.update((cur) => [...cur, { agentId: p.id, projectId: p.projectId, paths }]);
        this.filesBusy.update((n) => Math.max(0, n - 1));
      });
    }
  }

  // ----------------------------------------------------------------- status

  readonly busy = computed(() => (this.tab() === "symbols" && this.symbols.busy()) || this.filesBusy() > 0);

  /** "indexing 2,341 / 10,020 files…" while any root's symbol index runs —
   *  the Symbols tab answers from a partial index meanwhile. */
  readonly indexingText = computed<string | null>(() => {
    const roots = this.index.indexing().length;
    if (!roots) return null;
    const total = this.index.total();
    if (total > 0) return `indexing ${fmtN(this.index.done())} / ${fmtN(total)} files…`;
    return `indexing ${roots} root${roots === 1 ? "" : "s"}…`;
  });

  readonly statusText = computed<string | null>(() => {
    if (this.symbols.error() && this.tab() === "symbols") return this.symbols.error();
    if (this.tab() === "symbols" && this.indexingText()) return this.indexingText();
    if (this.tab() === "symbols" && this.symbols.busy()) return "searching…";
    const n = this.filesBusy();
    if (n > 0) return `indexing ${n} worktree${n === 1 ? "" : "s"}…`;
    if (this.tab() === "symbols" && this.symbols.truncated()) return "symbols capped — narrow the query";
    return null;
  });

  readonly emptyText = computed(() => {
    const t = this.tab();
    if (!this.q()) {
      if (t === "symbols") return "start typing to search symbols";
      return LAZY[t] ? "start typing to search files" : "start typing to filter";
    }
    if (t === "symbols" && this.symbols.busy()) return "";
    if (t === "symbols" && this.indexingText()) return "indexing — results appear as files are parsed";
    return 'nothing matches "' + this.q() + '"';
  });

  /** Stop whatever the overlay is still fetching for the current scope. */
  stop(): void {
    this.symbols.cancel();
    this.fileGen++;
    this.filesBusy.set(0);
  }

  /** The full project record for a group header (icon, color, name, path). */
  projectHead(projectId: string | null | undefined) {
    return projectId ? this.projects.projectOf(projectId) : undefined;
  }

  private readonly corpus = computed<Record<Exclude<TabKey, "all">, SeItem[]>>(() => {
    const reg = this.registry;
    const files: SeItem[] = this.fileList().flatMap((fl) =>
      fl.paths.map((p) => ({
        type: "file" as const,
        key: "f:" + fl.agentId + ":" + p,
        label: fileName(p),
        text: p,
        sub: fileDir(p),
        meta: langTag(p),
        icon: "file",
        projectId: fl.projectId,
        agentId: fl.agentId,
        open: () => reg.openFileAt(fl.agentId, p),
      })),
    );
    const agents: SeItem[] = this.runtime.agents().map((a) => ({
      type: "agent",
      key: "a:" + a.id,
      label: a.name,
      text: a.name + " " + a.branch + " " + (a.task || ""),
      sub: a.branch,
      meta: a.tool,
      icon: "agent",
      status: a.status,
      open: () => this.ui.openAgent(a.id),
    }));
    const tickets: SeItem[] = this.tickets.all().map((t) => ({
      type: "ticket",
      key: "t:" + t.id,
      label: t.title,
      text: t.title + " " + t.tags.join(" "),
      sub: t.tags.join(" · ") || t.status,
      meta: t.status,
      icon: "archive",
      open: () => this.ui.openTicket(t.id),
    }));
    const commands: SeItem[] = reg.commands().map((c) => ({
      type: "command",
      key: "c:" + c.id,
      label: c.label,
      text: c.label + " " + c.group,
      sub: c.group,
      meta: c.kbd ? kbdLabel(c.kbd) : "",
      icon: c.icon,
      danger: c.danger,
      open: () => {
        if (c.enabled) c.run();
      },
    }));
    const refs: SeItem[] = [];
    for (const p of this.projects.all()) {
      for (const b of p.branches ?? []) {
        refs.push({
          type: "ref",
          key: "r:" + p.id + ":" + b,
          label: b,
          text: b + " " + p.name,
          sub: p.name,
          meta: "branch",
          icon: "branch",
          open: () => this.openGitTabFor(null),
        });
      }
    }
    for (const a of this.runtime.agents()) {
      refs.push({
        type: "ref",
        key: "r:" + a.id,
        label: a.branch,
        text: a.branch + " " + a.name,
        sub: a.name,
        meta: "agent branch",
        icon: "branch",
          open: () => this.openGitTabFor(a.id),
      });
    }
    // symbols are NOT part of the synchronous corpus — they come back from the
    // index query, so `items()` builds them on its own branch below
    return { files, symbols: [], agents, tickets, commands, git: refs };
  });

  countOf(k: string): number {
    // the symbol corpus lives in the search service, not in `corpus()`
    if (k === "symbols") return this.symbolItems().length;
    const c = this.corpus();
    return (c as Record<string, SeItem[]>)[k]?.length ?? 0;
  }

  /** Index hits, ranked + highlighted the same way every other tab is —
   *  scored against the identifier, never the path. */
  private readonly symbolItems = computed(() => {
    const q = this.q();
    const byAgent = new Map(this.runtime.agents().map((a) => [a.id, a.projectId]));
    const fallbackProject = this.scope.project()?.id ?? null;
    const fallbackAgent = this.scope.realAgent()?.id ?? null;
    const scored = this.symbols
      .hits()
      .map((h) => {
        const m = fzMatch(h.name, q);
        if (!m) return null;
        const agentId = h.agentId ?? fallbackAgent;
        const it: SeItem = {
          type: "symbol",
          key: "s:" + h.key,
          label: h.name,
          text: h.name + " " + h.path,
          sub: h.path + ":" + h.line,
          meta: h.kind,
          icon: symbolKindIcon(h.kind),
          projectId: h.agentId ? (byAgent.get(h.agentId) ?? null) : fallbackProject,
          agentId: h.agentId,
          container: h.container,
          root: h.agentId ? h.root : null,
          open: () => {
            if (!agentId) {
              this.ui.flash("open an agent to view files");
              return;
            }
            this.registry.openFileAt(agentId, h.path, h.line);
          },
        };
        return { it, score: m.score, segs: fzSegments(h.name, m.idx) };
      })
      .filter((x): x is NonNullable<typeof x> => !!x);
    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, 80);
  });

  readonly items = computed(() => {
    const t = this.tab();
    const q = this.q();
    // lazy tabs (files, symbols) stay BLANK until the first character (SE_LAZY)
    if (LAZY[t] && !q) return [];
    // symbols are async: their rows come from the index query, not the
    // synchronous corpus every other tab reads
    if (t === "symbols") return this.symbolItems();
    const pool: SeItem[] = this.corpus()[t];
    const scored = pool
      .map((it) => {
        const direct = fzMatch(it.label, q);
        const m = direct ?? fzMatch(it.text, q);
        if (!m) return null;
        return { it, score: m.score, segs: fzSegments(it.label, direct ? direct.idx : []) };
      })
      .filter((x): x is NonNullable<typeof x> => !!x);
    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, 80);
  });

  /** Keyboard selection index — snaps back to the top whenever the result
   *  list itself changes, while staying writable for arrow keys / hover. */
  readonly sel = linkedSignal({ source: this.items, computation: () => 0 });

  /** Header data for a worktree group (design: sticky icon·name·path·count).
   *  Name = project · agent, path = the worktree's branch — enough to tell two
   *  checkouts of the same project apart at a glance. */
  private headOf(projectId: string | null | undefined, agentId: string | null, count: number) {
    const p = this.projectHead(projectId);
    const a = agentId ? this.runtime.agents().find((x) => x.id === agentId) : undefined;
    const name = (p?.name ?? "Outside project") + (a ? " · " + a.name : "");
    return {
      icon: p?.icon || "folder",
      color: p?.color || "var(--ink-4)",
      name,
      path: a?.branch ?? p?.path ?? "",
      count,
    };
  }

  /** Display rows. Lazy tabs group PER WORKTREE (same path in two worktrees =
   *  two rows in two groups) — groups ordered by their BEST HIT (first
   *  appearance in the ranked list), rows keeping score order inside each
   *  group. Symbol rows carry projectId/agentId exactly like file rows, so the
   *  same grouping applies unchanged. Row indices stay aligned with `items()`
   *  so keyboard selection and scroll targeting stay untouched by grouping. */
  readonly rows = computed(() => {
    const its = this.items();
    type Row = { head: { icon: string; color: string; name: string; path: string; count: number } | null; first: boolean; r: (typeof its)[number]; i: number };
    if (!LAZY[this.tab()]) return its.map((r, i): Row => ({ head: null, first: i === 0, r, i }));
    const groups = new Map<
      string,
      { projectId: string | null; agentId: string | null; rows: { r: (typeof its)[number]; i: number }[] }
    >();
    its.forEach((r, i) => {
      const key = (r.it.projectId ?? "_other") + "|" + (r.it.agentId ?? "");
      if (!groups.has(key))
        groups.set(key, { projectId: r.it.projectId ?? null, agentId: r.it.agentId ?? null, rows: [] });
      groups.get(key)!.rows.push({ r, i });
    });
    const out: Row[] = [];
    let first = true;
    for (const g of groups.values()) {
      g.rows.forEach(({ r, i }, j) => {
        out.push({ head: j === 0 ? this.headOf(g.projectId, g.agentId, g.rows.length) : null, first: first && j === 0, r, i });
      });
      first = false;
    }
    return out;
  });

  private openGitTabFor(agentId: string | null): void {
    // v2: branches live in the bottom tool window — surface it for the agent
    if (agentId) this.ui.openAgent(agentId);
    this.toolWindow.open("branches");
  }

  onInput(e: Event) {
    this.q.set((e.target as HTMLInputElement).value);
  }

  onKeys(e: KeyboardEvent) {
    const n = this.items().length;
    if (e.key === "Tab") {
      e.preventDefault();
      const i = TABS.findIndex((t) => t.k === this.tab());
      const next = (i + (e.shiftKey ? TABS.length - 1 : 1)) % TABS.length;
      this.tab.set(TABS[next].k);
    } else if (e.key === "ArrowDown") {
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
    const r = this.items()[i];
    if (!r) return;
    this.registry.close();
    setTimeout(() => r.it.open(), 0);
  }
}
