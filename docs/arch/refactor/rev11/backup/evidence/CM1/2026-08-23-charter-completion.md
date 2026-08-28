# CM1 charter completion — 2026-08-23

The three charter exits that survived the 2026-08-23 follow-on (`571ad4dd1`),
each with the code that closes it and the evidence that discriminates it.

## Gap 1 — producer-owned typed runtime-constructor fact (`CM1.md:110`)

**What was wrong.** `ConstructorBindingEntry` carried `spelling: Arc<str>` — the
identifier's raw source text. Two independent downstream string switches then
re-derived meaning from it:

- `component_meta.rs`'s `primitive_of`: `match spelling { "String" => …,
  "Number" => …, "Boolean" => …, "null" => … }`
- `macros.rs`'s `constructor_to_ts_type`: `match name { "String" => "string", … }`

`ConstructorBindingOutcome` is a typed *binding-resolution* outcome
(`Global`/`Local`/`Indeterminate`) — not typed constructor identity, and it does
not satisfy the charter item. Re-reading a type name below the producer is also
what the typed-IR-only rule in `CLAUDE.md` forbids.

**What closes it.** `verter_type_expr::facts::RuntimeConstructorIdentity`: a
closed enum over the ten `@vue/runtime-core`-recognised constructors, plus
`NullLiteral` (the literal `null` array element), `Other(Arc<str>)` (an
identifier naming none of them — display/diagnostics only) and `Unclassifiable`
(a spread, a computed expression, an elision).

- **Minted once, at the producer.** `RuntimeConstructorIdentity::classify` is the
  sole place a spelling is read as text, and it is called only from
  `root_binding_index::resolve_constructor_binding`.
  `macros::resolve_runtime_constructor_array` mints `NullLiteral` and
  `Unclassifiable` directly, since those positions have no identifier at all.
- **Matched exhaustively downstream.** `identity.primitive()`,
  `identity.display_ts_type()` and `identity.spelling()` each `match` every arm;
  adding an arm is a compile error at all three.
- **Structurally confined, with the residue named.** `ConstructorBindingEntry` no
  longer *has* a `spelling` field, so a consumer that reaches for the old string
  route gets an `E0609` rather than a review catch. That is the rail, and it is
  the closed-enum-matched-exhaustively form the charter's "Structural
  confinement" section requires, not a name-keyed source scanner.

  It is **not** a proof that reclassification is unwritable. `classify`
  (`facts.rs:248`) and `spelling()` (`facts.rs:313`) are both `pub`, because the
  producer lives in a different crate from the fact, so an explicit
  `classify(identity.spelling())` round-trip would compile. What the rail buys is
  that such a round-trip is a **visible new call**, not an accidental survival of
  the old route. Making it unrepresentable would need the mint confined behind a
  crate-private gateway — larger than this block owns, no owner assigned, and
  recorded here as accepted residue. No such call site exists today.

`classify("null")` deliberately yields `Other("null")`, not `NullLiteral`: it
reads *identifier* spellings, and `null` is a literal. The producer mints
`NullLiteral` for the literal element directly, so the two can never be conflated
by a caller that happens to have the string `"null"` in hand.

## Gap 2 — the Finding-C public acceptance matrix (`CM1.md:165`)

Analyzer- and fold-level coverage was already strong
(`component_meta_tests.rs`, `root_binding_index_tests.rs`); the *public* half was
narrow. Two new files prove the literal cross at the public boundary:

- `crates/verter_session/tests/cases/runtime_constructor_matrix.rs` — native.
- `packages/component-meta/test/runtime-constructor-matrix.test.ts` — compat,
  over on-disk fixtures carrying the same sources as the native cells (the
  native cells are inline string literals, so the two are the same source text,
  not the same bytes).

Axes covered per the charter table:

| Axis | Cells |
|---|---|
| runtime form | shorthand, expanded, `required: true`, optional, with `default`, two- and three-element constructor arrays, nullable in both orders (`[String, null]`, `[Number, null]`), and a MIXED array whose elements do not all carry a primitive fact. Mixed runtime + type-declared is **captured**, not covered: the recorded execution described below, rather than standing cross-mode coverage, observed the runtime siblings fold exactly across all invocation and view modes and the authored field reproducibly publish native `Primitive(Unknown)` and compat `"unknown | undefined"`; this deferred-capture evidence is owned by the maintainer's post-program type-correction work (`runtime_prop_as_function_assertion_publishes_its_object_shape`, native and compat) |
| constructor kind | `String`/`Number`/`Boolean` positive; and as negative controls: the seven recognised constructors with no primitive fact (`Array`/`Object`/`Function`/`Symbol`/`Date`/`RegExp`/`Promise`), a module-owned or imported custom class, and a locally shadowed spelling. `PropType<T>` and `<script setup>`-local custom classes are excluded from the control set because both publish `unknown`; they live in the deferred captures `prop_type_assertion_publishes_its_object_shape_on_both_prop_routes` and `script_setup_local_class_publishes_the_class_constructor_shape` |
| binding origin | global; module-owned local shadow; IMPORTED value (both a recognised constructor spelling and a non-constructor spelling, so origin is discriminated from name) |
| extraction path | `defineProps` carries the full five-kind × four-form cross; Options-API `props:` independently carries the positive constructors, required/default/array forms, the display-only and custom-class controls, local shadowing, and imported values |
| invocation | cold, warm, sequential, concurrent (`Promise.all`-equivalent), native batch, compat `batch/checker`, and compat `batch/session` |
| request-view scope | base session and overlay session, including overlay × checker-batch on the compat checker surface |
| surface | native, `@verter/component-meta/compat` |

That recorded execution predates conversion of the form into deferred
captures. With the form briefly live, it failed five native tests — the cold
lane, cold/warm lane, sequential/concurrent lane, scalar/batch lane, and
overlay lane — and seven compat tests spanning cold, warm, concurrent, checker
batch, session batch, overlay, and checker-overlay. In every failure the same
loop had already asserted the exact folds of the runtime siblings `label`,
`count`, and `flag` before execution reached the failing `item` assertion. The
authored value was native `UnknownPrimitive` (`TypeExpr::Primitive(Unknown)`,
reported above as `Primitive(Unknown)`) and compat `"unknown | undefined"` in
every mode. The two standing captures are ignored/skipped and each performs one
base/scalar read; they preserve the defect but do not provide standing
cross-mode coverage.

The invocation and request-view axes are a genuine **cross**, not independent
demonstrations. `matrix_overlay_lane_runs_the_full_invocation_cross` drives every
cell through the overlay lane cold, warm, sequentially, concurrently AND in
batch, and asserts each against both its own expectation and the base-lane
answer. A result correct in the base store but wrong (or mode-dependent) under a
request view is exactly the request-view defect class the acceptance criteria
name, and a base-only cross cannot see it.

On the compat surface the warm axis uses ONE shared checker for the whole file,
so a second read is a genuine warm read of the same session. A per-call checker
compares two cold sessions and cannot observe a warm-vs-cold divergence at all —
that was a real hole in the first draft of this file. The compat suite now has a
real `batch/checker` mode over `checker.getComponentMetaBatch`, a separate
`batch/session` mode over `ComponentMetaSession.getComponentMetaBatch`, and a
dedicated checker test crossing a session-only overlay with cold, warm,
concurrent, and checker-batch reads.

Two independent properties are asserted per cell: **exactness** (the published
type is exactly the required closed primitive/union, and `required`/`has_default`
are exactly as authored) and **invocation invariance** (every mode publishes an
identical surface — a mode-dependent answer fails even when each mode is
individually plausible).

The negative controls assert THREE independent properties: the position still
**publishes** a materialized type; the type is not one of the four
primitive-fold shapes; and it equals the route's exact pinned shape/rendering.
The first prevents absence from satisfying "not a primitive fold"; the third
prevents a control from staying green while degrading into a different non-fold
shape, especially an `unknown`-shaped loss.

## Gap 3 — the `UnraisableSource` fixture asserts the exact variant

`exposed_binding_regression.rs`'s fixture proved current success through
`.expect(...)`. It never captured the failure channel, so it could not
distinguish the pre-repair producer defect from any other error.

It now matches the `Result` explicitly and separates three outcomes:

- `Err(_)` — a DIAGNOSIS rail, not an oracle, and the code says so. The
  correct answer for this input is SUCCESS, so every `Err` is red whatever it
  carries: on this input no check on the lane or variant can change the
  verdict, and calling one a discriminator would be false. The arm classifies
  the observed failure, reports whether it REPRODUCES the known pre-repair
  triple (`Exposed` / `UnraisableSource`) or is a different one, and names
  `navigate_value_parts` in
  `crates/verter_session/src/decl_body_memo/locator_deref.rs` as the owning
  producer. That is worth keeping — it is the difference between a five-minute
  triage and an hour of one — but it prevents nothing on its own.

  The verdict-bearing exact-variant assertion lives instead in
  `runtime_constructor_matrix.rs::fail_closed_constructor_positions_surface_the_exact_typed_failure`,
  on two inputs where a typed failure is the CORRECT answer (a namespace
  import, which binds no single exported value; and an import whose module
  does not resolve). There the exact `(lane, failure)` pair genuinely
  discriminates: a different lane, a different `ComponentMetaOutputFailure`, a
  success, or a collapse into absence each fail the test.
- `Ok(None)` — the forbidden swallow (a failure demoted to absence).
- `Ok(Some(_))` — the repaired tree; the field's own materialized shape is then
  pinned so a per-field degrade cannot hide behind an overall-successful resolve.

**Proven by plant.** Deleting the lone-signature recovery in
`navigate_value_parts` drives the fixture into the `Err` arm, where BOTH
exact-variant assertions execute and hold, and the named regression message
fires with the full typed error:

```
REGRESSION: the `defineExpose`-bound function declaration reproduced the exact
pre-repair producer failure (Exposed / UnraisableSource). ... ComponentMetaOutputError
{ lane: Exposed, index: 0, ..., failure: UnraisableSource }
```

**Honest limitation, stated plainly.** Those two `assert_eq!`s sit inside an
`Err` arm that then panics unconditionally, so they sharpen the *diagnosis* but
do not change the test's pass/fail bit — every `Err` is red regardless of
variant. That is inherent to characterising a *repaired* defect: there is no live
failure left to assert against. What keeps the assertion non-vacuous is that the
machinery which produces the variant is still armed and positively asserted
elsewhere — `meta_resolve::projectors::define_shapes_tests` and
`verter_session::meta_tests` each assert
`ComponentMetaOutputFailure::UnraisableSource` on a position that genuinely
cannot raise.

## What is deliberately not done here

- No dispatch context packet is reconstructed. None ever existed;
  `context_packet_digest` stays empty and grandfathered.
- The historical `BLOCK` review verdict is preserved as a fact and is never
  relabelled `PASS`.
- `accepted_sha`, `accepted_tree` and `maintainer_decision` stay empty/`PENDING`.
  Acceptance is maintainer-only (`governance.md` §1.1).

## Round-2 blocker closures

### `exposed-error-not-asserted` — RETIRED

On an input whose correct result is success, every `Err` already fails, so
comparing error variants cannot further discriminate the verdict. The finding
is therefore **RETIRED** as framed. The verdict-bearing evidence lives in
`runtime_constructor_matrix.rs::fail_closed_constructor_positions_surface_the_exact_typed_failure`,
where the exact typed failure is the correct result, and in the sibling
[`mutation-receipts.md`](mutation-receipts.md) B2 receipt that makes that test
red by collapsing the failure source.

### B1 — imported value bindings route through the shared value-export authority

**Tier 3 (plant-proven).** The first fix resolved an import alias with a
two-branch helper: the name-only owner-import surface for the ordinary owner,
and the prepared bundle's direct hop for any other owner. The second branch was
a second import path: it stopped at the first hop, observed no chain facts, and
stamped `ordinary_file()` on the result regardless of the target's real owner.

`VerterHost::resolve_binding_import_origin` now composes three existing
authorities and re-implements none of them:

1. `ShallowFileState::import_target_in(owner, name)` — the OWNER-QUALIFIED
   import table. The name-only surface is unusable here for the reason
   `project_semantic_dispatch::walk`'s `InstanceTypeOf` arm already documents:
   it can select a sibling script/setup binding sharing a local spelling.
2. `ResolverContext::resolve_type_dependency_canonical` — specifier to canonical.
3. `VerterHost::resolve_value_export_target_graph_native` — the shared
   VALUE-export authority. It walks the export route, peels the value alias
   chain, observes each chain participant's version facts into the active fact
   tracer, and returns the target's **own** owner.

Ownership is therefore read from the target, never fabricated, and a barrel or
alias re-export resolves to its true root. A namespace import is refused
explicitly rather than anchored onto a body that does not exist.

Discriminated by a new cell, `/rcm/ImportedBarrel.vue`, which reaches its values
through a barrel that declares nothing. Plant `B1` restores the superseded
direct-hop behaviour; the cell fails with the anchor stranded on the barrel:

```
/rcm/ImportedBarrel.vue: ... anchor: AuthoredAnchor {
  canonical_id: "/rcm/imported-barrel.ts", owner: Module(0), symbol: "String" },
  failure: UnraisableSource
