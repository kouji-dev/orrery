import { describe, expect, it } from "vitest";
import type * as monacoApi from "monaco-editor";

import { ModelApi, ModelCache } from "./monaco-models";

/** A model registry that behaves like Monaco's: keyed by uri string, a
 *  disposed model leaves the registry, `isDisposed` is observable. */
function fakeMonaco() {
  const models = new Map<string, FakeModel>();
  class FakeModel {
    disposed = false;
    constructor(
      public value: string,
      public lang: string,
      public uri: { toString(): string },
    ) {}
    getValue() {
      return this.value;
    }
    setValue(v: string) {
      this.value = v;
    }
    getLanguageId() {
      return this.lang;
    }
    isDisposed() {
      return this.disposed;
    }
    dispose() {
      this.disposed = true;
      models.delete(this.uri.toString());
    }
  }
  const api: ModelApi = {
    Uri: { parse: (v: string) => ({ toString: () => v }) as unknown as monacoApi.Uri },
    editor: {
      createModel: (value, lang, uri) => {
        const m = new FakeModel(value, lang ?? "plaintext", uri!);
        models.set(uri!.toString(), m);
        return m as unknown as monacoApi.editor.ITextModel;
      },
      getModel: (uri) => (models.get(uri.toString()) as unknown as monacoApi.editor.ITextModel) ?? null,
      setModelLanguage: (m, lang) => {
        (m as unknown as FakeModel).lang = lang;
      },
    },
  };
  return { api, models };
}

describe("ModelCache", () => {
  it("acquire creates once and refcounts; release keeps the model alive", () => {
    const { api, models } = fakeMonaco();
    const cache = new ModelCache(24);
    const m1 = cache.acquire(api, "orrery://a/x.ts", "one", "typescript");
    const m2 = cache.acquire(api, "orrery://a/x.ts", "one", "typescript");
    expect(m1).toBe(m2);
    expect(models.size).toBe(1);
    expect(cache.isHeld(api, "orrery://a/x.ts")).toBe(true);

    cache.release(api, "orrery://a/x.ts");
    expect(cache.isHeld(api, "orrery://a/x.ts")).toBe(true); // still one holder
    cache.release(api, "orrery://a/x.ts");
    expect(cache.isHeld(api, "orrery://a/x.ts")).toBe(false);
    expect(m1.isDisposed()).toBe(false); // released ≠ disposed
    expect(cache.unheld()).toEqual(["orrery://a/x.ts"]);
  });

  it("acquire pushes newer text + language into a model a peek loaded first", async () => {
    const { api } = fakeMonaco();
    const cache = new ModelCache(24);
    const peeked = await cache.ensure(api, "orrery://a/x.ts", async () => ({ text: "disk", langId: "plaintext" }));
    const held = cache.acquire(api, "orrery://a/x.ts", "buffer", "typescript");
    expect(held).toBe(peeked);
    expect(held.getValue()).toBe("buffer");
    expect(held.getLanguageId()).toBe("typescript");
    expect(cache.unheld()).toEqual([]);
  });

  it("evicts the oldest UNHELD model past the cap and never a held one", async () => {
    const { api, models } = fakeMonaco();
    const cache = new ModelCache(2);
    const held = cache.acquire(api, "orrery://a/held.ts", "h", "typescript");
    for (const n of ["1", "2", "3"]) {
      await cache.ensure(api, `orrery://a/${n}.ts`, async () => ({ text: n, langId: "typescript" }));
    }
    // cap 2: "1" went first, "2" and "3" stay, the held one is untouched
    expect(models.has("orrery://a/1.ts")).toBe(false);
    expect(models.has("orrery://a/2.ts")).toBe(true);
    expect(models.has("orrery://a/3.ts")).toBe(true);
    expect(held.isDisposed()).toBe(false);
    expect(cache.unheld()).toEqual(["orrery://a/2.ts", "orrery://a/3.ts"]);

    // releasing the held one makes it the NEWEST unheld entry, bumping "2"
    cache.release(api, "orrery://a/held.ts");
    expect(held.isDisposed()).toBe(false);
    expect(models.has("orrery://a/2.ts")).toBe(false);
    expect(cache.unheld()).toEqual(["orrery://a/3.ts", "orrery://a/held.ts"]);
  });

  it("ensure returns an existing model without loading, and shares one load in flight", async () => {
    const { api } = fakeMonaco();
    const cache = new ModelCache(24);
    let loads = 0;
    const loader = async () => {
      loads++;
      return { text: "t", langId: "plaintext" };
    };
    const [a, b] = await Promise.all([
      cache.ensure(api, "orrery://a/x.ts", loader),
      cache.ensure(api, "orrery://a/x.ts", loader),
    ]);
    expect(a).toBe(b);
    expect(loads).toBe(1);
    await cache.ensure(api, "orrery://a/x.ts", loader);
    expect(loads).toBe(1);
  });

  it("ensure resolves null when the loader fails", async () => {
    const { api } = fakeMonaco();
    const cache = new ModelCache(24);
    const m = await cache.ensure(api, "orrery://a/x.ts", async () => {
      throw new Error("no such file");
    });
    expect(m).toBeNull();
  });
});
