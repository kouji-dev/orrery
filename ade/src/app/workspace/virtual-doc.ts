import { inject, Injectable } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { VirtualDoc } from "../models";
import { langId } from "../utils";
import { isVirtualUri } from "./nav-providers";

export { isVirtualUri };

/** The last path segment of a virtual uri, decoded; query and fragment
 *  stripped ("jdt://contents/java.util/List.class?=p/…" → "List.class"). */
export function virtualFileName(uri: string): string {
  const body = uri.replace(/^[a-z][\w+.-]*:\/\//i, "").replace(/[?#].*$/, "");
  const seg = body.split("/").filter(Boolean).pop() ?? "";
  try {
    return decodeURIComponent(seg) || uri;
  } catch {
    return seg || uri;
  }
}

/** App language tag of a virtual uri, from its file name ("" = plain). */
export function virtualLang(uri: string): string {
  return langId(virtualFileName(uri));
}

/** The library-source scheme (M4): `orrery-lib://<sourceId>/<entry path>`. */
export const LIB_SCHEME = "orrery-lib";

export function isLibraryUri(uri: string): boolean {
  return uri.startsWith(LIB_SCHEME + "://");
}

/**
 * Design LibDocToolbar crumbs for a virtual doc. A library entry
 * (`orrery-lib://jdk1/java.base/java/util/ArrayList.java`, title
 * "ArrayList.java — JDK 21 (java.base)") → crumbs `JDK 21 · java.base ·
 * java.util` + name `ArrayList.java`: the source label and the module come
 * from the title ("<name> — <label> (<module>)"), the rest from the entry's
 * directory segments (the artifact segment — the crate dir, or the jar
 * with its `!` — dropped when it leads, then the module; Java packages
 * collapse to one dotted crumb, other languages keep one crumb per
 * directory). Any other virtual doc (a jdt:// class file) has no crumbs and
 * shows its title as the name, as before.
 */
export function libCrumbs(uri: string, title: string): { crumbs: string[]; name: string } {
  if (!isLibraryUri(uri)) return { crumbs: [], name: title || virtualFileName(uri) };
  const body = uri.slice(LIB_SCHEME.length + 3).replace(/[?#].*$/, "");
  const entry = body.split("/").filter(Boolean).map(safeDecode).slice(1); // [0] = source id
  const file = entry.pop() ?? "";
  const m = /^(.*?)\s+[—–-]\s+(.*?)(?:\s+\((.*)\))?\s*$/.exec(title ?? "");
  const name = (m?.[1] || file || virtualFileName(uri)).trim();
  const label = (m?.[2] ?? "").trim();
  const module = (m?.[3] ?? "").trim();
  let dirs = entry;
  if (dirs.length && (dirs[0] === label || dirs[0].endsWith("!"))) dirs = dirs.slice(1);
  // the scope is a module ("java.base") or, for a crate file, its dir path
  // ("src/de"): one crumb per segment, the matching leading dirs dropped
  const scope = module ? module.split("/") : [];
  if (scope.length && scope.every((s, i) => dirs[i] === s)) dirs = dirs.slice(scope.length);
  else if (module && dirs.join(".") === module) dirs = []; // a jar's package IS the scope
  const rest = /\.java$/i.test(name) ? (dirs.length ? [dirs.join(".")] : []) : dirs;
  return { crumbs: [label, ...scope, ...rest].filter(Boolean), name };
}

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** Keep this many read docs around — a lib source is a few hundred KB. */
const MAX_DOCS = 24;

/**
 * Read-only virtual documents (M3 generic; M4 adds library breadcrumbs):
 * anything a navigation answer points at that is not an `orrery://` worktree
 * file — a jdtls class-file source, a library entry. One `nav_virtual_read`
 * per uri, memoized (a failed read is dropped so a retry asks again).
 */
@Injectable({ providedIn: "root" })
export class VirtualDocService {
  private readonly bridge = inject(BRIDGE);
  /** uri → the read (settled or in flight), newest last. */
  private readonly docs = new Map<string, Promise<VirtualDoc>>();

  read(uri: string): Promise<VirtualDoc> {
    const have = this.docs.get(uri);
    if (have) return have;
    const p = this.bridge
      .invoke<VirtualDoc>(Commands.NavVirtualRead, { uri })
      .then((d) => ({
        uri: d?.uri || uri,
        language: d?.language || virtualLang(uri),
        text: d?.text ?? "",
        title: d?.title || virtualFileName(uri),
      }))
      .catch((e) => {
        this.docs.delete(uri);
        throw e;
      });
    this.docs.set(uri, p);
    while (this.docs.size > MAX_DOCS) {
      const oldest = this.docs.keys().next().value;
      if (oldest === undefined) break;
      this.docs.delete(oldest);
    }
    return p;
  }

  /** Spec/introspection: is `uri` memoized? */
  has(uri: string): boolean {
    return this.docs.has(uri);
  }
}
