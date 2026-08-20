// Authored-source mapping oracle: one candidate's source map against that
// candidate's own generated code and the authored SFC fixture on disk.
// No golden map or official-compiler map is an input.
//
// Comparing candidate `mappings` to the official compiler's cannot work.
// A `mappings` field encodes (generated → original) over one specific
// generated document. Verter's JS is legitimately not byte-identical to
// official (cosmetic carrier differences are permitted), so the two maps
// address different documents. Comparing them rejects a correct map whose
// layout differs and accepts a wrong map whose segment shape happens to
// resemble official's. Normalization cannot fix this: the coordinate
// spaces are not the same.
//
// Inputs: authored fixture (read from disk — never the map's
// `sourcesContent`), candidate generated code, candidate map. A `profile`
// supplies per-framework rewrite vocabulary only.
//
// Every requirement — including generated-only ranges (requirement 6) —
// runs on every artifact, Vue and Svelte alike. Ranges come from the
// artifact's own parse, not the caller (see generatedOnlyRanges).
//
// A generated-only range covers a claimable name (profile emitted-identifier
// shape, not a context root, not a word in the fixture's script blocks) in
// an enumerated binding position or statement form. Uncovered remainder:
// compiler scaffolding with authored-shaped identifiers, and non-identifier
// payload of synthesized statements. Over that remainder the rail is the
// relation table (`framework-emitted-token` / `delimiter-anchor`), which
// are not position-exact (see RELATIONS).

import { readFileSync } from "node:fs";
import path from "node:path";

import { decodeMappings } from "./sourcemap.mjs";
import { parseModule } from "./normalize.mjs";

const WORD_CHARACTER = /[A-Za-z0-9_$]/;

/**
 * Splits on `"\n"` and keeps any trailing `"\r"`. Source-map columns are
 * UTF-16 code units; stripping CR would under-count every CRLF line by one
 * and reject valid end-of-line segments.
 */
export function lineTable(text) {
  return text.split("\n");
}

/**
 * Token at `(line, column)` in a line table. Columns are UTF-16 code units
 * (`String.length` / index access), never bytes.
 *
 * @returns {{ kind: "word-start"|"word-interior"|"punct"|"eol"|"out-of-bounds",
 *   text: string, rest: string }} `text` is the whole word (or the punct
 *   character); `rest` is the tail from `column`, used by exact carry.
 */
export function tokenAt(lines, line, column) {
  if (line < 0 || line >= lines.length) return { kind: "out-of-bounds", text: "", rest: "" };
  const text = lines[line];
  if (column < 0 || column > text.length) return { kind: "out-of-bounds", text: "", rest: "" };
  if (column === text.length) return { kind: "eol", text: "", rest: "" };
  if (!WORD_CHARACTER.test(text[column]))
    return { kind: "punct", text: text[column], rest: text[column] };
  let start = column;
  while (start > 0 && WORD_CHARACTER.test(text[start - 1])) start -= 1;
  let end = column;
  while (end < text.length && WORD_CHARACTER.test(text[end])) end += 1;
  return {
    kind: start === column ? "word-start" : "word-interior",
    text: text.slice(start, end),
    rest: text.slice(column, end),
  };
}

/**
 * Authored fixture script-block regions as `[start, end)` character offsets.
 *
 * A `.vue`/`.svelte` fixture is not a JS module; delimiter scan over the
 * committed test fixture (never a production carrier) only bounds the parse.
 * Structural facts come from the parser. An unparseable region contributes
 * nothing (fail-closed) — a mis-located block can only remove evidence.
 */
function authoredScriptRegions(source) {
  const regions = [];
  const opening = /<script\b[^>]*>/gi;
  let match;
  while ((match = opening.exec(source)) !== null) {
    const start = match.index + match[0].length;
    const closing = source.indexOf("</script", start);
    if (closing === -1) continue;
    regions.push({ start, end: closing });
    opening.lastIndex = closing;
  }
  return regions;
}

/**
 * Projection of `source` with characters outside `[start, end)` replaced by
 * spaces (newlines kept). Acorn locations are then the fixture's own
 * (line, column) — no offset arithmetic between parser and map.
 */
function blankedOutside(source, start, end) {
  const blank = (text) => text.replace(/[^\n]/g, " ");
  return blank(source.slice(0, start)) + source.slice(start, end) + blank(source.slice(end));
}

const BINDER_FUNCTION = /^(FunctionDeclaration|FunctionExpression|ArrowFunctionExpression)$/;

/**
 * Destructuring patterns in a genuine binding position: `VariableDeclarator.id`,
 * function parameter, or catch-clause parameter. Object-expression braces,
 * nested sub-patterns, and assignment-target patterns (`({ a } = obj)`) are
 * absent so a lookup cannot land on a brace that declares nothing.
 */
function rootBindingPatterns(ast) {
  const roots = new Set();
  const mark = (node) => {
    if (node === null || node === undefined) return;
    if (node.type === "AssignmentPattern") return mark(node.left);
    if (node.type === "ObjectPattern" || node.type === "ArrayPattern") roots.add(node);
  };
  walkAst(ast, (node) => {
    if (node.type === "VariableDeclarator") mark(node.id);
    else if (BINDER_FUNCTION.test(node.type)) for (const parameter of node.params) mark(parameter);
    else if (node.type === "CatchClause") mark(node.param);
  });
  return roots;
}

/**
 * Names a pattern binds at its own level. A property key is not a binding
 * (`{ disabled: other }` binds `other`); a default-value expression is not
 * (`{ other = disabled }` binds `other`); nested pattern names are not
 * (`{ data: { x } }` binds nothing here). Admitting inner names would
 * re-open "somewhere inside" looseness.
 */
