# Owner-aware root value-binding index — design (v2)

**v2 supersedes v1** after an independent architecture review (codex xhigh,
neutral framing, full text preserved at
`docs/arch/refactor/rev11/evidence/CM1/binding-index-design-v1-review.md`)
found v1's completeness-boundary CLAIM correct in direction but its mechanism
factually wrong in four places: (a) Vue script content is not unconditionally
ES-module source, so `with`/Annex-B are not moot; (b) a flat combined
module+instance `Program` binds both owners into ONE scope, which is not
parent/child and can misfire on legitimate same-name declarations across the
two scripts; (c) two separate `SemanticBuilder::build()` calls over variants
of the same AST cannot share `IdentifierReference::reference_id()` — OXC
clears/reassigns semantic IDs per build, so v1's "read `.reference_id()` off
the original node after either build" claim does not hold; (d) `is_value()`
alone is not a runtime-survival test — an ambient `declare` construct can
carry value flags with no runtime binding, and the pinned OXC 0.126
`TSImportEqualsDeclaration` binder path ignores the node's own `import_kind`,
so a `import type Foo = X.Y` is (mis)bound as a value in this exact pinned
version. v1's core direction (delegate to OXC's binder, not a hand-rolled
scanner) survives review. v1's placement, view model, and identity handoff do
not, and are replaced below.

## Problem restated

`defineProps({ label: String })` (and the structurally identical Options-API
`props: { label: String }`) must recognize `String` as Vue's runtime
constructor ONLY when `String` genuinely refers to the global constructor at
that point in the script. If the author has locally declared, imported, or
otherwise bound a name `String` (or any of the other nine recognized runtime
constructor spellings: `Number`, `Boolean`, `Array`, `Object`, `Function`,
`Symbol`, `Date`, `RegExp`, `Promise`), that local binding must win and the
identifier must be resolved as an ordinary authored value reference — never
silently treated as the global constructor.

A prior attempt (`block/cm1`, commits `e83f33d6d..96e1dc040`, reverted at
`a7bf8c696`) built `LocalConstructorShadowing`, a hand-rolled AST scanner that
walked statements looking for shadowing declarations. Four successive review
rounds each found and closed one gap class, and each closure revealed
another, until a final architecture review found the mechanism had **no
stateable completeness boundary** and deleted it rather than patch it a fifth
time.

**Current trunk baseline** (post-revert, pre-this-block, verified directly):
`extract_props_from_options` (`component_meta.rs:3053-3070`) recognizes a bare
`String`/`Number`/`Boolean` identifier **unconditionally by name** and folds
it to `ClosedTypeFact::Leaf(LeafTypeFact::Primitive(..))`; every other
spelling — including the other seven recognized runtime-constructor names —
falls through to `raw_type` display text only. `extract_prop_fields_from_runtime`
(`macros.rs`) currently writes **display text only** for every constructor
spelling, including `String`/`Number`/`Boolean` (`macros.rs:3059`) — it does
**not** currently fold the three primitives to a closed fact at all (v1
incorrectly stated this as existing, unchanged behavior; it is not — closing
this is in scope, see "Consumer wiring" below). Constructor-array
(`[String, Number]`) and nullable (`[String, null]`) forms are **not**
currently handled by either path: `options.rs:244` explicitly drops a
constructor array as "no single constructor," and `macros.rs`'s runtime
extraction only recognizes a direct identifier or an `as`-asserted identifier
(`macros.rs:3057`). CM1's charter (§4) requires closing all of this — the
prior deleted mechanism's evidence packet recorded it working before the
whole mechanism was reverted alongside the unsound shadow check; re-doing it
here, on the corrected foundation, is in scope for this block, not a
follow-on.

## Why a real index, not a scanner (unchanged from v1, still correct)

The scanner's failure mode was structural, not a coverage bug: "does this
identifier look shadowed" is an unbounded question when answered by
pattern-matching individual declaration shapes. The question the codebase
actually needs answered is **"what does this identifier resolve to, in this
scope, under this owner"** — a binding-resolution question with a complete,
spec-compliant, already-used-in-this-codebase answer:
`oxc_semantic::SemanticBuilder`, the same binder
`crates/verter_compiler/src/svelte/runtime/component_scope_facts.rs` already
trusts to source Svelte's own scope facts "from OXC's own binder — rather
than a hand-rolled per-frame visitor."

