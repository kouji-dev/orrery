import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  inject,
  input,
  signal,
  viewChild,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName, langTag } from "../utils";
import { AgentStatus } from "../models";
import { CommandRegistryService, WorkspaceFilesService } from "./command-registry.service";
import { fzMatch, kbdLabel } from "./fuzzy";
import { fzSegments, OverlayFooterComponent, OverlayShellComponent } from "./overlay-shell.component";

/** One Search-Everywhere corpus row. Symbols are deliberately OMITTED for now
 *  (no tree-sitter layer yet — roadmap B2.4/B2.5). */
interface SeItem {
  type: "file" | "agent" | "ticket" | "command" | "ref";
  key: string;
  /** Ranked/displayed primary text. */
  label: string;
  /** Extra searchable text (branch, task, group…). */
  text: string;
  sub: string;
  meta: string;
  icon: string;
  /** Owning project id — drives the per-project grouping of file results.
   *  null/absent = outside any project (or a non-file corpus). */
  projectId?: string | null;
  status?: AgentStatus;
  danger?: boolean;
  open: () => void;
}

/** Design commands.jsx: Actions first (and the default tab) — no "All". */
const TABS = [
  { k: "commands", label: "Actions" },
  { k: "files", label: "Files" },
  { k: "agents", label: "Agents" },
  { k: "tickets", label: "Tickets" },
  { k: "git", label: "Git" },
] as const;
type TabKey = (typeof TABS)[number]["k"];
/** Tabs whose corpus is too large to be useful empty — blank until you type,
 *  and their results group by project (design SE_LAZY / SE_GROUPED). */
const LAZY: Partial<Record<TabKey, true>> = { files: true };

/**
 * Search Everywhere (roadmap B2.1, double-Shift): one ranked fuzzy overlay
 * over files, agents, tickets, commands and git branches. Keyboard-only:
 * ↑↓ navigate, ⏎ opens, Tab cycles the type tabs, Esc closes.
 */
@Component({
  selector: "app-search-everywhere",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, OverlayShellComponent, OverlayFooterComponent],
  template: `
    <app-overlay-shell [width]="720" top="9vh" label="Search everywhere" (closed)="registry.close()">
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-6) var(--sp-7);border-bottom:1px solid var(--hair);flex:none">
        <app-icon name="search" color="var(--accent)" />
        <input
          #inp
          [value]="q()"
          (input)="onInput($event)"
          (keydown)="onKeys($event)"
          placeholder="Search files, agents, tickets, actions, branches…"
          spellcheck="false"
          autocomplete="off"
          style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-md)"
        />
      </div>

      <!-- type tabs -->
      <div style="display:flex;align-items:center;gap:var(--sp-1);padding:0 var(--sp-5);border-bottom:1px solid var(--hair);background:var(--panel);flex:none">
        @for (t of tabs; track t.k) {
          @let on = tab() === t.k;
          <button
            class="btn"
            (click)="tab.set(t.k)"
            [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'"
            style="padding:var(--sp-4) var(--sp-5);border-radius:0;position:relative;font-size:var(--fs-sm)"
          >
            @if (on) {
              <span style="position:absolute;left:8px;right:8px;bottom:0;height:var(--sp-1);background:linear-gradient(90deg,var(--accent),var(--accent-2))"></span>
            }
            {{ t.label }}
            <!-- lazy tabs hide their count until a query exists (design SE_LAZY) -->
            @if (!lazy[t.k] || q()) { <span class="tnum" style="font-size:var(--fs-3xs);color:var(--ink-4)">{{ countOf(t.k) }}</span> }
          </button>
        }
      </div>

      <div #list class="scroll-y" style="flex:1;padding:var(--sp-2) 0;min-height:120px">
        @if (!items().length) {
          <div style="padding:var(--sp-8) var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">
            {{ q() ? 'nothing matches "' + q() + '"' : lazy[tab()] ? 'start typing to search files' : 'start typing to filter' }}
          </div>
        }
        @for (row of rows(); track row.r.it.key) {
          @if (row.head) {
            <!-- sticky project header (design: grouped file results) -->
            <div
              style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-7) var(--sp-2);position:sticky;top:0;z-index:1;background:var(--panel)"
              [style.border-top]="row.first ? 'none' : '1px solid var(--hair)'"
            >
              <app-icon [name]="row.head.icon" size="sm" [px]="11" [color]="row.head.color" />
              <span class="up" style="font-size:var(--fs-3xs);letter-spacing:.07em;color:var(--ink-3)">{{ row.head.name }}</span>
              @if (row.head.path) {
                <span style="font-size:var(--fs-2xs);color:var(--ink-4);flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ row.head.path }}</span>
              }
              <span class="tnum" style="font-size:var(--fs-3xs);color:var(--ink-4);margin-left:auto;flex:none">{{ row.head.count }}</span>
            </div>
          }
          @let i = row.i;
          @let r = row.r;
          <div
            [attr.data-idx]="i"
            (click)="open(i)"
            (mouseenter)="sel.set(i)"
            [style.background]="sel() === i ? 'var(--panel-3)' : 'transparent'"
            [style.border-left]="'2px solid ' + (sel() === i ? 'var(--accent)' : 'transparent')"
            [style.color]="r.it.danger ? 'var(--st-blocked)' : 'inherit'"
            style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-3) var(--sp-7);cursor:pointer"
          >
            <app-icon [name]="r.it.icon" size="sm" [color]="sel() === i ? 'var(--accent)' : 'var(--ink-3)'" />
            <span style="font-size:var(--fs-ui);flex:none;max-width:52%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
              @for (s of r.segs; track $index) {
                @if (s.hit) { <b style="color:var(--accent-2);font-weight:600">{{ s.t }}</b> } @else { <span>{{ s.t }}</span> }
              }
            </span>
            @if (r.it.sub) {
              <span style="font-size:var(--fs-xs);color:var(--ink-4);flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ r.it.sub }}</span>
            }
            @if (r.it.status) { <app-status-dot [status]="r.it.status!" /> }
            @if (r.it.meta) { <span class="chip" style="font-size:var(--fs-3xs);flex:none;padding:1px var(--sp-3)">{{ r.it.meta }}</span> }
          </div>
        }
      </div>
      <app-overlay-footer [hints]="[['↑↓', 'navigate'], ['⏎', 'open'], ['⇥', 'next tab'], ['esc', 'close']]" />
    </app-overlay-shell>
  `,
})
export class SearchEverywhereComponent {
  readonly registry = inject(CommandRegistryService);
  private runtime = inject(AgentRuntimeService);
  private projects = inject(ProjectActionsService);
  private tickets = inject(TicketsStore);
  private files = inject(WorkspaceFilesService);
  private ui = inject(UiStore);

