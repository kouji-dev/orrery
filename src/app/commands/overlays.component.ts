import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { IconComponent } from "../shared/icon.component";
import { fileDir, fileName } from "../utils";
import { CommandRegistryService } from "./command-registry.service";
import { EditorNavService } from "./editor-nav.service";
import { fzMatch, kbdLabel } from "./fuzzy";
import { fzSegments, OverlayShellComponent } from "./overlay-shell.component";
import { RecentFilesService } from "./recent-files.service";
import { FindInFilesComponent } from "./find-in-files.component";
import { SearchEverywhereComponent } from "./search-everywhere.component";
import { KjBadgeComponent, KjCommandGroupComponent, KjCommandEmptyComponent,
  KjCommandItemComponent, KjCommandPaletteComponent, KjCommandPaletteFooter, KjKbdComponent } from "@kouji-ui/components";

// ------------------------------------------------------------ command palette
/**
 * The command palette on kouji's `<kj-command-palette>`.
 *
 * kj owns the chrome the hand-rolled OverlayShell only approximated: a real
 * `role="dialog"` + `role="listbox"` / `role="option"` tree, the APG combobox
 * 1.2 keyboard contract (arrows / Home / End / Enter) driven from the input
 * with `aria-activedescendant`, `aria-posinset` / `aria-setsize`, and focus on
 * open. Orrery keeps what kj cannot know about: `fzMatch` scoring (word-start
 * and consecutive-run bonuses, enabled-command bias) and the highlighted
 * segments. Hence `kjShouldFilter=false` — kj is told the visible set is
 * consumer-controlled and never runs its own substring filter.
 */
@Component({
  selector: "app-command-palette",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    KjKbdComponent,
    KjCommandPaletteComponent,
    KjCommandGroupComponent,
    KjCommandItemComponent,
    KjCommandPaletteFooter,
  ],
  template: `
    <kj-command-palette
      class="orr-palette"
      kjAriaLabel="Command palette"
      kjPlaceholder="Type a command…"
      [kjOpen]="true"
      (kjOpenChange)="registry.close()"
      [kjShouldFilter]="false"
      [(kjQuery)]="q"
      [kjEscBadge]="false"
      [kjAutoCloseOnActivate]="false"
      (kjValueChange)="active.set($event)"
      (kjActivate)="pick($event.value)"
    >
      @for (g of groups(); track g.label) {
        <kj-command-group [kjLabel]="g.label">
          @for (it of g.items; track it.c.id) {
            <kj-command-item
              [kjValue]="it.c.id"
              [kjDisabled]="!it.c.enabled"
              [kjShortcut]="it.c.kbd ?? null"
              [class.danger]="it.c.danger"
            >
              <app-icon [name]="it.c.icon" size="sm" color="var(--ink-3)" />
              <span class="orr-cmd-label orr-cmd-grow">
                @for (s of it.segs; track $index) {
                  @if (s.hit) { <b>{{ s.t }}</b> } @else { <span>{{ s.t }}</span> }
                }
              </span>
              @if (q()) { <span class="orr-cmd-meta">{{ it.c.group }}</span> }
              @if (it.c.kbd) { <kj-kbd>{{ kbd(it.c.kbd) }}</kj-kbd> }
            </kj-command-item>
          }
        </kj-command-group>
      }
      <div kjCommandPaletteFooter class="orr-palette-foot">
        <span><kj-kbd>↑↓</kj-kbd>navigate</span>
        <span><kj-kbd>⏎</kj-kbd>run</span>
        <span><kj-kbd>esc</kj-kbd>close</span>
        <span class="orr-palette-count tnum">{{ registry.commands().length }} commands</span>
      </div>
    </kj-command-palette>
  `,
})
export class CommandPaletteComponent {
  readonly registry = inject(CommandRegistryService);
  readonly q = signal("");
  /** Mirror of kj's active (highlighted) value — only used to drive scrolling. */
  readonly active = signal<unknown>(null);
  readonly kbd = kbdLabel;
  private host: ElementRef<HTMLElement> = inject(ElementRef);

  /** Scored + highlighted commands, best first. Unchanged Orrery matching. */
  private readonly scored = computed(() => {
    const q = this.q();
    const rows = this.registry
      .commands()
      .map((c) => {
        const direct = fzMatch(c.label, q);
        const m = direct ?? (q ? fzMatch(c.group + " " + c.label, q) : { score: 0, idx: [] });
        if (!m) return null;
        return { c, score: m.score + (c.enabled ? 4 : 0), segs: fzSegments(c.label, direct ? direct.idx : []) };
      })
      .filter((x): x is NonNullable<typeof x> => !!x);
    if (q) rows.sort((a, b) => b.score - a.score);
    return rows.slice(0, 60);
  });