Revised completeness-boundary statement (the v1 sentence overclaimed; this is
the corrected version the review proposed and this design adopts verbatim):

> **Completeness boundary.** Constructor-identifier resolution is the result
> of OXC binding over an EXHAUSTIVELY DEFINED runtime-surviving AST
> projection (ambient/type-erased constructs removed, exactly as the Svelte
> precedent already does for its own runtime-surviving projection), composed
> through a CLOSED owner scope graph (one real parent/child nesting, not two
> independently-bound views), with any dynamically-scoped or
> statically-unresolvable position (`with`, direct `eval`, a non-clean parse)
> returning `Indeterminate` rather than guessing `Global`.

## Structural pieces (reused, corrected)

1. **`oxc_semantic::SemanticBuilder`** over a dedicated binding-only AST
   clone (see "The index itself" — never the shared retained Program other
   consumers walk by flat statement index).
2. **Value-space filtering, WITH an explicit runtime-survival projection
   first.** `is_value()` alone is insufficient: an ambient `declare`
   construct (`declare const`, `declare function`, `declare class`, `declare
   namespace`) can carry `SymbolFlags` with a value bit set despite emitting
   no runtime binding, and OXC 0.126's `TSImportEqualsDeclaration` binder
   path (`binder.rs:367` in the pinned `oxc_semantic-0.126.0` source) calls
   `declare_symbol_for_import_specifier(..., false)` UNCONDITIONALLY,
   ignoring the node's own `import_kind` (`ts.rs:1688` in the pinned
   `oxc_ast-0.126.0` source) — so `import type Foo = X.Y` is bound as a value
   symbol by the binder itself in this exact pinned version. Before binding,
   run an explicit runtime-survival projection erasing every construct that
   is source-level-only per TypeScript emit semantics: `declare` ambient
   declarations of every kind, a type-only `namespace`/`module`, type-only
   `import`/`export` (already excluded via `is_value()`, kept as a second
   line of defense, not the sole line), and — as a targeted, explicit
   override rather than a reliance on the binder's own (buggy, for this
   pinned version) classification — force `TSImportEqualsDeclaration` to
   type-space whenever its own `import_kind` says `type`, regardless of what
   `is_value()` reports post-bind. **Do not re-derive this classification
   from scratch**: `SvelteScopeProjection`
   (`component_scope_facts.rs:375-…`) already implements the general
   "TS-erased-before-binding" projection for the same binder, exhaustively
   over OXC's `Statement`/`Declaration`/`Expression` variants; factor its
   erasure classification into a shared, framework-neutral helper both Vue's
   and Svelte's projections call, rather than authoring a second
   independently-maintained classifier that will drift from the first the
   way the deleted scanner drifted from `constructor_to_ts_type`. Vue-specific
   deltas from Svelte's classification (if any — e.g. Svelte's unconditional
   `enum` erasure is Svelte-specific; Vue's TS support keeps enums as real
   value bindings) are additive parameters to the shared classifier, not a
   forked copy of it.
3. **Real source-type awareness — `with`/Annex-B are NOT moot for Vue.**
   v1 asserted Vue script content always parses as an ES module; this is
   false. `vue_script_source_type`
   (`crates/verter_compiler/src/framework_common/vue_bridge.rs:106`) resolves
   a script's dialect from its `lang` attribute, and
   `oxc_source_type_from_neutral`
   (`crates/verter_session/src/parse.rs:1215`) maps a classic `<script
   lang="js">` (no `lang`, or `lang="js"`) to `SourceType::script()` — sloppy,
   non-module JS, where `with` is legal syntax and Annex-B function-in-block
   hoisting applies. The index MUST use the SAME `SourceType`/`source_type`
   value the caller already resolved for this exact script (available at the
   call site — `build_script_analysis_inner` already takes `source_type:
   SourceType`, see "Placement" below) rather than assuming module semantics.
   For a script whose resolved source type is a module (`<script setup>`
   always; TS/TSX; `lang="mjs"`), `with` is a syntax error and cannot appear
   in a clean parse — but the index does not assume this by dialect name, it
   is a structural consequence of binding with the real `SourceType` and
   trusting the parser's own diagnostics (see point 4).
