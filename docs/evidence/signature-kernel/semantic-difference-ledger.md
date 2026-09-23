# Semantic-difference ledger — Verter Semantic Signature Kernel

The four-class release authority of `docs/arch/signature-kernel.md` §5.8.
Every row recorded here is executable evidence: each names its subject,
both sides of the difference, and where the recorded bytes live. Classes:

1. **exact agreement** — Verter's completed observation equals the
   checker's byte-for-byte or through the one canonical checker-syntax
   projection.
2. **presentation-only** — the printed forms differ while every
   binding/callable effect agrees (verified, not assumed).
3. **`VerterStableV1` order-induced** — a difference attributable to
   semantic union traversal order, with the causal proof §5.8 demands
   (order-only counterfactual in an isolated cache namespace).
4. **independent semantic difference / incompleteness** — anything
   else, including checker-version-to-checker-version differences and
   Verter's typed gaps. Rows stay here until classified.

A row is re-classified only by moving it, never by deleting it. The
bundled `SDL-5..27` row that once stood for every owed corpus row was
SPLIT into `SDL-5`..`SDL-12`, one per owed capability, when the corpus
driver began asking its probes the question a consumer asks; no row left
the ledger in that split. `SDL-5` has since MOVED to Class 1: it was a
defect in the kernel's intersection-signature dedup, not an owed
capability. `SDL-6` has MOVED to Class 1 as well, once class expressions,
the checker's mixin rule and the instantiation of declared signatures
were implemented. The
corpus driver's flip law (`signature_corpus_flip_law_fires_in_both_directions`) makes the re-classification deliberate: a `MatchesChecker`
row fails when the live answer stops matching, and an owed row fails
when the live answer STARTS matching.

Companion evidence: [`determinism-matrix.md`](determinism-matrix.md)
(the §5.9 replay status) and [`performance-gates.md`](performance-gates.md)
(the §12 structural gates and what is not measured).

## Class 1 — exact agreement