  /**
   * Section runs for `<kj-command-group>`. Unfiltered the registry is already
   * ordered by group, so consecutive runs give real labelled sections; once a
   * query re-sorts by score the labels would be noise, so it collapses to one
   * unlabelled group (kj hides an empty `kjLabel`).
   */
  readonly groups = computed(() => {
    const rows = this.scored();
    if (this.q()) return [{ label: "", items: rows }];
    const out: { label: string; items: typeof rows }[] = [];
    for (const it of rows) {
      const last = out[out.length - 1];
      if (last && last.label === it.c.group) last.items.push(it);
      else out.push({ label: it.c.group, items: [it] });
    }
    return out;
  });

  constructor() {
    // kj's list navigator moves `data-active` but never scrolls; keep the
    // highlighted row in view the way the hand-rolled list did.
    afterRenderEffect(() => {
      this.active();
      this.host.nativeElement
        .querySelector<HTMLElement>(".kj-command-item[data-active]")
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  pick(value: unknown) {
    const c = this.registry.commands().find((x) => x.id === value);
    if (!c || !c.enabled) return;
    this.registry.close();
    setTimeout(() => c.run(), 0);
  }
}

// ------------------------------------------------------------- recent files
/**
 * Recent files on the same kj palette. The list is short and already ranked by
 * recency, so the query only narrows it — same `fzMatch` scoring as the command
 * palette, matched against the file name first and the full path as a fallback.
 */
@Component({
  selector: "app-recent-files-overlay",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjKbdComponent, KjCommandPaletteComponent, KjCommandItemComponent, KjCommandEmptyComponent, KjCommandPaletteFooter, KjBadgeComponent],
  template: `
    <kj-command-palette
      class="orr-palette orr-palette-narrow"
      kjAriaLabel="Recent files"
      kjPlaceholder="Recent files…"
      [kjOpen]="true"
      (kjOpenChange)="registry.close()"
      [kjShouldFilter]="false"
      [(kjQuery)]="q"
      [kjEscBadge]="false"
      [kjAutoCloseOnActivate]="false"
      (kjValueChange)="active.set($event)"
      (kjActivate)="pick($event.value)"
    >
      @if (!items().length) {
        <!-- the palette owns the empty slot; the hand-rolled div this replaced
             carried the copy the e2e asserts on -->
        <kj-command-empty>no files opened yet</kj-command-empty>
      }
      @for (it of items(); track it.key) {
        <kj-command-item [kjValue]="it.key">
          <app-icon name="file" size="sm" color="var(--ink-3)" />
          <span class="orr-cmd-label">
            @for (s of it.segs; track $index) {
              @if (s.hit) { <b>{{ s.t }}</b> } @else { <span>{{ s.t }}</span> }
            }
          </span>
          <span class="orr-cmd-meta orr-cmd-grow">{{ fdir(it.path) }}</span>
          @if (it.agentName) { <kj-badge class="orr-cmd-chip">{{ it.agentName }}</kj-badge> }
        </kj-command-item>
      }
      <div kjCommandPaletteFooter class="orr-palette-foot">
        <span><kj-kbd>↑↓</kj-kbd>navigate</span>
        <span><kj-kbd>⏎</kj-kbd>open</span>
        <span><kj-kbd>esc</kj-kbd>close</span>
        <span class="orr-palette-count tnum">{{ items().length }}</span>
      </div>
    </kj-command-palette>
  `,
})
export class RecentFilesOverlayComponent {
  readonly registry = inject(CommandRegistryService);
  private recents = inject(RecentFilesService);
  private runtime = inject(AgentRuntimeService);
  readonly q = signal("");
  readonly active = signal<unknown>(null);
  readonly fdir = fileDir;
  private host: ElementRef<HTMLElement> = inject(ElementRef);

  /** Entries whose agent still exists, tagged with the name and fuzzy-scored. */
  readonly items = computed(() => {
    const q = this.q();
    const agents = this.runtime.agents();
    const rows = this.recents
      .entries()
      .map((e) => {
        const ag = agents.find((a) => a.id === e.agentId);
        if (!ag) return null;
        const name = fileName(e.path);
        const direct = fzMatch(name, q);
        const m = direct ?? (q ? fzMatch(e.path, q) : { score: 0, idx: [] });
        if (!m) return null;
        return {
          ...e,
          agentName: ag.name,
          key: e.agentId + " " + e.path,
          score: m.score,
          segs: fzSegments(name, direct ? direct.idx : []),
        };
      })
      .filter((x): x is NonNullable<typeof x> => !!x);
    if (q) rows.sort((a, b) => b.score - a.score);
    return rows;
  });

  constructor() {
    afterRenderEffect(() => {
      this.active();
      this.host.nativeElement
        .querySelector<HTMLElement>(".kj-command-item[data-active]")
        ?.scrollIntoView({ block: "nearest" });
    });
  }

  pick(value: unknown) {
    const it = this.items().find((x) => x.key === value);
    if (!it) return;
    this.registry.close();
    setTimeout(() => this.registry.openFileAt(it.agentId, it.path), 0);
  }
}

// ---------------------------------------------------------------- go to line
/**
 * Left on the hand-rolled OverlayShell on purpose: this overlay has no result
 * list at all (one input plus a parsed-target hint), and `<kj-command-palette>`
 * is a combobox over a listbox — it would render a permanent "No results
 * found." under the field and offer nothing in return.
 */
@Component({
  selector: "app-goto-line-overlay",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, OverlayShellComponent, KjBadgeComponent],
  template: `
    <app-overlay-shell [width]="380" top="20vh" label="Go to line" (closed)="registry.close()">
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-6) var(--sp-7);border-bottom:1px solid var(--hair);flex:none">
        <app-icon name="enter" color="var(--ui-ink)" />
        <input
          #inp
          [value]="v()"
          (input)="onInput($event)"
          (keydown)="onKeys($event)"
          placeholder="[Line] or [Line:Column]"
          spellcheck="false"
          autocomplete="off"
          style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-md)"
        />
        <kj-badge style="font-size:var(--fs-badge)">{{ fname(file()) }}</kj-badge>
      </div>
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-4) var(--sp-7);color:var(--ink-4)">
        @if (parsed(); as p) {
          <span style="color:var(--ink-2)">→ line {{ p.line }}{{ p.col > 1 ? ', column ' + p.col : '' }}</span>
        } @else {
          <span>enter a line number</span>
        }
      </div>
    </app-overlay-shell>
  `,
})
export class GotoLineOverlayComponent {
  readonly registry = inject(CommandRegistryService);
  private editorNav = inject(EditorNavService);
  readonly v = signal("");
  readonly fname = fileName;
  private inp = viewChild.required<ElementRef<HTMLInputElement>>("inp");
  private focused = false;

