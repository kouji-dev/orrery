import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  type OnDestroy,
  type AfterViewInit,
  effect,
  input,
  output,
  signal,
  ViewEncapsulation,
  viewChild,
} from "@angular/core";
import {
  INSERT_ORDERED_LIST_COMMAND,
  INSERT_UNORDERED_LIST_COMMAND,
  registerList,
} from "@lexical/list";
import { $createCodeNode } from "@lexical/code";
import { createEmptyHistoryState, registerHistory } from "@lexical/history";
import { $createQuoteNode, registerRichText } from "@lexical/rich-text";
import { $setBlocksType } from "@lexical/selection";
import {
  $getSelection,
  $isRangeSelection,
  COMMAND_PRIORITY_NORMAL,
  KEY_MODIFIER_COMMAND,
  type LexicalEditor,
  type TextFormatType,
} from "lexical";
import { IconComponent } from "../icon.component";
import {
  applyLinkToSelection,
  applyTextFormat,
  type BlockState,
  buildRichEditor,
  EMPTY_BLOCK_STATE,
  loadHtmlIntoEditor,
  type ParagraphStyle,
  readBlockState,
  serializeEditorToHtml,
  setParagraphStyle,
  snapshotSelection,
} from "./rich-editor.lexical";

/**
 * Lexical-based rich text editor (vanilla core, no React bindings).
 *
 * Value flows as HTML: `[value]` HTML in, `(valueChange)` HTML out. A feedback
 * guard prevents the editor reloading its own emitted HTML.
 *
 *   <app-rich-editor [value]="notes" (valueChange)="notes = $event"
 *     placeholder="Write something…" [compact]="true" [minHeight]="80"
 *     [resetSignal]="submitCount" />
 */
