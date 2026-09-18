// `node --import @orrery/ext/strip index.mjs` — install the loader hook.
//
// A convenience, not a boundary. See `loader.mjs` and the README.

import { register } from "node:module";

register(new URL("./loader.mjs", import.meta.url));
