/**
 * Shared Svelte golden-normalization primitives.
 *
 * The SINGLE source of truth for the pinned-compiler loader and the
 * topology-extraction helpers BOTH Svelte golden generators consume:
 *   - `scripts/gen-svelte-goldens.mjs`     — the hand-vendored corpus.
 *   - `scripts/gen-svelte-diff-corpus.mjs` — the generated differential corpus.
 *
 * Keeping these in one module (instead of forking them per generator) is the
 * shared-codebase rule applied to the test tooling: a normalization fix lands
 * once and both corpora pick it up. The hand-vendored extractors
 * (`maskNonCodeRegions`, `helperSequenceOf`, `extractImports`,
 * `extractExportDefault`, `extractTemplates`, `extractDelegatedEvents`,
 * `normalizeCss`) are byte-for-byte the same logic the hand-vendored generator
 * shipped — moved here verbatim. The EXPANDED differential extractors
 * (`extractClientEvents`, `extractClientNonStaticProperties`,
 * `extractClientAttrParts`, `extractClientNodePaths`, `extractDynamicSlotCounts`)
 * are new and serve only the generated corpus.
 *
 * Every extractor reads the OFFICIAL compiler's emitted client/server JS and
 * derives a NORMALIZED, variable-name-independent, whitespace-stable topology —
 * NEVER bytes. The Rust differential matrix produces the same normalized shape
 * from Verter's IR and diffs the two.
 */

import { createRequire } from "node:module";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** The pinned official Svelte version — the single oracle pin. */
export const SVELTE_ORACLE_VERSION = "5.56.3";

// ---------------------------------------------------------------------------
// Pinned-compiler loader
// ---------------------------------------------------------------------------

/**
 * Resolve the pinned svelte compiler from `repoRoot`. Pins the EXACT version
 * directory under pnpm so a different installed `svelte` cannot silently satisfy
 * the oracle. Throws a clear error (rather than resolving a floating `svelte`)
 * when the pinned version is not installed.
 */
export function loadPinnedCompiler(repoRoot) {
  const require = createRequire(join(repoRoot, "noop.js"));
  const pinnedDir = join(
    repoRoot,
    "node_modules/.pnpm",
    `svelte@${SVELTE_ORACLE_VERSION}`,
    "node_modules/svelte",
  );
  const pkgPath = join(pinnedDir, "package.json");
  if (!existsSync(pkgPath)) {
    throw new Error(
      `pinned svelte@${SVELTE_ORACLE_VERSION} not installed at ${pinnedDir}. ` +
        `Run \`pnpm install\` (svelte is a pinned devDependency).`,
    );
  }
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  if (pkg.version !== SVELTE_ORACLE_VERSION) {
    throw new Error(
      `installed svelte version ${pkg.version} != pinned SVELTE_ORACLE_VERSION ` +
        `${SVELTE_ORACLE_VERSION}. Re-pin the oracle and regenerate the goldens.`,
    );
  }
  const compilerPath = join(pinnedDir, "compiler/index.js");
  return require(compilerPath);
}

// ---------------------------------------------------------------------------
// Scope-hash masking
// ---------------------------------------------------------------------------

const SCOPE_HASH_RE = /svelte-[0-9a-z]+/g;
const SCOPE_HASH_PLACEHOLDER = "svelte-<scoped>";

/** First `svelte-<hash>` token in the source, or `null`. Topology, not bytes. */
export function extractScopeHash(text) {
  const m = text.match(/svelte-[0-9a-z]+/);
  return m ? m[0] : null;
}

export function maskScopeHash(text) {
  return text.replace(SCOPE_HASH_RE, SCOPE_HASH_PLACEHOLDER);
}

// ---------------------------------------------------------------------------
// Non-code masking (string/comment/regex/template-text → spaces)
// ---------------------------------------------------------------------------

/**
 * Mask the NON-CODE regions of a JS module — string literals, the TEXT spans of
 * template literals, line comments, block comments, and regex literals — by
 * overwriting their contents with spaces (newlines preserved so line structure
 * is unchanged). Template-literal `${…}` INTERPOLATIONS are deliberately NOT
 * masked (they are real code). Single-pass character scanner, not a full JS
 * parse — preserves helper FAMILY/sequence topology while excluding non-code
 * bytes, var-rename / whitespace stable.
 */
export function maskNonCodeRegions(code) {
  const out = Array.from(code);
  const n = code.length;
  const tmplStack = [];
  let prevSignificant = "";
  let i = 0;

  const inTemplateText = () =>
    tmplStack.length > 0 && tmplStack[tmplStack.length - 1].interpDepth === 0;

  const maskChar = (idx) => {
    if (code[idx] !== "\n" && code[idx] !== "\r") out[idx] = " ";
  };

  while (i < n) {
    if (inTemplateText()) {
      const ch = code[i];
      if (ch === "\\") {
        maskChar(i);
        if (i + 1 < n) maskChar(i + 1);
        i += 2;
        continue;
      }
      if (ch === "`") {
        tmplStack.pop();
        prevSignificant = "`";
        i += 1;
        continue;
      }
      if (ch === "$" && i + 1 < n && code[i + 1] === "{") {
        tmplStack[tmplStack.length - 1].interpDepth = 1;
        prevSignificant = "{";
        i += 2;
        continue;
      }
      maskChar(i);
      i += 1;
      continue;
    }

    const ch = code[i];
    const next = i + 1 < n ? code[i + 1] : "";

    if (ch === "/" && next === "/") {
      maskChar(i);
      maskChar(i + 1);
      i += 2;
      while (i < n && code[i] !== "\n") {
        maskChar(i);
        i += 1;
      }
      continue;
    }
    if (ch === "/" && next === "*") {
      maskChar(i);
      maskChar(i + 1);
      i += 2;
      while (i < n && !(code[i] === "*" && i + 1 < n && code[i + 1] === "/")) {
        maskChar(i);
        i += 1;
      }
      if (i < n) {
        maskChar(i);
        maskChar(i + 1);
        i += 2;
      }
      continue;
    }
    if (ch === "'" || ch === '"') {
      const quote = ch;
      prevSignificant = quote;
      i += 1;
      while (i < n && code[i] !== quote) {
        if (code[i] === "\\") {
          maskChar(i);
          if (i + 1 < n) maskChar(i + 1);
          i += 2;
          continue;
        }
        maskChar(i);
        i += 1;
      }
      if (i < n) i += 1;
      continue;
    }
    if (ch === "`") {
      tmplStack.push({ interpDepth: 0 });
      prevSignificant = "`";
      i += 1;
      continue;
    }
    if (tmplStack.length > 0 && tmplStack[tmplStack.length - 1].interpDepth > 0) {
      const frame = tmplStack[tmplStack.length - 1];
      if (ch === "{") {
        frame.interpDepth += 1;
        prevSignificant = "{";
        i += 1;
        continue;
      }
      if (ch === "}") {
        frame.interpDepth -= 1;
        prevSignificant = "}";
        i += 1;
        continue;
      }
    }
    if (ch === "/" && regexAllowedAfter(prevSignificant)) {
      i += 1;
      let inClass = false;
      while (i < n) {
        const rc = code[i];
        if (rc === "\\") {
          maskChar(i);
          if (i + 1 < n) maskChar(i + 1);
          i += 2;
          continue;
        }
        if (rc === "[") inClass = true;
        else if (rc === "]") inClass = false;
        else if (rc === "/" && !inClass) {
          i += 1;
          break;
        }
        if (rc === "\n") break;
        maskChar(i);
        i += 1;
      }
      while (i < n && /[a-z]/i.test(code[i])) i += 1;
      prevSignificant = "/";
      continue;
    }

    if (!/\s/.test(ch)) prevSignificant = ch;
    i += 1;
  }

  return out.join("");
}

