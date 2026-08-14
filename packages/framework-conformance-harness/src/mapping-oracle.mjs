// The AUTHORED-SOURCE mapping oracle: validates ONE candidate's source map
// against THAT candidate's own generated code and the AUTHORED SFC fixture
// on disk. No golden map, and no official-compiler map, is an input.
//
// WHY THE REPLACED ORACLE COULD NOT WORK. The mapping axis previously
// compared the candidate's `mappings` field against the official
// compiler's. That is not merely the wrong oracle for this axis; it is
// structurally incapable of ever being the right one, as a direct
// consequence of the cosmetic-tolerance rule this project already enforces
// everywhere else. A `mappings` field encodes (generated position ->
// original position) correspondences over ONE SPECIFIC generated document.
// Verter's generated JS is legitimately NOT byte-identical to the official
// compiler's — differing indentation, line breaks, and behavior-preserving
// carrier shape are explicitly permitted — so the two maps address
// DIFFERENT generated documents by construction. Comparing them rejects a
// completely correct candidate map whose generated layout legitimately
// differs, and accepts a wrong candidate map whose segment shape happens to
// resemble official's. Neither direction is fixable by normalization: the
// coordinate spaces are not the same space.
//
// What IS checkable, and is what a user actually needs, is self-referential:
// does the candidate's map tell the truth about the candidate's own output
// and the source the user wrote? That is this module.
//
// THE THREE INPUTS are always: the authored fixture (read from disk here —
// never the map's self-reported `sourcesContent`), the candidate's generated
// code, and the candidate's map. The design is therefore backend- and
// framework-agnostic; a `profile` supplies the per-framework rewrite
// vocabulary and nothing else.
//
// WHAT IS AND IS NOT ENFORCED, stated up front rather than inferred from the
// requirement list. Every requirement below — including the generated-only
// range rule (requirement 6) — runs on every artifact this module validates,
// for BOTH Vue and Svelte, because the ranges are derived from the artifact's
// own parse rather than supplied by the caller (see generatedOnlyRanges).
//
// Its coverage is bounded, and the bound is stated as what it IS rather than
// as a flattering approximation. A generated-only range is produced for a
// CLAIMABLE name — one matching the profile's emitted-identifier shapes, not
// a render-scope context root, and not a word the author wrote in the
// fixture's own script blocks — and only where that name occupies an
// enumerated BINDING position (declarator id, function/catch parameter,
// pattern target, function/class declaration id, class member key,
// import-specifier local, object-literal key) or one of the enumerated
// STATEMENT forms (a runtime-module import, a member-assignment plumbing
// statement, a bare default export or wrapper-call default export, a helper
// call's callee, a claimable identifier handed directly to a helper call, a
// bare claimable return). Everything else is uncovered — most materially,
// compiler scaffolding spelled with AUTHORED-shaped identifiers, and the
// non-identifier PAYLOAD of a synthesized statement. Over that remainder the
// rail is the relation table, which for a claimable-shaped generated token
// means `framework-emitted-token` and for punctuation `delimiter-anchor` —
// and those two are NOT position-exact (see RELATIONS). That is the honest
// residual, in both frameworks alike.

import { readFileSync } from "node:fs";
import path from "node:path";

import { decodeMappings } from "./sourcemap.mjs";
import { parseModule } from "./normalize.mjs";

const WORD_CHARACTER = /[A-Za-z0-9_$]/;

/**
 * Splits text into lines on "\n", RETAINING any trailing "\r". Source-map
 * columns are UTF-16 code-unit offsets into the generated/original text as
 * the producer saw it, so a CRLF document's "\r" occupies a real column and
 * must stay in the line's length; stripping it would under-count every line
 * by one and reject valid end-of-line segments.
 */
export function lineTable(text) {
  return text.split("\n");
}