function ownLevelBindings(pattern) {
  const names = new Set();
  const target = (node) => {
    if (node === null || node === undefined) return;
    if (node.type === "AssignmentPattern") return target(node.left);
    if (node.type === "RestElement") return target(node.argument);
    if (node.type === "Identifier") names.add(node.name);
  };
  if (pattern.type === "ObjectPattern") {
    for (const property of pattern.properties)
      target(property.type === "Property" ? property.value : property);
  } else {
    for (const element of pattern.elements) target(element);
  }
  return names;
}

/**
 * Authored script index, memoized per line table.
 *
 *  - `bindingPatterns`: `"<line>:<column>"` of a binding pattern's opening
 *    delimiter → names it binds at its own level. Lookup is the parser's
 *    start for a declaration-position pattern, so object literals, `{{ x }}`,
 *    Svelte `{x}`, and `{#each …}` are absent by construction.
 *  - `scriptNames`: identifier-shaped words in authored script blocks.
 *    A generated name the author also wrote is not claimable; subtracted
 *    from generated-only ranges (see generatedOnlyRanges).
 */
const AUTHORED_SCRIPT_INDEX = new WeakMap();

function authoredScriptIndex(srcLines) {
  const cached = AUTHORED_SCRIPT_INDEX.get(srcLines);
  if (cached !== undefined) return cached;
  const source = srcLines.join("\n");
  const bindingPatterns = new Map();
  const scriptNames = new Set();
  for (const region of authoredScriptRegions(source)) {
    for (const word of source.slice(region.start, region.end).matchAll(/[A-Za-z_$][\w$]*/g))
      scriptNames.add(word[0]);
    let ast;
    try {
      ast = parseModule(blankedOutside(source, region.start, region.end), "authored-script-region");
    } catch {
      continue; // fail-closed: an unparseable region contributes no evidence
    }
    for (const pattern of rootBindingPatterns(ast)) {
      const key = `${pattern.loc.start.line - 1}:${pattern.loc.start.column}`;
      const names = bindingPatterns.get(key) ?? new Set();
      for (const name of ownLevelBindings(pattern)) names.add(name);
      bindingPatterns.set(key, names);
    }
  }
  const index = { bindingPatterns, scriptNames };
  AUTHORED_SCRIPT_INDEX.set(srcLines, index);
  return index;
}

/**
 * Whether the authored position is the opening delimiter of a genuine
 * destructuring pattern that binds `name` at its own level.
 */
function patternAtBinds(srcLines, line, column, name) {
  const bound = authoredScriptIndex(srcLines).bindingPatterns.get(`${line}:${column}`);
  return bound !== undefined && bound.has(name);
}

/** The member property immediately following a `<root>.` at `column`. */
function memberPropertyAfter(lines, line, column, rootLength) {
  const text = lines[line] ?? "";
  const dot = column + rootLength;
  if (text[dot] !== ".") return null;
  const property = tokenAt(lines, line, dot + 1);
  return property.kind === "word-start" ? property.text : null;
}

/** Strips a trailing `_<digits>` disambiguator from a generated local name. */
function withoutDisambiguator(name) {
  const match = /^(.*?)_\d+$/.exec(name);
  return match === null ? name : match[1];
}

/**
 * Per-framework rewrite vocabulary — the only framework-specific oracle
 * input. Each field names a rewrite class, never a fixture:
 *
 *  - `contextRoots`: render-scope roots (`_ctx.count`, `$setup.count`, …).
 *    A generated token in this set must satisfy `context-binding-prefix`
 *    (accessed property is the authored identifier) or the narrower
 *    `component-instance-surface` — never the generic emitted-token relation.
 *  - `macroBindings`: generated binding → authored macro (`__props` ← `defineProps`).
 *  - `emittedIdentifier`: shapes of compiler-emitted identifiers (helpers,
 *    hoisted nodes, synthesized locals). Outside these shapes there is no
 *    fallback: verbatim authored text or a named tie.
 *  - `runtimeModules`: closed exact-string set of framework-runtime
 *    specifiers. Provenance, not spelling: `import { thing as _thing } from
 *    "./my-utils.js"` is outside the set. Exact membership, not a prefix —
 *    a prefix would also claim author-facing entry points (`vue/reactivity`,
 *    `@vue/shared`, `svelte/store`) that fixtures may import and whose
 *    truthful segments the pinned compilers carry. Side-effect form matters
 *    most: `import "svelte/store";` binds no local, so authored-name
 *    subtraction cannot rescue it. The six members are measured from all 48
 *    goldens (`vue` ×48, `vue/server-renderer` ×12, `svelte/internal/client`
 *    ×6, `svelte/internal/server` ×6, `svelte/internal/disclose-version` ×6,
 *    `svelte/internal/flags/legacy` ×2). A compiler upgrade that emits a new
 *    specifier fails loudly rather than silently widening the claim.
 */
const VUE_VOCABULARY = {
  framework: "vue",
  contextRoots: ["_ctx", "$setup", "$props", "$data", "$options"],
  macroBindings: {
    __props: "defineProps",
    __emit: "defineEmits",
    __expose: "defineExpose",
  },
  emittedIdentifier: [/^_[A-Za-z_$][\w$]*$/, /^__[\w$]+$/],
  runtimeModules: ["vue", "vue/server-renderer"],
};

const SVELTE_VOCABULARY = {
  framework: "svelte",
  contextRoots: ["$$props", "$$restProps"],
  macroBindings: {},
  emittedIdentifier: [/^\$\$?[\w$]*$/, /^root(_\d+)?$/],
  runtimeModules: [
    "svelte/internal/client",
    "svelte/internal/server",
    "svelte/internal/flags/legacy",
    "svelte/internal/disclose-version",
  ],
};

/**
 * One profile per (framework, emission target). Vocabulary is shared;
 * the target only selects required anchors (see FIXTURE_ANCHORS).
 */