@Component({
  selector: "app-rich-editor",
  changeDetection: ChangeDetectionStrategy.OnPush,
  // Lexical builds the content DOM at runtime (no Angular scoping attribute), so
  // emulated encapsulation would never style the .rte-* nodes it creates. None
  // makes these rules global — safe because every selector is rte-/.rte-content
  // scoped.
  encapsulation: ViewEncapsulation.None,
  imports: [IconComponent],
  template: `
    <div class="rte" [class.compact]="compact()">
      <div class="rte-toolbar">
        <div class="rte-block-wrap">
          <button type="button" class="rte-btn rte-block" title="Paragraph style"
            [class.on]="blockMenuOpen()" (mousedown)="toggleBlockMenu($event)">
            <span class="cv">{{ blockLabel() }}</span><span class="chev"></span>
          </button>
          @if (blockMenuOpen()) {
            <div class="rte-block-pop" (mousedown)="$event.stopPropagation()">
              @for (opt of blockOptions; track opt.value) {
                <button type="button" class="rte-block-opt"
                  [class.on]="state().block === opt.value"
                  (mousedown)="setBlock($event, opt.value)">
                  <span [class]="'pv ' + opt.value">{{ opt.label }}</span>
                </button>
              }
            </div>
          }
        </div>
        <span class="rte-div"></span>
        <button type="button" class="rte-btn" title="Bold (⌘B)" [class.on]="state().bold"
          (mousedown)="fmt($event, 'bold')"><span class="bold">B</span></button>
        <button type="button" class="rte-btn" title="Italic (⌘I)" [class.on]="state().italic"
          (mousedown)="fmt($event, 'italic')"><span class="ital">I</span></button>
        <button type="button" class="rte-btn" title="Strikethrough" [class.on]="state().strikethrough"
          (mousedown)="fmt($event, 'strikethrough')"><span class="strike">S</span></button>
        <button type="button" class="rte-btn" title="Inline code" [class.on]="state().code"
          (mousedown)="fmt($event, 'code')"><span class="mono">&lt;/&gt;</span></button>
        <span class="rte-div"></span>
        <button type="button" class="rte-btn" title="Bulleted list" [class.on]="state().block === 'ul'"
          (mousedown)="list($event, 'ul')"><app-icon name="dots" size="sm" /></button>
        <button type="button" class="rte-btn" title="Numbered list" [class.on]="state().block === 'ol'"
          (mousedown)="list($event, 'ol')"><span class="num">1.</span></button>
        <button type="button" class="rte-btn" title="Quote" [class.on]="state().block === 'quote'"
          (mousedown)="quote($event)"><span class="quo">"</span></button>
        <button type="button" class="rte-btn" title="Code block" [class.on]="state().block === 'code'"
          (mousedown)="codeblock($event)"><app-icon name="terminal" size="sm" /></button>
        <span class="rte-div"></span>
        <div class="rte-link-wrap">
          <button type="button" class="rte-btn" [class.on]="linkOpen()" title="Link (⌘K)"
            (mousedown)="openLink($event)"><app-icon name="link" size="sm" /></button>
          @if (linkOpen()) {
            <div class="rte-link-pop" (mousedown)="$event.stopPropagation()">
              <input #linkInput class="rte-link-input" [value]="linkUrl()"
                (input)="onLinkInput($event)"
                placeholder="https://…"
                (keydown.enter)="applyLink(); $event.preventDefault()"
                (keydown.escape)="closeLink()" />
              <button type="button" class="rte-link-add"
                (mousedown)="$event.preventDefault(); applyLink()">Add</button>
            </div>
          }
        </div>
      </div>
      <div #content class="rte-content scroll-y" [class.compact]="compact()"
        contenteditable="true" spellcheck="true" role="textbox" aria-multiline="true"
        [style.minHeight.px]="minHeight()"
        [attr.data-ph]="placeholder()"></div>
    </div>
  `,
  styles: [
    `
    app-rich-editor { display: block; }
    .rte {
      border: 1px solid var(--hair);
      border-radius: var(--r-md);
      background: var(--panel-2);
      /* NOT overflow:hidden — the toolbar dropdowns (heading style / link) drop
         below the toolbar and must escape the editor box. Corners are kept
         clean by rounding the toolbar/content edges individually instead. */
    }
    .rte-toolbar {
      display: flex; align-items: center; gap: 2px;
      padding: 5px 6px; flex-wrap: wrap;
      border-bottom: 1px solid var(--hair);
      background: var(--panel);
      border-radius: calc(var(--r-md) - 1px) calc(var(--r-md) - 1px) 0 0;
    }
    .rte-div { width: 1px; height: 16px; background: var(--hair); margin: 0 3px; flex: none; }
    .rte-btn {
      width: 27px; height: 27px; border-radius: var(--r-sm);
      border: 1px solid transparent; background: transparent;
      color: var(--ink-3); cursor: pointer; display: grid; place-items: center;
      font-family: var(--font-mono); font-size: 12px; flex: none;
      transition: background .12s, color .12s, border-color .12s;
    }
    .rte-btn:hover { background: var(--panel-3); color: var(--ink); }
    .rte-btn.on {
      background: color-mix(in oklch, var(--accent), transparent 86%);
      color: var(--accent);
      border-color: color-mix(in oklch, var(--accent), transparent 60%);
    }
    .rte-btn .bold { font-weight: 800; }
    .rte-btn .ital { font-style: italic; font-family: Georgia, serif; }
    .rte-btn .strike { text-decoration: line-through; }
    .rte-btn .mono { font-size: 13px; font-family: var(--font-mono); }
    .rte-btn .num { font-size: 11px; letter-spacing: -1px; }
    .rte-btn .quo { font-family: Georgia, serif; font-size: 16px; line-height: 1; }

    /* heading / paragraph dropdown */
    .rte-block-wrap { position: relative; }
    .rte-btn.rte-block {
      width: auto; min-width: 56px; padding: 0 7px; gap: 5px;
      display: flex; align-items: center; justify-content: space-between;
      font-family: var(--font-disp);
    }
    .rte-btn.rte-block .cv { font-size: 11px; font-weight: 600; }
    .rte-btn.rte-block .chev {
      width: 0; height: 0; flex: none;
      border-left: 3.5px solid transparent; border-right: 3.5px solid transparent;
      border-top: 4px solid currentColor; opacity: 0.7;
    }
    .rte-block-pop {
      position: absolute; top: calc(100% + 6px); left: 0; z-index: 20;
      display: flex; flex-direction: column; gap: 1px; padding: 4px; width: 156px;
      background: var(--elev); border: 1px solid var(--hair-2);
      border-radius: var(--r-md); box-shadow: var(--shadow);
    }
    .rte-block-opt {
      display: flex; align-items: center; padding: 6px 9px;
      border: none; border-radius: var(--r-sm); background: transparent;
      color: var(--ink-2); cursor: pointer; text-align: left;
      font-family: var(--font-mono); font-size: 11.5px;
    }
    .rte-block-opt:hover { background: var(--panel-3); color: var(--ink); }
    .rte-block-opt.on { color: var(--accent); }
    .rte-block-opt .pv { color: var(--ink); }
    .rte-block-opt .pv.paragraph { font-family: var(--font-mono); font-weight: 400; }
    .rte-block-opt .pv.h1 { font-family: var(--font-disp); font-size: 16px; font-weight: 600; }
    .rte-block-opt .pv.h2 { font-family: var(--font-disp); font-size: 14.5px; font-weight: 600; }
    .rte-block-opt .pv.h3 { font-family: var(--font-disp); font-size: 13px; font-weight: 600; }
    .rte-block-opt .pv.h4 { font-family: var(--font-disp); font-size: 12.5px; font-weight: 600; }
    .rte-block-opt .pv.h5,
    .rte-block-opt .pv.h6 {
      font-family: var(--font-disp); font-size: 11px; font-weight: 600;
      text-transform: uppercase; letter-spacing: 0.06em;
    }

    .rte-link-wrap { position: relative; }
    .rte-link-pop {
      position: absolute; top: calc(100% + 6px); left: 0; z-index: 20;
      display: flex; gap: 6px; padding: 6px; width: 250px;
      background: var(--elev); border: 1px solid var(--hair-2);
      border-radius: var(--r-md); box-shadow: var(--shadow);
    }
    .rte-link-input {
      flex: 1; min-width: 0; padding: 5px 8px;
      background: var(--panel-2); border: 1px solid var(--hair);
      border-radius: var(--r-sm); color: var(--ink);
      font-family: var(--font-mono); font-size: 11.5px; outline: none;
    }
    .rte-link-add {
      padding: 4px 9px; border: none; border-radius: var(--r-sm);
      background: linear-gradient(180deg, var(--accent), color-mix(in oklch, var(--accent), #000 14%));
      color: white; font-family: var(--font-disp); font-size: 11.5px;
      cursor: pointer; white-space: nowrap;
    }

    /* Editor body layout only. Content typography (headings, lists, code,
       bold/italic/strike, …) is global in src/styles.css — shared with the
       read-only view and applied to the DOM Lexical builds at runtime. */
    .rte-content {
      outline: none;
      padding: 12px 14px; max-height: 460px; overflow-y: auto;
      border-radius: 0 0 calc(var(--r-md) - 1px) calc(var(--r-md) - 1px);
    }
    .rte-content.compact { padding: 9px 11px; max-height: 220px; }
    .rte-content:empty::before,
    .rte-content > p:only-child:empty::before {
      content: attr(data-ph); color: var(--ink-4); pointer-events: none;
    }
    `,
  ],
})
export class RichEditorComponent implements AfterViewInit, OnDestroy {
  readonly value = input<string>("");
  readonly placeholder = input<string>("Write something…");
  readonly compact = input<boolean>(false);
  readonly minHeight = input<number>(120);
  /** Bump to clear/reload the editor from `value` (e.g. after submitting a comment). */
  readonly resetSignal = input<number>(0);
  readonly valueChange = output<string>();

