import { inject, Injectable, signal } from "@angular/core";
import type * as monacoApi from "monaco-editor";

import { CommandRegistryService } from "../commands/command-registry.service";
import { BRIDGE, Commands } from "../data-source/bridge";
import { DocSymbol, NavHover, NavResult } from "../models";
import { LspStatusStore } from "../lsp/lsp-status.store";
import { AgentsStore } from "../stores/agents.store";
import { EditsStore } from "../stores/edits.store";
import { LibSrcStore } from "../extensions/libsrc.store";
import { langId } from "../utils";
import { MODEL_CACHE } from "./monaco-models";
import { MonacoApi, monacoLanguage } from "./monaco-loader";
import { buildProviders, isVirtualUri, parseModelUri, rawVirtualUri } from "./nav-providers";
import { VirtualDocService } from "./virtual-doc";

/** What the editor's NavHint says: "no definition found", (M3) "jdtls
 *  starting… showing index result", or (M4) why the library index had
 *  nothing: "JDK not found — set JAVA_HOME" / "sources jar not downloaded —
 *  run mvn dependency:sources". */
export type NavHintKind = "none" | "fallback" | "jdk-missing" | "sources-missing";

export interface NavHintEvent {
  id: string;
  path: string;
  kind: NavHintKind;
  /** `fallback`: the server that is starting ("jdtls"). */
  label?: string;
  /** Monotonic, so the same hint twice in a row still re-triggers the fade. */
  n: number;
}

/**
 * Registers the navigation providers with Monaco — ONCE per Monaco language
 * id, and the editor opener once per app — and answers their requests over
 * the bridge. Monaco's contributions (go-to-definition link on Ctrl+hover,
 * F12, Shift+F12 peek, Ctrl+Shift+O outline, hover) are all in the core
 * bundle; this is the only missing piece.
 *
 * Root-provided: providers are global Monaco state, and a per-editor
 * instance would register the same language four times per open tab.
 */
@Injectable({ providedIn: "root" })
export class NavProvidersService {
  private readonly bridge = inject(BRIDGE);
  private readonly edits = inject(EditsStore);
  private readonly agents = inject(AgentsStore);
  private readonly registry = inject(CommandRegistryService);
  private readonly lsp = inject(LspStatusStore);
  private readonly virtual = inject(VirtualDocService);
  private readonly libsrc = inject(LibSrcStore);

  /** The last "no definition" (or, later, fallback) event — the editor
   *  showing that file renders it as its NavHint chip. */
  readonly hint = signal<NavHintEvent | null>(null);

  private readonly langs = new Set<string>();
  private openerDone = false;
  /** Roots whose index we asked the backend to start this session. */
  private readonly indexed = new Set<string>();
  private hintN = 0;

  /** Register the four providers for `langId` unless already done. */
  ensureLanguage(monaco: MonacoApi, lang: string): void {
    if (this.langs.has(lang)) return;
    this.langs.add(lang);
    const p = buildProviders(monaco, {
      definition: (id, path, line, col, word, text) =>
        this.bridge.invoke<NavResult>(Commands.NavDefinition, { id, path, line, col, word, text: text ?? null }),
      references: (id, path, line, col, word, includeDeclaration, text) =>
        this.bridge.invoke<NavResult>(Commands.NavReferences, {
          id,
          path,
          line,
          col,
          word,
          includeDeclaration,
          text: text ?? null,
        }),
      hover: (id, path, line, col, word) =>
        this.bridge.invoke<NavHover | null>(Commands.NavHover, { id, path, line, col, word }),
      documentSymbols: (id, path, text) =>
        this.bridge.invoke<DocSymbol[]>(Commands.SymbolsDocument, { id, path, text }),
      ensureModel: (uri) => this.ensureModel(monaco, uri),
      sendText: (id, path) => this.edits.isDirty(id, path),
      onNoDefinition: (id, path, hint) => this.hint.set({ id, path, kind: hint ?? "none", n: ++this.hintN }),
      onFallback: (id, path) => this.postFallback(id, path),
    });
    monaco.languages.registerDefinitionProvider(lang, p.definition);
    monaco.languages.registerReferenceProvider(lang, p.references);
    monaco.languages.registerDocumentSymbolProvider(lang, p.documentSymbol);
    monaco.languages.registerHoverProvider(lang, p.hover);
  }