export const MAPPING_PROFILES = {
  "vue:vdom": { ...VUE_VOCABULARY, key: "vue:vdom" },
  "vue:vapor": { ...VUE_VOCABULARY, key: "vue:vapor" },
  "vue:ssr": { ...VUE_VOCABULARY, key: "vue:ssr" },
  "svelte:client": { ...SVELTE_VOCABULARY, key: "svelte:client" },
  "svelte:server": { ...SVELTE_VOCABULARY, key: "svelte:server" },
};

const VUE_PROFILE_KEYS = ["vue:vdom", "vue:vapor", "vue:ssr"];
const SVELTE_PROFILE_KEYS = ["svelte:client", "svelte:server"];

/**
 * Ordered relation table. First matching named rule classifies the segment;
 * none matching is a `segment-provenance` violation — never a skip.
 *
 * Position-exact (mapped original must be a specific authored lexeme, and
 * for `verbatim-carry` a specific offset inside it): `verbatim-carry`,
 * `context-binding-prefix`, `macro-result-binding`, `event-handler-key`,
 * `synthesized-local-for-authored-name`, `destructured-binding-pattern`.
 * Re-pointing elsewhere on the same line breaks them.
 *
 * `verbatim-carry` is text-equality (same lexeme, same offset), so two
 * occurrences of the same token at the same offset are interchangeable.
 * Requiring `word-start` on both sides would reject interior→interior
 * segments the pinned official compilers emit. The other five pin a
 * position no other occurrence of the same text satisfies.
 *
 * Not position-exact: `component-instance-surface`, `framework-emitted-token`,
 * `delimiter-anchor` constrain the generated side only and accept any
 * in-bounds non-word-interior authored position. Authored-looking generated
 * identifiers fall outside all three and must satisfy a position-exact
 * relation. Generated-only ranges cover the scaffolding half of the loose
 * class; remainder is recorded at generatedOnlyRanges.
 */
const RELATIONS = [
  {
    name: "verbatim-carry",
    // Position-exact: same lexeme (`gen.text === src.text`) and same offset
    // inside it (`gen.rest === src.rest`). Comparing only `rest` accepted
    // different tokens sharing a trailing substring (`import`@col5 vs
    // `script`@col6 both yield "t"). Word-interior stays admissible — pinned
    // compilers emit interior→interior segments; requiring `word-start` on
    // both sides would reject genuine official output.
    match: ({ gen, src }) =>
      gen.kind !== "eol" &&
      gen.kind === src.kind &&
      gen.text === src.text &&
      gen.rest === src.rest &&
      gen.rest.length > 0,
  },
  {
    name: "context-binding-prefix",
    match: ({ gen, src, genLines, segment, profile }) => {
      if (gen.kind !== "word-start" || !profile.contextRoots.includes(gen.text)) return false;
      if (src.kind !== "word-start") return false;
      const property = memberPropertyAfter(
        genLines,
        segment.genLine,
        segment.genCol,
        gen.text.length,
      );
      return property !== null && property === src.text;
    },
  },
  {
    name: "component-instance-surface",
    match: ({ gen, src, genLines, segment, profile }) => {
      if (gen.kind !== "word-start" || !profile.contextRoots.includes(gen.text)) return false;
      if (src.kind === "word-interior" || src.kind === "out-of-bounds") return false;
      const property = memberPropertyAfter(
        genLines,
        segment.genLine,
        segment.genCol,
        gen.text.length,
      );
      return property !== null && property.startsWith("$");
    },
  },
  {
    name: "macro-result-binding",
    match: ({ gen, src, profile }) =>
      gen.kind === "word-start" &&
      src.kind === "word-start" &&
      profile.macroBindings[gen.text] === src.text,
  },
  {
    name: "event-handler-key",
    match: ({ gen, src }) =>
      gen.kind === "word-start" &&
      src.kind === "word-start" &&
      /^on[A-Z]/.test(gen.text) &&
      gen.text.slice(2).toLowerCase() === src.text.toLowerCase(),
  },
  {
    name: "synthesized-local-for-authored-name",
    // Position-exact: mapped position must be the start of the authored
    // occurrence the generated local was named after, not merely share its
    // line. Line-scoped `containsWholeWord` accepted a generated `items`
    // re-pointed to column 0 (`const`) because the line still contained the
    // word.
    match: ({ gen, src }) =>
      gen.kind === "word-start" &&
      src.kind === "word-start" &&
      src.text === withoutDisambiguator(gen.text),
  },
  {
    name: "destructured-binding-pattern",
    // Pinned Svelte anchors a local hoisted out of a destructuring pattern
    // at the pattern's opening delimiter, not the bound name:
    // `let { label, disabled = false } = $props()` → `let disabled = $.prop(…)`
    // whose `disabled` maps to the authored `{`. Named because it is a real
    // official correspondence, not a line-scoped text match.
    //
    // Position-exact: mapped position must be the parser's start for a
    // declaration-position pattern (`VariableDeclarator.id`, function
    // parameter, catch-clause parameter) that binds the name at its own
    // level. Object-literal braces, `{{ x }}`, Svelte `{x}` / `{#each …}`,
    // property keys, default-value expressions, nested sub-patterns, and
    // other same-line positions are not keys in the binding-pattern index.
    match: ({ gen, src, srcLines, segment }) =>
      gen.kind === "word-start" &&
      src.kind === "punct" &&
      (src.text === "{" || src.text === "[") &&
      patternAtBinds(srcLines, segment.srcLine, segment.srcCol, withoutDisambiguator(gen.text)),
  },
  {
    name: "framework-emitted-token",
    match: ({ gen, src, profile }) =>
      (gen.kind === "word-start" || gen.kind === "word-interior") &&
      !profile.contextRoots.includes(gen.text) &&
      profile.emittedIdentifier.some((shape) => shape.test(gen.text)) &&
      src.kind !== "out-of-bounds" &&
      src.kind !== "word-interior",
  },
  {
    name: "delimiter-anchor",
    match: ({ gen, src }) =>
      (gen.kind === "punct" || gen.kind === "eol") &&
      src.kind !== "out-of-bounds" &&
      src.kind !== "word-interior",
  },
];

