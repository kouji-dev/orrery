import { Marked } from "marked";
import { mermaidExtension } from "./marked-mermaid";
import { tablesExtension } from "./marked-tables";

/**
 * The one marked configuration the app renders markdown with — GFM (marked's
 * default) plus the mermaid fence extension. Every consumer goes through this
 * factory so the tokenizer fix and the `.md-box` markup can never diverge
 * between the preview and the source map that stamps its blocks.
 */
export function createMarked(): Marked {
  return new Marked().use(mermaidExtension()).use(tablesExtension());
}