  private readonly contentRef = viewChild<ElementRef<HTMLElement>>("content");
  private readonly linkInputRef = viewChild<ElementRef<HTMLInputElement>>("linkInput");

  readonly linkOpen = signal(false);
  readonly linkUrl = signal("");
  /** The editor selection captured when the link popover opened — restored
   *  before toggling, since focusing the URL input collapses the live one. */
  private savedSelection: ReturnType<typeof snapshotSelection> = null;

  /** Live selection state — drives the toolbar's active highlights and the
   *  paragraph-style dropdown's current value. */
  readonly state = signal<BlockState>(EMPTY_BLOCK_STATE);
  readonly blockMenuOpen = signal(false);

  /** Options for the paragraph-style dropdown (Body + H1…H6). */
  readonly blockOptions: ReadonlyArray<{ value: ParagraphStyle; label: string }> = [
    { value: "paragraph", label: "Body" },
    { value: "h1", label: "Heading 1" },
    { value: "h2", label: "Heading 2" },
    { value: "h3", label: "Heading 3" },
    { value: "h4", label: "Heading 4" },
    { value: "h5", label: "Heading 5" },
    { value: "h6", label: "Heading 6" },
  ];

  /** Compact label shown on the dropdown trigger ("Body" / "H1"…"H6"). */
  readonly blockLabel = computed(() => {
    const b = this.state().block;
    return b === "h1" || b === "h2" || b === "h3" || b === "h4" || b === "h5" || b === "h6"
      ? b.toUpperCase()
      : "Body";
  });