```

Tiers 1 and 2 are not reachable from this block: preventing the wrong path
structurally would mean sealing `PreparedDeclBundle`'s owner-scope import map,
which has other consumers and is not this block's to seal.

### B2 — a verdict-bearing exact-variant assertion now exists

**Tier 3 (plant-proven).** The round-1 charge was right that
`exposed_binding_regression.rs`'s `Err` arm cannot discriminate: for an input
whose correct answer is success, every `Err` is red whatever it carries. That is
a property of the input, not a defect, and the test now says so.

The strengthening is real and lives elsewhere:
`runtime_constructor_matrix.rs::fail_closed_constructor_positions_surface_the_exact_typed_failure`
pins the exact `(lane, failure)` pair on two inputs where a typed failure is the
CORRECT answer — a namespace import (binds the module object, not one exported
value) and an import whose module does not resolve. Both must be exactly
`(Prop, UnraisableSource)`. A different lane, a different
`ComponentMetaOutputFailure`, a success, or a collapse into absence each fail it.
That is what "discriminating" means here, and it is now landed rather than only
reachable under a plant.

### B3 / B4 — claims narrowed to the mechanism

**Tier 4 (documentation), and labelled as such.** Correcting an overclaim is
documentation work; it prevents nothing on its own. The substance behind B3 is
carried by B2's landed test.

- B3: the evidence record described the `Err` arm as asserting the exact
  variant. It does not — it classifies and reports. Rewritten to name it a
  diagnosis rail and to point at the verdict-bearing test.
- B4: `.claude/skills/component-meta/SKILL.md` claimed a downstream
  reclassification is a compile error. It is not: `classify` and `spelling()`
  are `pub` and re-exported at the crate root, so
  `classify(identity.spelling())` compiles. Narrowed to what the rail actually
  buys — the old `entry.spelling` route is an `E0609`, and a round-trip is a
  visible new call rather than an accidental survival — with the residue named.

### `analysis-wire-shape` — assessed, no compatibility contract

**Tier 4 (recorded assessment).** The `spelling` -> `identity` field rename does
change `getAnalysis` JSON, and old JSON no longer decodes. Evidence that this
surface carries no compatibility contract:

- `getAnalysis` is a `serde_json::to_string` passthrough of the internal
  `FileAnalysisSnapshot`. Its own rustdoc (`crates/verter_napi/src/lib.rs:1848`)
  states that typed NAPI structs were deliberately not defined because the
  method "is primarily used by the playground" — a debug surface, not a DTO.
- The analysis snapshot carries no `schema_version`.
- No generated TS binding pins the field; contrast `packages/types/audit.generated.ts`
  and `typeinfo.proto`, which do have that discipline.
- No consumer anywhere in `packages/` reads a `spelling` field off this JSON
  (the only textual hits are the English word in comments and docs).
- `getAnalysis` is not part of the closed Typeinfo Wire Contract, which is the
  surface `CLAUDE.md` gives reserved-tag and schema-version rules to.

So the rename is not a breaking change against a contract; there is no contract.
Recorded rather than versioned.

### `disp-uncovered` — the display mapping is now pinned exactly

**Tier 3 (plant-proven).** `display_mapping_covers_exactly_the_ten_constructors`
asserted only `is_some()` for seven of ten constructors while its name promised
exact coverage — so corrupting `Date`'s display text left the whole workspace
green. It now pins all ten to their exact strings, and asserts the pin table is
the same length and order as the recognised table so an arm cannot be dropped
from the pin. Plant `D` (the adversarial leg's own probe, reproduced) now fails:
`` `Date` must display EXACTLY as `Date` ... left: Some("zzplantd_regexp") ``.

### B5 — the compat PASS is now source-bound, with the binding shown

**Tier 4 for the procedure; the behavioural half is tier 3.** The earlier compat
PASS was taken against `packages/native/dist/verter-native.darwin-arm64.node`
dated before the freeze. That artifact predated the import re-anchor, so the run
described a different tree. Withdrawn.

How the binding was established this time, rather than asserted:

1. The tree was committed and verified clean (`git status --porcelain` empty)
   BEFORE the build, and the source SHA recorded: `09ffb5b0d`.
2. The old artifact was deleted, then rebuilt with
   `pnpm --filter @verter/native build` (exit 0, zero `error` lines in the log).
3. **Nothing in `crates/` or `packages/` changed between the build SHA and the
   candidate.** `git diff --stat 09ffb5b0d..361364056 -- crates/ packages/` is
   EMPTY — the only commits since the build touch `docs/` and
   `.claude/skills/`. So the binary's Rust and TS sources are byte-identical to
   the candidate's, which is the property that matters; identical bytes in, not
   merely "built recently".
4. Artifact identity recorded: sha256
   `0d73ff2fc40e10ea2f4cb35a7b9c6106fa27c440f33b8b89ece51b6fb75ae959`,
   mtime `2026-08-24T16:19:28`, size 33018272.
5. **Behavioural binding, which a hash cannot give.** The suite contains
   `RcImportedBarrel.vue`, which resolves only through the B1 fix. A binary
   built before that fix fails that cell with the anchor stranded on the barrel.
   Its passing is positive evidence that the loaded binary contains this
   candidate's code, independent of any file metadata.

```
$ pnpm vitest --run packages/component-meta/test/runtime-constructor-matrix.test.ts
Test Files  1 passed (1)
     Tests  5 passed (5)
```

Standing consequence: a compat result is only meaningful once the binding above
is re-established. Any compat conclusion taken against an unrebuilt binary is
withdrawn, not carried.

### B6 — the compat cross matches the native cross

**Tier 3 for the cells (plant-proven); the parity itself is structural.** Compat
cells went 11 -> 12, one-for-one with the native matrix cells, adding
`RcImportedBarrel.vue` + `rc-imported-barrel.ts`.

The overlay lane now runs the FULL invocation cross rather than a single scalar
read: cold, warm, concurrent and batch, each asserted for its own exactness AND
for agreement with the base lane. It runs on `ComponentMetaSession`, which
carries both halves — `updateFile` publishes the session-local overlay and
`getComponentMetaBatch` is the real batch entry — so overlay x batch is a
genuine cell rather than a scalar loop relabelled.

Every mode routes through the one `assertCell` authority, so no mode is held to
a weaker contract than another, and each asserts the published prop COUNT as
well as each prop's exact rendering.