  /** The fallback hint stays posted for the length of its fade so the editor
   *  that the jump opens (and replaces the source with) still catches it;
   *  a "none" hint is consumed by the one editor of that file instead. */
  private postFallback(id: string, path: string): void {
    const n = ++this.hintN;
    this.hint.set({ id, path, kind: "fallback", label: this.lsp.labelFor(this.projectOf(id), langId(path)), n });
    setTimeout(() => {
      if (this.hint()?.n === n) this.hint.set(null);
    }, 4000);
  }

  /** The project a root belongs to: an agent's projectId, or the root id
   *  itself for a project tab (its pseudo-agent id IS the project id). */
  private projectOf(id: string): string {
    return this.agents.all().find((a) => a.id === id)?.projectId ?? id;
  }

  /** Register the editor opener once: a go-to target in ANOTHER model opens
   *  that file's tab at the location. Same-model jumps never reach it. A
   *  target outside `orrery://` is a virtual read-only doc: it opens as a
   *  tab of the SOURCE editor's root, the uri string being the tab path. */
  ensureOpener(monaco: MonacoApi): void {
    if (this.openerDone) return;
    this.openerDone = true;
    monaco.editor.registerEditorOpener({
      openCodeEditor: (source, resource, sel) => {
        let line = 1;
        let col = 1;
        if (sel) {
          if ("startLineNumber" in sel) {
            line = sel.startLineNumber;
            col = sel.startColumn;
          } else {
            line = sel.lineNumber;
            col = sel.column;
          }
        }
        const key = resource.toString();
        const t = parseModelUri(key);
        if (t) {
          this.registry.openFileAt(t.id, t.path, line, col);
          return true;
        }
        const uri = rawVirtualUri(key) ?? key;
        if (!isVirtualUri(uri)) return false;
        const from = parseModelUri(source.getModel()?.uri.toString() ?? "");
        const root = from?.id ?? this.rootOfVirtual(source);
        if (!root) return false;
        this.registry.openFileAt(root, uri, line, col);
        return true;
      },
    });
  }

  /** A jump FROM a virtual doc: its tab lives under some root — the editor
   *  showing it knows which (`data-root` on the host, set by the editor). */
  private rootOfVirtual(source: monacoApi.editor.ICodeEditor): string | null {
    const el = source.getDomNode()?.closest<HTMLElement>("[data-root]");
    return el?.dataset["root"] || null;
  }

  /** Ask the backend to index `id` (idempotent there; once per id here) —
   *  and (M4) the library sources that root needs (JDK for Java files,
   *  cargo for a Cargo.toml…; the backend decides). */
  startIndex(id: string): void {
    if (!id || this.indexed.has(id)) return;
    this.indexed.add(id);
    void this.bridge.invoke(Commands.SymbolsIndexStart, { id }).catch(() => this.indexed.delete(id));
    this.libsrc.ensure(id);
  }

  /** A model for a peek target: the edit buffer if that file is open, else
   *  the working-tree content; a virtual uri reads through `nav_virtual_read`
   *  (so a peek into a jdt:// target previews the class-file source). */
  private ensureModel(monaco: MonacoApi, uri: string): Promise<monacoApi.editor.ITextModel | null> {
    const t = parseModelUri(uri);
    if (!t) {
      if (!isVirtualUri(uri)) return Promise.resolve(null);
      return MODEL_CACHE.ensure(monaco, uri, async () => {
        const d = await this.virtual.read(uri);
        return { text: d.text, langId: await monacoLanguage(d.language) };
      });
    }
    return MODEL_CACHE.ensure(monaco, uri, async () => {
      const buf = this.edits.get(t.id, t.path);
      const text = buf ? buf.text : (await this.agents.diff(t.id, t.path)).new;
      const lang = await monacoLanguage(langId(t.path));
      // a peek into a new language needs its providers too, or the preview
      // pane has no hover / no nested navigation
      this.ensureLanguage(monaco, lang);
      return { text, langId: lang };
    });
  }
}