export const RELATION_NAMES = RELATIONS.map((relation) => relation.name);

/** Classifies one source-bearing segment. @returns relation name or null */
export function classifySegment(context) {
  for (const relation of RELATIONS) {
    if (relation.match(context)) return relation.name;
  }
  return null;
}

/**
 * Required anchors per authored fixture — taken from fixture text (each
 * names the lexeme at that position; the oracle re-reads the fixture from
 * disk to confirm), never from any compiler's output. Each names the
 * relation(s) a correct map may use. No segment at the exact position fails.
 *
 * `requiredFor` scopes an anchor to emission profiles. Most are required
 * for every profile of their framework. Exception: the pinned Vue compiler's
 * map for template-only `slots.vue` is sparse (vapor maps two positions,
 * ssr four, and they do not overlap with vdom), so that fixture's anchors
 * are per-backend. Floor from what the pinned oracles actually map, not a
 * ceiling: listed for a profile means required there.
 */
export const FIXTURE_ANCHORS = {
  "fixtures/vue/basic-interpolation.vue": [
    {
      id: "script-count-declaration",
      region: "script",
      line: 3,
      column: 6,
      text: "count",
      expectRelations: ["verbatim-carry"],
      requiredFor: VUE_PROFILE_KEYS,
    },
    {
      id: "template-count-interpolation",
      region: "template",
      line: 9,
      column: 27,
      text: "count",
      expectRelations: ["verbatim-carry", "context-binding-prefix"],
      requiredFor: VUE_PROFILE_KEYS,
    },
  ],
  "fixtures/vue/props-emit.vue": [
    {
      id: "script-onclick-declaration",
      region: "script",
      line: 7,
      column: 9,
      text: "onClick",
      expectRelations: ["verbatim-carry"],
      requiredFor: VUE_PROFILE_KEYS,
    },
    {
      id: "template-label-interpolation",
      region: "template",
      line: 13,
      column: 51,
      text: "label",
      expectRelations: ["verbatim-carry", "context-binding-prefix"],
      requiredFor: VUE_PROFILE_KEYS,
    },
  ],
  "fixtures/vue/slots.vue": [
    {
      id: "template-root-class-attribute",
      region: "template",
      line: 1,
      column: 7,
      text: "class",
      expectRelations: ["verbatim-carry"],
      requiredFor: ["vue:vdom", "vue:ssr"],
    },
    {
      id: "template-slot-fallback-text",
      region: "template",
      line: 3,
      column: 26,
      text: "Untitled",
      expectRelations: ["verbatim-carry", "delimiter-anchor"],
      requiredFor: ["vue:vdom"],
    },
    {
      id: "template-slot-name-attribute",
      region: "template",
      line: 3,
      column: 12,
      text: "name",
      expectRelations: ["verbatim-carry", "delimiter-anchor"],
      requiredFor: ["vue:vapor"],
    },
  ],
  "fixtures/svelte/basic-runes.svelte": [
    {
      id: "script-count-declaration",
      region: "script",
      line: 1,
      column: 6,
      text: "count",
      expectRelations: ["verbatim-carry"],
      requiredFor: SVELTE_PROFILE_KEYS,
    },
    {
      id: "template-count-condition",
      region: "template",
      line: 6,
      column: 7,
      text: "count",
      expectRelations: ["verbatim-carry"],
      requiredFor: SVELTE_PROFILE_KEYS,
    },
  ],
  "fixtures/svelte/props-events.svelte": [
    {
      id: "script-onclick-declaration",
      region: "script",
      line: 3,
      column: 11,
      text: "onClick",
      expectRelations: ["verbatim-carry"],
      requiredFor: SVELTE_PROFILE_KEYS,
    },
    {
      id: "template-disabled-shorthand-binding",
      region: "template",
      line: 8,
      column: 9,
      text: "disabled",
      expectRelations: ["verbatim-carry"],
      requiredFor: SVELTE_PROFILE_KEYS,
    },
  ],
  "fixtures/svelte/legacy-slots.svelte": [
    {
      id: "script-title-declaration",
      region: "script",
      line: 1,
      column: 13,
      text: "title",
      expectRelations: ["verbatim-carry"],
      requiredFor: SVELTE_PROFILE_KEYS,
    },
    {
      id: "template-title-interpolation",
      region: "template",
      line: 6,
      column: 25,
      text: "title",
      expectRelations: ["verbatim-carry"],
      requiredFor: SVELTE_PROFILE_KEYS,
    },
  ],
};

/** The 0-based span an acorn node occupies, from the parser's own `loc`. */
function spanOf(node) {
  return {
    startLine: node.loc.start.line - 1,
    startColumn: node.loc.start.column,
    endLine: node.loc.end.line - 1,
    endColumn: node.loc.end.column,
  };
}

/** Visits every AST node, passing its parent and the key it hangs under. */
function walkAst(node, visit, parent = null, key = null) {
  if (node === null || typeof node !== "object") return;
  if (Array.isArray(node)) {
    for (const child of node) walkAst(child, visit, parent, key);
    return;
  }
  if (typeof node.type !== "string") return;
  visit(node, parent, key);
  for (const field of Object.keys(node)) {
    if (field === "loc" || field === "start" || field === "end" || field === "range") continue;
    walkAst(node[field], visit, node, field);
  }
}

