# Semantic-difference ledger — Verter Semantic Signature Kernel (V0 registration)

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

V0 registers the ledger with the port evidence. Later blocks
re-classify rows; a row never silently disappears.

## Class 1 — exact agreement

| Subject | Evidence |
|---|---|
| The 344 `u6_flow_shape_corpus_rows_tests` checker columns | Re-measured on the installed TypeScript 7.0.2 executable on the implementing host: 344/344 byte-identical to the recorded columns (the `E01_spread_any` row verified through its `IsAny` leg: `any`→`null` is error-free, so the leg firing `true` is the print). The port from `tsgo 7.0.0-dev.20260526.1` changed no recorded checker value. |
| The oracle snapshot tree (93 files) | Regenerated with the pinned 7.0.2 engine via the `oracle_gen` binary: every snapshot rewrote byte-identically, zero stale files pruned. Recorded in `docs/evidence/signature-kernel/manifest.json` (`port_witness`). |
| Signature corpus `MatchesChecker` rows | NONE under the probe-driven observation basis. When the driver observed the WITNESS's own return (the pre-probe basis) `SV09`, `SV10`, `SV14`, `SV15`, `SV16`, `SV17` matched (`SV10`/`SV14` at the rendered-carrier level). The driver now observes the RECORDED PROBE through the type-position lane; the substrate defers every recorded probe's outer operator (measured `InstantiationRef(ReturnType)` / `InstantiationRef(Awaited)`), so those six rows are re-pinned `KnownOwed` (Class 4) — the recorded observations are unchanged, only the live verdict basis moved to the recorded question. |

## Class 2 — presentation-only

No rows at registration. The class opens when a row's printed forms
differ while the binding/callable effects provably agree.

## Class 3 — `VerterStableV1` order-induced (causal proof required)

No rows at registration, and none are possible before `VerterStableV1`
exists (V4). An admission needs the §5.8 counterfactual: only semantic
union traversal order changed, in an isolated cache namespace, and
restoring the stable order recovers the recorded observation.

## Class 4 — independent semantic difference / incompleteness

| ID | Subject | Both sides | Disposition |
|---|---|---|---|
| SDL-1 | Conditional labeled-break join (`flow_return_lexical_tests::flow_return_conditional_labeled_break_joins_the_write`) | tsgo `7.0.0-dev.20260526.1` printed `number | boolean`; TypeScript 7.0.2 `tsc` prints `number | true` (the `boolean` arm assignment-reduced to its `true` constituent). The Verter substrate still publishes the `boolean` arm. | Checker-version difference, recorded as `independent semantic difference` until classified. The test's oracle note records both strings; the Verter answer is re-pinned when the flow lattice models constituent reduction (V4 lattice work owns the re-pin). |
| SDL-2 | Intersection grouping witness `L` (`type L<T extends string> = (number & T) & { x: 1 }`) | TypeScript 5.8.3 (the contract's historical probe): `L<"a">` is `never`. TypeScript 7.0.2: `L<"a">` is `never`. | Agreement across versions; recorded because the contract's §6.2 recheck clause requires the oracle answer to be re-established, not assumed. Corpus row `SV12_grouping_witness_L`. |
| SDL-3 | Intersection grouping witness `R` (`type R<T extends string> = number & (T & { x: 1 })`) | TypeScript 5.8.3: `R<"a">` remained an UNREDUCED intersection representation. TypeScript 7.0.2: `R<"a">` reduces to `never`. | A checker-version semantic difference in reduction state: on 7.0.2 BOTH groupings agree. `independent semantic difference` until classified; the reduction-state witness binds V4's ReduceIntersection grouping rules (corpus row `SV13_grouping_witness_R`; the pair is pinned as a pair by `signature_corpus_records_the_7_0_2_grouping_witness_as_a_pair`). |
| SDL-4 | Recursive thenable (`interface Rec { then(onfulfilled: (v: Rec) => void): void }`) | TypeScript 7.0.2 refuses and recovers with `any`: the authored `Awaited<Rec>` is `any` under diagnostic TS2589 (`Type instantiation is excessively deep and possibly infinite.`), and the async position is `Promise<any>` under TS1062 (`Type is referenced directly or indirectly in the fulfillment callback of its own 'then' method`). No type is printed for the corpus probe. | Recorded as the observation itself (corpus row `SV21_awaited_recursive_thenable` carries the diagnostic as the checker column). DISPOSITION: the runtime protocol TERMINATES through the shared family cycle guard and publishes the typed gap with zero candidates; the lib conditional keeps the authored `Awaited<Rec>` application unreduced. An error recovery is not a type this substrate fabricates clean and warm, so the recorded diagnostic-only observation stands. Acceptance: `flow_return_coverage_tests::recursive_thenable_terminates_without_fabricating_a_type`. |
| SDL-5..27 | The signature corpus's twenty-three `KnownOwed` rows beyond SDL-2/3/4 | Recorded 7.0.2 observation vs. the current implementation's honest non-answer to the RECORDED PROBE (the type-position lane defers each probe's outer operator; the six formerly `MatchesChecker` rows — SV09/SV10/SV14/SV15/SV16/SV17 — are re-pinned here with the basis change, their recorded observations unchanged). | `independent difference/incompleteness` by construction — Verter's typed gaps, each row naming its owning successor block (V4/V5/V6/V7) in `crates/verter_session/src/signature_corpus_rows_tests.rs`. A row flips out of this class only through the corpus driver's re-pin rail (`signature_corpus_live_answers_follow_their_verdicts`; both flip directions are proven by `signature_corpus_flip_law_fires_in_both_directions`). |

## Unrecovered evidence

The reported 9,300-union / 56,548-observation corpus (measured on
`tsgo 7.0.0-dev.20260526.1`) was not recovered. No reproduction claim
is made; the new signature corpus carries its own identity
(`verter-signature-corpus-v0@typescript-7.0.2`). Recorded in
`manifest.json` (`unrecovered_report`).

The 7.1.0-dev nightlies are out of scope for this train (moving
target; their content-mapper work is unrelated).