| Subject | Evidence |
|---|---|
| The 344 `u6_flow_shape_corpus_rows_tests` checker columns | Re-measured on the installed TypeScript 7.0.2 executable on the implementing host: 344/344 byte-identical to the recorded columns (the `E01_spread_any` row verified through its `IsAny` leg: `any`→`null` is error-free, so the leg firing `true` is the print). The port from `tsgo 7.0.0-dev.20260526.1` changed no recorded checker value. |
| The oracle snapshot tree (93 files) | Regenerated with the pinned 7.0.2 engine via the `oracle_gen` binary: every snapshot rewrote byte-identically, zero stale files pruned. Recorded in `docs/evidence/signature-kernel/manifest.json` (`port_witness`). |
| Signature corpus `MatchesChecker` rows | **15 of 26**: `SV01`, `SV02`, `SV03`, `SV04`, `SV09`, `SV12`, `SV13`, `SV14`, `SV15`, `SV16`, `SV17`, `SV18`, `SV19`, `SV20`, `SV22` — the union-valued and mixed-arm `then` residuals, the construct-signature intersection instance side, the mixin construct intersection over a class expression, explicit type arguments, both 7.0.2 grouping witnesses, the transparent intersection group, and seven Awaited residuals. The live answers are structural matches of the recorded 7.0.2 observations, compared through the typed checker-syntax projection at the altitude the checker prints (utility applications reduced, named declarations kept by name). |
| `SV03_construct_intersection` (formerly `SDL-5`) | Moved from Class 4. `InstanceType<typeof CtorA & typeof CtorB>` is `B` on 7.0.2 and the live answer was `A`: the kernel's intersection-signature dedup compared signatures with their RESULTS IGNORED, so `new () => A` and `new () => B` collapsed onto the first. The checker's `appendSignatures` compares with `ignoreReturnTypes: false`, re-measured on the pinned 7.0.2 for both construct (`InstanceType`) and call (`ReturnType`) intersections, and for the converse: two signatures identical including their result still collapse. Held by `signature_kernel::discovery_tests::intersection_keeps_signatures_that_differ_only_in_their_result`. |
| `SV04_mixin_intersection` (formerly `SDL-6`) | Moved from Class 4. `InstanceType<ReturnType<typeof Mixin<typeof BaseCtor>>>` over `function Mixin<S extends new (...args: any[]) => Base>(Base: S) { return class extends Base { extra = 1; }; }` prints `Mixin.(Anonymous class) & Base` on 7.0.2; the live answer was a typed gap (`Opaque(Miss)`, degraded). Four missing pieces, each measured on the pinned 7.0.2: (1) a class expression in a function body had no type — the flow lane now composes its constructor type (construct signatures from the declared constructor, else the base constructor's; static members) over an instance that carries the class's own identity (`ClassExpressionInstance`, printed `Mixin.(Anonymous class)`) and its own members over the base instance, intersected with a type-variable base (`getBaseTypeVariableOfClass`); (2) `typeof f<T>` over a function DECLARATION was `Opaque(Miss)` — an instantiation expression now instantiates every signature the declaration carries whose type-parameter list accepts the arguments (`ReturnType<typeof G<string>>` is `string`); (3) `InstanceType` and `ConstructorParameters` read authored signature nodes only and refused the composed mixin candidate — they now read the shared candidate list's node form; (4) the kernel kept every mixin member's own construct signature — it now applies `resolveIntersectionTypeMembers`: a mixin constructor contributes no signature of its own and mixes its instance into the others', and when every constructor is a mixin the first keeps its signature (`typeof CtorB & typeof MixA` constructs `B & A` from `[s: string]`). Held by `signature_discovery_tests::mixin_construct_intersections_follow_the_checker_mixin_rule` and the `flow_return_coverage_tests` class-expression rows (`mixin_class_expression_composes_with_its_instantiated_base`, `declared_mixin_factory_instance_is_its_constructor_result_over_the_base`, `instantiation_expression_instantiates_a_declarations_signatures`). |

## Class 2 — presentation-only

No rows. The class opens when a row's printed forms differ while the
binding/callable effects provably agree.

## Class 3 — `VerterStableV1` order-induced (causal proof required)

No rows. `VerterStableV1` exists (`semantic_query/stable_key.rs`), so
the class is open, but nothing has been admitted to it. An admission
needs the §5.8 counterfactual: only semantic union traversal order
changed, in an isolated cache namespace (parents included), and
restoring the stable order recovers the recorded observation.

## Class 4 — independent semantic difference / incompleteness

| ID | Subject | Both sides | Disposition |
|---|---|---|---|
| SDL-1 | Conditional labeled-break join (`flow_return_lexical_tests::flow_return_conditional_labeled_break_joins_the_write`) | tsgo `7.0.0-dev.20260526.1` printed `number \| boolean`; TypeScript 7.0.2 `tsc` prints `number \| true` (the `boolean` arm assignment-reduced to its `true` constituent). The Verter substrate still publishes the `boolean` arm. | Checker-version difference, recorded as `independent semantic difference` until classified. The test's oracle note records both strings; the Verter answer is re-pinned when the flow lattice models constituent reduction. |
| SDL-2 | Intersection grouping witness `L` (`type L<T extends string> = (number & T) & { x: 1 }`) | TypeScript 5.8.3 (the contract's historical probe): `L<"a">` is `never`. TypeScript 7.0.2: `L<"a">` is `never`. | Agreement across versions; recorded because the contract's §6.2 recheck clause requires the oracle answer to be re-established, not assumed. Corpus row `SV12_grouping_witness_L` is `MatchesChecker`: the live rail answers `never` too, so this row records only the VERSION history, no live difference. |
| SDL-3 | Intersection grouping witness `R` (`type R<T extends string> = number & (T & { x: 1 })`) | TypeScript 5.8.3: `R<"a">` remained an UNREDUCED intersection representation. TypeScript 7.0.2: `R<"a">` reduces to `never`. | A checker-VERSION difference only; against the pinned 7.0.2 oracle both groupings agree and corpus row `SV13_grouping_witness_R` is `MatchesChecker` (the live rail answers `never`). The reduction-state witness still binds the `ReduceIntersection` grouping rules, and the pair is pinned as a pair by `signature_corpus_records_the_7_0_2_grouping_witness_as_a_pair`. |
| SDL-4 | Recursive thenable (`interface Rec { then(onfulfilled: (v: Rec) => void): void }`) | TypeScript 7.0.2 refuses and recovers with `any`: the authored `Awaited<Rec>` is `any` under diagnostic TS2589 (`Type instantiation is excessively deep and possibly infinite.`), and the async position is `Promise<any>` under TS1062 (`Type is referenced directly or indirectly in the fulfillment callback of its own 'then' method`). No type is printed for the corpus probe. | Recorded as the observation itself (corpus row `SV21_awaited_recursive_thenable` carries the diagnostic as the checker column). DISPOSITION: the substrate does not fabricate a recovery type — the structural-fact demand measures `Opaque(Miss)`, a typed gap, and corpus row `SV21` pins that NON-ANSWER. Which mechanism terminated it (the shared family cycle guard) is NOT established by this row; the lib conditional keeps the authored `Awaited<Rec>` application unreduced. An error recovery is not a type this substrate fabricates clean and warm, so the recorded diagnostic-only observation stands. Acceptance: `flow_return_coverage_tests::recursive_thenable_terminates_without_fabricating_a_type`. |
| SDL-7 | Binder-space default application (`SV05_generic_default`, `SV06_default_references_binder`) | 7.0.2 applies the declared default (`WithDefault<string>`, `Chain<number, number[]>`); the consumer-expanded answer is the bare alias reference (measured `DeclRef(WithDefault)`) or the alias's structural expansion (measured `{ self: number, others: Array(number) }`). | Owed by the binder-space default application. |
| SDL-8 | Predicate and assertion signature projection (`SV07_type_predicate`, `SV08_assertion_signature`) | 7.0.2 prints the predicate/assertion callable; the consumer-expanded answer is an EMPTY surface (measured `{  }`, degraded) — callable and predicate both lost. | Owed by the `SignaturesOfType` result projection. |
| SDL-9 | Alias-applied display through a type-position instantiation (`SV10_nested_instantiation`) | 7.0.2 prints the alias-applied `G<boolean>`; the consumer-expanded answer is the alias's EXPANSION (measured `{ f: Array(boolean) }`). The semantic content agrees; the alias-applied FORM does not. | Owed by the `SignaturesOfType` result projection. Re-examine against Class 2 (presentation-only) once the callable/binding effects are verified equal — it is recorded here, not there, because that verification has not been done. |
| SDL-10 | Constrained substitution through an indexed access (`SV11_constrained_substitution`) | 7.0.2 reduces `T['a']` over `T extends { a: 1 }` to the literal `1`; the consumer-expanded answer is an EMPTY surface (measured `{  }`). | Owed by the constrained-substitution stage. |
| SDL-11 | Deferred-symbolic and constrained-generic Awaited (`SV23_awaited_deferred_generic`, `SV24_awaited_constrained_generic`) | The recorded claims live in the declaration bytes (a display-only `unknown` column); the consumer-expanded answer is `unknown`, which does not carry the declared-return structure, and a typed gap for the constrained async wrap (measured `Opaque(Miss)`). | Owed by the runtime/lib `Awaited` lane through the constraint. |
| SDL-12 | Authored-precedence union ordering (`SV25_generic_union_dedup`, `SV26_literal_generic_union_order`) | 7.0.2 dedups preserving FIRST occurrence and prints literals-first-then-binder; SV25 dedups but REVERSES the authored arm order (measured `B \| A`); SV26's recorded basis is its declared return `"a" \| "b" \| T` (compared order-sensitively, its checker column being a display-only `unknown`), and the consumer-expanded answer is `unknown` — the binder at its constraint, without the declared order. | Owed by `ReduceUnion` authored-precedence ordering. **Not** admitted to Class 3: an order difference is Class 3 only with the §5.8 causal counterfactual, and this one is an unimplemented ordering rule, not a demonstrated `VerterStableV1` traversal-order effect. |
| SDL-13 | Semantic policy on the flow-return lane (`strictNullChecks`) | Measured on the pinned 7.0.2: `function leaf(v?: string) { return v; }` returns `string \| undefined` with `strictNullChecks` on and `string` with it off, and `(v: string \| null) => v` returns `string` when off. The live flow-return answer is `string \| undefined` and `string \| null` under BOTH settings, on a fresh host whose project provably carries the setting. | `independent difference/incompleteness`: the flow lattice does not model the policy's erasure of `undefined`/`null`. Not a signature-kernel capability; it is also why `DET-09` cannot claim Ready — no answer depends on the policy, so a stale resident parent is unobservable. Owner to be ruled. |

## Unrecovered evidence

The reported 9,300-union / 56,548-observation corpus (measured on
`tsgo 7.0.0-dev.20260526.1`) was not recovered. No reproduction claim
is made; the new signature corpus carries its own identity
(`verter-signature-corpus-v0@typescript-7.0.2`). Recorded in
`manifest.json` (`unrecovered_report`).

The 7.1.0-dev nightlies are out of scope for this train (moving
target; their content-mapper work is unrelated).
