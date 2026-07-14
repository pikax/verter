# Stage-10 B6 — FFI/output-boundary TypeExpr materialization (binding decision)

Owning decision for the NAPI/WASM/LSP component-meta wire boundary after the
`*Analysis` carriers were narrowed from eager `TypeExpr` to the content-free
`SemanticTypeSource` locator. Linked from the Stage-10 design (§5.7 census). This
is the single canonical output-boundary decision; do not duplicate competing
versions.

Authority: neutral unprimed `gpt-5.6-sol` architect scope pass under the
CODEX-ARCHITECT MANDATE + Consult Framing Discipline (framing neutral-verified,
best-not-lowest-effort-explicit), first-hand-verified against the live tree.
Verdict: **VALID-WITH-AMENDMENTS** — the m1 core mechanism is correct; five
material amendments below are binding.

## Decision (amended m1)

Materialize `SemanticTypeSource → TypeExpr` for the full component-meta wire
payload ONLY inside `verter_session`, immediately before the protocol/FFI
boundary, through the existing sealed `OutputProjector` capability driven by one
live `ProjectSemanticDispatch` under the request-bound validated `StoreView`.
Hand `verter_ffi` a session-owned, fully-materialized, context-free output
envelope. The wire contract is UNCHANGED: `Ffi*Meta.type` / `payload` stay
`verter_type_expr::TypeExpr`; no `component_meta.proto` change, no generated
TS/prost regen, no `schema_version` bump, no reserved tags.

`verter_ffi` stays a context-free mechanical mapper: no dispatch, no lowering, no
source lookup, no reparse, no second resolver. It moves already-materialized
`TypeExpr`s (by value) into the DTO.

Rejected (kept here so the option set stays exhaustive): shipping the
`SemanticTypeSource` locator to the wire (forces a JS resolver — violates
single-engine + Native-vs-Compat); a handle-native (`HotTypeRef`) wire (handles
are generation-local, non-hashable/non-persistable — must not cross FFI); a
broad `HotComponentMetaAnalysis` query surface or a `MaterializedExpanded*` DTO
family (over-broad for this boundary, and B7 explicitly built no such family);
productionizing the selective `TypeHandle` API (changes wire + consumer
protocol, different contract).

## Session output envelope

New `crates/verter_session/src/meta_resolve/output.rs`:

- `ComponentMetaOutput { analysis: ComponentMetaAnalysis, resolution:
  Option<ComponentMetaResolutionOutput>, types: MaterializedComponentMetaTypes }`
  — PRIVATE fields, constructible ONLY by the session terminal output sink
  (unrepresentable until materialization succeeds).
- `MaterializedComponentMetaTypes` is a NESTED POSITIONAL topology order-aligned
  with the analysis across all 11 wire lanes — positional vectors, never
  name-keyed maps (names repeat across slots/fallthrough branches / registry
  rows). Positional alignment covers repeated names, nested slot bindings,
  registry rows, and every fallthrough branch.
- The envelope is NON-`Clone`, transported BY VALUE, with a single DESTRUCTIVE
  terminal transfer accessor consumed by the ffi converter. Its sole non-test
  consumer is `verter_ffi`; pin that structurally.
- `ComponentMetaResolutionOutput` is a NARROWED output-only sidecar carrying only
  the state the wire converter needs (mode, resolved-macro output, registry-decl
  output, origin output) — NOT the whole `ResolvedComponentMetaState`.
- `ComponentMetaOutputError` is a strict typed error carrying the failed lane
  path/index and the failed source.

The envelope is NEVER stored on `ResolvedComponentMetaState`, never on
`ComponentMetaAnalysis`, never in `ComponentMetaResultDb` or any warm semantic
cache. It is request-local. Putting wire `TypeExpr` into cached semantic state
would relaunder output IR into semantic authority.

## The 11 wire type lanes

All 11 must materialize (all read a `SemanticTypeSource` / `Option<..>` /
`payload` source; none has a working `.type_expr` read after the B3 narrowing):
props, event payloads, slot bindings, models, exposed members, public-instance
members, merged type-registry entries, accepted props, accepted event payloads,
fallthrough props, fallthrough event payloads.

## Materialization algorithm

`materialize_component_meta_output_types` in
`meta_resolve/projectors/output_sink.rs`, reusing the existing
`MetaResolveProjectorsOutputCap` (NO new/ninth capability):

- ONE `ProjectSemanticDispatch` for the whole payload; request-local dedupe keyed
  by (effective scope, source identity) — a repeated source raises once per
  effective scope.
