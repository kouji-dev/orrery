// Pure, framework-agnostic helpers around Lexical's vanilla core.
//
// These are extracted from the Angular component so the HTML round-trip and
// editor wiring can be unit-tested in jsdom without mounting a contentEditable
// (selection/contentEditable are flaky in jsdom; the headless editor is not).
import { $isCodeNode, CodeHighlightNode, CodeNode } from "@lexical/code";
import { $generateHtmlFromNodes, $generateNodesFromDOM } from "@lexical/html";
import { $toggleLink, LinkNode } from "@lexical/link";
import { $isListNode, ListItemNode, ListNode } from "@lexical/list";
import {
  $createHeadingNode,
  $isHeadingNode,
  $isQuoteNode,
  type HeadingTagType,
  HeadingNode,
  QuoteNode,
} from "@lexical/rich-text";
import { $setBlocksType } from "@lexical/selection";
import {
  $createParagraphNode,
  $getRoot,
  $getSelection,
  $insertNodes,
  $isRangeSelection,
  $isTextNode,
  $setSelection,
  type BaseSelection,
  type CreateEditorArgs,
  type DOMExportOutput,
  type EditorThemeClasses,
  type LexicalEditor,
  type LexicalNode,
  type TextFormatType,
  TextNode,
  createEditor,
} from "lexical";

/** CSS classes Lexical assigns to the nodes it renders. Kept small + stable so
 *  the component stylesheet and the read-only view can target them. */
export const RICH_THEME: EditorThemeClasses = {
  heading: {
    h1: "rte-h1",
    h2: "rte-h2",
    h3: "rte-h3",
    h4: "rte-h4",
    h5: "rte-h5",
    h6: "rte-h6",
  },
  quote: "rte-quote",
  list: {
    ul: "rte-ul",
    ol: "rte-ol",
    listitem: "rte-li",
    nested: { listitem: "rte-nested-li" },
  },
  link: "rte-link",
  code: "rte-codeblock",
  text: {
    bold: "rte-bold",
    italic: "rte-italic",
    strikethrough: "rte-strike",
    code: "rte-inline-code",
  },
};

/** The node set the editor must register to support the feature set. */
export const RICH_NODES = [
  HeadingNode,
  QuoteNode,
  ListNode,
  ListItemNode,
  LinkNode,
  CodeNode,
  CodeHighlightNode,
];

/** Export override for text runs. Lexical renders strikethrough as a class-only
 *  `<span class="rte-strike">` — styled fine by our CSS, but NOT round-trippable
 *  (the importer keys off `<s>`/`<del>` tags or an inline `text-decoration`, not
 *  our class), so re-opening saved notes to edit would drop the strike. We add
 *  the inline style on export so it survives the round-trip and stays styled
 *  even in contexts without our stylesheet. */
function exportTextNode(editor: LexicalEditor, node: LexicalNode): DOMExportOutput {
  const output = node.exportDOM(editor);
  if ($isTextNode(node) && node.hasFormat("strikethrough") && output.element instanceof HTMLElement) {
    const prev = output.element.style.textDecorationLine;
    output.element.style.textDecorationLine =
      prev && prev !== "none" ? `${prev} line-through` : "line-through";
  }
  return output;
}

/** Build a configured (but un-mounted) Lexical editor. Safe to call headless. */
export function buildRichEditor(namespace = "rich-editor"): LexicalEditor {
  const config: CreateEditorArgs = {
    namespace,
    nodes: RICH_NODES,
    theme: RICH_THEME,
    html: { export: new Map([[TextNode, exportTextNode]]) },
    onError: (err) => {
      // Surface in dev; never throw out of Lexical's internal pipeline.
      console.error("[rich-editor]", err);
    },
  };
  return createEditor(config);
}

/** Parse an HTML string into Lexical nodes and replace the editor's content.
 *  Must run inside `editor.update(...)`. */
export function loadHtmlIntoEditor(editor: LexicalEditor, html: string): void {
  const dom = new DOMParser().parseFromString(html || "", "text/html");
  const nodes = $generateNodesFromDOM(editor, dom);
  const root = $getRoot();
  root.clear();
  root.select();
  $insertNodes(nodes);
}

/** Serialize the editor's current state to an HTML string.
 *  Must run inside `editorState.read(...)` (or any read context). */
export function serializeEditorToHtml(editor: LexicalEditor): string {
  return $generateHtmlFromNodes(editor, null);
}

// ── Block / inline formatting (shared by the toolbar) ────────────────────────

/** The paragraph styles the heading dropdown offers. */
export type ParagraphStyle = "paragraph" | HeadingTagType;

