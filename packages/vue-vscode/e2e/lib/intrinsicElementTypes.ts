/**
 * Intrinsic HTML element typing oracle for IDE parity E2E.
 *
 * First-class for Vue and Svelte equally. Official bar: hovering a native tag
 * (e.g. `<div>`) must surface a *concrete* element interface
 * (HTMLDivElement / HTMLDivAttributes / closed IntrinsicElements key), never an
 * open index signature like `(index) IntrinsicElements[string]: any`.
 *
 * Pure predicates so Vitest can discriminate without the extension host.
 */

/** Open-index / any-degraded intrinsic surfaces that must never pass. */
export const OPEN_INTRINSIC_INDEX_PATTERNS: readonly RegExp[] = [
  /IntrinsicElements\s*\[\s*string\s*\]/i,
  /\(index\)\s*IntrinsicElements/i,
  /IntrinsicElements\s*\[\s*string\s*\]\s*:\s*any/i,
  /\[\s*(?:x|key|k)\s*:\s*string\s*\]\s*:\s*any/i,
  /index\s+signature[^.\n]*:\s*any/i,
];

export interface IntrinsicTagSpec {
  readonly tag: string;
  /**
   * At least one of these substrings must appear in hover text.
   * Framework-neutral: accepts closed DOM/JSX spellings used by either
   * Vue (HTMLDivElement / HTMLAttributes) or Svelte (HTMLDivAttributes /
   * SvelteHTMLElements) — no preferred framework.
   */
  readonly anyOf: readonly string[];
  /**
   * Unique opening-tag prefix used as a TokenAnchor.token so occurrence
   * search is unambiguous across the fixture.
   */
  readonly openTagToken: string;
  /** Caret into openTagToken — default 1 lands on the first letter of the tag. */
  readonly tagCaretOffset?: number;
}

/**
 * Representative HTML / SVG intrinsic tags with distinct DOM interfaces.
 * Keep the set small enough for CI but broad enough to catch "every tag is any".
 */
