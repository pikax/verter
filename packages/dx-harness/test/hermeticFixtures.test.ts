import { existsSync, readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { addFileAnchors, stripAnchors, type AnchorMap } from "../src/anchors.js";
import { canonicalizePath, joinCanonical } from "../src/paths.js";
import { ORACLE_FAMILIES } from "../src/semantic-oracle/model.js";

/** Absolute path to `fixtures/hermetic/` (the committed default corpus root). */
const HERMETIC = canonicalizePath(fileURLToPath(new URL("../fixtures/hermetic", import.meta.url)));
/** Absolute path to `oracles/semantic/` (the paired `.ts` gold-standard oracles). */
const ORACLES = canonicalizePath(fileURLToPath(new URL("../oracles/semantic", import.meta.url)));

/** The seven default corpus members. */
const FIXTURES = [
  "minimal-member-access",
  "template-events",
  "definition-precision",
  "diagnostics",
  "auto-import",
  "drawer-realistic",
  "semantic-oracle",
] as const;

/** Source extensions whose anchors strip (mirrors the materializer's TEXT_SOURCE). */
const TEXT_SOURCE = /\.(vue|ts|tsx|js|jsx|mts|cts)$/;

/** Recursively collect forward-slashed relative paths under `root` (skipping dotfiles). */
function walk(root: string, rel = "", out: string[] = []): string[] {
  const here = joinCanonical(root, rel);
  for (const entry of readdirSync(here, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) continue;
    const childRel = joinCanonical(rel, entry.name);
    if (entry.isDirectory()) walk(root, childRel, out);
    else if (entry.isFile()) out.push(childRel);
  }
  return out;
}

/** Strip every text file in a fixture dir into one anchor map, as the materializer does. */
function fixtureAnchors(fixture: string): { anchors: AnchorMap; stripped: Map<string, string> } {
  const dir = joinCanonical(HERMETIC, fixture);
  const anchors: AnchorMap = new Map();
  const stripped = new Map<string, string>();
  for (const rel of walk(dir).sort()) {
    if (!TEXT_SOURCE.test(rel)) continue;
    const result = stripAnchors(readFileSync(joinCanonical(dir, rel), "utf-8"));
    addFileAnchors(anchors, rel, result);
    stripped.set(rel, result.stripped);
  }
  return { anchors, stripped };
}

/** Split a stripped source into its LSP-folded lines. */
function lines(text: string): string[] {
  return text.split(/\r\n|\r|\n/);
}

/** The last ASCII identifier in `code`, or `null` if it has none. */
function lastIdentifier(code: string): string | null {
  const matches = code.match(/[A-Za-z_$][A-Za-z0-9_$]*/g);
  return matches ? matches[matches.length - 1] : null;
}

/**
 * The first non-whitespace text at or after `(line, character)`, scanning across
 * blank lines. A template anchor sits immediately before its element; the
 * canonical formatter may place the stripped comment on its own line above a long
 * element, so the element is the next meaningful token rather than literally on the
 * anchor's line.
 */
function nextToken(allLines: string[], line: number, character: number): string {
  let rest = (allLines[line] ?? "").slice(character);
  let li = line;
  while (rest.trim() === "" && li + 1 < allLines.length) {
    li += 1;
    rest = allLines[li];
  }
  return rest.trimStart();
}

/** A per-anchor expectation: where it lands relative to its target token. */
type Expectation =
  | { readonly before: string } // template anchor sits immediately before this token
  | { readonly after: string } // script code-trailing anchor: last identifier before it
  | { readonly lineContains: string } // the stripped anchor line contains this substring
  | { readonly emptyLine: true }; // own-line edit anchor on a now-empty line, column 0

/**
 * The intended landing token for every authored anchor, per fixture file. This is
 * the corpus's anchor contract: each entry asserts the strip leaves the recomputed
 * position pointing at the construct the scenarios probe.
 */
const ANCHOR_CONTRACT: Record<string, Record<string, Record<string, Expectation>>> = {
  "minimal-member-access": {
    "App.vue": {
      "mma.member": { after: "label" },
      "mma.incomplete": { after: "item" },
    },
  },
  "template-events": {
    "App.vue": {
      "evt.click": { before: "<button" },
      "evt.touchmove": { before: "<button" },
      "evt.ref": { before: "<input" },
      "evt.handlerArg": { after: "event" },
    },
  },
  "definition-precision": {
    "App.vue": {
      "def.importedUse": { after: "makeConfig" },
      "def.localUse": { after: "heading" },
      "def.propsRef": { after: "props" },
    },
  },
  diagnostics: {
    "App.vue": {
      "diag.realError": { lineContains: 'number = "not a number"' },
      "diag.validLet": { lineContains: "let message" },
    },
  },
  "auto-import": {
    "App.vue": {
      "imp.autoImportSite": { emptyLine: true },
    },
  },
  "drawer-realistic": {
    "Drawer.vue": {
      "drawer.overlay": { before: "<div" },
      "drawer.panel": { before: "<aside" },
      "drawer.slot": { before: "<slot" },
      "drawer.model": { lineContains: "defineModel" },
      "drawer.computed": { after: "title" },
      "drawer.emit": { lineContains: "defineEmits" },
      "drawer.editPoint": { emptyLine: true },
    },
  },
  "semantic-oracle": {
    "define-props.vue": { "props.title": { after: "title" }, "props.count": { after: "count" } },
    "define-emits.vue": { emit: { after: "emit" } },
    "define-model.vue": { "model.value": { after: "value" } },
    "slots.vue": { "slots.default": { after: "default" }, "slots.header": { after: "header" } },
    "template-ref.vue": { "ref.value": { after: "value" } },
    "fallthrough-attrs.vue": {
      "attrs.id": { after: "id" },
      "attrs.onClick": { after: "onClick" },
    },
    "auto-import-shape.vue": {
      "autoImport.ref": { after: "ref" },
      "autoImport.value": { after: "value" },
    },
    "event-args.vue": { "click.event": { after: "event" }, "keydown.event": { after: "event" } },
  },
};

/**
 * A single NAME-to-TYPE binding one oracle family models, expressed in BOTH the
 * `.vue` fixture's macro syntax and its `.ts` oracle's declaration syntax. The
 * pairing test matches each binding ONLY inside its family's comment-free owner
 * declaration span (see {@link FAMILY_SCOPES}) — never against the whole file — so a
 * drift that PRESERVES the family's token bag but rebinds a name to the wrong type (a
 * lost `?`, a type permutation, a moved emit payload, a swapped event arg or slot
 * shape) fails, and a stale doc comment, string/template literal, or unrelated
 * declaration that still spells the old binding cannot mask it. The regexes pin
 * name→type adjacency (`\s*` between tokens) and are deliberately tolerant of
 * incidental whitespace/formatting, never of the binding itself.
 */
interface FamilyBinding {
  /** What this binding means; the failing assertion's message. */
  readonly what: string;
  /** Must match the `.vue` fixture (the macro/template form of the binding). */
  readonly vue: RegExp;
  /** Must match the paired `.ts` oracle (the declaration form of the same binding). */
  readonly ts: RegExp;
}

/**
 * Per-family name→type bindings, keyed by fixture basename. Every curated oracle
 * family appears (asserted against {@link ORACLE_FAMILIES} in the pairing test), and
 * each cited drift class — `count?`→`count`, a `title`/`count` type permutation, a
 * moved emit payload, a swapped `MouseEvent`/`KeyboardEvent` arg, a swapped slot
 * prop shape, a dropped `| null` — flips its binding red on the side it mutates.
 */
const FAMILY_BINDINGS: Record<string, readonly FamilyBinding[]> = {
  "define-props": [
    { what: "title binds to string", vue: /\btitle\s*:\s*string\b/, ts: /\btitle\s*:\s*string\b/ },
    {
      what: "count binds to number and stays optional",
      vue: /\bcount\s*\?\s*:\s*number\b/,
      ts: /\bcount\s*\?\s*:\s*number\b/,
    },
  ],
  "define-emits": [
    {
      what: "submit carries a value: string payload",
      vue: /\bsubmit\s*:\s*\[\s*value\s*:\s*string\s*\]/,
      ts: /\bemit\s*\(\s*event\s*:\s*"submit"\s*,\s*value\s*:\s*string\s*\)/,
    },
    {
      what: "close carries no payload",
      vue: /\bclose\s*:\s*\[\s*\]/,
      ts: /\bemit\s*\(\s*event\s*:\s*"close"\s*\)/,
    },
  ],
  "define-model": [
    {
      what: "the model binds to boolean",
      vue: /\bdefineModel\s*<\s*boolean\s*>/,
      ts: /\bModelRef\s*<\s*boolean\s*>/,
    },
  ],
  slots: [
    {
      what: "the default slot's props are { item: string }",
      vue: /\bdefault\s*\(\s*props\s*:\s*\{\s*item\s*:\s*string\s*\}\s*\)/,
      ts: /\bdefault\s*\(\s*props\s*:\s*\{\s*item\s*:\s*string\s*\}\s*\)/,
    },
    {
      what: "the header slot's props are { title: string }",
      vue: /\bheader\s*\(\s*props\s*:\s*\{\s*title\s*:\s*string\s*\}\s*\)/,
      ts: /\bheader\s*\(\s*props\s*:\s*\{\s*title\s*:\s*string\s*\}\s*\)/,
    },
  ],
  "template-ref": [
    {
      what: "the ref element type is HTMLInputElement",
      vue: /\bref\s*<\s*HTMLInputElement\b/,
      ts: /\bTemplateRef\s*<\s*HTMLInputElement\s*>/,
    },
    {
      what: "the ref unwrap preserves | null",
      vue: /\bHTMLInputElement\s*\|\s*null\b/,
      ts: /\bvalue\s*:\s*T\s*\|\s*null\b/,
    },
  ],
  "fallthrough-attrs": [
    {
      what: "id binds to optional string",
      vue: /\bid\s*\?\s*:\s*string\b/,
      ts: /\bid\s*\?\s*:\s*string\b/,
    },
    {
      what: "onClick binds to (event: MouseEvent) => void",
      vue: /\bonClick\s*\?\s*:\s*\(\s*event\s*:\s*MouseEvent\s*\)\s*=>\s*void/,
      ts: /\bonClick\s*\?\s*:\s*\(\s*event\s*:\s*MouseEvent\s*\)\s*=>\s*void/,
    },
  ],
  "auto-import-shape": [
    {
      what: "counter is ref(0)",
      vue: /\bcounter\s*=\s*ref\s*\(\s*0\s*\)/,
      ts: /\bcounter\s*=\s*ref\s*\(\s*0\s*\)/,
    },
    {
      what: "current unwraps counter.value",
      vue: /\bcurrent\s*=\s*counter\s*\.\s*value\b/,
      ts: /\bcurrent\s*=\s*counter\s*\.\s*value\b/,
    },
  ],
  "event-args": [
    {
      what: "the click handler arg is MouseEvent",
      vue: /\bonClick\s*\(\s*event\s*:\s*MouseEvent\s*\)/,
      ts: /\bonClick\s*\(\s*handler\s*:\s*\(\s*event\s*:\s*MouseEvent\s*\)/,
    },
    {
      what: "the keydown handler arg is KeyboardEvent",
      vue: /\bonKeydown\s*\(\s*event\s*:\s*KeyboardEvent\s*\)/,
      ts: /\bonKeydown\s*\(\s*handler\s*:\s*\(\s*event\s*:\s*KeyboardEvent\s*\)/,
    },
  ],
};

/**
 * Remove every comment from a corpus source — `//` line comments, `/* … *\/` block
 * comments, and `<!-- … -->` template/HTML comments (which subsumes both
 * `@dx-anchor` forms) — while copying string and template-literal contents verbatim.
 * This is the string-PRESERVING view: dropping comments stops a stale doc comment
 * from standing in for the declaration it paraphrases, and keeping literal contents
 * lets the few legitimately string-aware checks read what they must — an import
 * specifier (`from "vue"`), an emit event-name discriminant (`event: "submit"`), a
 * template attribute value (`@click="onClick"`). {@link neutralizeLiterals} derives
 * the companion code-only view for everything that must NOT be satisfiable from a
 * literal. Comment bytes are dropped; surrounding text (including the newlines a
 * comment did not span) is preserved. A controlled scanner for the curated corpus,
 * not a general lexer.
 */
function stripComments(source: string): string {
  let out = "";
  for (let i = 0; i < source.length; ) {
    const c = source[i];
    // String / template literal: copy through the matching close, honoring escapes,
    // so a comment opener INSIDE a literal is never mistaken for a comment.
    if (c === '"' || c === "'" || c === "`") {
      out += c;
      i += 1;
      while (i < source.length) {
        out += source[i];
        if (source[i] === "\\" && i + 1 < source.length) {
          out += source[i + 1];
          i += 2;
          continue;
        }
        const closed = source[i] === c;
        i += 1;
        if (closed) break;
      }
      continue;
    }
    // `//` line comment: drop to (but not including) the line break.
    if (c === "/" && source[i + 1] === "/") {
      while (i < source.length && source[i] !== "\n" && source[i] !== "\r") i += 1;
      continue;
    }
    // C-style block comment.
    if (c === "/" && source[i + 1] === "*") {
      i += 2;
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) i += 1;
      i += 2;
      continue;
    }
    // `<!-- … -->` template/HTML comment (covers the template `@dx-anchor` form).
    if (c === "<" && source.startsWith("<!--", i)) {
      i += 4;
      while (i < source.length && !source.startsWith("-->", i)) i += 1;
      i += 3;
      continue;
    }
    out += c;
    i += 1;
  }
  return out;
}

/** The neutral filler a blanked literal interior is rebuilt from. */
const BLANK = " ";

/**
 * Blank the CONTENTS of every string (`'…'`, `"…"`) and template literal
 * (`` `…` ``) in an already comment-free source, preserving the delimiters, the
 * total length, and every line break, and keeping `${…}` template-interpolation
 * CODE live (only the literal text around an interpolation is blanked). This is the
 * code-only companion to {@link stripComments}: with literal interiors blanked, no
 * literal can spell a CODE CONSTRUCT — a type name, a `declare const x: T`, a
 * `defineProps<T>`, an `interface` body — so owner-span discovery, owner-LINK
 * matching, and the code (non-string-aware) bindings cannot be satisfied by text
 * that survives only inside a literal. It is length- and line-preserving, so a span
 * discovered in this view addresses the SAME range in the string-preserving view —
 * that aligned literal span is what feeds the string-aware bindings (the emit event
 * names). Comments are already gone before this runs, so a comment opener inside a
 * literal is moot and a quote the comment scanner already consumed never reaches
 * here. A controlled scanner for the curated corpus, not a general lexer.
 */
function neutralizeLiterals(source: string): string {
  let out = "";
  for (let i = 0; i < source.length; ) {
    const c = source[i];
    // Single- or double-quoted string: blank through the matching close (honoring
    // escapes), keeping the delimiters; a hard line break ends an unterminated quote.
    if (c === '"' || c === "'") {
      out += c;
      i += 1;
      while (i < source.length) {
        const d = source[i];
        if (d === "\\" && i + 1 < source.length) {
          out += BLANK + BLANK; // escape + target are literal content, not code
          i += 2;
          continue;
        }
        if (d === c) {
          out += d; // closing delimiter
          i += 1;
          break;
        }
        if (d === "\n" || d === "\r") {
          out += d; // an unterminated quote ends at the line break; keep it
          i += 1;
          break;
        }
        out += BLANK;
        i += 1;
      }
      continue;
    }
    // Template literal: blank the literal text, keep `${…}` interpolation code live.
    if (c === "`") {
      out += c;
      i += 1;
      while (i < source.length) {
        const d = source[i];
        if (d === "\\" && i + 1 < source.length) {
          out += BLANK + BLANK;
          i += 2;
          continue;
        }
        if (d === "`") {
          out += d; // closing delimiter
          i += 1;
          break;
        }
        if (d === "$" && source[i + 1] === "{") {
          // Interpolation: copy `${`, then the code verbatim through its matching
          // `}` (tracking nested braces), then resume blanking the literal text.
          out += "${";
          i += 2;
          let depth = 1;
          while (i < source.length && depth > 0) {
            const e = source[i];
            if (e === "{") depth += 1;
            else if (e === "}") depth -= 1;
            out += e;
            i += 1;
          }
          continue;
        }
        out += d === "\n" || d === "\r" ? d : BLANK; // keep line breaks
        i += 1;
      }
      continue;
    }
    out += c;
    i += 1;
  }
  return out;
}

/** True iff a regex deliberately matches literal CONTENT — its source carries a string
 * delimiter (`"`, `'`, or `` ` ``). Routes BINDINGS only: a string-aware binding (the
 * emit event-name discriminant) reads its owner declaration's aligned literal span,
 * every other (code-construct) binding the neutralized span, where no literal can stand
 * in for it. Owner LINKS are routed by {@link OwnerLink.kind} (see {@link
 * ownerLinkPresent}), not by this helper. */
function isStringAware(re: RegExp): boolean {
  return /["'`]/.test(re.source);
}

/**
 * The inner body of a `.vue` SFC's single `<script>` block — the region the
 * declaration-owning macros live in. Owner-span discovery runs here so the `.vue`
 * template region (markup, `@event="handler"` bindings, `{{ }}` interpolations) can
 * never satisfy a binding as non-declaration text.
 */
function scriptBody(sfc: string): string {
  const m = sfc.match(/<script\b[^>]*>([\s\S]*?)<\/script>/);
  return m ? m[1] : "";
}

/**
 * One owner-declaration form for a family/side: a GLOBAL pattern whose every match is
 * a span a binding may legitimately match within, plus the EXACT number of times it
 * must occur. A duplicate or stale shadow of an owner declaration changes the count
 * and fails — and because the matched spans are the only text bindings are checked
 * against, binding text surviving in a comment, literal, template region, or
 * unrelated declaration cannot satisfy the parity check.
 */
interface OwnerPattern {
  readonly pattern: RegExp;
  readonly count: number;
}

/**
 * The region an owner LINK can legitimately occupy, which fixes the view it is matched
 * against — never whole-source. `code`: a declaration/use construct, matched in the
 * NEUTRALIZED code (the `<script>` body for `.vue`, all code for `.ts`), so a literal
 * spelling the construct cannot stand in for it. `template-attr`: an `@event="handler"`
 * / `v-bind="$attrs"` attribute, matched only inside a start tag of the stripped
 * `<template>` body (string-preserving, so the attribute VALUE is real). `import-stmt`:
 * an import specifier, matched only on the aligned literal span of a real import
 * declaration discovered over neutralized code, so a script string can neither forge an
 * import nor stand in for the specifier.
 */
type OwnerLinkKind = "code" | "template-attr" | "import-stmt";

/**
 * One owner LINK: the macro/declaration/markup that wires the owner type to the family's
 * probe, tagged with the region it lives in. An owner that stops being passed to its
 * macro (`.vue`), used by its `declare` (`.ts`), bound in its template, or imported
 * fails even while the scoped declaration text still reads correctly. Like a binding, a
 * link is matched ONLY inside the region it can legitimately exist in (see {@link
 * ownerLinkPresent}), never against whole-source.
 */
interface OwnerLink {
  readonly kind: OwnerLinkKind;
  readonly pattern: RegExp;
}

/** One side (`.vue` or `.ts`) of a family's owner-declaration scope. */
interface SideScope {
  /** The owner declaration form(s) whose match text a binding must fall inside. */
  readonly owners: readonly OwnerPattern[];
  /**
   * Owner LINKS: each must be present in the region its {@link OwnerLink.kind} names, so
   * the owner stays wired to the family's probe. No link is matched against whole-source
   * — a `code` link reads the neutralized `<script>`/code, a `template-attr` link reads
   * a `<template>` start tag, an `import-stmt` link reads a real import declaration's
   * aligned literal span.
   */
  readonly links: readonly OwnerLink[];
}

/** A family's owner-declaration scope on both sides. */
interface FamilyScope {
  readonly vue: SideScope;
  readonly ts: SideScope;
}

/**
 * Per-family OWNER-DECLARATION scopes. Comment stripping alone is necessary but not
 * complete: each {@link FAMILY_BINDINGS} regex is matched ONLY inside the exact owner
 * declaration/use span(s) named here — the `interface`/macro-generic body that
 * declares the members plus the macro/`declare` site that binds them — never against
 * the whole file. Each owner form pins its occurrence count (a duplicate or stale
 * shadow declaration trips it), and `links` assert the owner stays wired to the
 * family's probe. Keyed by fixture basename, 1:1 with {@link FAMILY_BINDINGS}.
 */
const FAMILY_SCOPES: Record<string, FamilyScope> = {
  "define-props": {
    vue: {
      owners: [{ pattern: /\binterface\s+DrawerProps\s*\{[^{}]*\}/g, count: 1 }],
      links: [{ kind: "code", pattern: /\bdefineProps\s*<\s*DrawerProps\s*>/ }],
    },
    ts: {
      owners: [{ pattern: /\binterface\s+DrawerProps\s*\{[^{}]*\}/g, count: 1 }],
      links: [{ kind: "code", pattern: /\bdeclare\s+const\s+props\s*:\s*DrawerProps\b/ }],
    },
  },
  "define-emits": {
    vue: {
      owners: [{ pattern: /\bdefineEmits\s*<\s*\{[^{}]*\}\s*>/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\bemit\s*=\s*defineEmits\b/ },
        { kind: "code", pattern: /\btrigger\s*=\s*emit\b/ },
      ],
    },
    ts: {
      owners: [{ pattern: /\bdeclare\s+function\s+emit\b[^;]*;/g, count: 2 }],
      links: [{ kind: "code", pattern: /\btrigger\s*=\s*emit\b/ }],
    },
  },
  "define-model": {
    vue: {
      owners: [{ pattern: /\bdefineModel\s*<\s*boolean\s*>/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\bopen\s*=\s*defineModel\b/ },
        { kind: "code", pattern: /\bisOpen\s*=\s*open\s*\.\s*value\b/ },
      ],
    },
    ts: {
      owners: [{ pattern: /\bdeclare\s+const\s+open\s*:\s*ModelRef\b[^;]*;/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\binterface\s+ModelRef\b/ },
        { kind: "code", pattern: /\bisOpen\s*=\s*open\s*\.\s*value\b/ },
      ],
    },
  },
  slots: {
    vue: {
      owners: [{ pattern: /\bdefineSlots\s*<\s*\{(?:[^{}]|\{[^{}]*\})*\}\s*>/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\bslots\s*=\s*defineSlots\b/ },
        { kind: "code", pattern: /\brenderDefault\s*=\s*slots\s*\.\s*default\b/ },
      ],
    },
    ts: {
      owners: [{ pattern: /\binterface\s+DrawerSlots\s*\{(?:[^{}]|\{[^{}]*\})*\}/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\bdeclare\s+const\s+slots\s*:\s*DrawerSlots\b/ },
        { kind: "code", pattern: /\brenderDefault\s*=\s*slots\s*\.\s*default\b/ },
      ],
    },
  },
  "template-ref": {
    vue: {
      owners: [{ pattern: /\bconst\s+inputRef\s*=\s*ref\s*<[^>]*>\s*\([^)]*\)/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\bel\s*=\s*inputRef\s*\.\s*value\b/ },
        { kind: "import-stmt", pattern: /import\s*\{[^}]*\bref\b[^}]*\}\s*from\s*["']vue["']/ },
      ],
    },
    ts: {
      owners: [
        { pattern: /\binterface\s+TemplateRef\s*<[^>]*>\s*\{[^{}]*\}/g, count: 1 },
        { pattern: /\bdeclare\s+const\s+inputRef\s*:\s*TemplateRef\b[^;]*;/g, count: 1 },
      ],
      links: [{ kind: "code", pattern: /\bel\s*=\s*inputRef\s*\.\s*value\b/ }],
    },
  },
  "fallthrough-attrs": {
    vue: {
      owners: [{ pattern: /\binterface\s+FallthroughAttrs\s*\{[^{}]*\}/g, count: 1 }],
      links: [
        { kind: "code", pattern: /\battrs\s*=\s*useAttrs\s*\(\s*\)\s*as\s+FallthroughAttrs\b/ },
        {
          kind: "import-stmt",
          pattern: /import\s*\{[^}]*\buseAttrs\b[^}]*\}\s*from\s*["']vue["']/,
        },
        { kind: "template-attr", pattern: /v-bind\s*=\s*["']\$attrs["']/ },
      ],
    },
    ts: {
      owners: [{ pattern: /\binterface\s+FallthroughAttrs\s*\{[^{}]*\}/g, count: 1 }],
      links: [{ kind: "code", pattern: /\bdeclare\s+const\s+attrs\s*:\s*FallthroughAttrs\b/ }],
    },
  },
  "auto-import-shape": {
    vue: {
      owners: [
        { pattern: /\bconst\s+counter\s*=\s*ref\s*\([^)]*\)/g, count: 1 },
        { pattern: /\bconst\s+current\s*=\s*counter\s*\.\s*value\b/g, count: 1 },
      ],
      links: [
        { kind: "import-stmt", pattern: /import\s*\{[^}]*\bref\b[^}]*\}\s*from\s*["']vue["']/ },
      ],
    },
    ts: {
      owners: [
        { pattern: /\bconst\s+counter\s*=\s*ref\s*\([^)]*\)/g, count: 1 },
        { pattern: /\bconst\s+current\s*=\s*counter\s*\.\s*value\b/g, count: 1 },
      ],
      links: [{ kind: "code", pattern: /\bdeclare\s+function\s+ref\b/ }],
    },
  },
  "event-args": {
    vue: {
      owners: [
        { pattern: /\bfunction\s+onClick\s*\([^)]*\)/g, count: 1 },
        { pattern: /\bfunction\s+onKeydown\s*\([^)]*\)/g, count: 1 },
      ],
      links: [
        { kind: "template-attr", pattern: /@click\s*=\s*["']onClick["']/ },
        { kind: "template-attr", pattern: /@keydown\s*=\s*["']onKeydown["']/ },
      ],
    },
    ts: {
      owners: [
        { pattern: /\bdeclare\s+function\s+onClick\b[^;]*;/g, count: 1 },
        { pattern: /\bdeclare\s+function\s+onKeydown\b[^;]*;/g, count: 1 },
      ],
      links: [
        { kind: "code", pattern: /\bonClick\s*\(\s*\(\s*event\s*\)\s*=>/ },
        { kind: "code", pattern: /\bonKeydown\s*\(\s*\(\s*event\s*\)\s*=>/ },
      ],
    },
  },
};

/**
 * The two aligned views of one side's source. {@link SideView.code} blanks every
 * literal interior (a code construct can never hide in a literal); {@link
 * SideView.lit} preserves them (the string-aware checks read it). They are the same
 * length, so a span found in `code` addresses the same range in `lit`.
 */
interface SideView {
  readonly code: string;
  readonly lit: string;
}

/** Comment-strip a raw source, then derive its neutralized + preserved views. */
function sideView(raw: string): SideView {
  const lit = stripComments(raw);
  return { code: neutralizeLiterals(lit), lit };
}

/** One owner-declaration span in BOTH aligned views (same range, `code` blanked). */
interface OwnerSpan {
  readonly code: string;
  readonly lit: string;
}

/**
 * Discover the owner-declaration span(s) one side contributes, matching each owner
 * form over the NEUTRALIZED code (so a literal can neither BE an owner nor inflate
 * its count) and capturing the aligned literal span for the string-aware bindings.
 * Records a violation per owner form whose occurrence count is wrong (a duplicate or
 * stale shadow declaration), so the discovered spans are the only region a binding is
 * matched against AND their multiplicity is pinned.
 */
function discoverOwnerSpans(
  label: string,
  view: SideView,
  isVue: boolean,
  owners: readonly OwnerPattern[],
  violations: string[],
): OwnerSpan[] {
  // `.vue` owners live in the `<script>` body; `.ts` is all code. Both views are
  // sliced the same way, so their bodies stay length-aligned.
  const code = isVue ? scriptBody(view.code) : view.code;
  const lit = isVue ? scriptBody(view.lit) : view.lit;
  const spans: OwnerSpan[] = [];
  for (const { pattern, count } of owners) {
    const found = [...code.matchAll(pattern)];
    if (found.length !== count) {
      violations.push(
        `${label}: expected ${count} owner span(s) for /${pattern.source}/, found ${found.length}`,
      );
    }
    for (const m of found) {
      const start = m.index ?? 0;
      spans.push({ code: m[0], lit: lit.slice(start, start + m[0].length) });
    }
  }
  return spans;
}

/** A binding to check on one side, paired with the human-readable failure message. */
interface SideBinding {
  readonly what: string;
  readonly re: RegExp;
}

/**
 * The inner markup of a `.vue` SFC's single `<template>` block — the region where
 * template-attribute owner links (`@click="onClick"`, `v-bind="$attrs"`) live. Read from
 * the string-preserving view so the attribute VALUE survives. A controlled scanner for
 * the curated corpus, not a general lexer.
 */
function templateBody(sfc: string): string {
  const m = sfc.match(/<template\b[^>]*>([\s\S]*?)<\/template>/);
  return m ? m[1] : "";
}

/**
 * Every element START TAG in a markup region (`<button …>`, `<input … />`) — never a
 * closing tag (`</button>`) or raw text. A template-attribute link is tested against
 * these spans alone, so a `@event=` spelled in raw template text cannot satisfy it. A
 * controlled scanner for the curated corpus, not a general lexer.
 */
function startTags(markup: string): string[] {
  return markup.match(/<[A-Za-z][\w:.-]*(?:\s[^<>]*)?>/g) ?? [];
}

/**
 * The real import DECLARATIONS of one side's source, as their aligned string-preserving
 * spans. Spans are discovered over the NEUTRALIZED code (so a script string can never
 * forge an `import`), then sliced from the length-aligned literal view (so the real
 * specifier survives the link test) — the same alignment contract as {@link sideView}
 * and owner-span slicing. `.vue` imports live in the `<script>` body; `.ts` is all code.
 * A controlled scanner for the curated corpus, not a general lexer.
 */
function importStmtSpans(view: SideView, isVue: boolean): string[] {
  const code = isVue ? scriptBody(view.code) : view.code;
  const lit = isVue ? scriptBody(view.lit) : view.lit;
  const spans: string[] = [];
  for (const m of code.matchAll(/^\s*import\s+(?!\()[\s\S]*?\bfrom\s*["'][^"'\r\n]*["']\s*;?/gm)) {
    const start = m.index ?? 0;
    spans.push(lit.slice(start, start + m[0].length));
  }
  return spans;
}

/**
 * Whether an owner LINK is present in the ONLY region it can legitimately occupy — the
 * region its {@link OwnerLink.kind} names, never whole-source. `code`: the neutralized
 * code (the `<script>` body for `.vue`, all code for `.ts`), so a literal cannot stand in
 * for the construct. `template-attr`: any start tag of the stripped `<template>` body, so
 * only a real markup attribute (not a script string) satisfies it. `import-stmt`: any
 * real import declaration's aligned literal span, so a script string can neither forge an
 * import nor supply the specifier.
 */
function ownerLinkPresent(link: OwnerLink, view: SideView, isVue: boolean): boolean {
  switch (link.kind) {
    case "code":
      return link.pattern.test(isVue ? scriptBody(view.code) : view.code);
    case "template-attr":
      return startTags(templateBody(view.lit)).some((tag) => link.pattern.test(tag));
    case "import-stmt":
      return importStmtSpans(view, isVue).some((span) => link.pattern.test(span));
  }
}

/**
 * Evaluate one side (`.vue` or `.ts`) of a family, returning a violation message per
 * failed owner-count, owner-link, or binding. Each owner link is matched only inside the
 * region its {@link OwnerLink.kind} names (see {@link ownerLinkPresent}); a string-aware
 * BINDING (the emit event-name discriminant) reads the aligned literal span of its owner
 * declaration, every other binding the neutralized span.
 */
function evaluateSide(
  label: string,
  view: SideView,
  isVue: boolean,
  scope: SideScope,
  bindings: readonly SideBinding[],
): string[] {
  const violations: string[] = [];
  const spans = discoverOwnerSpans(label, view, isVue, scope.owners, violations);

  for (const link of scope.links) {
    if (!ownerLinkPresent(link, view, isVue))
      violations.push(`${label}: missing owner link /${link.pattern.source}/`);
  }

  for (const { what, re } of bindings) {
    const aware = isStringAware(re);
    const matched = spans.some((span) => re.test(aware ? span.lit : span.code));
    if (!matched) violations.push(`${label}: ${what}`);
  }
  return violations;
}

/**
 * The structural name→type binding-parity violations for one family across both
 * sides — empty iff every owner declaration occurs exactly once (or twice, where
 * declared), every owner link is wired, and every name→type binding holds on the
 * ACTUAL declarations. A drift that survives only in a comment, a string/template
 * literal, template text, an unrelated declaration, or a duplicate owner declaration
 * yields a non-empty result.
 */
function familyParityViolations(base: string, vueRaw: string, tsRaw: string): string[] {
  const scope = FAMILY_SCOPES[base];
  const bindings = FAMILY_BINDINGS[base];
  return [
    ...evaluateSide(
      `${base}.vue`,
      sideView(vueRaw),
      true,
      scope.vue,
      bindings.map((b) => ({ what: b.what, re: b.vue })),
    ),
    ...evaluateSide(
      `${base}.ts`,
      sideView(tsRaw),
      false,
      scope.ts,
      bindings.map((b) => ({ what: b.what, re: b.ts })),
    ),
  ];
}

describe("hermetic fixture corpus — layout", () => {
  it("commits all seven default corpus members, each with at least one `.vue`", () => {
    for (const fixture of FIXTURES) {
      const dir = joinCanonical(HERMETIC, fixture);
      expect(existsSync(dir), `${fixture} present`).toBe(true);
      const vues = walk(dir).filter((f) => f.endsWith(".vue"));
      expect(vues.length, `${fixture} has a .vue`).toBeGreaterThan(0);
    }
  });
});

describe("hermetic fixture corpus — anchors strip and land on their target token", () => {
  for (const [fixture, files] of Object.entries(ANCHOR_CONTRACT)) {
    describe(fixture, () => {
      it("leaves NO `@dx-anchor` residue anywhere after stripping", () => {
        const { stripped } = fixtureAnchors(fixture);
        for (const [rel, text] of stripped) {
          expect(text, `${rel} stripped`).not.toContain("@dx-anchor");
        }
      });

      for (const [file, anchors] of Object.entries(files)) {
        for (const [anchor, expectation] of Object.entries(anchors)) {
          it(`${file} :: ${anchor} lands on its target`, () => {
            const { anchors: map } = fixtureAnchors(fixture);
            const pos = map.get(anchor);
            expect(pos, `anchor "${anchor}" exists`).toBeDefined();
            expect(pos!.file, `anchor "${anchor}" owned by ${file}`).toBe(file);

            const { stripped } = fixtureAnchors(fixture);
            const allLines = lines(stripped.get(file)!);
            const lineText = allLines[pos!.line] ?? "";
            if ("before" in expectation) {
              expect(
                nextToken(allLines, pos!.line, pos!.character).startsWith(expectation.before),
              ).toBe(true);
            } else if ("after" in expectation) {
              expect(lastIdentifier(lineText.slice(0, pos!.character))).toBe(expectation.after);
            } else if ("lineContains" in expectation) {
              expect(lineText).toContain(expectation.lineContains);
            } else {
              expect(lineText.trim()).toBe("");
              expect(pos!.character).toBe(0);
            }
          });
        }
      }
    });
  }
});

describe("hermetic fixture corpus — per-region recompute and EOL independence", () => {
  it("a template-region strip does NOT shift a later script-region anchor's column", () => {
    // `template-events` puts a `<template>` region (with stripped HTML-comment
    // anchors) ABOVE its `<script>` region. If the stripper applied one global
    // offset correction, the script anchor's column would be corrupted; per-region
    // recompute keeps it at the END of its own (code-trailing) line.
    const { anchors, stripped } = fixtureAnchors("template-events");
    const tpl = anchors.get("evt.click")!;
    const script = anchors.get("evt.handlerArg")!;
    expect(tpl.line, "template anchor precedes the script anchor").toBeLessThan(script.line);
    const scriptLine = lines(stripped.get("App.vue")!)[script.line];
    // Code-trailing anchor: it sits at the very END of its own line, with `event`
    // the last identifier before it (the canonical formatter's statement-terminating
    // `;` is the only thing between). A bled-in global offset would not land it here.
    expect(lastIdentifier(scriptLine.slice(0, script.character))).toBe("event");
    expect(script.character).toBe(scriptLine.length);
  });

  it("yields identical anchor positions for CRLF and its LF twin (every fixture)", () => {
    for (const fixture of FIXTURES) {
      const dir = joinCanonical(HERMETIC, fixture);
      for (const rel of walk(dir)) {
        if (!TEXT_SOURCE.test(rel)) continue;
        const lf = readFileSync(joinCanonical(dir, rel), "utf-8").replace(/\r\n/g, "\n");
        const crlf = lf.replace(/\n/g, "\r\n");
        const a = stripAnchors(lf).anchors;
        const b = stripAnchors(crlf).anchors;
        expect([...b.entries()], `${fixture}/${rel} CRLF==LF`).toEqual([...a.entries()]);
      }
    }
  });
});

describe("drawer-realistic — construct completeness", () => {
  it("includes every required Drawer construct (a dropped one fails this gate)", () => {
    const drawer = readFileSync(joinCanonical(HERMETIC, "drawer-realistic", "Drawer.vue"), "utf-8");
    const required = [
      "defineProps",
      "defineEmits",
      "defineModel",
      "withDefaults",
      "computed",
      "ref",
      "Teleport",
      "Transition",
      "@click",
      "@click.stop",
      "@keydown.esc",
      "<slot",
    ];
    for (const token of required) {
      expect(drawer, `Drawer.vue contains ${token}`).toContain(token);
    }
    // The overlay `@click` must be the plain (un-stopped) form, distinct from the
    // panel's `@click.stop` — i.e. both an exact `@click=` and a `@click.stop` exist.
    expect(drawer).toMatch(/@click="/);
    expect(drawer).toMatch(/@click\.stop/);
  });

  it("ships the nested DrawerHeader.vue and a barrel that re-exports both", () => {
    expect(existsSync(joinCanonical(HERMETIC, "drawer-realistic", "DrawerHeader.vue"))).toBe(true);
    const drawer = readFileSync(joinCanonical(HERMETIC, "drawer-realistic", "Drawer.vue"), "utf-8");
    expect(drawer, "Drawer imports the nested header").toContain("DrawerHeader.vue");
    const barrel = readFileSync(joinCanonical(HERMETIC, "drawer-realistic", "index.ts"), "utf-8");
    expect(barrel).toContain("Drawer.vue");
    expect(barrel).toContain("DrawerHeader.vue");
    expect(barrel).toMatch(/export\s/);
  });
});

describe("hermetic fixture corpus — hermeticity", () => {
  it("contains NO node_modules dir and NO fixture-local install script", () => {
    const stack = [HERMETIC];
    let sawPackageJson = false;
    while (stack.length > 0) {
      const dir = stack.pop()!;
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = joinCanonical(dir, entry.name);
        if (entry.isDirectory()) {
          // A vendored dependency tree inside a fixture would break hermeticity.
          expect(entry.name, `${full} is not node_modules`).not.toBe("node_modules");
          stack.push(full);
        } else if (entry.name === "package.json") {
          sawPackageJson = true;
          const pkg = JSON.parse(readFileSync(full, "utf-8")) as {
            scripts?: Record<string, string>;
          };
          for (const hook of ["install", "preinstall", "postinstall", "prepare"]) {
            expect(pkg.scripts?.[hook], `${full} has no ${hook} script`).toBeUndefined();
          }
        }
      }
    }
    // The corpus is fully committed: no fixture-local install step is needed, so
    // the default corpus carries no package.json install hooks at all.
    expect(sawPackageJson, "default corpus needs no fixture-local package.json").toBe(false);
  });

  it("is unaffected by the external-corpus env flag (default corpus is always committed)", () => {
    // No `DX_HARNESS_EXTERNAL_CORPUS` is set in this hermetic suite, and the default
    // corpus loads purely from committed content regardless of it.
    expect(process.env.DX_HARNESS_EXTERNAL_CORPUS).toBeUndefined();
    for (const fixture of FIXTURES) expect(existsSync(joinCanonical(HERMETIC, fixture))).toBe(true);
  });
});

describe("semantic-oracle fixtures — paired with the `.ts` gold-standard oracles", () => {
  it("pairs every semantic-oracle `.vue` with an `oracles/semantic/*.ts` of the same family", () => {
    const vues = readdirSync(joinCanonical(HERMETIC, "semantic-oracle"))
      .filter((f) => f.endsWith(".vue"))
      .sort();
    const oracleTs = readdirSync(ORACLES)
      .filter((f) => f.endsWith(".ts"))
      .sort();
    // One `.vue` counterpart per curated oracle family; same basenames as the `.ts`.
    expect(vues.length).toBe(ORACLE_FAMILIES.length);
    expect(oracleTs.length).toBe(ORACLE_FAMILIES.length);
    const vueBases = vues.map((f) => f.replace(/\.vue$/, ""));
    const tsBases = oracleTs.map((f) => f.replace(/\.ts$/, ""));
    expect(vueBases).toEqual(tsBases);
    for (const base of vueBases) {
      expect(
        existsSync(joinCanonical(HERMETIC, "semantic-oracle", `${base}.vue`)),
        `${base}.vue`,
      ).toBe(true);
      expect(existsSync(joinCanonical(ORACLES, `${base}.ts`)), `${base}.ts`).toBe(true);
    }
  });

  it("each `.vue` counterpart references the same Vue macro/API its oracle models", () => {
    // The pairing is semantic, not just nominal: the props oracle's `.vue` uses
    // `defineProps`, the emits oracle's uses `defineEmits`, etc.
    const families: Record<string, string> = {
      "define-props": "defineProps",
      "define-emits": "defineEmits",
      "define-model": "defineModel",
      slots: "defineSlots",
      "template-ref": "ref",
      "fallthrough-attrs": "useAttrs",
      "auto-import-shape": "ref",
      "event-args": "@click",
    };
    for (const [base, token] of Object.entries(families)) {
      const vue = readFileSync(joinCanonical(HERMETIC, "semantic-oracle", `${base}.vue`), "utf-8");
      expect(vue, `${base}.vue uses ${token}`).toContain(token);
    }
  });

  /**
   * Bind each fixture name to the TYPE its paired oracle models, on BOTH sides — over
   * the ACTUAL declarations, not the whole file.
   *
   * Nominal pairing (same basename + the right macro) and even shared token-bag
   * membership are not enough: a fixture can keep every token its family uses yet
   * rebind a name to the wrong type — drop a prop's `?`, permute the `title`/`count`
   * types, move an emit payload, swap a slot's prop shape, or swap the
   * Mouse/Keyboard event args — and the token set is unchanged. {@link FAMILY_BINDINGS}
   * pins each name→type binding in both the `.vue` macro form and the `.ts`
   * declaration form, and {@link FAMILY_SCOPES} confines each match to the owner
   * declaration span. {@link familyParityViolations} enforces it: comments are
   * stripped AND string/template-literal interiors are neutralized on both sides, the
   * `.vue` template region is excluded from owner-span discovery, each owner
   * declaration is required exactly its declared number of times (a duplicate or stale
   * shadow fails), and the owner stays wired to its probe. So a drift that survives
   * only in a stale doc comment, a string/template literal, template text, an
   * unrelated declaration, or a duplicate owner declaration cannot mask the change —
   * it goes red here.
   *
   * Epistemic boundary: this is a STRUCTURAL name→type binding-parity guard over the
   * small controlled fixture text, NOT a proof of full type identity. The neutralized
   * view closes "a code construct hiding in a literal", and every owner LINK is matched
   * only inside the region it can legitimately occupy — neutralized code, a real import
   * declaration's aligned span, or a `<template>` start tag, never whole-source — so a
   * script string can no longer forge one. The one remaining string-aware read is the
   * emit event-name discriminant BINDING, which reads the aligned literal span INSIDE
   * its owner declaration by design. Exhaustive TYPE-IDENTITY parity (the fully resolved
   * types being equal) is established by the live differential — verter-on-`.vue` vs
   * tsgo/tsserver-on-`.ts` — at run time, not by this static check.
   */
  it("binds each fixture name to the type its `.ts` oracle models (structural parity)", () => {
    // Every curated family is covered, and every family with bindings has an owner
    // scope — a new family that adds neither (or only one) fails here.
    expect(Object.keys(FAMILY_BINDINGS).length).toBe(ORACLE_FAMILIES.length);
    expect(Object.keys(FAMILY_SCOPES).sort()).toEqual(Object.keys(FAMILY_BINDINGS).sort());

    for (const base of Object.keys(FAMILY_BINDINGS)) {
      const vueRaw = readFileSync(
        joinCanonical(HERMETIC, "semantic-oracle", `${base}.vue`),
        "utf-8",
      );
      const tsRaw = readFileSync(joinCanonical(ORACLES, `${base}.ts`), "utf-8");
      expect(familyParityViolations(base, vueRaw, tsRaw), `${base} structural parity`).toEqual([]);
    }
  });

  /**
   * The string/template-literal neutralizer that keeps the structural-parity guard
   * from being satisfied by a code construct hiding inside a literal. It blanks
   * literal CONTENTS while preserving the delimiters, the total length, and line
   * breaks, and keeps `${…}` interpolation code live.
   */
  describe("neutralizeLiterals — code constructs cannot survive inside a literal", () => {
    it("blanks single- and double-quoted contents but keeps the delimiters and length", () => {
      for (const src of ['const a = "declare const x: T";', "const a = 'interface Y {}';"]) {
        const out = neutralizeLiterals(src);
        expect(out.length, "length preserved").toBe(src.length);
        expect(out).not.toMatch(/declare|interface/);
        // The code OUTSIDE the literal and both delimiters survive verbatim.
        expect(out.startsWith("const a = ")).toBe(true);
        expect(out).toMatch(/(["']) {1,} *\1;$/);
      }
    });

    it("does not let an escaped quote terminate the literal early", () => {
      // `"a\"b"` is ONE string; a naive scanner would close at the escaped quote and
      // treat `b` as code. The neutralized output is a single blanked literal.
      const src = '"a\\"b"';
      expect(neutralizeLiterals(src)).toBe(`"${" ".repeat(src.length - 2)}"`);
    });

    it("blanks template text but keeps `${…}` interpolation code live", () => {
      const out = neutralizeLiterals("`aa${count.value}bb`");
      expect(out).toContain("${count.value}");
      expect(out).not.toMatch(/aa|bb/);
      expect(out.length).toBe("`aa${count.value}bb`".length);
    });

    it("preserves line breaks (offsets stay aligned with the string-preserving view)", () => {
      const src = "const a = `line1\nline2`;\nconst b = 1;";
      const out = neutralizeLiterals(src);
      expect(out.length).toBe(src.length);
      expect([...out].filter((c) => c === "\n")).toHaveLength(2);
      expect(out).not.toContain("line1");
      expect(out.endsWith("const b = 1;")).toBe(true);
    });
  });

  /**
   * The newly-closed shadow: comment stripping preserved literal CONTENTS, so an
   * owner LINK or a name→type binding could be satisfied by a stale copy hiding in a
   * string literal while the real declaration drifted. Each case below mutates the
   * REAL corpus source IN MEMORY, leaves the stale text alive in a literal (a naive
   * whole-source scan would still find it — asserted), and proves the neutralized
   * discovery now reports a violation anyway. The unmodified source stays clean.
   */
  describe("structural parity is not satisfiable from a string/template literal", () => {
    const readVue = (base: string) =>
      readFileSync(joinCanonical(HERMETIC, "semantic-oracle", `${base}.vue`), "utf-8");
    const readTs = (base: string) => readFileSync(joinCanonical(ORACLES, `${base}.ts`), "utf-8");

    it("rejects a `.ts` owner-LINK break masked by a stale string literal", () => {
      const mutated = readTs("define-props").replace(
        "declare const props: DrawerProps;",
        'declare const props: OtherProps;\nconst _shadow = "declare const props: DrawerProps";',
      );
      // The old link text literally survives (inside the string) — naive scan fooled.
      expect(mutated).toContain("declare const props: DrawerProps");
      const violations = familyParityViolations("define-props", readVue("define-props"), mutated);
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a `.vue` owner-LINK break masked by a stale string literal", () => {
      const mutated = readVue("define-props").replace(
        "const props = defineProps<DrawerProps>();",
        'const props = defineProps<WrongProps>();\nconst _s = "defineProps<DrawerProps>";',
      );
      expect(mutated).toContain("defineProps<DrawerProps>");
      const violations = familyParityViolations("define-props", mutated, readTs("define-props"));
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a binding drift hidden by a string literal INSIDE the owner span", () => {
      const mutated = readVue("define-props").replace(
        "  count?: number;\n",
        '  count: number;\n  _shadow: "count?: number";\n',
      );
      expect(mutated).toContain('"count?: number"');
      const violations = familyParityViolations("define-props", mutated, readTs("define-props"));
      expect(violations.some((v) => v.includes("stays optional"))).toBe(true);
    });

    it("keeps the emit event-name binding GREEN and reds a moved-payload drift", () => {
      // The emit `.ts` binding legitimately reads the event-name strings — they must
      // survive neutralization via the aligned literal span. Unmodified: clean.
      expect(
        familyParityViolations("define-emits", readVue("define-emits"), readTs("define-emits")),
      ).toEqual([]);
      // Move the payload submit→close: the preserved literal span still exposes the
      // (now wrong) event→payload wiring, so both emit bindings go red.
      const moved = readTs("define-emits")
        .replace('emit(event: "submit", value: string)', 'emit(event: "TMP", value: string)')
        .replace('emit(event: "close")', 'emit(event: "submit")')
        .replace('emit(event: "TMP", value: string)', 'emit(event: "close", value: string)');
      const violations = familyParityViolations("define-emits", readVue("define-emits"), moved);
      expect(violations.some((v) => v.includes("submit carries"))).toBe(true);
      expect(violations.some((v) => v.includes("close carries no payload"))).toBe(true);
    });

    it("keeps the import owner link GREEN (matched on a real import declaration's span)", () => {
      // An `import-stmt` link reads the aligned literal span of a real import
      // declaration, so the genuine `from "vue"` import satisfies it; dropping the
      // import declaration outright reds it.
      expect(
        familyParityViolations(
          "auto-import-shape",
          readVue("auto-import-shape"),
          readTs("auto-import-shape"),
        ),
      ).toEqual([]);
      const noImport = readVue("auto-import-shape").replace('import { ref } from "vue";', "");
      const violations = familyParityViolations(
        "auto-import-shape",
        noImport,
        readTs("auto-import-shape"),
      );
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });
  });

  /**
   * Owner LINKS are matched ONLY inside the region they can legitimately occupy — a
   * `code` link in the neutralized `<script>`/code, a `template-attr` link in a
   * `<template>` start tag, an `import-stmt` link on a real import declaration's aligned
   * literal span — never against whole-source. So a real link broken in its region while
   * a stale copy lingers in a SCRIPT STRING (which a whole-source scan of the
   * string-preserving view would still find — asserted) is reported anyway, and the real
   * markup attributes and imports still satisfy their links unmodified.
   */
  describe("owner links are region-scoped, not whole-source (string-shadow closed)", () => {
    const readVue = (base: string) =>
      readFileSync(joinCanonical(HERMETIC, "semantic-oracle", `${base}.vue`), "utf-8");
    const readTs = (base: string) => readFileSync(joinCanonical(ORACLES, `${base}.ts`), "utf-8");

    it("matches the real template attrs and imports unmodified (no region-scoped false miss)", () => {
      // The genuine `@click`/`@keydown`/`v-bind="$attrs"` start-tag attrs and the real
      // `from "vue"` imports satisfy their region-scoped links: every link family stays
      // green unmodified.
      for (const base of ["event-args", "fallthrough-attrs", "template-ref", "auto-import-shape"]) {
        expect(
          familyParityViolations(base, readVue(base), readTs(base)),
          `${base} unmodified`,
        ).toEqual([]);
      }
    });

    it("rejects a @click template-attr LINK break masked by a stale script string", () => {
      const mutated = readVue("event-args")
        .replace('@click="onClick"', '@click="onWrong"')
        .replace("void clickEvent;", "void clickEvent;\n  const _shadow = '@click=\"onClick\"';");
      // The real attribute is gone from the start tag; the old text survives only as a
      // SCRIPT STRING (a whole-source scan of the preserving view would still find it).
      expect(mutated).toContain('@click="onClick"');
      expect(mutated).toContain('@click="onWrong"');
      const violations = familyParityViolations("event-args", mutated, readTs("event-args"));
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a @keydown template-attr LINK break masked by a stale script string", () => {
      const mutated = readVue("event-args")
        .replace('@keydown="onKeydown"', '@keydown="onWrong"')
        .replace("void keyEvent;", "void keyEvent;\n  const _shadow = '@keydown=\"onKeydown\"';");
      expect(mutated).toContain('@keydown="onKeydown"');
      const violations = familyParityViolations("event-args", mutated, readTs("event-args"));
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a fallthrough v-bind template-attr LINK break masked by a stale script string", () => {
      const mutated = readVue("fallthrough-attrs")
        .replace('v-bind="$attrs"', 'v-bind="$other"')
        .replace(
          "const attrs = useAttrs() as FallthroughAttrs;",
          "const attrs = useAttrs() as FallthroughAttrs;\nconst _shadow = 'v-bind=\"$attrs\"';",
        );
      expect(mutated).toContain('v-bind="$attrs"');
      const violations = familyParityViolations(
        "fallthrough-attrs",
        mutated,
        readTs("fallthrough-attrs"),
      );
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a dropped auto-import `ref` import LINK masked by a stale script string", () => {
      const mutated = readVue("auto-import-shape").replace(
        'import { ref } from "vue";',
        "const _shadow = 'import { ref } from \"vue\"';",
      );
      // The import text survives only inside a script STRING; no real import declaration
      // remains.
      expect(mutated).toContain('import { ref } from "vue"');
      expect(mutated).not.toMatch(/^\s*import\s/m);
      const violations = familyParityViolations(
        "auto-import-shape",
        mutated,
        readTs("auto-import-shape"),
      );
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a dropped fallthrough `useAttrs` import LINK masked by a stale script string", () => {
      const mutated = readVue("fallthrough-attrs").replace(
        'import { useAttrs } from "vue";',
        "const _shadow = 'import { useAttrs } from \"vue\"';",
      );
      expect(mutated).toContain('import { useAttrs } from "vue"');
      expect(mutated).not.toMatch(/^\s*import\s/m);
      const violations = familyParityViolations(
        "fallthrough-attrs",
        mutated,
        readTs("fallthrough-attrs"),
      );
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });

    it("rejects a dropped template-ref `ref` import LINK masked by a stale script string", () => {
      const mutated = readVue("template-ref").replace(
        'import { ref } from "vue";',
        "const _shadow = 'import { ref } from \"vue\"';",
      );
      expect(mutated).toContain('import { ref } from "vue"');
      expect(mutated).not.toMatch(/^\s*import\s/m);
      const violations = familyParityViolations("template-ref", mutated, readTs("template-ref"));
      expect(violations.some((v) => v.includes("missing owner link"))).toBe(true);
    });
  });
});

describe("semantic-oracle pairs — `.vue` probe anchors stay paired with `.ts` oracle anchors", () => {
  it("pairs every `.vue` anchor with a same-named anchor in its `.ts` oracle", () => {
    const bases = readdirSync(joinCanonical(HERMETIC, "semantic-oracle"))
      .filter((f) => f.endsWith(".vue"))
      .map((f) => f.replace(/\.vue$/, ""))
      .sort();
    expect(bases.length, "one `.vue` per curated family").toBe(ORACLE_FAMILIES.length);
    for (const base of bases) {
      const vueSrc = readFileSync(
        joinCanonical(HERMETIC, "semantic-oracle", `${base}.vue`),
        "utf-8",
      );
      const tsSrc = readFileSync(joinCanonical(ORACLES, `${base}.ts`), "utf-8");
      const vueAnchors = [...stripAnchors(vueSrc).anchors.keys()].sort();
      const tsAnchors = [...stripAnchors(tsSrc).anchors.keys()].sort();
      // The runner queries the `.vue` probe anchor and the SAME-NAMED `.ts` oracle
      // anchor; this curated corpus pairs them by identical name. A rename on one
      // side only silently un-pairs the gold standard from its fixture — caught here.
      expect(vueAnchors.length, `${base}: has at least one anchor`).toBeGreaterThan(0);
      expect(tsAnchors, `${base}: .vue and .ts anchor names pair 1:1`).toEqual(vueAnchors);
    }
  });
});