4. **`with` / direct `eval` — explicit `Indeterminate`, not a byproduct of
   "unresolved."** A reference lexically inside a `WithStatement`, or inside
   a scope OXC's binder marks as containing a direct `eval` call, is
   `Indeterminate`, never `Global` — an unresolved-looking reference in that
   position is not evidence of "no local binding," it is evidence "static
   resolution does not apply here." Read this off OXC's own per-scope flags
   (the pinned `oxc_semantic` exposes scope-level tracking used by
   `is_global_reference.rs`'s own with-statement caveat; the implementer
   verifies the exact accessor against the pinned `oxc_semantic-0.126.0`
   source rather than this document guessing a method name) rather than
   inferring it from a `None` symbol id, which is indistinguishable from a
   genuinely free reference.
5. **`TopLevelOwnerId` / `TopLevelOwnerTable`** — unchanged from v1, still the
   correct owner-topology authority: `validated_lexical_parent_owner(Instance)
   == Some(unique_module_owner)`; `validated_lexical_parent_owner(Module) ==
   None`; ambiguous topology already fails closed. v1's error was not in this
   primitive, but in how the index used it (see next section).

## The index itself — ONE nested-scope bind, not two independently-bound views

v1's dual `module_view`/`instance_view` design is replaced. Building two
separate `SemanticBuilder::build()` results and expecting to read
`reference_id()` off the ORIGINAL retained Program's nodes after either is
unsound: `IdentifierReference::reference_id()` is a `Cell` populated in place
by whichever build ran; a second build over the same or a cloned tree either
leaves the original's cells stale (if it binds a clone) or overwrites them
(if it binds the original a second time) — there is no way to attribute a
resolution to "the module-only bind" versus "the whole-program bind" after
the fact from the original node.

Instead: build a **single dedicated binding clone** of the script's
statements, structured to mirror Vue's REAL runtime scope shape — which
happens to make this not a scoping trick but a structurally faithful model:
Vue actually compiles `<script setup>` content into the body of a `setup()`
function nested inside the options object, itself at module top level. The
binding clone:

1. Copies module-owned top-level statements (`TopLevelOwnerTable` selects
   them) as direct top-level statements, in original relative order, exactly
   as authored.
2. Wraps ALL instance-owned top-level statements as the body of ONE synthetic
   function statement, appended after the module-owned statements. A
   **function** wrapper, not a bare block: a block only opens a new lexical
   scope for `let`/`const`/`class`, but `var`/function-declaration hoisting
   in sloppy mode passes THROUGH a block to the nearest function/Program
   scope — wrapping in a bare block would let an instance-owned `var` bleed
   into the module scope, which is wrong. A function body gives real hoisting
   containment matching Vue's actual compiled shape.
3. Runs the runtime-survival projection (point 2 above) over the WHOLE
   clone, then a single `SemanticBuilder::new().build(&clone)`.
4. Degenerate/ambiguous owner topology (no table, or more than one owner of a
   kind) skips the wrap and marks the whole result `Indeterminate` at query
   time — never guesses a shape.

This is ONE bind, ONE scope tree: the synthetic function's body is a REAL
child scope of the Program root. A module-owned reference (living in a
top-level statement never moved into the wrapper) resolves starting at the
Program root scope, which has no child-scope visibility — it structurally
cannot see anything declared inside the instance wrapper, matching
`validated_lexical_parent_owner(Module) == None`. An instance-owned reference
(living inside the wrapper) resolves starting at the wrapper's function
scope, walking outward to the Program root — seeing both instance-local and
module-level top-level bindings, matching `validated_lexical_parent_owner(Instance)
== Some(module)`. Same-name declarations that are legitimately independent
across the two owners (a module `const X` and an unrelated instance `const
X`) are now genuinely separate declarations in separate scopes — no
redeclaration collision, unlike v1's flat single-scope merge.

