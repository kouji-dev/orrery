// Fuzzy matching + keybinding helpers for the command palette / Search
// Everywhere overlays (roadmap B2.1 / B2.2). Ported from the design reference
// (design/orrery/project/commands.jsx) — subsequence match with word-start and
// consecutive-run bonuses.

export interface FzResult {
  score: number;
  /** Indices (into the original text) of the matched characters. */
  idx: number[];
}

/** Subsequence fuzzy match of `q` against `text`; null = no match. */
export function fzMatch(text: string, q: string): FzResult | null {
  if (!q) return { score: 0, idx: [] };
  const t = text.toLowerCase();
  const s = q.toLowerCase().replace(/\s+/g, "");
  let ti = 0;
  let score = 0;
  let run = 0;
  const idx: number[] = [];
  for (let qi = 0; qi < s.length; qi++) {
    const c = s[qi];
    let found = -1;
    for (let i = ti; i < t.length; i++) {
      if (t[i] === c) {
        found = i;
        break;
      }
    }
    if (found < 0) return null;
    const prev = found ? t[found - 1] : "";
    const boundary =
      found === 0 || prev === "/" || prev === "." || prev === "-" || prev === "_" || prev === " " || prev === ":";
    score += 6 + (boundary ? 10 : 0) + (found === ti ? run * 5 : 0);
    run = found === ti ? run + 1 : 1;
    idx.push(found);
    ti = found + 1;
  }
  score -= Math.max(0, text.length - s.length) * 0.12;
  if (t.startsWith(s)) score += 14;
  return { score, idx };
}

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

interface Binding {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
}

function parseBinding(str: string): Binding | null {
  if (!str) return null;
  const b: Binding = { ctrl: false, alt: false, shift: false, key: "" };
  for (const part of str.split("+")) {
    const l = part.trim().toLowerCase();
    if (l === "ctrl" || l === "mod") b.ctrl = true;
    else if (l === "alt") b.alt = true;
    else if (l === "shift") b.shift = true;
    else b.key = l;
  }
  return b;
}

/** Does a keydown event match a binding like "Ctrl+Shift+p"? ("Mod" = Cmd on
 *  macOS, Ctrl elsewhere — Windows-first today, mac-proofed for Phase 0.) */
export function matchBinding(e: KeyboardEvent, str: string): boolean {
  const b = parseBinding(str);
  if (!b) return false;
  if ((e.key || "").toLowerCase() !== b.key) return false;
  const primary = IS_MAC ? e.metaKey : e.ctrlKey;
  if (b.ctrl !== primary) return false;
  if (b.shift !== e.shiftKey) return false;
  if (b.alt !== e.altKey) return false;
  return true;
}

/** "Ctrl+Shift+p" → "Ctrl+Shift+P" (or mac glyphs). For the kbd chips. */
export function kbdLabel(str: string): string {
  if (!str) return "";
  if (str === "Shift Shift") return IS_MAC ? "⇧⇧" : "Shift Shift";
  const map: Record<string, string> = IS_MAC
    ? { mod: "⌘", ctrl: "⌘", alt: "⌥", shift: "⇧" }
    : { mod: "Ctrl+", ctrl: "Ctrl+", alt: "Alt+", shift: "Shift+" };
  return str
    .split("+")
    .map((p) => {
      const l = p.trim().toLowerCase();
      if (map[l]) return map[l];
      const special: Record<string, string> = {
        arrowup: "↑",
        arrowdown: "↓",
        arrowleft: "←",
        arrowright: "→",
        enter: "⏎",
        escape: "esc",
      };
      return special[l] ?? p.trim().toUpperCase();
    })
    .join("");
}
