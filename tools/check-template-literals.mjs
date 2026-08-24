#!/usr/bin/env node
/**
 * A backtick inside a component's `styles:` / `template:` string terminates the
 * literal early. TypeScript reports it as a confusing syntax error far from the
 * cause, and a CSS-only break can even reach the browser looking like "my rule
 * isn't applying". Catch it at the source instead.
 */
import { readFileSync } from "node:fs";
import { globSync } from "node:fs";

const files = globSync("src/**/*.ts");
let bad = 0;
for (const f of files) {
  const src = readFileSync(f, "utf8");
  // walk template literals, flagging a stray backtick inside a CSS/HTML comment
  const re = /(template|styles)\s*:\s*\[?\s*`/g;
  let m;
  while ((m = re.exec(src))) {
    let i = re.lastIndex, depth = 0;
    for (; i < src.length; i++) {
      const c = src[i];
      if (c === "\\") { i++; continue; }
      if (c === "$" && src[i + 1] === "{") { depth++; i++; continue; }
      if (c === "}" && depth) { depth--; continue; }
      if (c === "`" && !depth) break;              // literal ends here
    }
    const body = src.slice(re.lastIndex, i);
    const line = src.slice(0, re.lastIndex).split("\n").length;
    const commentTick = body.match(/\/\*[^*]*`[\s\S]*?\*\//);
    if (commentTick) {
      console.error(`${f}:${line}: backtick inside a comment in a ${m[1]} literal`);
      console.error(`   ${commentTick[0].split("\n")[0].slice(0, 78)}`);
      bad++;
    }
    re.lastIndex = i + 1;
  }
}
console.log(bad ? `FAIL: ${bad} stray backtick(s)` : "OK: no stray backticks in template literals");
process.exit(bad ? 1 : 0);
