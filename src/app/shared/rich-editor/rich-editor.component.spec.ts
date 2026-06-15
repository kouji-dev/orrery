import { Component, SecurityContext, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { DomSanitizer } from "@angular/platform-browser";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { IconComponent } from "../icon.component";
import {
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  $getSelection,
  $isRangeSelection,
  $setSelection,
  FORMAT_TEXT_COMMAND,
} from "lexical";
import { $createHeadingNode, registerRichText } from "@lexical/rich-text";
import { $generateHtmlFromNodes } from "@lexical/html";
import { $setBlocksType } from "@lexical/selection";
import { beforeAll, describe, expect, it } from "vitest";
import {
  applyLinkToSelection,
  applyTextFormat,
  buildRichEditor,
  loadHtmlIntoEditor,
  readBlockState,
  roundTripHtml,
  serializeEditorToHtml,
  setParagraphStyle,
  snapshotSelection,
} from "./rich-editor.lexical";
import { RichEditorComponent } from "./rich-editor.component";
import { RichViewComponent } from "./rich-view.component";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

// Parse serialized HTML into a DOM so we can assert structure/text robustly.
// (Lexical wraps text runs in <span style="white-space: pre-wrap"> and adds
// theme classes, so naive `<tag>text</tag>` regexes are too brittle.)
function dom(html: string): HTMLElement {
  const d = new DOMParser().parseFromString(html, "text/html");
  return d.body;
}

// ── Pure helpers: fully headless, no contentEditable / selection needed ───────
describe("rich-editor HTML round-trip", () => {
  it("preserves a paragraph of plain text", () => {
    const body = dom(roundTripHtml("<p>hello world</p>"));
    const p = body.querySelector("p");
    expect(p).not.toBeNull();
    expect(p?.textContent).toBe("hello world");
  });

  it("keeps bold as <strong> (or <b>) through the round-trip", () => {
    const body = dom(roundTripHtml("<p><strong>bold text</strong></p>"));
    const bold = body.querySelector("strong, b");
    expect(bold).not.toBeNull();
    expect(bold?.textContent).toBe("bold text");
  });

  it("keeps an italic run", () => {
    const body = dom(roundTripHtml("<p><em>slanted</em></p>"));
    const ital = body.querySelector("em, i");
    expect(ital).not.toBeNull();
    expect(ital?.textContent).toBe("slanted");
  });

  it("preserves an H2 heading", () => {
    const body = dom(roundTripHtml("<h2>A Heading</h2>"));
    const h2 = body.querySelector("h2");
    expect(h2).not.toBeNull();
    expect(h2?.textContent).toBe("A Heading");
  });

  it("preserves a bulleted list with its items", () => {
    const body = dom(roundTripHtml("<ul><li>one</li><li>two</li></ul>"));
    expect(body.querySelector("ul")).not.toBeNull();
    const items = body.querySelectorAll("li");
    expect(items.length).toBe(2);
    expect(items[0].textContent).toBe("one");
    expect(items[1].textContent).toBe("two");
  });

  it("preserves a numbered list as <ol>", () => {
    const body = dom(roundTripHtml("<ol><li>first</li></ol>"));
    expect(body.querySelector("ol")).not.toBeNull();
    expect(body.querySelector("li")?.textContent).toBe("first");
  });

  it("preserves a blockquote", () => {
    const body = dom(roundTripHtml("<blockquote>quoted</blockquote>"));
    const bq = body.querySelector("blockquote");
    expect(bq).not.toBeNull();
    expect(bq?.textContent).toBe("quoted");
  });

  it("preserves a link with its href", () => {
    const body = dom(roundTripHtml('<p><a href="https://example.com">site</a></p>'));
    const a = body.querySelector("a");
    expect(a).not.toBeNull();
    expect(a?.textContent).toBe("site");
    expect(a?.getAttribute("href")).toBe("https://example.com");
  });

  it("preserves inline code", () => {
    const body = dom(roundTripHtml("<p><code>x = 1</code></p>"));
    const code = body.querySelector("code");
    expect(code).not.toBeNull();
    expect(code?.textContent).toBe("x = 1");
  });

  it("returns empty-ish HTML for empty input (no crash)", () => {
    const out = roundTripHtml("");
    expect(typeof out).toBe("string");
  });

  it("is idempotent: round-tripping twice yields the same HTML", () => {
    const first = roundTripHtml("<h2>Title</h2><p>body <strong>x</strong></p>");
    const second = roundTripHtml(first);
    expect(second).toBe(first);
  });
});

// ── Command behavior on a mounted editor (real contentEditable root) ──────────
// FORMAT_TEXT_COMMAND's handler is installed by registerRichText, which needs a
// root element. We mount one (jsdom contentEditable) so the command actually
// runs end-to-end, then assert the serialized markup.
describe("rich-editor commands", () => {
  it("FORMAT_TEXT bold on a fully-selected text node produces <strong>/<b>", () => {
    const editor = buildRichEditor("cmd-bold");
    const host = document.createElement("div");
    host.contentEditable = "true";
    document.body.appendChild(host);
    editor.setRootElement(host);
    const disposeRT = registerRichText(editor);

    editor.update(
      () => {
        const root = $getRoot();
        root.clear();
        const p = $createParagraphNode();
        const t = $createTextNode("make me bold");
        p.append(t);
        root.append(p);
        // anchor at start, focus at end of the text node = full selection
        t.select(0, t.getTextContentSize());
      },
      { discrete: true },
    );
    editor.dispatchCommand(FORMAT_TEXT_COMMAND, "bold");
    let html = "";
    editor.read(() => {
      html = serializeEditorToHtml(editor);
    });
    disposeRT();
    editor.setRootElement(null);
    host.remove();

    const bold = dom(html).querySelector("strong, b");
    expect(bold).not.toBeNull();
    expect(bold?.textContent).toBe("make me bold");
  });

  it("$setBlocksType to a heading serializes the paragraph as <h2>", () => {
    const editor = buildRichEditor("cmd-heading");
    editor.update(
      () => {
        const root = $getRoot();
        root.clear();
        const p = $createParagraphNode();
        const t = $createTextNode("Section");
        p.append(t);
        root.append(p);
        t.select(0, t.getTextContentSize());
        const sel = $getSelection();
        if ($isRangeSelection(sel)) {
          $setBlocksType(sel, () => $createHeadingNode("h2"));
        }
      },
      { discrete: true },
    );
    let html = "";
    editor.read(() => {
      html = $generateHtmlFromNodes(editor, null);
    });
    const h2 = dom(html).querySelector("h2");
    expect(h2).not.toBeNull();
    expect(h2?.textContent).toBe("Section");
  });

  it("registers rich-text on a root element without throwing", () => {
    const editor = buildRichEditor("cmd-register");
    const host = document.createElement("div");
    host.contentEditable = "true";
    document.body.appendChild(host);
    editor.setRootElement(host);
    const dispose = registerRichText(editor);
    expect(typeof dispose).toBe("function");
    dispose();
    editor.setRootElement(null);
    host.remove();
  });
});

// ── Block style + inline-format merge rule ────────────────────────────────────
// Mount a real root so selection.formatText() runs end-to-end, seed a single
// paragraph of text with the whole node selected, then exercise the helpers.
describe("rich-editor block style + merge rule", () => {
  function mountWithText(namespace: string, text: string) {
    const editor = buildRichEditor(namespace);
    const host = document.createElement("div");
    host.contentEditable = "true";
    document.body.appendChild(host);
    editor.setRootElement(host);
    const dispose = registerRichText(editor);
    editor.update(
      () => {
        const root = $getRoot();
        root.clear();
        const p = $createParagraphNode();
        const t = $createTextNode(text);
        p.append(t);
        root.append(p);
        t.select(0, t.getTextContentSize());
      },
      { discrete: true },
    );
    const teardown = () => {
      dispose();
      editor.setRootElement(null);
      host.remove();
    };
    return { editor, teardown };
  }

  function html(editor: ReturnType<typeof buildRichEditor>): string {
    let out = "";
    editor.read(() => {
      out = serializeEditorToHtml(editor);
    });
    return out;
  }

  it("setParagraphStyle promotes the block to the requested heading (h4)", () => {
    const { editor, teardown } = mountWithText("blk-h4", "Section");
    setParagraphStyle(editor, "h4");
    const body = dom(html(editor));
    expect(body.querySelector("h4")?.textContent).toBe("Section");
    teardown();
  });

  it("setParagraphStyle('paragraph') demotes a heading back to a paragraph", () => {
    const { editor, teardown } = mountWithText("blk-demote", "Section");
    setParagraphStyle(editor, "h2");
    setParagraphStyle(editor, "paragraph");
    const body = dom(html(editor));
    expect(body.querySelector("h2")).toBeNull();
    expect(body.querySelector("p")?.textContent).toBe("Section");
    teardown();
  });

  it("bold on a heading removes the heading first, then applies bold (no merge)", () => {
    const { editor, teardown } = mountWithText("blk-merge", "Heading text");
    setParagraphStyle(editor, "h2");
    applyTextFormat(editor, "bold");
    const body = dom(html(editor));
    // heading is gone…
    expect(body.querySelector("h1, h2, h3, h4, h5, h6")).toBeNull();
    // …and the text is now bold inside a paragraph
    const strong = body.querySelector("strong, b");
    expect(strong?.textContent).toBe("Heading text");
    expect(body.querySelector("p")).not.toBeNull();
    teardown();
  });

  it("bold on a plain paragraph just bolds it (block unchanged)", () => {
    const { editor, teardown } = mountWithText("blk-plain", "plain");
    applyTextFormat(editor, "bold");
    const body = dom(html(editor));
    expect(body.querySelector("strong, b")?.textContent).toBe("plain");
    expect(body.querySelector("h1, h2, h3, h4, h5, h6")).toBeNull();
    teardown();
  });

  it("strikethrough survives a save→reload round-trip (re-importable export)", () => {
    const { editor, teardown } = mountWithText("blk-strike", "struck");
    applyTextFormat(editor, "strikethrough");
    const saved = html(editor); // serialized notes, as they'd be stored
    teardown();
    // Re-importing + re-serializing keeps the strike only if the importer
    // recognized it — our exporter re-emits the inline style, so it's present.
    const reloaded = roundTripHtml(saved);
    expect(reloaded).toMatch(/line-through/);
    expect(dom(reloaded).textContent).toContain("struck");
  });

  it("readBlockState reports the current heading tag and active formats", () => {
    const { editor, teardown } = mountWithText("blk-state", "x");
    setParagraphStyle(editor, "h3");
    applyTextFormat(editor, "italic"); // demotes h3 → paragraph, then italic
    let state: ReturnType<typeof readBlockState> = null;
    editor.read(() => {
      state = readBlockState();
    });
    expect(state).not.toBeNull();
    expect(state!.block).toBe("paragraph");
    expect(state!.italic).toBe(true);
    teardown();
  });

  // The real bug: focusing the link popover's <input> collapses the editor
  // selection, so a naive toggle had no range to wrap. snapshotSelection +
  // applyLinkToSelection restore the captured range first. Here we simulate the
  // focus loss by clearing the selection between snapshot and apply.
  it("applyLinkToSelection wraps the SNAPSHOT selection even after it was cleared", () => {
    const { editor, teardown } = mountWithText("blk-link", "link me");
    const saved = snapshotSelection(editor);
    expect(saved).not.toBeNull();
    // simulate the popover input stealing focus → live selection gone
    editor.update(() => $setSelection(null), { discrete: true });
    applyLinkToSelection(editor, "https://example.com", saved);
    const a = dom(html(editor)).querySelector("a");
    expect(a).not.toBeNull();
    expect(a?.getAttribute("href")).toBe("https://example.com");
    expect(a?.textContent).toBe("link me");
    teardown();
  });

  it("applyLinkToSelection(null) removes a link from the saved selection", () => {
    const { editor, teardown } = mountWithText("blk-unlink", "unlink");
    applyLinkToSelection(editor, "https://example.com", snapshotSelection(editor));
    expect(dom(html(editor)).querySelector("a")).not.toBeNull();
    // selection still spans the now-linked text → snapshot + remove
    applyLinkToSelection(editor, null, snapshotSelection(editor));
    expect(dom(html(editor)).querySelector("a")).toBeNull();
    teardown();
  });
});

// NOTE on the component tests below.
// Raw vitest JIT does NOT apply Angular's signal-input transform to components
// it compiles, so host-template bindings to a child's `input()` do not flow
// (the child renders with its initializer defaults). We therefore:
//   • mount the real components to prove they boot + tear down cleanly and that
//     the template/toolbar structure is correct (structure is default-driven);
//   • cover all *input-driven* behavior (HTML round-trip, formatting commands,
//     placeholder/minHeight/compact wiring) elsewhere — the pure-helper suite
//     above, and the AOT app build/E2E. Sanitization is verified directly
//     against Angular's DomSanitizer (the exact engine `[innerHTML]` uses),
//     which needs no input wiring.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

// ── Editor mounts + tears down without error ──────────────────────────────────
describe("RichEditorComponent smoke", () => {
  it("mounts, renders the full toolbar + content area, and destroys cleanly", () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    TestBed.overrideComponent(RichEditorComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
    const fixture = TestBed.createComponent(RichEditorComponent);
    fixture.detectChanges();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector(".rte-toolbar")).not.toBeNull();
    const content = el.querySelector(".rte-content") as HTMLElement;
    expect(content).not.toBeNull();
    // placeholder default is exposed as the data-ph attribute used by the CSS
    expect(content.getAttribute("data-ph")).toBe("Write something…");
    // toolbar buttons: block-style dropdown, B, I, S, code, ul, ol, quote,
    // codeblock, link
    expect(el.querySelectorAll(".rte-btn").length).toBe(10);
    // the paragraph-style dropdown trigger is present, its menu closed initially
    expect(el.querySelector(".rte-btn.rte-block")).not.toBeNull();
    expect(el.querySelector(".rte-block-pop")).toBeNull();
    // the link popover is closed initially
    expect(el.querySelector(".rte-link-pop")).toBeNull();
    fixture.destroy(); // → ngOnDestroy disposes the Lexical listeners
    TestBed.resetTestingModule();
  });

  it("toggles the paragraph-style dropdown, rendering Body + H1…H6 options", () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    TestBed.overrideComponent(RichEditorComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
    const fixture = TestBed.createComponent(RichEditorComponent);
    fixture.detectChanges();
    fixture.componentInstance.toggleBlockMenu(new MouseEvent("mousedown"));
    fixture.detectChanges();
    const el: HTMLElement = fixture.nativeElement;
    const opts = el.querySelectorAll(".rte-block-opt");
    // Body + H1…H6 = 7 options
    expect(opts.length).toBe(7);
    expect(opts[0].textContent?.trim()).toBe("Body");
    expect(opts[1].textContent?.trim()).toBe("Heading 1");
    fixture.destroy();
    TestBed.resetTestingModule();
  });

  it("opens the link popover when the link button is pressed (no native prompt)", () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    TestBed.overrideComponent(RichEditorComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
    const fixture = TestBed.createComponent(RichEditorComponent);
    fixture.detectChanges();
    fixture.componentInstance.openLink();
    fixture.detectChanges();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector(".rte-link-pop")).not.toBeNull();
    expect(el.querySelector(".rte-link-input")).not.toBeNull();
    fixture.destroy();
    TestBed.resetTestingModule();
  });
});

// ── RichView renders + sanitizes stored HTML ──────────────────────────────────
describe("RichViewComponent", () => {
  it("mounts read-only and uses an [innerHTML] binding", () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const fixture = TestBed.createComponent(RichViewComponent);
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).querySelector(".rte-view")).not.toBeNull();
    fixture.destroy();
    TestBed.resetTestingModule();
  });

  it("Angular's HTML sanitizer (used by [innerHTML]) strips <script> but keeps content", () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const sanitizer = TestBed.inject(DomSanitizer);
    const clean = sanitizer.sanitize(
      SecurityContext.HTML,
      "<p>ok</p><script>window.__x=1</script><a href=\"javascript:alert(1)\">x</a>",
    );
    expect(clean).not.toBeNull();
    expect(clean).toContain("ok");
    // <script> is removed entirely…
    expect(clean).not.toContain("<script");
    // …and a javascript: URL is neutralized (rewritten to the inert "unsafe:" scheme)
    expect(clean).not.toMatch(/href="javascript:/);
    expect(clean).toContain("unsafe:");
    TestBed.resetTestingModule();
  });
});