- Per source, dispatch on the source kind:
  - `Closed(Leaf(..))` and `Closed(LeafUnion(..))` render DIRECTLY as their
    published shallow value — they are NOT raised. Raising a closed source can
    resolve a package alias and emit the internal declaration name, which is a
    semantic demand and breaks Shallow-By-Default (see
    `project_semantic_dispatch/semantic_source.rs` shallow-probe special case).
  - All other sources: `raise_semantic_type_source_to_hot(..)` under `Navigate`
    structural-transit → `OutputProjector::materialize_output_type_expr(node)`
    (PLAIN SHELL — refs stay refs; never the reduced/Expanded materializer) →
    strict unwrap of the sealed carrier only inside this terminal sink.
- NEVER use a raise fallback that synthesizes `Unknown` on a raise miss
  (`raise_node_to_sealed_carrier`-style). A present-but-unraisable source FAILS
  the output (typed error), it does not silently become `Unknown`.
- Finalize the resolved type-registry name-overlay HERE (session owner) — it is a
  semantic publication decision, not an ffi concern.

Publication demand stays `Navigate`-only: a full `get_component_meta` records
ZERO `Published(Expanded)` projection contexts.

## Absence / failure semantics (session-owned, never ffi)

- `type_source: None` → one CENTRALIZED missing-source output policy emits the
  canonical typed `TypeExpr::Unknown`; preserve any legacy raw display text where
  the pre-narrowing converter did (pin with byte/shape goldens). `raw_type` stays
  opaque display metadata; never reparsed.
- `Some(source)` that the live sink cannot raise / shell-materialize → typed
  `ComponentMetaOutputError` refusing the payload and suppressing encoded/output
  cache admission. If partial output is mandated, emit typed `Unknown` ONLY while
  marking the result partial + output-cache-suppressed + recording a diagnostic.
  FFI NEVER collapses an unraisable source to `Unknown`.

## View-fence integration (ALL entry paths)

The output must be materialized against the SAME validated view the analysis was
served under, on every path — cold, warm, base-host, overlay/session, fixed-view
scalar, fixed-view batch, audit, and LSP. Add an output-bearing internal entry
(e.g. `get_component_meta_output`) driven AFTER
`extract_component_meta_from_resolved` produces the final analysis + merged
registry, but BEFORE the request-bound `HostResolverContext` / validated
`StoreView` / dispatch lifetime ends. Never "resolve, return, then open a second
unrelated view" (that races a concurrent mutation pairing an old analysis with a
new dispatch view). Fixed-view batches must add NO extra per-item store-view
reads. Forward the API through the session module chain
(`component_meta_host.rs` / `meta.rs` / `meta_resolve/mod.rs` and the
`component_meta_entry*` paths). Keep ordinary locator-based
`get_component_meta` / `get_component_meta_with_resolution` for non-wire
consumers (this preserves lazy behavior and prevents a cascade).

## Cache rails (separate)

- Semantic-analysis cache dependency/admission is INDEPENDENT of output-cache
  dependency/admission. An output materialization failure suppresses only the
  encoded/output-payload admission; it must NOT suppress an independently
  complete semantic analysis cache entry.
- Output materialization dependencies are separately traced and included in
  encoded-payload cache validation.

## Effective scope for inherited / fallthrough sources

Fallthrough currently clones child sources into parent rows
(`resolver_core/fallthrough.rs`). The parent owner cannot be used blindly as the
raise scope for an inherited source. Normalize cross-owner sources to
self-anchoring sources before branch merging; at minimum carry the effective
source scope positionally and FAIL on ambiguous multi-origin merges.

## FFI cutover

- Replace `component_meta_analysis_to_ffi` +
  `component_meta_analysis_to_ffi_with_resolution` with one
  `component_meta_output_to_ffi(output: ComponentMetaOutput) -> FfiComponentMeta`
  that reads the materialized positional sidecar. Delete the raw-analysis
  converters from production (test-only construction of a session-created
  `ComponentMetaOutput` at most).
- Member visibility: change `ResolvedNativeProp.visibility` (session
  `resolver_core/surface_projector.rs`) to the canonical
  `verter_type_expr::MemberVisibility`, mapping the parser
  `ResolvedMemberVisibility` enum at the parser/session boundary. Do NOT add a
  `verter_parser` dependency to `verter_ffi`, and do NOT re-add a re-export shim
  solely to preserve the old `host::` path. Reconcile the architecture guard that
  pinned the old re-export to the canonical form.
- Cut the 6 callers over: `verter_napi/src/meta.rs`, `verter_wasm/src/lib.rs`
  (audit + ordinary + batch — keep the batch coordinator; add a with-output batch
  lane, do not loop outside it), `verter_lsp` component-meta custom method (it
  already holds the session host — retain and consume the output projection).