/**
 * The token at a (line, column) in a line table. Columns are UTF-16 code
 * units — exactly what `String.prototype.length` and index access measure —
 * so a non-ASCII or astral-plane character is counted the way a source-map
 * consumer counts it, never as bytes.
 *
 * @returns {{ kind: "word-start"|"word-interior"|"punct"|"eol"|"out-of-bounds",
 *   text: string, rest: string }} `text` is the whole word for word kinds
 *   (the single character for punct); `rest` is the text from `column` to
 *   the token's end, which is what an exact carry compares on.
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
 * The authored fixture's SCRIPT-block content regions, as `[start, end)`
 * character offsets into `source`.
 *
 * A `.vue`/`.svelte` fixture is not a JavaScript module, so the script halves
 * have to be located before anything can be parsed. That location is by
 * delimiter over the AUTHORED TEST FIXTURE — the harness's own committed
 * input, never a production carrier — and it is used only to bound a parse:
 * every structural fact below comes from the parser, not from the scan. A
 * region that does not parse contributes nothing (fail-closed), so a
 * mis-located or exotic block can only ever REMOVE evidence, never fabricate
 * it.
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
 * A projection of `source` in which every character OUTSIDE `[start, end)` is
 * replaced by a space, with newlines preserved. Parsing the projection yields
 * acorn locations that are already the authored fixture's own (line, column)
 * coordinates — no offset arithmetic, and therefore no class of off-by-one
 * between "where the parser saw it" and "where the map claims it is".
 */
function blankedOutside(source, start, end) {
  const blank = (text) => text.replace(/[^\n]/g, " ");
  return blank(source.slice(0, start)) + source.slice(start, end) + blank(source.slice(end));
}

const BINDER_FUNCTION = /^(FunctionDeclaration|FunctionExpression|ArrowFunctionExpression)$/;

