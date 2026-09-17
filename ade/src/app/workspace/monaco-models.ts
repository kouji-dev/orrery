import type * as monacoApi from "monaco-editor";

/**
 * The slice of the Monaco API the cache touches — narrow on purpose so the
 * spec can hand it a fake (the `fakeMonaco` pattern of monaco-loader.spec).
 */
export interface ModelApi {
  editor: {
    createModel(
      value: string,
      language?: string,
      uri?: monacoApi.Uri,
    ): monacoApi.editor.ITextModel;
    getModel(uri: monacoApi.Uri): monacoApi.editor.ITextModel | null;
    setModelLanguage(model: monacoApi.editor.ITextModel, languageId: string): void;
  };
  Uri: { parse(value: string): monacoApi.Uri };
}

/** What `ensure` needs to build a model it has never seen. */
export interface ModelSource {
  text: string;
  langId: string;
}

/**
 * Refcounted registry of Monaco text models keyed by uri.
 *
 * Why a cache at all: Monaco's peek/references widgets resolve a target by
 * `getModel(uri)` — a location in another file only previews if that file
 * ALREADY has a model. So navigation loads models it is not editing, and
 * something has to own their lifetime:
 *
 * - `acquire`/`release` are the editor's handle. A model with a positive
 *   refcount is HELD and is never disposed, whatever the LRU says.
 * - unheld models (released editors, peek targets) sit in an LRU of `max`;
 *   the oldest is disposed when a newer one pushes it out.
 * - a released model keeps its content (the last buffer text), so peeking
 *   into a file that was open a moment ago shows what the user last saw.
 */
export class ModelCache {
  /** uri → refcount of live editors on it. */
  private readonly held = new Map<string, number>();
  /** Unheld model uris, oldest first. */
  private lru: string[] = [];
  /** `ensure` loads in flight, so two peeks at once share one read. */
  private readonly loading = new Map<string, Promise<monacoApi.editor.ITextModel | null>>();

  constructor(private readonly max = 24) {}

  /** Get-or-create the model for `uri` and hold it. The text is pushed in
   *  when it differs (a peek may have loaded the file before the editor
   *  opened it, from disk rather than the edit buffer). */
  acquire(monaco: ModelApi, uri: string, text: string, langId: string): monacoApi.editor.ITextModel {
    const u = monaco.Uri.parse(uri);
    const key = u.toString();
    let model = monaco.editor.getModel(u);
    if (model && !model.isDisposed()) {
      if (model.getValue() !== text) model.setValue(text);
      if (model.getLanguageId() !== langId) monaco.editor.setModelLanguage(model, langId);
    } else {
      model = monaco.editor.createModel(text, langId, u);
    }
    this.held.set(key, (this.held.get(key) ?? 0) + 1);
    this.lru = this.lru.filter((k) => k !== key);
    return model;
  }

  /** Drop one hold. At zero the model joins the LRU (still alive — a peek
   *  target, a fast reopen). Never disposes here. */
  release(monaco: ModelApi, uri: string): void {
    const key = monaco.Uri.parse(uri).toString();
    const n = (this.held.get(key) ?? 0) - 1;
    if (n > 0) {
      this.held.set(key, n);
      return;
    }
    this.held.delete(key);
    const model = monaco.editor.getModel(monaco.Uri.parse(uri));
    if (!model || model.isDisposed()) return;
    this.touch(key);
    this.evict(monaco);
  }

  /** Make sure a model exists for `uri` (peek targets). Loads through
   *  `loader` only when there is none; an existing model — held or LRU — is
   *  returned as is. Resolves null when the load fails. */
  ensure(monaco: ModelApi, uri: string, loader: () => Promise<ModelSource>): Promise<monacoApi.editor.ITextModel | null> {
    const u = monaco.Uri.parse(uri);
    const key = u.toString();
    const have = monaco.editor.getModel(u);
    if (have && !have.isDisposed()) {
      if (!this.held.has(key)) this.touch(key);
      return Promise.resolve(have);
    }
    const inflight = this.loading.get(key);
    if (inflight) return inflight;
    const p = loader()
      .then((src) => {
        // the editor may have acquired it while the read was in flight
        const again = monaco.editor.getModel(u);
        if (again && !again.isDisposed()) return again;
        const model = monaco.editor.createModel(src.text, src.langId, u);
        this.touch(key);
        this.evict(monaco);
        return model;
      })
      .catch(() => null)
      .finally(() => this.loading.delete(key));
    this.loading.set(key, p);
    return p;
  }

  /** True while at least one editor holds `uri`. */
  isHeld(monaco: ModelApi, uri: string): boolean {
    return this.held.has(monaco.Uri.parse(uri).toString());
  }

  /** Unheld uris, oldest first (spec introspection). */
  unheld(): readonly string[] {
    return this.lru;
  }

  private touch(key: string): void {
    this.lru = this.lru.filter((k) => k !== key);
    this.lru.push(key);
  }

  private evict(monaco: ModelApi): void {
    while (this.lru.length > this.max) {
      const key = this.lru.shift()!;
      if (this.held.has(key)) continue; // belt and braces: a held model never goes
      const model = monaco.editor.getModel(monaco.Uri.parse(key));
      if (model && !model.isDisposed()) model.dispose();
    }
  }
}

/** The app-wide cache — one Monaco, one model registry. */
export const MODEL_CACHE = new ModelCache();
