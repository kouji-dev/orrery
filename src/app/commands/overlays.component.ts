import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
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
import { fzSegments, OverlayFooterComponent, OverlayShellComponent } from "./overlay-shell.component";
import { RecentFilesService } from "./recent-files.service";
import { FindInFilesComponent } from "./find-in-files.component";
import { SearchEverywhereComponent } from "./search-everywhere.component";

// ------------------------------------------------------------ command palette
@Component({
  selector: "app-command-palette",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, OverlayShellComponent, OverlayFooterComponent],
  template: `
    <app-overlay-shell [width]="600" label="Command palette" (closed)="registry.close()">
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-6) var(--sp-7);border-bottom:1px solid var(--hair);flex:none">
        <app-icon name="bolt" color="var(--accent)" />
        <input
          #inp
          [value]="q()"
          (input)="onInput($event)"
          (keydown)="onKeys($event)"
          placeholder="Type a command…"
          spellcheck="false"
          autocomplete="off"
          style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-md)"
        />
        <span class="chip" style="font-size:var(--fs-3xs)">{{ registry.commands().length }} commands</span>
      </div>
      <div #list class="scroll-y" style="flex:1;padding:var(--sp-3) 0">
        @if (!items().length) {
          <div style="padding:var(--sp-8) var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">no matching command</div>
        }
        @for (it of items(); track it.c.id; let i = $index) {
          @if (!q() && it.head) {
            <div class="up" style="font-size:var(--fs-3xs);color:var(--ink-4);padding:var(--sp-4) var(--sp-7) var(--sp-2)">{{ it.head }}</div>
          }
          <div
            [attr.data-idx]="i"
            (click)="pick(i)"
            (mouseenter)="sel.set(i)"
            [style.background]="sel() === i ? 'var(--panel-3)' : 'transparent'"
            [style.border-left]="'2px solid ' + (sel() === i ? 'var(--accent)' : 'transparent')"
            [style.color]="it.c.danger ? 'var(--st-blocked)' : 'inherit'"
            style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-3) var(--sp-7);cursor:pointer"
          >
            <app-icon [name]="it.c.icon" size="sm" [color]="sel() === i ? 'var(--accent)' : 'var(--ink-3)'" />
            <span [style.opacity]="it.c.enabled ? 1 : 0.42" style="flex:1;font-size:var(--fs-ui);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">
              @for (s of it.segs; track $index) {
                @if (s.hit) { <b style="color:var(--accent-2);font-weight:600">{{ s.t }}</b> } @else { <span>{{ s.t }}</span> }
              }
            </span>
            @if (q()) { <span style="font-size:var(--fs-2xs);color:var(--ink-4);flex:none">{{ it.c.group }}</span> }
            @if (it.c.kbd) { <span class="kbd" style="flex:none">{{ kbd(it.c.kbd) }}</span> }
          </div>
        }
      </div>
      <app-overlay-footer [hints]="[['↑↓', 'navigate'], ['⏎', 'run'], ['esc', 'close']]" />
    </app-overlay-shell>
  `,
})
export class CommandPaletteComponent {
  readonly registry = inject(CommandRegistryService);
  readonly q = signal("");
  readonly sel = signal(0);
  readonly kbd = kbdLabel;
  private inp = viewChild.required<ElementRef<HTMLInputElement>>("inp");
  private list = viewChild<ElementRef<HTMLElement>>("list");
  private focused = false;

  readonly items = computed(() => {
    const q = this.q();
    const scored = this.registry
      .commands()
      .map((c) => {
        const direct = fzMatch(c.label, q);
        const m = direct ?? (q ? fzMatch(c.group + " " + c.label, q) : { score: 0, idx: [] });
        if (!m) return null;
        return { c, score: m.score + (c.enabled ? 4 : 0), segs: fzSegments(c.label, direct ? direct.idx : []) };
      })
      .filter((x): x is NonNullable<typeof x> => !!x);
    if (q) scored.sort((a, b) => b.score - a.score);
    // group headers only for the unfiltered list
    let last: string | null = null;
    return scored.slice(0, 60).map((it) => {
      const head = !q && it.c.group !== last ? (last = it.c.group) : null;
      return { ...it, head };
    });
  });

  constructor() {
    afterRenderEffect(() => {
      if (!this.focused) {
        this.focused = true;
        this.inp().nativeElement.focus();
      }
    });
    effect(() => {
      const i = this.sel();
      const el = this.list()?.nativeElement.querySelector<HTMLElement>(`[data-idx="${i}"]`);
      el?.scrollIntoView({ block: "nearest" });
    });
    effect(() => {
      this.items();
      this.sel.set(0);
    });
  }

  onInput(e: Event) {
    this.q.set((e.target as HTMLInputElement).value);
  }