/**
 * The destructuring patterns that occupy a genuine BINDING position — a
 * `VariableDeclarator.id`, a function parameter, or a catch-clause parameter
 * — and nothing else. An object EXPRESSION brace, a nested sub-pattern, and
 * an assignment-target pattern (`({ a } = obj)`, whose node is an
 * ObjectPattern under an AssignmentExpression) are all deliberately absent:
 * only a pattern reached through one of the three declaration forms above is
 * marked, so the caller's lookup cannot land on a brace that declares nothing.
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
 * The names a pattern binds AT ITS OWN LEVEL. A property KEY is not a binding
 * (`{ disabled: other }` binds `other`); a default-value EXPRESSION is not a
 * binding (`{ other = disabled }` binds `other`); a NESTED pattern's names are
 * not at this level (`{ data: { x } }` binds nothing here) — the mapped brace
 * would be the outer pattern's, and admitting the inner names would re-open
 * the "somewhere inside" looseness this relation exists to close.
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
 * The authored fixture's script-derived index, memoized per line table.
 *
 *  - `bindingPatterns`: `"<line>:<column>"` of a binding pattern's OPENING
 *    delimiter -> the names it binds at its own level. This is what makes
 *    `destructured-binding-pattern` position-exact: the lookup key is the
 *    parser's own start position for a pattern in a declaration position, so
 *    an object literal, a template interpolation `{{ x }}`, a Svelte
 *    shorthand `{x}` and a block directive `{#each …}` are all absent by
 *    construction rather than by exclusion.
 *  - `scriptNames`: every identifier-shaped word inside the authored script
 *    blocks. A generated name that the AUTHOR also wrote is not claimable as
 *    compiler-introduced, so this set is subtracted from the generated-only
 *    ranges below (see generatedOnlyRanges).
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
 * The per-framework REWRITE VOCABULARY — the only framework-specific input
 * the oracle takes. Each field names a rewrite CLASS (never a fixture):
 *
 *  - `contextRoots`: the render-scope roots a template binding resolves
 *    through (`_ctx.count`, `$setup.count`, …). A segment whose generated
 *    token is one of these must satisfy the `context-binding-prefix` tie
 *    (the accessed property IS the authored identifier) or the narrower
 *    `component-instance-surface` relation — it never falls through to the
 *    generic emitted-token relation.
 *  - `macroBindings`: generated binding -> the authored macro call it
 *    stands for (`__props` <- `defineProps`).
 *  - `emittedIdentifier`: the shapes a compiler-EMITTED identifier may take
 *    (imported runtime helpers, hoisted nodes, synthesized locals). A
 *    generated identifier outside these shapes has no fallback: it must
 *    carry authored text verbatim or satisfy a named tie.
 *  - `runtimeModules`: the CLOSED set of module specifiers the framework's
 *    own runtime lives behind, matched by EXACT string membership. It is
 *    what makes the runtime-helper import rule a statement about provenance
 *    rather than about spelling: an authored `import { thing as _thing }
 *    from "./my-utils.js"` is outside the set and can never be swept in,
 *    however its local is spelled. Exact membership rather than a namespace
 *    prefix, because a prefix rule also claims the framework's
 *    AUTHOR-FACING entry points (`vue/reactivity`, `@vue/shared`,
 *    `svelte/store`) — which a fixture may legitimately import and whose
 *    truthful segments the pinned compilers really do carry. The
 *    side-effect form is where that matters most: `import "svelte/store";`
 *    binds no local, so the authored-name subtraction below cannot rescue
 *    it. The six members are the specifiers the pinned compilers actually
 *    emit across the whole committed corpus (enumerated from all 48
 *    goldens: `vue` ×48, `vue/server-renderer` ×12, `svelte/internal/client`
 *    ×6, `svelte/internal/server` ×6, `svelte/internal/disclose-version`
 *    ×6, `svelte/internal/flags/legacy` ×2), so a specifier that appears
 *    here is a measurement, not a guess. A compiler upgrade that emits a
 *    new one fails LOUDLY — its import is no longer generated-only — rather
 *    than silently widening the claim.
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
 * One profile per (framework, emission target). The rewrite vocabulary is
 * shared across a framework's targets — the target only distinguishes which
 * anchors a map is required to carry (see FIXTURE_ANCHORS).
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
 * The ORDERED relation table. Each entry is a NAMED, checkable rule from a
 * generated-token shape to an original-token shape; the first match
 * classifies the segment. A segment matching none is a violation
 * (`segment-provenance`) — never a skip, never an "in bounds so probably
 * fine" pass.
 *
 * POSITION BINDING, stated per relation rather than in general terms.
 * POSITION-EXACT — the mapped ORIGINAL position must BE a specific authored
 * lexeme (and for `verbatim-carry`, a specific offset inside it), so
 * re-pointing the segment anywhere else, including elsewhere on the same
 * authored line, breaks the relation: `verbatim-carry`,
 * `context-binding-prefix`, `macro-result-binding`, `event-handler-key`,
 * `synthesized-local-for-authored-name`, `destructured-binding-pattern`.
 * None of these is satisfied by "the right text appears somewhere nearby".
 *
 * One bound is narrower than the others and is stated rather than implied.
 * `verbatim-carry` is a TEXT-EQUALITY relation (same lexeme, same offset
 * inside it), so it is position-exact only UP TO identical-lexeme
 * interchangeability: two occurrences of the same token at the same offset
 * are interchangeable under it. That is inherent to the relation and
 * deliberate — the alternative, requiring `word-start` on both sides, would
 * reject the interior->interior segments the pinned official compilers
 * really emit. The other five above pin a position no other occurrence of
 * the same text satisfies.
 *
 * NOT position-exact, and named here so no reader has to infer it:
 * `component-instance-surface`, `framework-emitted-token` and
 * `delimiter-anchor` constrain only the GENERATED side (a profile-declared
 * compiler-emitted token, a context root, or a delimiter) and accept any
 * in-bounds authored position that is not word-interior. Their strength is
 * the generated-side precondition: any AUTHORED-looking generated
 * identifier falls outside all three and must satisfy a position-exact
 * relation above. That is the correspondence class every IDE feature
 * consumes. The generated-only-range requirement below covers the
 * scaffolding half of the loose class; what remains uncovered is recorded
 * at generatedOnlyRanges.
 */
