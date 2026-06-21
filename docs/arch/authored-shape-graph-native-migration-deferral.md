# Authored-Shape & Graph-Free Declaration-Body Readers — Graph-Native Migration Deferral (TEMPORARY debt row)

**Status**: DEFERRED — the declaration-body storage flip migrates the GENUINELY graph-backed semantic
consumers onto the thin `decl_body_hot_ref` hot accessor (the `SemanticGraphStore` `Instantiate` memo) via
graph-native predicates/materializers. A second class of readers stays on the lower-crate typed IR
(`TypeExpr`) this session and is recorded here for a future graph-native migration. **This is a TEMPORARY
debt row** (per Rule-File Integrity) — it is cleared when that future block lands the graph-native
replacements, not before.

**Ruling source (codex-DEFER, binding)**: the round-3 body-reader confinement codex-DEFER ruling — a
code-verifying neutral codex adjudication that supersedes the earlier two-converged-legs reference for the
body-reader confinement question. It applied the ratified
**no-`HotTypeRef`→`TypeExpr`→semantic-decision bridge** rule (Q-DISPOSITION C5/C6/C7) to determine which
readers can and cannot migrate to the hot accessor without violating it, and additionally ruled on how the
residual body-reader inventory guard must state its claim — see "Body-reader confinement narrowing" below.

### Body-reader confinement narrowing (round-3 codex-DEFER, binding)

The round-3 decider ruled, on VERIFIED evidence, that the residual `TypeExpr`-body-reader inventory guard
(`crates/verter_session/tests/cases/residual_type_expr_body_reader_inventory.rs`) must NARROW its documented
claim to exactly what it enforces (a curated residual-reader inventory + bounded structural tripwires) and
must NOT advertise a global structural-confinement proof for bare `<expr>.body` / `<expr>.type_annotation`
readers.

The deferral of structural confinement is justified by **missing semantic ownership / type identity**, NOT by
implementation effort:

- An untyped `syn` field-read scanner CANNOT attribute a `.body` (or `.type_annotation`) read to the
  declaration-body type: there are hundreds of `.body` / `.type_annotation` textual field reads across parser
  / OXC AST bodies, module blocks, fn bodies, DTOs, `Prepared*` / `Lowered*` / registry / hot-carriers, and
  `syn::ExprField { member: body }` cannot distinguish the four declaration-body fields from unrelated
  same-named fields without type information. A field-name scanner would therefore be either an over-broad
  embargo needing noisy exceptions OR a re-pin of the same curated enumeration (adding no real structural
  confinement).
- Rust privacy cannot expose the lower-crate `verter_semantic` `Prepared*` `TypeExpr` body fields to only the
  selected downstream `verter_session` owners (no friend visibility), so private-field confinement is not
  cleanly achievable at the current crate layout.

**GLOBAL `.body` / `.type_annotation` field scanning is REJECTED as a confinement proof** for this reason. The
genuine structural close — private owner-layer body storage plus `HotPrepared*` / `HotTypeRef` semantic
access and an explicit `AuthoredDeclBody` / authored-shape surface with no raw escape to graph-backed
consumers — is a dedicated follow-up, recorded as part of this debt row's closure criterion below.

## The two deferred reader classes

### 1. Authored-shape readers (decision is intrinsically about the AUTHORED syntactic form)

These readers inspect the AUTHORED syntactic shape of a declaration body / type expression — the literal
`Pick` / `Omit` head, an `IndexedAccess` object chain, a `Ref { type_arguments }` with explicit arguments,
heritage `extends` / `implements` refs, or closedness/key-domain over the authored `TypeExpr`. Migrating them
to the hot accessor would require materialising the handle BACK to a `TypeExpr` to recover the authored
syntax (the forbidden `HotTypeRef → TypeExpr → semantic decision` reverse bridge), or a dedicated
graph-native classifier that does not yet exist.

- `class_heritage_bases` (`project_semantic_dispatch/build.rs`) — needs the authored heritage `extends` /
  `implements` refs of a class declaration body.
- The three closedness / key-domain classifiers (`project_semantic_dispatch/raise.rs`):
  `userland_instantiation_body_is_closed_object`, `prepared_decl_body_is_closed_unguarded`,
  `prepared_instantiation_key_domain_is_closed` — bounded typed-IR closedness/key-domain classifiers that
  read the authored `TypeExpr` (Pick/Omit/Ref/Mapped/Object/Intersection syntactic closedness).
- `meta_resolve/registry_materialize.rs::nested_symbolic_member_route_should_stay_symbolic` — classifies a
  resolved declaration body by its authored shape (`Ref { type_arguments }` non-empty, utility route,
  indexed-access route).
- `meta_resolve/materialize/field_types.rs::type_expr_has_package_backed_object_like_root_with_fence` — the
  root extraction is authored-shape-intrinsic (literal `Pick` / `Omit`, `IndexedAccess.object`, `Ref` head).
- `component_meta_query_engine/registry_decl.rs::owner_collection_expr` (C1) — returns the RAW alias body the
  registry walker classifies by authored shape (`Ref { type_arguments }`); the value is keyed into the
  `TypeExpr`-keyed `OwnerCollectionDb`, which a handle-derived value must never populate, so it stays
  `TypeExpr` (the residual inventory classifies it AuthoredShape per that reason).