## F5 — evaluated composite-union `defineEmits` payload (DEBT ROW #1 close)

Feasible now with ONE shared finite schema extension — NOT an emits-only fork,
NOT B7 `MaterializedExpanded*`. The current tree has top-level
`ClosedTypeFact::LeafUnion` + its raiser and `ExpandedParameter.ty:
SemanticTypeSource`, but `FactOrLocator` lacks a leaf-union arm while
`TupleElementFact.ty` is a `FactOrLocator`, so `[payload: string | number]`
cannot yet be a closed tuple. The Vue path also discards the semantic payload
after producing display text (`vue_exec/normalize.rs` emits `payload: None`;
`define_emits_shape` falls back to honest Unknown; it sets `call_signatures:
Vec::new()`, so `ExpandedCallSignature` is not the propagation path).

Close:

1. Add shared `FactOrLocator::LeafUnion(Arc<[LeafTypeFact]>)` — finite,
   non-recursive, applicable to tuple/function/object interiors (serde / hash /
   witness / `NoTypeExpr` / `NoStoredSpan` clean).
2. Extend the exhaustive `raise_fact_or_locator` route to intern the ordered
   union.
3. Move/generalize `node_leaf_union_fact` into a shared dispatch-owned
   node→closed-fact helper so Vue normalization does not duplicate graph
   inspection.
4. Build a `SemanticTypeSource::Closed(ClosedTypeFact::Tuple(..))` from the
   post-event-name call parameters in the node domain, preserving label,
   optionality, rest, and order.
5. Replace `EmitsSurface.fields: Vec<AnalyzedEmitField>` with a session-owned
   resolved row `ResolvedEmitField { analysis: AnalyzedEmitField, payload_source:
   Option<SemanticTypeSource> }` — authored property events populate
   `Authored(..)`; realized call signatures populate the closed tuple.
6. `define_emits_shape` publishes that source into `ExpandedProperty.ty`; the
   existing `expanded_define_emit_events` route carries it into
   `EventAnalysis.payload`.

Forbidden: emits-only enum arm; recursively-boxed arbitrary union schema;
fabricated authored locator; reverse-parse of the display `TypeExpr`.

On close: un-ignore
`package_pick_heritage_survives_local_indexed_access_helpers_in_component_meta`
and
`generic_package_pick_heritage_and_indexed_access_helpers_survive_in_component_meta`;
delete the degradation guard
`evaluated_union_emit_payload_degrades_to_honest_unknown_not_fabricated_source`;
add a focused exact-source discriminator proving
`Closed(Tuple(.. FactOrLocator::LeafUnion ..))` → demanded `Tuple([Union(String,
Number)])` with no `Unknown` / `Authored` / ad-hoc `Synthesized` fallback. Update
`docs/arch/stage10-b6-p4b-debt-rows.md` DEBT ROW #1 to closed and
`docs/arch/stage10-fact-schema-field-maps.md` for the new resolved emit payload
carrier + shared nested union arm.

CLOSED (amendment): steps 1–6 landed (the leaf-union closure), and the two
residual REQUIRED-payload classes the leaf-union cut could not express landed
after it — the projected CALLABLE-PARAMS replay route
(`ProjectedTypeFact::CallableParams { base, signature_ordinal, first_param }`,
raised by `raise_projected_callable_params` through the one shared dispatch)
covers call-signature payload params richer than the closed element vocabulary
(cross-file references, composites, nested objects, arrays/callbacks,
instantiated generics), and `define_emits_shape` publishes the normalized
`ResolvedEmitField.payload_source` as the SOLE emit payload authority (the
flat evaluated field contributes exactness/status/diagnostics metadata only;
the flat-lane REQUIRED-payload residue arm is deleted). Closure evidence and
the executable rails live on `docs/arch/stage10-b6-p4b-debt-rows.md` DEBT ROW
#1 (CLOSED); the member-value analogue is DEBT ROW #3 (open).

## Non-negotiable invariants

1. Exactly one resolution engine — all source raising goes through
   `ProjectSemanticDispatch`.
2. OXC/parser remains syntax + locator production only.
3. Output materialization is `Navigate` structural-transit + plain shell — never
   reduced, never `Published(Expanded)`.
4. Closed leaf and leaf-union sources render directly; they are not re-resolved.
5. One dispatch per output payload, request-local source dedupe.
6. `ComponentMetaAnalysis`, `ResolvedComponentMetaState`, `ComponentMetaResultDb`,
   and warm semantic caches stay free of output `TypeExpr`.
7. The output envelope is request-local and never stored on
   `ResolvedComponentMetaState`.