/**
 * True when a `/` appearing after `prev` (the previous significant character)
 * begins a regex literal rather than a division operator.
 */
export function regexAllowedAfter(prev) {
  if (prev === "") return true;
  return "([{,;:=&|!?+-*%^~<>".includes(prev);
}

// ---------------------------------------------------------------------------
// Hand-vendored topology extractors (shared, moved verbatim)
// ---------------------------------------------------------------------------

/** The ORDERED `$.<helper>` reference sequence over the CODE-only view. */
export function helperSequenceOf(code) {
  const masked = maskNonCodeRegions(code);
  const seq = [];
  const re = /\$\.([A-Za-z_][A-Za-z0-9_]*)/g;
  let m;
  while ((m = re.exec(masked)) !== null) {
    seq.push(m[1]);
  }
  return seq;
}

/** The per-helper occurrence counts of a helper sequence. */
export function helperCountsOf(helperSequence) {
  const counts = {};
  for (const h of helperSequence) counts[h] = (counts[h] || 0) + 1;
  return counts;
}

/** Extract the import topology as sorted `{source, kind, names}` rows. */
export function extractImports(code) {
  const rows = [];
  for (const raw of code.split("\n")) {
    const line = raw.trim();
    if (!line.startsWith("import ")) continue;
    let m = line.match(/^import\s+['"]([^'"]+)['"]\s*;?$/);
    if (m) {
      rows.push({ source: m[1], kind: "sideEffect", names: [] });
      continue;
    }
    m = line.match(/^import\s+\*\s+as\s+([A-Za-z_$][\w$]*)\s+from\s+['"]([^'"]+)['"]\s*;?$/);
    if (m) {
      rows.push({ source: m[2], kind: "namespace", names: [m[1]] });
      continue;
    }
    m = line.match(/^import\s+(.+?)\s+from\s+['"]([^'"]+)['"]\s*;?$/);
    if (m) {
      const clause = m[1].trim();
      const source = m[2];
      const names = [];
      let kind = "named";
      const braceIdx = clause.indexOf("{");
      if (braceIdx === 0) {
        kind = "named";
      } else if (braceIdx > 0) {
        kind = "defaultAndNamed";
        names.push(`default:${clause.slice(0, braceIdx).replace(/,$/, "").trim()}`);
      } else {
        kind = "default";
        names.push(`default:${clause}`);
      }
      const braceMatch = clause.match(/\{([^}]*)\}/);
      if (braceMatch) {
        for (const part of braceMatch[1].split(",")) {
          const t = part.trim();
          if (t) names.push(t);
        }
      }
      rows.push({ source, kind, names });
      continue;
    }
  }
  const cmp = (x, y) => (x < y ? -1 : x > y ? 1 : 0);
  rows.sort(
    (a, b) =>
      cmp(a.source, b.source) || cmp(a.kind, b.kind) || cmp(a.names.join(","), b.names.join(",")),
  );
  return rows;
}

/** Extract the default-exported component function shape: `{name, params}`. */
export function extractExportDefault(code) {
  const m = code.match(/export\s+default\s+function\s+([A-Za-z_$][\w$]*)\s*\(([^)]*)\)/);
  if (!m) return null;
  const name = m[1];
  const params = m[2]
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => p.split("=")[0].trim());
  return { name, params };
}

/**
 * Extract the template skeletons: every `from_html`/`from_svg`/`from_mathml`/
 * `from_tree` first-argument template literal + the optional trailing fragment
 * flag. Scope hashes masked.
 */
