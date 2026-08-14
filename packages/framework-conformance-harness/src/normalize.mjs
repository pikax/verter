// Parser-backed cosmetic normalizer (conformance-normalizer.md).
//
// Produces a CANONICAL FORM of a generated module by re-parsing it to an
// ESTree AST and returning a position-free structural tree. Two programs
// whose canonical forms are deep-equal are cosmetically identical under the
// allowed normalization rules:
//
//   - whitespace/line-layout: FREE — canonical form never carries
//     start/end/loc/range, so re-indentation or reflow never affects it.
//   - quote-delimiter spelling: FREE — acorn decodes string literals to
//     their `.value`; the canonical form compares decoded values, never raw
//     source text, so `'x'` and `"x"` canonicalize identically. EXCEPTION:
//     a TAGGED template's raw spelling is NOT free — the tag function
//     receives the `.raw` array (`strings.raw`), so raw escape-sequence
//     spelling is observable program input there and enters the canonical
//     form (see the TaggedTemplateExpression case in `canonicalize`).
//   - harmless redundant parentheses: FREE — parentheses are not ESTree
//     nodes; the parser already resolves precedence, so re-parenthesizing
//     an equivalent expression produces the identical AST shape.
//   - comments WITHOUT semantic force (plain prose): FREE — only
//     tool-consumed comment classes (see `classifySemanticComment`) enter
//     the canonical form; adding or removing an ordinary explanatory
//     comment never affects it.
//   - NAMED import-specifier ORDER within ONE import declaration: FREE —
//     `import { a, b } from "x"` and `import { b, a } from "x"` bind the
//     same hoisted ESM bindings to the same module, so the slot a named
//     specifier occupies in its own declaration is not observable. The
//     named specifiers of one declaration are therefore canonicalized into
//     a deterministic content order (see the ImportDeclaration case in
//     `canonicalize`). This exception is NARROW; each of the following
//     stays STRUCTURAL and is proven so by a negative test:
//       * specifier MEMBERSHIP (adding or removing a named specifier),
//       * the IMPORTED name and the LOCAL alias of each specifier, paired,
//       * the `source` module,
//       * DEFAULT and NAMESPACE specifier form and position (at most one of
//         either may appear and the ECMAScript grammar fixes it ahead of
//         the named list, so it is never reordered and never sorted),
//       * import ATTRIBUTES (`with { type: "json" }`),
//       * the top-level ORDER of the import declarations themselves — the
//         declarations are ordinary `Program.body` items compared
//         positionally, and declaration GROUPING is not merged (the same
//         binding set split across two declarations is a difference),
//       * the SIDE-EFFECT import sequence (`import "x"` carries no
//         specifiers at all and is compared in body order like any other
//         statement).
//
// IDENTIFIERS ARE STRUCTURAL — there is NO alpha-renaming. The contract
// permits "private generated identifier spelling under scope-aware
// alpha-normalization" only for bindings with private generated provenance.
// The pinned official Vue 3.6.0-rc.3 / Svelte 5.56.8 compilers emit no
// structural provenance marker distinguishing a compiler-generated private
// binding from an authored one in their output (a leading-underscore
// spelling like `_sfc_main` is a naming convention, and inferring
// generatedness from spelling is exactly the name-based inference this
// repository's architecture rules forbid). Without explicit provenance an
// identifier must be treated as potentially authored/public-adjacent, so
// EVERY identifier participates in equality like any other token. A
// candidate that spells a local binding differently from the official
// output is structurally different — never silently equated.
//
// SEMANTIC COMMENTS ARE PRESERVED — tool-consumed comments (`/*#__PURE__*/`
// -class annotations, license/preserve blocks, source-map/sourceURL
// directives, TS directives, triple-slash references, JSDoc, JSX/bundler
// pragmas, Istanbul coverage directives, ESLint disable/enable directives,
// Prettier ignore directives) are collected,
// classified, and attached to the canonical node they precede, in order.
// Deleting one, mutating its text, or relocating it to a different
// expression/statement changes the canonical form and is caught as a
// structural difference. The classifier is deliberately over-inclusive
// toward "semantic": misclassifying a prose comment as semantic can only
// produce a false DIFFERENCE (fail closed), never a false equivalence.
//
// Everything else the contract forbids stays live BY CONSTRUCTION:
//   - import/export sources and specifiers are ordinary struct fields the
//     deep-equal walk compares verbatim — the single exception being the
//     ORDER (never the content) of one declaration's named import
//     specifiers, canonicalized as described above.
//   - helper/declaration/statement ORDER: the canonical form is a
//     positional array walk (`Program.body`, `BlockStatement.body`, …) —
//     reordering two statements changes the canonical array order.
//   - literal values, property keys, template content, element tags, prop
//     names, diagnostic text carried as string literals: compared verbatim
//     (only the `raw` quote/escape spelling is dropped).

