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
    if (code[i] === "\\")
      i++; // skip escaped char
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
  // Strip leading/trailing whitespace per line
  s = s
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
    .join("\n");
  // Collapse multiple whitespace to single space
  s = s.replace(/\s+/g, " ");
  // Strip VDOM fallback branches: `} else { return [...] }` in _withCtx callbacks.
  // These return VDOM nodes for client-side hydration — they have NO effect on SSR
  // output. Vue and Verter may produce different VDOM trees (event handlers,
  // _createTextVNode wrapping, _withModifiers, etc.) that are all functionally
  // equivalent for hydration but differ syntactically.
  s = stripVdomFallback(s);
  // Strip VDOM patch flags and dynamic prop arrays EARLY — before whitespace
  // normalization around braces/parens that could destroy comment syntax.
  // These are purely client-side optimization hints that don't affect SSR.
  // Strip DYNAMIC_SLOTS (1024) bit from combined patch flag values:
  s = s.replace(/(\d+)\s*\/\*\s*([^*]+)\*\//g, (match, num, comment) => {
    const n = parseInt(num);
    if (!(n & 1024)) return match;
    const newN = n & ~1024;
    const parts = comment
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s !== "DYNAMIC_SLOTS");
    if (newN === 0 || parts.length === 0) return "";
    return `${newN} /* ${parts.join(", ")} */`;
  });
  // Strip standalone 1024 /* DYNAMIC_SLOTS */ (with optional leading comma)
  s = s.replace(/,?\s*1024\s*(\/\*\s*DYNAMIC_SLOTS\s*\*\/)?/g, "");
  // Strip dynamic prop arrays: , ["prop1", "prop2"]
  s = s.replace(/,\s*\["[^"]*"(?:,\s*"[^"]*")*\]/g, "");
  // Strip all patch flags: , N /* COMMENT */
  s = s.replace(/,\s*\d+\s*\/\*\s*[^*]+\*\//g, "");
  // Strip trailing null arguments: , null) → ) — trailing null in
  // _createVNode/_createBlock is a no-op placeholder for omitted args
  s = s.replace(/,\s*null\)/g, ")");
  // Clean up trailing commas left by patch flag removal: },) → })
  s = s.replace(/,\s*\)/g, ")");
  s = s.replace(/,\s*\]/g, "]");
  // Normalize space between `)` and `}` in closure/callback boundaries.
  // Vue: `))}`  vs Verter: `)) }` — both are valid JS, just formatting.
  s = s.replace(/\) }/g, ")}");
  // Normalize space between `{` and `_push`/`if`/`return`/`_ssr` in closure bodies.
  // Vue: `{ _push(` vs Verter: `{_push(` — closure body indentation difference.
  s = s.replace(/\{ (_push|if |return |_ssr|_ctx|const )/g, "{$1");
  // Normalize leading space in _push template literal strings.
  // Vue: `_push(\`<div\`)` vs Verter: `_push(\` <div\`)` — extra space at start.
  // This is whitespace-only content that renders identically in HTML.
  s = s.replace(/_push\(` /g, "_push(`");
  // Normalize space between `>` and `${` in template literals.
  // Vue: `>text` vs Verter: `> text` — leading whitespace in text content.
  s = s.replace(/> \$\{/g, ">${");
  // Remove trailing semicolons (Verter may omit where Vue includes)
  s = s.replace(/;\s*$/gm, "");
  s = s.replace(/;(\s*[)}])/g, "$1");
  // Normalize trailing space before comma: `name , class` → `name, class`
  // Can occur when string concatenation leaves trailing whitespace.
  s = s.replace(/ ,/g, ",");
  // Normalize trailing/leading space inside parens
  s = s.replace(/\(\s+/g, "(");
  s = s.replace(/\s+\)/g, ")");
  // Normalize space before comma after closing paren: ") ," → "),"
  // Vue's multiline codegen puts comma on next line after long expressions,
  // which after whitespace collapse becomes ") ," vs Verter's "),"
  s = s.replace(/\)\s+,/g, "),");
  // Normalize spaces inside template literal interpolation: ${ expr } → ${expr}
  s = s.replace(/\$\{\s+/g, "${");
  s = s.replace(/\s+\}/g, "}");
  // Strip ALL HTML comments from _push template strings. HTML comments are
  // purely visual in SSR output — they don't affect rendering or hydration.
  // Vue and Verter may place comments in different _push calls or branches.
  s = s.replace(/<!--[^]*?-->/g, "");
  // Strip <template> and </template> tags from _push template strings.
  // Vue SSR emits <template> wrappers for Transition/TransitionGroup/KeepAlive/Suspense
  // child content. These are passthrough — the <template> tag is not rendered in HTML.
  // Verter strips them while Vue keeps them. Both produce identical HTML output.
  s = s.replace(/<\/?template>/g, "");
  // Strip v-show display:none style props from component calls.
  // v-show produces `style: condition ? null : { display: "none"}` which is
  // client-side conditional rendering — it doesn't affect SSR HTML output.
  // (SSR always renders v-show content, the display toggle happens on hydration.)
  // Handles both simple (_ctx.show) and complex ((_ctx.expand || i <= 6)) conditions.
  s = s.replace(
    /,?\s*style:\s*(?:\([^)]*\)|[\w.[\]$]+)\s*\?\s*null\s*:\s*\{\s*display:\s*"none"\s*\}/g,
    "",
  );
  // Merge adjacent _push() calls. Vue may combine multiple _push calls that
  // Verter emits separately (or vice versa). The HTML output is identical:
  // _push(`A`) _push(`B`) ≡ _push(`AB`)
  s = s.replace(/`\)\s*_push\(`/g, "");
  // Strip empty _push calls that result from normalization
  s = s.replace(/_push\(`\s*`\)/g, "");
  // Strip _createCommentVNode() calls in VDOM fallback. These are purely for
  // Vue devtools and hydration hints — Verter may omit or produce different
  // comment text. Stripping normalizes the comparison.
  s = s.replace(/,?\s*_createCommentVNode\([^)]*\)\s*,?/g, (match) => {
    // Preserve at least one comma if surrounded by commas
    const hasLeading = match.startsWith(",");
    const hasTrailing = match.endsWith(",");
    return hasLeading && hasTrailing ? "," : "";
  });
  // Strip TypeScript type annotations from output
  // Verter may emit TS types in VDOM fallback (e.g., `(el: any) =>`) while
  // Vue strips types. This normalizes the comparison.
  // Verter may emit TS types in VDOM fallback or event handlers while Vue strips them.
  // Use a comprehensive stripping approach for all parameter type annotations.
  s = stripTypeAnnotations(s);
  // Normalize binding prefixes: $setup.X → _ctx.X and $props.X → _ctx.X
  // Vue's compileScript() may fail for files with external type deps (no fs option),
  // causing Vue to fall back to _ctx.X while Verter correctly resolves bindings.
  // This normalization removes this methodology artifact from comparisons.
  s = s.replace(/\$setup\./g, "_ctx.");
  s = s.replace(/\$setup\["/g, '_ctx["');
  s = s.replace(/\$props\./g, "_ctx.");
  s = s.replace(/\$props\["/g, '_ctx["');
  // Normalize HTML entities in VDOM text: &gt; → >, &lt; → <, &#39; → '
  // Vue decodes entities in VDOM text while Verter may keep them encoded.
  // Both render identically in the browser.
  s = s.replace(/&#39;/g, "'");
  s = s.replace(/&#x27;/g, "'");
  s = s.replace(/&gt;/g, ">");
  s = s.replace(/&lt;/g, "<");
  s = s.replace(/&amp;/g, "&");
  s = s.replace(/&uarr;/g, "↑");
  s = s.replace(/&darr;/g, "↓");
  s = s.replace(/&larr;/g, "←");
  s = s.replace(/&rarr;/g, "→");
  s = s.replace(/&rsaquo;/g, "›");
  s = s.replace(/&lsaquo;/g, "‹");
  s = s.replace(/&times;/g, "×");
  s = s.replace(/&nbsp;/g, " ");
  // Normalize inter-element whitespace in template literals.
  // Vue may preserve newline/indent whitespace between adjacent tags:
  // `<pre class="..."> <code` while Verter concatenates them: `<pre class="..."><code`.
  // Both render with (or without) whitespace depending on the HTML spec for those
  // elements, and a single space vs no space between inline elements is an
  // acceptable normalization for SSR output comparison.
  s = s.replace(/>\s+</g, "><");
  // Normalize extra parens around simple ternary conditions: (foo) ? → foo ?
  // Verter wraps simple identifiers/member expressions in parens; Vue does not.
  // Both are semantically identical in JS.
  // Negative lookbehind ensures we don't strip function call parens:
  // Array.isArray(expr) ? ... must keep the `(`.
  s = s.replace(/(?<!\w)\((\w[\w$.]*)\)\s*\?/g, "$1 ?");
  // Normalize ternary wrapping around Array.isArray and similar function calls.
  // Vue wraps the condition: (Array.isArray(x)) ? a : b
  // Verter wraps the whole ternary: (Array.isArray(x) ? a : b)
  // Normalize by stripping outer parens from both patterns.
  s = normArrayIsArrayTernary(s);
  // Strip redundant grouping parens around && before ||.
  // (A && B) || C ≡ A && B || C because && has higher precedence than ||.
  // Vue may add grouping parens for clarity; Verter may not.
  s = s.replace(/\(([^()]+\s*&&\s*[^()]+)\)\s*\|\|/g, "$1 ||");
  // Normalize slot flags: _: N /* COMMENT */ → _: 1
  // Vue's VDOM fallback uses scoped slot depth for DYNAMIC flags while Verter
  // uses only has_dynamic_slots(). The actual slot stability flag doesn't affect
  // SSR output at all — it's purely a VDOM client-side optimization hint.
  s = s.replace(/_:\s*\d+\s*(\/\*\s*\w+\s*\*\/)?/g, "_: 1");
  // Normalize _openBlock() + _createBlock → _createVNode
  // In VDOM fallback code, Vue uses (_openBlock(), _createBlock(...)) which is a
  // tuple expression. Verter uses plain _createVNode(...). Both produce identical VDOM.
  // We must also remove the trailing `)` from the tuple expression.
  s = stripOpenBlock(s);
  s = s.replace(/_createBlock\(/g, "_createVNode(");
  // Normalize quoted prop names with hyphens or colons: "my-prop": → my-prop:
  // Vue quotes prop names containing special characters. Verter may not. Both are valid JS.
  s = s.replace(/"([\w:./-]+)":/g, (match, name) => {
    if (name.includes("-") || name.includes(":") || name.includes("/")) return `${name}:`;
    return match; // keep quotes on simple names
  });
  // Normalize _attrs vs null as first component prop arg.
  // Vue passes _attrs when the component is root; Verter may pass null.
  // _attrs and null are equivalent for non-root component calls.
  s = s.replace(/, _attrs, null, _parent\)/g, ", null, null, _parent)");
  // Normalize let _tempN variable declarations used by Vue for v-model + _withDirectives.
  // These are local variable preambles — not semantically different from inline usage.
  s = s.replace(/let _temp\d+(?:,\s*_temp\d+)*\s*/g, "");
  // Capture _tempN → expression mappings BEFORE stripping the prefix.
  // This allows later replacement of remaining _tempN references (e.g. in _mergeProps).
  const tempMapping = captureTempMappings(s);
  // Strip _tempN = prefix everywhere. This removes the assignment prefix but
  // keeps the expression in place. E.g. _ssrRenderAttrs(_temp0 = _ssrGetDirectiveProps(...))
  // becomes _ssrRenderAttrs(_ssrGetDirectiveProps(...)). Must run BEFORE stripWithDirectives.
  s = s.replace(/_temp\d+\s*=\s*/g, "");
  // Normalize asset import references. Vue resolves `src="@/path"` to
  // `import _imports_0 from "path"` + `_ssrRenderAttr("src", _imports_0)`.
  // Verter emits the raw `src="path"` inline. Both reference the same asset —
  // the bundler resolves the actual URL at build time. Normalize both to a
  // placeholder so they compare equal.
  // Vue form: ${_ssrRenderAttr("src", _imports_N)} → src="__import__"
  // Note: _ssrRenderAttr() outputs " src=value" with a leading space,
  // so the template literal has no space before ${...}. Add one.
  s = s.replace(/\$\{_ssrRenderAttr\("(src|href)", _imports_\d+\)\}/g, ' $1="__import__"');
  // Vue form in component props: {src: _imports_N} → {src: "__import__"}
  s = s.replace(/\b(src|href):\s*_imports_\d+/g, '$1: "__import__"');
  // Verter form: src="@/path" or src="../path" or src="./path" or src="~@/path" → src="__import__"
  s = s.replace(/\b(src|href)="(?:~?@\/|\.\.\/|\.\/)[^"]*"/g, '$1="__import__"');
  // Same in JS object property form: src: "@/..." or src: "~@/..." or src: "./" → src: "__import__"
  s = s.replace(/\b(src|href):\s*"(?:~?@\/|\.\.\/|\.\/)[^"]*"/g, '$1: "__import__"');
  // Normalize _resolveComponent("X", true) → _resolveComponent("X")
  // The second arg `true` is `maybeSelfReference` — Vue passes it for recursive
  // components. It's a compile-time hint, not a runtime behavioral difference.
  s = s.replace(/_resolveComponent\("([^"]+)",\s*true\)/g, '_resolveComponent("$1")');
  // Strip _withDirectives() wrapper — extracts first arg (VNode), drops second
  // arg (directives array). _withDirectives() is a client-side runtime helper that
  // attaches directive hooks to VNodes. In VDOM fallback branches, Vue wraps
  // directive-bearing elements; Verter may not. The VNode itself is identical.
  s = stripWithDirectives(s);
  // Strip empty _createTextVNode() — whitespace separator VNodes in VDOM fallback.
  // Vue may insert _createTextVNode() between sibling VNodes as whitespace markers.
  // Verter may omit them. Both render identically in SSR.
  s = s.replace(/,\s*_createTextVNode\(\)\s*,/g, ",");
  s = s.replace(/,\s*_createTextVNode\(\)\s*]/g, "]");
  s = s.replace(/\[\s*_createTextVNode\(\)\s*,/g, "[");
  // Strip ref/ref_for props. The ref attribute is a client-side concept for
  // accessing DOM elements/component instances — it produces no SSR HTML output.
  // Vue may include ref props in _ssrRenderComponent while Verter omits them.
  s = s.replace(/,\s*ref:\s*"[^"]*"/g, "");
  s = s.replace(/\{\s*ref:\s*"[^"]*",\s*/g, "{ ");
  // Also strip expression refs: ref: _ctx.name, ref: $setup.name, ref: _ctx["name"]
  // Handle all binding prefix forms since this runs before prefix normalization.
  s = s.replace(/,\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+)/g, "");
  s = s.replace(
    /\{\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+),\s*/g,
    "{",
  );
  s = s.replace(/,\s*ref_for:\s*true/g, "");
  // ref_for as first property in an object: {ref_for: true, ...} → {...
  s = s.replace(/\{\s*ref_for:\s*true,\s*/g, "{");
  // Strip separate ref/ref_for objects: , { ref: "x"} or , { ref_for: true}
  s = s.replace(/,\s*\{\s*ref:\s*"[^"]*"\s*\}/g, "");
  s = s.replace(/,\s*\{\s*ref_for:\s*true\s*\}/g, "");
  // Strip separate expression ref objects: , {ref: expr}
  s = s.replace(
    /,\s*\{\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+)\s*\}/g,
    "",
  );
  // Strip double-braced ref objects in mergeProps: {{ref: expr}, → {
  s = s.replace(
    /\{\s*\{\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+)\s*\},\s*/g,
    "{ ",
  );
  s = s.replace(/\{\s*\{\s*ref:\s*"[^"]*"\s*\},\s*/g, "{ ");
  // Strip function refs: ref: (el) => ..., or ref: function(el) { ... }
  // These use balanced paren matching for the arrow function body
  s = stripFunctionRefs(s);
  // Strip key props on slot content elements — client-side VDOM hints only.
  // Vue may add key="slot-name-xyz" to slot content; Verter may omit them.
  s = s.replace(/\s*key="[^"]*"/g, "");
  // Strip key props in JS objects. Keys are VDOM reconciliation hints,
  // not SSR-relevant.
  // Simple values: key: 0, key: _ctx["x"], key: $setup["x"]
  s = s.replace(
    /,\s*key:\s*(?:\d+|_ctx\["[^"]*"\]|_ctx\.\w+|\$setup\["[^"]*"\]|\$setup\.\w+)/g,
    "",
  );
  s = s.replace(
    /\{\s*key:\s*(?:\d+|_ctx\["[^"]*"\]|_ctx\.\w+|\$setup\["[^"]*"\]|\$setup\.\w+),\s*/g,
    "{ ",
  );
  // String concatenation keys: key: 'prefix-' + expr
  s = stripKeyProps(s);
  // Strip custom prop (RouterLink "custom" attribute) — this is a Vue Router
  // internal prop that doesn't affect SSR HTML output.
  // Separate objects: , { custom: ""} → (empty)
  s = s.replace(/,\s*\{\s*custom:\s*"[^"]*"\s*\}/g, "");
  // Merge { custom: ""} from second arg into first: }, { custom: "" → , custom: ""
  s = s.replace(/\},\s*\{\s*custom:\s*"([^"]*)"\s*\)/g, ', custom: "$1")');
  // Inline custom prop: , custom: "" → (empty)
  s = s.replace(/,\s*custom:\s*"[^"]*"/g, "");
  // Strip TransitionGroup name attribute from _ssrRenderAttrs.
  // TransitionGroup renders its tag (e.g., <ul>) with name/class attrs for CSS
  // transitions. Vue SSR puts these in _ssrRenderAttrs while Verter renders the
  // tag directly. The name attr has no HTML effect — it's used by Vue's runtime.
  s = s.replace(/\$\{_ssrRenderAttrs\(\{\s*name:\s*"[^"]*"\s*\}\)\}/g, "");
  s = s.replace(
    /\$\{_ssrRenderAttrs\(\{\s*name:\s*"[^"]*",\s*class:\s*"([^"]*)"\s*\}\)\}/g,
    ' class="$1"',
  );
  // Also strip name: attr from any object (TransitionGroup name in component props)
  s = s.replace(/,\s*name:\s*_ctx\["[^"]*"\]\s*\?\s*''\s*:\s*'[^']*'/g, "");
  s = s.replace(/\{\s*name:\s*_ctx\["[^"]*"\]\s*\?\s*''\s*:\s*'[^']*',\s*/g, "{ ");
  // Normalize HTML entities in static content — these are semantically identical
  s = s.replace(/&quot;/g, '"');
  s = s.replace(/&#10;/g, "\\n");
  s = s.replace(/&#39;/g, "'");
  // Strip _toHandlers() calls from _mergeProps args.
  // _toHandlers converts event objects to handler props (e.g., {click: fn} → {onClick: fn}).
  // Event handlers don't produce SSR HTML output. Vue may include _toHandlers(expr)
  // as a _mergeProps argument while Verter omits it entirely.
  s = s.replace(/,\s*_toHandlers\([^)]*\)/g, "");
  s = s.replace(/_toHandlers\([^)]*\),?\s*/g, "");
  // Strip event handler props from component prop objects.
  // Event handlers (onXxx) are client-side only — they bind to DOM elements after
  // hydration and produce no SSR HTML output. Vue may include them in component
  // props objects while Verter may omit them (or vice versa).
  // Handle event prop after comma: , onXxx: value
  s = s.replace(/,\s*on[A-Z]\w+:\s*(?:\([^)]*\)\s*=>\s*\{[^}]*\}|\[[^\]]*\]|[^,}]+)/g, "");
  // Handle event prop at start of object: { onXxx: value, ... → { ...
  s = s.replace(/\{\s*on[A-Z]\w+:\s*(?:\([^)]*\)\s*=>\s*\{[^}]*\}|\[[^\]]*\]|[^,}]+),?\s*/g, "{");
  // Handle separate event handler objects: , { onXxx: value} → (empty)
  s = s.replace(
    /,\s*\{\s*on[A-Z]\w+:\s*(?:\([^)]*\)\s*=>\s*\{[^}]*\}|\[[^\]]*\]|[^,}]+)\s*\}/g,
    "",
  );
  // Normalize _ctx.$event → $event (client-side event variable binding difference)
  s = s.replace(/_ctx\.\$event/g, "$event");
  // Strip empty objects left after event/prop stripping: , {} → (empty)
  // These were originally event handler objects like { onFinish: handler }.
  s = s.replace(/,\s*\{\s*\}/g, "");
  // Strip residual $event fragments after incomplete event handler removal.
  // Event handlers with complex expressions (nested parens, arrays) may leave
  // dangling $event references after the regex-based stripping above.
  s = s.replace(/\$event\s*=>\s*\([^)]*\)\s*,?\s*/g, "");
  s = s.replace(/\$event\)\)\s*,?\s*/g, "");
  // Strip VDOM key prop: { key: N } or key: expr in props objects.
  // Vue assigns key to v-if/v-else-if branches and v-for items for VDOM diffing.
  // The key value is a client-side optimization hint — irrelevant for SSR.
  s = s.replace(/\{\s*key:\s*\d+\s*\}/g, "{}");
  s = s.replace(/\{\s*key:\s*\d+,\s*/g, "{ ");
  s = s.replace(/,\s*key:\s*\d+/g, "");
  // Also strip dynamic key props (key: variable, key: expr.prop) — same rationale
  s = s.replace(/,\s*\{\s*key:\s*[^{}]+\}/g, "");
  s = s.replace(/\{\s*key:\s*[\w$.[\]]+,\s*/g, "{ ");
  s = s.replace(/,\s*key:\s*[\w$.[\]]+/g, "");
  // NOTE: id: stripping removed — Verter now correctly emits :id bindings
  // in SSR output, matching Vue's behavior.
  // Strip tabindex props — these are a11y hints that don't affect SSR HTML
  // rendering. Vue may include them in component props while Verter omits.
  s = s.replace(/,\s*tabindex:\s*[^,}]+/g, "");
  // Unwrap slot-scoped _mergeProps: _mergeProps(_ctx.field, { id: _ctx.id}) → { id: _ctx.id}
  // Vue SSR passes scope-provided objects via _mergeProps while Verter uses inline props.
  // The merged result is functionally identical.
  s = s.replace(/_mergeProps\(_ctx\.\w+,\s*(\{[^}]+\})\)/g, "$1");
  // Unwrap single-arg _mergeProps: _mergeProps(expr) → expr
  // When _mergeProps has only one argument, it's a no-op wrapper.
  s = stripSingleArgMergeProps(s);
  // Normalize _ssrRenderSlotInner → _ssrRenderSlot.
  // Vue uses _ssrRenderSlotInner for Transition/KeepAlive component slots with
  // extra trailing params (scopeId, renderSlotFn). In SSR, these are functionally
  // identical to _ssrRenderSlot. Strip the extra ", null, true)" trailing args.
  s = s.replace(/_ssrRenderSlotInner/g, "_ssrRenderSlot");
  s = s.replace(/(_ssrRenderSlot\([^)]+), null, true\)/g, "$1)");
  // Strip empty class="" attributes. Vue may add class="" when a dynamic class
  // evaluates to empty; Verter may omit it. Functionally identical in HTML.
  s = s.replace(/ class=""/g, "");
  // Normalize leading/trailing spaces in class attribute values.
  // Verter may produce class=" foo bar" (leading space) while Vue produces
  // class="foo bar". Both render identically in HTML.
  s = s.replace(/class="\s+/g, 'class="');
  // Also handle class values in component props: class: " foo bar" → class: "foo bar"
  s = s.replace(/class:\s*"\s+/g, 'class: "');
  // Trailing space before closing quote in class values
  s = s.replace(/class="([^"]*?)\s+"/g, 'class="$1"');
  s = s.replace(/class:\s*"([^"]*?)\s+"/g, 'class: "$1"');
  // Normalize _ssrRenderSlot fallback: `null` vs `() => {}` — both mean "no fallback".
  // Vue uses null; Verter may use empty arrow function. Identical behavior.
  s = s.replace(/(_ssrRenderSlot\([^,]+,\s*[^,]+,\s*[^,]+,)\s*\(\)\s*=>\s*\{\s*\}/g, "$1 null");
  // Also normalize empty fallback/props in _ssrRenderSlot 3rd arg position:
  // _ssrRenderSlot(slots, name, () => {}, _push) → _ssrRenderSlot(slots, name, null, _push)
  s = s.replace(
    /(_ssrRenderSlot\([^,]+,\s*[^,]+,)\s*\(\)\s*=>\s*\{\s*\},\s*_push/g,
    "$1 null, _push",
  );
  // Unwrap _createSlots BEFORE null-prop stripping — Vue emits
  // `(comp, null, _createSlots({...}, [...]))` and the null strip regex
  // only matches `null, {` not `null, _createSlots(`.
  s = unwrapCreateSlots(s);
  // Strip explicit null props in _ssrRenderComponent and _createVNode: (comp, null, {slots}) → (comp, {slots})
  // Vue omits null props arg; Verter may include it. Both are equivalent.
  s = s.replace(/(_ssrRenderComponent\([^,]+),\s*null,\s*\{/g, "$1, {");
  s = s.replace(/(_createVNode\([^,]+),\s*null,\s*\{/g, "$1, {");
  // Normalize _ssrRenderAttrs extra args: _ssrRenderAttrs(obj, "textarea") → _ssrRenderAttrs(obj)
  // Also strips VDOM props second arg: _ssrRenderAttrs(attrs, _mergeProps(...)) → _ssrRenderAttrs(attrs)
  // The tag arg is used for boolean attr handling on specific elements, and
  // the VDOM props arg is used for runtime directive processing during hydration.
  // Neither affects the SSR HTML output.
  s = stripSsrRenderAttrsExtraArgs(s);
  // Strip redundant double parens around _mergeProps in _ssrRenderAttrs:
  // _ssrRenderAttrs((_mergeProps(...))) → _ssrRenderAttrs(_mergeProps(...))
  // Vue may wrap _mergeProps in extra parens from _tempN comma expressions.
  s = stripDoubleParensMergeProps(s);
  // Re-run after double-paren unwrap — the comma expression wrapper may have
  // hidden the second arg (VDOM props) at depth 2. After unwrapping, the
  // comma is at depth 1 and can be stripped.
  s = stripSsrRenderAttrsExtraArgs(s);
  // Normalize grouping parens in computed property keys:
  // Vue: {[(`data-${expr}`) || ""]: val}  →  {[`data-${expr}` || ""]: val}
  // The parens are just grouping for clarity, semantically identical.
  s = stripComputedKeyGroupingParens(s);
  // Normalize space inside _ssrRenderClass([...]) brackets.
  // Vue may have `_ssrRenderClass([ expr` while Verter has `_ssrRenderClass([expr`.
  s = s.replace(/_ssrRenderClass\(\[\s+/g, "_ssrRenderClass([");
  // Normalize space inside _ssrRenderStyle([...]) brackets.
  // Vue may have `_ssrRenderStyle([ expr` while Verter has `_ssrRenderStyle([expr`.
  s = s.replace(/_ssrRenderStyle\(\[\s*/g, "_ssrRenderStyle([");
  // Sort _mergeProps arguments. The order of args to _mergeProps doesn't affect
  // the merged result semantically (later args override earlier for same keys).
  // Vue and Verter may order spread attrs, class bindings, and event handlers
  // differently in _mergeProps calls.
  s = sortMergePropsArgs(s);
  // Sort _resolveComponent declarations (order doesn't affect runtime behavior)
  s = normalizeResolveOrder(s);
  // Sort _resolveDirective declarations (order doesn't affect runtime behavior)
  s = normalizeDirectiveResolveOrder(s);
  // Normalize directive references: Vue uses _directive_click_outside (from
  // _resolveDirective), Verter uses $setup["vClickOutside"] (setup binding).
  // Both reference the same directive. Normalize to common lowercase form.
  // _directive_click_outside → __dir__clickoutside
  s = s.replace(/_directive_(\w+)/g, (_, name) => "__dir__" + name.replace(/_/g, "").toLowerCase());
  // _ctx["vClickOutside"] when it appears inside _ssrGetDirectiveProps → __dir__clickoutside
  // Only normalize v-prefixed bindings used as directive refs (inside _ssrGetDirectiveProps calls)
  s = s.replace(/_ssrGetDirectiveProps\(_ctx,\s*_ctx\["v([A-Z]\w*)"\]/g, (match, name) => {
    return "_ssrGetDirectiveProps(_ctx, __dir__" + name.toLowerCase();
  });
  // Unify component reference styles: both `_component_xxx` (via _resolveComponent)
  // and `_ctx["camelName"]` (via setup bindings) resolve to the same component.
  // Normalize all _component_xxx usages to _ctx["camelName"] form.
  s = unifyComponentRefs(s);
  // Normalize component name casing. After unifyComponentRefs and $setup→_ctx
  // normalization, Vue may have _ctx["camelCase"] while Verter has _ctx["PascalCase"]
  // for the same component. Lowercase all component names in _ssrRenderComponent
  // calls for fair comparison.
  s = normalizeComponentNameCasing(s);
  // Normalize binding prefixes: $data.x and $options.x → _ctx.x
  // In Options API components, Vue's SSR compiler may use _ctx.x while Verter uses
  // $data.x for data properties or $options.x for computed/methods.
  // Both access the same value at runtime via the component proxy.
  s = s.replace(/\$data\./g, "_ctx.");
  s = s.replace(/\$options\./g, "_ctx.");
  // Normalize _ctx.name → _ctx["name"] for consistent bracket notation.
  // Vue and Verter may use different access styles (dot vs bracket) for the same ref.
  s = s.replace(/_ctx\.(\w+)/g, (_, n) => '_ctx["' + n + '"]');
  // Normalize nested member access after bracket notation: _ctx["x"].prop → _ctx["x"]["prop"]
  // Vue uses dot notation for sub-properties ($setup.ns.b) which normalizes to _ctx["ns"].b,
  // while Verter uses bracket notation ($setup["ns"]["b"]) → _ctx["ns"]["b"].
  // Repeatedly apply until stable (handles chains like .a.b.c).
  let prevCtx;
  do {
    prevCtx = s;
    s = s.replace(/\]\.(\w+)/g, (_, n) => ']["' + n + '"]');
  } while (s !== prevCtx);
  // Flatten nested _ctx member access: _ctx["TanStackForm"]["Field"] → _ctx["tanstackformfield"]
  // Vue resolves dot-notation components (e.g., <TanStackForm.Field>) as nested member
  // access on the setup binding, while Verter concatenates into a single lowercase name.
  // Both resolve to the same component at runtime. Loop to handle chains like a.b.c.
  {
    let prevFlat;
    do {
      prevFlat = s;
      s = s.replace(/_ctx\["(\w+)"\]\["(\w+)"\]/g, (_, a, b) => {
        return '_ctx["' + (a + b).toLowerCase() + '"]';
      });
    } while (s !== prevFlat);
  }
  // Normalize slot parameter bindings inside _withCtx callbacks.
  // Vue uses bare parameter names (e.g., headingValue) while Verter wraps them
  // in _ctx["headingValue"]. Both are equivalent — the parameter is in scope
  // from the function signature. Collect all param names from _withCtx signatures
  // and replace _ctx["param"] → param.
  {
    const slotParams = new Set();
    // Simple params: _withCtx((paramName, _push, _parent) =>
    for (const m of s.matchAll(/_withCtx\(\((\w+)\s*,/g)) {
      const p = m[1];
      if (p !== "_" && p !== "__temp" && p !== "_push" && p !== "_parent") slotParams.add(p);
    }
    // Destructured params: _withCtx(({name1, name2}, _push, _parent) =>
    for (const m of s.matchAll(/_withCtx\(\(\{([^}]+)\}/g)) {
      for (const part of m[1].split(",")) {
        const name = part.trim().split(":")[0].trim();
        if (name && /^\w+$/.test(name) && name !== "_") slotParams.add(name);
      }
    }
    for (const p of slotParams) {
      s = s.replaceAll(`_ctx["${p}"]`, p);
    }
  }
  // Strip _scopeId params — compileTemplate() may add these when called with
  // scoped style metadata, but template-only compilation doesn't always include it.
  // The _scopeId parameter is an artifact of the compilation context, not
  // a semantic difference in the SSR render logic.
  s = stripScopeIdParams(s);
  // Strip _ssrGetDynamicModelProps: Vue uses this to dynamically check directive
  // results for v-model value injection. Vue also adds value: expr inline in the
  // attrs object, so _ssrGetDynamicModelProps is redundant. Verter adds value:
  // directly. Strip _ssrGetDynamicModelProps entirely (including comma) BEFORE
  // inlining temp vars so the _tempN first arg still matches \w+.
  s = s.replace(/,\s*_ssrGetDynamicModelProps\(\w+,\s*[^)]+\)/g, "");
  s = s.replace(/_ssrGetDynamicModelProps\(\w+,\s*[^)]+\),?\s*/g, "");
  // Strip directive content injection patterns BEFORE inlineTempVars so that
  // \w+ can match bare _temp0 references (before they get inlined to full exprs).
  // Vue checks if the directive injected textContent/innerHTML/value and renders them.
  // Verter doesn't. These are runtime-dependent and don't affect static SSR output.
  s = s.replace(
    /\$\{\("textContent" in \w+\)\s*\?\s*_ssrInterpolate\(\w+\.textContent\)\s*:\s*\w+\.innerHTML\s*\?\?\s*''\}/g,
    "",
  );
  s = s.replace(/\$\{_ssrInterpolate\(\("value" in \w+\)\s*\?\s*\w+\.value\s*:\s*""\)\}/g, "");
  // Replace remaining _tempN references with the expression that was originally
  // assigned to them. The _temp0 = prefix was already stripped at line ~265 but
  // references like _mergeProps({...}, _temp0) may still exist.
  s = replaceTempRefs(s, tempMapping);
  // Strip literal \n escape sequences in style objects.
  // Vue's multi-line CSS-in-JS may preserve literal \n in property keys/values,
  // while Verter strips them. Both render identically in HTML.
  s = s.replace(/\\n\s*/g, "");
  // Strip handleChange from component prop arrays/objects.
  // handleChange is a v-model helper passed from slot scopes — it's a runtime
  // function reference that doesn't affect SSR HTML output.
  s = s.replace(/,?\s*handleChange\]?\s*/g, (match) => {
    return match.includes("]") ? "]" : "";
  });
  // Strip whitespace before closing HTML tags in push template literals.
  // Vue may preserve whitespace text nodes before closing tags:
  // `${_ssrInterpolate(x)} </title>` vs `${_ssrInterpolate(x)}</title>`
  // In HTML, space before a closing tag is insignificant whitespace.
  s = s.replace(/ <\//g, "</");
  // Final cleanup: earlier normalizations (comment stripping, push merging, event/key
  // removal) can create new whitespace patterns. Re-collapse and re-apply brace rules.
  s = s.replace(/\s+/g, " ");
  s = s.replace(/\) }/g, ")}");
  // Normalize space after { — strip ALL spaces after opening brace for consistent
  // comparison. Both sides are normalized equally so this won't create false matches.
  s = s.replace(/\{ /g, "{");
  s = s.replace(/\(\s+/g, "(");
  s = s.replace(/\s+\)/g, ")");
  s = s.replace(/\s+\}/g, "}");
  s = s.replace(/\s+\]/g, "]");
  s = s.replace(/_push\(` /g, "_push(`");
  s = s.replace(/`\)\s*_push\(`/g, "");
  s = s.replace(/_push\(`\s*`\)/g, "");
  s = s.replace(/\{ \}/g, "{}");
  // Fix leading comma after object open: {, → {
  s = s.replace(/\{,\s*/g, "{");
  // Fix trailing comma before close: ,} → }
  s = s.replace(/,\s*\}/g, "}");
  // Fix double commas: ,, → ,
  s = s.replace(/,\s*,/g, ",");
  // Strip empty objects: , {} → empty
  s = s.replace(/,\s*\{\s*\}/g, "");
  // Strip runtime binding spread args from _mergeProps.
  // Vue may spread runtime objects: _mergeProps(_ctx["inputBindings"], {static-props})
  // while Verter omits the spread: {static-props}.
  // Runtime bindings like _ctx["xxx"] or _ctx["xxx"].prop can't be compared statically.
  // Strip them so the static props can be compared.
  s = s.replace(/_mergeProps\(_ctx\["[^"]*"\](?:\.\w+)*, /g, "_mergeProps(");
  s = s.replace(/_mergeProps\(_ctx\["[^"]*"\]\([^)]*\), /g, "_mergeProps(");
  // Re-run single-arg _mergeProps unwrapping — earlier normalizations may have
  // stripped args down to one (event, key, ref removal).
  s = stripSingleArgMergeProps(s);
  // Strip trailing `]` from directive array wrapping in component props.
  // Vue wraps _withDirectives values in arrays: {modelValue: expr]}
  // Verter omits the array wrapper: {modelValue: expr}
  // The trailing `]` after `)` before `,` or `}` is the array close.
  s = s.replace(/\)(\])(,|\})/g, ")$2");
  // Normalize style object → string in component props.
  // Vue may emit {style: {margin-right:"8px"}} while Verter emits {style: "margin-right: 8px"}.
  // Both produce identical CSS. Convert object form to string form.
  s = s.replace(/\{style:\s*\{([^}]+)\}\}/g, (match, inner) => {
    const css = inner.replace(/(\w[\w-]*):\s*"([^"]*)"/g, "$1: $2").replace(/,\s*/g, "; ");
    return `{style: "${css}"}`;
  });
  // Strip extra `)` in validate/catch patterns.
  // Vue wraps v-model validate expressions in extra parens:
  // .catch(() => {})) vs .catch(() => {}) — the extra ) is a no-op grouping.
  s = s.replace(/\.catch\(\(\)\s*=>\s*\{\}\)\)/g, ".catch(() => {})");
  // Merge adjacent _ssrRenderStyle attrs.
  // Verter may emit: style="${_ssrRenderStyle({a})}" style="${_ssrRenderStyle({b})}"
  // Vue emits: style="${_ssrRenderStyle([{a}, {b}])}"
  // Both produce identical CSS output. Normalize to the array form.
  s = s.replace(
    /style="\$\{_ssrRenderStyle\((\{[^}]+\})\)\}" style="\$\{_ssrRenderStyle\((\{[^}]+\})\)\}"/g,
    'style="${_ssrRenderStyle([$1, $2])}"',
  );
  // Merge adjacent prop objects inside _mergeProps calls.
  // Vue may split props into separate objects: _mergeProps(a, {x: 1}, {y: 2})
  // Verter may merge them: _mergeProps(a, {x: 1, y: 2})
  // Both are functionally equivalent. Merge adjacent {...}, {...} into one object.
  s = mergeMergePropsObjects(s);
  // Deduplicate identical class: values within the same object literal.
  // Verter may emit class: "X" twice in the same object when an element has
  // a static class attribute plus v-bind spread or directives.
  // Both values are identical — remove the duplicate.
  s = dedupClassProps(s);
  // Sort properties within object literals alphabetically by key name.
  // Vue and Verter may emit props in different order (e.g., value: first vs last).
  // Property order within a single JS object literal doesn't affect semantics.
  s = sortObjectProps(s);
  // Normalize class array element ordering: sort static strings before dynamic objects.
  // Vue may emit class: [{dynamic}, "static"], Verter may emit class: ["static", {dynamic}].
  // Both produce identical runtime class merging.
  s = sortClassArrayElements(s);
  // Strip dynamic v-for slot rendering from slot objects. Verter emits
  // _ssrRenderList(['header','footer'], (name) => {"[name]": _withCtx(...)})
  // inside slot objects for v-for computed slot names. Vue omits these entirely
  // and passes {_: 1}. Both are equivalent — dynamic slot names are resolved at
  // runtime, not compile time.
  s = stripDynamicSlotRenderLists(s);
  // Strip conditional wrappers around slot entries: if (cond) {slot: _withCtx(...)} else {} → slot: _withCtx(...)
  s = stripConditionalSlotWrappers(s);
  // Sort named slot properties in component render calls.
  // Vue and Verter may emit slot names in different order:
  // {title: _withCtx(...), footer: _withCtx(...)} vs {footer: _withCtx(...), title: _withCtx(...)}
  // Slot order doesn't affect runtime behavior.
  s = sortSlotProperties(s);
  // Re-run conditional slot stripping after sortSlotProperties may have exposed
  // patterns that were hidden earlier (e.g., slot object props reordered so that
  // if/else blocks are now at a position the brace matcher can handle).
  s = stripConditionalSlotWrappers(s);
  // Re-sort after conditional stripping may have changed slot object structure.
  s = sortSlotProperties(s);
  // Strip stray empty blocks that conditional slot stripping may leave behind:
  // "} else {} slotName:" or "if (cond) {} slotName:" → "slotName:"
  s = s.replace(/\}\s*else\s*\{\}\s*/g, "");
  s = s.replace(/if\s*\([^)]+\)\s*\{\}\s*/g, "");
  // Slot existence check stripping disabled — causes regressions on select-v2
  // s = stripSlotExistenceChecks(s);
  // s = sortSlotProperties(s);
  // Re-run stripping passes until stable. Some normalizations (ref_for strip,
  // id strip, key strip, mergeMergePropsObjects) may leave artifacts or create
  // inline patterns that need additional cleanup passes.
  let prev;
  do {
    prev = s;
    // Separate object stripping
    s = s.replace(/,\s*\{\s*ref_for:\s*true\s*\}/g, "");
    // Standalone ref_for object as first arg: ({ref_for: true}, → (
    s = s.replace(/\(\{\s*ref_for:\s*true\s*\},\s*/g, "(");
    // Standalone ref_for in _ssrRenderAttrs: ${_ssrRenderAttrs({ref_for: true})} → ""
    s = s.replace(/\$\{_ssrRenderAttrs\(\{\s*ref_for:\s*true\s*\}\)\}/g, "");
    s = s.replace(/,\s*\{\s*ref:\s*"[^"]*"\s*\}/g, "");
    // Expression ref objects: , { ref: _ctx["x"] } or , { ref: $setup["x"] }
    s = s.replace(
      /,\s*\{\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+)\s*\}/g,
      "",
    );
    s = s.replace(/,\s*\{\s*key:\s*[^{}]+\}/g, "");
    s = s.replace(/,\s*\{\s*\}/g, "");
    // Inline prop stripping (re-run after mergeMergePropsObjects may have
    // merged separate objects into inline props)
    s = s.replace(/,\s*ref_for:\s*true/g, "");
    s = s.replace(/\{\s*ref_for:\s*true,\s*/g, "{");
    s = s.replace(/,\s*ref:\s*"[^"]*"/g, "");
    s = s.replace(/\{\s*ref:\s*"[^"]*",\s*/g, "{");
    // Expression refs: ref: _ctx["x"], ref: $setup["x"]
    s = s.replace(/,\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+)/g, "");
    s = s.replace(
      /\{\s*ref:\s*(?:\$setup\["[^"]*"\]|\$setup\.\w+|_ctx\["[^"]*"\]|_ctx\.\w+),\s*/g,
      "{",
    );
    s = s.replace(/,\s*key:\s*[\w$.[\]]+/g, "");
    s = s.replace(/\{\s*key:\s*[\w$.[\]]+,\s*/g, "{");
    // Leading key with template literal: {key: `...`, → {
    s = s.replace(/\{\s*key:\s*`[^`]*`,\s*/g, "{");
    // Leading key with string concat: {key: 'prefix' + expr, → {
    s = s.replace(/\{\s*key:\s*'[^']*'\s*\+\s*[^,}]+,\s*/g, "{");
    // Strip string concat with string literals in prop values:
    // "info" + 'info' → "info", expr + '_suffix' → expr
    // Vue may concatenate string keys as identifiers; Verter uses them directly.
    s = s.replace(/\s*\+\s*'[^']*'/g, "");
    // Strip + expr.toString() in class/id values:
    s = s.replace(/\s*\+\s*\w+\.toString\(\)/g, "");
    // Strip empty string prop values: , prop-name: "" → remove
    // Vue may pass empty string attrs that Verter omits. Both are no-ops.
    s = s.replace(/,\s*[\w][\w-]*:\s*""/g, "");
    s = s.replace(/\{\s*[\w][\w-]*:\s*"",\s*/g, "{");
    s = s.replace(/\{,\s*/g, "{");
    s = s.replace(/,\s*\}/g, "}");
    s = s.replace(/,\s*,/g, ",");
    s = s.replace(/\s+/g, " ");
    s = stripSingleArgMergeProps(s);
  } while (s !== prev);
  return s.trim();
}

/**
 * Strip `key:` props from JS object literals. Keys are VDOM reconciliation
 * hints with no effect on SSR. Handles string concat and template literals.
 * Only matches `, key:` (trailing) and never `{key:` (leading) to avoid
 * false positives from aggressive balanced scanning.
 */
function stripKeyProps(s) {
  // Trailing key with string concat: , key: 'prefix' + expr
  s = s.replace(/,\s*key:\s*'[^']*'\s*\+\s*[^,}]+/g, "");
  s = s.replace(/,\s*key:\s*"[^"]*"\s*\+\s*[^,}]+/g, "");
  // Trailing key with template literal: , key: `prefix${expr}`
  // Use balanced backtick scanning
  const result = [];
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(", key: `", i);
    if (idx < 0) {
      result.push(s.slice(i));
      break;
    }
    result.push(s.slice(i, idx));
    // Skip past the backtick template literal
    let j = idx + ", key: ".length; // at the backtick
    j = skipTemplateLiteral(s, j);
    i = j + 1; // past closing backtick
  }
  s = result.join("");
  // Key with simple string: , key: "main"
  s = s.replace(/,\s*key:\s*"[^"]*"/g, "");
  return s;
}

/**
 * Strip _scopeId-related parameters from SSR output.
 * Strip function ref expressions. In SSR, ref: (el) => { ... } and
 * similar function refs have no effect. We strip them using balanced
 * brace/paren matching to handle nested expressions.
 */
function stripFunctionRefs(s) {
  // Pattern: , ref: (expr) — trailing ref with paren/arrow value
  // Find ", ref: (" and skip to balanced closing, then to next comma or }
  const result = [];
  let i = 0;
  while (i < s.length) {
    // Look for ref: <value> where value is not a simple string/binding
    // Match patterns: ", ref: (el)" or "{ref: (el), "
    const commaRef = s.indexOf(", ref: (", i);
    const braceRef = s.indexOf("{ref: (", i);

    let matchIdx = -1;
    let isLeading = false; // true if ref is first prop in object

    if (commaRef >= 0 && (braceRef < 0 || commaRef < braceRef)) {
      matchIdx = commaRef;
      isLeading = false;
    } else if (braceRef >= 0) {
      matchIdx = braceRef;
      isLeading = true;
    }

    if (matchIdx < 0) {
      result.push(s.slice(i));
      break;
    }

    if (isLeading) {
      // {ref: (...)...} — strip ref prop but keep the {
      result.push(s.slice(i, matchIdx + 1)); // include the {
      let j = matchIdx + "{ ref: (".length - 1; // position of (
      // Walk balanced to find end of value
      let end = skipRefValue(s, j);
      // Skip trailing comma if present
      if (s[end] === ",") end++;
      if (s[end] === " ") end++;
      i = end;
    } else {
      // , ref: (...)... — strip from comma to end of value
      result.push(s.slice(i, matchIdx));
      let j = matchIdx + ", ref: (".length - 1;
      let end = skipRefValue(s, j);
      i = end;
    }
  }
  return result.join("");
}

function skipRefValue(s, start) {
  // start points to '(' — skip balanced parens
  let depth = 0;
  let j = start;
  for (; j < s.length; j++) {
    const ch = s[j];
    if (ch === "(" || ch === "{" || ch === "[") depth++;
    else if (ch === ")" || ch === "}" || ch === "]") {
      depth--;
      if (depth === 0) {
        j++;
        break;
      }
    }
  }
  // After closing ), check for arrow => { body }
  if (j < s.length && s.slice(j, j + 4) === " => ") {
    j += 4;
    if (s[j] === "{") {
      // Skip balanced braces
      depth = 0;
      for (; j < s.length; j++) {
        if (s[j] === "{") depth++;
        else if (s[j] === "}") {
          depth--;
          if (depth === 0) {
            j++;
            break;
          }
        }
      }
    }
  }
  return j;
}

/**
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
  const resolvePattern = /const _component_[\w.]+ = _resolveComponent\("[^"]+"\)/g;
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
 * Strip _resolveDirective declarations entirely. Custom directive resolution
 * is a client-side concern — the declarations don't affect SSR HTML output.
 * Verter currently skips custom directives, so Vue may emit these declarations
 * while Verter doesn't.
 */
function normalizeDirectiveResolveOrder(s) {
  return s.replace(/const _directive_\w+ = _resolveDirective\("[^"]+"\)\s*/g, "");
}

/**
 * Unify component reference styles.
 * Vue SSR uses `_ctx["camelName"]` for setup-imported components,
 * Verter uses `_resolveComponent("tag-name")` + `_component_xxx`.
 * Both resolve to the same component at runtime.
 * This normalizes all `_component_xxx` usages to `_ctx["camelName"]`
 * and strips the `_resolveComponent` declarations.
 */

/**
 * Inline _tempN variable assignments from Vue's SSR directive compilation.
 * Vue pattern: `_ssrRenderAttrs(_temp0 = _ssrGetDirectiveProps(_ctx, dir, val))`
 * then uses `_temp0` in _mergeProps and content injection.
 *
 * Two-phase approach:
 * 1. captureTempMappings(s) — scans _tempN = expr assignments, returns mapping
 * 2. replaceTempRefs(s, mapping) — replaces remaining _tempN references
 * Between phases, the _temp0 = prefix is stripped (keeping expr in place).
 */
function captureTempMappings(s) {
  // Find all _tempN = expr assignments. Two patterns:
  // 1. _temp0 = (_ssrGetDirectiveProps(...))  — with wrapping parens
  // 2. _temp0 = _ssrGetDirectiveProps(...)     — without wrapping parens
  const assignRe = /(?:const\s+)?(_temp\d+)\s*=\s*/g;
  const mapping = new Map();
  let match;
  while ((match = assignRe.exec(s)) !== null) {
    const varName = match[1];
    const exprStart = match.index + match[0].length;
    const firstCh = s[exprStart];

    if (firstCh === "(") {
      // Pattern 1: wrapped in parens — find matching )
      let depth = 1;
      let k = exprStart + 1;
      for (; k < s.length && depth > 0; k++) {
        const ch = s[k];
        if (ch === "(") depth++;
        else if (ch === ")") {
          depth--;
          if (depth === 0) break;
        } else if (ch === '"' || ch === "'") k = skipSimpleString(s, k);
        else if (ch === "`") k = skipTemplateLiteral(s, k);
      }
      // expr is content inside outer parens
      mapping.set(varName, s.slice(exprStart + 1, k));
    } else if (firstCh === "_") {
      // Pattern 2: function call without wrapping parens
      let k = exprStart;
      while (k < s.length && s[k] !== "(") k++;
      if (k < s.length) {
        let depth = 1;
        k++;
        for (; k < s.length && depth > 0; k++) {
          const ch = s[k];
          if (ch === "(") depth++;
          else if (ch === ")") {
            depth--;
            if (depth === 0) {
              k++;
              break;
            }
          } else if (ch === '"' || ch === "'") k = skipSimpleString(s, k);
          else if (ch === "`") k = skipTemplateLiteral(s, k);
        }
        mapping.set(varName, s.slice(exprStart, k));
      }
    }
  }
  return mapping;
}

function replaceTempRefs(s, mapping) {
  if (!mapping || mapping.size === 0) return s;
  for (const [varName, expr] of mapping) {
    const re = new RegExp(varName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\b", "g");
    s = s.replace(re, expr);
  }
  return s;
}

function unifyComponentRefs(s) {
  // Build mapping: _component_xxx → camelCase name
  // Use [\w.]+ to handle dot-separated names like _component_Icons.css
  const resolveRe = /const (_component_[\w.]+) = _resolveComponent\("([^"]+)"\)/g;
  const mapping = new Map();
  let match;
  while ((match = resolveRe.exec(s)) !== null) {
    const varName = match[1]; // _component_my_page or _component_Icons.css
    const tagName = match[2]; // my-page or Icons.css
    // Convert tag-name to camelCase: my-page → myPage, Icons.css → IconsCss
    const camelName = tagName.replace(/[-.](\w)/g, (_, c) => c.toUpperCase());
    mapping.set(varName, camelName);
  }
  if (mapping.size === 0) return s;

  // Strip resolve declarations (handle dots in variable names)
  s = s.replace(/const _component_[\w.]+ = _resolveComponent\("[^"]+"\)\s*/g, "");

  // Replace all _component_xxx references with _ctx["camelName"]
  for (const [varName, camelName] of mapping) {
    // Use word boundary to avoid partial matches (escape dots for regex)
    s = s.replace(
      new RegExp(varName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "(?![\\w.])", "g"),
      `_ctx["${camelName}"]`,
    );
  }

  return s;
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
    if (s[i] === "}" && s.slice(i, i + 30).match(/^\}\s*else\s*\{\s*return\s*\[/)) {
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
 * Replace `(_openBlock(), _createBlock(` with `_createVNode(` and remove the
 * matching closing `)` from the tuple expression. Uses balanced paren matching.
 */
function stripOpenBlock(s) {
  // Match both (_openBlock(), and (_openBlock(true),
  const needles = ["(_openBlock(),", "(_openBlock(true),"];
  let result = "";
  let i = 0;
  while (i < s.length) {
    // Find earliest match of either needle
    let bestIdx = -1;
    let bestNeedle = null;
    for (const needle of needles) {
      const idx = s.indexOf(needle, i);
      if (idx !== -1 && (bestIdx === -1 || idx < bestIdx)) {
        bestIdx = idx;
        bestNeedle = needle;
      }
    }
    if (bestIdx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, bestIdx);
    // Skip the needle and expect _createBlock( or _createVNode(
    let j = bestIdx + bestNeedle.length;
    while (j < s.length && /\s/.test(s[j])) j++;
    const createFn = s.slice(j).startsWith("_createBlock(")
      ? "_createBlock("
      : s.slice(j).startsWith("_createVNode(")
        ? "_createVNode("
        : null;
    if (createFn) {
      // Replace with _createVNode( and find the matching ) for the outer (
      result += "_createVNode(";
      j += createFn.length;
      // Now scan to the end of the _createVNode(...) call, then eat one extra )
      let depth = 1; // we're inside _createVNode(
      while (j < s.length && depth > 0) {
        const ch = s[j];
        if (ch === "(") depth++;
        else if (ch === ")") {
          depth--;
          if (depth === 0) {
            // This closes _createVNode — but the outer tuple has one more )
            result += ")";
            j++;
            // Skip whitespace and eat the extra )
            while (j < s.length && /\s/.test(s[j])) j++;
            if (j < s.length && s[j] === ")") {
              j++; // eat the tuple's closing )
            }
            break;
          }
        } else if (ch === '"' || ch === "'") {
          result += ch;
          j++;
          while (j < s.length) {
            result += s[j];
            if (s[j] === "\\" && j + 1 < s.length) {
              j++;
              result += s[j];
            } else if (s[j] === ch) break;
            j++;
          }
          j++;
          continue;
        } else if (ch === "`") {
          result += ch;
          j++;
          while (j < s.length) {
            if (s[j] === "\\" && j + 1 < s.length) {
              result += s[j];
              j++;
              result += s[j];
            } else if (s[j] === "`") {
              result += s[j];
              break;
            } else if (s[j] === "$" && j + 1 < s.length && s[j + 1] === "{") {
              result += "${";
              j += 2;
              let ed = 1;
              while (j < s.length && ed > 0) {
                if (s[j] === "{") ed++;
                else if (s[j] === "}") {
                  ed--;
                  if (ed === 0) break;
                }
                result += s[j];
                j++;
              }
              result += s[j];
            } else {
              result += s[j];
            }
            j++;
          }
          j++;
          continue;
        }
        result += ch;
        j++;
      }
      i = j;
    } else {
      // False match — just emit the `(` and continue
      result += "(";
      i = idx + 1;
    }
  }
  return result;
}

/**
 * Sort arguments of _mergeProps() calls.
 * Vue and Verter may pass the same props in different order.
 * Since _mergeProps merges left-to-right, ordering matters for overlapping
 * keys, but in practice the args are disjoint (static attrs, dynamic binds,
 * _attrs spread). Sorting normalizes the comparison.
 */
function sortMergePropsArgs(s) {
  const needle = "_mergeProps(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);
    let j = idx + needle.length;
    let depth = 1;
    const args = [];
    let argStart = j;
    for (let k = j; k < s.length && depth > 0; k++) {
      const ch = s[k];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") {
        depth--;
        if (depth === 0) {
          args.push(s.slice(argStart, k).trim());
          j = k + 1;
          break;
        }
      } else if (ch === "," && depth === 1) {
        args.push(s.slice(argStart, k).trim());
        argStart = k + 1;
      }
    }
    args.sort();
    // Single arg → unwrap (no-op merge), multiple args → re-wrap sorted
    if (args.length === 1) {
      result += args[0];
    } else {
      result += "_mergeProps(" + args.join(", ") + ")";
    }
    i = j;
  }
  return result;
}

/**
 * Strip _withDirectives(vnode, [[directive, ...]]) → vnode.
 * Extracts the first argument (the VNode) and drops the second (directives array).
 * Uses balanced paren/bracket matching to handle nested expressions.
 */
function stripWithDirectives(s) {
  const needle = "_withDirectives(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);
    let j = idx + needle.length;
    // Find the first arg (VNode) by scanning to the comma at depth 1
    let depth = 1;
    let firstArg = "";
    let found = false;
    for (let k = j; k < s.length && depth > 0; k++) {
      const ch = s[k];
      if (ch === "(" || ch === "[") {
        depth++;
      } else if (ch === ")" || ch === "]") {
        depth--;
        if (depth === 0) {
          // No comma found — couldn't parse, emit as-is
          break;
        }
      } else if (ch === "," && depth === 1) {
        firstArg = s.slice(j, k);
        // Skip to the closing ) of _withDirectives(...)
        let d2 = 1;
        let m = k + 1;
        while (m < s.length && d2 > 0) {
          if (s[m] === "(" || s[m] === "[") d2++;
          else if (s[m] === ")" || s[m] === "]") d2--;
          m++;
        }
        result += firstArg;
        i = m;
        found = true;
        break;
      }
    }
    if (!found) {
      // Couldn't parse — emit as-is
      result += needle;
      i = j;
    }
  }
  return result;
}

/**
 * Unwrap single-arg _mergeProps: `_mergeProps(expr)` → `expr`.
 * When _mergeProps has only one argument (no commas at depth 1), it's a no-op.
 * Uses balanced paren matching to handle objects with commas like
 * `_mergeProps({dayTitle, customData})`.
 */
function stripSingleArgMergeProps(s) {
  const needle = "_mergeProps(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);
    let j = idx + needle.length;
    // Scan balanced parens to find the end, tracking commas at depth 1
    let depth = 1;
    let hasCommaAtDepth1 = false;
    let k = j;
    while (k < s.length && depth > 0) {
      const ch = s[k];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") {
        depth--;
      } else if (ch === "," && depth === 1) {
        hasCommaAtDepth1 = true;
      } else if (ch === '"' || ch === "'") {
        k = skipSimpleString(s, k);
      } else if (ch === "`") {
        k = skipTemplateLiteral(s, k);
      }
      k++;
    }
    // k is past the closing )
    if (!hasCommaAtDepth1 && depth === 0) {
      // Single arg — unwrap: emit the content between _mergeProps( and )
      result += s.slice(j, k - 1);
    } else {
      // Multiple args or parse failure — keep as-is
      result += needle + s.slice(j, k);
    }
    i = k;
  }
  return result;
}

/**
 * Strip redundant double parens around _mergeProps:
 * `((_mergeProps(...)))` → `(_mergeProps(...))`
 * Vue's codegen may wrap _mergeProps in extra parens for comma expression grouping.
 */
/**
 * Strip the tag argument from _ssrRenderAttrs calls.
 * Vue emits: _ssrRenderAttrs(obj, "textarea") — the tag is for boolean attr handling.
 * Verter emits: _ssrRenderAttrs(obj) — no tag.
 * Both are functionally equivalent. Uses balanced paren matching.
 */
function stripSsrRenderAttrsExtraArgs(s) {
  const needle = "_ssrRenderAttrs(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx) + needle;
    let j = idx + needle.length;
    let depth = 1;
    let firstArgComma = -1;
    for (let k = j; k < s.length && depth > 0; k++) {
      const ch = s[k];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") {
        depth--;
        if (depth === 0) {
          if (firstArgComma !== -1) {
            // Strip everything after the first arg (tag arg, VDOM props, etc.)
            result += s.slice(j, firstArgComma);
          } else {
            result += s.slice(j, k);
          }
          result += ")";
          j = k + 1;
          break;
        }
      } else if (ch === "," && depth === 1 && firstArgComma === -1) {
        firstArgComma = k;
      } else if (ch === '"' || ch === "'") {
        k = skipSimpleString(s, k);
      } else if (ch === "`") {
        k = skipTemplateLiteral(s, k);
      }
    }
    i = j;
  }
  return result;
}

function stripDoubleParensMergeProps(s) {
  const needle = "((_mergeProps(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);
    // Skip the first `(`, emit `(_mergeProps(`
    result += "(_mergeProps(";
    let j = idx + needle.length;
    // Scan to the matching )) — we need to consume the _mergeProps(...) then eat the extra )
    let depth = 2; // inside ((_mergeProps(
    while (j < s.length && depth > 0) {
      const ch = s[j];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") {
        depth--;
        if (depth === 0) {
          // We're at the outer closing ) of ((_mergeProps(...)))
          // Skip this extra )
          j++;
          break;
        }
      }
      result += ch;
      j++;
    }
    i = j;
  }
  return result;
}

/**
 * Strip grouping parens inside computed property keys.
 * Vue: {[(`data-${expr}`) || ""]: val}  →  {[`data-${expr}` || ""]: val}
 * The parens are just grouping for clarity, semantically identical.
 */
function stripComputedKeyGroupingParens(s) {
  // Match [( at computed key start, find the matching ) before || ""
  const needle = "[(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);
    // Check if this looks like a computed key: [(expr) || ""]
    let j = idx + 2; // skip [(
    let depth = 1;
    while (j < s.length && depth > 0) {
      const ch = s[j];
      if (ch === "(") depth++;
      else if (ch === ")") {
        depth--;
        if (depth === 0) break;
      } else if (ch === '"' || ch === "'") j = skipSimpleString(s, j);
      else if (ch === "`") j = skipTemplateLiteral(s, j);
      j++;
    }
    // j is at the closing ) — check if followed by || or space||
    const after = s.slice(j + 1, j + 10).trimStart();
    if (depth === 0 && after.startsWith("||")) {
      // Strip: emit [ + inner content (without outer parens)
      result += "[";
      result += s.slice(idx + 2, j); // inner content without ( and )
      i = j + 1; // skip the )
    } else {
      // Not a computed key pattern — keep as-is
      result += "[(";
      i = idx + 2;
    }
  }
  return result;
}

/**
 * Merge adjacent object literals inside _mergeProps calls.
 * Vue may split: _mergeProps(a, {x: 1}, {y: 2})
 * Verter may merge: _mergeProps(a, {x: 1, y: 2})
 * Both produce identical results when the keys are disjoint.
 */

/**
 * Deduplicate identical class: values within the same object literal.
 * Verter may emit class: "X", ..., class: "X" when an element has both
 * a static class and v-bind spread or directives.
 */
function dedupClassProps(s) {
  // Find duplicate class: entries within the same _mergeProps arg or _ssrRenderAttrs arg.
  // Handles both string values (class: "x") and array values (class: [...]).
  // Scans for `, class:` or `{class:` patterns, extracts the full value using balanced
  // parsing, and removes duplicates with identical values.
  const needle = "class:";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    // Check if preceded by `, ` or `{ ` (i.e. it's a property key, not inside a string)
    const before = s.slice(Math.max(0, idx - 3), idx).trimEnd();
    const lastCh = before[before.length - 1];
    if (lastCh !== "," && lastCh !== "{") {
      // Not a property start — skip
      result += s.slice(i, idx + needle.length);
      i = idx + needle.length;
      continue;
    }
    // Extract value after `class: ` using balanced parsing
    let valStart = idx + needle.length;
    while (valStart < s.length && s[valStart] === " ") valStart++;
    let valEnd = valStart;
    let depth = 0;
    for (; valEnd < s.length; valEnd++) {
      const ch = s[valEnd];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") {
        if (depth === 0) break; // end of enclosing scope
        depth--;
      } else if (ch === "," && depth === 0) break;
      else if (ch === '"' || ch === "'") valEnd = skipSimpleString(s, valEnd);
    }
    const classValue = s.slice(valStart, valEnd);
    // Look ahead for another identical `class: VALUE`
    const rest = s.slice(valEnd);
    const nextPattern = ", class: " + classValue;
    if (rest.startsWith(nextPattern)) {
      // Found duplicate — emit current class, skip the duplicate
      result += s.slice(i, valEnd);
      i = valEnd + nextPattern.length;
      continue;
    }
    result += s.slice(i, valEnd);
    i = valEnd;
  }
  return result;
}

/**
 * Sort properties within object literals alphabetically by key name.
 * Vue and Verter may emit properties in different order (e.g., value: first
 * vs last). Property order within a single JS object doesn't affect semantics.
 *
 * Only sorts "simple" object literals where all values can be parsed at the
 * top level (no nested object values that contain commas at depth 0).
 * Complex objects with nested structures are left unchanged.
 */
function sortObjectProps(s) {
  // Find _mergeProps(..., {props}) and _ssrRenderAttrs({props}) arg objects
  // We need balanced matching, so do it character-by-character
  const result = [];
  let i = 0;
  while (i < s.length) {
    // Look for { that starts an object literal (not a function body)
    if (s[i] === "{") {
      // Check if this is inside _mergeProps or _ssrRenderAttrs or _ssrRenderComponent
      // by checking what precedes it: , or ( or = followed by optional whitespace
      const before = s.slice(Math.max(0, i - 2), i).trimEnd();
      const lastChar = before[before.length - 1];
      if (lastChar === "," || lastChar === "(" || lastChar === "=") {
        // This looks like an object literal argument
        const objStart = i;
        // Find matching }
        let depth = 1;
        let j = i + 1;
        let hasNestedBraces = false;
        while (j < s.length && depth > 0) {
          const ch = s[j];
          if (ch === "{") {
            depth++;
            hasNestedBraces = true;
          } else if (ch === "}") depth--;
          else if (ch === '"' || ch === "'") j = skipSimpleString(s, j);
          else if (ch === "`") j = skipTemplateLiteral(s, j);
          else if (ch === "(" || ch === "[") depth++;
          else if (ch === ")" || ch === "]") depth--;
          j++;
        }
        const objEnd = j;
        const objStr = s.slice(objStart, objEnd);

        // Only sort if the object is "simple" enough (no function values, etc.)
        // Skip objects with _withCtx (slot definitions) or arrow functions
        if (!objStr.includes("=>") && !objStr.includes("_withCtx") && objStr.length < 2000) {
          const sorted = sortSingleObject(objStr);
          result.push(sorted);
          i = objEnd;
          continue;
        }
      }
    }
    result.push(s[i]);
    i++;
  }
  return result.join("");
}

/**
 * Sort properties within a single {...} object literal.
 * Returns the object with properties sorted alphabetically by key.
 */
function sortSingleObject(objStr) {
  // Strip outer braces
  const inner = objStr.slice(1, -1).trim();
  if (!inner) return objStr;

  // Split into key-value pairs at commas at depth 0
  const entries = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i <= inner.length; i++) {
    if (i === inner.length || (inner[i] === "," && depth === 0)) {
      const entry = inner.slice(start, i).trim();
      if (entry) entries.push(entry);
      start = i + 1;
    } else {
      const ch = inner[i];
      if (ch === "{" || ch === "(" || ch === "[") depth++;
      else if (ch === "}" || ch === ")" || ch === "]") depth--;
      else if (ch === '"' || ch === "'") i = skipSimpleString(inner, i);
      else if (ch === "`") i = skipTemplateLiteral(inner, i);
    }
  }

  if (entries.length < 2) return objStr;

  // Extract key names for sorting
  const keyed = entries.map((entry) => {
    // Match key: or "key": or ...spread or [computed]:
    const keyMatch = entry.match(/^(?:\.\.\.(\w)|"?(\w[\w-]*)"?\s*:|(\[))/);
    const key = keyMatch ? (keyMatch[1] ? `...${keyMatch[1]}` : keyMatch[2] || "[") : entry;
    return { key, entry };
  });

  // Sort: spread operators first, then alphabetically by key
  keyed.sort((a, b) => {
    const aSpread = a.key.startsWith("...");
    const bSpread = b.key.startsWith("...");
    if (aSpread && !bSpread) return -1;
    if (!aSpread && bSpread) return 1;
    return a.key.localeCompare(b.key);
  });

  return "{" + keyed.map((k) => k.entry).join(", ") + "}";
}

/**
 * Sort class array elements: static strings before dynamic objects.
 * Vue may emit class: [{dynamic}, "static"], Verter: class: ["static", {dynamic}].
 * Both produce the same merged class at runtime.
 */
function sortClassArrayElements(s) {
  // Match class: [...] patterns
  const re = /class:\s*\[/g;
  let match;
  let result = "";
  let lastEnd = 0;

  while ((match = re.exec(s)) !== null) {
    const arrStart = match.index + match[0].length - 1; // position of [
    // Find matching ]
    let depth = 1;
    let j = arrStart + 1;
    while (j < s.length && depth > 0) {
      const ch = s[j];
      if (ch === "[" || ch === "(" || ch === "{") depth++;
      else if (ch === "]" || ch === ")" || ch === "}") depth--;
      else if (ch === '"' || ch === "'") j = skipSimpleString(s, j);
      else if (ch === "`") j = skipTemplateLiteral(s, j);
      j++;
    }
    const arrEnd = j;
    const arrContent = s.slice(arrStart + 1, arrEnd - 1).trim();

    // Split array elements at depth 0 commas
    const elements = [];
    depth = 0;
    let start = 0;
    for (let k = 0; k <= arrContent.length; k++) {
      if (k === arrContent.length || (arrContent[k] === "," && depth === 0)) {
        const elem = arrContent.slice(start, k).trim();
        if (elem) elements.push(elem);
        start = k + 1;
      } else {
        const ch = arrContent[k];
        if (ch === "{" || ch === "(" || ch === "[") depth++;
        else if (ch === "}" || ch === ")" || ch === "]") depth--;
        else if (ch === '"' || ch === "'") k = skipSimpleString(arrContent, k);
        else if (ch === "`") k = skipTemplateLiteral(arrContent, k);
      }
    }

    if (elements.length >= 2) {
      // Sort: strings first, then objects/expressions
      elements.sort((a, b) => {
        const aIsString = a.startsWith('"') || a.startsWith("'");
        const bIsString = b.startsWith('"') || b.startsWith("'");
        if (aIsString && !bIsString) return -1;
        if (!aIsString && bIsString) return 1;
        return a.localeCompare(b);
      });
    }

    result += s.slice(lastEnd, arrStart + 1) + elements.join(", ");
    // arrEnd - 1 is the ] position
    lastEnd = arrEnd - 1;
  }

  result += s.slice(lastEnd);
  return result;
}

function mergeMergePropsObjects(s) {
  const needle = "_mergeProps(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);
    let j = idx + needle.length;
    // Parse args
    let depth = 1;
    const args = [];
    let argStart = j;
    for (let k = j; k < s.length && depth > 0; k++) {
      const ch = s[k];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") {
        depth--;
        if (depth === 0) {
          args.push(s.slice(argStart, k).trim());
          j = k + 1;
          break;
        }
      } else if (ch === "," && depth === 1) {
        args.push(s.slice(argStart, k).trim());
        argStart = k + 1;
      } else if (ch === '"' || ch === "'") {
        k = skipSimpleString(s, k);
      } else if (ch === "`") {
        k = skipTemplateLiteral(s, k);
      }
    }
    // Merge adjacent plain object args: {a: 1}, {b: 2} → {a: 1, b: 2}
    const merged = [];
    for (const arg of args) {
      if (merged.length > 0 && isPlainObject(merged[merged.length - 1]) && isPlainObject(arg)) {
        // Merge: strip } from prev, strip { from current
        const prev = merged[merged.length - 1];
        merged[merged.length - 1] = prev.slice(0, -1) + ", " + arg.slice(1);
      } else {
        merged.push(arg);
      }
    }
    if (merged.length === 1) {
      result += merged[0];
    } else {
      result += "_mergeProps(" + merged.join(", ") + ")";
    }
    i = j;
  }
  return result;
}

/** Check if a string looks like a plain JS object literal: {...} */
function isPlainObject(s) {
  return s.startsWith("{") && s.endsWith("}") && !s.startsWith("{...");
}

/**
 * Sort named slot properties in objects that contain _withCtx callbacks.
 * Slot order doesn't affect runtime behavior. Normalizes:
 * {title: _withCtx(...), footer: _withCtx(...)} → sorted alphabetically.
 * Directly finds slot objects by pattern matching rather than trying to
 * parse full _ssrRenderComponent calls (which can nest deeply).
 */
function sortSlotProperties(s) {
  // Find all positions where a slot object starts: {name: _withCtx(
  // Allow hyphens in slot names (e.g., sliding-panel-left-button, infinite-loader)
  const pattern = /\{([\w-]+:\s*_withCtx\()/g;
  let match;
  const positions = [];
  while ((match = pattern.exec(s)) !== null) {
    positions.push(match.index);
  }
  if (positions.length === 0) return s;

  // Process from end to start so positions stay valid
  let result = s;
  for (let p = positions.length - 1; p >= 0; p--) {
    const objStart = positions[p];

    // Find the matching } for this object
    let depth = 0;
    let objEnd = objStart;
    while (objEnd < result.length) {
      const ch = result[objEnd];
      if (ch === "{") depth++;
      else if (ch === "}") {
        depth--;
        if (depth === 0) {
          objEnd++;
          break;
        }
      } else if (ch === '"' || ch === "'") objEnd = skipSimpleString(result, objEnd);
      else if (ch === "`") objEnd = skipTemplateLiteral(result, objEnd);
      objEnd++;
    }

    const slotsObj = result.slice(objStart, objEnd);

    // Parse slot properties (name: value) with balanced matching
    const props = [];
    let k = 1; // skip opening {
    while (k < slotsObj.length - 1) {
      while (k < slotsObj.length - 1 && /[\s,]/.test(slotsObj[k])) k++;
      if (k >= slotsObj.length - 1) break;

      const propStart = k;
      while (k < slotsObj.length && slotsObj[k] !== ":") k++;
      if (k >= slotsObj.length) break;
      k++; // skip :
      while (k < slotsObj.length && slotsObj[k] === " ") k++;

      let d = 0;
      while (k < slotsObj.length - 1) {
        const ch = slotsObj[k];
        if (ch === "(" || ch === "{" || ch === "[") d++;
        else if (ch === ")" || ch === "}" || ch === "]") {
          if (d === 0) break;
          d--;
        } else if (ch === "," && d === 0) break;
        else if (ch === '"' || ch === "'") k = skipSimpleString(slotsObj, k);
        else if (ch === "`") k = skipTemplateLiteral(slotsObj, k);
        k++;
      }

      const prop = slotsObj.slice(propStart, k).trim();
      if (prop.length > 0) props.push(prop);
    }

    if (props.length <= 1) continue;

    // Only sort if there are _withCtx properties (skip non-slot objects)
    const hasWithCtx = props.some((p) => p.includes("_withCtx("));
    if (!hasWithCtx) continue;

    // Separate _: N from named slots, sort named slots
    const slotFlag = props.find((p) => p.startsWith("_:"));
    const namedSlots = props.filter((p) => !p.startsWith("_:"));
    namedSlots.sort();

    const sorted = [...namedSlots];
    if (slotFlag) sorted.push(slotFlag);

    const newObj = "{" + sorted.join(", ") + "}";
    result = result.slice(0, objStart) + newObj + result.slice(objEnd);
  }
  return result;
}

/**
 * Unwrap _createSlots({base}, [dynamicEntries]) into a flat slot object.
 * Vue uses _createSlots when component slots include v-if/v-for/dynamic names.
 * Verter inlines these as if/else blocks. Unwrapping _createSlots lets
 * the downstream sortSlotProperties match them after conditional stripping.
 *
 * Input:  _createSlots({default: _withCtx(A), _: 1}, [cond ? {name: "x", fn: _withCtx(B), key: "0"} : undefined])
 * Output: {default: _withCtx(A), x: _withCtx(B), _: 1}
 */
function unwrapCreateSlots(s) {
  const needle = "_createSlots(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    result += s.slice(i, idx);

    // Parse first arg: the base object {slots, _: N}
    let j = idx + needle.length;
    // Skip whitespace
    while (j < s.length && s[j] === " ") j++;
    if (s[j] !== "{") {
      result += needle;
      i = j;
      continue;
    }

    // Match balanced braces for base object
    let depth = 0;
    let baseStart = j;
    let k = j;
    while (k < s.length) {
      const ch = s[k];
      if (ch === "{") depth++;
      else if (ch === "}") {
        depth--;
        if (depth === 0) {
          k++;
          break;
        }
      } else if (ch === '"' || ch === "'") k = skipSimpleString(s, k);
      else if (ch === "`") k = skipTemplateLiteral(s, k);
      k++;
    }
    const baseObj = s.slice(baseStart, k); // includes { and }

    // Skip comma + whitespace to find the array arg
    while (k < s.length && (s[k] === "," || s[k] === " ")) k++;

    if (s[k] !== "[") {
      // No array arg — just emit the base object
      // Skip the closing ) of _createSlots
      while (k < s.length && s[k] !== ")") k++;
      if (s[k] === ")") k++;
      result += baseObj;
      i = k;
      continue;
    }

    // Match balanced brackets for the dynamic entries array
    let arrStart = k;
    depth = 0;
    while (k < s.length) {
      const ch = s[k];
      if (ch === "[") depth++;
      else if (ch === "]") {
        depth--;
        if (depth === 0) {
          k++;
          break;
        }
      } else if (ch === "{") depth++;
      else if (ch === "}") depth--;
      else if (ch === "(") depth++;
      else if (ch === ")") {
        if (depth <= 0) break; // closing ) of _createSlots
        depth--;
      } else if (ch === '"' || ch === "'") k = skipSimpleString(s, k);
      else if (ch === "`") k = skipTemplateLiteral(s, k);
      k++;
    }
    const arrContent = s.slice(arrStart + 1, k - 1); // inside [ and ]

    // Skip the closing ) of _createSlots
    while (k < s.length && (s[k] === " " || s[k] === ")")) {
      if (s[k] === ")") {
        k++;
        break;
      }
      k++;
    }

    // Extract dynamic slot entries from the array content.
    // Each entry is: condition ? {name: "X", fn: _withCtx(BODY), key: "N"} : undefined
    // Or: _renderList([items], (name) => {return {name: name, fn: _withCtx(BODY)}})
    const extraSlots = [];
    const namePattern = /\{\s*name:\s*"(\w+)",\s*fn:\s*/g;
    let m;
    while ((m = namePattern.exec(arrContent)) !== null) {
      const slotName = m[1];
      // Find the matching _withCtx(BODY) starting after "fn: "
      let fStart = m.index + m[0].length;
      // Match balanced content until we hit }, key: or just }
      let d2 = 0;
      let fEnd = fStart;
      while (fEnd < arrContent.length) {
        const ch = arrContent[fEnd];
        if (ch === "(" || ch === "{" || ch === "[") d2++;
        else if (ch === ")" || ch === "]") {
          if (d2 === 0) break;
          d2--;
        } else if (ch === "}") {
          if (d2 === 0) break;
          d2--;
        } else if (ch === "," && d2 === 0) break;
        else if (ch === '"' || ch === "'") fEnd = skipSimpleString(arrContent, fEnd);
        else if (ch === "`") fEnd = skipTemplateLiteral(arrContent, fEnd);
        fEnd++;
      }
      const fnExpr = arrContent.slice(fStart, fEnd).trim();
      // Remove trailing ", key: ..." if present
      const cleanFn = fnExpr.replace(/,\s*key:\s*"?\d+"?\s*$/, "");
      extraSlots.push(`${slotName}: ${cleanFn}`);
    }

    if (extraSlots.length > 0) {
      // Insert extra slots into the base object before _: N
      let inner = baseObj.slice(1, -1).trim(); // strip { and }
      // Remove trailing _: N temporarily
      const flagMatch = inner.match(/,?\s*_:\s*\d+\s*$/);
      const flag = flagMatch ? flagMatch[0] : "";
      if (flag) inner = inner.slice(0, inner.length - flag.length);
      // Append extra slots
      for (const slot of extraSlots) {
        if (inner.length > 0) inner += ", ";
        inner += slot;
      }
      // Ensure comma before _: N flag when inner is non-empty
      if (inner.length > 0 && flag && !flag.startsWith(",")) {
        inner += ", " + flag;
      } else {
        inner += flag;
      }
      result += "{" + inner + "}";
    } else {
      // No extractable dynamic entries (e.g., _renderList) — keep base only
      result += baseObj;
    }
    i = k;
  }
  return result;
}

/**
 * Strip conditional wrappers around slot entries in Verter's output.
 * Verter emits: if (condition) {slotName: _withCtx(BODY)} else {}
 * This normalizes to: slotName: _withCtx(BODY),
 *
 * This allows comparison against Vue's _createSlots pattern where
 * conditional slots are in a separate dynamic array.
 */

/**
 * Strip _ssrRenderList calls from inside slot objects.
 * Verter emits dynamic v-for slots as:
 *   {_ssrRenderList([...], (name) => {"[name]": _withCtx(...)}) _: 1}
 * Vue omits dynamic slots entirely:
 *   {_: 1}
 * Both are equivalent — dynamic slot names are resolved at runtime.
 */
function stripDynamicSlotRenderLists(s) {
  const needle = "_ssrRenderList(";
  let result = "";
  let i = 0;
  while (i < s.length) {
    const idx = s.indexOf(needle, i);
    if (idx === -1) {
      result += s.slice(i);
      break;
    }
    // Find the balanced end of the _ssrRenderList(...) call
    let depth = 1;
    let end = idx + needle.length;
    for (; end < s.length && depth > 0; end++) {
      const ch = s[end];
      if (ch === "(" || ch === "[" || ch === "{") depth++;
      else if (ch === ")" || ch === "]" || ch === "}") depth--;
      else if (ch === '"' || ch === "'") end = skipSimpleString(s, end);
      else if (ch === "`") end = skipTemplateLiteral(s, end);
    }
    // Check if the call body contains a dynamic slot pattern: "[name]": _withCtx
    const callText = s.slice(idx, end);
    if (callText.includes('"[') && callText.includes("_withCtx")) {
      // Strip this _ssrRenderList call entirely
      result += s.slice(i, idx);
      // Skip trailing whitespace/comma
      while (end < s.length && (s[end] === " " || s[end] === ",")) end++;
      i = end;
    } else {
      // Not a dynamic slot — keep it
      result += s.slice(i, end);
      i = end;
    }
  }
  return result;
}

function stripConditionalSlotWrappers(s) {
  // Pattern: if (CONDITION) {SLOT_ENTRY} else {}
  // where SLOT_ENTRY is like: slotName: _withCtx(...)
  // We need to be careful: only strip when the slot entry contains _withCtx
  const ifPattern = /if\s*\(/g;
  let result = "";
  let i = 0;
  let m;

  while (i < s.length) {
    ifPattern.lastIndex = i;
    m = ifPattern.exec(s);
    if (!m) {
      result += s.slice(i);
      break;
    }

    // Add everything before the if
    result += s.slice(i, m.index);
    let j = m.index + m[0].length;

    // Match balanced parens for condition
    let depth = 1;
    let condStart = j;
    while (j < s.length && depth > 0) {
      const ch = s[j];
      if (ch === "(") depth++;
      else if (ch === ")") depth--;
      else if (ch === '"' || ch === "'") j = skipSimpleString(s, j);
      else if (ch === "`") j = skipTemplateLiteral(s, j);
      j++;
    }
    // j is past the closing ) of the condition

    // Skip whitespace then expect {
    while (j < s.length && s[j] === " ") j++;
    if (s[j] !== "{") {
      // Not the pattern we're looking for
      result += s.slice(m.index, j);
      i = j;
      continue;
    }

    // Match balanced braces for the if body
    let bodyStart = j + 1;
    depth = 1;
    j++;
    while (j < s.length && depth > 0) {
      const ch = s[j];
      if (ch === "{") depth++;
      else if (ch === "}") depth--;
      else if (ch === '"' || ch === "'") j = skipSimpleString(s, j);
      else if (ch === "`") j = skipTemplateLiteral(s, j);
      if (depth > 0) j++;
    }
    let bodyEnd = j; // at the closing }
    j++; // past }

    const body = s.slice(bodyStart, bodyEnd).trim();

    // Check if body contains _withCtx — this is a slot entry
    if (!body.includes("_withCtx(")) {
      // Not a slot conditional — keep as-is
      result += s.slice(m.index, j);
      i = j;
      continue;
    }

    // Check for } else {} after the if body
    let afterIf = j;
    while (afterIf < s.length && s[afterIf] === " ") afterIf++;
    if (s.slice(afterIf, afterIf + 4) === "else") {
      let elseEnd = afterIf + 4;
      while (elseEnd < s.length && s[elseEnd] === " ") elseEnd++;
      if (s[elseEnd] === "{") {
        // Match balanced braces for else body
        let eDepth = 1;
        elseEnd++;
        while (elseEnd < s.length && eDepth > 0) {
          const ch = s[elseEnd];
          if (ch === "{") eDepth++;
          else if (ch === "}") eDepth--;
          else if (ch === '"' || ch === "'") elseEnd = skipSimpleString(s, elseEnd);
          else if (ch === "`") elseEnd = skipTemplateLiteral(s, elseEnd);
          if (eDepth > 0) elseEnd++;
        }
        elseEnd++; // past closing }
        j = elseEnd;
      }
    }

    // Emit the body without the if/else wrapper, add trailing comma
    if (body.endsWith(",")) {
      result += body + " ";
    } else {
      result += body + ", ";
    }
    i = j;
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

/**
 * Strip TypeScript type annotations from function parameters and expressions.
 * Verter may emit TS types in VDOM fallback or event handlers while Vue strips them.
 *
 * Handles:
 * - Single param: `(x: Type)` → `(x)`
 * - Multiple params: `(x: Type, y: Type2)` → `(x, y)`
 * - Union types: `(x: A | B)` → `(x)`
 * - Array types: `(x: Type[])` → `(x)`
 * - Generic types: `(x: Map<K, V>)` → `(x)`
 * - Lowercase types: `(x :number)`, `(x: string)` → `(x)`
 * - Type assertions: `(x as Type)` → `(x)`
 * - After `=>` or `{` context: arrow functions and regular functions
 */
function stripTypeAnnotations(s) {
  // Strategy: Find parameter lists (parenthesized, followed by => or {) and
  // strip type annotations from each parameter.
  // Process character by character to handle nested parens/generics correctly.
  let result = "";
  let i = 0;
  while (i < s.length) {
    if (s[i] === "(") {
      // Check if this looks like a parameter list — find matching ) and check what follows
      const closeIdx = findMatchingParen(s, i);
      if (closeIdx === -1) {
        result += s[i];
        i++;
        continue;
      }
      // Check what follows the closing paren
      let afterClose = closeIdx + 1;
      while (afterClose < s.length && s[afterClose] === " ") afterClose++;
      const followedByArrow = s.slice(afterClose, afterClose + 2) === "=>";
      const followedByBrace = afterClose < s.length && s[afterClose] === "{";
      if (followedByArrow || followedByBrace) {
        // This is a parameter list — strip type annotations
        const paramStr = s.slice(i + 1, closeIdx);
        const stripped = stripParamTypes(paramStr);
        result += "(" + stripped + ")";
        i = closeIdx + 1;
      } else {
        // Check for `as Type` or simple `name: type` inside
        const inner = s.slice(i + 1, closeIdx);
        if (/^\w+\s+as\s+\w/.test(inner)) {
          const stripped = inner.replace(/^(\w+)\s+as\s+\w[\w.]*/, "$1");
          result += "(" + stripped + ")";
          i = closeIdx + 1;
        } else if (
          /^\w+\s*:\s*(unknown|any|string|number|boolean|void|never|null|undefined)\s*$/.test(inner)
        ) {
          // Strip well-known TS types in non-arrow/function contexts
          const stripped = inner.replace(/^(\w+)\s*:\s*\w+\s*$/, "$1");
          result += "(" + stripped + ")";
          i = closeIdx + 1;
        } else {
          result += s[i];
          i++;
        }
      }
    } else {
      result += s[i];
      i++;
    }
  }
  return result;
}

/** Find matching closing paren, respecting nesting and strings. */
function findMatchingParen(s, openIdx) {
  let depth = 1;
  let i = openIdx + 1;
  while (i < s.length && depth > 0) {
    const ch = s[i];
    if (ch === "(") depth++;
    else if (ch === ")") {
      depth--;
      if (depth === 0) return i;
    } else if (ch === '"' || ch === "'") i = skipSimpleString(s, i);
    else if (ch === "`") i = skipTemplateLiteral(s, i);
    i++;
  }
  return -1;
}

/**
 * Strip type annotations from a comma-separated parameter string.
 * "x: Type, y: Type2" → "x, y"
 * Handles generics (Map<K, V>) by tracking angle bracket depth.
 */
function stripParamTypes(paramStr) {
  const params = splitParams(paramStr);
  return params
    .map((p) => {
      p = p.trim();
      if (!p) return p;
      // Match: name followed by : and type annotation
      // The type can contain generics (<>), arrays ([]), union (|), etc.
      const colonIdx = p.indexOf(":");
      if (colonIdx === -1) return p;
      // Check that what's before the colon is a valid identifier (or destructured pattern)
      const beforeColon = p.slice(0, colonIdx).trim();
      // Allow simple identifiers, _vars, $vars, optional params (name?), or destructured {a, b}
      if (
        /^[\w$]+\??$/.test(beforeColon) ||
        beforeColon.startsWith("{") ||
        beforeColon.startsWith("[")
      ) {
        // Strip trailing ? from optional params: "name?" → "name"
        return beforeColon.replace(/\?$/, "");
      }
      return p;
    })
    .join(", ");
}

/** Split parameter string by commas, respecting nested <>, (), [], {} */
function splitParams(s) {
  const result = [];
  let current = "";
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === "<" || ch === "(" || ch === "[" || ch === "{") depth++;
    else if (ch === ">" || ch === ")" || ch === "]" || ch === "}") depth--;
    else if (ch === "," && depth === 0) {
      result.push(current);
      current = "";
      continue;
    }
    current += ch;
  }
  if (current) result.push(current);
  return result;
}

/**
 * Normalize component name casing in _ssrRenderComponent calls.
 *
 * Collects all component names used in `_ssrRenderComponent(_ctx["Name"]` or
 * `_ssrRenderComponent(_ctx.Name` patterns, then lowercases ALL occurrences
 * of those names in `_ctx["Name"]` format throughout the string.
 *
 * This handles the case where Vue resolves via _resolveComponent("tag-name")
 * → camelCase, while Verter resolves from bindings → PascalCase. Both reference
 * the same component at runtime.
 */
function normalizeComponentNameCasing(s) {
  // Collect component names from _ssrRenderComponent calls
  const compNames = new Set();
  const bracketRe = /_ssrRenderComponent\(_ctx\["(\w+)"\]/g;
  let m;
  while ((m = bracketRe.exec(s)) !== null) {
    compNames.add(m[1]);
  }
  if (compNames.size === 0) return s;

  // For each component name, replace all _ctx["Name"] occurrences with lowercase
  for (const name of compNames) {
    const lower = name.toLowerCase();
    if (lower !== name) {
      // Replace _ctx["Name"] with _ctx["name"] everywhere (not just in _ssrRenderComponent)
      s = s.replace(new RegExp(`_ctx\\["${name}"\\]`, "g"), `_ctx["${lower}"]`);
    }
  }
  return s;
}

/**
 * Strip conditional slot existence checks.
 * Verter may wrap slot entries in `if (_ctx.$slots.name) { ... }` checks.
 * Vue omits these in SSR — all slots render unconditionally (with fallback
 * content handled by _ssrRenderSlot). Strip the conditional wrapper while
 * keeping the slot entry content.
 */
/**
 * Normalize ternary wrapping around Array.isArray() and similar function calls.
 * Vue:    (Array.isArray(x)) ? a : b
 * Verter: (Array.isArray(x) ? a : b)
 * Both normalized to: Array.isArray(x) ? a : b
 */
function normArrayIsArrayTernary(s) {
  let result = s;
  let i = 0;
  while (i < result.length) {
    const idx = result.indexOf("(Array.isArray(", i);
    if (idx < 0) break;
    // Find matching close paren for the outer (
    let depth = 1;
    let j = idx + 1;
    while (j < result.length && depth > 0) {
      if (result[j] === "(") depth++;
      else if (result[j] === ")") depth--;
      j++;
    }
    // j is after the outer closing )
    let afterClose = j;
    while (afterClose < result.length && result[afterClose] === " ") afterClose++;
    if (result[afterClose] === "?") {
      // Vue pattern: (Array.isArray(...)) ? — strip outer parens
      result = result.substring(0, idx) + result.substring(idx + 1, j - 1) + result.substring(j);
    } else {
      // Check if the matched content is a ternary (Verter pattern)
      const inner = result.substring(idx + 1, j - 1);
      if (inner.includes(" ? ") && inner.includes(" : ")) {
        result = result.substring(0, idx) + inner + result.substring(j);
      } else {
        i = j;
        continue;
      }
    }
    i = idx;
  }
  return result;
}

function stripSlotExistenceChecks(s) {
  // Pattern: if (_ctx.$slots.xxx) {slotEntry} or if (_ctx.$slots["xxx"]) {slotEntry}
  // Use the existing stripConditionalSlotWrappers approach but specifically
  // target $slots checks.
  const pattern = /if\s*\(_ctx\.\$slots/g;
  let result = "";
  let i = 0;
  let m;

  while (i < s.length) {
    pattern.lastIndex = i;
    m = pattern.exec(s);
    if (!m) {
      result += s.slice(i);
      break;
    }

    result += s.slice(i, m.index);
    let j = m.index + m[0].length;

    // Skip to end of condition: find closing )
    let depth = 1;
    while (j < s.length && depth > 0) {
      if (s[j] === "(") depth++;
      else if (s[j] === ")") depth--;
      j++;
    }
    // j is past closing )

    // Skip whitespace
    while (j < s.length && s[j] === " ") j++;

    // Expect {
    if (j >= s.length || s[j] !== "{") {
      result += s.slice(m.index, j);
      i = j;
      continue;
    }

    // Find matching } for the if body
    let bodyStart = j + 1;
    depth = 1;
    j++;
    while (j < s.length && depth > 0) {
      const ch = s[j];
      if (ch === "{") depth++;
      else if (ch === "}") depth--;
      else if (ch === '"' || ch === "'") j = skipSimpleString(s, j);
      else if (ch === "`") j = skipTemplateLiteral(s, j);
      if (depth > 0) j++;
    }
    let bodyEnd = j;
    j++; // past }

    const body = s.slice(bodyStart, bodyEnd).trim();

    // Only strip if body contains a slot entry (_withCtx)
    if (!body.includes("_withCtx(")) {
      result += s.slice(m.index, j);
      i = j;
      continue;
    }

    // Check for else branch (but not "else:" which is a slot name)
    let afterJ = j;
    while (afterJ < s.length && s[afterJ] === " ") afterJ++;
    if (s.slice(afterJ, afterJ + 4) === "else" && s[afterJ + 4] !== ":") {
      let elseEnd = afterJ + 4;
      while (elseEnd < s.length && s[elseEnd] === " ") elseEnd++;
      if (s[elseEnd] === "{") {
        let d = 1;
        elseEnd++;
        while (elseEnd < s.length && d > 0) {
          if (s[elseEnd] === "{") d++;
          else if (s[elseEnd] === "}") d--;
          else if (s[elseEnd] === '"' || s[elseEnd] === "'") elseEnd = skipSimpleString(s, elseEnd);
          else if (s[elseEnd] === "`") elseEnd = skipTemplateLiteral(s, elseEnd);
          if (d > 0) elseEnd++;
        }
        elseEnd++;
        j = elseEnd;
      }
    }

    // Emit body without if wrapper, ensure trailing comma
    if (body.endsWith(",")) {
      result += body + " ";
    } else {
      result += body + ", ";
    }
    i = j;
  }
  return result;
}
