# Native Typeinfo Parity — Cache / Exporter / DB / Session / Projections / Wire

Parent architecture: docs/arch/native-typeinfo-parity.md
Sequencing authority: docs/arch/semantic-db-overhaul-unified-remaining-plan.md
Owning U-block(s): U3, U8, U10, U11, U12, U13
Prerequisites: U0 (the manifest / ledger / coverage / cutover substrate that gates every block's `Verifying` transition — `docs/arch/native-typeinfo-parity-u2-reducers.md::U0.MANIFEST_SUBSTRATE`); U2 (the whole reducer + typed-value-domain + `SemanticQueryKeySpec` parent — `docs/arch/native-typeinfo-parity-u2-reducers.md`); U6 (the demand-sliced flow / call solver — `docs/arch/native-flow-return.md`). U1 / U4 / U5 (the persistent cache-runtime node substrate, the scheduler DAG, the artifact-store rehoming) are non-parity prerequisites depended upon, not owned here.
Consumers: U14 / U15 (framework adapters, integrations, final lift — `docs/arch/native-typeinfo-parity-adapters-final-lift.md`) consume the exporter (U12), the published `GraphTypeNode` projection (U13), the public relation / session surfaces (U11), and the wire payload (U8). The LSP, MCP, unplugin, playground, and `@verter/component-meta` host-backed consumers read the U12 exporter + U13 projection + U11 session surfaces.
Progress ledger: crates/verter_session/tests/typeinfo_ignored_test_manifest.rs

---

## Scope and authority

This child subplan owns the **back half** of the native-parity engine — the facts,
the wire surface, the result database, the public relation / session surfaces, the
exporter, and the published projections (including the TS `TypeDescriptor`
projection) — and cites, never restates, the parent for the engine architecture.

It owns the parity blocks landing in **U3, U8, U10, U11, U12, U13**:

1. **U3** — the cache / fact model end-state: per-budget non-admission and typed
   admission (R21 / R6), route-fact validation / invalidation, and the
   cross-file-resolution route-demand facts the exporter and session surfaces read.
   Parent authority: PART 1 §6, the Cache Architecture rules, the Canonical
   Dependency Cache Rule.

2. **U8** — the `ProgramAnalysisGraph` flow / contextual fact placement on the wire
   payload (`TypeInfoGraphPayload.program_analysis`), the `DeclarationAnalysisGraph`
   declaration / environment-mutation placement
   (`TypeInfoGraphPayload.declaration_surfaces`), the diagnostics / relation-proof
   side tables, the retired-and-`reserved` `GraphTypeNode` arms + `SemanticTypeGraph`
   embeddings, and the schema-version gates — the whole-surface wire-purity closure.
   Parent authority: PART 1 §§1.3–1.5, the Typeinfo Wire Contract.

3. **U10** — `TypeInfoGraphResultDb` / final-result admission, and the
   mode / demand / expansion-boundary EXACTNESS gating over the U2 reducer substrate
   (the `DemandBoundary` / `ModeBoundary` / `ExpansionBoundaries` rows whose coverage
   `block_id` is a U10 block, not a U2 block — `docs/arch/native-typeinfo-parity-u2-reducers.md`
   → "Row-level-split capabilities"). Parent authority: PART 1 §§5–6, the
   fact-based cache architecture.

4. **U11** — the public relation / session surfaces (`relate` returning the public
   `RelationPayload`), the request-footprint attachment pipeline, and fact
   validation / route invalidation observed through the host audit runtime over the
   U2 / U3 cache families. Parent authority: PART 1 §4, PART 2 §10–11, the audit
   infrastructure.

5. **U12** — the exporter: the request → graph projection that materialises the
   `GraphTypeNode` type-values surface + the `ProgramAnalysisGraph` /
   `DeclarationAnalysisGraph` / diagnostics / relation-proof side tables into
   `TypeInfoGraphPayload`, and the `RelationPayload` for public `relate`. Parent
   authority: PART 1 §§1.3–1.5, §3.

6. **U13** — the published projection: the closed `GraphTypeNode` type-value
   projection of every U2 type value, and the TS `TypeDescriptor` projection
   (`@verter/type-ir` + `@verter/component-meta` decode / projection) that consumes
   the wire payload structurally. Parent authority: PART 1 §1, §8, the Typed-IR-Only
   Resolver Rule, the Component-Meta Native vs Compat Rule.

The parent is the architecture authority. Each block below cites the parent
section that defines the architecture it implements and states only the concrete
**block contract** — what changes, what is deleted, which named guards land, which
exact manifest rows lift, and how it is verified. No block restates the wire-purity
closure (PART 1 §§1.3–1.5), the per-key cache-soundness rules (PART 1 §2), the typed
value-domain (PART 1 §3), or the budget contracts (PART 1 §6); those live in the
parent and are referenced by section number.

Every block contract uses the parent's per-block contract template (PART 2 §9).
"Done" for any block is the parent's four-part done predicate (PART 2 §11.7); a
block may enter `Verifying` only after its rows' coverage is complete and
non-placeholder (PART 2 §10.4); acceptance is the two-phase prepare-verify → gate →
accept lifecycle with the input-hash-PAIR gate receipt over the
`.cutover-state.typeinfo_parity` namespace (PART 2 §§11–14). None of that machinery
is re-specified here.

### Block dependency graph (within this subplan)

```
U2 parent done  +  U6 parent done           (the reducer + flow substrate; not owned here)
        │
        ▼
U8.WIRE_SURFACE_CLOSURE   (the whole-proto wire-purity end-state: payload + program-analysis +
        │                  declaration-surface + diagnostics/relation-proof side tables + schema-version gates)
        │
        ├─► U3.CACHE_FACT_MODEL   (per-budget non-admission + typed admission + route-fact validation/invalidation)
        │        │
        │        ▼
        │   U10.RESULT_DB         (TypeInfoGraphResultDb final-result admission + mode/demand/expansion EXACTNESS)
        │        │
        │        ▼
        ├─► U12.EXPORTER          (request -> TypeInfoGraphPayload projection + RelationPayload)
        │        │
        │        ├─► U11.PUBLIC_RELATION_SESSION  (public relate/RelationPayload + footprint attachment + route invalidation)
        │        │
        │        └─► U13.PROJECTION             (closed GraphTypeNode projection + TS TypeDescriptor projection)
```

`U8.WIRE_SURFACE_CLOSURE` is the keystone of this subplan: the exporter (U12) cannot
populate a payload whose shape is not yet closed, the result DB (U10) caches that
payload, and the projection (U13) decodes it — so all three declare the wire closure
a prerequisite. `U3.CACHE_FACT_MODEL` lands the typed-admission / route-fact rails
the result DB (U10), the exporter (U12), and the session surfaces (U11) all admit
through. Downstream U14 / U15 stay blocked until every block in this subplan — and
the whole U2 / U6 parents — are done.

> **State note:** the cache / fact model, the typed value domain, the wire payload,
> the exporter, and the projections described below are the **end state** to be
> built. The current `crates/verter_protocol/proto/verter/v1/typeinfo.proto` still
> carries the live non-type-value `GraphTypeNode` arms and the bare
> `SemanticTypeGraph` embeddings the parent retires (PART 1 §§1.3–1.5); the current
> `SemanticQueryKey::Relate` is the bare `{ source, target }` tri-state-returning
> shape; the manifest is the old four-field schema with 363 rows. This subplan does
> not imply the end state already exists.

---

# U8 — Wire-surface closure

## U8.WIRE_SURFACE_CLOSURE

ID: U8.WIRE_SURFACE_CLOSURE
Parent U-block: U8
Subplan: docs/arch/native-typeinfo-parity-cache-export-session.md

Prerequisites: U0.MANIFEST_SUBSTRATE, U2 (parent), U6 (parent).
Blocked until: the whole U2 parent and the whole U6 parent are done (the wire surface carries the U2 type-value arms, the typed value domain, and the U6 flow / relation facts; closing it before the producers exist would close it against a moving target). This block is the wire keystone every other block in this subplan depends on.

Context: The current wire / proto contradicts the parent's type-values-only ruling at several public sites discovered one at a time — `GraphFlowNarrowing` (tag 26) / `GraphContextualType` (tag 27) / `GraphRelationProof relation_proof` (tag 28) as `GraphTypeNode` arms, `SemanticTypeGraph.diagnostics` (tag 9), `module_augmentation` (tag 23) / `global_augmentation` (tag 25) as `GraphTypeNode` arms, `FrameworkSurfacePayload.graph = 4` embedding `SemanticTypeGraph` directly, `TypeInfoGraphResponse.graph = 1` as the success arm, and `GraphTypeParameter.no_infer = 9` modelling `NoInfer` as type-parameter metadata. The parent requires ONE comprehensive deliverable that reconciles the ENTIRE public proto surface with the moved-concept end-state under the Typeinfo Wire Contract (closed-enum discipline + wire-compat + additive-audit + validate-before-execute — PART 1 §§1.3–1.5). This block lands that whole-surface closure: it introduces `TypeInfoGraphPayload`, `ProgramAnalysisGraph`, `DeclarationAnalysisGraph`, the diagnostics / diagnostic-directive / relation-proof side tables, and the `RelationPayload`; retires-and-`reserved`s every relocated/retired concept on every type-value message; and migrates every public `SemanticTypeGraph` embedding additively. It exists now, before the exporter/DB/projection blocks, because every later block in this subplan reads or populates this closed payload shape.

Changes (exact files / functions):
- `crates/verter_protocol/proto/verter/v1/typeinfo.proto` — add `message TypeInfoGraphPayload { graph, program_analysis, declaration_surfaces, diagnostics, diagnostic_directives, relation_proofs }`; add `message ProgramAnalysisGraph { flow_narrowings, contextual_types }`; add `message DeclarationAnalysisGraph { module_augmentations, global_augmentations }`; add `message RelationPayload` (outcome / bindings / proof) and the `TypeInfoGraphPayload.relation_proofs` payload-side proof table keyed by opaque proof id (PART 1 §§1.3–1.4). On `GraphTypeNode`: retire + `reserved` (tag + name at the enclosing message scope, proto3 forbids `reserved` inside an `oneof`) `flow_narrowing` (26), `contextual_type` (27), `relation_proof` (28), `module_augmentation` (23), `global_augmentation` (25); keep the closed type-value allowlist (PART 1 §1.3) live. On `SemanticTypeGraph`: retire + `reserved` `diagnostics` (9); bump `SemanticTypeGraph.schema_version`. On `GraphTypeParameter`: retire + `reserved` `no_infer` (9) with no replacement (PART 1 §1.2). On `TypeInfoGraphResponse`: retire field `1` (`graph`) — `reserved` OR downgrade-encoder-only — and add `TypeInfoGraphPayload payload = <next free tag>` as the success arm (PART 1 §1.5). On `FrameworkSurfacePayload`: retire field `4` (`graph`) and add a `TypeInfoGraphPayload` carrier at the next free tag. Extend `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` for the new schema version.
- `crates/verter_protocol/src/typeinfo/graph.rs` — the Rust-side DTO surface for the new messages (`TypeInfoGraphPayload`, `ProgramAnalysisGraph`, `DeclarationAnalysisGraph`, `RelationPayload`, the relation-proof table); the additive constructors/accessors mirroring the existing `wire_*` helper discipline. The downgrade encoder (if `TypeInfoGraphResponse.graph` / `FrameworkSurfacePayload.graph` are kept behind a registered versioned downgrade path rather than `reserved`).
- `crates/verter_protocol/proto` TS bindings — regenerate via the workspace `buf` + `oxfmt` binaries (the generated TS bindings are byte-equal to regenerated `buf` output; this is mechanically pinned — see guards).
- `crates/verter_session/src/typeinfo/request_validation.rs::validate_type_info_graph_request` — extend the closed schema-version gate (`SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS`) to the new version and keep per-variant structured-expression validation exhaustive over the `oneof` taxonomy (validate-before-execute — Typeinfo Wire Contract invariant 4).
- `crates/verter_audit/src/payloads/typeinfo_graph.rs` — additive audit-envelope fields for the new payload concepts (`structured_event` / `kind_payload` arms) land as new arms / default-zero fields, never replacements (Typeinfo Wire Contract invariant 3; additive-audit).

Deliverables:
- `TypeInfoGraphPayload { graph, program_analysis, declaration_surfaces, diagnostics, diagnostic_directives, relation_proofs }` as the closed response payload, with `ProgramAnalysisGraph { flow_narrowings, contextual_types }` and `DeclarationAnalysisGraph { module_augmentations, global_augmentations }` siblings and the `RelationPayload` + `relation_proofs` proof table.
- Every relocated/retired concept (flow narrowing, contextual type, relation proof, diagnostics, module/global augmentation, `no_infer`) retired + `reserved` on its type-value message and relocated to its end-state home; every public `SemanticTypeGraph` embedding (`TypeInfoGraphResponse.graph`, `FrameworkSurfacePayload.graph`) migrated additively to `TypeInfoGraphPayload`; the schema-version bumped; the regenerated byte-equal TS bindings.
- The closed-set schema-version gate + exhaustive structured-expression validation over the new payload.

Legacy deletions:
- The live `GraphTypeNode` arms `flow_narrowing` (26) / `contextual_type` (27) / `relation_proof` (28) / `module_augmentation` (23) / `global_augmentation` (25) — retired + `reserved`, never reused (PART 1 §1.3).
- The live `SemanticTypeGraph.diagnostics` (9) and `GraphTypeParameter.no_infer` (9) fields — retired + `reserved` (PART 1 §§1.2, 1.5); `no_infer` has no replacement field.
- The `SemanticTypeGraph graph = 1` success arm of `TypeInfoGraphResponse` and the `FrameworkSurfacePayload.graph = 4` direct embedding as server-populated carriers — retired/`reserved` or downgrade-only; replaced by `TypeInfoGraphPayload` carriers at fresh tags (PART 1 §1.5). No field number is reused.
- The stale recovered-doc wire wording (`TypeNode::RelationProof`, the §3 `module_augmentation = 23` / `global_augmentation = 25` type-value arms, the §2.17 flow/contextual `TypeNode::FlowNarrowing` / `TypeNode::ContextualType` placements) is amended to the relocated homes (Cross-reference / doc-update obligations).
- No projection-repair / second-engine wire path is removed by this block (the wire surface is the one transport contract). Stated explicitly per the template.

SemanticQueryKey/facts touched: none directly (this is the wire/transport surface; query keys + facts land in U2 / U6). The wire payload carries the value-domain results the typed `SemanticQueryValue` layer produces — `ProgramAnalysisGraph` for `FlowNarrowingAt` / `ContextualTypeAt`, `DeclarationAnalysisGraph` for `ResolveDeclarationAugmentation`, the `RelationPayload` for `Relate` — but adds/changes no key or fact.

Exact test rows lifted: none. U8 lifts no `#[ignore]` row — it closes the wire surface every later block populates / reads. Its discriminating proof is the wire-surface-purity guard suite below (each a `StructuralGuard`-class default-suite test).

Required new guards (PART 1 §§1.2–1.5; parent Guards index → Type IR — `GraphTypeNode` / wire-surface purity + `SemanticTypeGraph` embeddings):
- `graph_type_node_oneof_contains_only_type_value_arms`, `graph_type_node_allowlist_arms_have_type_value_classification`, `no_non_type_value_smuggled_into_graph_type_node`.
- `typeinfo_wire_surface_has_no_retired_concept_fields`.
- `flow_contextual_facts_not_graph_type_nodes`, `program_analysis_graph_exposes_flow_contextual_queries`, `flow_contextual_doc_and_wire_placement_match_program_analysis_graph`.
- `relation_proofs_not_graph_type_nodes`, `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`.
- `no_infer_not_type_parameter_metadata`, `diagnostics_only_on_typeinfo_graph_payload`.
- `typeinfo_graph_response_payload_arm_is_additive_not_retyped`, `framework_surface_payload_graph_payload_is_additive_not_retyped`, `all_public_semantic_type_graph_embeddings_are_payload_wrapped`.

Critical-rule guards: this block implements the parent's `(CRITICAL)` Typeinfo Wire Contract — the four invariants (closed-enum discipline, wire-compat, additive-audit, validate-before-execute). The wire-purity + embedding + additive-audit + validation guards above ARE their R6 guards: proto/TS oneof parity (`crates/verter_session/tests/typeinfo_graph_taxonomy.rs`), byte-equal TS freshness (`crates/verter_protocol/tests/typeinfo_proto_ts_freshness.rs::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output`), audit-parity (`crates/verter_audit/tests/request_kind_payload_parity.rs`), and request validation (`crates/verter_session/tests/typeinfo_request_validation.rs`). Any new `(CRITICAL)` rule text added to docs in this change registers its guard here in the same change.

Proof requirement: structural guards (the wire-purity, embedding, additive-audit, and validation guards above), all default-suite tests. The discriminating property: every retired concept appears ONLY in its message's `reserved` list (or behind a registered versioned downgrade encoder); no field number is reused; `GraphTypeNode` carries only the closed type-value allowlist; every public `SemanticTypeGraph` embedding except the canonical `TypeInfoGraphPayload.graph` is payload-wrapped; the regenerated TS bindings are byte-equal.

Exit acceptance:
- `graph_type_node_oneof_contains_only_type_value_arms` + `graph_type_node_allowlist_arms_have_type_value_classification` green (only the closed type-value allowlist remains live; no declaration/environment-mutation arm is allowlisted).
- `typeinfo_wire_surface_has_no_retired_concept_fields` + `all_public_semantic_type_graph_embeddings_are_payload_wrapped` green (no retired-concept field live on any type-value message; every embedding payload-wrapped except the canonical exempt one).
- `program_analysis_graph_exposes_flow_contextual_queries` + `diagnostics_only_on_typeinfo_graph_payload` + the relation-proof / `no_infer` guards green (mandatory-BOTH: arms retired AND facts reachable on their relocated homes).
- The proto/TS taxonomy + byte-equal freshness + audit-parity + request-validation guards green; the schema version is bumped; no field number reused.

Verification commands:
- `cargo test --package verter_session --test typeinfo_graph_taxonomy` (proto/TS oneof parity).
- `cargo test --package verter_session --test typeinfo_wire_surface_guards` and `--test typeinfo_graph_contract_guards` (wire-purity / embedding closure).
- `cargo test --package verter_protocol --test typeinfo_proto_ts_freshness` (byte-equal regenerated TS bindings) and `--test typeinfo_proto_roundtrip`.
- `cargo test --package verter_audit --test request_kind_payload_parity` (additive audit envelope).
- `cargo test --package verter_session --test typeinfo_request_validation` (closed schema-version + exhaustive structured-expression).
- Regenerate the TS bindings via the workspace `buf` + `oxfmt` binaries before the freshness test.
- The full workspace gate (the complete Rust **AND** JavaScript gate, green only when BOTH pass; non-mutating — clippy-fix / formatting run BEFORE `prepare-verify`, so the gated content is already clean and the receipt certifies an unchanging input): `cargo test --workspace --tests --verbose`; `cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`; `pnpm test`; `pnpm install --frozen-lockfile`.
- `node scripts/gen-corpus-audit-tests.mjs` (idempotent; audit-record schema/fixtures changed).
- Commit cadence / review gate: PARENT-UNIFORM — the uniform discipline for EVERY block in this subplan (parent PART 2 §11.11 / §11.12), stated once and not restated per block: each block lands as ONE squashed commit (WIP series during the work, no per-commit gate) after the three-reviewer LAND verdict (1 Claude Code + 2 codex).

Docs updated: amend `docs/arch/semantic-type-graph-plan-recovered.md` stale wire wording (the §2.17 / §3.11 flow/contextual `TypeNode::FlowNarrowing` / `TypeNode::ContextualType` placements → `ProgramAnalysisGraph` payload entries; the stale `TypeNode::RelationProof` wording → `RelationPayload` / payload-side proof table; the §3 `module_augmentation = 23` / `global_augmentation = 25` type-value arms → `DeclarationAnalysisGraph` on `TypeInfoGraphPayload.declaration_surfaces`). Update the `/type-cache-architecture` skill's wire-payload notes. Pinned by `flow_contextual_doc_and_wire_placement_match_program_analysis_graph` and the value-domain / wire-surface guards.

Re-entry notes: the wire relocation is a closed-contract change (Typeinfo Wire Contract — schema-version bump, `reserved` tags never reused). Regenerate the TS bindings via the workspace `buf` / `oxfmt` and re-run the byte-equal freshness test; if a tag was reused or a concept left live on a type-value message, its purity guard fails and tells exactly which. Idempotent — re-running the proto regeneration is byte-stable.

---

# U3 — Cache / fact model

## U3.CACHE_FACT_MODEL

ID: U3.CACHE_FACT_MODEL
Parent U-block: U3
Subplan: docs/arch/native-typeinfo-parity-cache-export-session.md

Prerequisites: U8.WIRE_SURFACE_CLOSURE, U2 (parent), U6 (parent).
Blocked until: U8.WIRE_SURFACE_CLOSURE done (the typed admission this block enforces produces values whose wire shape U8 closes), and the whole U2 + U6 parents done (the reducers + flow solver this block bounds and admits). The result DB (U10), the exporter (U12), and the session surfaces (U11) all admit through this block's rails.

Context: The demand-sliced engine is only safe with explicit typed budgets and a single typed admission rail (PART 1 §6, the Cache Architecture rules). Every hot reducer must return a typed `BudgetExceeded` non-admission that is `ReturnOnly` — never warm-admitted, never backfilled, never published as a partial/torn cache entry, never recorded as a fact signature/backfill (the three-layer non-admission rule). Cache keys split across the five orthogonal env-hash dimensions (R21 — no bundled `project_config_hash`); query-identity keys carry a content-free `DeclKey` and never include content/version hashes or `fact_dep_signature` (R6); version rooting lives on the cached value via `ReadSetSignature.facts` (the sole cache-validity rail). Route-fact validation / invalidation is read-side authoritative: VFS is the file-change authority, route invalidation is not file-hash-only (tsconfig / vite alias / workspace graph / package-target changes invalidate affected route facts), and concurrent cold requests collapse onto one materialization path. This block lands the typed-admission + per-budget non-admission + route-fact-validation rails that the result DB, exporter, and session surfaces ride on, and lifts the `CrossFileResolution` route-demand rows whose mechanism is a route-fact path. It exists now because the result DB and exporter cannot admit results before the admission contract is closed.

Changes (exact files / functions):
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — the typed `BudgetExceeded` non-admission for every hot reducer budget (`RelationBudget`, `KeyspaceBudget`, `CallResolutionBudget`, `FlowSliceBudget`, the apparent-type member-demand index — PART 1 §6), each routing through `ReturnOnly` (no semantic result, no artifact/intermediate, no fact signature/backfill, no degraded exact-cache entry). The `RelationBudget` pair memo keyed on the FULL `Relate` identity, not the bare pair (PART 1 §§2.7, 6).
- `crates/verter_session/src/semantic_query_memo/mod.rs` — the multi-candidate `FamilySlots` admission substrate (cap 4, FIFO eviction) with per-candidate `ReadSetSignature.validate_with_self_roots` as the sole validity rail; the `ComputeAdmission::{ReturnOnly, ...}` typed admission enum (overflow / budget exhaustion / cancellation / generation supersession / incomplete self-rooting / unresolved provenance all route through `ReturnOnly`); content-free `DeclKey` query-identity keys (R6) re-sourcing the live content version at value-compute time.
- `crates/verter_session/src/cache_schema.rs` — the five-dimension env-hash split (R21) for the cache families this subplan adds (`TypeInfoGraphResultDb` in U10), with `lib_env_hash` entering a key only when the cached value depends on lib data (PART 1 cache rules: `RouteDb` / typed-IR resolve / the result DB DO include it).
- `crates/verter_session/src/owner_import_surface.rs` and the `RouteDb` route-fact path — route-fact validation/invalidation that revalidates the selected leaf's content hash on each request, redirects a barrel re-export target's route fact on a barrel edit, drops a prior leaf from the route footprint when the barrel re-routes (path-precise invalidation), and invalidates a package-backed route fact on an in-place package source change (VFS is the authority; not file-hash-only). Concurrent cold requests collapse onto one materialization path.
- `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` + the route-demand projection path — reduce indexed-access projection through a barrel-renamed imported generic alias, reduce a cross-file terminal property, and combine `Parameters<T>[0]` with a cross-file indexed-access function property (the `CrossFileResolution` route-demand rows).

Deliverables:
- The typed three-layer `BudgetExceeded` non-admission across every hot reducer budget, with `ReturnOnly` for overflow / budget / cancellation / supersession / incomplete-rooting / unresolved-provenance.
- The five-dimension env-hash-split (R21) query-identity keys with content-free `DeclKey` (R6) and `ReadSetSignature.facts` as the sole validity rail; the multi-candidate `FamilySlots` admission substrate.
- Route-fact validation/invalidation: selected-leaf-edit propagation, barrel-route redirect + prior-leaf drop, package-source-change invalidation, single-materialization collapse; the cross-file route-demand projection.

Legacy deletions:
- Any boolean / sentinel / side-channel cache admission (replaced by the typed `ComputeAdmission` enum — Cache runtime hard rules: admission is typed, not boolean/sentinel/side-channel).
- Any bundled `project_config_hash` in a cache key (replaced by the five-dimension split — R21).
- Any content/version hash or `fact_dep_signature` on a query-identity cache KEY (version rooting moves to the cached value — R6).
- Any route-fact revalidation that is file-hash-only / does not consult tsconfig / vite alias / workspace-graph / package-target changes (replaced by the VFS-authoritative invalidation — Canonical Dependency Cache Rule).
- No projection-repair / second-engine resolution path remains on the route-demand projection (it routes through the one resolver). Stated explicitly per the template.

SemanticQueryKey/facts touched: no NEW key (the keys land in U2 / U6); this block lands their ADMISSION + budget behavior and the route-fact rails the `SemanticQueryKeySpec` table records. Facts read/validated: `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`, `MemberPresence` / `Member`, `LibIntrinsic`, `TypeEnvOptions`, and project-generation facts; the `ReadSetSignature.facts` validity rail over every cache family. Admission: every typed budget (`RelationBudget` / `KeyspaceBudget` / `CallResolutionBudget` / `FlowSliceBudget` / apparent-type member-demand) with `ReturnOnly` three-layer non-admission.

Exact test rows lifted (capability `CrossFileResolution`, `cross_file.rs` — all three `cross_file.rs` manifest rows, whose mechanism is the route-fact / cross-file indexed-access projection path):
- cross_file.rs::cross_file_projected_item_resolves_local_extension
- cross_file.rs::cross_file_projected_extra_resolves_number_terminal
- cross_file.rs::cross_file_label_parameter_resolves_local_item

(3 rows. The `CrossFileResolution` capability (3 rows) is listed as split U3/U6 in the Capability Map, but §10.4.1 resolves all three `cross_file.rs` manifest rows to THIS block — barrel-renamed imported generic alias projection, cross-file terminal projection, and `Parameters<T>[0]` over a cross-file indexed-access property — because each row's dominant mechanism is the route-fact / cross-file indexed-access path, not the flow-return slice. The U6 cross-file rows are the `flow_return_catalog.rs` `xf*` value-return rows (owned by `docs/arch/native-flow-return.md::U6.CROSS_FILE`), a disjoint set. No `cross_file.rs` row is double-counted.)

Required new guards (PART 1 §6; parent Guards index → Performance budgets — non-admission):
- `relation_budget_exceeded_admits_nothing`, `keyspace_budget_exceeded_admits_nothing`, `call_resolution_budget_exceeded_admits_nothing`, `apparent_type_budget_exceeded_admits_nothing` (if a budget's guard already landed in its owning U2/U6 reducer block, this block must not regress it; the route-fact-only budgets land here).
- The route-fact validation/invalidation discriminators are exercised by the `cache_invalidation.rs` rows owned by U10 / U11 (this block lands the rails; U10 / U11 gate the exactness — see those blocks).

Critical-rule guards: this block implements the parent's `(CRITICAL)` Cache Architecture / Canonical Dependency Cache / fact-based-cache rules (R1–R31). The typed-admission + per-budget non-admission + `ReadSetSignature` validity-rail + route-invalidation guards above are their R6 guards; the R21 split + R6 query-identity-key guards live in the existing fact-based-cache guard suite and must stay green. Any new `(CRITICAL)` rule text added to docs registers its guard here in the same change.

Proof requirement: per-row — the three `cross_file.rs` route-demand rows are TS7-oracle-pinned (`Ts7Oracle`) for the resolved projection shapes; each pairs the oracle with a structural route-fact assertion (`OracleAndGuard`) where it pins that only the requested leaf enters the route footprint (path-precision). Consumed by each row's generated wrapper. The budget non-admission guards are `StructuralGuard`-class.

Exit acceptance:
- The three `cross_file.rs` route-demand rows lift and pass; barrel-renamed / cross-file-terminal / `Parameters` projection resolves the terminal precisely.
- Every hot reducer budget returns typed `BudgetExceeded` that admits nothing (the four `*_budget_exceeded_admits_nothing` guards green); overflow / cancellation / supersession / incomplete-rooting / unresolved-provenance route through `ReturnOnly`.
- Query-identity cache keys carry no content/version hash or `fact_dep_signature` (R6) and no bundled `project_config_hash` (R21); `ReadSetSignature.facts` is the sole validity rail; concurrent cold requests collapse onto one materialization.

Verification commands:
- `cargo test --package verter_session` cross-file / route / budget / cache-discipline tests (`semantic_query_memo`, `cache_schema`, `owner_import_surface`).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate for this block's rows).
- The block's lifted-row proofs via the generated wrapper (or `cargo test … -- --ignored` pre-`prepare-verify`).
- Full workspace gate (as U8); `node scripts/gen-corpus-audit-tests.mjs` if audit fixtures change.

Docs updated: keep the `/type-cache-architecture` skill's R21/R6 + budget-non-admission rules current; update the Canonical Dependency Cache Rule notes in the `/type-resolution` skill for the route-fact validation contract.

Re-entry notes: idempotent under the one-engine + cache-validity guards. If partial, the manifest shows which `cross_file_*` rows remain `#[ignore]`. The typed admission rail is the load-bearing invariant — a `BudgetExceeded` that warm-admits anything fails its `*_admits_nothing` guard; do not add a sentinel/boolean admission side channel.

---

# U10 — Result DB + mode/demand exactness

## U10.RESULT_DB

ID: U10.RESULT_DB
Parent U-block: U10
Subplan: docs/arch/native-typeinfo-parity-cache-export-session.md

Prerequisites: U3.CACHE_FACT_MODEL, U8.WIRE_SURFACE_CLOSURE, U2 (parent), U6 (parent).
Blocked until: U3.CACHE_FACT_MODEL done (the result DB admits through U3's typed-admission rail) and U8.WIRE_SURFACE_CLOSURE done (the result DB caches the closed `TypeInfoGraphPayload` shape). The exporter (U12) and session surfaces (U11) read this DB; the mode/demand/expansion EXACTNESS gating it owns runs against the U2 reducer substrate.

Context: The final typeinfo-graph result needs a query-identity final-result cache — `TypeInfoGraphResultDb` — that hands out immutable `Arc<TypeInfoGraphPayload>` values, validates the entry's `ReadSetSignature.facts` against the live `StoreView` on every warm hit, and admits cold cacheable nodes through singleflight (mirroring the existing `ComponentMetaResultDb` / `MaterializeStructureDb` / `RefCycleResultDb` discipline — the project-global cache final state, the fact-based cache architecture). Its query-identity key excludes content/version hashes and `fact_dep_signature` (R6) and includes `lib_env_hash` (the cached value depends on lib data — PART 1 cache rules). Beyond the DB, U10 owns the mode / demand / expansion-boundary EXACTNESS gating over the U2 reducers: the `Identity` / `Navigate` / `Shallow` / `Expanded` / `Skeleton` mode contract must hold exactly (Identity returns the alias declaration identity, not its body / not a miss; Shallow exposes one shell level without recursive member-body expansion; Expanded `keyof T` emits the member-name literal-union from T's SHALLOW surface without entering member bodies; re-export chains transit every intermediate hop without leaving unresolved `Ref` shells), and the demand / expansion boundary must be path-precise (`Pick` does not load unpicked imports, `Omit` does not load the excluded import, inline / local / imported projection expands only the terminal path without sibling materialization). These rows' coverage `block_id` is U10 — the U2 reducers must satisfy them, but the EXACTNESS gating is owned here (`docs/arch/native-typeinfo-parity-u2-reducers.md` → "Row-level-split capabilities"). This block exists now because the result DB caches what the exporter publishes, and the mode/demand exactness is the gate that confirms the U2 reducers are path-precise at the published boundary.

Changes (exact files / functions):
- `crates/verter_session/src/project_type_store.rs` — add `TypeInfoGraphResultDb` to the `ProjectTypeStore` membership (the single project-global store accessed via `.project_type_store()`), keyed by query identity (R6: content-free `DeclKey`-style identity + the five-dimension env split incl. `lib_env_hash`), storing immutable `Arc<TypeInfoGraphPayload>` candidates in the multi-candidate `FamilySlots` substrate (cap 4, FIFO eviction) with per-candidate `ReadSetSignature.facts` + `validated_at_generation`; warm reads validate the fact signature against the live `StoreView` before return; cold cacheable nodes singleflight; in-flight joiners validate the winner against their own view.
- `crates/verter_session/src/host_construction.rs::project_type_store` — wire `TypeInfoGraphResultDb` into store construction alongside the existing result DBs (`ComponentMetaResultDb`, `MaterializeStructureDb`, `RefCycleResultDb`).
- `crates/verter_session/src/cache_schema.rs` — the `TypeInfoGraphResultDb` key composition (the env dimensions it depends on; `lib_env_hash` included).
- `crates/verter_session/src/typeinfo/raise.rs` + `crates/verter_session/src/typeinfo/evaluate_type_expression.rs` + the mode-boundary call sites (the `enumerate` / `evaluate` / `lower` object-lowering paths) — the EXACTNESS gating: `Identity` returns the alias declaration identity (`Ref` / `RecursiveRef`), not the body and not a semantic miss; `Shallow` exposes one shell level (member names + per-member reference nodes) without recursive member-body expansion — an interface member typed as an operator carrier (`Pick<...>`) stays a `Ref` / unevaluated operator, not a reduced `Object`; `Expanded` `keyof T` on an object literal/interface emits the literal-union of T's member names from T's SHALLOW member-name surface (member bodies must not enter the keyspace enumeration); re-export chains transit every intermediate hop without leaving unresolved `Ref` shells; `keyof (A & B)` enumerates the union of keys from both arms after fully resolving each arm.
- `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` + the projection-demand path — the demand / expansion boundary: `Pick<T, K>` does not load imports for keys K excludes; `Omit<T, K>` applies the excluded-key filter before loading the excluded branch; inline / local-alias / imported projection expands only the terminal path; imported aliases stay shallow until the consumer walks the path (Component-Meta Shallow-By-Default Rule).

Deliverables:
- `TypeInfoGraphResultDb` on `ProjectTypeStore`: immutable `Arc<TypeInfoGraphPayload>` values, query-identity key (R6 / R21), `ReadSetSignature.facts` warm-read validation, singleflight cold admission, multi-candidate `FamilySlots` storage.
- The mode-boundary EXACTNESS over the U2 reducers (`Identity` / `Shallow` / `Expanded` / re-export-chain / `keyof (A & B)`).
- The demand / expansion-boundary path-precision over the U2 reducers (`Pick` / `Omit` / inline / local / imported projection — no sibling materialization).

Legacy deletions:
- Any final-result typeinfo cache keyed on content/version hash or `fact_dep_signature` (R6 — query-identity keys exclude them; version rooting is on the value).
- Any `Identity`-mode path that returns the alias body or a semantic miss instead of the declaration identity (replaced by the contracted identity shape).
- Any `Shallow`-mode path that materialises a `Pick<...>` member body into a reduced `Object` (replaced by the one-shell-level shape; the object-lowering call site that propagated the caller's mode is corrected).
- Any `Expanded` `keyof` path that walks member bodies into the keyspace (replaced by the shallow member-name-surface enumeration).
- Any eager sibling materialization during `Pick` / `Omit` / path projection (path-precision: only the walked/selected hops load).
- No projection-repair / second-engine path remains in the result-DB / mode-boundary path. Stated explicitly per the template.

SemanticQueryKey/facts touched: no NEW key; the result DB caches the final `TypeInfoGraphPayload` keyed by query identity, and the mode/demand exactness gates the existing reducer keys (`Instantiate`, `IndexedAccess`, `KeyOf`, `MappedType`, `Conditional`, `ProjectPath`, `ResolveDecl` — the mode-boundary mechanisms per PART 1 §10.4). Facts read/validated: `MemberPresence` / `Member`, `RouteGeneration`, `ExportSurface`, `TypeEnvOptions`, project-generation facts; `ReadSetSignature.facts` over the result DB. Admission: singleflight on the result DB; the reducers' own budgets gate the mode/demand work; `ReturnOnly` on overflow / supersession.

Exact test rows lifted:
- capability `ModeBoundary` (`mode_boundary_invariants.rs`):
  - mode_boundary_invariants.rs::mode_boundary_keyof_deep_chain_is_bounded_in_expanded
  - mode_boundary_invariants.rs::mode_boundary_identity_does_not_materialize_alias_body
  - mode_boundary_invariants.rs::mode_boundary_shallow_does_not_expand_member_bodies
  - mode_boundary_invariants.rs::mode_boundary_reexport_chain_resolves_imported_alias
  - mode_boundary_invariants.rs::mode_boundary_keyof_across_reexport_chain_resolves_all_keys
- capability `ExpansionBoundaries` (`expansion_boundaries.rs`):
  - expansion_boundaries.rs::expansion_pick_does_not_load_unpicked_imports
  - expansion_boundaries.rs::expansion_omit_does_not_load_excluded_import
  - expansion_boundaries.rs::expansion_inline_details_projection_expands_only_terminal_inline_path
  - expansion_boundaries.rs::expansion_local_branch_projection_expands_target_without_sibling_meta
  - expansion_boundaries.rs::expansion_imported_projection_loads_selected_but_not_unselected_branch
  - expansion_boundaries.rs::expansion_imported_terminal_projection_reduces_flag_without_unselected_branch
- capability `DemandBoundary` (`demand_boundary.rs`) — the projection-demand-exactness subset whose mechanism is reducer mode/demand (the footprint-attachment subset is owned by U11):
  - demand_boundary.rs::demand_boundary_projection_into_selected_alias_loads_needed_but_not_unused
  - demand_boundary.rs::demand_boundary_terminal_projection_resolves_value_without_unused_branch

(13 rows: 5 `mode_boundary_invariants.rs` + 6 `expansion_boundaries.rs` + 2 `demand_boundary.rs` projection-exactness rows. These are mode / demand / expansion-EXACTNESS rows: the U2 reducers EXERCISE them — the U2 reducers are path-precise and member-demand aware — but their exactness gating `block_id` is this U10 block, not a U2 block, per `docs/arch/native-typeinfo-parity-u2-reducers.md` → "Row-level-split capabilities". The third `demand_boundary.rs` row, whose mechanism is the footprint-attachment pipeline — `demand_boundary_barrel_resolution_does_not_load_unrequested_reexport` — is owned by U11.PUBLIC_RELATION_SESSION. §10.4.1 assigns each manifest row to exactly one `block_id`.)

Required new guards: no NEW `(CRITICAL)` engine rule of its own. The mode-boundary EXACTNESS implements the existing Macro-Type-Traversal mode contract and the Component-Meta Shallow-By-Default Rule, pinned by their existing traversal / projector guards (`/type-resolution` mode-contract guards; `crates/verter_session/src/meta_tests.rs` shallow-by-default negative tests) — this block must not regress them. The result-DB admission rides on the U3 typed-admission + `ReadSetSignature` validity rail (no new admission guard). Stated explicitly per the template.

Critical-rule guards: none new — implements the existing Macro-Type-Traversal mode contract, the Component-Meta Shallow-By-Default Rule, and the fact-based-cache result-DB discipline. The result DB's `ReadSetSignature.facts` warm-read validation must satisfy the existing cache-validity guards.

Proof requirement: per-row — the `ModeBoundary` rows are `OracleAndGuard` (a TS7 oracle pin on the resolved mode shape paired with a structural non-materialization assertion that member bodies / sibling branches do not enter the keyspace / object shape); the `ExpansionBoundaries` / `DemandBoundary` rows are `OracleAndGuard` pairing the oracle terminal-projection result with a structural assertion that the unpicked / excluded / unselected branch is NOT loaded (path-precision). Consumed by each row's generated wrapper.

Exit acceptance:
- All five `mode_boundary_invariants.rs` rows, all six `expansion_boundaries.rs` rows, and the two `demand_boundary.rs` projection-exactness rows lift and pass.
- `Identity` returns the alias declaration identity (not body / not miss); `Shallow` exposes one shell level (operator carriers stay `Ref`); `Expanded` `keyof` is bounded to the shallow member-name surface; re-export chains transit cleanly; `keyof (A & B)` enumerates both arms.
- `Pick` / `Omit` / inline / local / imported projection is path-precise (no unpicked / excluded / unselected branch loaded).
- `TypeInfoGraphResultDb` hands out immutable `Arc<TypeInfoGraphPayload>`, validates `ReadSetSignature.facts` on warm hits, singleflights cold admission, and excludes content/version hashes + `fact_dep_signature` from its key (R6).

Verification commands:
- `cargo test --package verter_session` mode-boundary / expansion / demand / result-DB tests (`project_type_store`, `cache_schema`, the typeinfo mode-boundary tests).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U8).

Docs updated: keep the `/type-resolution` mode-contract section and the `/type-cache-architecture` result-DB membership notes current (add `TypeInfoGraphResultDb` to the `ProjectTypeStore` membership list).

Re-entry notes: idempotent. If partial, the manifest shows which `mode_boundary_invariants` / `expansion_boundaries` / `demand_boundary` rows remain `#[ignore]`. The object-lowering call site that propagates the caller's mode is the load-bearing fix for the `Shallow` / `Expanded` rows — do not let object lowering reduce member bodies at one shell level.

---

# U11 — Public relation / session surfaces

## U11.PUBLIC_RELATION_SESSION

ID: U11.PUBLIC_RELATION_SESSION
Parent U-block: U11
Subplan: docs/arch/native-typeinfo-parity-cache-export-session.md

Prerequisites: U12.EXPORTER, U3.CACHE_FACT_MODEL, U8.WIRE_SURFACE_CLOSURE, U2 (parent), U6 (parent).
Blocked until: U12.EXPORTER done (the public session surface returns the exporter's `TypeInfoGraphPayload` / `RelationPayload`), U3.CACHE_FACT_MODEL done (footprint attachment + route invalidation observe U3's route-fact rails), and the whole U2 + U6 parents done (relation + flow are the surfaces exposed).

Context: The public-facing session surface must expose the relation result as the public `RelationPayload` (outcome / bindings / proof + typed `BudgetExceeded`), not the bare tri-state `RelationResult` (PART 1 §§2.7, 3, 4) — `relate` is the sole assignability authority and returns its proof off the type-values surface through `RelationPayload`. The host audit runtime must attach a request footprint on every audited typeinfo resolver path when `footprint_capture=true` on `HostConfig` (the audit-passive-observer footprint-attachment pipeline — the audit infrastructure), so an audited request records its declared dependency footprint precisely (only the requested leaf / projected member appears; unselected branches are excluded; the warm cross-file dependency footprint stays attached on warm reads). Cache invalidation across an edit cycle must hold end-to-end at the session boundary: a selected-leaf edit flips the published surface; an unselected sibling edit keeps the warm cache (zero VFS reads, zero RouteDb misses, footprint still attached); a barrel edit redirects the route + drops the prior leaf from the V2 footprint; a side-effect-imported augmentation patch surfaces the augmented shape on edit; an in-place package source change flips the published surface. This block exists now because the footprint pipeline and the public relation surface sit at the session boundary the exporter feeds, and the cache-invalidation rows are end-to-end edit-cycle contracts over the U3 route-fact rails + the U12 exporter.

Changes (exact files / functions):
- `crates/verter_session/src/semantic_query.rs` — the public `RelationPayload` value (outcome / inference bindings / relation proof + typed `BudgetExceeded`), replacing the bare tri-state `RelationResult` on the public path (the `Relate` query value domain is `SemanticQueryValue::Relation(RelationPayload)` — PART 1 §3; the relation engine itself lands in `docs/arch/native-typeinfo-parity-u2-reducers.md::U2.RELATION_INFER`, this block exposes its result publicly).
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` — the public `execute_relation` wrapper (over the shared `SemanticGraphStore` admission/inflight substrate) handing out the `RelationPayload`; the relation proof carried through `RelationPayload` / the payload-side proof table, never a `GraphTypeNode` arm.
- The host audit runtime (the `HostAuditRuntime` + accumulator + footprint miner in `verter_session`, per the audit infrastructure / `/audit-infrastructure` skill) — attach a `RequestAuditRecord` footprint on every audited typeinfo resolver path when `footprint_capture=true`; the footprint reports the requested import / projected indexed-access members precisely and excludes unprojected branches; warm reads keep the cross-file dependency footprint attached.
- `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` (the `resolve_named_symbol_with_audit` entrypoint) + `crates/verter_session/src/owner_import_surface.rs` + the `RouteDb` invalidation path — wire the footprint-attachment pipeline into the resolver path; honor the end-to-end edit-cycle invalidation (selected-leaf flip, unselected-sibling warm reuse, barrel-route redirect + prior-leaf drop, augmentation-patch-on-edit, in-place-package-edit flip) over the U3 route-fact rails.
- `crates/verter_semantic/src/analysis/` — discover module augmentations contributed by a side-effect-imported patch file (the canonical Vue/Vite augmentation pattern) so the augmented shape surfaces across an edit cycle (`augmentation_index` lookup by `AugmentationTargetKey` — Cache Architecture).

Deliverables:
- The public `relate` surface returning `RelationPayload` (outcome / bindings / proof + typed `BudgetExceeded`), with the proof off the type-values surface.
- The request-footprint attachment pipeline on every audited typeinfo resolver path (footprint reports the requested import / projected members precisely; warm reads keep the footprint attached).
- End-to-end edit-cycle cache invalidation at the session boundary: selected-leaf flip, unselected-sibling warm reuse (zero VFS reads / zero RouteDb misses), barrel-route redirect + prior-leaf drop, augmentation-patch-on-edit, in-place-package-edit flip.

Legacy deletions:
- The bare tri-state `RelationResult` on the public path (replaced by `RelationPayload`; the internal relation engine's `RelationResult` is superseded by `RelationPayload` per U2.RELATION_INFER — this block removes any residual public tri-state surface).
- Any resolver path that records the scratch/owner footprint but does not attribute projected imported indexed-access members precisely (replaced by the demand-bounded footprint).
- Any cache participant that invalidates on an unreferenced barrel sibling edit (replaced by path-precise invalidation: only the route-reached participant is touched).
- No projection-repair / second-engine path remains on the public session surface (relation routes through the one engine; resolution through the one resolver). Stated explicitly per the template.

SemanticQueryKey/facts touched: `Relate` (public value domain `Relation(RelationPayload)`). Facts read/validated at the session boundary: `RouteGeneration`, `ExportSurface`, `ModuleAugmentation`, `AmbientGlobal`, `MemberPresence` / `Member`, `LibIntrinsic`, `TypeEnvOptions`, project-generation facts; the footprint records the declared dependency facts the request touched. Admission: `RelationBudget` (full-identity keyed) with `ReturnOnly` on `BudgetExceeded`; route-fact invalidation through the U3 rails.

Exact test rows lifted:
- capability `AuditFootprint` (`footprint.rs`):
  - footprint.rs::typeinfo_footprint_is_attached_for_named_symbol_request
  - footprint.rs::typeinfo_footprint_reports_requested_import_and_excludes_unprojected_branch
- capability `CacheInvalidation` (`cache_invalidation.rs`):
  - cache_invalidation.rs::cache_invalidation_basic_selected_leaf_edit_flips_published_surface
  - cache_invalidation.rs::cache_invalidation_unselected_leaf_edit_keeps_warm_cache
  - cache_invalidation.rs::cache_invalidation_barrel_edit_redirects_route_to_new_leaf
  - cache_invalidation.rs::cache_invalidation_barrel_edit_excludes_prior_leaf_from_v2_footprint
  - cache_invalidation.rs::cache_invalidation_aug_patch_edit_surfaces_augmented_shape
  - cache_invalidation.rs::cache_invalidation_in_place_package_edit_flips_published_surface
- capability `DemandBoundary` (`demand_boundary.rs`) — the footprint-attachment subset (the projection-exactness subset is owned by U10):
  - demand_boundary.rs::demand_boundary_barrel_resolution_does_not_load_unrequested_reexport

(9 rows: 2 `footprint.rs` + 6 `cache_invalidation.rs` + 1 `demand_boundary.rs` footprint-attachment row. The `RelationSemantics` rows are lifted by `docs/arch/native-typeinfo-parity-u2-reducers.md::U2.RELATION_INFER` — the relation ENGINE — NOT here; this U11 block lifts the footprint / cache-invalidation / public-surface rows whose mechanism is the audit-footprint pipeline or the end-to-end edit-cycle route invalidation. §10.4.1 assigns each manifest row to exactly one `block_id`; no `RelationSemantics` row is double-counted here.)

Required new guards (PART 1 §3; parent Guards index → Query keys — value domain):
- `relate_query_value_carries_relation_proof_and_budget_state` (the public `Relate` value is `RelationPayload` carrying the proof + typed `BudgetExceeded`, if not already landed in U2.QUERY_VALUE_DOMAIN; this block exercises the public surface).
- `relation_proofs_not_graph_type_nodes`, `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node` (the public proof stays off the type-values surface — these also land in U8; this block must keep them green at the session boundary).

Critical-rule guards: this block implements the parent's `(CRITICAL)` typed-value-domain (public relation surface) and audit-infrastructure rules; the relation-payload + relation-proof-off-graph guards above are their R6 guards. If this block lands any new `(CRITICAL)` footprint-attachment rule text in docs, it registers the corresponding guard here in the same change.

Proof requirement: per-row — the `footprint.rs` and the cache-invalidation / `demand_boundary` footprint rows are `OracleAndGuard` (a TS7 oracle pin on the resolved surface paired with a structural assertion on the attached footprint's contents — the requested leaf present / the unprojected branch absent / the prior leaf dropped on the V2 footprint / zero VFS reads on the unselected-sibling edit); the edit-cycle invalidation rows pin the V1→V2 surface flip (or warm reuse) plus the footprint delta (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance:
- All rows above lift and pass; a footprint is attached on every audited typeinfo request when `footprint_capture=true`, reporting the requested import / projected members and excluding unprojected branches.
- `relate` returns `RelationPayload` (outcome / bindings / proof + typed `BudgetExceeded`) with the proof off `GraphTypeNode`.
- Selected-leaf edit flips the surface; unselected-sibling edit keeps the warm cache (zero VFS reads, zero RouteDb misses, footprint attached); barrel edit redirects the route and drops the prior leaf from the V2 footprint; augmentation-patch edit surfaces the augmented shape; in-place package edit flips the surface.

Verification commands:
- `cargo test --package verter_session` footprint / cache-invalidation / demand-boundary / relation-payload tests; the audit-contract guards (`typeinfo_audit_contract_guards`).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U8); `node scripts/gen-corpus-audit-tests.mjs` (audit footprint fixtures changed).

Docs updated: keep the `/audit-infrastructure` skill's footprint-attachment pipeline notes current; update the `/component-meta` native-vs-compat notes if the public relation surface contract changes.

Re-entry notes: idempotent. If partial, the manifest shows which `footprint` / `cache_invalidation` / `demand_boundary` rows remain `#[ignore]`. The footprint-attachment pipeline is wired into the resolver path once — do not attach a second footprint surface; the public relation surface returns `RelationPayload` only — do not reintroduce the bare tri-state.

---

# U12 — Exporter

## U12.EXPORTER

ID: U12.EXPORTER
Parent U-block: U12
Subplan: docs/arch/native-typeinfo-parity-cache-export-session.md

Prerequisites: U10.RESULT_DB, U8.WIRE_SURFACE_CLOSURE, U3.CACHE_FACT_MODEL, U2 (parent), U6 (parent).
Blocked until: U8.WIRE_SURFACE_CLOSURE done (the exporter populates the closed `TypeInfoGraphPayload` shape) and U10.RESULT_DB done (the exporter's published payload is admitted into `TypeInfoGraphResultDb`). The session surfaces (U11) and the projection (U13) consume the exporter's output.

Context: The exporter is the request → graph projection that materialises the engine's typed value-domain results into the closed wire payload `TypeInfoGraphPayload { graph, program_analysis, declaration_surfaces, diagnostics, diagnostic_directives, relation_proofs }` and the `RelationPayload` for public `relate` (PART 1 §§1.3–1.5, §3). It maps type values to the closed `GraphTypeNode` type-value surface on `graph`, flow / contextual facts to `ProgramAnalysisGraph` on `program_analysis`, declaration / environment-mutation facts to `DeclarationAnalysisGraph` on `declaration_surfaces`, diagnostics / diagnostic directives to their payload side tables, and relation proofs to the payload-side `relation_proofs` proof table referenced by opaque proof id — never smuggling a non-type value into a `GraphTypeNode` arm. The exporter is a thin projection of the engine's results, not a second resolver: it reads the typed `SemanticQueryValue` results (TypeNode / ProgramAnalysis / DeclarationAnalysis / Relation) and writes the wire shape; it does no query-time resolution. It exists now because the result DB caches what the exporter publishes, and the public session surface + the TS projection both decode the exporter's payload.

Changes (exact files / functions):
- `crates/verter_session/src/typeinfo/surface.rs::TypeInfoSurface::build` and the request → graph projection it drives — produce `TypeInfoGraphPayload`: project the engine's `SemanticQueryValue::TypeNode` results onto the closed `GraphTypeNode` type-value allowlist (PART 1 §1.3), the `ProgramAnalysis` results onto `ProgramAnalysisGraph { flow_narrowings, contextual_types }`, the `DeclarationAnalysis` results onto `DeclarationAnalysisGraph { module_augmentations, global_augmentations }`, diagnostics onto `diagnostics` / `diagnostic_directives`, and relation proofs onto the `relation_proofs` proof table by opaque proof id. No non-type value is materialised as a `GraphTypeNode` arm.
- `crates/verter_session/src/typeinfo/raise.rs` + `crates/verter_session/src/typeinfo/evaluate_type_expression.rs` — the per-node raise/evaluate path that lifts a `SemanticNodeId` / typed value into its `GraphTypeNode` / side-table wire representation; the relation-proof id allocation + table population.
- `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` (`resolve_named_symbol_with_audit`) — the request entrypoint that drives the exporter and publishes the `TypeInfoGraphPayload` into `TypeInfoGraphResultDb` (U10) via cooperative admission; the `RelationPayload` for the public `relate` operation (U11 exposes it).
- `crates/verter_protocol/src/typeinfo/graph.rs` — the Rust→wire encoding helpers the exporter uses for the new payload messages (`TypeInfoGraphPayload`, `ProgramAnalysisGraph`, `DeclarationAnalysisGraph`, `RelationPayload`, the proof table), mirroring the existing `wire_*` helper discipline.

Deliverables:
- The exporter projecting the engine's typed results into the closed `TypeInfoGraphPayload` (type values on `graph`; flow/contextual on `program_analysis`; declaration/environment facts on `declaration_surfaces`; diagnostics on their side tables; relation proofs on `relation_proofs` by proof id) and the `RelationPayload` for public `relate`.
- Publication of the payload into `TypeInfoGraphResultDb` (U10) via cooperative admission; no non-type value on a `GraphTypeNode` arm.

Legacy deletions:
- Any exporter path that emits flow / contextual / relation-proof / module-or-global-augmentation facts as `GraphTypeNode` arms (replaced by the relocated payload side tables — PART 1 §§1.3–1.5; the recovered-doc §8 exporter `TypeNode::ModuleAugmentation` DTO is amended to `DeclarationAnalysisGraph`).
- Any exporter path that re-resolves a type at projection time (the exporter is a thin projection of the engine's typed results — the one-resolver rule; OXC is never a query-time resolver, and projection never re-resolves).
- No projection-repair / second-engine path remains in the exporter. Stated explicitly per the template.

SemanticQueryKey/facts touched: no NEW key; the exporter READS the typed `SemanticQueryValue` results (`TypeNode` / `ProgramAnalysis` / `DeclarationAnalysis` / `Relation`) and writes the wire payload. Facts read: the results' `ReadSetSignature.facts` (the published payload's validity rail, validated by the U10 result DB on warm hits). Admission: the exporter's payload admits into `TypeInfoGraphResultDb` (U10) via cooperative admission; `ReturnOnly` on a non-cacheable / superseded result.

Exact test rows lifted: none directly. The exporter is the projection substrate the published-surface and TS-projection rows ride on; it lifts no `#[ignore]` row by itself. Its discriminating proof is the wire-projection guards (the exporter never emits a non-type value as a `GraphTypeNode` arm — `no_non_type_value_smuggled_into_graph_type_node`; flow/contextual/declaration/relation-proof facts land on their relocated side tables) plus the typeinfo type-value rows lifted by U13 reading the exporter's payload.

Required new guards: no NEW guard of its own — the exporter's correctness rides on U8's wire-purity guards (`no_non_type_value_smuggled_into_graph_type_node`, `program_analysis_graph_exposes_flow_contextual_queries`, `relation_proofs_not_graph_type_nodes`, `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`, `all_public_semantic_type_graph_embeddings_are_payload_wrapped`) which assert the exporter's OUTPUT shape; the exporter must keep them green. Stated explicitly per the template.

Critical-rule guards: none new — implements the parent's `(CRITICAL)` Typed-IR-Only Resolver Rule (the exporter projects typed results, never re-resolves at projection time) and the Typeinfo Wire Contract (the exporter populates only the closed payload shape). The U8 wire-purity guards are these rules' R6 guards; the exporter must not regress them.

Proof requirement: structural — the U8 wire-purity guards over the exporter's output (no non-type value on a `GraphTypeNode` arm; flow/contextual/declaration/relation-proof facts on their relocated side tables) are the exporter's `StructuralGuard`-class proof; the type-value projection correctness is proven through the U13 published-surface rows that read the exporter's payload.

Exit acceptance:
- The exporter produces `TypeInfoGraphPayload` with type values on `graph`, flow/contextual on `program_analysis`, declaration/environment facts on `declaration_surfaces`, diagnostics on their side tables, and relation proofs on `relation_proofs` by proof id — `no_non_type_value_smuggled_into_graph_type_node` green.
- The `RelationPayload` for public `relate` carries the proof off the type-values surface (U8 relation-proof guards green).
- The payload is admitted into `TypeInfoGraphResultDb` (U10) via cooperative admission; the exporter does no query-time resolution.

Verification commands:
- `cargo test --package verter_session` typeinfo surface / raise / evaluate tests; the wire-surface guards (`typeinfo_wire_surface_guards`, `typeinfo_graph_contract_guards`).
- `cargo test --package verter_protocol --test typeinfo_proto_roundtrip` (the exporter's payload round-trips).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate — the exporter enables the U13 published-surface rows).
- Full workspace gate (as U8).

Docs updated: amend the recovered-doc §8 exporter `TypeNode::ModuleAugmentation` DTO wording → `DeclarationAnalysisGraph` on `TypeInfoGraphPayload.declaration_surfaces`; keep the `/type-cache-architecture` exporter / payload notes current.

Re-entry notes: idempotent. The exporter is a thin projection — if a guard reports a non-type value on a `GraphTypeNode` arm, the exporter is emitting to the wrong surface; relocate it to the payload side table. Do not add a query-time resolution branch in the exporter.

---

# U13 — Published projection

## U13.PROJECTION

ID: U13.PROJECTION
Parent U-block: U13
Subplan: docs/arch/native-typeinfo-parity-cache-export-session.md

Prerequisites: U12.EXPORTER, U8.WIRE_SURFACE_CLOSURE, U2 (parent), U6 (parent).
Blocked until: U12.EXPORTER done (the projection decodes the exporter's `TypeInfoGraphPayload`) and U8.WIRE_SURFACE_CLOSURE done (the projection reads the closed wire shape). This is the published-surface block U14 / U15 and the host-backed consumers (`@verter/component-meta`, LSP, MCP, unplugin, playground) read.

Context: The published projection is the closed `GraphTypeNode` type-value projection of every U2 type value, plus the TS `TypeDescriptor` projection (`@verter/type-ir` + `@verter/component-meta`) that consumes the wire payload STRUCTURALLY — driving every semantic decision from the typed IR, never from raw / display strings (the Typed-IR-Only Resolver Rule, the Component-Meta Native vs Compat Rule). The TS compat layer reads `prop.type` (`TypeDescriptor`) for every semantic decision; `prop.rawType` is display passthrough only and must not feed any `looksLike*` / `extract*` / `normalize*` / `split*` / `strip*` / `prefer*` branch. Type-role classification is structural (a type is a "prop type" because a macro consumes it, not because its identifier ends with `"Props"`). This block exists now to publish the engine's results to consumers: the published `GraphTypeNode` projection of U2 type values, decoded structurally into `TypeDescriptor`, with the published typeinfo type-value rows lifting against the published surface.

Changes (exact files / functions):
- `crates/verter_session/src/typeinfo/surface.rs` (the published `TypeInfoSurface` projection) — the closed `GraphTypeNode` type-value projection: every U2 type value (object / union / intersection / literal / tuple / reference / alias-instantiation / type-parameter / keyof / indexed-access / conditional / mapped / template-literal / typeof / satisfies / class / this-type / merged-declaration / ambient-module / ambient-namespace / infer / enum / opaque / cycle — PART 1 §1.3) projects to its closed allowlist arm; no non-type value enters the projection (it reads the exporter's relocated side tables for flow/contextual/declaration/relation-proof facts).
- `packages/type-ir/src/type-ir.ts` — the `TypeDescriptor` schema covering the published type-value variants (the TS-side typed IR the compat layer reads); any missing variant added here rather than recovered through text in the consumer.
- `packages/component-meta/src/type-graph-decode.ts` (`decodeComponentMetaPayload`) + `packages/component-meta/src/type-graph-proto-decode.ts` + `packages/component-meta/src/compat/native-projection.ts` (its consumer) — decode the wire `TypeInfoGraphPayload` (the `graph` type-value surface + the `program_analysis` / `declaration_surfaces` / `diagnostics` / `relation_proofs` side tables) into the TS `TypeDescriptor` structurally; read the relocated side tables for the moved-off facts, never a `GraphTypeNode` arm.
- `packages/component-meta/src/type-expr-bridge.ts` + `packages/component-meta/src/type-graph.ts` — the TS `TypeDescriptor` bridge that drives semantic decisions from the typed IR; no raw/display-string parsing.
- `packages/component-meta/src/compat/checker.ts` (`mapPropMeta` / `mapEventMeta` / `mapSlotMeta` / `mapExposedMeta` / `mapComponentMeta`) + `packages/component-meta/src/compat/schema.ts` (`typeDescriptorToSchema` / `typeDescriptorToString`) — the `vue-component-meta` interop projection reading `prop.type` (`TypeDescriptor`) for every semantic decision; `prop.rawType` display-only; type-role classification structural (macro participation, not identifier suffix).

Deliverables:
- The closed `GraphTypeNode` type-value projection of every U2 type value on the published `TypeInfoSurface` (no non-type value in the projection).
- The TS `TypeDescriptor` projection (`@verter/type-ir` schema + `@verter/component-meta` structural decode + compat interop) consuming the wire payload structurally, driving semantic decisions from the typed IR.

Legacy deletions:
- Any published projection that emits a flow / contextual / declaration / relation-proof fact as a `GraphTypeNode` arm (replaced by reading the relocated side tables — PART 1 §§1.3–1.5).
- Any TS compat branch that drives a semantic decision from `prop.rawType` / a raw / display string (`looksLike*` / `extract*` / `normalize*` / `split*` / `strip*` / `prefer*` / `shouldPrefer*` / `repairOpaque*`) — replaced by reading `prop.type` (`TypeDescriptor`) structurally (Typed-IR-Only Resolver Rule; the `@verter/component-meta` no-rawtype-reads contract).
- Any identifier-name-suffix type-role classification (`name.ends_with("Props")` etc.) — replaced by structural macro-participation classification.
- Any AST/source fallback or second resolver/expander in the TS projection (cache-owned type recovery only — Component-Meta Native vs Compat Rule).
- No projection-repair / second-engine path remains in the published projection. Stated explicitly per the template.

SemanticQueryKey/facts touched: no NEW key; the projection READS the published `TypeInfoGraphPayload` (the exporter's output) and decodes it into `TypeDescriptor`. Facts read: none directly (the projection consumes the closed payload; the published surface's validity is the U10 result-DB `ReadSetSignature.facts` rail). Admission: none (the projection is a structural decode of an already-admitted payload).

Exact test rows lifted: none. U13 owns ZERO `IgnoredTestRow`s — every published typeinfo type-value row (object / union / literal / tuple / reference / class / enum surfaces) is owned by the U2/U6 block whose reducer COMPUTES it (e.g. `enums.rs` rows → `U2.ENUMS`, `class_features.rs` rows → `U2.CLASS_SURFACES`, narrowing rows → `U6.NARROWING`), per §10.4.1; a row's owning `block_id` is its compute mechanism, not the publication step. U13 is the structural-decode substrate those rows ride through to the wire, so it lifts no `#[ignore]` row by itself (the same "substrate block owns no rows" status as `U8.WIRE_SURFACE_CLOSURE` and `U12.EXPORTER`). Its discriminating proof is the wire-projection / structural-decode guard suite below plus the existing `@verter/component-meta` no-rawtype-reads + published-surface-parity tests, which run against the rows the owning U2/U6 blocks lift through this projection. `capability_rows_map_to_expected_query_fact_mechanisms` asserts no published-surface row is mapped to U13.

Required new guards: no NEW `(CRITICAL)` engine rule of its own — the projection implements the existing Typed-IR-Only Resolver Rule, the Component-Meta Native vs Compat Rule, and the published-surface structural-decode contract, pinned by their existing guards (the `@verter/component-meta` no-rawtype-reads contract — `packages/component-meta/tests/no-rawtype-reads.spec.ts`; the architecture-guard list for the typed-IR-only pipeline; the published-surface constants parity — `crates/verter_audit/tests/published_surface_constants_match_ts_port.rs`). This block must not regress them.

Critical-rule guards: none new — implements the existing Typed-IR-Only Resolver Rule and Component-Meta Native vs Compat Rule. The no-rawtype-reads + structural-classification + published-surface-parity guards are these rules' R6 guards; this block must keep them green.

Proof requirement: per-row — the published type-value projection rows are TS7-oracle-pinned (`Ts7Oracle`) for the published surface shapes; rows that pin the structural-decode / no-rawtype contract pair the oracle with the structural no-rawtype / structural-classification assertion (`OracleAndGuard`). Consumed by each row's generated wrapper. The TS-side structural contract is pinned by the existing `no-rawtype-reads` + published-surface-parity tests.

Exit acceptance:
- The published `GraphTypeNode` projection of every U2 type value is closed (only the allowlist arms appear; no non-type value in the projection) — the U8 wire-purity guards green over the published surface.
- The TS `TypeDescriptor` projection decodes the wire payload structurally; the compat layer drives every semantic decision from `prop.type`; `prop.rawType` is display-only (the no-rawtype-reads guard green); type-role classification is structural.
- U13 lifts no `IgnoredTestRow` of its own; every published type-value row stays owned by its computing U2/U6 block (§10.4.1). The published projection of every owning block's rows passes through the closed `GraphTypeNode` surface with the wire-purity guards green, and no published-surface row is mapped to U13.

Verification commands:
- `cargo test --package verter_session` published-surface tests; `cargo test --package verter_audit --test published_surface_constants_match_ts_port` (Rust/TS published-surface parity).
- `pnpm vitest --run packages/component-meta/tests/no-rawtype-reads.spec.ts` and the `@verter/component-meta` type-graph / native-projection / compat checker+schema spec suites.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U8) PLUS `pnpm test` (the TS projection is exercised).

Docs updated: keep the `/component-meta` native-vs-compat + Typed-IR-Only sections and the `/type-resolution` typed-schema-contract notes current; update the `@verter/type-ir` schema docs if a `TypeDescriptor` variant is added.

Re-entry notes: idempotent. U13 owns no `IgnoredTestRow`s, so its progress is measured by the wire-purity / structural-decode guards (not by manifest rows); the published type-value rows it carries remain owned by their computing U2/U6 blocks. The TS projection is structural-decode only — if a compat branch reaches for `prop.rawType` to recover meaning, the no-rawtype-reads guard fails; fix the producer (add the missing `TypeDescriptor` variant) rather than parsing text. Do not reintroduce an AST/source fallback or a second TS resolver.

---

## Row-ownership note (no double-counting)

This subplan's blocks own only the rows whose coverage `block_id` is one of their
blocks; no row is owned twice with any other subplan. The split, stated explicitly
so the binding 363 `IgnoredTestRow` total stays exact (Capability Map; PART 2
§10.4–10.5):

- **`CrossFileResolution` (3)** — listed as split U3 / U6 in the Capability Map, but
  §10.4.1 resolves ALL three `cross_file.rs` manifest rows (barrel-renamed imported
  generic alias projection, cross-file terminal projection, `Parameters<T>[0]` over a
  cross-file indexed-access property) to **U3.CACHE_FACT_MODEL** — their dominant
  mechanism is the route-fact / cross-file indexed-access path. The U6 cross-file rows
  are the disjoint `flow_return_catalog.rs` `xf*` value-return rows (U6.CROSS_FILE), not
  any `cross_file.rs` row.
- **`ModeBoundary` (5)** — all five `mode_boundary_invariants.rs` rows lift in
  **U10.RESULT_DB** (mode-EXACTNESS over the U2 reducers).
- **`ExpansionBoundaries` (6)** — all six `expansion_boundaries.rs` rows lift in
  **U10.RESULT_DB** (expansion-EXACTNESS over the U2 reducers).
- **`DemandBoundary` (3)** — split U10 / U11. The two projection-exactness rows
  (`demand_boundary_projection_into_selected_alias_loads_needed_but_not_unused`,
  `demand_boundary_terminal_projection_resolves_value_without_unused_branch`) lift
  in **U10.RESULT_DB**; the footprint-attachment row
  (`demand_boundary_barrel_resolution_does_not_load_unrequested_reexport`) lifts in
  **U11.PUBLIC_RELATION_SESSION**.
- **`CacheInvalidation` (6)** — all six `cache_invalidation.rs` rows lift in
  **U11.PUBLIC_RELATION_SESSION** (end-to-end edit-cycle invalidation over the U3
  route-fact rails + the U12 exporter). U3 lands the route-fact rails these rows
  exercise; U11 gates the end-to-end edit-cycle behavior.
- **`AuditFootprint` (2)** — both `footprint.rs` rows lift in
  **U11.PUBLIC_RELATION_SESSION** (the footprint-attachment pipeline).
- **`RelationSemantics` (10)** — lifted by
  `docs/arch/native-typeinfo-parity-u2-reducers.md::U2.RELATION_INFER` (the relation
  ENGINE), NOT here. U11 exposes the public `RelationPayload` surface but lifts no
  `RelationSemantics` row; those are U2 rows.
- The `ModeBoundary` / `ExpansionBoundaries` / `DemandBoundary` rows are EXERCISED by
  the U2 reducers (the reducers are path-precise and member-demand aware) but their
  EXACTNESS gating `block_id` is U10 / U11, not a U2 block — the U2 subplan's
  "Row-level-split capabilities" note states this reciprocally, so neither subplan
  double-claims them.
- **Substrate blocks own zero rows.** `U8.WIRE_SURFACE_CLOSURE`, `U12.EXPORTER`, and
  `U13.PROJECTION` lift NO `IgnoredTestRow`s — they build the wire / exporter /
  projection substrate the owning U2/U6/U10/U11 blocks lift their rows through; each
  block's `Exact test rows lifted` is explicitly `none`, and §10.4.1 maps no row to
  them.

In every case the parent's §10.4.1 partition (the generated coverage table, PART 2
§10.4) is the authority: each row maps to exactly one `block_id` via its
`mechanism_id`, and `capability_rows_map_to_expected_query_fact_mechanisms` asserts
the mapping is consistent with the capability. The binding 363 total stays exact.

---

## Verification (whole-subplan)

Every block runs the full workspace gate as its final acceptance step (PART 2
§§11.3, 14) — the complete Rust **AND** JavaScript gate, green only when BOTH pass;
non-mutating — clippy-fix / formatting run BEFORE `prepare-verify`
so the gated content is already clean and the receipt certifies an unchanging
input: `cargo test --workspace --tests --verbose`, `cargo clippy --workspace --
-D warnings`, `cargo fmt --all --check`, `pnpm test` (U13 in particular exercises the
TS projection), and `pnpm install --frozen-lockfile`. A block reaches `Lifted` /
`landed_blocks` only after a green workspace gate over the EXACT accepted content AND
the three-reviewer LAND verdict (1 Claude Code + 2 codex; PART 2 §11.12), via the
two-phase prepare-verify → gate → accept lifecycle with the input-hash-PAIR gate
receipt (PART 2 §11.3); the block's WIP series squashes to ONE mainline land commit at
`accept` (PART 2 §11.11). The parent
U3 / U8 / U10 / U11 / U12 / U13 tokens are each the aggregate over their child block
and are done only when every row in the union of their child block's row-set is
`Lifted` (PART 2 §11.9) — never vacuously. Downstream U14 / U15 stay blocked until
every block in this subplan — and the whole U2 / U6 parents — are done.

The whole-subplan parity guarantee is the parent's composition (Capability Map →
"The guarantee over the 363 rows"): the two-table ledger with the exact-363 count +
bijection (PART 2 §§10.1, 10.5); the U0 row-exact coverage table that DEFINES
completeness (PART 2 §10.4); the per-row executable `ProofRequirement` with the
generated proof registry + row-test wrapper (PART 2 §§10.2–10.3); the two-phase
lifecycle with the gate receipt (PART 2 §11); the no-skip guarantee (PART 2 §12);
and the lease-based parallel-safe resume protocol (PART 2 §14). U0 builds that
machinery; the U3 / U8 / U10 / U11 / U12 / U13 blocks lift their exact manifest rows
through it.