**Correlation without shared `reference_id`s.** The clone's nodes have their
own, independent semantic IDs — never compared against the original retained
Program's IDs. Query by **span**, not by node identity: build the clone once
per file version, walk its bound tree once to produce a
`FxHashMap<Span, ReferenceId>` (or resolve on demand via a single indexed
lookup) keyed by each `IdentifierReference`'s byte span — spans are `u32`
offsets into the SAME source text the clone was parsed from, stable and
directly comparable to the span already carried by the ORIGINAL AST node the
caller holds (macro-argument identifier, Options-API constructor identifier).
The query function takes a `Span`, not a node reference:

```rust
fn resolve_value_identifier(index: &RootBindingIndex, at: Span) -> BindingResolution
```

**Canonical binding identity, not a bare `SymbolId`.** `SymbolId` is a dense
index local to this one clone's bind and means nothing to any other Verter
subsystem. The `Local` arm must carry something the existing shared resolver
can actually consume: the admission gate this composes with
(`component_meta_binding_type_entries`, keyed by `DeclBindingKey(owner,
name)` — `eval_env.rs:942`) looks up by owner + name, not by an OXC symbol
id. `Local` therefore carries the resolved `(TopLevelOwnerId, name)` pair (the
symbol's declaring name plus the owner it was found in, itself derived from
whether the resolving scope was the Program root or the instance wrapper) —
enough to construct the same `DeclBindingKey` the general admission path
already indexes by, with no new identity scheme:

```rust
enum BindingResolution {
    /// No local declaration binds this name in the runtime-surviving scope
    /// graph for this owner — safe to treat as the language/runtime global
    /// by identity.
    Global,
    /// Bound to a real local, runtime-surviving value declaration. Resolve
    /// through the general authored-value-reference route keyed by this
    /// pair — never through name-based global semantics.
    Local(DeclBindingKey),
    /// Static resolution does not apply (a `with`/direct-`eval` scope, a
    /// non-clean parse on the relevant script, or ambiguous/missing owner
    /// topology). Fails closed — never defaults to `Global`.
    Indeterminate,
}
```

## Placement — built once in the outer analysis driver, threaded to both consumers

v1 placed the index inside `analyze_macros_from_program_with_owners`, which
is wrong on two counts: `extract_props_from_options`
(`component_meta.rs:1583,3053`) runs on already-extracted `AnalyzedOptionsApi`
data with no `Program`/owner access at that point — the Options-API path's
integration point is wherever its OWN earlier walk populates
`type_constructor` from the raw AST (the implementer locates this walk; it is
not `extract_props_from_options` itself, which is too late) — and parser
cleanliness (`parse_errors: bool`) is not available inside
`analyze_macros_from_program_with_owners` at all; it is carried by
`build_script_analysis_inner` (`build.rs:360`) alongside `program`,
`source_type`, and `owners`.

Build the `RootBindingIndex` ONCE at that outer level — `build_script_analysis_inner`
or its immediate caller, which already has `program`, `source_type`, `owners`,
and `parse_errors` together — and thread a reference down into BOTH the
macro-analysis walk and the Options-API walk. Neither performs its own
separate bind. This also matches CLAUDE.md's "process once per parse" build
philosophy: one dedicated binding clone, one bind, reused by every
constructor-identifier check in that file version — not rebuilt per macro
call site.

The index is a **transient, per-parse artifact**, exactly like the retained
OXC parse arena it derives from — never persisted, memoized across file
versions, or exposed as a new `ProjectTypeStore` cache layer. No
`FileArtifactStore`/`SemanticGraphStore` key composition changes.