export function extractTemplates(code) {
  const out = [];
  const re =
    /\$\.(from_html|from_svg|from_mathml|from_tree)\(\s*`((?:\\.|[^`\\])*)`(?:\s*,\s*([^)]+?))?\s*\)/g;
  let m;
  while ((m = re.exec(code)) !== null) {
    out.push({
      factory: m[1],
      html: maskScopeHash(m[2]),
      flag: m[3] !== undefined ? m[3].trim() : null,
    });
  }
  return out;
}

/** Extract the ORDERED delegated event-type set (`$.delegate([...])`). */
export function extractDelegatedEvents(code) {
  const masked = maskNonCodeRegions(code);
  const m = masked.match(/\$\.delegate\(\[([^\]]*)\]\)/);
  if (!m) return [];
  const start = m.index + m[0].indexOf("[") + 1;
  const end = m.index + m[0].lastIndexOf("]");
  const rawBody = code.slice(start, end);
  const out = [];
  const re = /["']([^"']*)["']/g;
  let lit;
  while ((lit = re.exec(rawBody)) !== null) {
    out.push(lit[1]);
  }
  return out;
}

/**
 * Normalize a FULL JS module for the emitted-JS equivalence comparison: collapse
 * cosmetic whitespace OUTSIDE string/template/HTML literals (so a tabs-vs-spaces /
 * line-wrap / blank-line reflow does not false-fail), while preserving whitespace
 * INSIDE string / template-literal / regex literals BYTE-EXACT (so an `$$props.bar`
 * vs `.foo`, a raw `count` vs `$.get(count)`, a dropped `$.child(_, true)` arg, a
 * sibling-offset drift, or meaningful TEXT whitespace inside a template literal
 * still fails). This is the FIDELITY the topology gate needs that the helper-name
 * sequence misses.
 *
 * The algorithm walks the module with the SAME literal-aware scanner as
 * `maskNonCodeRegions`, but instead of blanking literal contents it COPIES them
 * verbatim; outside literals, it collapses every run of whitespace to a single
 * space and trims spaces adjacent to a newline, dropping blank lines. The scope
 * hash inside template literals is masked first (build-noise), matching the
 * template-skeleton normalization.
 */
export function normalizeModuleForComparison(code) {
  const masked = maskScopeHash(code);
  const n = masked.length;
  const tmplStack = [];
  let out = "";
  let i = 0;

  const inTemplateText = () =>
    tmplStack.length > 0 && tmplStack[tmplStack.length - 1].interpDepth === 0;

  while (i < n) {
    if (inTemplateText()) {
      const ch = masked[i];
      if (ch === "\\") {
        out += masked.slice(i, i + 2);
        i += 2;
        continue;
      }
      if (ch === "`") {
        tmplStack.pop();
        out += "`";
        i += 1;
        continue;
      }
      if (ch === "$" && i + 1 < n && masked[i + 1] === "{") {
        tmplStack[tmplStack.length - 1].interpDepth = 1;
        out += "${";
        i += 2;
        continue;
      }
      // Template TEXT — copied verbatim (whitespace is significant DOM text).
      out += ch;
      i += 1;
      continue;
    }

    const ch = masked[i];
    const next = i + 1 < n ? masked[i + 1] : "";

    // Line / block comments are dropped entirely (cosmetic).
    if (ch === "/" && next === "/") {
      while (i < n && masked[i] !== "\n") i += 1;
      continue;
    }
    if (ch === "/" && next === "*") {
      i += 2;
      while (i < n && !(masked[i] === "*" && i + 1 < n && masked[i + 1] === "/")) i += 1;
      i += 2;
      continue;
    }
    // String literals — copied verbatim.
    if (ch === "'" || ch === '"') {
      const quote = ch;
      out += ch;
      i += 1;
      while (i < n && masked[i] !== quote) {
        if (masked[i] === "\\") {
          out += masked.slice(i, i + 2);
          i += 2;
          continue;
        }
        out += masked[i];
        i += 1;
      }
      if (i < n) {
        out += masked[i];
        i += 1;
      }
      continue;
    }
    // Template-literal open — copied verbatim, contents handled by the text loop.
    if (ch === "`") {
      tmplStack.push({ interpDepth: 0 });
      out += "`";
      i += 1;
      continue;
    }
    // Track `${…}` interpolation depth (so a `}` returns to template text).
    if (tmplStack.length > 0 && tmplStack[tmplStack.length - 1].interpDepth > 0) {
      const frame = tmplStack[tmplStack.length - 1];
      if (ch === "{") {
        frame.interpDepth += 1;
        out += "{";
        i += 1;
        continue;
      }
      if (ch === "}") {
        frame.interpDepth -= 1;
        out += "}";
        i += 1;
        continue;
      }
    }
    // Whitespace OUTSIDE a literal — collapse a run to a single space.
    if (/\s/.test(ch)) {
      while (i < n && /\s/.test(masked[i])) i += 1;
      out += " ";
      continue;
    }
    out += ch;
    i += 1;
  }

  // Trim the leading/trailing space the outside-literal collapse may have left.
  // A `replace(/\s+/g, " ")` here would DESTROY the whitespace inside string /
  // template literals this scanner deliberately preserved, so only trim.
  return out.trim();
}

/** Normalize the compiled CSS to `{present, hash, code}` (scope hash masked). */
export function normalizeCss(compiled) {
  const cssCode = compiled.css && compiled.css.code ? compiled.css.code : null;
  return {
    present: !!cssCode,
    hash: cssCode ? extractScopeHash(cssCode) : null,
    code: cssCode ? maskScopeHash(cssCode) : null,
  };
}

// ---------------------------------------------------------------------------
// EXPANDED differential extractors (generated corpus only)
//
// These derive the NORMALIZED differential axes from the official client output.
// Each is variable-name-independent: it keys on the EMITTED `$.`-call shape and
// the literal event-type/property-name arguments, never on the local variable
// names the compiler chose.
// ---------------------------------------------------------------------------

/**
 * Extract the registered EVENTS as normalized `{type, target, delegation}` rows.
 *
 * Three emission shapes (all on the code-only view, so a template-literal
 * `$.event` text cannot false-match):
 *   - `$.delegated('click', node, h)`        → {type:'click', target, delegation:'delegated'}
 *   - `$.event('focus', node, h)`            → {type:'focus', target, delegation:'direct'}
 *   - `$.event('click', node, h, true)`      → {type:'click', target, delegation:'direct'} (capture)
 *
 * The TARGET is derived from the second argument: `$.window` → window,
 * `$.document.body` → body, `$.document` → document, anything else (a local var)
 * → element.
 *
 * A FORWARDED-PROP event (an event handler on a `<Component>`) is emitted by
 * official as a `{ onclick: h }` object property in the component CALL, not as a
 * `$.event`/`$.delegated` call — captured here by scanning the component-call
 * argument objects for `on<name>:` properties. An attribute_effect-routed event
 * (e.g. an event delivered through a `{...spread}`) surfaces as part of the
 * spread (no per-event call); those are not enumerated as discrete events.
 *
 * Rows are returned in source order, then stably sorted by (type, target,
 * delegation) so the golden is order-stable regardless of emission order.
 */