/** What the toolbar needs to reflect the current selection: the block kind and
 *  which inline text formats are active. `block` is the top-level element kind
 *  under the selection ("paragraph" | "h1"…"h6" | "quote" | "code" | "ul" |
 *  "ol"). */
export interface BlockState {
  block: ParagraphStyle | "quote" | "code" | "ul" | "ol";
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  code: boolean;
}

export const EMPTY_BLOCK_STATE: BlockState = {
  block: "paragraph",
  bold: false,
  italic: false,
  strikethrough: false,
  code: false,
};

/** Convert the selected block(s) to a paragraph or a heading (h1…h6).
 *  Must be dispatched against the editor's live selection. */
export function setParagraphStyle(editor: LexicalEditor, style: ParagraphStyle): void {
  editor.update(() => {
    const selection = $getSelection();
    if (!$isRangeSelection(selection)) return;
    $setBlocksType(selection, () =>
      style === "paragraph" ? $createParagraphNode() : $createHeadingNode(style),
    );
  });
}

/** Apply an inline text format (bold/italic/strikethrough/code) to the
 *  selection. Enforces the cosmetic merge rule: a heading is already
 *  display-weighted, so layering an inline format on top is meaningless —
 *  demote the heading to a paragraph first, then apply the format. */
export function applyTextFormat(editor: LexicalEditor, format: TextFormatType): void {
  editor.update(() => {
    const selection = $getSelection();
    if (!$isRangeSelection(selection)) return;
    const block = selection.anchor.getNode().getTopLevelElement();
    if (block !== null && $isHeadingNode(block)) {
      $setBlocksType(selection, () => $createParagraphNode());
    }
    // Re-read: $setBlocksType may have replaced the block nodes the selection
    // pointed at, so grab the current selection before toggling the format.
    const next = $getSelection();
    if ($isRangeSelection(next)) next.formatText(format);
  });
}

/** Read the current block kind + active inline formats from the selection.
 *  Must run inside a read context. Returns `null` when there is no usable
 *  range selection (caller keeps the last known state). */
export function readBlockState(): BlockState | null {
  const selection = $getSelection();
  if (!$isRangeSelection(selection)) return null;
  const top = selection.anchor.getNode().getTopLevelElement();
  let block: BlockState["block"] = "paragraph";
  if ($isHeadingNode(top)) block = top.getTag();
  else if ($isQuoteNode(top)) block = "quote";
  else if ($isCodeNode(top)) block = "code";
  else if ($isListNode(top)) block = top.getListType() === "number" ? "ol" : "ul";
  return {
    block,
    bold: selection.hasFormat("bold"),
    italic: selection.hasFormat("italic"),
    strikethrough: selection.hasFormat("strikethrough"),
    code: selection.hasFormat("code"),
  };
}

// ── Links ────────────────────────────────────────────────────────────────────

/** Snapshot the current range selection. The link popover focuses its URL
 *  `<input>`, which steals DOM focus from the contentEditable and collapses the
 *  editor's selection — so we grab a clone here (while the selection is still
 *  the user's text run) to restore before toggling the link. Returns `null`
 *  when there's no usable range selection. */
export function snapshotSelection(editor: LexicalEditor): BaseSelection | null {
  return editor.getEditorState().read(() => {
    const selection = $getSelection();
    return $isRangeSelection(selection) ? selection.clone() : null;
  });
}

/** Apply a link (`url`) — or remove any link (`url === null`) — over `saved`,
 *  the selection captured by {@link snapshotSelection}. Restores that selection
 *  first (the URL input collapsed the live one), then toggles the link in the
 *  same update. Falls back to the live selection when `saved` is null. Self-
 *  contained: calls `$toggleLink` directly so it doesn't depend on the editor
 *  having a `TOGGLE_LINK_COMMAND` handler registered. */
export function applyLinkToSelection(
  editor: LexicalEditor,
  url: string | null,
  saved: BaseSelection | null,
): void {
  editor.update(() => {
    if (saved) $setSelection(saved.clone());
    const selection = $getSelection();
    if (!$isRangeSelection(selection)) return;
    $toggleLink(url);
  });
}

/**
 * Pure HTML round-trip: parse `html` → Lexical nodes → serialize back to HTML.
 * Used both by the component (on reset) and by tests to assert the editor
 * normalizes/preserves the markup it is given. Fully headless.
 */
export function roundTripHtml(html: string, namespace = "rich-rt"): string {
  const editor = buildRichEditor(namespace);
  let out = "";
  editor.update(
    () => {
      loadHtmlIntoEditor(editor, html);
    },
    { discrete: true },
  );
  // `editor.read()` (not `editorState.read()`) installs the active-editor
  // context that `$generateHtmlFromNodes` → `node.createDOM()` requires.
  editor.read(() => {
    out = serializeEditorToHtml(editor);
  });
  return out;
}