**This is binding resolution, not type resolution — it does not touch the
single-type-engine rule.** CLAUDE.md's "OXC is a syntax/lowering front-end
ONLY and never resolves types at query time" governs `TypeExpr`/semantic
*type* resolution reachable from `SemanticQueryKey` /
`ProjectSemanticDispatch::execute` — the query-time engine this index never
calls, participates in, or is reachable from. `RootBindingIndex` answers a
categorically different, narrower question — "does this identifier name a
runtime-surviving local declaration in this lexical scope, under this
owner" — using OXC's binder exactly as `SvelteScopeProjection`
(`component_scope_facts.rs`) already does for the same class of question:
scope/binding facts over a script's own retained parse, at PRODUCER time
(shallow/macro analysis, building `AnalyzedPropField`/`AnalyzedOptionsProp`
facts), never inside a `SemanticQueryKey` dispatch or any path a later
query-time type lookup re-enters. It returns a binding identity
(`DeclBindingKey` or `Global`/`Indeterminate`), never a `TypeExpr` — the
result feeds the SAME general authored-value-reference route any other
locally-declared identifier already resolves through; it does not create a
second place types get resolved. The implementation must keep this true
structurally: `RootBindingIndex` construction and `resolve_value_identifier`
calls live only in the shallow/macro-analysis producer path
(`build_script_analysis_inner` and its two consumers), never behind
`ProjectSemanticDispatch`, `SemanticGraphStore`, or any other query-time
entry point.

There is no legacy path to shim: the deleted mechanism left no trace on trunk
(the revert restored pre-existing, shadow-blind behavior; `a7bf8c696` is not
an ancestor of `d74267780`). This lands as the only path.

## Consumer wiring — both extraction paths, ten spellings, one shared typed carrier

CM1 charter §4 requires "one typed runtime-constructor fact/enum,
producer-owned, shared by Options … and macro … extraction," covering the
shorthand and expanded forms, `required`/`has_default`/default-value
combinations, constructor-array (`[String, Number]`) and nullable forms. This
is genuinely unimplemented on trunk today (see "Problem restated" above for
what each path currently, incompletely, does) — implement it here, gated by
the index, not as a follow-on:

- For each recognized spelling at a runtime-constructor position (single
  identifier, or each element of a constructor array), consult
  `resolve_value_identifier` at that identifier's span.
- `Global` ⇒ apply the shared runtime-constructor mapping: `String`/`Number`/
  `Boolean` fold to `ClosedTypeFact::Leaf(LeafTypeFact::Primitive(..))`
  (a union of primitives, via `ClosedTypeFact::LeafUnion`, for a
  multi-primitive constructor array); the other seven spellings keep their
  existing display-text-only route, unchanged in shape — this design does
  not add new closed-fact plumbing for `Array`/`Object`/`Function`/`Symbol`/
  `Date`/`RegExp`/`Promise`, matching the charter's explicit "existing
  correct paths … stay on their current authored-payload route" for anything
  outside the three primitives. A `null` array element is `PrimitiveName::Null`
  added to the leaf union (the deleted mechanism's evidence packet recorded
  this exact interpretation and flagged it for maintainer confirmation
  rather than treating it as self-evidently correct — preserve that framing,
  do not silently re-decide it).
- `Local(key)` ⇒ for ALL ten spellings, do not apply any global-constructor
  semantics (neither the closed-fact fold nor the hardcoded display string).
  Resolve through the general authored-value-reference route keyed by `key`
  — the SAME route a locally-declared, unrecognized-spelling constructor
  (a custom class) already takes. No new resolution path.
- `Indeterminate` ⇒ fails closed as a genuine preparation failure (CM1's
  established `Failed`-vs-`Absent` discipline, charter §3) — not a silent
  `Unknown`, not a silent `Global` fallback. The implementer specifies the
  concrete typed carrier from shallow extraction (`AnalyzedPropField` /
  `AnalyzedOptionsProp`) through to `SourcePosition`/`ComponentMetaOutputFailure`
  — "the same channel Finding B uses" is a pointer to the pattern, not a
  finished wiring spec, and needs its own carrier field(s) since `Local`/
  `Indeterminate` are new outcomes those types do not yet represent.

This unification means the ONLY remaining name comparison in the whole
mechanism is the pre-existing, charter-sanctioned "which of the ten spellings
is this" match, run only after the gate has already answered `Global` —
i.e., only once no runtime-surviving local binding exists under that name.
Resolution is by binding identity (`DeclBindingKey`), never by spelling, at
the gate itself.

## Discriminating test matrix (one test per gap class, must fail without the index)