export function extractClientEvents(code) {
  const masked = maskNonCodeRegions(code);
  const rows = [];

  const targetOf = (argText) => {
    const a = argText.trim();
    if (a === "$.window") return "window";
    if (a === "$.document.body") return "body";
    if (a === "$.document") return "document";
    return "element";
  };

  // `$.delegated('type', target, ...)` — delegated.
  // `$.event('type', target, ...[, true])` — direct (capture flag is still direct).
  const callRe = /\$\.(delegated|event)\(/g;
  let m;
  while ((m = callRe.exec(masked)) !== null) {
    const helper = m[1];
    const argsStart = m.index + m[0].length;
    const args = readCallArgs(code, argsStart);
    if (args.length < 2) continue;
    const typeMatch = args[0].trim().match(/^['"]([^'"]*)['"]$/);
    if (!typeMatch) continue;
    rows.push({
      type: typeMatch[1],
      target: targetOf(args[1]),
      delegation: helper === "delegated" ? "delegated" : "direct",
    });
  }

  // Forwarded-prop events: a component call `Name($$anchor, { onclick: h, … })`
  // (or `$.append`-free standalone). Scan object literals passed as a component
  // call's second argument for `on<name>:` properties.
  for (const ev of extractForwardedPropEvents(code)) {
    rows.push(ev);
  }

  rows.sort(
    (a, b) =>
      cmpStr(a.type, b.type) || cmpStr(a.target, b.target) || cmpStr(a.delegation, b.delegation),
  );
  return rows;
}

/**
 * Scan for component calls whose argument object carries `on<event>:` props (a
 * forwarded event prop). Official emits an event handler on a `<Component>` as a
 * plain prop, not a `$.event`. We detect a call of the form
 * `Ident($$anchor, { … })` / `Ident(node, { … })` and read `on<name>:` keys.
 */
function extractForwardedPropEvents(code) {
  const out = [];
  // A component call: an Identifier followed by `(`, a first arg, then `, {`.
  const re = /\b([A-Z][A-Za-z0-9_$]*)\(\s*[A-Za-z_$][\w$.]*\s*,\s*\{/g;
  let m;
  while ((m = re.exec(code)) !== null) {
    const objStart = m.index + m[0].length - 1; // points at the `{`
    const obj = readBalanced(code, objStart, "{", "}");
    if (obj === null) continue;
    // Read the TOP-LEVEL property keys of the object literal, then keep the
    // `on<name>` ones. Keys are read at brace-depth 1 only (a nested object's
    // keys do not count), so a handler EXPRESSION that mentions `on…:` inside a
    // value cannot false-match.
    for (const key of topLevelObjectKeys(obj)) {
      const m = key.match(/^on([a-z]+)$/);
      if (m) {
        out.push({ type: m[1], target: "element", delegation: "forwarded_prop" });
      }
    }
  }
  return out;
}

/**
 * Extract the NON-STATIC properties ("cannot be set statically") as normalized
 * `{name, kind, value}` rows. Two emission shapes:
 *   - `$.autofocus(node, value)`              → {name:'autofocus', kind:'autofocus'}
 *   - `node.<name> = value;` (muted / defaultValue / defaultChecked) → {name, kind:'dom_property'}
 *
 * The DOM-property write is scanned over the code-only view as a
 * `<ident>.<prop> = ` assignment whose property is in the official
 * `cannot_be_set_statically` set MINUS autofocus (autofocus is the helper form).
 *
 * The `value` field is the VALUE chunk-topology of the assigned RHS (the official
 * `cannot_be_set_statically` writes are a DIRECT property assignment in
 * `5.56.3` — `input.defaultValue = 'x'` or a `$.template_effect(() =>
 * input.defaultValue = `a ${x ?? ''} b`)`, NOT a `$.set_default_value` helper).
 * A static-literal RHS reduces to `['literal']`; a mixed template-literal RHS
 * (`a {x} b`) reduces to its literal/expr alternation `['literal','expr','literal']`;
 * any other expression RHS reduces to `['expr']`; the boolean `true` of a valueless
 * `muted`/`autofocus` reduces to `['boolean']`. This is what surfaces the dropped
 * literal chunks of a mixed `defaultValue` (Verter collapses the mixed value to a
 * single expression — its candidate `value` is `['expr']` where official keeps the
 * alternation).
 *
 * Returned stably sorted by (name, kind, value-joined).
 */
export function extractClientNonStaticProperties(code) {
  const masked = maskNonCodeRegions(code);
  const rows = [];

  // `$.autofocus(node, …)` — the value is the boolean `true` of a valueless
  // `autofocus` (a dynamic `autofocus={x}` would carry an expression, but the
  // generated corpus only exercises the valueless boolean form).
  const autoRe = /\$\.autofocus\(/g;
  let m;
  while ((m = autoRe.exec(masked)) !== null) {
    rows.push({ name: "autofocus", kind: "autofocus", value: ["boolean"] });
  }

  // DOM-property writes for the remaining set members. The RHS (from after the
  // `=` to the statement-terminating `;` / `)` at depth 0) is reduced to its
  // value chunk-topology over the CODE view (string literals / template literals
  // are read on the original code so their bytes are intact).
  const DOM_PROPS = ["muted", "defaultValue", "defaultChecked"];
  for (const prop of DOM_PROPS) {
    const re = new RegExp(`\\b[A-Za-z_$][\\w$]*\\.${prop}\\s*=\\s*`, "g");
    let mm;
    while ((mm = re.exec(masked)) !== null) {
      const rhsStart = mm.index + mm[0].length;
      const rhs = readAssignmentRhs(code, rhsStart);
      rows.push({ name: prop, kind: "dom_property", value: domPropValueChunks(rhs) });
    }
  }

  rows.sort(
    (a, b) =>
      cmpStr(a.name, b.name) ||
      cmpStr(a.kind, b.kind) ||
      cmpStr(a.value.join(","), b.value.join(",")),
  );
  return rows;
}

/**
 * Read the RHS of a DOM-property assignment starting at `start` (the first char
 * after `<prop> = `), up to the statement terminator (`;`) or the closing `)` of
 * an enclosing `$.template_effect(() => …)` arrow at depth 0. Respects nested
 * parens/braces/brackets and string/template literals. Returns the trimmed RHS
 * text.
 */
function readAssignmentRhs(code, start) {
  let depth = 0;
  let i = start;
  let str = null;
  const n = code.length;
  while (i < n) {
    const ch = code[i];
    if (str) {
      if (ch === "\\") {
        i += 2;
        continue;
      }
      if (ch === str) str = null;
      i += 1;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      str = ch;
      i += 1;
      continue;
    }
    if (ch === "(" || ch === "{" || ch === "[") {
      depth += 1;
      i += 1;
      continue;
    }
    if (ch === ")" || ch === "}" || ch === "]") {
      if (depth === 0) break; // the closing `)` of the enclosing template_effect arrow
      depth -= 1;
      i += 1;
      continue;
    }
    if (ch === ";" && depth === 0) break;
    i += 1;
  }
  return code.slice(start, i).trim();
}

/** Reduce a DOM-property RHS to its value chunk-topology. */
function domPropValueChunks(rhs) {
  const arg = rhs.trim();
  if (arg === "true" || arg === "false") return ["boolean"];
  if (arg.startsWith("`") && arg.endsWith("`")) return templateLiteralChunks(arg);
  if (/^(['"]).*\1$/s.test(arg)) return ["literal"];
  return ["expr"];
}

/**
 * Extract the DECODED-TEXT topology — the set of text-node SEED strings the
 * official `$.text('seed')` factory creates. The seed is the DECODED text (the
 * official compiler runs `decode_character_references`, so `&copy;` becomes `©`).
 * An empty `$.text()` (a dynamic text-first node the reactive `$.set_text` fills)
 * carries NO seed and is not recorded. Returned as the sorted multiset of decoded
 * seed strings.
 *
 * This is the dynamic-text-form decode axis: Verter's `TemplateFactory::TextNode {
 * seed }` candidate carries the RAW (un-decoded) seed text, so a `&copy;` seed
 * surfaces as a divergence here against official's decoded `©`. The static-text
 * inside a `from_html` skeleton is NOT a `$.text` seed (it lives in the template
 * HTML) and is covered by the static-HTML axis instead.
 */
export function extractClientDecodedText(code) {
  const seeds = [];
  const re = /\$\.text\(/g;
  let m;
  while ((m = re.exec(code)) !== null) {
    const args = readCallArgs(code, m.index + m[0].length);
    if (args.length === 0) continue;
    const first = args[0].trim();
    if (first === "") continue; // `$.text()` — dynamic, no seed
    // A single string-literal seed: decode the JS string-literal escapes to the
    // literal text the node is created with (the official seed is already
    // entity-decoded; here we only unwrap the JS string-literal quoting).
    const lit = parseJsStringLiteral(first);
    if (lit !== null) seeds.push(lit);
  }
  seeds.sort(cmpStr);
  return seeds;
}

/**
 * Extract the DIRECTIVE-VALUE inner-expression SHAPE for every `class:`/`style:`/
 * `bind:`/`use:`/`on:` directive, as normalized `{kind, shape}` rows.
 *
 *   - kind  ∈ {class, style, bind, use, on} — the directive family.
 *   - shape ∈ {object, expr, none}          — the inner-expression shape of the
 *     directive's VALUE: `object` when the value is a brace-wrapped object literal
 *     `{…}`, `none` for a value-less directive (a 2-arg `$.action(node, fn)` use),
 *     `expr` for any other expression.
 *
 * The official compiler ALWAYS lowers a directive value to its INNER expression
 * (a quoted `class:active="{dx}"` and an unquoted `class:active={dx}` emit the
 * IDENTICAL `{ active: $$props.dx }` — the inner `dx`, never the literal `{dx}`),
 * so a faithfully-projected golden never has a `shape: object` directive row. That
 * is exactly what surfaces Verter's object-literal mis-lowering: Verter keeps the
 * braces for a QUOTED `class:`/`style:`/`bind:`/`use:` value (its candidate row is
 * `shape: object`), which diverges from official's `shape: expr`. The `on:` /
 * `onclick` event path correctly unwraps the quoted single-expression handler, so
 * its candidate shape stays `expr` (a no-divergence confirmation).
 *
 * The emission shapes scanned (code-only view, so a template-literal text cannot
 * false-match):
 *   - `$.set_class(node, flag, static, …, { name: VALUE })`           → class
 *   - `$.set_style(node, static, …, { prop: VALUE })`                  → style
 *   - `$.bind_<target>(node, () => VALUE, …)`                          → bind
 *   - `$.action(node, ($$n[, $$arg]) => fn?.(…)[, () => ARG])`         → use
 *   - `$.event('type', node, HANDLER)` / `$.delegated('type', node, HANDLER)` → on
 *
 * Returned stably sorted by (kind, shape).
 */
export function extractClientDirectiveExprs(code) {
  const masked = maskNonCodeRegions(code);
  const rows = [];

  // class:/style: — the directive object is the LAST argument; its property
  // values are the directive values. Official emits a non-object value per prop.
  for (const [helper, kind] of [
    ["set_class", "class"],
    ["set_style", "style"],
  ]) {
    const re = new RegExp(`\\$\\.${helper}\\(`, "g");
    let m;
    while ((m = re.exec(masked)) !== null) {
      const args = readCallArgs(code, m.index + m[0].length);
      // The directive object is the last `{ … }` argument (a class/style
      // directive run); its top-level property VALUES are the directive values.
      const objArg = [...args].reverse().find((a) => {
        const t = a.trim();
        return t.startsWith("{") && t.endsWith("}");
      });
      if (!objArg) continue;
      for (const value of topLevelObjectValues(objArg.trim())) {
        rows.push({ kind, shape: directiveValueShape(value) });
      }
    }
  }

  // bind: — `$.bind_<target>(node, () => VALUE, …)`. The VALUE is inside the
  // first getter arrow.
  {
    const re = /\$\.bind_[a-z_]+\(/g;
    let m;
    while ((m = re.exec(masked)) !== null) {
      const args = readCallArgs(code, m.index + m[0].length);
      const getter = args[1] ? args[1].trim() : "";
      const value = arrowBody(getter);
      if (value !== null) rows.push({ kind: "bind", shape: directiveValueShape(value) });
    }
  }

  // use: — `$.action(node, fnArrow[, () => ARG])`. A 3-arg form carries the
  // action ARGUMENT (the directive value) in the trailing getter; a 2-arg form is
  // the no-argument `use:fn` (shape none).
  {
    const re = /\$\.action\(/g;
    let m;
    while ((m = re.exec(masked)) !== null) {
      const args = readCallArgs(code, m.index + m[0].length);
      if (args.length >= 3) {
        const value = arrowBody(args[2].trim());
        rows.push({ kind: "use", shape: value === null ? "expr" : directiveValueShape(value) });
      } else {
        rows.push({ kind: "use", shape: "none" });
      }
    }
  }

  // on: — `$.event('type', node, HANDLER)` / `$.delegated('type', node, HANDLER)`.
  {
    const re = /\$\.(event|delegated)\(/g;
    let m;
    while ((m = re.exec(masked)) !== null) {
      const args = readCallArgs(code, m.index + m[0].length);
      const handler = args[2] ? args[2].trim() : "";
      if (handler !== "") rows.push({ kind: "on", shape: directiveValueShape(handler) });
    }
  }

  rows.sort((a, b) => cmpStr(a.kind, b.kind) || cmpStr(a.shape, b.shape));
  return rows;
}

/** The inner-expression shape of a directive value token. */
function directiveValueShape(value) {
  const t = value.trim();
  if (t === "") return "none";
  if (t.startsWith("{") && t.endsWith("}")) return "object";
  return "expr";
}

/** The body of a single-expression arrow `() => BODY` / `($$x) => BODY`, or null. */
function arrowBody(text) {
  const arrow = text.indexOf("=>");
  if (arrow < 0) return null;
  return text.slice(arrow + 2).trim();
}

/**
 * The TOP-LEVEL property VALUES of an object-literal string (including the outer
 * `{` … `}`). Mirrors [`topLevelObjectKeys`] but returns the value token after
 * each depth-0 `:` up to the next depth-0 `,` (or the closing brace).
 */
function topLevelObjectValues(objText) {
  const values = [];
  const body = objText.slice(1, -1);
  let i = 0;
  let depth = 0;
  let str = null;
  let valueStart = -1;
  const n = body.length;
  const flush = (end) => {
    if (valueStart >= 0) {
      const seg = body.slice(valueStart, end).trim();
      if (seg) values.push(seg);
      valueStart = -1;
    }
  };
  while (i < n) {
    const ch = body[i];
    if (str) {
      if (ch === "\\") {
        i += 2;
        continue;
      }
      if (ch === str) str = null;
      i += 1;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      str = ch;
      i += 1;
      continue;
    }
    if (ch === "{" || ch === "[" || ch === "(") {
      depth += 1;
      i += 1;
      continue;
    }
    if (ch === "}" || ch === "]" || ch === ")") {
      depth -= 1;
      i += 1;
      continue;
    }
    if (depth === 0 && ch === ":") {
      valueStart = i + 1;
      i += 1;
      continue;
    }
    if (depth === 0 && ch === ",") {
      flush(i);
      i += 1;
      continue;
    }
    i += 1;
  }
  flush(n);
  return values;
}

/**
 * Decode a JS single/double-quoted string-literal token to its text value
 * (handling the common `\n` / `\t` / `\\` / `\'` / `\"` / `\uXXXX` escapes).
 * Returns `null` if `token` is not a plain string literal.
 */
function parseJsStringLiteral(token) {
  const t = token.trim();
  if (t.length < 2) return null;
  const q = t[0];
  if ((q !== "'" && q !== '"') || t[t.length - 1] !== q) return null;
  const body = t.slice(1, -1);
  let out = "";
  let i = 0;
  while (i < body.length) {
    if (body[i] === "\\") {
      const e = body[i + 1];
      if (e === "n") out += "\n";
      else if (e === "t") out += "\t";
      else if (e === "r") out += "\r";
      else if (e === "u" && body[i + 2] === "{") {
        const close = body.indexOf("}", i + 3);
        out += String.fromCodePoint(parseInt(body.slice(i + 3, close), 16));
        i = close + 1;
        continue;
      } else if (e === "u") {
        out += String.fromCharCode(parseInt(body.slice(i + 2, i + 6), 16));
        i += 6;
        continue;
      } else out += e;
      i += 2;
      continue;
    }
    out += body[i];
    i += 1;
  }
  return out;
}

/**
 * Extract the ATTRIBUTE-VALUE PART topology for dynamic / mixed attributes. Each
 * row is `{helper, attr, chunks}` where:
 *   - `helper` ∈ {set_attribute, set_class, set_style, set_value} — the emitted
 *     value-setter.
 *   - `attr`   — the attribute / property the setter targets (the literal name
 *     argument when present; `class`/`style`/`value` for the typed setters).
 *   - `chunks` — the value-part topology: `['expr']` for a single dynamic value,
 *     or the literal/expr alternation of a template literal `` `a ${x ?? ''} b` ``
 *     reduced to `['literal','expr','literal']`. A class-directive object
 *     (`{ active: x }`) is recorded as a `directive` chunk.
 *
 * Returned stably sorted by (helper, attr, chunks-joined).
 */
export function extractClientAttrParts(code) {
  const masked = maskNonCodeRegions(code);
  const rows = [];

  const SETTERS = {
    set_attribute: { typed: false },
    set_class: { typed: "class" },
    set_style: { typed: "style" },
    set_value: { typed: "value" },
  };

  for (const [helper, info] of Object.entries(SETTERS)) {
    const re = new RegExp(`\\$\\.${helper}\\(`, "g");
    let m;
    while ((m = re.exec(masked)) !== null) {
      const argsStart = m.index + m[0].length;
      const args = readCallArgs(code, argsStart);
      let attr;
      let valueArgs;
      if (info.typed) {
        attr = info.typed;
        valueArgs = args.slice(1); // first arg is the node
      } else {
        // set_attribute(node, 'name', value)
        const nameMatch = args[1] ? args[1].trim().match(/^['"]([^'"]*)['"]$/) : null;
        attr = nameMatch ? nameMatch[1] : "<dynamic>";
        valueArgs = args.slice(2);
      }
      rows.push({ helper, attr, chunks: chunkTopology(valueArgs) });
    }
  }

  rows.sort(
    (a, b) =>
      cmpStr(a.helper, b.helper) ||
      cmpStr(a.attr, b.attr) ||
      cmpStr(a.chunks.join(","), b.chunks.join(",")),
  );
  return rows;
}

/**
 * Reduce a setter's value argument(s) to a normalized chunk-kind list. A single
 * template-literal arg `` `a ${x ?? ''} b` `` reduces to its literal/expr
 * alternation; a single non-template arg reduces to `['expr']`; a class/style
 * directive object (`{ active: x }`) reduces to a `['directive']` chunk.
 */
function chunkTopology(valueArgs) {
  const chunks = [];
  for (const raw of valueArgs) {
    const arg = raw.trim();
    if (arg === "") continue;
    if (/^\d+$/.test(arg)) {
      // A bare integer is the setter's leading FLAG argument (`set_class(node,
      // 1, …)` / `set_style(node, 1, …)`), NOT a value chunk — skip it.
      continue;
    }
    if (arg.startsWith("`") && arg.endsWith("`")) {
      // Template literal — walk literal text vs `${…}` interpolations.
      chunks.push(...templateLiteralChunks(arg));
    } else if (arg.startsWith("{") && arg.endsWith("}")) {
      chunks.push("directive");
    } else if (arg === "null" || arg === "''" || arg === '""') {
      // A placeholder slot (e.g. set_class's empty static-class arg). Skip — it
      // is not a value chunk.
    } else {
      chunks.push("expr");
    }
  }
  if (chunks.length === 0) chunks.push("expr");
  return chunks;
}

/** Decompose a template literal into a literal/expr alternation of chunk kinds. */
function templateLiteralChunks(tmpl) {
  const body = tmpl.slice(1, -1); // strip backticks
  const chunks = [];
  let i = 0;
  let literal = "";
  while (i < body.length) {
    if (body[i] === "\\") {
      literal += body.slice(i, i + 2);
      i += 2;
      continue;
    }
    if (body[i] === "$" && body[i + 1] === "{") {
      if (literal.length > 0) {
        chunks.push("literal");
        literal = "";
      }
      // Skip the balanced `${…}`.
      const end = matchBrace(body, i + 1);
      chunks.push("expr");
      i = end + 1;
      continue;
    }
    literal += body[i];
    i += 1;
  }
  if (literal.length > 0) chunks.push("literal");
  return chunks;
}

/**
 * Extract the NORMALIZED node-path topology per client REGION.
 *
 * The official compiler reaches each dynamic node via a chain of named-variable
 * declarations: `var div = root();` (the region's fragment root, base=fragment),
 * `var span = $.child(div);`, `var t = $.sibling(span, 1, true);`, etc. We:
 *   1. Group the emitted statements into REGIONS — the top-level component body
 *      and each nested arrow-function body (`($$anchor) => { … }`, the if /each /
 *      key / await / snippet branch bodies). A region is a `{ … }` block that
 *      contains at least one `var X = root…()` / `$.first_child` / `$.child` /
 *      `$.sibling` / `$.text` declaration.
 *   2. Within each region, resolve each declared variable to a path:
 *        - `root()` / `root_N()` (a template clone)      → base=fragment, steps=[]
 *        - `$.first_child(base)`                          → base path + [first_child]
 *        - `$.child(base[, transparent])`                 → base path + [child]
 *        - `$.sibling(base[, offset][, transparent])`     → base path + [sibling]
 *        - `$.text(...)` / `$.comment()`                  → base=fragment, steps=[]
 *      where `base` is the variable the call's first argument names (resolved to
 *      its own path) or `fragment` when the call descends from the region root.
 *   3. The NORMALIZED region path-set is the MULTISET of `{base, steps}` for every
 *      variable that a DYNAMIC op later reads (we conservatively include EVERY
 *      declared walk variable — the matrix compares the multiset, and Verter's
 *      candidate likewise enumerates every planned `NodePathPlan`). Cursor-only
 *      ops (`$.reset`, `$.next`) are NOT path variables and are dropped.
 *
 * Each path's `base` is normalized to `fragment` (descends from the cloned
 * fragment / region root) or `node` (descends from another named node). The
 * step list is the ORDERED step KINDS — variable names, offsets, and transparency
 * flags are dropped (they are the backend's DOM-walk strategy, not topology).
 *
 * Returns `{ regions: [ { paths: [ {base, steps} … ] } … ] }` with regions and
 * within-region paths stably sorted so the golden is emission-order-independent.
 */
export function extractClientNodePaths(code) {
  const regions = [];
  for (const regionBody of splitRegions(code)) {
    const paths = nodePathsInRegion(regionBody);
    if (paths.length === 0) continue;
    paths.sort((a, b) => cmpStr(a.base, b.base) || cmpStr(a.steps.join(">"), b.steps.join(">")));
    regions.push({ paths });
  }
  // Stable region order: sort by serialized path-set.
  regions.sort((a, b) => cmpStr(JSON.stringify(a.paths), JSON.stringify(b.paths)));
  return { regions };
}

/**
 * Split a module into REGION bodies: the default-export function body plus every
 * nested arrow/function body that declares its own walk variables. Implemented
 * as a balanced-brace scan that, for every `{` opening a function body, captures
 * the body text; a region is reported when its IMMEDIATE statements (not those of
 * a nested region) contain a walk declaration. Nested regions are reported
 * separately (each branch body is its own region).
 */
function splitRegions(code) {
  const regions = [];
  // Find every function/arrow body open brace and capture the balanced body.
  // We approximate "function body" as a `)` or `=>` immediately preceding a `{`.
  const n = code.length;
  let i = 0;
  while (i < n) {
    const ch = code[i];
    if (ch === "{") {
      // Look back for `)` or `=>` (a function/arrow body) skipping whitespace.
      let j = i - 1;
      while (j >= 0 && /\s/.test(code[j])) j -= 1;
      const isBody = code[j] === ")" || (code[j] === ">" && code[j - 1] === "=");
      if (isBody) {
        const body = readBalanced(code, i, "{", "}");
        if (body !== null) {
          // The region's IMMEDIATE body = the body with nested function bodies
          // blanked, so a walk decl in a nested arrow does not count here.
          regions.push(body.slice(1, -1));
        }
      }
    }
    i += 1;
  }
  return regions;
}

/**
 * Resolve the node-path multiset within ONE region body. Reads `var X = …;`
 * declarations whose initializer is a `root…()` / `$.first_child` / `$.child` /
 * `$.sibling` / `$.text` / `$.comment` call (the only path-producing forms) and
 * builds each variable's `{base, steps}`. A nested region's own decls are
 * EXCLUDED (the brace-scan only reads top-level `var` statements of THIS body).
 */
function nodePathsInRegion(body) {
  // Blank nested function bodies so we only read THIS region's own decls.
  const flat = blankNestedBodies(body);
  const vars = new Map(); // name -> {base, steps}
  const order = [];

  const declRe = /\bvar\s+([A-Za-z_$][\w$]*)\s*=\s*([^;]+);/g;
  let m;
  while ((m = declRe.exec(flat)) !== null) {
    const name = m[1];
    const init = m[2].trim();
    const path = pathFromInit(init, vars);
    if (path) {
      vars.set(name, path);
      order.push(name);
    }
  }

  // The region path multiset: every walk variable that is NOT a bare fragment
  // root with no steps is a reachable dynamic-node path; a bare `root()` clone is
  // the region's own fragment (steps=[]) and IS a path base, so we include it too
  // when the region has interior walks (it mirrors Verter's PathBase::Fragment
  // root). To keep the comparison robust we include every NON-root-clone path
  // (steps.length > 0) — those are the dynamic-node reaching walks both sides
  // enumerate; a pure fragment clone with no descent carries no NodePathPlan in
  // Verter, so it is excluded.
  const out = [];
  for (const name of order) {
    const p = vars.get(name);
    if (p.steps.length > 0) out.push({ base: p.base, steps: p.steps });
  }
  return out;
}

/** Build a `{base, steps}` path from a declaration initializer. */
function pathFromInit(init, vars) {
  // Template-clone root: `root()`, `root_1()`.
  if (/^root(_\d+)?\(\s*\)$/.test(init)) {
    return { base: "fragment", steps: [] };
  }
  // `$.text(...)` / `$.comment()` — a fresh anchor/text node (region fragment).
  if (/^\$\.(text|comment)\(/.test(init)) {
    return { base: "fragment", steps: [] };
  }
  // `$.first_child(base)`.
  let m = init.match(/^\$\.first_child\(\s*([A-Za-z_$][\w$]*)/);
  if (m) return descend(m[1], "first_child", vars);
  // `$.child(base[, …])`.
  m = init.match(/^\$\.child\(\s*([A-Za-z_$][\w$]*)/);
  if (m) return descend(m[1], "child", vars);
  // `$.sibling(base[, …])`.
  m = init.match(/^\$\.sibling\(\s*([A-Za-z_$][\w$]*)/);
  if (m) return descend(m[1], "sibling", vars);
  return null;
}

/** Append a step kind to the base variable's resolved path. */
function descend(baseName, step, vars) {
  const basePath = vars.get(baseName);
  if (!basePath) {
    // The base is the region fragment / an $$anchor — treat as fragment base.
    return { base: "fragment", steps: [step] };
  }
  // Base is another named node → the new path's base kind is `node`.
  const baseKind = basePath.steps.length === 0 ? basePath.base : "node";
  return { base: baseKind, steps: [...basePath.steps, step] };
}

/**
 * Extract a per-slot-KIND count of the dynamic surfaces the official client
 * emits — a region-agnostic topology summary mirroring Verter's
 * `StaticTemplatePlan::slots` multiset. Keyed off the EMITTED helper that
 * realizes each surface kind:
 *   - text       → `$.set_text` + `.textContent =` + `.nodeValue =`
 *   - attribute  → `$.set_attribute`
 *   - class      → `$.set_class`
 *   - style      → `$.set_style`
 *   - value      → `$.set_value`
 *   - spread     → `$.attribute_effect`
 *   - bind       → `$.bind_*`
 *   - html       → `$.html`
 *   - block      → `$.if` + `$.each` + `$.await` + `$.key`
 *
 * Returns an object `{kind: count}` (zero-count kinds omitted), so the matrix
 * compares the realized dynamic-surface kinds.
 */
export function extractDynamicSlotCounts(code) {
  const masked = maskNonCodeRegions(code);
  const counts = {};
  const bump = (k, n) => {
    if (n > 0) counts[k] = (counts[k] || 0) + n;
  };
  const countOf = (re) => (masked.match(re) || []).length;

  bump(
    "text",
    countOf(/\$\.set_text\(/g) + countOf(/\.textContent\s*=/g) + countOf(/\.nodeValue\s*=/g),
  );
  bump("attribute", countOf(/\$\.set_attribute\(/g));
  bump("class", countOf(/\$\.set_class\(/g));
  bump("style", countOf(/\$\.set_style\(/g));
  bump("value", countOf(/\$\.set_value\(/g));
  bump("spread", countOf(/\$\.attribute_effect\(/g));
  bump("bind", countOf(/\$\.bind_[a-z_]+\(/g));
  bump("html", countOf(/\$\.html\(/g));
  bump(
    "block",
    countOf(/\$\.if\(/g) + countOf(/\$\.each\(/g) + countOf(/\$\.await\(/g) + countOf(/\$\.key\(/g),
  );

  return counts;
}

// ---------------------------------------------------------------------------
// Low-level scanning helpers
// ---------------------------------------------------------------------------

const cmpStr = (x, y) => (x < y ? -1 : x > y ? 1 : 0);

/**
 * Read the comma-separated argument substrings of a call whose `(` is at (just
 * before) `argsStart` — `argsStart` points at the first character AFTER the `(`.
 * Respects nested parens / braces / brackets and string/template literals.
 * Returns the raw argument text list (trimmed by the caller).
 */
function readCallArgs(code, argsStart) {
  const args = [];
  let depth = 1; // we are inside the opening `(`
  let cur = "";
  let i = argsStart;
  let str = null; // active string/template delimiter
  const n = code.length;
  while (i < n && depth > 0) {
    const ch = code[i];
    if (str) {
      cur += ch;
      if (ch === "\\") {
        if (i + 1 < n) cur += code[i + 1];
        i += 2;
        continue;
      }
      if (ch === str) str = null;
      i += 1;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      str = ch;
      cur += ch;
      i += 1;
      continue;
    }
    if (ch === "(" || ch === "{" || ch === "[") {
      depth += 1;
      cur += ch;
      i += 1;
      continue;
    }
    if (ch === ")" || ch === "}" || ch === "]") {
      depth -= 1;
      if (depth === 0) break;
      cur += ch;
      i += 1;
      continue;
    }
    if (ch === "," && depth === 1) {
      args.push(cur);
      cur = "";
      i += 1;
      continue;
    }
    cur += ch;
    i += 1;
  }
  if (cur.trim().length > 0 || args.length > 0) args.push(cur);
  return args;
}

/**
 * Read a balanced `(open … close)` substring starting at `start` (which must
 * point at an `open` char). Respects nested open/close + string/template
 * literals. Returns the substring INCLUDING the delimiters, or `null` if
 * unbalanced.
 */
function readBalanced(code, start, open, close) {
  let depth = 0;
  let i = start;
  let str = null;
  const n = code.length;
  while (i < n) {
    const ch = code[i];
    if (str) {
      if (ch === "\\") {
        i += 2;
        continue;
      }
      if (ch === str) str = null;
      i += 1;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      str = ch;
      i += 1;
      continue;
    }
    if (ch === open) depth += 1;
    else if (ch === close) {
      depth -= 1;
      if (depth === 0) return code.slice(start, i + 1);
    }
    i += 1;
  }
  return null;
}

/**
 * The TOP-LEVEL property keys of an object-literal string (including the outer
 * `{` … `}`). A key is the identifier / quoted-string token immediately before a
 * `:` at the object's OWN brace depth (depth 1). Nested objects/arrays/parens
 * and string/template literals are skipped, so a `:` inside a value (a ternary,
 * a nested object, a getter body) does not produce a phantom key. A `get foo()`
 * getter contributes the key `foo`.
 */
function topLevelObjectKeys(objText) {
  const keys = [];
  // Strip the outer braces, then scan the body at depth 0 (the body's own top).
  const body = objText.slice(1, -1);
  let i = 0;
  let depth = 0;
  let str = null;
  let tokenStart = 0;
  const n = body.length;
  const flushKeyBefore = (colonIdx) => {
    // The key is the last token before the colon at depth 0.
    const seg = body.slice(tokenStart, colonIdx).trim();
    // A quoted key.
    let m = seg.match(/^['"]([^'"]*)['"]$/);
    if (m) {
      keys.push(m[1]);
      return;
    }
    // A bare identifier, optionally preceded by `get `/`set `/`async `/`*`.
    m = seg.match(/(?:^|[\s*])([A-Za-z_$][\w$]*)$/);
    if (m) keys.push(m[1]);
  };
  while (i < n) {
    const ch = body[i];
    if (str) {
      if (ch === "\\") {
        i += 2;
        continue;
      }
      if (ch === str) str = null;
      i += 1;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      str = ch;
      i += 1;
      continue;
    }
    if (ch === "{" || ch === "[" || ch === "(") {
      depth += 1;
      i += 1;
      continue;
    }
    if (ch === "}" || ch === "]" || ch === ")") {
      depth -= 1;
      i += 1;
      continue;
    }
    if (depth === 0 && ch === ":") {
      flushKeyBefore(i);
      // Advance to the next top-level comma (the value is whatever follows).
      i += 1;
      continue;
    }
    if (depth === 0 && ch === ",") {
      tokenStart = i + 1;
      i += 1;
      continue;
    }
    i += 1;
  }
  return keys;
}

/** Index of the `}` that closes the `{` at `braceOpen` (a `${` interpolation). */
function matchBrace(body, braceOpen) {
  let depth = 0;
  let i = braceOpen;
  while (i < body.length) {
    if (body[i] === "{") depth += 1;
    else if (body[i] === "}") {
      depth -= 1;
      if (depth === 0) return i;
    }
    i += 1;
  }
  return body.length - 1;
}

/**
 * Blank every NESTED function/arrow body within a region body (replace inner
 * body chars with spaces) so a `var` decl inside a nested branch arrow does not
 * leak into this region's path scan. Newlines preserved.
 */
function blankNestedBodies(body) {
  const out = Array.from(body);
  const n = body.length;
  let i = 0;
  while (i < n) {
    const ch = body[i];
    if (ch === "{") {
      let j = i - 1;
      while (j >= 0 && /\s/.test(body[j])) j -= 1;
      const isBody = body[j] === ")" || (body[j] === ">" && body[j - 1] === "=");
      if (isBody) {
        const inner = readBalanced(body, i, "{", "}");
        if (inner !== null) {
          for (let k = i + 1; k < i + inner.length - 1; k += 1) {
            if (out[k] !== "\n" && out[k] !== "\r") out[k] = " ";
          }
          i += inner.length;
          continue;
        }
      }
    }
    i += 1;
  }
  return out.join("");
}