  readonly file = computed(() => this.registry.activeFileLeaf()?.activeFile ?? "");

  readonly parsed = computed(() => {
    const m = this.v().match(/^(\d+)(?::(\d+))?$/);
    return m ? { line: Math.max(1, +m[1]), col: m[2] ? Math.max(1, +m[2]) : 1 } : null;
  });

  constructor() {
    afterRenderEffect(() => {
      if (!this.focused) {
        this.focused = true;
        this.inp().nativeElement.focus();
      }
    });
  }

  onInput(e: Event) {
    this.v.set((e.target as HTMLInputElement).value.replace(/[^\d:]/g, ""));
  }

  onKeys(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      const p = this.parsed();
      const leaf = this.registry.activeFileLeaf();
      if (!p || !leaf?.agentId || !leaf.activeFile) return;
      this.registry.close();
      this.editorNav.goTo(leaf.agentId, leaf.activeFile, p.line, p.col);
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.registry.close();
    }
  }
}

// -------------------------------------------------------------- overlay host
/** Renders whichever overlay the registry says is open. Sits once in the shell. */
@Component({
  selector: "app-command-overlays",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    CommandPaletteComponent,
    RecentFilesOverlayComponent,
    GotoLineOverlayComponent,
    SearchEverywhereComponent,
    FindInFilesComponent,
  ],
  template: `
    @switch (registry.overlay()?.kind) {
      @case ('palette') { <app-command-palette /> }
      @case ('search') { <app-search-everywhere [initialTab]="registry.overlay()?.tab ?? 'all'" /> }
      @case ('recent') { <app-recent-files-overlay /> }
      @case ('goto') { <app-goto-line-overlay /> }
      @case ('find') { <app-find-in-files /> }
    }
  `,
})
export class CommandOverlaysComponent {
  readonly registry = inject(CommandRegistryService);
}