  private editor: LexicalEditor | null = null;
  private cleanups: Array<() => void> = [];
  /** True while we apply `value`/`resetSignal` into the editor — suppresses the
   *  resulting update-listener emit so we don't fight the parent. */
  private loadingExternal = false;
  /** Last HTML we emitted; lets us ignore identical `value` echoes from the parent. */
  private lastEmitted: string | null = null;
  private lastResetSeen = 0;

  constructor() {
    // React to external value / resetSignal changes after mount.
    effect(() => {
      const html = this.value();
      const reset = this.resetSignal();
      if (!this.editor) return;
      const resetChanged = reset !== this.lastResetSeen;
      this.lastResetSeen = reset;
      // Ignore the echo of HTML we just emitted, unless a reset was requested.
      if (!resetChanged && html === this.lastEmitted) return;
      this.applyExternalHtml(html);
    });
  }

  ngAfterViewInit(): void {
    const root = this.contentRef()?.nativeElement;
    if (!root) return; // view query not resolved (defensive)
    const editor = buildRichEditor("rich-editor");
    this.editor = editor;
    editor.setRootElement(root);

    this.cleanups.push(
      registerRichText(editor),
      registerHistory(editor, createEmptyHistoryState(), 300),
      registerList(editor),
    );

    // Keyboard: ⌘B / ⌘I handled natively by registerRichText; we add ⌘K for link.
    this.cleanups.push(
      editor.registerCommand(
        KEY_MODIFIER_COMMAND,
        (event: KeyboardEvent) => {
          const meta = event.metaKey || event.ctrlKey;
          if (meta && event.key.toLowerCase() === "k") {
            event.preventDefault();
            this.openLink();
            return true;
          }
          return false;
        },
        COMMAND_PRIORITY_NORMAL,
      ),
    );

    // HTML out. `editor.read()` installs the active-editor context that
    // `$generateHtmlFromNodes` needs (plain `editorState.read()` would not).
    this.cleanups.push(
      editor.registerUpdateListener(() => {
        if (this.loadingExternal) return;
        editor.read(() => {
          const html = serializeEditorToHtml(editor);
          this.lastEmitted = html;
          this.valueChange.emit(html);
        });
      }),
    );

    // Mirror the selection into `state` so the toolbar shows active formats and
    // the dropdown reflects the current block. Keep the last value when the
    // selection isn't a usable range (e.g. focus left the editor).
    this.cleanups.push(
      editor.registerUpdateListener(({ editorState }) => {
        editorState.read(() => {
          const next = readBlockState();
          if (next) this.state.set(next);
        });
      }),
    );

    // Seed initial value.
    this.lastResetSeen = this.resetSignal();
    this.applyExternalHtml(this.value());
  }