/**
 * Ranges of one generated module with no authored counterpart — derived
 * from that module's own syntax tree plus the profile's emitted-identifier
 * vocabulary, nothing else.
 *
 * Candidate-derived, not golden-derived: cosmetic carrier differences are
 * permitted, so golden geometry would address the wrong document the moment
 * a candidate emits an extra blank line. Parsing the artifact under
 * validation is the only derivation that stays true for every candidate.
 *
 * A name is compiler-introduced only when it matches a profile
 * `emittedIdentifier` shape AND does not occur as a word in the authored
 * fixture's script blocks (an author who wrote `_ref` / `_authored` /
 * `_component` is never swept in). `contextRoots` (`_ctx`, `$$props`) are
 * never claimed — they carry authored provenance via `context-binding-prefix`.
 *
 * Range classes:
 *
 *  - Runtime-helper import: source in closed `runtimeModules`, and either
 *    binds nothing (`import 'svelte/internal/disclose-version'`) or binds
 *    only claimable locals. `.every` (not `.some`) keeps mixed imports out;
 *    the closed source set keeps `import { thing as _thing } from "./my-utils.js"`
 *    out however the local is spelled.
 *  - Emitted declaration site: claimable name in an enumerated binding
 *    position (declarator id, function/catch parameter, pattern target,
 *    function/class id, non-computed class member key, import-specifier
 *    local, non-computed object-literal key). Object-pattern keys are not
 *    sites — they name an authored source property (`{ _sourceKey: authored }`
 *    binds `authored`; official maps carry `_sourceKey` verbatim). A
 *    reference to such a binding is not a declaration site: pinned Vue maps
 *    helper call sites and hoisted-node arguments back to the template, so
 *    claiming them would reject correct maps.
 *  - Generated plumbing: any-depth statement wiring two generated bindings
 *    with no authored payload (`_sfc_main.render = render` /
 *    `_sfc_main['render'] = render`). Non-identifier RHS (`_sfc_main.props = { … }`)
 *    is not captured.
 *  - Generated default export: `export default _sfc_main` and
 *    `export default _export_sfc(_sfc_main, [ … ])` (claimable callee).
 *  - Generated helper call: callee of a statement-level call rooted at a
 *    claimable binding, plus claimable identifiers handed directly to such
 *    a call. Only callee and direct identifier arguments.
 *  - Generated return: `return <claimable identifier>` — no authored payload.
 *
 * Uncovered (pinned compilers emit no provenance marker):
 *
 *  1. Compiler scaffolding with authored-shaped identifiers. Vue
 *     `compileScript` interleaves synthesized wrapper with authored
 *     statements (`render`, `setup`, `Object.defineProperty`).
 *  2. A reference to a claimable binding outside the enumerated statement
 *     forms. Widening `return <claimable identifier>` to `return <call
 *     rooted at a claimable binding>` would claim 5 corpus sites; 2 of them
 *     (`return _createElementVNode("li", …)` in `basic-interpolation__vdom`)
 *     carry a real official source-bearing segment on the callee. The 3
 *     Svelte `return $.pop($$exports);` sites carry none, but no rule
 *     reaches them without also reaching the two that are not free.
 *  3. Non-identifier payload of a synthesized statement. A literal in a
 *     synthesized call can carry authored provenance (`'click'` from
 *     `onclick=`), so claiming the whole statement would reject correct maps.
 *
 * Uncovered remainder still goes through the relation table
 * (`framework-emitted-token` / `delimiter-anchor`) — not position-exact
 * (see RELATIONS).
 *
 * `boundary` applies the no-inherited-provenance check only when the range
 * starts its own generated line. A range covers only the construct it names,
 * so a plant in the whitespace immediately to its left escapes containment
 * while a consumer resolving the start column still finds it (`resolveAt` =
 * last segment on the line at or before that column). Whitespace-only prefix
 * keeps the requirement; mid-line ranges are exempt because they sit inside
 * a legitimately mapped statement (`for (const _for_item0 of …)`,
 * `if (count > 0) $$render(consequent)`), and demanding a boundary segment
 * there requires density no compiler emits. Corpus: of 244 statement-level
 * ranges, 193 with whitespace-only prefix cost zero official segments;
 * enforcing on the remaining 51 would reject 4 real `$$render` calls in
 * basic-runes client goldens. Inline `emitted declaration` is exempt.
 *
 * Mid-line exemption therefore accepts a fabricated segment immediately
 * before the range (4 nested helper calls, 47 direct call arguments, 265
 * emitted declarations in the corpus). Bound, not oversight: the enclosing
 * expression at those columns is itself legitimately mappable.
 *
 * @param {string} code the generated module under validation
 * @param {{ emittedIdentifier: RegExp[], runtimeModules: string[],
 *   contextRoots: string[] }} profile the framework vocabulary
 * @param {string[]} [authoredLines] authored fixture line table; script-block
 *   words are subtracted from the claimable set. Omitted only by callers
 *   with no authored side (synthetic-module probes).
 * @returns {Array<{ label: string, startLine: number, startColumn: number,
 *   endLine: number, endColumn: number, boundary?: boolean }>}
 */