| Gap class | Fixture | Expected without index (red) | Expected with index (green) |
|---|---|---|---|
| Hoisted `var` at arbitrary nesting depth | `if (x) { if (y) { var String = 1 } }` before `defineProps({ label: String })` | closed-fact `String` primitive | authored local reference |
| TS namespace (real, non-ambient) | `namespace String { export const x = 1 }` | closed-fact primitive | authored local reference |
| TS namespace (ambient — must NOT shadow) | `declare namespace String { export const x: 1 }` | (mechanism-dependent) | `Global` — ambient erasure means no runtime binding |
| Ordinary type-only import | `import type { String } from './x'` | must not be a local binding | `Global` — closed-fact primitive, unchanged |
| Value import (ordinary) | `import { String } from './x'` | closed-fact primitive (wrong) | authored local reference |
| `TSImportEqualsDeclaration` (value) | `import String = require('./x')` | closed-fact primitive (wrong) | authored local reference |
| `TSImportEqualsDeclaration` (type-only — binder quirk) | `import type String = X.Y` | closed-fact primitive (right, by luck) | `Global` — via the explicit `import_kind` override, not the (buggy, for pinned OXC) `is_value()` result |
| Non-primitive spellings locally shadowed | `class Array {}` then `defineProps({ items: Array })` | hardcoded `"Array<any>"` display text (wrong) | authored local reference |
| Owner topology — module vs instance | `<script>` declares `const String = 1`; `<script setup>` uses `defineProps({ label: String })` | implementation-accident-dependent | `Local` in instance (module is instance's parent) |
| Owner topology — reverse | `<script setup>` declares `const String = 1`; `<script>` Options `props: { label: String }` | module incorrectly sees the instance-only binding | `Global` in module (module has no parent) |
| Owner topology — legitimate same-name across owners | module `const Foo = 1` AND unrelated instance `const Foo = 2`, no relation to each other | may misfire as a redeclaration under a flat single-scope bind | both resolve independently, no collision |
| Destructuring pattern binding | `const { String } = obj` | closed-fact primitive (wrong) | authored local reference |
| `enum` declaration | `enum String { A }` | closed-fact primitive (wrong) | authored local reference |
| Export-wrapped Options declaration | `export const String = 1` used later in the same module's Options `props` object | closed-fact primitive (wrong) | authored local reference |
| Annex-B function-in-block, sloppy script | `<script lang="js">` (non-module, non-setup): `if (x) { function String() {} }` before a runtime-constructor use | closed-fact primitive (wrong) | authored local reference, honoring sloppy-mode Annex-B hoisting |
| Annex-B function-in-block, module/TS script | same shape under `<script setup>` (strict/module — Annex-B does not apply) | N/A | block-scoped only; correctly does NOT shadow module-level use outside the block |
| `with` statement (sloppy script only — a syntax error under module/TS) | `<script lang="js">`: `with (obj) { defineProps ... String ... }`-shaped reachable reference inside a `with` block | ill-defined | `Indeterminate`, never `Global` |
| Direct `eval` in scope | `eval("var String = 1")` in the same function scope as the constructor use | ill-defined | `Indeterminate`, never `Global` |
| Constructor array | `defineProps({ label: [String, Number] })` | not handled at all today (`options.rs:244` drops it; macro path only recognizes one identifier) | `ClosedTypeFact::LeafUnion([String, Number])` |
| Nullable constructor array | `defineProps({ label: [String, null] })` | not handled at all today | union includes `PrimitiveName::Null` |
| Genuinely unshadowed (regression control) | plain `defineProps({ label: String })`, no local `String` anywhere | closed-fact primitive | closed-fact primitive — must NOT regress |

## Open items for the implementer (routine wiring, not architectural)

- Verify the exact OXC 0.126 accessor(s) for "is this scope/reference inside
  a `with`" and "does this scope contain a direct `eval`" against the pinned
  `oxc_semantic-0.126.0` source (`~/.cargo/registry/.../oxc_semantic-0.126.0/`
  in a normal toolchain checkout) — this document does not guess a method
  name it has not verified.
- Locate the Options-API path's actual pre-`AnalyzedOptionsApi` walk (the
  producer of `type_constructor`) as the real Options-side integration point,
  not `extract_props_from_options` itself.
- Factor `SvelteScopeProjection`'s TS-erasure classification into a shared,
  framework-neutral helper both projections call, rather than duplicating it
  for Vue.
