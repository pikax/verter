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
capability. `SDL-7`, `SDL-9`, `SDL-10` and `SDL-11` have MOVED to Class 1
as well: two were the corpus lane printing below the checker's altitude,
one a path-walker defect, and one a signature-utility defect plus a
display-only flag the checker column never needed. The
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
| Signature corpus `MatchesChecker` rows | **20 of 26**: `SV01`, `SV02`, `SV03`, `SV05`, `SV06`, `SV09`, `SV10`, `SV11`, `SV12`, `SV13`, `SV14`, `SV15`, `SV16`, `SV17`, `SV18`, `SV19`, `SV20`, `SV22`, `SV23`, `SV24` — the union-valued and mixed-arm `then` residuals, the construct-signature intersection instance side, both generic-default applications, explicit type arguments, the nested alias instantiation, the constrained indexed access, both 7.0.2 grouping witnesses, the transparent intersection group, and nine Awaited residuals. The live answers are structural matches of the recorded 7.0.2 observations, compared through the typed checker-syntax projection at the altitude the checker prints (utility and conditional applications reduced; interfaces, classes and the alias applications the checker names kept by name, with omitted defaulted arguments filled). |
| `SV03_construct_intersection` (formerly `SDL-5`) | Moved from Class 4. `InstanceType<typeof CtorA & typeof CtorB>` is `B` on 7.0.2 and the live answer was `A`: the kernel's intersection-signature dedup compared signatures with their RESULTS IGNORED, so `new () => A` and `new () => B` collapsed onto the first. The checker's `appendSignatures` compares with `ignoreReturnTypes: false`, re-measured on the pinned 7.0.2 for both construct (`InstanceType`) and call (`ReturnType`) intersections, and for the converse: two signatures identical including their result still collapse. Held by `signature_kernel::discovery_tests::intersection_keeps_signatures_that_differ_only_in_their_result`. |
| `SV05_generic_default`, `SV06_default_references_binder` (formerly `SDL-7`) | Moved from Class 4. The semantics already agreed: `Instantiate` binds an omitted parameter to its declared default, instantiated under the arguments before it (`WithDefault` resolves to `{ value: string }`, `Chain<number>` to `{ self: number; others: number[] }`). The difference was the comparison altitude. 7.0.2 names an alias application the alias constructs by the alias, with the omitted defaults filled (`WithDefault<string>`, `Chain<number, number[]>`), while the corpus lane stopped at the bare reference (`DeclRef(WithDefault)`) or reduced the application to its expansion. The lane now prints at the checker's altitude: the alias application, its defaults filled through the same binding `Instantiate` applies. Held by `signature_corpus_tests::the_print_altitude_names_declaration_applications_as_the_checker_does` (18 prints measured on 7.0.2: object, function, mapped, array, tuple, union and intersection aliases by name; conditional, bare-parameter and primitive aliases, and a union collapsed to one member, as what they resolve to; a generic interface with its default filled). |
| `SV10_nested_instantiation` (formerly `SDL-9`) | Moved from Class 4. The same altitude rule: `type G<U> = F<U[]>` constructs its type through `F`, and 7.0.2 names the OUTERMOST alias, `G<boolean>`; the expansion `{ f: boolean[] }` already agreed. The lane prints the alias application, so no presentation-only difference remains to classify. |
| `SV11_constrained_substitution` (formerly `SDL-10`) | Moved from Class 4. Not a constraint defect: call-site inference binds `T` to `{ a: 1; extra: string }`, and the witness return `{ a: 1; extra: string }['a']` reduced to `1` under every demand except a Shallow one (`published(Shallow)` and the relation engine's `structural_transit()`). The path walker ran the EMPTY-path surface synthesis on a projected path's terminal, so every surfaceless member type (`1`, `string`, `string[]`, `[1, 2]`, `{ b: 1 } \| 1`, `() => void`) became the empty surface `{}`, relation reads included. A projected terminal is now its own answer unless it has a surface, and a reference whose body contributes none keeps the reference. Held by `project_semantic_dispatch::projected_terminal_surface_tests`. |
| `SV23_awaited_deferred_generic`, `SV24_awaited_constrained_generic` (formerly `SDL-11`) | Moved from Class 4. `SV23`'s checker column was never display-only: `ReturnType` of a generic signature reads the checker's base signature, each type parameter at its constraint (`unknown` when unconstrained), so `Awaited<unknown>` = `unknown` IS the answer and the row now compares against it. `SV24` had two defects. The signature utilities instantiated every type parameter at `unknown` and ignored its constraint (`ReturnType<typeof f>` for `f<T extends string>(v: T): T` was `unknown`; 7.0.2 prints `string`). And instantiating a builtin runtime nominal (`Promise<number>`, `Date`, …) produced a miss instead of its own application carrier, in production `normalize_node_for_structural_fact_demand` as well as in the lane. Both utility routes (the whole result and the `ReturnType<…>['k']` member lookup) now read the base signature: sibling constraints substituted for `N - 1` rounds, what is still referenced erased to `any`, a circular constraint (TS2313) dropped, a default never applied, each re-measured on 7.0.2. A runtime nominal instantiates to its carrier, which the callable realizer and the event-name enumerator read as a complete non-callable, as they read the miss. Held by `project_semantic_dispatch::base_signature_tests`, `component_meta_flow_return_admission_tests::return_type_member_of_constrained_callee_publishes_the_base_constraint` and the builtin-nominal cases in `meta_resolve::callable_view::tests`. |

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
| SDL-6 | Mixin construct intersection (`SV04_mixin_intersection`) | 7.0.2 prints `Mixin.(Anonymous class) & Base`; the consumer-expanded answer is a typed gap (measured `Opaque(Miss)`, degraded). | Owed by the `SignaturesOfType` construct-signature/mixin semantics. |
| SDL-8 | Predicate and assertion signature projection (`SV07_type_predicate`, `SV08_assertion_signature`) | 7.0.2 prints the predicate/assertion callable; the consumer-expanded answer is an EMPTY surface (measured `{  }`, degraded) — callable and predicate both lost. | Owed by the `SignaturesOfType` result projection. |
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