const RELATIONS = [
  {
    name: "verbatim-carry",
    // POSITION-EXACT, not text-similar. The two positions must sit at the
    // SAME offset inside the SAME lexeme: `gen.text === src.text` pins the
    // whole token on both sides, and `gen.rest === src.rest` on top of equal
    // token text pins the offset within it (rest is the token's tail from
    // the mapped column, so equal text + equal tail => equal offset).
    // Comparing only `rest` — the tail — accepted two DIFFERENT tokens that
    // happened to share a trailing substring: generated `import`@col5 and
    // authored `script`@col6 both yield the tail "t".
    //
    // Word-INTERIOR positions are deliberately still admissible (the pinned
    // official compilers really do emit interior→interior segments, e.g. the
    // final `t` of a generated `import` carried from the final `t` of the
    // authored one); what the tightening removes is any pair whose lexemes
    // differ. Requiring `word-start` on both sides instead would reject
    // genuine official output.
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
    // POSITION-EXACT: the mapped position must BE the start of the authored
    // occurrence the generated local was named after, not merely share its
    // LINE. The previous line-scoped tie (`containsWholeWord` over
    // `srcLines[segment.srcLine]`) accepted a generated `items` re-pointed
    // from the authored `items` declarator to column 0 of the same line —
    // the `const` keyword — because the line still contained the word
    // somewhere.
    match: ({ gen, src }) =>
      gen.kind === "word-start" &&
      src.kind === "word-start" &&
      src.text === withoutDisambiguator(gen.text),
  },
  {
    name: "destructured-binding-pattern",
    // A local the compiler hoisted OUT of an authored destructuring pattern
    // is anchored by the pinned Svelte compiler at the PATTERN's opening
    // delimiter, not at the bound name: `let { label, disabled = false } =
    // $props()` yields `let disabled = $.prop(…)` whose `disabled` maps to
    // the authored `{`. This is a real official correspondence, so the
    // position-exact tightening above needs it named rather than absorbed
    // back into a line-scoped text match.
    //
    // It stays position-exact in its own right, and STRUCTURALLY so: the
    // mapped position must be the PARSER's own start position for a
    // destructuring pattern that sits in a declaration position (a
    // `VariableDeclarator.id`, a function parameter, or a catch-clause
    // parameter) inside the authored fixture's script block, and the name
    // must be one of the identifiers that pattern binds at its own level.
    // Re-pointing the segment to any other position — an object LITERAL
    // brace, a Vue interpolation `{{ x }}`, a Svelte shorthand `{x}` or block
    // directive `{#each …}`, a property key, a default-value expression, a
    // nested sub-pattern, or anywhere else on the same line — breaks it,
    // because none of those is a key in the parsed binding-pattern index.
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
 * The REQUIRED anchors per authored fixture — authored HERE from the fixture
 * text itself (each anchor names the lexeme it points at, and the oracle
 * re-reads the fixture from disk to confirm that lexeme really is at that
 * position), never derived from any compiler's output. Each anchor also
 * names the relation(s) under which a correct map may express it. An anchor
 * with no segment at its exact position is a completeness gap and FAILS.
 *
 * `requiredFor` scopes an anchor to the emission profiles that must carry
 * it. Most anchors are required for every profile of their framework. The
 * exceptions are recorded, not hidden: the pinned official Vue compiler's
 * map for a TEMPLATE-ONLY SFC (`slots.vue`) is extremely sparse — the vapor
 * backend maps two positions in the whole file and the ssr backend four, and
 * they do not overlap with the vdom backend's — so that fixture's anchors
 * are per-backend. This is a FLOOR derived from what the pinned oracles
 * demonstrably map, not a ceiling: an anchor listed for a profile is a hard
 * requirement there.
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
 * The ranges of ONE generated module that have no authored counterpart —
 * derived from THAT module's own syntax tree plus the profile's declared
 * emitted-identifier vocabulary, and from nothing else.
 *
 * This is deliberately CANDIDATE-DERIVED rather than golden-derived. A
 * candidate's generated layout is legitimately its own (cosmetic carrier
 * differences are permitted), so geometry recorded when the golden was
 * produced would address the wrong document the moment a candidate emits an
 * extra blank line. Parsing the artifact under validation is the only
 * derivation that stays true for every candidate — and it is why this rail
 * runs at the acceptance boundary rather than only where an assembler's
 * fragment geometry happens to be in scope.
 *
 * WHAT MAKES A POSITION CLAIMABLE. A name is treated as compiler-introduced
 * only when BOTH hold: it matches one of the profile's `emittedIdentifier`
 * shapes, and it does NOT occur as a word anywhere in the AUTHORED fixture's
 * script blocks. The second half is the one that turns a spelling heuristic
 * into evidence: an author who really did write `_ref`, `_authored` or
 * `_component` puts that word in the file the oracle already reads from disk,
 * and their code is then never swept into a generated-only range. A profile
 * `contextRoots` name (`_ctx`, `$$props`) is likewise never claimed — those
 * carry authored provenance by design, through `context-binding-prefix`.
 *
 * The range classes, each a structural fact about the parsed module:
 *
 *  - RUNTIME-HELPER IMPORT: an `import` whose SOURCE is in the profile's
 *    closed `runtimeModules` set AND which either binds nothing at all
 *    (`import 'svelte/internal/disclose-version'` — a side-effect import of
 *    the framework runtime is generated by construction) or binds only
 *    claimable locals (`import { toDisplayString as _toDisplayString } from
 *    "vue"`, `import * as $ from "svelte/internal/client"`). Both halves are
 *    load-bearing: `.every` (not `.some`) keeps a MIXED import — one emitted
 *    local, one authored local — entirely out, and the closed source set
 *    keeps an authored `import { thing as _thing } from "./my-utils.js"` out
 *    however its local is spelled.
 *  - EMITTED DECLARATION SITE: a claimable name in an enumerated BINDING
 *    position — declarator id, function parameter, catch-clause parameter,
 *    default/rest/array pattern target, object-pattern property value,
 *    function- or class-declaration id, non-computed class member key,
 *    import-specifier local, or non-computed object-LITERAL key (`__name:`,
 *    `setup(__props)`, `_hoisted_1`, `root_1`, `$$anchor`). The compiler
 *    INTRODUCED the name at that position; no authored token sits behind it.
 *    An object-PATTERN key is NOT one of them — it names a property of the
 *    source object, which is authored material the compiler carried through
 *    (`{ _sourceKey: authored }` binds `authored`, and real official maps
 *    do carry `_sourceKey` verbatim). The pattern's VALUE is the binding.
 *    A REFERENCE to such a binding is deliberately NOT a declaration site:
 *    the pinned Vue compiler really does map its helper call sites and
 *    hoisted-node arguments (`_createElementVNode(_hoisted_1, …)`) back to
 *    the authored template, so claiming them would reject correct maps.
 *  - GENERATED PLUMBING STATEMENT: a statement, AT ANY DEPTH, that wires two
 *    generated bindings together and carries no authored payload —
 *    `_sfc_main.render = render` and its computed spelling
 *    `_sfc_main['render'] = render` (member assignment rooted at a claimable
 *    binding, bare-identifier right-hand side). A statement with a
 *    non-identifier right-hand side (`_sfc_main.props = { … }`) carries
 *    authored material and is NOT captured.
 *  - GENERATED DEFAULT EXPORT: `export default _sfc_main`, and the standard
 *    wrapper footer `export default _export_sfc(_sfc_main, [ … ])` (a call
 *    whose callee is claimable).
 *  - GENERATED HELPER CALL: the CALLEE of a statement-level call rooted at a
 *    claimable binding (`__expose()`, `$.push(…)`, `$.pop()`,
 *    `$.delegate([…])`), plus any claimable identifier handed DIRECTLY to
 *    such a call (`Object.defineProperty(__returned__, …)` — whose callee is
 *    the authored-shaped `Object`). Only the callee and direct identifier
 *    arguments; see the payload bound below.
 *  - GENERATED RETURN: `return <claimable identifier>` (`return
 *    __returned__`), which carries no authored payload at all.
 *
 * WHAT THIS DOES NOT COVER, stated as the real boundary rather than a
 * flattering approximation of it. The pinned compilers emit no provenance
 * marker, so the covered set is exactly: claimable names in the enumerated
 * binding positions, plus the enumerated statement forms above. Everything
 * else is uncovered, and the three materially large classes are:
 *
 *  1. Compiler scaffolding spelled with AUTHORED-shaped identifiers. Vue's
 *     `compileScript` interleaves its synthesized wrapper with authored
 *     statements (`render`, `setup`, `Object.defineProperty`), and nothing
 *     in the emitted artifact separates the two without re-deriving the
 *     compiler's own bookkeeping.
 *  2. A REFERENCE to a claimable binding outside the enumerated statement
 *     forms — a helper call nested in an expression, a hoisted node passed
 *     as an argument inside a render return. The measurement behind the
 *     exclusion is stated per RULE, not per occurrence, because that is
 *     what a rule change costs: widening `return <claimable identifier>` to
 *     `return <call rooted at a claimable binding>` would claim 5 corpus
 *     sites, and 2 of them (`return _createElementVNode("li", …)` in the
 *     mapped `basic-interpolation__vdom` goldens) carry a REAL official
 *     source-bearing segment on the callee the rule would claim. Individual
 *     sites inside the excluded class are cheaper than that — the 3 Svelte
 *     `return $.pop($$exports);` occurrences carry no real segment at all,
 *     so a fabrication there is accepted — but no rule reaches them without
 *     also reaching the two that are not free. The exclusion is that
 *     measurement, not a claim that every uncovered site is load-bearing.
 *  3. The non-identifier PAYLOAD of a synthesized statement — string and
 *     object literals, punctuation, and the non-identifier argument lists of
 *     helper calls (`$.delegate(['click'])`, `Object.defineProperty(…,
 *     '__isScriptSetup', …)`). A literal in a synthesized call genuinely can
 *     carry authored provenance (`'click'` comes from an authored
 *     `onclick=`), so claiming the whole statement would reject correct maps.
 *
 * A segment inside the uncovered remainder is still subject to the relation
 * table — which for a claimable-shaped generated token means
 * `framework-emitted-token`, and for punctuation `delimiter-anchor`. Those
 * two are NOT position-exact (see RELATIONS): they constrain the generated
 * side only and accept any in-bounds, non-word-interior authored position.
 * That is the honest residual rail over the uncovered set.
 *
 * `boundary` decides whether the no-inherited-provenance check applies to a
 * range, and the deciding property is whether the range STARTS ITS OWN
 * generated LINE. This is not a stylistic distinction: a range covers only
 * the construct it names, so the column immediately to its left is outside
 * every range and a fabricated segment planted there escapes the
 * containment check entirely — while a consumer resolving the range's own
 * start column still finds it, because the applying segment is the last one
 * on the line at or before that column (`resolveAt`). A plant in the
 * whitespace before `__expose();` therefore reports authored provenance for
 * `__expose` exactly as a plant ON the callee would.
 *
 * So a range whose same-line prefix is pure whitespace keeps the
 * requirement: no legitimate enclosing expression can supply provenance at
 * that column. A range that begins MID-LINE is exempt — an emitted
 * identifier sitting inside a larger, legitimately-mapped statement (`for
 * (const _for_item0 of …)`, `if (count > 0) $$render(consequent)`)
 * correctly inherits the enclosing expression's provenance, and demanding a
 * boundary segment before every one would require a segment density no
 * compiler emits. Both halves are measured over the committed corpus: of
 * the 244 statement-level ranges, the 193 with a whitespace-only prefix
 * carry the requirement and cost ZERO real official segments, while
 * enforcing it on the remaining 51 would reject 4 real ones (the `$$render`
 * calls nested inside a mapped expression in the basic-runes client
 * goldens). The inline `emitted declaration` class is exempt throughout.
 *
 * What the exemption therefore still admits, said plainly: for a mid-line
 * range — the 4 nested helper calls, the 47 direct call arguments, the 265
 * emitted declarations in the committed corpus — a fabricated segment at
 * the column immediately before it is ACCEPTED, and a consumer resolving
 * the range's start column inherits it. That is the price of not demanding
 * a boundary segment inside expressions no compiler terminates, and it is
 * the bound, not an oversight: the enclosing expression at those columns is
 * itself legitimately mappable.
 *
 * @param {string} code the generated module under validation
 * @param {{ emittedIdentifier: RegExp[], runtimeModules: string[],
 *   contextRoots: string[] }} profile the framework vocabulary
 * @param {string[]} [authoredLines] the authored fixture's line table; its
 *   script-block words are subtracted from the claimable set. Omitted only
 *   by callers that have no authored side (unit probes over synthetic
 *   modules), in which case nothing is subtracted.
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
  // Whether a range STARTS ITS OWN generated line — nothing but whitespace
  // precedes it. That is exactly the condition under which the boundary
  // requirement is meaningful; see the `boundary` note above.
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
      // Only the CALLEE is claimed. A helper call's ARGUMENTS legitimately
      // carry authored payload — `$.delegate(['click'])`'s literal comes
      // from an authored `onclick=` — so claiming the whole statement would
      // reject correct maps.
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
      // A claimable identifier handed DIRECTLY to a statement-level call is
      // the compiler passing one of its own bindings to a helper
      // (`Object.defineProperty(__returned__, …)`), whatever the callee is.
      // Only direct arguments qualify: a claimable name nested inside an
      // argument EXPRESSION is a reference the pinned compilers really do
      // map (`_createElementVNode(…, [_hoisted_1, …])` inside a render
      // return), and claiming those rejects correct maps.
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

  // Object-PATTERN properties bind their value; object-LITERAL properties do
  // not. The two share a node type, so the patterns are collected first.
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
      // An object-LITERAL key is introduced by the statement that writes it
      // (`__name:`). An object-PATTERN key is not: it names a property of
      // the SOURCE object, which is authored material the compiler carried
      // through — `v-for="{ _sourceKey: authored } in items"` lowers to
      // `({ _sourceKey: authored }) => …`, where `authored` is the bound
      // name and `_sourceKey` carries the authored token verbatim. Only the
      // pattern's VALUE (the arm above) binds.
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
 * Resolves a map source spelling to an absolute path under each declared
 * base. Separators are normalized to the host's before any platform-aware
 * path operation: a map produced on Windows may spell its sources with `\`,
 * and mixing `path.posix.join` with the platform-aware `path.isAbsolute` /
 * `path.resolve` would leave such a spelling unresolvable.
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
 * Line-scoped source-map resolution: the segment a consumer would apply at
 * (line, column) is the last segment on THAT line at or before the column.
 * A line with no earlier segment is unmapped — which is what makes the
 * boundary requirement below meaningful.
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
 * Validates a candidate's map against the candidate's own generated code and
 * the authored fixture.
 *
 * The generated-only ranges requirement 6 enforces are DERIVED here, from
 * the `code` under validation (`generatedOnlyRanges`) — never supplied by
 * the caller. No call site can therefore disable that requirement by
 * passing an empty range list, which is exactly how it came to be inert on
 * the acceptance path. `extraSyntheticRanges` only ADDS ranges a caller
 * knows about from geometry the module text does not expose; it can never
 * subtract.
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

  // ---- requirement 1: contract + bounds -----------------------------------
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

  // Derived from the code under validation, so requirement 6 below runs on
  // EVERY artifact this oracle sees. Unparseable generated code is a
  // candidate defect the parse axis reports independently; here it means the
  // rail cannot be derived, which is recorded rather than passed over.
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

  // ---- requirement 2: source identity -------------------------------------
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

  // ---- requirement 3: per-segment truthfulness -----------------------------
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
    // `names[nameIdx]` is a CLAIM about the symbol the segment carries, and
    // a bounds check does not test it. A named segment must name the
    // AUTHORED symbol at its own original position, or the generated symbol
    // at its own generated position (with any `_<digits>` disambiguator
    // stripped) — the two readings real producers use. Anything else is a
    // name the map invented; garbage entries and a wholesale-rewritten
    // `names` array are rejected here rather than silently carried.
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

  // ---- requirement 4: required anchors, both directions --------------------
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
    // The two directions are checked INDEPENDENTLY, so each reports on its
    // own: generated -> source (a segment lands on the anchor's exact start)
    // and source -> generated (the anchor's authored span has any generated
    // counterpart at all).
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

  // ---- requirement 6: synthetic ranges carry no authored provenance --------
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
    // Only a range that STARTS ITS OWN generated line carries the
    // no-inherited-provenance requirement; a range beginning mid-line sits
    // inside a legitimately mapped statement, where inheriting the enclosing
    // expression's provenance is correct. See generatedOnlyRanges.
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