export function generatedOnlyRanges(code, profile, authoredLines = []) {
  const authored = authoredLines.length > 0 ? authoredScriptIndex(authoredLines).scriptNames : null;
  const claimable = (name) =>
    profile.emittedIdentifier.some((shape) => shape.test(name)) &&
    !profile.contextRoots.includes(name) &&
    (authored === null || !authored.has(name));
  const ast = parseModule(code, "generated-only-ranges");
  const genLines = lineTable(code);
  // Range starts its own generated line (prefix is whitespace) — the only
  // case where the boundary requirement is meaningful.
  const startsOwnLine = (span) =>
    /^\s*$/.test((genLines[span.startLine] ?? "").slice(0, span.startColumn));
  const ranges = [];

  walkAst(ast, (node) => {
    if (node.type === "ImportDeclaration") {
      if (
        profile.runtimeModules.includes(String(node.source.value)) &&
        node.specifiers.every((specifier) => claimable(specifier.local.name))
      ) {
        ranges.push({
          label: `runtime-helper import ${JSON.stringify(node.source.value)}`,
          ...spanOf(node),
        });
      }
      return;
    }
    if (node.type === "ExpressionStatement" && node.expression.type === "AssignmentExpression") {
      const { left, right } = node.expression;
      const property = left.type === "MemberExpression" ? memberName(left) : null;
      if (
        left.type === "MemberExpression" &&
        property !== null &&
        left.object.type === "Identifier" &&
        claimable(left.object.name) &&
        right.type === "Identifier"
      ) {
        ranges.push({
          label: `generated plumbing ${left.object.name}.${property}`,
          ...spanOf(node),
        });
      }
      return;
    }
    if (node.type === "ExpressionStatement" && node.expression.type === "CallExpression") {
      // Only the callee is claimed. Arguments can carry authored payload
      // (`$.delegate(['click'])` from `onclick=`).
      const callee = node.expression.callee;
      const root =
        callee.type === "Identifier"
          ? callee
          : callee.type === "MemberExpression" && callee.object.type === "Identifier"
            ? callee.object
            : null;
      if (root !== null && claimable(root.name)) {
        const span = spanOf(callee);
        ranges.push({
          label: `generated helper call ${root.name}`,
          ...span,
          boundary: startsOwnLine(span),
        });
      }
      // Claimable identifier handed directly to a statement-level call
      // (`Object.defineProperty(__returned__, …)`). Nested names inside an
      // argument expression are mapped by the pinned compilers
      // (`_createElementVNode(…, [_hoisted_1, …])`).
      for (const argument of node.expression.arguments) {
        if (argument.type === "Identifier" && claimable(argument.name)) {
          const span = spanOf(argument);
          ranges.push({
            label: `generated helper argument ${argument.name}`,
            ...span,
            boundary: startsOwnLine(span),
          });
        }
      }
      return;
    }
    if (
      node.type === "ReturnStatement" &&
      node.argument !== null &&
      node.argument !== undefined &&
      node.argument.type === "Identifier" &&
      claimable(node.argument.name)
    ) {
      const span = spanOf(node);
      ranges.push({
        label: `generated return ${node.argument.name}`,
        ...span,
        boundary: startsOwnLine(span),
      });
      return;
    }
    if (node.type === "ExportDefaultDeclaration") {
      const declaration = node.declaration;
      if (declaration.type === "Identifier" && claimable(declaration.name)) {
        ranges.push({
          label: `generated default export ${declaration.name}`,
          ...spanOf(node),
        });
      } else if (
        declaration.type === "CallExpression" &&
        declaration.callee.type === "Identifier" &&
        claimable(declaration.callee.name)
      ) {
        ranges.push({
          label: `generated default export ${declaration.callee.name}(…)`,
          ...spanOf(node),
        });
      }
    }
  });

  // Object-pattern properties bind their value; object-literal properties
  // do not. Same node type, so patterns are collected first.
  const patternProperties = new Set();
  walkAst(ast, (node) => {
    if (node.type === "ObjectPattern") {
      for (const property of node.properties) patternProperties.add(property);
    }
  });
  walkAst(ast, (node, parent, key) => {
    if (node.type !== "Identifier" || parent === null || !claimable(node.name)) return;
    const binds =
      (parent.type === "VariableDeclarator" && key === "id") ||
      (BINDER_FUNCTION.test(parent.type) && key === "params") ||
      (parent.type === "CatchClause" && key === "param") ||
      (parent.type === "AssignmentPattern" && key === "left") ||
      (parent.type === "RestElement" && key === "argument") ||
      (parent.type === "ArrayPattern" && key === "elements") ||
      (parent.type === "Property" && key === "value" && patternProperties.has(parent)) ||
      // Object-literal key is introduced by the writer (`__name:`).
      // Object-pattern key names a source property (`v-for="{ _sourceKey:
      // authored } in items"` → `({ _sourceKey: authored }) => …`); only
      // the pattern value binds.
      (parent.type === "Property" &&
        key === "key" &&
        !parent.computed &&
        !patternProperties.has(parent)) ||
      (DECLARES_ID.test(parent.type) && key === "id") ||
      (CLASS_MEMBER.test(parent.type) && key === "key" && !parent.computed) ||
      (IMPORT_SPECIFIER.test(parent.type) && key === "local");
    if (binds) {
      ranges.push({
        label: `emitted declaration ${node.name}`,
        ...spanOf(node),
        boundary: false,
      });
    }
  });

  return ranges;
}

const DECLARES_ID = /^(FunctionDeclaration|FunctionExpression|ClassDeclaration|ClassExpression)$/;
const CLASS_MEMBER = /^(MethodDefinition|PropertyDefinition)$/;
const IMPORT_SPECIFIER = /^(ImportSpecifier|ImportDefaultSpecifier|ImportNamespaceSpecifier)$/;

/** A member expression's property NAME, for `a.b` and `a["b"]` alike. */
function memberName(member) {
  if (!member.computed) return member.property.name ?? null;
  return typeof member.property.value === "string" ? member.property.value : null;
}

/**
 * Resolve a map source spelling to an absolute path under each declared
 * base. Normalize separators to the host's before any platform-aware path
 * op: a Windows-produced map may use `\`, and mixing `path.posix.join` with
 * `path.isAbsolute` / `path.resolve` would leave that spelling unresolvable.
 */