export const HTML_INTRINSIC_TAGS: readonly IntrinsicTagSpec[] = [
  {
    tag: "div",
    anyOf: ["HTMLDivElement", "HTMLDivAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<div data-intrinsic="div-tag"',
  },
  {
    tag: "span",
    anyOf: ["HTMLSpanElement", "HTMLSpanAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<span data-intrinsic="span-tag"',
  },
  {
    tag: "button",
    anyOf: ["HTMLButtonElement", "HTMLButtonAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<button data-intrinsic="button-tag"',
  },
  {
    tag: "input",
    anyOf: ["HTMLInputElement", "HTMLInputAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<input data-intrinsic="input-tag"',
  },
  {
    tag: "a",
    anyOf: ["HTMLAnchorElement", "HTMLAnchorAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<a data-intrinsic="a-tag"',
  },
  {
    tag: "img",
    anyOf: ["HTMLImageElement", "HTMLImgAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<img data-intrinsic="img-tag"',
  },
  {
    tag: "form",
    anyOf: ["HTMLFormElement", "HTMLFormAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<form data-intrinsic="form-tag"',
  },
  {
    tag: "select",
    anyOf: ["HTMLSelectElement", "HTMLSelectAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<select data-intrinsic="select-tag"',
  },
  {
    tag: "textarea",
    anyOf: ["HTMLTextAreaElement", "HTMLTextareaAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<textarea data-intrinsic="textarea-tag"',
  },
  {
    tag: "h1",
    anyOf: ["HTMLHeadingElement", "HTMLHeadingAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<h1 data-intrinsic="h1-tag"',
  },
  {
    tag: "p",
    anyOf: ["HTMLParagraphElement", "HTMLParagraphAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<p data-intrinsic="p-tag"',
  },
  {
    tag: "ul",
    anyOf: ["HTMLUListElement", "HTMLUListAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<ul data-intrinsic="ul-tag"',
  },
  {
    tag: "li",
    anyOf: ["HTMLLIElement", "HTMLLiAttributes", "HTMLElement", "HTMLAttributes"],
    openTagToken: '<li data-intrinsic="li-tag"',
  },
  {
    tag: "svg",
    anyOf: ["SVGSVGElement", "SVGAttributes", "SVGElement", "SVG"],
    openTagToken: '<svg data-intrinsic="svg-tag"',
  },
];

/** Attribute sites with expected type needles (not open any). */
export interface IntrinsicAttrSpec {
  readonly id: string;
  readonly token: string;
  readonly caretOffset?: number;
  readonly anyOf: readonly string[];
  /** Optional positive mention of the attribute name. */
  readonly attrName?: string;
}

export const HTML_INTRINSIC_ATTRS: readonly IntrinsicAttrSpec[] = [
  {
    id: "a.href",
    token: 'href="https://example.com/intrinsic-a"',
    caretOffset: 0,
    anyOf: ["string", "href"],
    attrName: "href",
  },
  {
    id: "input.type",
    token: 'type="text"',
    caretOffset: 0,
    anyOf: ["string", "text", "type"],
    attrName: "type",
  },
  {
    id: "button.disabled",
    token: "disabled",
    // first `disabled` on the intrinsic button
    caretOffset: 0,
    anyOf: ["boolean", "disabled"],
    attrName: "disabled",
  },
  {
    id: "img.alt",
    token: 'alt="intrinsic-img"',
    caretOffset: 0,
    anyOf: ["string", "alt"],
    attrName: "alt",
  },
  {
    id: "div.class",
    token: 'class="intrinsic-div"',
    caretOffset: 0,
    anyOf: ["string", "class"],
    attrName: "class",
  },
];

export function looksLikeOpenIntrinsicIndex(text: string): boolean {
  return OPEN_INTRINSIC_INDEX_PATTERNS.some((re) => re.test(text));
}

export function hasConcreteIntrinsicType(text: string, anyOf: readonly string[]): boolean {
  return anyOf.some((needle) => text.includes(needle));
}

/**
 * Discriminating assert for intrinsic tag hover.
 * Throws with the full hover body on failure (for PRODUCT_GAP triage).
 */
export function assertIntrinsicElementHoverText(
  text: string,
  tag: string,
  anyOf: readonly string[],
): void {
  if (!text || !text.trim()) {
    throw new Error(`intrinsic <${tag}> hover empty`);
  }
  if (looksLikeOpenIntrinsicIndex(text)) {
    throw new Error(
      `intrinsic <${tag}> hover is open IntrinsicElements[string] (or equivalent), not a concrete element interface:\n${text}`,
    );
  }
  if (/:\s*any\b/.test(text) && !hasConcreteIntrinsicType(text, anyOf)) {
    throw new Error(
      `intrinsic <${tag}> hover degraded to any without a concrete element interface:\n${text}`,
    );
  }
  if (!hasConcreteIntrinsicType(text, anyOf)) {
    throw new Error(
      `intrinsic <${tag}> hover missing concrete interface (wanted one of ${anyOf.join("|")}):\n${text}`,
    );
  }
}

export function assertIntrinsicAttrHoverText(
  text: string,
  id: string,
  anyOf: readonly string[],
): void {
  if (!text || !text.trim()) {
    throw new Error(`intrinsic attr ${id} hover empty`);
  }
  if (looksLikeOpenIntrinsicIndex(text)) {
    throw new Error(`intrinsic attr ${id} hover is open index any:\n${text}`);
  }
  if (/:\s*any\b/.test(text) && !anyOf.some((n) => n !== "any" && text.includes(n))) {
    throw new Error(`intrinsic attr ${id} hover degraded to any:\n${text}`);
  }
  if (!anyOf.some((needle) => text.includes(needle))) {
    throw new Error(
      `intrinsic attr ${id} hover missing type needle (wanted one of ${anyOf.join("|")}):\n${text}`,
    );
  }
}

/** Relative fixture paths by framework. */
export function intrinsicElementsFile(framework: "vue" | "svelte"): string {
  return framework === "vue"
    ? "src/features/IntrinsicElements.vue"
    : "src/features/IntrinsicElements.svelte";
}