  ngOnDestroy(): void {
    for (const dispose of this.cleanups) dispose();
    this.cleanups = [];
    this.editor?.setRootElement(null);
    this.editor = null;
  }

  private applyExternalHtml(html: string): void {
    const editor = this.editor;
    if (!editor) return;
    this.loadingExternal = true;
    editor.update(
      () => {
        loadHtmlIntoEditor(editor, html);
      },
      {
        discrete: true,
        onUpdate: () => {
          this.loadingExternal = false;
          this.lastEmitted = html;
        },
      },
    );
  }

  // ── toolbar handlers ──────────────────────────────────────────────────────

  /** Run a toolbar command against the editor's CURRENT selection. Each button
   *  calls preventDefault() on mousedown, so the editor keeps focus + its live
   *  selection. We must NOT call editor.focus() here — re-focusing an already
   *  focused contentEditable collapses that selection, after which the command
   *  has nothing to act on and silently no-ops. So we dispatch directly. */
  private withFocus(run: () => void): void {
    const editor = this.editor;
    if (!editor) return;
    run();
  }

  fmt(event: Event, format: TextFormatType): void {
    event.preventDefault();
    this.withFocus(() => {
      if (this.editor) applyTextFormat(this.editor, format);
    });
  }

  toggleBlockMenu(event: Event): void {
    event.preventDefault();
    this.blockMenuOpen.update((open) => !open);
  }

  /** Apply a paragraph style (Body / H1…H6) to the selected block, then close
   *  the dropdown. Uses mousedown+preventDefault to keep the editor selection. */
  setBlock(event: Event, style: ParagraphStyle): void {
    event.preventDefault();
    this.blockMenuOpen.set(false);
    this.withFocus(() => {
      if (this.editor) setParagraphStyle(this.editor, style);
    });
  }

  quote(event: Event): void {
    event.preventDefault();
    this.withFocus(() =>
      this.editor?.update(() => {
        const selection = $getSelection();
        if ($isRangeSelection(selection)) {
          $setBlocksType(selection, () => $createQuoteNode());
        }
      }),
    );
  }

  codeblock(event: Event): void {
    event.preventDefault();
    this.withFocus(() =>
      this.editor?.update(() => {
        const selection = $getSelection();
        if ($isRangeSelection(selection)) {
          $setBlocksType(selection, () => $createCodeNode());
        }
      }),
    );
  }

  list(event: Event, kind: "ul" | "ol"): void {
    event.preventDefault();
    this.withFocus(() =>
      this.editor?.dispatchCommand(
        kind === "ul" ? INSERT_UNORDERED_LIST_COMMAND : INSERT_ORDERED_LIST_COMMAND,
        undefined,
      ),
    );
  }

  // ── link popover ──────────────────────────────────────────────────────────

  openLink(event?: Event): void {
    event?.preventDefault();
    this.blockMenuOpen.set(false);
    // Capture the selection NOW, before focusing the input collapses it.
    this.savedSelection = this.editor ? snapshotSelection(this.editor) : null;
    this.linkUrl.set("");
    this.linkOpen.set(true);
    // Focus the input next frame (after the @if renders it).
    queueMicrotask(() => this.linkInputRef()?.nativeElement.focus());
  }

  closeLink(): void {
    this.linkOpen.set(false);
    this.savedSelection = null;
  }

  onLinkInput(event: Event): void {
    this.linkUrl.set((event.target as HTMLInputElement).value);
  }

  applyLink(): void {
    const editor = this.editor;
    this.linkOpen.set(false);
    if (!editor) return;
    let url: string | null = this.linkUrl().trim();
    // Empty → remove any link on the selection.
    if (!url) url = null;
    else if (!/^(https?:|mailto:)/i.test(url)) url = `https://${url}`;
    applyLinkToSelection(editor, url, this.savedSelection);
    this.savedSelection = null;
  }
}