function resolvesToFixture(spelling, sourceRoot, bases, fixtureAbsolutePath) {
  const toPosix = (value) => String(value).split("\\").join("/");
  const posixSpelling = toPosix(spelling);
  const joined = sourceRoot ? path.posix.join(toPosix(sourceRoot), posixSpelling) : posixSpelling;
  const native = joined.split("/").join(path.sep);
  const target = path.resolve(fixtureAbsolutePath);
  if (path.isAbsolute(native)) return path.resolve(native) === target;
  return bases.some((base) => path.resolve(base, native) === target);
}

/**
 * Line-scoped resolution: last segment on that line at or before the
 * column. No earlier segment → unmapped (makes the boundary requirement
 * meaningful).
 */
function resolveAt(segmentsByLine, line, column) {
  const onLine = segmentsByLine.get(line);
  if (onLine === undefined) return null;
  let found = null;
  for (const segment of onLine) {
    if (segment.genCol <= column) found = segment;
    else break;
  }
  return found;
}

/**
 * Validate a candidate's map against the candidate's own generated code and
 * the authored fixture.
 *
 * Requirement 6's generated-only ranges are derived here from `code`
 * (`generatedOnlyRanges`) — never supplied by the caller, so an empty range
 * list cannot disable the rail. `extraSyntheticRanges` only adds ranges the
 * module text does not expose; it cannot subtract.
 *
 * @param {{
 *   code: string|null,
 *   map: object|null,
 *   sourceMapRequested: boolean,
 *   fixture: { path: string, absolutePath: string },
 *   sourceResolveBases: string[],
 *   profile: object,
 *   anchors?: Array<object>,
 *   extraSyntheticRanges?: Array<{ label: string, startLine: number, startColumn: number,
 *     endLine: number, endColumn: number, boundary?: boolean }>,
 * }} input
 * @returns {{ ok: boolean, violations: Array<{ rule: string, detail: string }>,
 *   stats: object }}
 */
