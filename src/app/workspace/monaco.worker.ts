/// <reference lib="webworker" />
// Monaco's base editor worker (tokenization/diff/word services). This stub
// exists so the app-side `new Worker(new URL("./monaco.worker", ...))` uses a
// specifier the bundler can statically resolve and bundle. No language-service
// workers (ts/json/css/html) are shipped — highlighting is Monarch-only.
import "monaco-editor/esm/vs/editor/editor.worker.js";