8. `None` source follows one centralized missing-source output policy; legacy raw
   display text preserved where applicable and pinned with byte/shape goldens.
9. `Some(source)` that cannot be raised/shell-materialized returns a typed output
   error; FFI never collapses it to `Unknown`.
10. Output failure suppresses encoded/output-cache admission only; it must not
    suppress an independently complete semantic analysis cache.
11. Output materialization dependencies are separately traced and included in
    encoded-payload cache validation.
12. Wire DTOs, proto, generated TypeScript, and `schema_version` unchanged.
13. Positional alignment covers all nested topology (repeated names, slot
    bindings, registry rows, every fallthrough branch).
14. Base/warm/cold/overlay/fixed-view batch/audit/LSP output is materialized
    against the SAME validated view the analysis was served under.
15. No extra per-item store-view reads in fixed-view batches.
16. Cross-owner sources have a defined effective scope; the parent owner is not
    used blindly for inherited sources.

## Required tests / gates

Discriminating tests (RED pre-change, GREEN post-change; read every body):

1. One fixture populating all 11 lanes with distinct sentinel types, duplicate
   names, nested slot bindings, multiple fallthrough branches.
2. Closed `Ref` alias and `LeafUnion` shallow-output tests proving the published
   alias survives (not expanded to the internal decl name).
3. `None`-source output-policy goldens for every lane with raw display text.
4. Present-but-unraisable source → `ComponentMetaOutputError`, no encoded-payload
   admission, not FFI `Unknown`.
5. Recovery: the same output succeeds after the missing dependency is available.
6. Cold/warm/base/overlay equivalence.
7. Fixed-view scalar/batch equivalence + O(1) store-view-read counter.
8. Audit and LSP equivalence with NAPI/WASM output.
9. Cross-owner fallthrough fixture whose child publishes a child-local alias.
10. Registry ordering / duplicate-name / declaration-metadata alignment.
11. Request-local dedupe counters (repeated source raises once per effective
    scope).
12. `publication_routes_never_demand_expanded` extended to the new output APIs.
13. Output-projector confinement guards; reusing `MetaResolveProjectorsOutputCap`
    leaves the sanctioned capability inventory unchanged.
14. F5 exact-source discriminator (above).
15. Un-ignore the two trackers; delete the degradation guard.
16. `FactOrLocator::LeafUnion` serde/hash/witness/`NoTypeExpr`/`NoStoredSpan` +
    exhaustive-raiser tests.
17. Existing NAPI/WASM/LSP byte-equivalence and schema-freshness tests.
18. Structural guards: sole envelope construction, sole terminal extraction, no
    semantic-cache embedding, all 11 lanes present, no `Published(Expanded)`
    output demand.

Gate commands: `cargo build -p verter_ffi`; `verter_napi`/`verter_wasm`/
`verter_lsp` target checks; `cargo nextest run --workspace`; `cargo test -p
verter_session --tests`; `node scripts/gate.mjs`; `cargo fmt --all --check`;
`cargo clippy --workspace -- -D warnings`; proto/generated-binding freshness with
a clean diff proving no regeneration; `pnpm run build:native`; `pnpm run
build:wasm`; `pnpm test`.

## Scope freeze

No wire/proto/TypeScript/schema-version change; no `SemanticTypeSource` /
`HotTypeRef` / `TypeHandle` wire redesign; no JS resolver; no broad
`HotComponentMetaAnalysis` or `MaterializedExpanded*` family; no output envelope
in semantic caches; no arbitrary recursive composite-fact vocabulary beyond the
finite nested leaf union F5 needs; no productionization/repair of the selective
`TypeHandle` API; no unrelated Stage-10 reader migrations, parser/codegen
consolidation, cache redesign, or other debt rows.

## Implementation cuts

One Lane-3 review; two independently coherent cuts:

- Cut 1 — output-boundary cutover: characterization tests first; add the session
  output envelope + materializer; neutralize member visibility; convert all 11
  lanes and all 6 callers ATOMICALLY; delete the raw-analysis converters; restore
  the FFI/NAPI/WASM/LSP compile surface. Do NOT land a temporary compile fix that
  maps missing fields to `Unknown`.
- Cut 2 — F5 shared-schema closure (above).

A/B implementer experiment: one `events[].payload` vertical slice (session
materialization through one FFI caller) using a simple authored/closed-leaf
payload — exercising `None` vs `Some`, typed materialization failure, output
ownership, positional transport, shallow-shell behavior. Keep the composite-union
F5 fixture OUT of the A/B (schema feasibility must not confound the boundary
decision). Once the winning boundary is chosen, implement all 11 lanes together.

Review tier: S, full 3/3.