export function validateAuthoredMapping({
  code,
  map,
  sourceMapRequested,
  fixture,
  sourceResolveBases,
  profile,
  anchors = [],
  extraSyntheticRanges = [],
}) {
  const violations = [];
  const fail = (rule, detail) => violations.push({ rule, detail });
  const stats = {
    sourceBearingSegments: 0,
    sourcelessSegments: 0,
    classifications: {},
    anchors: anchors.length,
    syntheticRanges: 0,
  };

  if (map === null || map === undefined) {
    if (sourceMapRequested) fail("map-presence", "sourceMap was requested but no map was produced");
    return { ok: violations.length === 0, violations, stats };
  }
  if (!sourceMapRequested) {
    fail("map-presence", "a map was produced although sourceMap was not requested");
    return { ok: false, violations, stats };
  }
  if (code === null || code === undefined) {
    fail("map-presence", "a map was produced for a compilation that emitted no code");
    return { ok: false, violations, stats };
  }

  // requirement 1: contract + bounds
  if (map.version !== 3)
    fail("map-version", `expected version 3, got ${JSON.stringify(map.version)}`);

  let segments;
  try {
    segments = decodeMappings(map.mappings ?? "");
  } catch (error) {
    fail("mappings-decode", `mappings is not decodable source-map v3 VLQ: ${error.message}`);
    return { ok: false, violations, stats };
  }

  const sources = map.sources ?? [];
  const names = map.names ?? [];
  const genLines = lineTable(code);
  const fixtureSource = readFileSync(fixture.absolutePath, "utf8");
  const srcLines = lineTable(fixtureSource);

  // Derived from the code under validation so requirement 6 runs on every
  // artifact. Unparseable generated code is a parse-axis defect; here the
  // rail cannot be derived and that is recorded.
  let syntheticRanges;
  try {
    syntheticRanges = [...generatedOnlyRanges(code, profile, srcLines), ...extraSyntheticRanges];
  } catch (error) {
    fail(
      "synthetic-range-derivation",
      `generated-only ranges could not be derived from the generated code: ${error.message}`,
    );
    syntheticRanges = [...extraSyntheticRanges];
  }
  stats.syntheticRanges = syntheticRanges.length;

  // requirement 2: source identity
  if (sources.length === 0) fail("source-identity", "map declares no sources");
  sources.forEach((spelling, index) => {
    if (typeof spelling !== "string") {
      fail("source-identity", `sources[${index}] is not a string`);
      return;
    }
    if (!resolvesToFixture(spelling, map.sourceRoot, sourceResolveBases, fixture.absolutePath)) {
      fail(
        "source-identity",
        `sources[${index}] ${JSON.stringify(spelling)} (sourceRoot ${JSON.stringify(map.sourceRoot ?? null)}) does not resolve to the authored fixture ${fixture.path}`,
      );
    }
    const content = map.sourcesContent?.[index];
    if (content !== undefined && content !== null && content !== fixtureSource) {
      fail(
        "sources-content",
        `sourcesContent[${index}] differs from the bytes of ${fixture.path} on disk`,
      );
    }
  });

  // requirement 3: per-segment truthfulness
  const segmentsByLine = new Map();
  for (const segment of segments) {
    if (!segmentsByLine.has(segment.genLine)) segmentsByLine.set(segment.genLine, []);
    segmentsByLine.get(segment.genLine).push(segment);
  }
  for (const onLine of segmentsByLine.values()) onLine.sort((a, b) => a.genCol - b.genCol);

  for (const segment of segments) {
    const gen = tokenAt(genLines, segment.genLine, segment.genCol);
    if (gen.kind === "out-of-bounds") {
      fail(
        "generated-position-bounds",
        `segment generated position ${segment.genLine}:${segment.genCol} is outside the generated code`,
      );
      continue;
    }
    if (segment.srcIdx === null) {
      stats.sourcelessSegments += 1;
      continue;
    }
    stats.sourceBearingSegments += 1;
    if (segment.srcIdx < 0 || segment.srcIdx >= sources.length) {
      fail(
        "source-index-bounds",
        `segment at ${segment.genLine}:${segment.genCol} names source index ${segment.srcIdx}, out of ${sources.length}`,
      );
      continue;
    }
    const nameInBounds =
      segment.nameIdx !== null && segment.nameIdx >= 0 && segment.nameIdx < names.length;
    if (segment.nameIdx !== null && !nameInBounds) {
      fail(
        "name-index-bounds",
        `segment at ${segment.genLine}:${segment.genCol} names name index ${segment.nameIdx}, out of ${names.length}`,
      );
    }
    const src = tokenAt(srcLines, segment.srcLine, segment.srcCol);
    if (src.kind === "out-of-bounds") {
      fail(
        "original-position-bounds",
        `segment at ${segment.genLine}:${segment.genCol} maps to ${segment.srcLine}:${segment.srcCol}, outside ${fixture.path}`,
      );
      continue;
    }
    // `names[nameIdx]` is a claim, not a bounds check. A named segment must
    // name the authored symbol at its original position or the generated
    // symbol at its generated position (`_<digits>` stripped) — the two
    // readings real producers use. Anything else is invented.
    if (nameInBounds) {
      const declared = names[segment.nameIdx];
      const admissible =
        typeof declared === "string" &&
        declared.length > 0 &&
        (declared === src.text ||
          declared === gen.text ||
          declared === withoutDisambiguator(gen.text));
      if (!admissible) {
        fail(
          "name-token-relation",
          `segment at ${segment.genLine}:${segment.genCol} declares name ${JSON.stringify(declared)}, which is neither the authored token ${JSON.stringify(src.text)} at ${segment.srcLine}:${segment.srcCol} nor its own generated token ${JSON.stringify(gen.text)}`,
        );
      }
    }
    const relation = classifySegment({ gen, src, genLines, srcLines, segment, profile });
    if (relation === null) {
      fail(
        "segment-provenance",
        `segment at ${segment.genLine}:${segment.genCol} (generated ${JSON.stringify(gen.rest)}) maps to ${segment.srcLine}:${segment.srcCol} (authored ${JSON.stringify(src.rest)}) under no declared relation`,
      );
      continue;
    }
    stats.classifications[relation] = (stats.classifications[relation] ?? 0) + 1;
  }

  // requirement 4: required anchors, both directions
  const requiredAnchors = anchors.filter((anchor) => anchor.requiredFor.includes(profile.key));
  stats.anchors = requiredAnchors.length;
  for (const anchor of requiredAnchors) {
    const line = srcLines[anchor.line];
    if (
      line === undefined ||
      line.slice(anchor.column, anchor.column + anchor.text.length) !== anchor.text
    ) {
      fail(
        "anchor-source-text",
        `anchor ${anchor.id}: ${fixture.path}:${anchor.line}:${anchor.column} does not read ${JSON.stringify(anchor.text)}`,
      );
      continue;
    }
    // Directions checked independently: generated → source (segment lands
    // on the anchor's exact start) and source → generated (authored span
    // has any generated counterpart).
    const exact = segments.filter(
      (segment) =>
        segment.srcIdx !== null &&
        segment.srcLine === anchor.line &&
        segment.srcCol === anchor.column,
    );
    if (exact.length === 0) {
      fail(
        "anchor-missing",
        `anchor ${anchor.id}: no segment maps to ${fixture.path}:${anchor.line}:${anchor.column}`,
      );
    }
    const withinSpan = segments.some(
      (segment) =>
        segment.srcIdx !== null &&
        segment.srcLine === anchor.line &&
        segment.srcCol >= anchor.column &&
        segment.srcCol < anchor.column + anchor.text.length,
    );
    if (!withinSpan) {
      fail(
        "anchor-span-coverage",
        `anchor ${anchor.id}: no segment falls within its authored span`,
      );
    }
    if (exact.length === 0) continue;
    const satisfied = exact.some((segment) => {
      const gen = tokenAt(genLines, segment.genLine, segment.genCol);
      const src = tokenAt(srcLines, segment.srcLine, segment.srcCol);
      const relation = classifySegment({ gen, src, genLines, srcLines, segment, profile });
      return relation !== null && anchor.expectRelations.includes(relation);
    });
    if (!satisfied) {
      fail(
        "anchor-relation",
        `anchor ${anchor.id}: no segment at its position expresses it under ${anchor.expectRelations.join(" | ")}`,
      );
    }
  }

  // requirement 6: synthetic ranges carry no authored provenance
  for (const range of syntheticRanges) {
    for (const segment of segments) {
      if (segment.srcIdx === null) continue;
      const afterStart =
        segment.genLine > range.startLine ||
        (segment.genLine === range.startLine && segment.genCol >= range.startColumn);
      const beforeEnd =
        segment.genLine < range.endLine ||
        (segment.genLine === range.endLine && segment.genCol < range.endColumn);
      if (afterStart && beforeEnd) {
        fail(
          "synthetic-provenance",
          `${range.label}: source-bearing segment at ${segment.genLine}:${segment.genCol} inside generated-only code`,
        );
      }
    }
    // No-inherited-provenance only when the range starts its own generated
    // line; mid-line ranges inherit enclosing-expression provenance.
    // See generatedOnlyRanges.
    if (range.boundary === false) continue;
    const inherited = resolveAt(segmentsByLine, range.startLine, range.startColumn);
    if (inherited !== null && inherited.srcIdx !== null) {
      fail(
        "synthetic-boundary",
        `${range.label}: a lookup at ${range.startLine}:${range.startColumn} inherits authored provenance from the segment at ${inherited.genLine}:${inherited.genCol} (no boundary segment separates them)`,
      );
    }
  }

  return { ok: violations.length === 0, violations, stats };
}
