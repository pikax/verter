// Parser-backed cosmetic normalizer (conformance-normalizer.md).
//
// Canonical form of a generated module: re-parse to an ESTree AST and
// return a position-free structural tree. Deep-equal canonical forms are
// cosmetically identical under the allowed rules:
//
//   - whitespace/line-layout: free — no start/end/loc/range.
//   - quote-delimiter spelling: free — acorn compares decoded `.value`, so
//     `'x'` and `"x"` match. Exception: a tagged template's raw spelling
//     is not free — the tag receives `strings.raw`, so raw escapes enter
//     the canonical form (see TaggedTemplateExpression in `canonicalize`).
//   - harmless redundant parentheses: free — not ESTree nodes; the parser
//     already resolved precedence.
//   - comments without semantic force: free — only tool-consumed classes
//     (see `classifySemanticComment`) enter the form.
//   - named import-specifier order within one import declaration: free —
//     `import { a, b }` and `import { b, a }` bind the same hoisted ESM
//     bindings. Narrow exception; these stay structural:
//       * specifier membership
//       * imported name + local alias, paired
//       * `source` module
//       * default and namespace specifier form/position (grammar-fixed
//         ahead of the named list — never reordered)
//       * import attributes (`with { type: "json" }`)
//       * top-level order of import declarations (`Program.body`
//         positional; grouping is not merged)
//       * side-effect import sequence (`import "x"` compared in body order)
//
// Identifiers are structural — no alpha-renaming. The contract permits
// alpha-normalization only for bindings with private generated provenance.
// The pinned official compilers emit no provenance marker distinguishing
// compiler-generated private bindings from authored ones (`_sfc_main` is a
// naming convention; inferring generatedness from spelling is forbidden).
// Without explicit provenance every identifier participates in equality.
//
// Semantic comments are preserved: tool-consumed comments (`/*#__PURE__*/`,
// license/preserve, source-map/sourceURL, TS directives, triple-slash,
// JSDoc, JSX/bundler pragmas, Istanbul, ESLint, Prettier) attach to the
// canonical node they precede. Deleting, mutating, or relocating one is a
// structural difference. Classifier is over-inclusive toward "semantic":
// misclassifying prose as semantic can only produce a false difference
// (fail closed), never a false equivalence.
//
// Everything else the contract forbids stays live by construction:
//   - import/export sources and specifiers compared verbatim, except named
//     specifier order within one declaration as above
//   - helper/declaration/statement order: positional array walk
//   - literals, property keys, template content, tags, prop names,
//     diagnostic text: verbatim (only `raw` quote/escape spelling dropped)

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
 * Classify a comment as a tool-consumed (semantic-force) class, or null
 * for plain prose. `type` is acorn's `"Line"` | `"Block"`; `value` is the
 * text without delimiters.
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
  // Coverage directives: consumed by Istanbul/nyc; deleting or relocating
  // one changes which code is exempt.
  if (/^istanbul\s+ignore\b/.test(trimmed)) return "coverage-directive";
  // Lint directives (eslint-disable / enable / *-line / *-next-line).
  // Removing eslint-enable silently extends a disabled region.
  if (/^eslint-(disable|enable)(-next-line|-line)?\b/.test(trimmed)) return "lint-directive";
  // Format directives: prettier-ignore; relocation changes the exempt node.
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
 * Attach each semantic comment to the canonical node it precedes: the
 * outermost node whose start is the smallest position ≥ the comment's end.
 * A trailing comment with no following node (e.g. sourceMappingURL at EOF)
 * attaches to the Program. Relocating a comment to a different node is a
 * structural difference.
 *
 * ESLint-family directives also record line-adjacency: `eslint-disable-next-line`
 * suppresses the next source line, so a blank line between directive and
 * target changes what ESLint suppresses even if nearest-node attachment is
 * unchanged. Records exact `targetLineDelta` (attached start line minus
 * directive end line), not a boolean adjacency bit — the exact delta
 * discriminates every relocation the boolean would, plus multi-blank-line
 * changes. Delta enters the canonical form for lint directives only; other
 * families target the following node, so line reflow around them stays
 * cosmetic.
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

  // Attach the AST node's leading semantic comments to a canonical node.
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
        // Quote/escape spelling is cosmetic: compare decoded value, never `raw`.
        out = { type: "Literal", value: node.value, regex: node.regex, bigint: node.bigint };
        break;
      case "TemplateElement":
        // Untagged templates: no receiver observes `raw`; only cooked is
        // structural. Tagged-template elements go through the case below.
        out = { type: "TemplateElement", tail: node.tail, cooked: node.value?.cooked };
        break;
      case "TaggedTemplateExpression": {
        // Tagged templates: the tag receives `strings.raw`, so a raw-only
        // change is observable (String.raw`a\u0041b` vs String.raw`aAb`
        // cook identically). `raw` enters the canonical form here only.
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
        // Only named-specifier order is canonicalized; other fields walk
        // untouched. See the header.
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
   * Canonical specifier list of one import declaration: default/namespace
   * keep their original slot (grammar-fixed ahead of the named list);
   * following named specifiers are sorted. Sort key is full canonical
   * content (imported name, local alias, semantic comments) — a permutation
   * yields an identical list; adding, removing, or changing a specifier
   * changes the multiset. Two specifiers are never merged.
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
