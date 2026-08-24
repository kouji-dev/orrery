#!/usr/bin/env node
/**
 * Two failure modes that reach the BROWSER as "my CSS isn't applying", because
 * esbuild reports them as warnings (or not at all) rather than errors:
 *
 *  1. A backtick inside a comment in a `styles:` / `template:` literal ends the
 *     string early. TypeScript then reports a syntax error far from the cause.
 *  2. A regex sweep that strips a declaration can leave a custom property with
 *     no value (`--kj-tab-}`), which silently kills the rest of that rule.
 *
 * Both bit this codebase repeatedly during the type-system sweep. Run in CI, or
 * from the pre-push hook alongside lint.
 */
import { readFileSync } from "node:fs";
import { globSync } from "node:fs";

let bad = 0;

for (const file of globSync("src/**/*.ts")) {
  const src = readFileSync(file, "utf8");
  const re = /(template|styles)\s*:\s*\[?\s*`/g;
  let m;
  while ((m = re.exec(src))) {
    // walk to the literal's real end, stepping over ${…} interpolations
    let i = re.lastIndex;
    let depth = 0;
    for (; i < src.length; i++) {
      const c = src[i];
      if (c === "\\") { i++; continue; }
      if (c === "$" && src[i + 1] === "{") { depth++; i++; continue; }
      if (c === "}" && depth) { depth--; continue; }
      if (c === "`" && !depth) break;
    }
    const body = src.slice(re.lastIndex, i);
    const line = src.slice(0, re.lastIndex).split("\n").length;

    const tick = body.match(/\/\*[^*]*`[\s\S]*?\*\//);
    if (tick) {
      console.error(`${file}:${line}: backtick inside a comment in a ${m[1]} literal`);
      console.error(`    ${tick[0].split("\n")[0].slice(0, 76)}`);
      bad++;
    }

    const dead = body.match(/--[a-zA-Z0-9-]+\s*(?=[};])/);
    if (dead) {
      const at = body.indexOf(dead[0]);
      const around = body.slice(Math.max(0, at - 44), at + 24).replace(/\s+/g, " ");
      console.error(`${file}:${line}: custom property with no value in a ${m[1]} literal`);
      console.error(`    …${around}…`);
      bad++;
    }

    re.lastIndex = i + 1;
  }
}

console.log(bad ? `FAIL: ${bad} problem(s)` : "OK: template literals are intact");
process.exit(bad ? 1 : 0);
