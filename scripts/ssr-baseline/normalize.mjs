/**
 * Normalization functions for fair comparison of SSR output between
 * Vue's @vue/compiler-sfc and Verter.
 */

/**
 * Extract the ssrRender function body from compiled SSR output.
 * Matches `function ssrRender(` or `export function ssrRender(` and
 * returns the balanced-brace body (excluding the outer declaration).
 *
 * @param {string} code - Full compiled SSR output
 * @returns {string | null} - The function body, or null if not found
 */
export function extractSsrRenderBody(code) {
  const idx = code.indexOf("function ssrRender(");
  if (idx === -1) return null;

  // Find the opening brace of the function body
  const braceStart = code.indexOf("{", idx);
  if (braceStart === -1) return null;

  // Match balanced braces with proper template literal handling
  let depth = 1;
  let i = braceStart + 1;
  while (i < code.length && depth > 0) {
    const ch = code[i];
    if (ch === "{") depth++;
    else if (ch === "}") depth--;
    else if (ch === '"' || ch === "'") {
      // Skip simple string literals
      i = skipSimpleString(code, i);
    } else if (ch === "`") {
      // Skip template literal with ${...} interpolation support
      i = skipTemplateLiteral(code, i);
    } else if (ch === "/" && i + 1 < code.length) {
      // Skip comments
      if (code[i + 1] === "/") {
        i = code.indexOf("\n", i);
        if (i === -1) i = code.length;
      } else if (code[i + 1] === "*") {
        i = code.indexOf("*/", i + 2);
        if (i === -1) i = code.length;
        else i += 1; // position at the '/', will be incremented below
      }
    }
    i++;
  }

  if (depth !== 0) return null;

  // Return body between braces (exclusive)
  return code.slice(braceStart + 1, i - 1);
}

/** Skip a simple string (' or "), returning the index of the closing quote. */
function skipSimpleString(code, start) {
  const quote = code[start];
  let i = start + 1;
  while (i < code.length) {
    if (code[i] === "\\") i++; // skip escaped char
    else if (code[i] === quote) return i;
    i++;
  }
  return i;
}

/** Skip a template literal with proper ${...} interpolation handling.
 *  Returns the index of the closing backtick. */
function skipTemplateLiteral(code, start) {
  let i = start + 1;
  while (i < code.length) {
    if (code[i] === "\\") {
      i++; // skip escaped char
    } else if (code[i] === "`") {
      return i; // closing backtick
    } else if (code[i] === "$" && i + 1 < code.length && code[i + 1] === "{") {
      // Enter ${...} expression — scan with balanced braces
      i += 2; // skip ${
      let exprDepth = 1;
      while (i < code.length && exprDepth > 0) {
        const ch = code[i];
        if (ch === "{") exprDepth++;
        else if (ch === "}") {
          exprDepth--;
          if (exprDepth === 0) break; // end of interpolation
        } else if (ch === '"' || ch === "'") {
          i = skipSimpleString(code, i);
        } else if (ch === "`") {
          i = skipTemplateLiteral(code, i); // nested template literal
        }
        i++;
      }
      // i is now at the closing } of the interpolation
    }
    i++;
  }
  return i;
}

/**
 * Normalize SSR output for fair comparison.
 * Handles acceptable whitespace/formatting differences.
 *
 * @param {string} code - Raw ssrRender function body
 * @returns {string} - Normalized code
 */
