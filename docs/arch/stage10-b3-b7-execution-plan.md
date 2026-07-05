# Stage 10 — B3–B7 execution plan + parallelization DAG (SEQUENCING AUTHORITY)

**STATUS: DAG + forks DECIDED and triple-review-validated; the design residual census is MATERIALLY
INCOMPLETE (§7), which couples into most blocks — only B5 + the TP0 prelude are cleanly dispatchable now.**
The scoping pass VALIDATED the parallelization DAG, the FN5.2→B6 redraw, the boundary forks, and the four
carrier/mechanism forks (all survived adversarial 3/3 review + a fix-cycle re-review). It ALSO discovered —
via review + a code-verifying codex consult — that the binding design's residual census (§3, "39 semantic
readers") is **materially incomplete**: an entire class of query-time resolved/generated `TypeExpr` surfaces is
not scoped — `type_expand`/`ExpandedField` (the central component-meta resolved-field representation, ~40
production files) is entirely uncensused (0 design mentions); `svelte_default_synth` is mentioned once in the
design (§5.4) but not censused/scoped/assigned; `resolver_core::type_expansion` is source-verified present but
absent from the design + field-maps entirely. That gap is NOT confined to
B6/B7: B3's `Analyzed*` narrowing forces edits to the `type_expand` producer (`type_eval_build.rs`), and B4
(which consumes B3's schema — the `B3 → B4` edge — and whose `build_typeof` carries synthesized-default logic)
is entangled with both B3 and the svelte-synth surface. **So: only B5 and the TP0 prelude are cleanly
dispatchable now; B3, B4, B6, B7 are all gated on the §7 design re-census + the `type_expand` sub-design
(B4 transitively, via B3 + the synthesized-default entanglement).**

This document sits UNDER the binding design
[`stage10-typeexpr-terminal-removal-design.md`](./stage10-typeexpr-terminal-removal-design.md) (mechanism/
schema authority) and the field maps [`stage10-fact-schema-field-maps.md`](./stage10-fact-schema-field-maps.md);
it decides sequencing/parallelization + the open carrier forks the design left as menus, and records the
census gap it found. Authored on `refactor/semantic-db-overhaul` @ `84b2d59a8` (B0/B1/B2 landed+confirmed).

**Provenance (record, not architectural evidence).** The DAG + fork decisions were made on the codex
architecture rail (unprimed, effort-neutral, best-on-merits); divergences resolved by a code-verifying
codex decider; the plan passed a design-bearing 3/3 review whose findings are folded in here. The
correctness of each decision rests on the source facts cited, not on the process labels.

**Numbering.** Canonical = the design's B3–B8 (used here + in the field-maps doc). A brief saying "B3–B6"
is the compressed paraphrase for the five remaining implementation blocks B3–B7 (B8 = atomic squash).

---

## 0. Fixed constraints (not decided here)

- **Landing is atomic.** B1–B8 squash into ONE clean cutover (design §10, §14). Every consumer-flip and
  `TypeExpr` field REMOVAL lands together; interim dual paths are WIP-only on the staging branch.
  **Parallelism here is concurrent IMPLEMENTATION on the staging branch, never landing** — landing always
  serializes into the one linear-ff squash. Because the intermediate tree is deliberately non-compiling
  between narrowing slices, "assemble on staging" between waves means cherry-pick/merge WITHOUT a green
  compile gate; per-slice proof is the WIP parity oracle (design §10), not a green staging tree.
- **B0/B1/B2 landed.** The locator/fact substrate (`verter_type_expr::{facts,locators,span_origins,
  fact_witnesses}`), the `lower_locator` provider, the `LowerLocator` query, the sealed
  `LocatorLoweringKey`/`InstantiateKey`, and the `InstantiateBodySource` axis all exist.
- **Fact schema fixed** by design §8 + field-maps. **Crate boundary** `verter_semantic ⊥ verter_session`
  compiler-enforced; `HotTypeRef` ONLY in `verter_session`; locators/facts lower-neutral in `verter_type_expr`.

---

## 1. The decided execution DAG

Two independent unprimed codex legs converged on the core; a third code-verifying decider resolved the one
divergence (FN5.2 placement + second parallel wave, §3). All of this survived a subsequent adversarial 3/3
review of this plan.

```
Prelude  TP0 : shared NarrowTypeParam / TypeParamDeclFact LOCATOR SUBSTRATE in verter_type_expr
                (facts.rs + locators.rs + session locator-deref/identity + witnesses — a real
                 prelude, NOT just a fact type; B3/B5 are CONSUMERS, never competing producers)
                │
        ┌───────┴────────┐
Wave 1  │  B3  ∥   B5    │   VALIDATED FF-safe (per-file disjoint; see §2)
        └───────┬────────┘
                │  (assemble slices on staging — no green-compile gate, atomic model)
        ┌───────┴────────┐
Wave 2  │  B4  ∥   B7*   │   PROVISIONAL — safe for the ORIGINAL B7 surface, but B7 GREW (§7);
        └───────┬────────┘   re-verify B4∩B7 = ∅ after the re-census reassigns svelte-synth (§7).
                │
Wave 3        B6*             consumes B3 schema + B4 hot surface; owns FN5.2 + type_expand internal (§7)
                │
        B8   serial atomic squash landing
```

**DAG edges (verified dependencies):**

| Edge | Reason |
|---|---|
| `TP0 → B3`, `TP0 → B5` | Both consume `NarrowTypeParam`/`TypeParamDeclFact`; one lower-neutral producer prevents two worktrees forking it. TP0 must also extend `locators.rs` + session deref so type-param constraint/default positions are ADDRESSABLE (they are not today). |
| `B3 → B4` | B4's `HotPrepared*` consumes B3's narrowed `Prepared*`/`Analyzed*` fact schema. |
| `B3 → B6` | B6 reads B3-owned narrowed `Projected*` facts + lower-crate fact/locator schema. |
| `B5 → B4` | Shared `crates/verter_session/src/host_manage/eval_env.rs` (B5 typeof-peel; B4 `component_meta_binding_type_entries`). |
| `B4 → B6` | Shared `crates/verter_session/src/project_semantic_dispatch/build.rs` (B4 `build_typeof`; B6 `class_heritage_bases`); B6 migrates orphan consumers after the hot-prepared bundle exists. |
| `B6 → B8`, `B7 → B8` | fold into the squash. |

**Parallel-wave FF-safety (evidence in §2):**

- **Wave 1 `B3 ∥ B5` — VALIDATED FF-safe.** B3's files are all `verter_semantic`; B5's are `verter_session`
  PLUS `verter_semantic/src/facts/hashing.rs` (the no-`TypeExpr` fingerprint-encoder API — fork c, §4). The
  file INTERSECTION with B3 is still ∅ because B3 does not touch `hashing.rs`. The disjointness rests on the
  per-file lists in §2.1, not on a "B5 = verter_session only" crate claim (which is false). Residual risk:
  neither worktree edits the shared `verter_type_expr` type-param substrate — that is TP0, done first.
- **Wave 2 `B4 ∥ B7` — PROVISIONAL.** For the ORIGINAL B7 surface (Svelte persisted facts + framework/output
  DTOs), B4∩B7 = ∅. BUT the re-census (§7) grows B7 (svelte default synth, cache identity, more framework
  consumers) and one newly-found surface (`svelte_default_synth`) is read at `project_semantic_dispatch/
  build.rs:940`, a B4 file — so the grown B7 may collide with B4 on `build.rs`. **This pairing must be
  re-verified after §7 reassigns the newly-found surfaces** (likely moving the svelte-synth semantic-carrier
  cut into B6/Wave3, which already owns `build.rs`, leaving B7 output-boundary-only again).
- **Wave 3 `B6`** runs alone; coupled to B3 (schema), B4 (`build.rs`), owns FN5.2 + the type_expand internal
  carrier. Running last removes remaining conflicts.

**Why not more parallelism.** The only other file-disjoint pairs are `B3∥B7` / `B5∥B7`; full B7 output-projects
over the final internal representation, so it is best not scheduled ahead of B6. `B4/B6` and (pre-redraw) `B6/B7`
share files and MUST serialize. Two parallel waves at ≤2 wide + a serial tail is the clean plan; forcing a third
parallel pair re-conflicts on a shared file.

---

## 2. File-touch-set evidence (verified @ `84b2d59a8`)

Residual readers extracted from `crates/verter_session/tests/cases/residual_type_expr_body_reader_inventory.rs`
(`RESIDUAL_BODY_READERS`, `COMPAT_BODY_READERS`) and cross-checked first-hand. Paths repo-relative. **This set
is the KNOWN scope; §7 adds surfaces the inventory itself omits.**

### 2.1 Per-block file-touch set (known scope)

**TP0 (`verter_type_expr` + session deref)**
- `crates/verter_type_expr/src/facts.rs` — `NarrowTypeParam`/`TypeParamDeclFact` (already models
  constraint/default as `TypeBodySlot`).
- `crates/verter_type_expr/src/locators.rs` + `crates/verter_session/src/decl_body_memo/locator_deref.rs` —
  extend so a type-parameter constraint/default POSITION is addressable + deref-able (not possible today);
  update `fact_witnesses.rs`.

**B3 — Surface C (entirely `verter_semantic`)**
- `crates/verter_semantic/src/analysis/type_solver/prepared.rs`, `.../analysis/types.rs` (incl. the manual
  `Serialize`/`Deserialize` `Wire` for `AnalyzedMacro.parsedTypeArgument`), `.../analysis/type_solver/query_engine.rs`
  (`Projected*` DEFINITION), `.../analysis/type_eval.rs` (`FunctionSignature`), `.../analysis/fact_projection.rs`
  (demo/witness producer). B3 does NOT touch `facts/hashing.rs` (keeps Wave-1 disjoint from B5).

**B4 — Session hot-prepared cutover (`verter_session`)**
- `resolver_core/hot_prepared.rs`, `resolver_core/prepared_decl.rs`, `host_manage/prepared_decl.rs`,
  `host_manage/eval_program.rs`; GraphBackedPending annotation readers `project_semantic_dispatch/build.rs::build_typeof`,
  `resolver_core/runtime_values.rs::prepared_value_decl_to_value_decl_info`,
  `host_manage/eval_env.rs::component_meta_binding_type_entries`.

**B5 — Surface B + hash-input trio (`verter_session` + one `verter_semantic` file)**
- `resolver_core/shallow_file_state.rs` (8 route closures; reduce `compat_type_contributors_for_typeinfo` to
  test-only), `resolver_core/external_type_frontier.rs`, `host_manage/eval_env.rs` (typeof peel),
  `decl_body_memo.rs` (`compat_type_body_hash_input`), `fact_emission.rs`
  (`compat_value_body_hash_input`, `LazyBodyFactSource::compute`, `value_body_for_hash`),
  `typeinfo/oracle_core/source_walk.rs::walk` (test-only).
- **`crates/verter_semantic/src/facts/hashing.rs`** — B5 ADDS a no-`TypeExpr` fingerprint-encoder entry point
  reusing the legacy byte grammar. The byte grammar to preserve is `compute_semantic_hash(body: &TypeExpr)`
  (`hashing.rs:186`) ONLY. `compute_display_hash` is NOT in this file and NOT part of the TypeExpr byte grammar:
  it lives at `crates/verter_session/src/fact_emission.rs:656` and its `_body: &TypeExpr` param is already UNUSED
  (a trivial dead-param cleanup, not a parity encoder). B5 owns the `compute_semantic_hash` encoder refactor +
  the byte-parity fixture (fork c, §4). Disjoint from B3 (B3 stays off `hashing.rs`).

**B6 — Surface A + orphan carriers + FN5.2 (`verter_session`, several `verter_semantic` touches)**
- `project_semantic_dispatch/build.rs` (`class_heritage_bases`), `project_semantic_dispatch/raise.rs`
  (closedness/key-domain), `resolver_core/component_meta_registry.rs`,
  `resolver_core/component_meta_query_engine/{shallow_preserve,registry_decl,node_materialize,helpers,mod}.rs`
  (incl. carrier defs `ResolvedImportedRegistrySymbol.body`, `FastShallowFieldExpr.expr`, `owner_collection_exprs`),
  **`crates/verter_session/src/component_meta_caches.rs`** (`OwnerCollectionDb` — corrected path, NOT under
  `resolver_core/`), `host_manage/component_meta_methods.rs`, `component_meta_resolution_policy/core.rs`,
  `meta_resolve/{registry_materialize.rs,materialize/field_types.rs}`.
- FN5.2 (moved from B7, §3): `resolver_core/component_meta_query_engine/{surface,mod}.rs`,
  `project_semantic_dispatch/{raise,raise_sentinel,raise/shape_engine/*}.rs`, `host_manage/jsdoc_resolve.rs`, plus
  `Unknown{raw}` control-check consumers `meta_resolve/projectors/output_sink.rs`, `meta_resolve/materialize/field_types.rs`.
- Session consumers of B3-owned narrowed `Projected*` facts.
- **fork-b carried file:** `crates/verter_semantic/src/analysis/type_expand/request.rs` (`ExpandedNormalizedExpr`
  — see fork b, §4, and §7 Surface 1).

**B7 — framework/output boundary (FN5.2 REMOVED)**
- `crates/verter_semantic/src/analysis/framework_facts/svelte.rs` (Svelte persisted facts + `SvelteScriptProvider::VERSION`
  + `stable_candidate_hash`, §7 Surface 3), `typeinfo/framework_surface/{results.rs,graph_export.rs,vue_exec/*,svelte_exec.rs}`,
  **plus the `NamedTypeMember`/`MacroSurfaceDtos` aggregation/adapter consumers**
  `typeinfo/framework_surface/{executor.rs,mod.rs}`, `typeinfo/adapters/{vue,svelte}/adapter.rs`, and the framework
  stores (verify each compiles under the split DTO shape or is edited).

### 2.2 Cross-block shared-file map (known scope)

| Shared file | Blocks (function) | Resolution |
|---|---|---|
| `project_semantic_dispatch/build.rs` | B4 `build_typeof` (W2) · B6 `class_heritage_bases` (W3) [· B2 done] [· §7 svelte-synth?] | serial across waves; §7 svelte-synth touch (`:940`) must land in a build.rs-owning block (B6). |
| `host_manage/eval_env.rs` | B5 typeof-peel (W1) · B4 `component_meta_binding_type_entries` (W2) · **B6 fork-b `fast_to_expansion`/`FastShallowFieldExpr` (W3)** | 3-block, serial across waves — no conflict, but all three touch it. |
| `project_semantic_dispatch/raise.rs` | B6 closedness · FN5.2 (now B6) | single-owner (B6). |
| `resolver_core/component_meta_query_engine/mod.rs` | B6 carriers · FN5.2 (now B6) | single-owner (B6). |
| `verter_semantic/.../type_solver/query_engine.rs` | B3 `Projected*` def · B6 consumers (NOT this file) | def→B3 only. |
| `resolver_core/prepared_decl.rs` | B2 done · B4 bundle | B4 owns forward. |

After the FN5.2 redraw, no two CONCURRENT (same-wave) blocks share a file in the KNOWN scope. §7's additions
must preserve this (esp. the svelte-synth/`build.rs` reassignment).

---

## 3. FN5.2 redraw — B7 → B6 (codex-ratified design amendment)

Both DAG legs diverged on the second parallel wave; the code-verifying decider ruled to MOVE FN5.2 into B6,
verified first-hand: (1) the 6 `UNKNOWN_SENTINEL_OWNER_FILES` (per
`crates/verter_session/tests/cases/output_projector_residual_guards.rs`) are the sentinel/control substrate
co-located with B6's `raise.rs` closedness readers + `component_meta_query_engine/mod.rs` carriers — semantic
control-flow, not framework output; typed authority already in `project_semantic_dispatch/raise_sentinel.rs`;
(2) B7's framework output resolves through the PRE-EXISTING macro hot mirror route + a thin `raise_member_value`
projection and does NOT read B6's orphan carriers; `graph_export.rs` is output-only zero-dispatch. The review
independently verified the 6 owner files match and that none is a framework file.

**Consequence:** FN5.2 → B6 removes the only B6∩B7 overlap; the ORIGINAL B7 becomes file-disjoint from B4.
The MECHANISM (design §5.6) is unchanged. **REQUIRED before dispatch (not B8):** amend the binding design §10
B6/B7 rows + §5.6 owner reference to "FN5.2 → B6", so the two authorities do not disagree during execution.

---

## 4. Decided per-block carrier/mechanism forks

All four converged across two independent unprimed codex legs and survived the plan review. Each is
`NoTypeExpr` by construction, respecting R6 / crate-boundary / anti-tear.

**Fork a — `OwnerCollectionDb` value + query-local memo (B6). DECIDED: `Option<AuthoredBodyLocator>`.**
The persistent store value (`crates/verter_session/src/component_meta_caches.rs`, `OwnerCollectionDb`, currently
`Option<Arc<TypeExpr>>`) becomes `Option<AuthoredBodyLocator>` (content-free, keyable, lowers lazily through the
one engine); the query-local `owner_collection_exprs` memo likewise. Producer `owner_collection_expr` computes the
locator from the observed prepared/indexed declaration (NOT `prepared.body.clone()`); consumers lower the locator +
read node-domain predicates, never inspect a stored body; consumers that need `LocatorLoweringKey` obtain the
matching resolved declaration slot from the same observed owner artifact that mints the locator. Anti-tear: publish
locator + fact signature from that one artifact. Rejected: `HotTypeRef` (generation-local; not a persistent store
value), `SessionDemandIdentity` (authored body, not an adapter route), closed fact (open type body). Codex noted a
FURTHER simplification — deleting the body-bearing DB and routing purely through the slot/locator substrate — as a
PREFERRED end-state; that is a routine B6 dedup/perf decision (both routes carry the identical locator
representation), NOT an open architecture fork: the DECIDED value type is `Option<AuthoredBodyLocator>` whether or
not the DB layer survives.

**Fork b — `FastShallowFieldExpr.expr` (B6). DECIDED: session `HotTypeRef` handle + exactness discriminant**
(a `NoTypeExpr` session carrier). It is a live PRODUCTION hot carrier (read at `host_manage/eval_env.rs:899`
`fast_to_expansion`). Rejected: locator-alone (producer emits alias-rewritten SYNTHESIZED bodies), closed fact,
removal. Authored-exact paths lower via locator/hot mirror; synthesized alias-rewrites intern nodes through one
sanctioned session builder. **This directly feeds the type_expand family (§7 Surface 1)** — the downstream
`ExpandedNormalizedExpr.expr`/`ExpandedField.r#type` carrier must become handle/node-native too (that is the §7
scope-expansion, owned with fork b in B6; `TypeExpr` materialization stays at the sealed output sink).

**Fork c — hash/fingerprint-input trio (B5), parity REQUIRED. DECIDED: a dedicated no-`TypeExpr`
`body_fingerprint` producer** at the lazy-body/prep boundary that REUSES the `compute_semantic_hash` byte grammar
(`crates/verter_semantic/src/facts/hashing.rs:186`, currently `&TypeExpr`) to emit the SAME legacy semantic-hash
bytes WITHOUT constructing a `TypeExpr` (type-decl encodes the legacy `lookup_object()` view; value-decl → a closed
`ValueBodyFingerprintInput` encoder). The `display_hash` side is NOT a TypeExpr byte grammar — `compute_display_hash`
(`crates/verter_session/src/fact_emission.rs:656`) already ignores its `_body` param, so it needs only a trivial
dead-param cleanup, not a parity encoder. Rejected: hashing the new facts directly (changes the input domain —
breaks byte-parity), hashing locator identity. If exact byte-parity proves non-isomorphic AFTER the encoder exists,
take a DELIBERATE documented cache-generation bump — never reconstruct a `TypeExpr` to preserve warm hits, never
silently redefine while claiming parity. The fingerprint-parity fixture (design §10) is the impl-time proof; the
architecture is decided. Requires a `verter_semantic/src/facts/hashing.rs` edit (a no-`TypeExpr` semantic-hash
encoder API) owned by B5.

**Fork d — FN5.2 typed-degradation carrier (B6, per §3). DECIDED: typed outcomes carrying the EXISTING
`QueryError`** (structurally `Value(T) | Degraded(QueryError)`, reusing the `raise_sentinel.rs` authority) — NOT a
new taxonomy. Downstream branches that pattern-match `TypeExpr::Unknown{raw}`
(`component_meta_query_engine/surface.rs`, `meta_resolve/projectors/output_sink.rs`,
`meta_resolve/materialize/field_types.rs`) branch on the typed `QueryError`/`RaisedShapeFacts` predicates. The
sentinel-spelling authority moves into `raise_sentinel.rs`. `raise/shape_engine/materialize.rs` stays the sealed
output seam emitting `Unknown{raw}` for display/JSDoc/raw-fallback only.

**Boundary forks (DAG consult, all three legs agree):**
- **Projected\*** DEFINITION narrowing (`verter_semantic/.../query_engine.rs`) → **B3**; B6 owns only session
  consumers (no lower-crate DTO edit).
- **GraphBackedPending 6 readers:** 3 annotation-handle → **B4** (`build_typeof`,
  `prepared_value_decl_to_value_decl_info`, `component_meta_binding_type_entries`); 3 registry/member → **B6**
  (`resolve_imported_registry_symbol_with_budget`, `append_component_meta_registry_entries`,
  `locate_declaration`/`named_decl_body`).
- **Type-params** → one shared TP0 producer (§2.1).
- **`NamedTypeMember`** OUTPUT DTO/projection → **B7** (output-boundary only; the only reads are the
  `graph_export` encoder + producer/raise arms — no semantic-decision reader). B7 must supply a producer/consumer
  table for `NamedTypeMember`/`MacroSurfaceDtos` separating semantic decisions from output/cache storage, and cover
  the aggregation/adapter consumers (§2.1).

---

## 5. Known-census assignment (complete for the 39-reader inventory; NOT terminal — see §7)

Within the design's KNOWN census, every residual is owned by exactly one block, none double- or unassigned:
17 AuthoredShape → **B6**; 12 GraphFreeDto → **B5**; 6 GraphBackedPending → **B4**(3)+**B6**(3); the lower-crate
`Prepared*`/`Analyzed*`/`Projected*` field removals → **B3**; `HotPrepared*`+`PreparedDeclBundle` → **B4**; orphan
carriers → **B6** (+ `NamedTypeMember` output/Svelte facts → **B7**); hash trio → **B5** CONVERT; 2 oracle readers
→ **B5** test-only; FN5.2 → **B6**.

**No deferrals INSIDE the old 39-reader census** — every known-census fork is decided (fork c's byte-parity is an
impl-time fixture proof against a decided architecture; fork b's downstream type_expand dependency is owned, not
deferred). But §7 is an OPEN re-census/design gate: surfaces OUTSIDE the 39-reader inventory (the `type_expand`
family, `svelte_default_synth`, `resolver_core::type_expansion`) are only partially scoped. **The plan does NOT
currently guarantee zero semantic-`TypeExpr` residual by B8 — terminal-completeness is contingent on the §7
re-census + the `type_expand` sub-design.**

---

## 6. How the CTO dispatches from this plan

**Dispatchable NOW (independent of the §7 gap):**
1. **TP0** — a locator-substrate prelude (facts.rs + locators.rs + session `locator_deref.rs` + witnesses).
2. **B5** — Surface B + the hash-input trio (fork c). Cleanly independent of the §7 surfaces; may run concurrently
   with TP0-consuming B3 ONLY once B3 is unblocked (below). Assembling a WIP slice onto staging means cherry-pick
   WITHOUT a green-compile gate (atomic model; per-slice proof is the WIP parity oracle, not a green tree).

**Gated on the §7 re-census + `type_expand` sub-design (do NOT dispatch until resolved):**
3. **B3** — its `Analyzed*`/`Prepared*` narrowing forces the `type_expand` producer (`type_eval_build.rs`) to change;
   the Analyzed* fact schema must be designed to feed `ExpandedField`'s no-`TypeExpr` replacement. Dispatch B3 in
   Wave 1 alongside B5 ONLY after the `type_expand` sub-design fixes that schema (B3∥B5 stays FF-safe per §2).
4. **B4** — gated TRANSITIVELY: it consumes B3's schema (`B3 → B4`), so it cannot precede B3; and its own
   `build_typeof` carries synthesized-default logic (`build.rs:556`, calling `build_synthesized_vue_default_construct_object`
   `build.rs:575`) beside the neutral synthesized-default reader `build_vue_default_instance` (`build.rs:940`) that
   reads the `value_decl("default")` slot the svelte-synth surface populates. When B4 IS dispatched (after B3 + the
   re-census), it must NOT touch ANY synthesized-default logic in `build.rs` — B6 owns that migration.
5. **B6** — Surface A + orphan carriers + FN5.2 + the `type_expand` internal carrier + (per §7.2) the svelte-synth
   semantic cut. Gated on the re-census.
6. **B7** — framework/output boundary; its scope grew (§7). Re-verify `B4 ∩ B7 = ∅` after the re-census reassigns
   the svelte-synth semantic cut to B6.

**Before ANY B3/B4/B6/B7 dispatch:** (a) amend the binding design for FN5.2→B6 (§3); (b) complete the §7 systematic
re-census; (c) codex sub-design the `type_expand` handle-native migration; (d) ratify the svelte-synth reassignment.

**B8** — squash B1–B8; land §9 structural guards; delete the residual inventory + deferral docs; correct §3.5 stale
prose. Every block is S-tier → full 3/3 + discrimination verification + independent confirm; the WIP parity oracle
runs across every slice.

---

## 7. CRITICAL — design residual census is materially incomplete (GATES B3/B4/B6/B7)

A code-verifying codex consult (framed on review findings, verified first-hand) established that the binding
design's residual census (§3.1 "39 semantic readers" + §3.2 orphan carriers) and the residual-inventory test
**under-count an entire class of query-time RESOLVED/GENERATED `TypeExpr` outputs** — surfaces produced by the
resolver at query time, distinct from the authored-body readers the census enumerated. The KNOWN-scope decisions
above stand; these ADDITIONS must be censused, scoped, and (for the largest) sub-designed before B6/B7 briefs are
final.

**Newly-found in-scope semantic-`TypeExpr` surfaces (verified):**

1. **`type_expand` DTO family** — `ExpandedNormalizedExpr.expr`, `ExpandedField.r#type`, `ExpandedObjectShape`,
   `ExpandedProperty`, signatures/params, `ExpandedComponentTypes`, defined in
   `crates/verter_semantic/src/analysis/type_expand/request.rs` and PRODUCED in
   `crates/verter_semantic/src/analysis/type_eval_build.rs` (~:2745+, which reads the B3-owned `Analyzed*.type_expr`
   fields). This is the CENTRAL component-meta RESOLVED-field representation, `TypeExpr`-based, read for SEMANTIC
   decisions at `host_manage/component_meta_extract.rs:260` (root-identity + improvement comparison) and
   `meta_resolve/macro_member_walk.rs:102`, and consumed across **~40 production files** (the entire
   `meta_resolve/projectors/*` published-field pipeline). It appears ZERO times in the design + field-maps.
   **NOT sealed output** — only the terminal `output_sink` materialization is a survivor.
   **Owner:** B6 for the internal handle/node carrier (downstream of fork b's `FastShallowFieldExpr → HotTypeRef`);
   B7 only for the final sealed output DTO arm. **This is a substantial sub-design** (handle-native `ExpandedField`
   across the projector pipeline) that must be codex-designed like the other surfaces — it is NOT a file-list tweak.
   **Coupling into B3:** because `type_eval_build.rs` reads `Analyzed*.type_expr`, B3's narrowing of those fields
   forces this producer to change — B3's `Analyzed*` fact schema must be designed to feed `ExpandedField`'s
   no-`TypeExpr` replacement, so B3 is gated on this sub-design (§6).

2. **`svelte_default_synth` / `ComponentDefaultSynth`** — builds a `LoweredValueDecl` with `TypeExpr` bodies
   (`crates/verter_session/src/resolver_core/svelte_default_synth.rs`, `crates/verter_session/src/framework/synth.rs`),
   whose output lands in the `value_decl("default")` slot (via `insert_synthesised_value_default`) that the
   framework-NEUTRAL synthesized-default dispatch reader `build_vue_default_instance`
   (`project_semantic_dispatch/build.rs:940`, gated on `is_synthesised_component_default`) consumes for
   typeof/instantiate; also read for display by `crates/verter_session/src/framework/api_projectors/svelte.rs`.
   **In-scope.** Mechanism: return a synthesized closed-fact product (facts for `$props`/`$events`/`$slots`/exports/
   legacy-props/shapes; locators for props/dispatcher bodies; session handles only after demand). NOTE: the design
   mentions `svelte_default_synth` once (§5.4:178) as a synthesized-(d) target, but does NOT enter it in the
   39-reader census, the field-map obligations, or any block scope — so it is "mentioned but not censused/scoped/
   assigned," not literally absent. **Owner CONFLICT to resolve:** codex assigned it to B7, but its dispatch consumer
   lives in `build.rs` — a B4 (Wave 2) file — so the
   semantic-carrier cut collides with `B4 ∥ B7`. Recommended resolution: the svelte-synth SEMANTIC-carrier cut
   (touching `build.rs` + `ShallowFileState::insert_synthesised_value_default` + the default-branch dispatch) moves
   to **B6 (Wave 3)**, leaving B7 the persisted facts + output + cache identity. This reassignment needs codex
   ratification.

3. **Svelte capture-candidate cache identity** — `SvelteScriptProvider::VERSION` +
   `stable_candidate_hash` hash `Debug` strings of `props_type`/`dispatcher_events`
   (`crates/verter_semantic/src/analysis/framework_facts/svelte.rs`). When those fields become locators/facts, bump
   the provider version + replace the hash grammar with canonical locator/fact hashing + add a cache-miss
   discrimination fixture. **Owner:** B7.

4. **`resolver_core::type_expansion{,_verter,_host}`** — `TypeExpansionResult`/`ExpandedMember` carry `TypeExpr`
   (`crates/verter_session/src/resolver_core/type_expansion.rs`), apparently stale/protocol-shaped, not a hot
   semantic path. Must be DELETED (if unused) or explicitly quarantined as protocol/output — not left an
   unclassified resolver-core `TypeExpr` API.

**Required actions (design-owner + CTO, before B3/B4/B6/B7 dispatch):**
- Run a SYSTEMATIC design re-census of query-time resolved/generated `TypeExpr` surfaces on the component-meta +
  framework-surface hot paths (codex swept the obvious ones; more of this class may exist). Update design §3
  census, §4.1 scope, and the residual-inventory test.
- Codex sub-design the `type_expand` handle-native migration (30+ files) as a first-class Stage-10 surface.
- Ratify the svelte_default_synth block reassignment (§7.2) and re-verify `B4 ∥ B7` disjointness afterward.
- **Dispatch gating (per §6):** ONLY B5 + the TP0 prelude are CLEAN and dispatchable now; B3, B4, B6, B7 are all
  GATED on the re-census + the `type_expand` sub-design (B4 transitively, via the `B3 → B4` edge + its `build_typeof`
  synthesized-default logic — when dispatched it must not touch ANY synthesized-default logic in `build.rs`). The
  DAG shape (Wave 1 `B3∥B5`, Wave 2 `B4∥B7`, Wave 3 `B6`) and all fork decisions remain valid once the gate clears
  — the gap changes WHEN blocks dispatch, not the DAG topology or the forks.

## 8. Carried obligations

### 8.1 TP0 carried obligation — type-parameter bound lexical env (prefix-for-both)

The TP0 prelude makes a type parameter's constraint / default bound POSITION addressable + deref-able. Recorded with the six required fields:

- **Item.** The lexical type-parameter environment returned for a type-parameter bound (constraint OR default) is the prior-sibling PREFIX frame (`type_parameters[..ordinal]`) at BOTH convention sites — the `TypeParamBound` deref (`navigate_type_space_body`, `crates/verter_session/src/decl_body_memo/locator_deref.rs`) and the full type-param-head lowering (`locator_shape_binder_frame`, `crates/verter_session/src/project_semantic_dispatch/locator_shape.rs` — the current param is bound to the frame AFTER its own constraint/default lower, so both lower under prior siblings only). Prefix-for-both means a constraint cannot reference a LATER sibling type parameter. TypeScript permits a *constraint* (not a default) to forward-reference a later sibling (`type Foo<T extends U, U>`); Verter's engine convention does not.
- **Why not now.** Prefix-for-both is the engine-wide binder-frame convention already in force for the full type-param-head lowering; TS-exact constraint forward-reference visibility is a binder-frame-family POLICY change that must land at BOTH sites together (changing only the new deref path would diverge the two conventions). It is out of TP0's additive-substrate scope — TP0 adds the addressable bound position + its deref, it does not re-decide the engine's lexical binder-frame policy.
- **Owning future block.** The binder-frame-family engine work that owns the lexical type-parameter env policy (the block owning `locator_shape_binder_frame` / the type-param-head binder convention). NOT B3/B5 — they are producers/consumers of the locator, not owners of the binder-frame policy.
- **Temporary behavior.** Prefix-for-both: `type_parameters[..ordinal]` for both constraint and default, at both sites. A constraint's later-sibling reference resolves to the outer/unbound name, never the later sibling. `T` at ordinal 0 sees an empty prefix.
- **Executable fail-closed.** No production producer emits `TypeBodyPathStep::TypeParamBound` yet — the only construction sites are the enum definition (`locators.rs`) and `#[test]` code (`fact_witnesses.rs`); every `locator_deref.rs` occurrence is a match pattern — so the prefix-for-both env is UNREACHABLE by any live path and nothing reachable is mis-scoped. Any structurally-invalid demand fails closed with a typed error (`TypeParamBoundStepMisplaced` / `TypeParamOrdinalOutOfRange` / `TypeParamBoundAbsent`). The prefix-for-both contract is pinned (and mutation-verified RED-on-widen) by `locator_deref_binds_only_prior_sibling_type_params`, `aug_module_type_param_prefix_env_for_sibling_bound`, and `lower_locator_type_param_bound_binds_only_prior_sibling_prefix`.
- **Closure condition.** When the binder-frame-family block ratifies the lexical env policy: if prefix-for-both is final, keep both sites + the pinning tests as-is (optionally add a `type Foo<T extends U, U>` fixture asserting `U` is intentionally NOT bound); if TS-exact constraint forward-references are required, widen the CONSTRAINT env at BOTH `navigate_type_space_body` and `locator_shape_binder_frame` together (the DEFAULT env stays prefix — TS forbids default forward-refs) and update the pinning tests. The row closes when both convention sites are aligned to the ratified policy and no producer activation (B3/B5) depends on the interim behavior.
