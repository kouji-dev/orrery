// The runtime chunk imports monaco's fine-grained ESM entry (editor + all
// editor contributions, no language services). Its API surface is the same
// `monaco` namespace the root package types describe.
declare module "monaco-editor/esm/vs/editor/edcore.main.js" {
  export * from "monaco-editor";
}