  readonly initialTab = input<string>("commands");
  readonly tabs = TABS;
  readonly tab = signal<TabKey>("commands");
  readonly lazy = LAZY;
  readonly q = signal("");
  readonly sel = signal(0);
  /** File corpora, one per project (its scope/first agent's worktree),
   *  fetched lazily on open. */
  private readonly fileList = signal<{ agentId: string; projectId: string; paths: string[] }[]>([]);

  private inp = viewChild.required<ElementRef<HTMLInputElement>>("inp");
  private list = viewChild<ElementRef<HTMLElement>>("list");
  private focused = false;

  constructor() {
    // honor the requested initial tab (e.g. "files" for Go to File)
    effect(() => {
      const t = this.initialTab();
      if (TABS.some((x) => x.k === t)) this.tab.set(t as TabKey);
    });
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
    effect(() => {
      this.items();
      this.sel.set(0);
    });
    // fetch one file corpus PER PROJECT (the scope agent's worktree for its
    // project, the first agent's for every other) so file results span — and
    // group by — projects. Why cap 5: one invoke per project on overlay open.
    const scope = this.runtime.activeAgent();
    const picks: { id: string; projectId: string }[] = [];
    const seen = new Set<string>();
    for (const a of [scope, ...this.runtime.agents()]) {
      if (!a || seen.has(a.projectId)) continue;
      seen.add(a.projectId);
      picks.push({ id: a.id, projectId: a.projectId });
    }
    for (const p of picks.slice(0, 5)) {
      void this.files
        .filesFor(p.id)
        .then((paths) =>
          this.fileList.update((cur) => [...cur, { agentId: p.id, projectId: p.projectId, paths }]),
        );
    }
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
    return { files, agents, tickets, commands, git: refs };
  });

  countOf(k: string): number {
    const c = this.corpus();
    return (c as Record<string, SeItem[]>)[k]?.length ?? 0;
  }

  readonly items = computed(() => {
    const t = this.tab();
    const q = this.q();
    // lazy tabs (files) stay BLANK until the first character (design SE_LAZY)
    if (LAZY[t] && !q) return [];
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

  /** Header data for a project group (design: sticky icon·name·path·count). */
  private headOf(projectId: string | null | undefined, count: number) {
    const p = this.projectHead(projectId);
    return p
      ? { icon: p.icon || "box", color: p.color || "var(--ink-4)", name: p.name, path: p.path, count }
      : { icon: "folder", color: "var(--ink-4)", name: "Outside project", path: "", count };
  }

  /** Display rows. Lazy tabs group by project — groups ordered by their BEST
   *  HIT (first appearance in the ranked list), rows keeping score order
   *  inside each group. Row indices stay aligned with `items()` so keyboard
   *  selection and scroll targeting stay untouched by grouping. */
  readonly rows = computed(() => {
    const its = this.items();
    type Row = { head: { icon: string; color: string; name: string; path: string; count: number } | null; first: boolean; r: (typeof its)[number]; i: number };
    if (!LAZY[this.tab()]) return its.map((r, i): Row => ({ head: null, first: i === 0, r, i }));
    const groups = new Map<string, { projectId: string | null; rows: { r: (typeof its)[number]; i: number }[] }>();
    its.forEach((r, i) => {
      const key = r.it.projectId ?? "_other";
      if (!groups.has(key)) groups.set(key, { projectId: r.it.projectId ?? null, rows: [] });
      groups.get(key)!.rows.push({ r, i });
    });
    const out: Row[] = [];
    let first = true;
    for (const g of groups.values()) {
      g.rows.forEach(({ r, i }, j) => {
        out.push({ head: j === 0 ? this.headOf(g.projectId, g.rows.length) : null, first: first && j === 0, r, i });
      });
      first = false;
    }
    return out;
  });

  private openGitTabFor(agentId: string | null): void {
    // branches live in the right panel's Git tab — surface it for the agent
    if (agentId) this.ui.openAgent(agentId);
    if (!this.ui.tweaks().rightPanel) this.ui.setTweak("rightPanel", true);
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