import * as acorn from "acorn";

export function parseModule(code, sourceFileForDiagnostics) {
  const comments = [];
  const ast = acorn.parse(code, {
    ecmaVersion: "latest",
    sourceType: "module",
    locations: true,
    allowAwaitOutsideFunction: true,
    sourceFile: sourceFileForDiagnostics,
    onComment: comments,
  });
  // Non-enumerable so generic AST walks never see it as a child.
  Object.defineProperty(ast, "sourceComments", { value: comments, enumerable: false });
  return ast;
}

/**
 * Classifies a comment as a tool-consumed (semantic-force) class, or null
 * for plain prose. `type` is acorn's "Line" | "Block"; `value` is the
 * comment text without its delimiters.
 *
 * @returns {string|null} the category name, or null when cosmetic
 */
export function classifySemanticComment(type, value) {
  const trimmed = value.trim();
  // Bundler annotations: /*#__PURE__*/, /*@__PURE__*/, /*#__NO_SIDE_EFFECTS__*/, …
  if (/^[#@]__[A-Z_]+__$/.test(trimmed)) return "annotation";
  // License/preserve blocks: /*! … */, @license, @preserve, @copyright.
  if (type === "Block" && value.startsWith("!")) return "license";
  if (/@(license|preserve|copyright)\b/.test(value)) return "license";
  // Source-map directives: //# sourceMappingURL=…, //# sourceURL=…, //@ legacy form.
  if (/^[#@]\s*source(Mapping)?URL=/.test(trimmed)) return "source-map-directive";
  // TS directives: // @ts-ignore, @ts-expect-error, @ts-nocheck, @ts-check.
  if (/^@ts-(ignore|expect-error|nocheck|check)\b/.test(trimmed)) return "ts-directive";
  // Triple-slash directives: /// <reference …>, /// <amd-… > (line comment
  // value starts with the third slash).
  if (type === "Line" && /^\/\s*<(reference|amd)/.test(value)) return "triple-slash-directive";
  // JSDoc blocks: /** … */.
  if (type === "Block" && value.startsWith("*")) return "jsdoc";
  // JSX / bundler pragmas: @jsx …, @vite-ignore, webpackChunkName: … etc.
  if (/^@jsx(Runtime|ImportSource|Frag)?\b/.test(trimmed)) return "pragma";
  if (/^@vite-ignore\b/.test(trimmed)) return "pragma";
  if (/^webpack[A-Z]\w*\s*:/.test(trimmed)) return "pragma";
  // Coverage directives: /* istanbul ignore next */, ignore if/else/file —
  // consumed by Istanbul/nyc instrumentation; deleting or relocating one
  // changes which code is exempt from coverage.
  if (/^istanbul\s+ignore\b/.test(trimmed)) return "coverage-directive";
  // Lint directives: eslint-disable, eslint-disable-line,
  // eslint-disable-next-line (and the paired eslint-enable, whose removal
  // silently EXTENDS a disabled region) — consumed by ESLint.
  if (/^eslint-(disable|enable)(-next-line|-line)?\b/.test(trimmed)) return "lint-directive";
  // Format directives: prettier-ignore (and prettier-ignore-start/end) —
  // consumed by Prettier; relocation changes which node is exempt.
  if (/^prettier-ignore\b/.test(trimmed)) return "format-directive";
  return null;
}

/** Collects every positioned AST node in the tree (generic walk). */
function collectNodes(root) {
  const nodes = [];
  const stack = [root];
  while (stack.length > 0) {
    const value = stack.pop();
    if (value === null || typeof value !== "object") continue;
    if (Array.isArray(value)) {
      for (const item of value) stack.push(item);
      continue;
    }
    if (typeof value.type === "string" && typeof value.start === "number") nodes.push(value);
    for (const [key, child] of Object.entries(value)) {
      if (key === "loc" || key === "range") continue;
      stack.push(child);
    }
  }
  return nodes;
}

/**
 * Attaches each SEMANTIC comment to the canonical node it precedes: the
 * outermost node whose start is the smallest position >= the comment's end.
 * A trailing comment with no following node (e.g. a sourceMappingURL at
 * end-of-file) attaches to the Program as a trailing list. Attachment is
 * positional and deterministic, so relocating a comment to a different
 * expression/statement moves it to a different canonical node — a
 * structural difference.
 *
 * ESLint-family directives additionally record their LINE-ADJACENCY to the
 * attached node: `eslint-disable-next-line` suppresses literally the next
 * source LINE, so a blank line opened between the directive and its target
 * changes what ESLint suppresses even though the comment text and the
 * nearest-node attachment are both unchanged. Of the two viable encodings —
 * (a) the exact line-delta between the directive and its target, or (b) a
 * boolean blank-line-adjacency bit — this records (a), the exact delta
 * (`targetLineDelta` = attached node's start line minus the directive's end
 * line), because the existing attachment loop already has both positions in
 * hand and the exact delta discriminates every relocation the boolean
 * would, plus multi-blank-line changes. The delta enters the canonical form
 * for lint directives ONLY: for the other directive families the consumer
 * targets the following NODE, not the following LINE, so pure line reflow
 * around them stays cosmetic as the contract requires.
 *
 * @returns {{ leading: WeakMap<object, object[]>, trailing: object[] }}
 */
function attachSemanticComments(ast) {
  const leading = new WeakMap();
  const trailing = [];
  const semantic = (ast.sourceComments ?? [])
    .map((c) => ({
      type: c.type,
      text: c.value.trim(),
      start: c.start,
      end: c.end,
      endLine: c.loc?.end?.line,
      category: classifySemanticComment(c.type, c.value),
    }))
    .filter((c) => c.category !== null);
  if (semantic.length === 0) return { leading, trailing };

  const nodes = collectNodes(ast).filter((n) => n !== ast);
  semantic.sort((a, b) => a.start - b.start);
  for (const comment of semantic) {
    let attached = null;
    for (const node of nodes) {
      if (node.start < comment.end) continue;
      if (
        attached === null ||
        node.start < attached.start ||
        (node.start === attached.start && node.end > attached.end) // outermost on ties
      ) {
        attached = node;
      }
    }
    const record = { type: comment.type, category: comment.category, text: comment.text };
    if (attached === null) {
      trailing.push(record);
    } else {
      if (
        comment.category === "lint-directive" &&
        comment.endLine !== undefined &&
        attached.loc !== undefined
      ) {
        record.targetLineDelta = attached.loc.start.line - comment.endLine;
      }
      const list = leading.get(attached) ?? [];
      list.push(record);
      leading.set(attached, list);
    }
  }
  return { leading, trailing };
}

/**
 * @returns {{ tree: object }} the position-free canonical structural tree
 */
export function canonicalize(ast) {
  const { leading, trailing } = attachSemanticComments(ast);

  // Attaches the AST node's leading semantic comments to a canonical node
  // built for it — shared by the generic walk and manually-built nodes.
  function withComments(astNode, out) {
    const comments = leading.get(astNode);
    if (comments !== undefined) out.semanticComments = comments;
    return out;
  }

  function visit(node) {
    if (node === null || typeof node !== "object") return node;
    if (Array.isArray(node)) return node.map(visit);

    let out;
    switch (node.type) {
      case "Literal":
        // Quote-delimiter and escape spelling are cosmetic: compare the
        // DECODED value only, never `raw`.
        out = { type: "Literal", value: node.value, regex: node.regex, bigint: node.bigint };
        break;
      case "TemplateElement":
        // Same rationale for ORDINARY (untagged) template literals: no
        // receiver can observe `raw`, so only the decoded `cooked` value is
        // structural and the escape spelling stays cosmetic. Elements of a
        // TAGGED template never reach this case — they are canonicalized by
        // the TaggedTemplateExpression case below, WITH `raw`.
        out = { type: "TemplateElement", tail: node.tail, cooked: node.value?.cooked };
        break;
      case "TaggedTemplateExpression": {
        // TAGGED templates are different from ordinary ones: the tag
        // function receives the raw spellings too (`strings.raw`), so a
        // raw-only change (cooked value identical) is observable program
        // input — String.raw`a\u0041b` returns the 8-char string
        // "a\u0041b" while String.raw`aAb` returns "aAb", though both
        // COOK to "aAb". `raw` enters the canonical form here ONLY.
        // Expressions interpolated into the tagged template (including any
        // nested untagged template inside them) canonicalize normally.
        const quasi = node.quasi;
        out = {
          type: "TaggedTemplateExpression",
          tag: visit(node.tag),
          quasi: withComments(quasi, {
            type: "TemplateLiteral",
            quasis: quasi.quasis.map((element) =>
              withComments(element, {
                type: "TemplateElement",
                tail: element.tail,
                cooked: element.value?.cooked,
                raw: element.value?.raw,
              }),
            ),
            expressions: quasi.expressions.map((expression) => visit(expression)),
          }),
        };
        break;
      }
      case "ImportDeclaration": {
        // Only the ORDER of the NAMED specifiers is canonicalized; every
        // other field (source, attributes, each specifier's own content)
        // goes through the ordinary walk untouched. See the header.
        out = {};
        for (const [key, value] of Object.entries(node)) {
          if (key === "start" || key === "end" || key === "loc" || key === "range") continue;
          out[key] =
            key === "specifiers" && Array.isArray(value)
              ? canonicalImportSpecifiers(value)
              : visit(value);
        }
        break;
      }
      default: {
        out = {};
        for (const [key, value] of Object.entries(node)) {
          if (key === "start" || key === "end" || key === "loc" || key === "range") continue;
          out[key] = visit(value);
        }
      }
    }
    return withComments(node, out);
  }

  /**
   * Canonical specifier list of ONE import declaration: any
   * default/namespace specifier keeps its exact original slot (the
   * ECMAScript grammar admits at most one of either and fixes it ahead of
   * the named list, so this leading run is never permutable in valid
   * source), and the named specifiers that follow are sorted into a
   * deterministic order.
   *
   * The sort key is the specifier's FULL canonical content — imported name,
   * local alias, and any attached semantic comments — so the sort is total
   * and content-complete: a permutation yields an identical sorted list,
   * while adding, removing, or changing ANY specifier changes the multiset
   * and therefore the sorted list. Two specifiers are never merged.
   */
  function canonicalImportSpecifiers(specifiers) {
    const leading = [];
    const named = [];
    for (const specifier of specifiers) {
      (specifier?.type === "ImportSpecifier" ? named : leading).push(visit(specifier));
    }
    if (named.length > 1) {
      const keys = new Map(named.map((canonical) => [canonical, stableStringify(canonical)]));
      named.sort((a, b) => {
        const keyA = keys.get(a);
        const keyB = keys.get(b);
        return keyA < keyB ? -1 : keyA > keyB ? 1 : 0;
      });
    }
    return [...leading, ...named];
  }

  const tree = visit(ast);
  if (trailing.length > 0) tree.trailingSemanticComments = trailing;
  return { tree };
}

export function deepEqualCanonical(a, b) {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (a === null || b === null) return a === b;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return false;
    return a.every((item, i) => deepEqualCanonical(item, b[i]));
  }
  if (typeof a === "object") {
    const keysA = Object.keys(a).sort();
    const keysB = Object.keys(b).sort();
    if (keysA.length !== keysB.length || keysA.some((k, i) => k !== keysB[i])) return false;
    return keysA.every((k) => deepEqualCanonical(a[k], b[k]));
  }
  return a === b;
}

/** Digest of the canonical tree — used as the normalized-golden digest. */
export function canonicalDigest(canonicalTree) {
  return stableStringify(canonicalTree);
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(value[k])}`).join(",")}}`;
}

/** Versioned identity for this normalizer's behavior — bump on any rule change. */
export const NORMALIZER_VERSION = 6;