- `verter_semantic/Cargo.toml` needs `oxc_semantic = { workspace = true }`
  added (already pinned at the workspace root; already used by
  `verter_compiler` for the structurally identical problem).
- Specify the concrete `Local`/`Indeterminate` carrier fields on
  `AnalyzedPropField`/`AnalyzedOptionsProp` and their path through to
  `SourcePosition`/`ComponentMetaOutputFailure` — not yet designed at the
  field level, only at the outcome level above.

## Amendment (v3) — after a second adversarial verification pass

A verification review of v2 (full text:
`docs/arch/refactor/rev11/evidence/CM1/binding-index-design-v2-review.md`)
confirmed the one-bind/span-correlation fix and the outer-driver placement
fix both hold, and found one materially important NEW defect plus several
smaller corrections. Per this repo's document-review cap (one round, plus one
verification pass when that round finds something substantive), this is
recorded as a targeted amendment rather than a third full rewrite — the
findings below are concrete, evidenced, actionable corrections appropriate to
resolve during implementation and its own code-level review, not further
design-document iteration.

### The important one: the wrapper's DEFAULT scope is right for defineExpose, wrong for the runtime-constructor argument specifically

Vue's real compiler does not leave a `defineProps`/Options `props:` runtime
argument sitting inside the setup() function it is textually written in: the
macro's object/array argument is extracted and relocated into the component's
`props` option, emitted in the options object BEFORE `setup()` runs
(`verter_compiler/src/script/macros.rs:398,431`,
`verter_compiler/src/script/process.rs:398/837,872`); imports are similarly
hoisted to true module scope regardless of which script block authored them
(`process.rs:192`). A setup-body-local, non-import declaration (`class String
{}` sitting as an ordinary statement beside `defineProps(...)`) is therefore
**never visible** to the emitted runtime-constructor expression — resolving
it as `Local` (as v2's wrapper would, since the identifier's AST node sits
inside the wrapper's function scope) is wrong in the opposite direction from
the original bug: it would treat a declaration Vue's own compiled output
cannot see as a shadow.

The fix does not discard the synthetic-function wrapper (it is still needed
for `defineExpose`-style consumers that genuinely run inside the compiled
`setup()` body, and for correctly containing instance-owned `var`/function
hoisting to that body rather than leaking to Program scope). It is two
narrow, structural corrections:

1. **Imports always stay Program-root siblings, regardless of owner.**
   When building the wrapper, `ImportDeclaration` and
   `TSImportEqualsDeclaration` top-level statements are NEVER moved into the
   instance wrapper even when `TopLevelOwnerTable` marks them
   `Instance` — they stay direct children of the synthetic Program, exactly
   matching Vue's real import-hoisting behavior the compiler already relies
   on (an ES import statement cannot syntactically live inside a function
   body in the first place).
2. **Per-consumer resolution start scope.** `resolve_value_identifier` takes
   an explicit start-scope parameter, not always "the scope the identifier's
   clone node landed in." A runtime-constructor identifier inside a
   `defineProps`/`defineModel`/Options `props:` argument always resolves
   starting from the **Program root scope** — never from the instance
   wrapper — because that is the scope Vue's compiler actually relocates the
   argument to. A `defineExpose` binding (which genuinely runs inside the
   compiled `setup()` body) resolves starting from its own owner's natural
   scope (the wrapper, for an instance owner). This is a per-call parameter
   on the query, not a second index.

Net effect for the runtime-constructor consumer specifically: an ordinary
instance-local (non-import) declaration is now NEVER shadow-relevant —
this is a clean, directly testable rule (add it as its own discriminating
test: a setup-local `class String {}` beside `defineProps({ items: String
})` must resolve `Global`, not `Local` — the inverse of the fixture v2's
matrix stated, corrected here).

### Smaller corrections

- **Preserve the retained parse's resolved `SourceType`, not the pre-parse
  parameter.** `SemanticBuilder` reads `Program.source_type`
  (`oxc_semantic-0.126.0/src/builder.rs:258`), which is the PARSER's
  resolved (possibly `Unambiguous`-disambiguated) kind, not the
  `source_type` value `build_script_analysis_inner` was called with
  (`build.rs:360`) — that parameter can be the pre-resolution/unambiguous
  request. Copy `program.source_type` onto the binding clone, never the
  caller's separate parameter.
- **Directives.** `Program.directives` are not covered by
  `TopLevelOwnerTable`, which is parallel only to `Program.body`
  (`top_level_owners.rs:232`). Classify each directive's owner via
  `TopLevelOwnerTable::owner_of_span` over its span and place it in the
  matching prologue: a module-owned directive stays in the clone's Program
  prologue; an instance-owned directive moves into the wrapper function
  body's own prologue. Do not let an instance-only `"use strict"` change the
  module owner's strictness or vice versa.
- **Non-binding wrapper.** Use an anonymous function EXPRESSION (e.g. the
  callee of an IIFE, or simply an unreferenced `FunctionExpression`), never a
  named `FunctionDeclaration` — a named declaration binds its own name in
  Program scope before entering its body (`oxc_semantic-0.126.0/src/builder.rs:1849`)
  and can create a synthetic, spurious collision with an unrelated authored
  name.
- **Direct-`eval` is strictness- and ancestry-aware, not a blanket flag
  read.** Sloppy-mode direct `eval` can inject a new `var`/function binding
  into the scope that lexically contains it; strict-mode direct `eval` gets
  its own inner scope and can never leak a binding outward. `Indeterminate`
  applies only when (a) the containing eval is sloppy-mode, AND (b) the
  queried reference's own scope-resolution walk passes through the scope
  that contains that eval. An eval in an unrelated sibling subtree, or a
  strict-mode eval anywhere, must not force `Indeterminate` — OXC propagates
  a raw `DirectEval` flag up the scope chain
  (`oxc_semantic-0.126.0/src/builder.rs:669,2498`); do not read that
  propagated flag directly without the strictness + ancestry check.
- **The admission-gate reuse claim was too strong.** `component_meta_binding_type_entries`
  / `eval_env.rs`'s existing gate is shaped specifically around `defineExpose`
  admission (`visible_value_binding` explicitly separates `Import` from
  `Local`, `eval_env.rs:924,942`; its output feeds `FieldKind::Binding`, not a
  prop type row). This block does not force a `Local` runtime-constructor
  resolution through that specific function. It adds its own call site that
  reuses the SAME underlying shared primitives that gate is built from
  (`PreparedValueDecl` resolution for a local declaration; the existing
  import-route resolver for an import) — fix at the lowest reusable
  PRIMITIVE layer, not by routing through a demand-shaped orchestration
  function built for a different field kind.
- **Fact/cache carrier.** `AnalyzedPropFieldFact`/`AnalyzedOptionsPropFact`
  (`verter_type_expr/src/facts.rs:2582,2686`) currently have no
  constructor-resolution carrier and must gain one alongside whatever field
  is added to the runtime `AnalyzedPropField`/`AnalyzedOptionsProp` structs,
  kept in sync the same way every other dual runtime/fact pair in this
  module already is.
- **Deterministic macro replay.** `lower_macro_field_payload_at_with_owners`
  (`macros.rs:763`) replays macro assembly through
  `analyze_macros_from_program_with_owners` without a source-type,
  parse-cleanliness signal, or an index available to it. The
  runtime-constructor resolution outcome is computed once at initial shallow
  analysis and stored on the analyzed field — the replay path reads the
  stored outcome, it never recomputes it.
- **Nullable constructor-array element — DEFER, do not silently decide.**
  Per CLAUDE.md's Fix Quality / explicit finding disposition rule: whether
  `{ type: [String, null] }` means "add `PrimitiveName::Null` to the union"
  is a real Vue-semantics question this design has not verified against Vue's
  own runtime behavior, and the prior deleted mechanism's own evidence packet
  flagged the same interpretation as unconfirmed rather than self-evident.
  Implement plain (non-`null`-bearing) constructor arrays now. For a `null`
  array element, DEFER: do not guess an interpretation — route it through the
  same `Indeterminate`-shaped failure channel as an unresolvable case, and
  record the deferral (owner: this evidence directory; resolution gate:
  before this block's final review) rather than silently landing a guessed
  semantics.