export function normalizeForComparison(code) {
  let s = code;
  // Strip "use strict"
  s = s.replace(/"use strict";?\s*/g, "");
  // Strip VDOM fallback branches: } else { return [...] }
  // These only affect client-side hydration, not SSR rendering.
  s = stripVdomFallback(s);
  // Strip leading/trailing whitespace per line
  s = s
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
    .join("\n");
  // Collapse multiple whitespace to single space
  s = s.replace(/\s+/g, " ");
  // Remove trailing semicolons (Verter may omit where Vue includes)
  s = s.replace(/;\s*$/gm, "");
  s = s.replace(/;(\s*[)}])/g, "$1");
  // Normalize trailing/leading space inside parens
  s = s.replace(/\(\s+/g, "(");
  s = s.replace(/\s+\)/g, ")");
  // Normalize spaces inside template literal interpolation: ${ expr } → ${expr}
  s = s.replace(/\$\{\s+/g, "${");
  s = s.replace(/\s+\}/g, "}");
  // Sort _resolveComponent declarations (order doesn't affect runtime behavior)
  s = normalizeResolveOrder(s);
  // Strip _scopeId params — compileTemplate() may add these when called with
  // scoped style metadata, but template-only compilation doesn't always include it.
  // The _scopeId parameter is an artifact of the compilation context, not
  // a semantic difference in the SSR render logic.
  s = stripScopeIdParams(s);
  return s.trim();
}

/**
 * Strip _scopeId-related parameters from SSR output.
 * Vue's compileTemplate() adds _scopeId params to slot closures and
 * component calls when scoped styles are detected. This is an artifact
 * of the compilation context, not the template logic itself.
 */
function stripScopeIdParams(s) {
  // Remove _scopeId function param: (_, _push, _parent, _scopeId) → (_, _push, _parent)
  s = s.replace(/,\s*_scopeId\)/g, ")");
  // Remove ${_scopeId} from template literals
  s = s.replace(/\$\{_scopeId\}/g, "");
  // Remove _scopeId arg in function calls: , _scopeId at end of arg list
  s = s.replace(/,\s*_scopeId\b/g, "");
  return s;
}

/**
 * Sort _resolveComponent const declarations.
 * Vue and Verter may emit these in different order (template traversal vs
 * collection order). The order doesn't affect runtime behavior.
 */
function normalizeResolveOrder(s) {
  const resolvePattern =
    /const _component_\w+ = _resolveComponent\("[^"]+"\)/g;
  const resolves = [];
  let cleaned = s.replace(resolvePattern, (match) => {
    resolves.push(match);
    return ""; // remove from original position
  });
  if (resolves.length === 0) return s;
  // Sort and re-insert at the beginning
  resolves.sort();
  const prefix = resolves.join(" ");
  return prefix + " " + cleaned.replace(/^\s+/, "");
}

/**
 * Strip VDOM fallback branches: `} else { return [...] }`
 * In SSR output, _withCtx slots have `if (_push) { ... } else { return [...] }`.
 * The else branch returns VDOM nodes for client-side hydration — irrelevant
 * for SSR correctness. Normalizing this away allows fair comparison.
 */
function stripVdomFallback(s) {
  let result = "";
  let i = 0;
  while (i < s.length) {
    // Match "} else { return ["
    if (
      s[i] === "}" &&
      s.slice(i, i + 30).match(/^\}\s*else\s*\{\s*return\s*\[/)
    ) {
      const m = s.slice(i).match(/^\}\s*else\s*\{\s*return\s*\[/);
      result += "}";
      // Skip past "} else { return [", then find matching ]
      let j = i + m[0].length;
      let depth = 1;
      while (j < s.length && depth > 0) {
        if (s[j] === "[") depth++;
        else if (s[j] === "]") depth--;
        else if (s[j] === '"' || s[j] === "'") {
          j = skipSimpleString(s, j);
        } else if (s[j] === "`") {
          j = skipTemplateLiteral(s, j);
        }
        j++;
      }
      // j is past the ] — skip whitespace and the closing }
      while (j < s.length && /\s/.test(s[j])) j++;
      if (j < s.length && s[j] === "}") j++;
      i = j;
    } else {
      result += s[i];
      i++;
    }
  }
  return result;
}

/**
 * Extract import lines from SSR output.
 * Returns sorted import lines for comparison of helper usage.
 *
 * @param {string} code - Full compiled SSR output
 * @returns {string[]} - Sorted import lines
 */
export function extractImports(code) {
  const imports = [];
  for (const line of code.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("import ")) {
      imports.push(trimmed);
    }
  }
  return imports.sort();
}
