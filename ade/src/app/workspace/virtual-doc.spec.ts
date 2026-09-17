import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { isLibraryUri, isVirtualUri, libCrumbs, virtualFileName, virtualLang, VirtualDocService } from "./virtual-doc";

const JDT = "jdt://contents/java.base/java.util/ArrayList.class?=demo/%5C/usr%5C/lib%5C/jvm%5C/lib%5C/jrt-fs.jar%60java.base=/javadoc_location=/https:%5C/%5C/docs.oracle.com%5C/en%5C/java%5C/javase%5C/21%5C/docs%5C/api%5C/=/=/maven.pomderived=/true=/<java.util(ArrayList.class";

describe("virtual uri helpers", () => {
  it("isVirtualUri: any scheme but orrery://, never a worktree path", () => {
    expect(isVirtualUri("jdt://contents/x.class")).toBe(true);
    expect(isVirtualUri("orrery-lib://jdk/java/util/List.java")).toBe(true);
    expect(isVirtualUri("orrery://agent-1/src/x.ts")).toBe(false);
    expect(isVirtualUri("src/x.ts")).toBe(false);
    expect(isVirtualUri("C:/x/y.ts")).toBe(false);
  });

  it("virtualFileName: last path segment, decoded, query/fragment stripped", () => {
    expect(virtualFileName(JDT)).toBe("ArrayList.class");
    expect(virtualFileName("orrery-lib://jdk/java/util/List.java")).toBe("List.java");
    expect(virtualFileName("orrery-lib://jdk/java/util/Map%24Entry.java#frag")).toBe("Map$Entry.java");
    expect(virtualFileName("x://host/")).toBe("host");
    expect(virtualFileName("x://")).toBe("x://");
  });

  it("virtualLang comes from the file name", () => {
    expect(virtualLang("orrery-lib://jdk/java/util/List.java")).toBe("java");
    expect(virtualLang("orrery-lib://cargo/serde-1.0/src/lib.rs")).toBe("rust");
    expect(virtualLang(JDT)).toBe("");
  });
});

describe("libCrumbs (M4, design LibDocToolbar)", () => {
  it("a JDK entry: label · module · dotted package, file name last", () => {
    expect(isLibraryUri("orrery-lib://jdk1/java.base/java/util/ArrayList.java")).toBe(true);
    expect(libCrumbs("orrery-lib://jdk1/java.base/java/util/ArrayList.java", "ArrayList.java — JDK 21 (java.base)")).toEqual({
      crumbs: ["JDK 21", "java.base", "java.util"],
      name: "ArrayList.java",
    });
  });

  it("a Maven entry without a module: label · dotted package", () => {
    expect(libCrumbs("orrery-lib://m2/org/apache/commons/lang3/StringUtils.java", "StringUtils.java — commons-lang3 3.12.0")).toEqual({
      crumbs: ["commons-lang3 3.12.0", "org.apache.commons.lang3"],
      name: "StringUtils.java",
    });
  });

  it("a cargo entry keeps one crumb per directory", () => {
    expect(libCrumbs("orrery-lib://cargo/serde-1.0.200/src/lib.rs", "lib.rs — serde 1.0.200")).toEqual({
      crumbs: ["serde 1.0.200", "serde-1.0.200", "src"],
      name: "lib.rs",
    });
  });

  it("M4.1 artifact-prefixed entries: the crate dir / the jar segment is dropped in favour of the title's label", () => {
    expect(libCrumbs("orrery-lib://cargo:p1/serde-1.0.219/src/de/mod.rs", "mod.rs — serde-1.0.219 (src/de)")).toEqual({
      crumbs: ["serde-1.0.219", "src", "de"],
      name: "mod.rs",
    });
    expect(libCrumbs("orrery-lib://maven:p1/gson-2.11.0-sources.jar!/com/google/gson/Gson.java", "Gson.java — gson-2.11.0-sources.jar (com.google.gson)")).toEqual({
      crumbs: ["gson-2.11.0-sources.jar", "com.google.gson"],
      name: "Gson.java",
    });
  });

  it("a sparse title still yields the file name and the path crumbs; a nested class decodes", () => {
    expect(libCrumbs("orrery-lib://jdk1/java.base/java/util/Map%24Entry.java", "")).toEqual({
      crumbs: ["java.base.java.util"],
      name: "Map$Entry.java",
    });
    expect(libCrumbs("orrery-lib://jdk1/java.base/java/util/Map%24Entry.java", "Map$Entry.java — JDK 21 (java.base)").crumbs).toEqual(["JDK 21", "java.base", "java.util"]);
  });

  it("any other virtual doc has no crumbs and keeps its title as the name (M3 behaviour)", () => {
    expect(isLibraryUri("jdt://contents/x.class")).toBe(false);
    expect(libCrumbs("jdt://contents/java.base/java.util/ArrayList.class?=p", "java.util.ArrayList")).toEqual({ crumbs: [], name: "java.util.ArrayList" });
    expect(libCrumbs("jdt://contents/java.base/java.util/ArrayList.class?=p", "")).toEqual({ crumbs: [], name: "ArrayList.class" });
  });
});

describe("VirtualDocService", () => {
  function make(answer: (uri: string) => Promise<unknown>) {
    const invoke = vi.fn((cmd: string, payload?: Record<string, unknown>) => {
      if (cmd !== "nav_virtual_read") return Promise.resolve(null);
      return answer(String(payload?.["uri"]));
    });
    const bridge = { invoke } as unknown as Bridge;
    const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }, { provide: VirtualDocService, useClass: VirtualDocService }] });
    const svc = runInInjectionContext(injector, () => injector.get(VirtualDocService));
    return { svc, invoke };
  }

  it("reads once per uri and memoizes (concurrent + later reads share the answer)", async () => {
    const { svc, invoke } = make(async (uri) => ({ uri, language: "java", text: "class A {}", title: "java.util.A" }));
    const [a, b] = await Promise.all([svc.read("orrery-lib://jdk/A.java"), svc.read("orrery-lib://jdk/A.java")]);
    const c = await svc.read("orrery-lib://jdk/A.java");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("nav_virtual_read", { uri: "orrery-lib://jdk/A.java" });
    expect(a).toEqual({ uri: "orrery-lib://jdk/A.java", language: "java", text: "class A {}", title: "java.util.A" });
    expect(b).toBe(a);
    expect(c).toBe(a);
    expect(svc.has("orrery-lib://jdk/A.java")).toBe(true);
  });

  it("fills a sparse payload from the uri (language from the name, title = file name)", async () => {
    const { svc } = make(async () => ({ text: "x" }));
    const d = await svc.read("orrery-lib://jdk/java/util/List.java");
    expect(d).toEqual({ uri: "orrery-lib://jdk/java/util/List.java", language: "java", text: "x", title: "List.java" });
  });

  it("a failed read is not memoized — the next read asks again", async () => {
    let n = 0;
    const { svc, invoke } = make(async (uri) => {
      if (++n === 1) throw new Error("server gone");
      return { uri, language: "java", text: "ok", title: "T" };
    });
    await expect(svc.read("jdt://x")).rejects.toThrow("server gone");
    expect(svc.has("jdt://x")).toBe(false);
    expect((await svc.read("jdt://x")).text).toBe("ok");
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("keeps a bounded number of docs, dropping the oldest", async () => {
    const { svc } = make(async (uri) => ({ uri, language: "", text: "", title: uri }));
    for (let i = 0; i < 30; i++) await svc.read(`x://d/${i}`);
    expect(svc.has("x://d/0")).toBe(false);
    expect(svc.has("x://d/29")).toBe(true);
  });
});
