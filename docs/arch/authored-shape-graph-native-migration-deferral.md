# Authored-Shape & Graph-Free Declaration-Body Readers — Final-State Note (deferral CLOSED)

**Status:** the reader-class deferral is closed, and the two memoized type-parameter-bound pockets are CLOSED (the type-parameter-bound confinement block landed): `LoweredTypeDecl` is wholly `NoTypeExpr` and `TypeParamBinding` is the content-free `(name, ordinal)` fact pair.

**Terminal invariant (target, satisfied on this surface):** zero stored/hot `TypeExpr` and zero post-lowering semantic decisions over `TypeExpr`. Permanent producer boundaries may transiently consume lease-only authored `TypeExpr` to mint graph, fact, or locator outputs, but may not retain it. The two former violations are CLOSED: the memoized `LoweredTypeDecl.type_parameters: Vec<TypeParam>` storage is deleted — the `narrow_type_parameters` mirror (name + ordinal + content-free bound locators) is the sole stored authority, the locator/binder deref re-borrows bound CONTENT + the full sibling frame lease-only via `transient_type_parts`, and the external frontier derives its narrow output from the mirror with a content-free re-anchor of bound slots to the frontier symbol (`export default` behavior preserved); and `TypeParamBinding` is shrunk to `(name, ordinal)` (`NoTypeExpr`), its `<script setup generic="…">` bounds re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers — a missing/stale re-borrow is a typed cache-suppressed miss, never a bound-free fabricated binder.

**Open question — DECIDED.** The earlier recorded question (whether the authored-shape readers are
PERMANENT split-carrier compat vs eventual full graph-native migration) is decided: **permanent
ingress**. The producer bridge reads authored syntax to lower it; every semantic decision runs on
nodes/facts downstream. No full graph-native migration of the producer boundary is planned or
required. The round-3 body-reader confinement narrowing also stands: global `.body` /
`.type_annotation` field scanning stays REJECTED as a confinement proof, which is why the live
guard is a curated ratchet/census rather than a structural-confinement proof.

## The reader classes at the terminal state

The live census is the curated ratchet in
`crates/verter_source_policy_gate/tests/cases/residual_type_expr_body_reader_inventory.rs` (a curated
inventory/ratchet, NOT terminal confinement): 1 `GraphBackedMigrated` + 6 `ProducerLowering` +
0 `AuthoredShape` + 5 `GraphFreeDto` + 0 `GraphBackedPending`.

- **`ProducerLowering` (6 rows) — PERMANENT, not on a path to zero.** The authored-syntax→graph
  bridge: the lowering mint (`lower_decl_body_with_provenance`), the two prepared-decl assembly
  tails (`finish_prepared_type_decl`, `prepare_local_value_decl`), the locator-deref shape
  assembler (`transient_body_shape`), and the two closedness recipe-escape supply lines
  (`deref_slot_body`, `lower_body_under_env`). The mint, the shape assembler, and the supply lines
  transiently consume lease-only authored `TypeExpr` to lower it into graph IR or content-free
  carriers (decision-free structural transit — every closedness verdict runs on nodes in
  `OpenWalk`); the two assembly tails read content-free slot carriers (`TypeDeclBody` merge-shape
  / `ValueTypeAnnotationFact`), not `TypeExpr`.
- **`AuthoredShape` (0) — DONE.** The heritage candidates and the closedness/key-domain cluster
  went fact-native (`HeritageBaseFact` / `KeyDomainClosednessFact` minted at lazy decl-body
  lowering, evaluated dispatch-side over recipes + nodes); the registry ref-collection surface
  went node-domain; the shallow fast-path walkers were deleted with their functions. No
  query-time authored-shape `TypeExpr` walk remains.
- **`GraphBackedPending` (0) — DONE.** Every named structural arm landed: the narrowed
  value-annotation fact (`ValueTypeAnnotationFact`), the imported-registry facts carrier, and the
  locator-native `named_decl_body`.
- **`GraphFreeDto` (5 rows) — the honest remaining residual.** Below-graph readers:
  `resolver_core/shallow_file_state.rs::route_closure` (thin driver over the shared fact-closure
  core), `resolver_core/external_type_frontier.rs::{resolve_through_export, resolve_one}`, and
  `host_manage/eval_env.rs::{peel_value_decl_alias_graph_native,
  dependency_value_symbol_graph_native}`. One-line status: they live below the session
  `SemanticGraphStore` and read content-free facts/locators — the two external-frontier rows
  included, whose former read of the stored type-parameter pocket is CLOSED (the frontier now
  derives its narrow output from the memo's `narrow_type_parameters` mirror with a content-free
  re-anchor of the bound slots to the frontier symbol); any other authored-body traffic is
  lease-only fact minting at the provider edge.
  Forcing them through the session graph would
  be a layering inversion — they stay as the below-graph residual and remain named until the
  separate producer-boundary-confinement cutover (ledger retirement is owned by the
  producer-boundary-confinement block), not a closure
  condition of anything landed.