**Future graph-native migration** = heritage-as-explicit-metadata (the authored heritage refs surfaced as a
typed, addressable metadata facet rather than recovered from the body `TypeExpr`) + a dedicated
closedness/key-domain GRAPH-NATIVE classifier (over `SemanticNodeData`, mirroring the established
`exactness::node_root_should_stay_symbolic` graph-native-vs-`TypeExpr`-arm equivalence pattern). This is a
SEPARATE design with its own guarded surface — NOT the bounded `decl_body_hot_ref` accessor plumbing.

### 2. Graph-free DTO readers (live BELOW the session graph)

These readers live in shallow/frontier/eval-env layers below the session `SemanticGraphStore` and cannot
carry a `HotTypeRef` without making a below-graph DTO depend on the session graph (a layering inversion):

- `resolver_core/shallow_file_state.rs` route closures (`route_closure`, `member_path_route_closure`,
  `member_route_closure`, `whole_route_closure`, `follow_local_symbol_precise`, `follow_routed_expr`,
  `extract_string_literal_keys_from_type_expr`, `collect_member_path_seed_names`).
- `resolver_core/external_type_frontier.rs` (`resolve_through_export`, `resolve_one`).
- `host_manage/eval_env.rs` value-decl peel (`peel_value_decl_alias_graph_native`,
  `dependency_value_symbol_graph_native`).

**Future graph-native migration** = prefer Cargo/module dependency enforcement of the below-graph boundary;
any genuinely same-crate residual reads route through a graph-native surface once the below-graph layers gain
a graph-aware seam. SEPARATE future work.

### 3. Graph-backed-PENDING (genuinely graph-backed, but needs a larger refactor than the bounded session)

These ARE genuinely graph-backed but their migration is larger than the bounded accessor flip and is deferred
with the rest:

- `component_meta_query_engine/helpers.rs::resolve_imported_registry_symbol_with_budget` — the
  `ResolvedImportedRegistrySymbol.body` CARRIER (not the `prepared_type_decl(..).is_some()` existence check,
  which stays a cheap shallow presence check) migrates to carry identity / `HotTypeRef` and route through
  graph-native materialization when publication needs semantic body data.
- `component_meta_resolution_policy/core.rs::locate_declaration` — the TypeExpr-returning LOCATOR is too
  broad; it splits into an identity/hot locator (for semantic consumers) and an explicit authored-body
  locator (for authored-shape policy code), by downstream need.

## What migrates NOW (the bounded session)

ONLY the thin shared accessor + the single non-vacuous consumer that has an EXISTING graph-native arm to
route through:

- the thin `decl_body_hot_ref` accessor (`project_semantic_dispatch/mod.rs`) over the existing
  `SemanticGraphStore` `Instantiate` memo — no new store/cache (R6: the returned `HotTypeRef` is never lifted
  into a cache key);
- the NON-VACUOUS C-anchor `meta_resolve/projectors/macro_payload_substrate.rs::lower_decl_body_to_node`,
  re-pointed onto `decl_body_hot_ref` (the emit-payload conditional-root carrier walk now reaches a named
  alias body through the shared hot accessor instead of lowering the prepared body `TypeExpr` directly —
  node-equivalent, no reverse bridge).

The producer/mint (`lower_decl_body_with_provenance`) is unchanged — the accessor WRAPS its `Instantiate`
result; it is not itself migrated (it stays the authored-IR→graph-IR bridge).

**Explicitly NOT migrated this session** (all genuinely deferred — see the deferred classes above):

- **C1** (`component_meta_query_engine/registry_decl.rs::owner_collection_expr`) — returns the RAW alias body
  the registry walker classifies by authored shape (`Ref { type_arguments }`), and the value is keyed into the
  `TypeExpr`-keyed `OwnerCollectionDb`, which a handle-derived value must NEVER populate. Classified
  **AuthoredShape** (see §1) — it stays `TypeExpr`; there is no member-surface body caller that takes a
  freshly-cloned declaration body, so the existing `materialize_member_surface_node` arm has no non-vacuous
  production caller to migrate it onto.
- **C2** (`component_meta_query_engine/registry_decl.rs::owner_collection_surface_from_node`) — the dormant
  graph arm stays `#[allow(dead_code)]`: it is producer-blocked (no settled handle holder / non-vacuous
  production caller exists yet), and giving it one would require touching the `TypeExpr`-keyed
  `OwnerCollectionDb` keying.
- **C4** (`component_meta_query_engine/route_keys.rs::enumerate_member_surface_keys_via_route`) — the
  graph-native `SemanticNodeData` member-surface-route key enumerator (over union/intersection/conditional/
  object surfaces) does not yet exist. Classified **graph-backed-PENDING** (§3) — it stays `TypeExpr`.

## Closure criterion

This debt row is cleared when a future block lands: (a) heritage-as-explicit-metadata + the dedicated
graph-native closedness/key-domain classifier (retiring the authored-shape class to graph-native), (b) the
below-graph layering seam (retiring the graph-free-DTO class), and (c) the C3-carrier / C7-split refactors —
at which point the residual `TypeExpr` body-reader inventory's `AuthoredShape` / `GraphFreeDto` /
graph-backed-pending classes empty and this file is deleted.

**Open question for the later integration-confirm (recorded, NOT resolved here)**: whether the authored-shape
readers are PERMANENT split-carrier compat (a body is read as authored `TypeExpr` for syntax + as a
`HotTypeRef` for graph reduction, by design) vs eventual full graph-native migration. The CTO owns that
determination at the Slice-4-end confirm; this row only records the deferral.
