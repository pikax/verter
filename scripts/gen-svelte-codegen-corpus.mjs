#!/usr/bin/env node
/**
 * The CLIENT-CODEGEN PARITY corpus generator (Block 5a's slice of the cumulative
 * 5a–5m codegen corpus).
 *
 * Verter compiles a Svelte SFC to byte-faithful `svelte/internal/client` JS. The
 * parity bar is FINITE BYTE-PARITY: for every shape in the declared codegen surface,
 * Verter's emitted module must equal the OFFICIAL pinned-`svelte@5.56.3` module
 * byte-for-byte (modulo the cosmetic whitespace the shared normalizer collapses).
 * Hand-auditing the per-edge byte tail "failed to create a stopping rule" (the
 * codegen architect's ruling), so this generator mechanically ENUMERATES Block 5a's
 * codegen surface across three orthogonal axes and pins the official output of every
 * cell as the golden:
 *
 *   - value-expression SHAPE  — the attr/class/style VALUE expression:
 *       literal, binary, template (mixed), member, call_pure, call_impure,
 *       optional_call, optional_member, conditional, logical_and / logical_or /
 *       logical_nullish, sequence, object, array, call_arg_spread, array_spread,
 *       object_spread, new, tagged_template.
 *   - TARGET                  — where the value lands: generic attr (`id={…}`),
 *       boolean DOM property (`disabled={…}`), `class={…}`, a `class:foo={…}`
 *       directive, `style={…}`, a `style:prop={…}` directive (+ `|important` +
 *       `--custom`).
 *   - REACTIVITY              — the subject binding's reactive kind:
 *       state (a reassigned `$state` signal), props (a `$props()` read), demoted
 *       (a never-reassigned `$state`, non-reactive), pure (no binding — a literal /
 *       global).
 *
 * The pinned compiler decides each cell's OUTPUT at generation time; the committed
 * golden is the OFFICIAL normalized module (the `clientModule` field) plus the
 * helper-topology fields. The Rust gate (`svelte_codegen_corpus_matrix.rs`)
 * recompiles each fixture with Verter, normalizes its emitted module the SAME way,
 * and asserts byte-equality — the argument/offset/identifier-precise oracle. A Rust
 * COVERAGE gate enforces that every required value-shape / target / reactivity axis
 * contributes at least one committed row, so a dropped enumerator fails HARD (the
 * corpus cannot silently lose a finite axis to a generator gap).
 *
 * SCOPE — Block 5a is dynamic ATTRIBUTES + class/style. Spread ATTRIBUTES
 * (`<div {...rest}>`) and `{@html}` are Block 5b and are NOT in this corpus; but a
 * value-expression `call_arg_spread` / `array_spread` / `object_spread` / `object` /
 * `array` INSIDE a value IS in 5a (the architect's explicit refinement). The two
 * formerly-open in-contract divergences (clsx-on-binary, spread-call memoization)
 * are covered by the `binary`-shaped `class` rows and the `*_spread`-shaped rows
 * respectively.
 *
 * The corpus is CUMULATIVE / extensible: 5b–5m add value-shape / target / reactivity
 * rows (or whole new axes) to the same generator + gate.
 *
 * Sibling of `gen-svelte-goldens.mjs` (the hand-vendored corpus) and
 * `gen-svelte-diff-corpus.mjs` (the differential corpus); reuses the SHARED
 * `loadPinnedCompiler` + topology extractors from `svelte-golden-lib.mjs` (the single
 * oracle pin). Writes a NEW `codegen/` subtree (segregated from the other corpora).
 *
 * USAGE
 *   node scripts/gen-svelte-codegen-corpus.mjs           # rewrite the corpus
 *   node scripts/gen-svelte-codegen-corpus.mjs --check   # assert in sync (CI gate)
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import {
  extractDelegatedEvents,
  extractExportDefault,
  extractImports,
  extractScopeHash,
  extractTemplates,
  helperCountsOf,
  helperSequenceOf,
  loadPinnedCompiler,
  maskScopeHash,
  normalizeModuleForComparison,
  SVELTE_ORACLE_VERSION,
} from "./svelte-golden-lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const CORPUS_ROOT = join(REPO_ROOT, "crates/verter_compiler/tests/svelte_oracle_corpus");
// The codegen corpus is its OWN subtree (segregated from the hand-vendored corpus,
// the reject corpus, the parse-parity corpus, and the generated differential corpus):
// a `<name>.svelte` fixture + a `<name>.client.json` golden carrying the official
// normalized module + topology + the `axis` tags.
const CODEGEN_DIR = join(CORPUS_ROOT, "codegen");

// ---------------------------------------------------------------------------
// Reactivity axis — the subject binding's reactive kind.
//
// Each reactivity kind supplies the instance-script preamble + the SUBJECT token a
// value-expression operates on. Verter's §5a instance-script surface is narrow
// (it refuses a plain `let`, an `import`, a local `function`, a `const`, a
// `$derived`, and a non-primitive `$state(...)` init), so the only subject forms are
// a primitive `$state`, a `$props()` read, and a demoted (never-reassigned) `$state`:
//
//   - state   : `let n = $state(0)` REASSIGNED in the element's onclick → a true
//               reactive signal read as `$.get(n)`. Needs the onclick (else it
//               demotes); the onclick lands in the golden.
//   - props   : `let { p } = $props()` → a reactive `$$props.p` read.
//   - demoted : `let d = $state(0)` never reassigned → a plain non-reactive `let d`.
//   - pure    : the value references NO binding (a literal / global). Verter's §5a
//               surface is RUNES-ONLY (it refuses a legacy/no-rune component), so a
//               pure cell still declares an UNUSED `$state` marker (`let __rune =
//               $state(0)`, demoted to `let __rune = 0` since never read/written) to
//               force runes mode — without it official emits a legacy
//               `import 'svelte/internal/flags/legacy'` module Verter does not
//               support. The marker is byte-identical noise in every pure golden.
//
// `kind` is the reactivity-axis tag; `subject` is the token the value-shape consumes
// (`null` for `pure` — a pure value-shape supplies its own literal/global).
// ---------------------------------------------------------------------------

const REACTIVITIES = [
  { kind: "state", subject: "n" },
  { kind: "props", subject: "p" },
  { kind: "demoted", subject: "d" },
  { kind: "pure", subject: null },
];

// The instance-script preamble + the element's `onclick` (only `state` needs the
// reassignment so the `$state` stays a live signal). For `props` the destructure
// names the subject; `demoted` declares a never-reassigned `$state`; `pure` declares
// an UNUSED `$state` runes marker (the value references no binding, but the component
// must be runes mode).
function scriptFor(reactivity) {
  switch (reactivity.kind) {
    case "state":
      return { script: "let n = $state(0);", onclick: "onclick={() => n++}" };
    case "props":
      return { script: "let { p } = $props();", onclick: null };
    case "demoted":
      return { script: "let d = $state(0);", onclick: null };
    case "pure":
      return { script: "let __rune = $state(0);", onclick: null };
    default:
      throw new Error(`unknown reactivity ${reactivity.kind}`);
  }
}

// ---------------------------------------------------------------------------
// Value-expression SHAPE axis.
//
// Each shape is a function `(sub) => exprText` building the VALUE expression from the
// reactivity subject `sub` (the `state`/`props`/`demoted` binding name) — or, for the
// `pure` reactivity, from a literal/global standin. A shape declares the reactivities
// it supports (`reactivities`) and the targets it is meaningful on (`targets`); the
// generator emits the cartesian product of (supported reactivity × supported target).
//
// `kind` is the value-shape-axis tag. The COVERAGE gate requires every shape here to
// contribute ≥1 committed row.
//
// Notes on the narrow §5a surface:
//   - an IMPURE call cannot use a local function / import (refused), so it is a METHOD
//     call on the subject binding (`n.toFixed(2)`); for `pure` it is a global member
//     call WITHOUT a binding (`Math.max(1, 2)`), which official treats as pure-callee +
//     no-deps → NOT memoized (a genuine inline cell).
//   - a `pure` literal subject is a string literal; a `pure` binary is two literals;
//     a `pure` call_arg_spread spreads a GLOBAL (`globalThis.things`).
// ---------------------------------------------------------------------------

// A `pure` standin per shape-family: the literal/global a no-binding cell uses.
// Building the value-expr for the `pure` reactivity passes this instead of a binding.
function pureSubjectFor(shapeKind) {
  // For spread shapes the pure standin must be ITERABLE/spreadable → a global; for
  // scalar shapes a string literal is the simplest pure value.
  switch (shapeKind) {
    case "call_arg_spread":
    case "array_spread":
    case "object_spread":
      return "globalThis.things";
    case "member":
    case "optional_member":
      return "globalThis";
    case "optional_call":
    case "call_impure":
      // A pure optional/impure-shaped call roots at a global so it stays pure-callee.
      return "globalThis";
    default:
      return "'x'";
  }
}

const SHAPES = [
  // A bare literal value (no binding) — only the `pure` reactivity is meaningful.
  {
    kind: "literal",
    reactivities: ["pure"],
    targets: ["attr", "boolean", "class", "style"],
    build: () => `'lit'`,
  },
  // A binary string-concatenation — the clsx-on-binary divergence lives on the
  // `class` target (official emits the value RAW, NOT `$.clsx`-wrapped).
  {
    kind: "binary",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class", "style", "style_directive"],
    build: (sub, react) => (react.kind === "pure" ? `'a' + 'b'` : `${sub} + '!'`),
  },
  // A mixed string template (`a {x} b`) — lowered to a TemplateLiteral; the per-part
  // memoize + the non-clsx class path.
  {
    kind: "template",
    reactivities: ["state", "props", "demoted"],
    targets: ["attr", "class", "style"],
    mixed: true, // a quoted mixed value `"a {sub} b"`, not a brace expression
    build: (sub) => `a {${sub}} b`,
  },
  // A member access on the subject (`sub.x`) — reactive via the object root.
  {
    kind: "member",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class", "class_directive", "style", "style_directive"],
    build: (sub) => `${sub}.x`,
  },
  // A PURE-callee call wrapping the subject (`String(sub)`) — the global callee is
  // pure; reactive when `sub` is a binding (deps > 0 → memoized), inline for `pure`.
  {
    kind: "call_pure",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class", "style", "style_directive"],
    build: (sub, react) => (react.kind === "pure" ? `String('x')` : `String(${sub})`),
  },
  // An IMPURE-callee call — a method on the subject (`sub.toString()`); for `pure` a
  // global member call (`Math.max(1, 2)`), still pure-callee. A method call on a prop
  // (`{p.toString()}`) is a READ of the receiver, not a write (Verter — like official —
  // never classifies a `CallExpression` as a prop write), so `props` is included.
  {
    kind: "call_impure",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class", "style"],
    build: (sub, react) => (react.kind === "pure" ? `Math.max(1, 2)` : `${sub}.toString()`),
  },
  // An OPTIONAL call (`sub?.toString?.()`); for `pure` a global optional call.
  {
    kind: "optional_call",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class"],
    build: (sub, react) => (react.kind === "pure" ? `globalThis?.x?.()` : `${sub}?.toString?.()`),
  },
  // An OPTIONAL member (`sub?.x`) — NOT a call (must not memoize on has_call alone).
  {
    kind: "optional_member",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class"],
    build: (sub, react) => (react.kind === "pure" ? `globalThis?.x` : `${sub}?.x`),
  },
  // A conditional (`sub ? 'a' : 'b'`).
  {
    kind: "conditional",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class", "class_directive", "style", "style_directive"],
    build: (sub, react) => (react.kind === "pure" ? `true ? 'a' : 'b'` : `${sub} ? 'a' : 'b'`),
  },
  // Logical `&&` / `||` / `??`.
  {
    kind: "logical_and",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class", "class_directive"],
    build: (sub, react) => (react.kind === "pure" ? `true && 'a'` : `${sub} && 'a'`),
  },
  {
    kind: "logical_or",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "boolean", "class"],
    build: (sub, react) => (react.kind === "pure" ? `false || 'a'` : `${sub} || 'a'`),
  },
  {
    kind: "logical_nullish",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "class", "style"],
    build: (sub, react) => (react.kind === "pure" ? `null ?? 'a'` : `${sub} ?? 'a'`),
  },
  // A sequence (`(sub, 'a')`).
  {
    kind: "sequence",
    reactivities: ["state", "demoted", "pure"],
    targets: ["attr", "class"],
    build: (sub, react) => (react.kind === "pure" ? `(0, 'a')` : `(${sub}, 'a')`),
  },
  // An object literal (`{ a: sub }`) — meaningful on `class` (clsx) + generic attr.
  {
    kind: "object",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "class"],
    build: (sub, react) => (react.kind === "pure" ? `{ a: 1 }` : `{ a: ${sub} }`),
  },
  // An array literal (`[sub, 'b']`) — meaningful on `class` (clsx) + generic attr.
  {
    kind: "array",
    reactivities: ["state", "props", "demoted", "pure"],
    targets: ["attr", "class"],
    build: (sub, react) => (react.kind === "pure" ? `[1, 'b']` : `[${sub}, 'b']`),
  },
  // A CALL with a SPREAD argument (`String(...sub)`) — the spread-call memoization
  // divergence. For `pure` the spread is of a GLOBAL iterable (still memoized).
  {
    kind: "call_arg_spread",
    reactivities: ["props", "pure"],
    targets: ["attr", "boolean", "class"],
    build: (sub, react) =>
      react.kind === "pure" ? `String(...globalThis.things)` : `String(...${sub})`,
  },
  // An ARRAY SPREAD value (`[...sub]`) — official sets has_call unconditionally.
  {
    kind: "array_spread",
    reactivities: ["props", "pure"],
    targets: ["attr", "class"],
    build: (sub, react) => (react.kind === "pure" ? `[...globalThis.things]` : `[...${sub}]`),
  },
  // An OBJECT SPREAD value (`{ ...sub }`) — official sets has_call unconditionally.
  // Written as the bare object form (the Svelte `class={{ … }}` delimiter pair, like
  // the non-spread `object` shape) — NOT author-parenthesized `({ … })`, which would
  // ride through as a redundant `ParenthesizedExpression` the official serializer
  // drops but Verter's source-faithful rewrite preserves.
  {
    kind: "object_spread",
    reactivities: ["props", "pure"],
    targets: ["attr", "class"],
    build: (sub, react) => (react.kind === "pure" ? `{ ...globalThis.things }` : `{ ...${sub} }`),
  },
  // A `new X()` expression — official `NewExpression` sets only needs_context, NOT
  // has_call; a bare `new GlobalCtor()` stays an inline init.
  {
    kind: "new",
    reactivities: ["state", "demoted", "pure"],
    targets: ["attr"],
    build: (sub, react) => (react.kind === "pure" ? `new Date()` : `new Date(${sub})`),
  },
  // A tagged template — pure-global tag (`String.raw`) is NOT has_call; the `${…}`
  // interpolation carries the subject's reactivity.
  {
    kind: "tagged_template",
    reactivities: ["state", "demoted", "pure"],
    targets: ["attr"],
    build: (sub, react) =>
      react.kind === "pure" ? "String.raw`abc`" : `String.raw\`x${"${" + sub + "}"}y\``,
  },
];

// ---------------------------------------------------------------------------
// Target axis — where the value lands in the markup.
//
// Each target builds the ATTRIBUTE/DIRECTIVE markup fragment from a value-expr. The
// `mixed` flag (a `template`-shape value) is a QUOTED mixed value (`"a {x} b"`),
// otherwise a brace expression (`{expr}`). `class`/`style` targets put the value as
// the base attribute; the directive targets use a `class:`/`style:` directive name.
// ---------------------------------------------------------------------------

const TARGETS = [
  {
    axis: "attr",
    // A generic dynamic attribute on a `<div>` (`id={…}`).
    element: "div",
    attr: (expr, mixed) => (mixed ? `id="${expr}"` : `id={${expr}}`),
  },
  {
    axis: "boolean",
    // A boolean DOM property on an `<input>` (`disabled={…}`).
    element: "input",
    selfClose: true,
    attr: (expr, mixed) => (mixed ? `disabled="${expr}"` : `disabled={${expr}}`),
  },
  {
    axis: "class",
    element: "div",
    attr: (expr, mixed) => (mixed ? `class="${expr}"` : `class={${expr}}`),
  },
  {
    axis: "class_directive",
    element: "div",
    // A `class:on={…}` directive (the value is a boolean-ish expr).
    attr: (expr) => `class:on={${expr}}`,
  },
  {
    axis: "style",
    element: "div",
    attr: (expr, mixed) => (mixed ? `style="${expr}"` : `style={${expr}}`),
  },
  {
    axis: "style_directive",
    element: "div",
    // A `style:color={…}` directive.
    attr: (expr) => `style:color={${expr}}`,
  },
];

// Map a shape's declared `targets` token to the TARGETS row. The shape token
// `style_directive` selects the `style:color` directive; `class`/`style`/`attr`/
// `boolean` select the same-named base target. (A shape that lists `class` also
// implicitly never targets the directives unless it lists them — directives carry a
// narrower value vocabulary.)
const TARGET_BY_AXIS = new Map(TARGETS.map((t) => [t.axis, t]));

// ---------------------------------------------------------------------------
// Content sub-axis — the INNER value-shape a CONTAINER's hole holds.
//
// The flat `SHAPES` axis above varies the WHOLE value expression. A CONTAINER
// shape (a mixed-template chunk, a conditional branch, a logical operand, a call
// argument) has a HOLE whose contents independently drive official's fold /
// memoize / dependency decisions: `id="a {d + 1} b"` (binary content) folds over a
// demoted `$state` while `id="a {d.x} b"` (member content) stays live, and
// `String(d.toString())` (call-arg content) is `has_call` while `String(d + 1)`
// is not. A container×content cell that is MISSING is a generator bug (the
// doctrine), so the second generation pass MECHANICALLY crosses every container
// with every content shape it declares — the official output of each is the pinned
// golden.
//
// Each content shape is `(sub) => exprText` building the inner expression from the
// reactivity subject. `subjectKnown` marks the content shapes whose value is
// STATICALLY KNOWN over a demoted `$state` (the const-fold class FIX #6 closes);
// the rest stay live. The generator does not branch on `subjectKnown` — it is a
// documentation tag the golden already encodes — but it names the divergence axis
// the corpus exists to pin.
// ---------------------------------------------------------------------------

// `nullishMix` marks a content shape whose top-level operator is `??` — illegal to
// place UNPARENTHESIZED as an operand of `&&` / `||` (JS forbids mixing `??` with
// `&&`/`||` without parens). The `log` container parenthesizes exactly these (and
// official emits the same NECESSARY parens). `seqParens` marks a content shape that
// is ALWAYS parenthesized in its build (a bare sequence is ambiguous in most
// positions); those parens are necessary everywhere, so official keeps them too.
const CONTENT_SHAPES = [
  { kind: "identifier", subjectKnown: true, build: (sub) => `${sub}` },
  { kind: "binary", subjectKnown: true, build: (sub) => `${sub} + 1` },
  { kind: "logical_and", subjectKnown: true, build: (sub) => `${sub} && 1` },
  { kind: "logical_or", subjectKnown: true, build: (sub) => `${sub} || 1` },
  {
    kind: "logical_nullish",
    subjectKnown: true,
    nullishMix: true,
    build: (sub) => `${sub} ?? 1`,
  },
  { kind: "conditional", subjectKnown: true, build: (sub) => `${sub} ? 1 : 2` },
  { kind: "unary", subjectKnown: true, build: (sub) => `-${sub}` },
  // The two content shapes official's `Evaluation` does NOT statically know (so they
  // stay live even over a demoted `$state`) — the negative half of the fold class.
  // (A sequence carries its own necessary parens.)
  { kind: "sequence", subjectKnown: false, seqParens: true, build: (sub) => `(${sub}, 1)` },
  { kind: "member", subjectKnown: false, build: (sub) => `${sub}.x` },
  { kind: "call", subjectKnown: false, build: (sub) => `${sub}.toString()` },
];

// ---------------------------------------------------------------------------
// Container axis — a value-expression form with a content HOLE.
//
// `embed(content, sub)` builds the value-expression (or, for a `mixed` container,
// the quoted-attribute BODY) from a rendered content expression. `mixed` marks the
// quoted-template form (`"a {content} b"`) routed through the multi-chunk
// evaluate-fold; the rest are brace expressions (`{expr}`) routed through the
// single-expression path. `reactivities` are the subject kinds crossed (the
// container fold/live split is exercised over `demoted` AND `state`); `targets` are
// the markup positions. A `multi` flag adds the official multi-interpolation
// `"a {content} b {content} c"` body (two holes) for the mixed container.
// ---------------------------------------------------------------------------

// `embed(content, sub)` receives the content SHAPE object (so it can inspect
// `nullishMix` to add the minimal NECESSARY parens) and the subject token, and
// returns the value-expression / quoted body. It must produce source whose parens
// EXACTLY match official's (a REDUNDANT paren diverges — Verter preserves source
// parens faithfully), so a paren is added only where JS requires it.
const CONTAINERS = [
  {
    kind: "tmpl",
    mixed: true,
    multi: true,
    reactivities: ["demoted", "state"],
    // A mixed quoted value lands on a generic attr (`id="a {…} b"`), a boolean DOM
    // property (`disabled="a {…} b"` → an always-truthy STRING property assignment that
    // folds the same as `id`), and `class`/`style`. The `boolean` target exercises the
    // boolean-property × mixed-template × content fold axis (`input.disabled = 'a 6 b'`).
    targets: ["attr", "boolean", "class", "style"],
    // The quoted mixed-template body with the content in the single `{…}` hole — every
    // content shape is a legal standalone interpolation, so no extra parens.
    embed: (content, sub) => `a {${content.build(sub)}} b`,
    // The multi-interpolation body (two holes) — official folds each known chunk
    // independently and keeps the live chunks as `${… ?? ''}` parts.
    embedMulti: (content, sub) => `a {${content.build(sub)}} b {${content.build(sub)}} c`,
  },
  {
    kind: "cond",
    reactivities: ["demoted", "state"],
    targets: ["attr"],
    // The content is the CONSEQUENT branch; a ternary branch is delimited, so any
    // content (the sequence carries its own parens) is legal unparenthesized.
    embed: (content, sub) => `${sub} > 0 ? ${content.build(sub)} : 0`,
  },
  {
    kind: "log",
    reactivities: ["demoted", "state"],
    targets: ["attr"],
    // The content is the RIGHT operand of `&&`. A `??`-topped content (`nullishMix`)
    // is illegal as a bare `&&` operand → parenthesize it (official emits the same
    // necessary parens); every other content is legal unparenthesized under `&&`.
    embed: (content, sub) => {
      const inner = content.build(sub);
      return `${sub} && ${content.nullishMix ? `(${inner})` : inner}`;
    },
  },
  {
    kind: "call_arg",
    reactivities: ["demoted", "state"],
    targets: ["attr"],
    // The content is the sole ARGUMENT to a pure-global `String(…)` call — a call
    // argument is delimited, so any content (the sequence carries its own parens) is
    // legal unparenthesized.
    embed: (content, sub) => `String(${content.build(sub)})`,
  },
];

// ---------------------------------------------------------------------------
// Const-fold edge sub-axis — the EXACT-value rows that pin official's
// `scope.evaluate` coercion / globals / operator semantics.
//
// The container×content matrix above varies the SHAPE of a foldable chunk over one
// fixed `$state(0)` subject, so it cannot express the rows whose DIVERGENCE is the
// folded VALUE itself: a BigInt subject, a JS `Number()`/`String()` coercion edge, a
// previously-missing global, or a tricky float. This sub-pass enumerates those as
// fold-edge fixtures — each a tailored instance-script `$state` initializer + a
// foldable attribute value whose official fold is a SPECIFIC string. The generator
// pins the pinned-compiler output of each (the Rust matrix gate byte-compares), and
// the coverage gate requires every fold-edge FAMILY to contribute ≥1 row, so the
// evaluator's coercion / globals / operator semantics cannot silently regress.
//
// Each row: `{ family, name, script, value }` — `script` is the instance preamble
// (a `$state` subject `d` or a runes-marker `__r`), `value` is the BRACE/mixed
// attribute value expression (placed in `id="…"` mixed-template form so it routes
// through the multi-chunk evaluate-fold). `family` is the coverage-axis tag.
// ── BUCKET 1 of 3: `fold-exact` — official folds to a literal AND Verter folds to the
//    SAME byte-exact literal (the ExactFold allow-list). Pinned official output is the
//    golden; the Rust matrix gate byte-compares Verter's fold.
const FOLD_EXACT_EDGES = [
  // BigInt — a concrete value that folds on its own and through arithmetic / typeof
  // (official's `NUMBER` includes BigInt; a literal stores the actual `1n`).
  { family: "bigint", name: "bigint_plain", script: "let d = $state(1n);", value: "d" },
  { family: "bigint", name: "bigint_add", script: "let d = $state(5n);", value: "d + 1n" },
  { family: "bigint", name: "bigint_mul", script: "let d = $state(5n);", value: "d * 2n" },
  { family: "bigint", name: "bigint_typeof", script: "let d = $state(5n);", value: "typeof d" },
  { family: "bigint", name: "bigint_neg", script: "let d = $state(5n);", value: "-d" },
  // A BigInt + String concatenates (`5n + 'x'` → `'5x'`).
  { family: "bigint", name: "bigint_concat", script: "let d = $state(5n);", value: "d + 'x'" },
  // `-` and bitwise `&` are magnitude-growing ops V8 size-guards (the result reaches the
  // digit-allocation boundary only for ~2^30-bit operands). The CHEAP guard refuses ONLY when
  // the result's upper bit-bound EXCEEDS 2^30 (V8 throws iff `result_bits > 2^30`; a provable
  // `<= 2^30` folds), so a normal small operand folds byte-exact here — pinning that the size
  // guard does NOT over-refuse the provable-safe case (the boundary itself is a ~134 MB operand
  // the oracle cannot fold, asserted directly in the cheap predicate's unit test instead).
  { family: "bigint", name: "bigint_sub", script: "let d = $state(9n);", value: "d - 4n" },
  { family: "bigint", name: "bigint_bitand", script: "let d = $state(6n);", value: "d & 3n" },

  // BigInt SHIFTS / EXPONENT — arbitrary-precision arithmetic, folded to the exact JS BigInt
  // value (the result fits far under V8's 2^30-bit limit). A NEGATIVE shift count is VALID
  // (`a << -b` ≡ `a >> b`, `a >> -b` ≡ `a << b`) and must fold the CORRECT value, NOT fall to
  // the Number 32-bit masked-shift path. Probed against pinned svelte.
  { family: "bigint_shift", name: "bigint_shl", script: "let d = $state(1n);", value: "d << 4n" },
  {
    family: "bigint_shift",
    name: "bigint_shr",
    script: "let d = $state(256n);",
    value: "d >> 2n",
  },
  {
    family: "bigint_shift",
    name: "bigint_shl_neg_operand",
    script: "let d = $state(-5n);",
    value: "d << 3n",
  },
  {
    family: "bigint_shift",
    name: "bigint_shl_neg_count",
    script: "let d = $state(256n);",
    value: "d << -2n",
  },
  {
    family: "bigint_shift",
    name: "bigint_shr_neg_count",
    script: "let d = $state(256n);",
    value: "d >> -2n",
  },
  {
    family: "bigint_shift",
    name: "bigint_shl_zero",
    script: "let d = $state(0n);",
    value: "d << 4294967296n",
  },
  {
    family: "bigint_exponent",
    name: "bigint_pow",
    script: "let d = $state(2n);",
    value: "d ** 10n",
  },
  {
    family: "bigint_exponent",
    name: "bigint_pow_zero",
    script: "let d = $state(7n);",
    value: "d ** 0n",
  },

  // JS `Number(string)` coercion via arithmetic (`d - 0`) over a demoted string `$state`
  // — official folds each to the JS `Number()` value, the bug the port fixes.
  {
    family: "number_coerce",
    name: "ncoerce_hex",
    script: "let d = $state('0x10');",
    value: "d - 0",
  },
  {
    family: "number_coerce",
    name: "ncoerce_octal",
    script: "let d = $state('0o17');",
    value: "d - 0",
  },
  {
    family: "number_coerce",
    name: "ncoerce_binary",
    script: "let d = $state('0b101');",
    value: "d - 0",
  },
  { family: "number_coerce", name: "ncoerce_empty", script: "let d = $state('');", value: "d - 0" },
  {
    family: "number_coerce",
    name: "ncoerce_ws",
    script: "let d = $state(' 15 ');",
    value: "d - 0",
  },
  {
    family: "number_coerce",
    name: "ncoerce_infinity",
    script: "let d = $state('Infinity');",
    value: "d - 0",
  },
  {
    family: "number_coerce",
    name: "ncoerce_invalid",
    script: "let d = $state('a 15 b');",
    value: "d - 0",
  },

  // JS `String(x)` coercion via concat (`… + ''`). The `-0` / `true` cases use a demoted
  // primitive-literal `$state` subject; the `Infinity` / `NaN` cases compute the value
  // INLINE over a runes-marker subject (Verter's §5a surface refuses a non-primitive
  // `$state(1 / 0)` init, and `Infinity` / `NaN` are not primitive literals).
  {
    family: "string_coerce",
    name: "scoerce_negzero",
    script: "let d = $state(-0);",
    value: "d + ''",
  },
  {
    family: "string_coerce",
    name: "scoerce_bool",
    script: "let d = $state(true);",
    value: "d + ''",
  },
  {
    family: "string_coerce",
    name: "scoerce_infinity",
    script: "let __r = $state(0);",
    value: "(1 / 0) + ''",
  },
  {
    family: "string_coerce",
    name: "scoerce_neginfinity",
    script: "let __r = $state(0);",
    value: "(-1 / 0) + ''",
  },
  {
    family: "string_coerce",
    name: "scoerce_nan",
    script: "let __r = $state(0);",
    value: "(0 / 0) + ''",
  },

  // The EXACT globals — IEEE-754-mandated / integer / bit-op / decimal-scan / string
  // pure-global calls (a runes-marker subject keeps the component runes mode; the value
  // references no binding). The TRANSCENDENTALS (Math.log / atan2 / pow / cbrt / log2 / …)
  // are NOT here — they live in `LIVE_FALLBACK_EDGES` (Rust libm vs V8 fdlibm is not
  // provably bit-identical cross-platform). `Math.sqrt` IS exact (IEEE-754).
  {
    family: "global_call",
    name: "glob_sqrt",
    script: "let __r = $state(0);",
    value: "Math.sqrt(16)",
  },
  {
    family: "global_call",
    name: "glob_sign",
    script: "let __r = $state(0);",
    value: "Math.sign(-5)",
  },
  {
    family: "global_call",
    name: "glob_clz32",
    script: "let __r = $state(0);",
    value: "Math.clz32(1)",
  },
  {
    family: "global_call",
    name: "glob_imul",
    script: "let __r = $state(0);",
    value: "Math.imul(3, 4)",
  },
  {
    family: "global_call",
    name: "glob_trunc",
    script: "let __r = $state(0);",
    value: "Math.trunc(4.7)",
  },
  {
    family: "global_call",
    name: "glob_numisint",
    script: "let __r = $state(0);",
    value: "Number.isInteger(5)",
  },
  {
    family: "global_call",
    name: "glob_numisnan",
    script: "let __r = $state(0);",
    value: "Number.isNaN(0 / 0)",
  },
  {
    family: "global_call",
    name: "glob_numparseint",
    script: "let __r = $state(0);",
    value: "Number.parseInt('0x1F')",
  },
  {
    family: "global_call",
    name: "glob_numparsefloat",
    script: "let __r = $state(0);",
    value: "Number.parseFloat('3.14xy')",
  },
  {
    family: "global_call",
    name: "glob_fromchar",
    script: "let __r = $state(0);",
    value: "String.fromCharCode(65, 66)",
  },
  {
    family: "global_call",
    name: "glob_fromcodepoint",
    script: "let __r = $state(0);",
    value: "String.fromCodePoint(128512)",
  },

  // Global numeric CONSTANTS (`Math.PI`, …) — a member keypath, not a call.
  { family: "global_const", name: "gconst_pi", script: "let __r = $state(0);", value: "Math.PI" },
  { family: "global_const", name: "gconst_e", script: "let __r = $state(0);", value: "Math.E" },
  {
    family: "global_const",
    name: "gconst_sqrt2",
    script: "let __r = $state(0);",
    value: "Math.SQRT2",
  },

  // Tricky float values — the full-precision / Infinity / NaN spellings. (`2 ** 53` is NOT
  // here — the `**` operator is the same fdlibm `pow` as `Math.pow`, so it live-falls-back.)
  {
    family: "tricky_number",
    name: "tricky_eps",
    script: "let __r = $state(0);",
    value: "0.1 + 0.2",
  },
  { family: "tricky_number", name: "tricky_div0", script: "let __r = $state(0);", value: "1 / 0" },
  {
    family: "tricky_number",
    name: "tricky_negdiv0",
    script: "let __r = $state(0);",
    value: "-1 / 0",
  },
  { family: "tricky_number", name: "tricky_nan", script: "let __r = $state(0);", value: "0 / 0" },
  {
    family: "tricky_number",
    name: "tricky_shift",
    script: "let __r = $state(0);",
    value: "-1 >>> 0",
  },
];

// ── BUCKET 2 of 3: `refuse` — the Svelte `Evaluation` evaluates a native JS op that
//    THROWS at compile time, so OFFICIAL COMPILE-FAILS the component. Verter must REFUSE
//    deterministically (NEVER live code — that would turn the compile-failure into a
//    runtime crash). The generator confirms the pinned compiler REJECTS each (no golden
//    JS); the Rust gate asserts Verter refuses with the `const-fold-throw` diagnostic. The
//    EAGERNESS rows (`false && (1n / 0n)`, `true ? 1 : (1n / 0n)`) prove official evaluates
//    the non-selected operand/branch (NOT runtime short-circuit) → throws → refuse.
//
// Each row: `{ family, name, script, value, reason }` — `reason` is the expected
// `ConstFoldRefuse` label (documentation; the Rust gate pins the diagnostic CODE).
const REFUSE_EDGES = [
  // Mixing BigInt with a Number in arithmetic / bitwise → TypeError.
  {
    family: "refuse_bigint_mixed",
    name: "refuse_mix_add",
    script: "let d = $state(2n);",
    value: "d + 1",
    reason: "BigInt mixed with a Number in arithmetic / bitwise",
  },
  {
    family: "refuse_bigint_mixed",
    name: "refuse_mix_bitand",
    script: "let d = $state(2n);",
    value: "d & 1",
    reason: "BigInt mixed with a Number in arithmetic / bitwise",
  },
  // BigInt division / remainder by zero → RangeError.
  {
    family: "refuse_bigint_throw",
    name: "refuse_div0n",
    script: "let d = $state(6n);",
    value: "d / 0n",
    reason: "BigInt division / remainder by zero",
  },
  // BigInt unsigned right shift `>>>` → TypeError.
  {
    family: "refuse_bigint_throw",
    name: "refuse_ushr",
    script: "let d = $state(6n);",
    value: "d >>> 0n",
    reason: "BigInt unsigned right shift `>>>`",
  },
  // Unary `+` on a BigInt → TypeError.
  {
    family: "refuse_bigint_throw",
    name: "refuse_unary_plus",
    script: "let d = $state(6n);",
    value: "+d",
    reason: "unary `+` on a BigInt",
  },
  // A negative BigInt exponent → RangeError ("Exponent must be positive").
  {
    family: "refuse_bigint_throw",
    name: "refuse_neg_exponent",
    script: "let d = $state(2n);",
    value: "d ** -1n",
    reason: "BigInt exponentiation with a negative exponent",
  },
  // A BigInt `<<` / `**` whose RESULT exceeds V8's `kMaxLengthBits` (2^30 bits) → RangeError
  // ("Maximum BigInt size exceeded"). Official compile-FAILS; Verter refuses via the CHEAP
  // bit-length size guard WITHOUT attempting the multi-gigabit allocation (no hang). Probed:
  // pinned svelte rejects `1n << 4294967296n` and `2n ** 4294967296n` in <30 ms.
  {
    family: "refuse_bigint_size",
    name: "refuse_shl_oversize",
    script: "let d = $state(1n);",
    value: "d << 4294967296n",
    reason: "BigInt `<<` / `**` result exceeds the maximum size",
  },
  {
    family: "refuse_bigint_size",
    name: "refuse_pow_oversize",
    script: "let d = $state(2n);",
    value: "d ** 4294967296n",
    reason: "BigInt `<<` / `**` result exceeds the maximum size",
  },
  // A negative right-shift whose effective LEFT shift overflows also refuses
  // (`1n >> -4294967296n` ≡ `1n << 4294967296n` → exceeds).
  {
    family: "refuse_bigint_size",
    name: "refuse_shr_neg_oversize",
    script: "let d = $state(1n);",
    value: "d >> -4294967296n",
    reason: "BigInt `<<` / `**` result exceeds the maximum size",
  },
  // `in` with a primitive RHS → TypeError.
  {
    family: "refuse_in_instanceof",
    name: "refuse_in_string",
    script: "let __r = $state(0);",
    value: "'x' in 'abc'",
    reason: "`in` operator with a primitive right-hand side",
  },
  // A throwing global under a known arg (`Math.clz32(1n)` — BigInt arg → TypeError).
  {
    family: "refuse_global_throw",
    name: "refuse_clz32_bigint",
    script: "let __r = $state(0);",
    value: "Math.clz32(1n)",
    reason: "a foldable global throwing under known arguments",
  },
  // An invalid `String.fromCodePoint` code point → RangeError.
  {
    family: "refuse_global_throw",
    name: "refuse_fromcodepoint_neg",
    script: "let __r = $state(0);",
    value: "String.fromCodePoint(-1)",
    reason: "a foldable global throwing under known arguments",
  },
  // EAGERNESS — official evaluates BOTH logical operands / BOTH conditional branches
  // before selecting, so a throw in the NON-selected position STILL compile-fails.
  {
    family: "refuse_eager",
    name: "refuse_eager_and",
    script: "let __r = $state(0);",
    value: "false && (1n / 0n)",
    reason: "BigInt division / remainder by zero (non-selected && operand)",
  },
  {
    family: "refuse_eager",
    name: "refuse_eager_ternary",
    script: "let __r = $state(0);",
    value: "true ? 1 : (1n / 0n)",
    reason: "BigInt division / remainder by zero (non-selected ternary alternate)",
  },
];

// ── BUCKET 3 of 3: `live-fallback` — official folds to a literal, but Verter cannot prove
//    byte-exact emission (a transcendental libm result, a huge-finite ToInt32, a parseInt
//    radix/whitespace gap, a lone surrogate), so Verter emits the LIVE expression. Official
//    compiles (the golden records its FOLDED literal); the Rust gate asserts Verter's
//    output is the LIVE form (NOT byte-equal to official's literal). Each row documents
//    official's fold + the ledger `reason`.
//
// Each row: `{ family, name, script, value, reason }` — `reason` is the checked-in
// `LiveFallbackReason` LABEL (the Rust ledger gate cross-checks the corpus against the
// `live_fallback_ledger()` rows).
const LIVE_FALLBACK_EDGES = [
  // Transcendental Math.* — Rust system libm vs V8 fdlibm not provably bit-identical.
  {
    family: "live_transcendental",
    name: "live_log",
    script: "let __r = $state(0);",
    value: "Math.log(10)",
    reason: "transcendental-libm",
  },
  {
    family: "live_transcendental",
    name: "live_atan2",
    script: "let __r = $state(0);",
    value: "Math.atan2(1, 1)",
    reason: "transcendental-libm",
  },
  {
    family: "live_transcendental",
    name: "live_pow",
    script: "let __r = $state(0);",
    value: "Math.pow(2, 10)",
    reason: "transcendental-libm",
  },
  {
    family: "live_transcendental",
    name: "live_cbrt",
    script: "let __r = $state(0);",
    value: "Math.cbrt(27)",
    reason: "transcendental-libm",
  },
  {
    family: "live_transcendental",
    name: "live_log2",
    script: "let __r = $state(0);",
    value: "Math.log2(8)",
    reason: "transcendental-libm",
  },
  // The `**` operator over Numbers is the same fdlibm `pow`.
  {
    family: "live_transcendental",
    name: "live_pow53",
    script: "let __r = $state(0);",
    value: "2 ** 53",
    reason: "transcendental-libm",
  },
  // BigInt-vs-Number COERCING comparison — f64 coercion loses precision past 2^53.
  {
    family: "live_bigint_precision",
    name: "live_bigint_eq",
    script: "let d = $state(9007199254740993n);",
    value: "d == 9007199254740992",
    reason: "bigint-number-precision-compare",
  },
  {
    family: "live_bigint_precision",
    name: "live_bigint_gt",
    script: "let d = $state(9007199254740993n);",
    value: "d > 9007199254740992",
    reason: "bigint-number-precision-compare",
  },
  // A huge-finite ToInt32 / ToUint32 (modulo-2^32 JS semantics Verter's cast misses).
  {
    family: "live_large_to_int32",
    name: "live_clz32_1e20",
    script: "let __r = $state(0);",
    value: "Math.clz32(1e20)",
    reason: "large-to-int32",
  },
  {
    family: "live_large_to_int32",
    name: "live_or_1e20",
    script: "let __r = $state(0);",
    value: "1e20 | 0",
    reason: "large-to-int32",
  },
  // A `parseInt` radix needing JS ToInt32 / a non-ASCII JS-whitespace prefix.
  {
    family: "live_parseint",
    name: "live_parseint_radix",
    script: "let __r = $state(0);",
    value: "Number.parseInt('10', 4294967298)",
    reason: "parseint-radix-or-whitespace",
  },
  {
    family: "live_parseint",
    name: "live_parsefloat_nbsp",
    script: "let __r = $state(0);",
    value: "Number.parseFloat('\\u00A03.5x')",
    reason: "parseint-radix-or-whitespace",
  },
  // A lone surrogate (UTF-16) Verter's UTF-8 value model cannot byte-exactly represent.
  {
    family: "live_lone_surrogate",
    name: "live_fromcharcode_surrogate",
    script: "let __r = $state(0);",
    value: "String.fromCharCode(55296)",
    reason: "lone-surrogate",
  },
  {
    family: "live_lone_surrogate",
    name: "live_fromcodepoint_surrogate",
    script: "let __r = $state(0);",
    value: "String.fromCodePoint(55296)",
    reason: "lone-surrogate",
  },
];

// The REQUIRED axes the coverage gate enforces: every value-shape kind, every target
// axis, every reactivity kind, every content sub-shape, every container family, and
// every const-fold edge family must contribute ≥1 committed row.
const REQUIRED_SHAPE_AXES = SHAPES.map((s) => s.kind);
const REQUIRED_TARGET_AXES = TARGETS.map((t) => t.axis);
const REQUIRED_REACTIVITY_AXES = REACTIVITIES.map((r) => r.kind);
const REQUIRED_CONTENT_AXES = CONTENT_SHAPES.map((c) => c.kind);
const REQUIRED_CONTAINER_AXES = CONTAINERS.map((c) => c.kind);
// The three const-fold BUCKETS' families (the coverage gate requires ≥1 row per family in
// each bucket, so a dropped fold-edge / refuse / live-fallback family fails generation).
const REQUIRED_FOLD_EXACT_FAMILIES = [...new Set(FOLD_EXACT_EDGES.map((e) => e.family))];
const REQUIRED_REFUSE_FAMILIES = [...new Set(REFUSE_EDGES.map((e) => e.family))];
const REQUIRED_LIVE_FALLBACK_FAMILIES = [...new Set(LIVE_FALLBACK_EDGES.map((e) => e.family))];

// ---------------------------------------------------------------------------
// Fixture assembly
// ---------------------------------------------------------------------------

// Build the full `.svelte` source for one (shape, reactivity, target) cell. The
// element carries the value attribute/directive; the `state` reactivity adds the
// onclick reassignment (and, when the element is not self-closing, a `{subject}` text
// child is NOT added — the onclick alone keeps the signal live and the golden minimal).
function buildFixture(shape, reactivity, target) {
  const { script, onclick } = scriptFor(reactivity);
  const sub = reactivity.kind === "pure" ? pureSubjectFor(shape.kind) : reactivity.subject;
  const valueExpr = shape.build(sub, reactivity);
  const attrFrag = target.attr(valueExpr, !!shape.mixed);

  const scriptTag = script ? `<script>\n\t${script}\n</script>\n\n` : "";

  // Assemble the element's attribute list: the onclick (state only) + the value attr.
  const attrs = [onclick, attrFrag].filter(Boolean).join(" ");
  const element =
    target.element === "input" || target.selfClose
      ? `<input ${attrs} />`
      : `<${target.element} ${attrs}></${target.element}>`;

  return `${scriptTag}${element}\n`;
}

// The stable slug for a cell: `<shape>__<target>__<reactivity>`. Filesystem-safe
// (alnum + underscore).
function cellSlug(shape, reactivity, target) {
  return `${shape.kind}__${target.axis}__${reactivity.kind}`;
}

// Build the `.svelte` source for one CONTAINER×CONTENT cell. `body` is the
// already-assembled value-expression (brace form) or quoted-attribute body (mixed
// form). The element + reactivity preamble are assembled exactly like `buildFixture`.
function buildContainerFixture(container, content, reactivity, target, body) {
  const { script, onclick } = scriptFor(reactivity);
  const attrFrag = target.attr(body, !!container.mixed);
  const scriptTag = script ? `<script>\n\t${script}\n</script>\n\n` : "";
  const attrs = [onclick, attrFrag].filter(Boolean).join(" ");
  const element =
    target.element === "input" || target.selfClose
      ? `<input ${attrs} />`
      : `<${target.element} ${attrs}></${target.element}>`;
  return `${scriptTag}${element}\n`;
}

// The stable slug for a container×content cell:
// `<container>_<content>[_multi]__<target>__<reactivity>`.
function containerCellSlug(container, content, reactivity, target, variant = "") {
  const suffix = variant ? `_${variant}` : "";
  return `${container.kind}_${content.kind}${suffix}__${target.axis}__${reactivity.kind}`;
}

// The `componentNameFor` rule the golden generator + the Rust gate share (so Verter
// compiles under the same `name`). Mirrors `gen-svelte-goldens.mjs`.
function componentNameFor(slug) {
  const stem = slug.split("/").pop();
  const sanitized = stem.replace(/[^A-Za-z0-9_$]/g, "_");
  return /^[A-Za-z_$]/.test(sanitized) ? sanitized : `_${sanitized}`;
}

// ---------------------------------------------------------------------------
// Golden normalization — the SAME topology fields the hand-vendored corpus pins,
// plus the byte-precise `clientModule`. (Reuses the shared extractors so the Rust
// gate's expectations are identical.)
// ---------------------------------------------------------------------------

function normalizeGolden(slug, code) {
  const helperSequence = helperSequenceOf(code);
  const cssCode = null; // §5a fixtures carry no scoped CSS (refused upstream).
  return {
    slug,
    backend: "client",
    oracleVersion: SVELTE_ORACLE_VERSION,
    imports: extractImports(code),
    exportDefault: extractExportDefault(code),
    helperSequence,
    helperSet: [...new Set(helperSequence)].sort(),
    helperCounts: helperCountsOf(helperSequence),
    delegatedEvents: extractDelegatedEvents(code),
    templates: extractTemplates(code),
    clientModule: normalizeModuleForComparison(code),
    css: { present: false, hash: null, code: null },
  };
}

// ---------------------------------------------------------------------------
// Corpus build (path -> content map) + manifest
// ---------------------------------------------------------------------------

function buildCorpus(compiler) {
  const files = new Map();
  const manifest = [];
  const shapeCounts = new Map();
  const targetCounts = new Map();
  const reactivityCounts = new Map();
  const contentCounts = new Map();
  const containerCounts = new Map();
  const foldExactCounts = new Map();
  const refuseCounts = new Map();
  const liveFallbackCounts = new Map();
  const seen = new Set();

  // Compile one cell, pin its official golden, and record its manifest row + axis
  // counts. `extraManifest` carries the container/content tags for a container cell
  // (empty for a flat value-shape cell). Shared by both generation passes so the byte
  // golden + topology fields are pinned identically.
  const emitCell = (slug, source, manifestRow) => {
    if (seen.has(slug)) throw new Error(`duplicate codegen cell slug: ${slug}`);
    seen.add(slug);
    const name = componentNameFor(slug);
    let compiled;
    try {
      compiled = compiler.compile(source, {
        generate: "client",
        dev: false,
        filename: `${slug}.svelte`,
        name,
      });
    } catch (err) {
      throw new Error(
        `pinned svelte REFUSED codegen cell ${slug}:\n${source}\n→ ${err && err.message}`,
      );
    }
    const golden = normalizeGolden(slug, compiled.js.code);
    files.set(join(CODEGEN_DIR, `${slug}.svelte`), source);
    files.set(join(CODEGEN_DIR, `${slug}.client.json`), `${JSON.stringify(golden, null, 2)}\n`);
    manifest.push(manifestRow);
  };

  // Emit a `refuse`-bucket cell: assert the pinned compiler REJECTS (compile-fails) the
  // source — official compile-failure is the parity bar. NO golden JS is written (there is
  // no official output); a `.refuse.json` records the official rejection (the error
  // message tail) so the Rust gate knows this slug is an official-reject the Verter side
  // must ALSO refuse with the `const-fold-throw` diagnostic. A cell the pinned compiler
  // ACCEPTS is a generator bug (the row is not actually a compile-time throw).
  const emitRefuseCell = (slug, source, manifestRow) => {
    if (seen.has(slug)) throw new Error(`duplicate codegen cell slug: ${slug}`);
    seen.add(slug);
    const name = componentNameFor(slug);
    let accepted = false;
    let officialError = null;
    try {
      compiler.compile(source, {
        generate: "client",
        dev: false,
        filename: `${slug}.svelte`,
        name,
      });
      accepted = true;
    } catch (err) {
      officialError = (err && err.message) || String(err);
    }
    if (accepted) {
      throw new Error(
        `refuse-bucket cell ${slug} was ACCEPTED by pinned svelte (expected a compile-time ` +
          `throw):\n${source}\nThe row is not a genuine const-fold throw — move it to the ` +
          `fold-exact or live-fallback bucket.`,
      );
    }
    const record = {
      slug,
      bucket: "refuse",
      backend: "client",
      oracleVersion: SVELTE_ORACLE_VERSION,
      officialRejected: true,
      // The official error's first line (a deterministic-enough tail; the Rust gate pins
      // Verter's own diagnostic CODE, not this text).
      officialErrorSummary: String(officialError).split("\n")[0].trim(),
    };
    files.set(join(CODEGEN_DIR, `${slug}.svelte`), source);
    files.set(join(CODEGEN_DIR, `${slug}.refuse.json`), `${JSON.stringify(record, null, 2)}\n`);
    manifest.push(manifestRow);
  };

  // Emit a `live-fallback`-bucket cell: the pinned compiler ACCEPTS + FOLDS the source, but
  // Verter cannot prove byte-exact emission so it emits the LIVE expression. The `.live.json`
  // records the ledger `reason` + that official FOLDS (documentation) so the Rust gate can
  // assert Verter's output is the LIVE form. NO `.client.json` golden is written: Verter
  // deliberately does NOT match official's folded literal, AND official's folded literal can
  // be a lone surrogate (`String.fromCharCode(55296)`) that a strict JSON reader rejects —
  // so the live gate proves "Verter emits live" structurally (a `${…}` interpolation), not
  // by comparison. A cell the pinned compiler REJECTS is a generator bug (it belongs in the
  // refuse bucket).
  const emitLiveFallbackCell = (slug, source, reason, manifestRow) => {
    if (seen.has(slug)) throw new Error(`duplicate codegen cell slug: ${slug}`);
    seen.add(slug);
    const name = componentNameFor(slug);
    let compiled;
    try {
      compiled = compiler.compile(source, {
        generate: "client",
        dev: false,
        filename: `${slug}.svelte`,
        name,
      });
    } catch (err) {
      throw new Error(
        `live-fallback cell ${slug} was REJECTED by pinned svelte (expected an accepted ` +
          `fold):\n${source}\n→ ${err && err.message}\nThe row throws — move it to the ` +
          `refuse bucket.`,
      );
    }
    // Confirm official FOLDED the chunk (its emitted module has NO live `${` interpolation
    // for this chunk — the value is inlined as a literal). This pins the contrast: official
    // folds, Verter live-falls-back.
    const officialModule = normalizeModuleForComparison(compiled.js.code);
    const record = {
      slug,
      bucket: "live-fallback",
      backend: "client",
      oracleVersion: SVELTE_ORACLE_VERSION,
      // The checked-in `LiveFallbackReason` label (the Rust ledger gate cross-checks this
      // against `live_fallback_ledger()`).
      reason,
      // Documentation: official folds this chunk (Verter deliberately emits live instead).
      officialFolds: true,
      // Whether official's emitted module inlined the value WITHOUT a live interpolation
      // (the fold). A `set_attribute` with a quoted literal and no `${` for this chunk.
      officialModuleHasInterpolation: officialModule.includes("${"),
    };
    files.set(join(CODEGEN_DIR, `${slug}.svelte`), source);
    files.set(join(CODEGEN_DIR, `${slug}.live.json`), `${JSON.stringify(record, null, 2)}\n`);
    manifest.push(manifestRow);
  };

  // PASS 1 — the flat value-shape × target × reactivity cells.
  for (const shape of SHAPES) {
    for (const reactKind of shape.reactivities) {
      const reactivity = REACTIVITIES.find((r) => r.kind === reactKind);
      if (!reactivity) throw new Error(`shape ${shape.kind} lists unknown reactivity ${reactKind}`);
      for (const targetAxis of shape.targets) {
        const target = TARGET_BY_AXIS.get(targetAxis);
        if (!target) throw new Error(`shape ${shape.kind} lists unknown target ${targetAxis}`);
        const slug = cellSlug(shape, reactivity, target);
        const source = buildFixture(shape, reactivity, target);
        emitCell(slug, source, {
          slug,
          shape: shape.kind,
          target: target.axis,
          reactivity: reactivity.kind,
        });
        shapeCounts.set(shape.kind, (shapeCounts.get(shape.kind) ?? 0) + 1);
        targetCounts.set(target.axis, (targetCounts.get(target.axis) ?? 0) + 1);
        reactivityCounts.set(reactivity.kind, (reactivityCounts.get(reactivity.kind) ?? 0) + 1);
      }
    }
  }

  // PASS 2 — the CONTAINER × CONTENT cells: each container family crossed with EVERY
  // content sub-shape over its declared reactivities + targets (a missing
  // container×content cell is a generator bug). These exercise the inner-shape
  // fold/live/memoize divergence (the mixed-template const-fold class FIX #6 closes)
  // that the flat pass's fixed-content container shapes do not.
  for (const container of CONTAINERS) {
    for (const content of CONTENT_SHAPES) {
      for (const reactKind of container.reactivities) {
        const reactivity = REACTIVITIES.find((r) => r.kind === reactKind);
        if (!reactivity) {
          throw new Error(`container ${container.kind} lists unknown reactivity ${reactKind}`);
        }
        const sub = reactivity.subject;
        for (const targetAxis of container.targets) {
          const target = TARGET_BY_AXIS.get(targetAxis);
          if (!target) {
            throw new Error(`container ${container.kind} lists unknown target ${targetAxis}`);
          }
          // The single-hole cell.
          const body = container.embed(content, sub);
          const slug = containerCellSlug(container, content, reactivity, target);
          emitCell(slug, buildContainerFixture(container, content, reactivity, target, body), {
            slug,
            shape: `${container.kind}_container`,
            container: container.kind,
            content: content.kind,
            target: target.axis,
            reactivity: reactivity.kind,
          });
          containerCounts.set(container.kind, (containerCounts.get(container.kind) ?? 0) + 1);
          contentCounts.set(content.kind, (contentCounts.get(content.kind) ?? 0) + 1);
          targetCounts.set(target.axis, (targetCounts.get(target.axis) ?? 0) + 1);
          reactivityCounts.set(reactivity.kind, (reactivityCounts.get(reactivity.kind) ?? 0) + 1);

          // The MULTI-interpolation variant (two holes) for a `multi` container — the
          // official multi-chunk template fold (`"a {C} b {C} c"`).
          if (container.multi && container.embedMulti) {
            const multiBody = container.embedMulti(content, sub);
            const multiSlug = containerCellSlug(container, content, reactivity, target, "multi");
            emitCell(
              multiSlug,
              buildContainerFixture(container, content, reactivity, target, multiBody),
              {
                slug: multiSlug,
                shape: `${container.kind}_container`,
                container: container.kind,
                content: content.kind,
                variant: "multi",
                target: target.axis,
                reactivity: reactivity.kind,
              },
            );
            containerCounts.set(container.kind, (containerCounts.get(container.kind) ?? 0) + 1);
            contentCounts.set(content.kind, (contentCounts.get(content.kind) ?? 0) + 1);
            targetCounts.set(target.axis, (targetCounts.get(target.axis) ?? 0) + 1);
            reactivityCounts.set(reactivity.kind, (reactivityCounts.get(reactivity.kind) ?? 0) + 1);
          }
        }
      }
    }
  }

  // PASS 3 — the CONST-FOLD TRI-STATE cells: each of the three buckets (`fold-exact`,
  // `refuse`, `live-fallback`) compiled as a MIXED-template chunk (the multi-chunk
  // evaluate-fold). The baseline target is a generic attr (`id="a {value} b"`); a few
  // representative rows CROSS the bucket over the `class` / `style` / `boolean` targets to
  // prove the tri-state decision is target-INDEPENDENT (a const-fold throw inside a
  // `class=` / `style=` / boolean prop must also refuse; a live-fallback inside one must
  // also emit live). The mixed value body is `a {value} b` over the target's `mixed` form.
  const mixedBody = (value) => `a {${value}} b`;
  const mixedSourceFor = (target, script, value) => {
    const scriptTag = `<script>\n\t${script}\n</script>\n\n`;
    const frag = target.attr(mixedBody(value), true);
    const el =
      target.element === "input" || target.selfClose
        ? `<input ${frag} />`
        : `<${target.element} ${frag}></${target.element}>`;
    return `${scriptTag}${el}\n`;
  };
  // The cross-target set (besides the `attr` baseline) used for the representative
  // target-independence rows.
  const CROSS_TARGETS = ["boolean", "class", "style"];

  // BUCKET 1 — `fold-exact`: official folds, Verter folds to the SAME literal (byte-match).
  for (const edge of FOLD_EXACT_EDGES) {
    const attrTarget = TARGET_BY_AXIS.get("attr");
    const slug = `foldexact_${edge.name}`;
    emitCell(slug, mixedSourceFor(attrTarget, edge.script, edge.value), {
      slug,
      shape: "fold_edge",
      bucket: "fold-exact",
      foldEdge: edge.family,
      foldEdgeName: edge.name,
      target: "attr",
      reactivity: "demoted",
    });
    foldExactCounts.set(edge.family, (foldExactCounts.get(edge.family) ?? 0) + 1);
    targetCounts.set("attr", (targetCounts.get("attr") ?? 0) + 1);
    reactivityCounts.set("demoted", (reactivityCounts.get("demoted") ?? 0) + 1);
  }
  // A representative fold-exact row crossed over class / style / boolean targets.
  {
    const repr = FOLD_EXACT_EDGES.find((e) => e.name === "bigint_add") ?? FOLD_EXACT_EDGES[0];
    for (const axis of CROSS_TARGETS) {
      const target = TARGET_BY_AXIS.get(axis);
      const slug = `foldexact_${repr.name}__${axis}`;
      emitCell(slug, mixedSourceFor(target, repr.script, repr.value), {
        slug,
        shape: "fold_edge",
        bucket: "fold-exact",
        foldEdge: repr.family,
        foldEdgeName: repr.name,
        target: axis,
        reactivity: "demoted",
      });
      foldExactCounts.set(repr.family, (foldExactCounts.get(repr.family) ?? 0) + 1);
      targetCounts.set(axis, (targetCounts.get(axis) ?? 0) + 1);
      reactivityCounts.set("demoted", (reactivityCounts.get("demoted") ?? 0) + 1);
    }
  }

  // BUCKET 2 — `refuse`: official compile-FAILS, Verter must refuse (no golden JS).
  for (const edge of REFUSE_EDGES) {
    const attrTarget = TARGET_BY_AXIS.get("attr");
    const slug = `refuse_${edge.name}`;
    emitRefuseCell(slug, mixedSourceFor(attrTarget, edge.script, edge.value), {
      slug,
      shape: "fold_edge",
      bucket: "refuse",
      foldEdge: edge.family,
      foldEdgeName: edge.name,
      refuseReason: edge.reason,
      target: "attr",
      reactivity: "demoted",
    });
    refuseCounts.set(edge.family, (refuseCounts.get(edge.family) ?? 0) + 1);
    targetCounts.set("attr", (targetCounts.get("attr") ?? 0) + 1);
    reactivityCounts.set("demoted", (reactivityCounts.get("demoted") ?? 0) + 1);
  }
  // A representative refuse row crossed over class / style / boolean targets.
  {
    const repr = REFUSE_EDGES.find((e) => e.name === "refuse_mix_add") ?? REFUSE_EDGES[0];
    for (const axis of CROSS_TARGETS) {
      const target = TARGET_BY_AXIS.get(axis);
      const slug = `refuse_${repr.name}__${axis}`;
      emitRefuseCell(slug, mixedSourceFor(target, repr.script, repr.value), {
        slug,
        shape: "fold_edge",
        bucket: "refuse",
        foldEdge: repr.family,
        foldEdgeName: repr.name,
        refuseReason: repr.reason,
        target: axis,
        reactivity: "demoted",
      });
      refuseCounts.set(repr.family, (refuseCounts.get(repr.family) ?? 0) + 1);
      targetCounts.set(axis, (targetCounts.get(axis) ?? 0) + 1);
      reactivityCounts.set("demoted", (reactivityCounts.get("demoted") ?? 0) + 1);
    }
  }

  // BUCKET 3 — `live-fallback`: official folds, Verter emits LIVE (not byte-equal).
  for (const edge of LIVE_FALLBACK_EDGES) {
    const attrTarget = TARGET_BY_AXIS.get("attr");
    const slug = `livefallback_${edge.name}`;
    emitLiveFallbackCell(slug, mixedSourceFor(attrTarget, edge.script, edge.value), edge.reason, {
      slug,
      shape: "fold_edge",
      bucket: "live-fallback",
      foldEdge: edge.family,
      foldEdgeName: edge.name,
      liveFallbackReason: edge.reason,
      target: "attr",
      reactivity: "demoted",
    });
    liveFallbackCounts.set(edge.family, (liveFallbackCounts.get(edge.family) ?? 0) + 1);
    targetCounts.set("attr", (targetCounts.get("attr") ?? 0) + 1);
    reactivityCounts.set("demoted", (reactivityCounts.get("demoted") ?? 0) + 1);
  }
  // A representative live-fallback row crossed over class / style / boolean targets.
  {
    const repr = LIVE_FALLBACK_EDGES.find((e) => e.name === "live_log") ?? LIVE_FALLBACK_EDGES[0];
    for (const axis of CROSS_TARGETS) {
      const target = TARGET_BY_AXIS.get(axis);
      const slug = `livefallback_${repr.name}__${axis}`;
      emitLiveFallbackCell(slug, mixedSourceFor(target, repr.script, repr.value), repr.reason, {
        slug,
        shape: "fold_edge",
        bucket: "live-fallback",
        foldEdge: repr.family,
        foldEdgeName: repr.name,
        liveFallbackReason: repr.reason,
        target: axis,
        reactivity: "demoted",
      });
      liveFallbackCounts.set(repr.family, (liveFallbackCounts.get(repr.family) ?? 0) + 1);
      targetCounts.set(axis, (targetCounts.get(axis) ?? 0) + 1);
      reactivityCounts.set("demoted", (reactivityCounts.get("demoted") ?? 0) + 1);
    }
  }

  // COVERAGE GATE: every required value-shape / target / reactivity / content /
  // container / fold-edge axis must contribute ≥1 row, so a dropped enumerator (the
  // corpus silently losing a finite axis) fails generation HARD.
  const missingShapes = REQUIRED_SHAPE_AXES.filter((a) => (shapeCounts.get(a) ?? 0) === 0);
  const missingTargets = REQUIRED_TARGET_AXES.filter((a) => (targetCounts.get(a) ?? 0) === 0);
  const missingReact = REQUIRED_REACTIVITY_AXES.filter((a) => (reactivityCounts.get(a) ?? 0) === 0);
  const missingContent = REQUIRED_CONTENT_AXES.filter((a) => (contentCounts.get(a) ?? 0) === 0);
  const missingContainer = REQUIRED_CONTAINER_AXES.filter(
    (a) => (containerCounts.get(a) ?? 0) === 0,
  );
  // Each of the THREE const-fold buckets must contribute ≥1 row per family.
  const missingFoldExact = REQUIRED_FOLD_EXACT_FAMILIES.filter(
    (a) => (foldExactCounts.get(a) ?? 0) === 0,
  );
  const missingRefuse = REQUIRED_REFUSE_FAMILIES.filter((a) => (refuseCounts.get(a) ?? 0) === 0);
  const missingLiveFallback = REQUIRED_LIVE_FALLBACK_FAMILIES.filter(
    (a) => (liveFallbackCounts.get(a) ?? 0) === 0,
  );
  const missing = [
    ...missingShapes.map((a) => `shape:${a}`),
    ...missingTargets.map((a) => `target:${a}`),
    ...missingReact.map((a) => `reactivity:${a}`),
    ...missingContent.map((a) => `content:${a}`),
    ...missingContainer.map((a) => `container:${a}`),
    ...missingFoldExact.map((a) => `fold-exact:${a}`),
    ...missingRefuse.map((a) => `refuse:${a}`),
    ...missingLiveFallback.map((a) => `live-fallback:${a}`),
  ];
  // The three buckets must EACH be non-empty (a tri-state contract corpus that lost a whole
  // bucket — e.g. no refuse rows — is a generator regression).
  if (foldExactCounts.size === 0 || refuseCounts.size === 0 || liveFallbackCounts.size === 0) {
    missing.push(
      `const-fold-bucket(fold-exact=${foldExactCounts.size},refuse=${refuseCounts.size},` +
        `live-fallback=${liveFallbackCounts.size})`,
    );
  }
  // Every container × content combination MUST exist (the doctrine: a missing
  // container×content cell is a generator bug).
  const present = new Set(
    manifest.filter((m) => m.container).map((m) => `${m.container}×${m.content}`),
  );
  for (const container of CONTAINERS) {
    for (const content of CONTENT_SHAPES) {
      if (!present.has(`${container.kind}×${content.kind}`)) {
        missing.push(`container-content:${container.kind}×${content.kind}`);
      }
    }
  }
  if (missing.length > 0) {
    throw new Error(
      `codegen corpus is missing required axes (no rows generated): ${missing.join(", ")}`,
    );
  }

  // A top-level manifest summarizing the corpus (counts + per-axis row counts). The
  // per-fixture `.client.json` files are the authority the Rust gate consumes; the
  // `required_*` lists are what the Rust coverage gate enforces.
  const sortObj = (m) =>
    Object.fromEntries([...m.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)));
  const manifestJson = `${JSON.stringify(
    {
      svelte_oracle_version: SVELTE_ORACLE_VERSION,
      total: manifest.length,
      required_shape_axes: REQUIRED_SHAPE_AXES,
      required_target_axes: REQUIRED_TARGET_AXES,
      required_reactivity_axes: REQUIRED_REACTIVITY_AXES,
      required_content_axes: REQUIRED_CONTENT_AXES,
      required_container_axes: REQUIRED_CONTAINER_AXES,
      // The three const-fold tri-state BUCKETS' required families.
      required_fold_exact_families: REQUIRED_FOLD_EXACT_FAMILIES,
      required_refuse_families: REQUIRED_REFUSE_FAMILIES,
      required_live_fallback_families: REQUIRED_LIVE_FALLBACK_FAMILIES,
      shape_counts: sortObj(shapeCounts),
      target_counts: sortObj(targetCounts),
      reactivity_counts: sortObj(reactivityCounts),
      content_counts: sortObj(contentCounts),
      container_counts: sortObj(containerCounts),
      fold_exact_counts: sortObj(foldExactCounts),
      refuse_counts: sortObj(refuseCounts),
      live_fallback_counts: sortObj(liveFallbackCounts),
    },
    null,
    2,
  )}\n`;
  files.set(join(CODEGEN_DIR, "manifest.json"), manifestJson);
  return { files, total: manifest.length };
}

function walkFiles(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
  )) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...walkFiles(p));
    else out.push(p);
  }
  out.sort();
  return out;
}

function writeMode(compiler) {
  const { files, total } = buildCorpus(compiler);
  rmSync(CODEGEN_DIR, { recursive: true, force: true });
  for (const [path, content] of [...files].sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  console.log(
    `gen-svelte-codegen-corpus: wrote ${total} cell(s) from svelte@${SVELTE_ORACLE_VERSION} into ` +
      `${relative(REPO_ROOT, CODEGEN_DIR)}`,
  );
}

function checkMode(compiler) {
  const { files, total } = buildCorpus(compiler);
  const drift = [];
  for (const [path, content] of files) {
    const rel = relative(REPO_ROOT, path);
    if (!existsSync(path)) {
      drift.push(`MISSING codegen artifact: ${rel}`);
      continue;
    }
    if (readFileSync(path, "utf8") !== content) {
      drift.push(`DRIFTED codegen artifact (on-disk != regenerated): ${rel}`);
    }
  }
  for (const path of walkFiles(CODEGEN_DIR)) {
    if (!files.has(path)) {
      drift.push(`STALE codegen artifact (no fresh source): ${relative(REPO_ROOT, path)}`);
    }
  }
  if (drift.length > 0) {
    console.error(
      `gen-svelte-codegen-corpus --check: ${drift.length} drift(s) detected:\n` +
        drift.map((d) => `  - ${d}`).join("\n") +
        `\n\nRun \`node scripts/gen-svelte-codegen-corpus.mjs\` to regenerate.`,
    );
    process.exit(1);
  }
  console.log(
    `gen-svelte-codegen-corpus --check: ${total} cell(s) in sync with svelte@${SVELTE_ORACLE_VERSION}.`,
  );
}

function main() {
  const check = process.argv.includes("--check");
  const compiler = loadPinnedCompiler(REPO_ROOT);
  if (check) checkMode(compiler);
  else writeMode(compiler);
}

main();