  onKeys(e: KeyboardEvent) {
    const n = this.items().length;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      this.sel.update((s) => (n ? (s + 1) % n : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      this.sel.update((s) => (n ? (s - 1 + n) % n : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (n) this.pick(this.sel());
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.registry.close();
    }
  }

  pick(i: number) {
    const it = this.items()[i];
    if (!it || !it.c.enabled) return;
    this.registry.close();
    setTimeout(() => it.c.run(), 0);
  }
}

// ------------------------------------------------------------- recent files
@Component({
  selector: "app-recent-files-overlay",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, OverlayShellComponent, OverlayFooterComponent],
  template: `
    <app-overlay-shell [width]="480" top="16vh" label="Recent files" (closed)="registry.close()">
      <div #wrap tabindex="0" (keydown)="onKeys($event)" style="outline:none;display:flex;flex-direction:column;min-height:0">
        <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-5) var(--sp-7);border-bottom:1px solid var(--hair)">
          <app-icon name="clock" size="sm" color="var(--accent)" />
          <span class="up" style="font-size:var(--fs-2xs);color:var(--ink-3)">Recent files</span>
          <span class="tnum" style="margin-left:auto;font-size:var(--fs-2xs);color:var(--ink-4)">{{ items().length }}</span>
        </div>
        <div #list class="scroll-y" style="padding:var(--sp-2) 0;max-height:50vh">
          @if (!items().length) {
            <div style="padding:var(--sp-7);font-size:var(--fs-sm);color:var(--ink-4)">no files opened yet</div>
          }
          @for (it of items(); track it.agentId + it.path; let i = $index) {
            <div
              [attr.data-idx]="i"
              (click)="pick(i)"
              (mouseenter)="sel.set(i)"
              [style.background]="sel() === i ? 'var(--panel-3)' : 'transparent'"
              [style.border-left]="'2px solid ' + (sel() === i ? 'var(--accent)' : 'transparent')"
              style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-3) var(--sp-7);cursor:pointer"
            >
              <app-icon name="file" size="sm" [color]="sel() === i ? 'var(--accent)' : 'var(--ink-3)'" />
              <span style="font-size:var(--fs-ui)">{{ fname(it.path) }}</span>
              <span style="font-size:var(--fs-xs);color:var(--ink-4);flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fdir(it.path) }}</span>
              @if (it.agentName) { <span class="chip" style="font-size:var(--fs-3xs);padding:1px var(--sp-3)">{{ it.agentName }}</span> }
            </div>
          }
        </div>
        <app-overlay-footer [hints]="[['↑↓', 'navigate'], ['⏎', 'open'], ['esc', 'close']]" />
      </div>
    </app-overlay-shell>
  `,
})
export class RecentFilesOverlayComponent {
  readonly registry = inject(CommandRegistryService);
  private recents = inject(RecentFilesService);
  private runtime = inject(AgentRuntimeService);
  readonly sel = signal(0);
  readonly fname = fileName;
  readonly fdir = fileDir;
  private wrap = viewChild.required<ElementRef<HTMLElement>>("wrap");
  private list = viewChild<ElementRef<HTMLElement>>("list");
  private focused = false;

  /** Entries whose agent still exists, newest first, tagged with the name. */
  readonly items = computed(() => {
    const agents = this.runtime.agents();
    return this.recents
      .entries()
      .map((e) => {
        const ag = agents.find((a) => a.id === e.agentId);
        return ag ? { ...e, agentName: ag.name } : null;
      })
      .filter((x): x is NonNullable<typeof x> => !!x);
  });

  constructor() {
    afterRenderEffect(() => {
      if (!this.focused) {
        this.focused = true;
        this.wrap().nativeElement.focus();
      }
    });
    effect(() => {
      const i = this.sel();
      this.list()?.nativeElement.querySelector<HTMLElement>(`[data-idx="${i}"]`)?.scrollIntoView({ block: "nearest" });
    });
  }

  onKeys(e: KeyboardEvent) {
    const n = this.items().length;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      this.sel.update((s) => (n ? (s + 1) % n : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      this.sel.update((s) => (n ? (s - 1 + n) % n : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (n) this.pick(this.sel());
    } else if (e.key === "Escape") {
      e.preventDefault();
      this.registry.close();
    }
  }

  pick(i: number) {
    const it = this.items()[i];
    if (!it) return;
    this.registry.close();
    setTimeout(() => this.registry.openFileAt(it.agentId, it.path), 0);
  }
}

// ---------------------------------------------------------------- go to line
@Component({
  selector: "app-goto-line-overlay",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, OverlayShellComponent],
  template: `
    <app-overlay-shell [width]="380" top="20vh" label="Go to line" (closed)="registry.close()">
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-6) var(--sp-7);border-bottom:1px solid var(--hair);flex:none">
        <app-icon name="enter" color="var(--accent)" />
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
        <span class="chip" style="font-size:var(--fs-3xs)">{{ fname(file()) }}</span>
      </div>
      <div style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-4) var(--sp-7);font-size:var(--fs-xs);color:var(--ink-4)">
        @if (parsed(); as p) {
          <span style="color:var(--accent-2)">→ line {{ p.line }}{{ p.col > 1 ? ', column ' + p.col : '' }}</span>
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
