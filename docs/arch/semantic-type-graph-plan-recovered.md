> **SUPERSEDED.** This document is historical. The live authority is [`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md) (and the native-typeinfo-parity doc-set). Sections below are retained for provenance; where they contradict the unified plan, the unified plan wins.

> **Status (2026-06-02):** Remaining work from this plan is now tracked in [`semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md), which merges + sequences the remaining items of this plan with the other track. Drive new work from the unified plan; this file remains as historical/detail reference.

> **SUPERSEDED — typeinfo parity ledger (2026-06-03):** This document is historical / foundation reference for the typeinfo semantic-graph design. The authoritative typeinfo parity ledger is the **363-row `block_id` partition in [`native-typeinfo-parity.md`](./native-typeinfo-parity.md) §10.4.1** (the row-exact capability→mechanism→proof coverage table), backed by the two-table ledger at `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` (363 `IgnoredTestRow`s + the separate `AdditionalProofRow` coverage table; `IgnoreStatus { Ignored, Lifted { block_id } }`; binding total exactly 363). The stale unignore-manifest wording in this file (the `typeinfo_tests_unignore_plan.md` doc-manifest with `(file, fn_name, target_phase, unblocked_by)` columns and the "384 tests" lift schedule) describes a SUPERSEDED schema — the live ledger is the in-repo `.rs` manifest, never a competing `.md` doc, and the binding count is the parser-derived 363, not 384 (384 was the raw `#[ignore]` line count). Read §3.0/§8 here for design intent only; take the ledger schema, the count, and the per-block partition from `native-typeinfo-parity.md` §10 / §10.4.1.

# Verter TypeInfo Semantic Graph Plan — Revision 17

## 0. Identity

This is the final-state plan for `@verter/typeinfo` as Verter's TypeScript-replacement-grade semantic graph foundation. It defines the only typeinfo pipeline that exists; there is no parallel implementation surviving alongside it. The plan reads as final state: rule text and final-state assertions do not reference phase numbers, "the cutover", "the new design", or any history-rooted vocabulary. Structural section headings in §8 carry phase numbers solely so an implementer agent can sequence the blocks; the landed source code carries none of them.

## 0.1 Why `typeinfo` Exists

Verter's long-term direction is to **be TypeScript for everyone** — LSP, MCP, type-aware tooling, framework adapters, runtime schema generation. Not "a Vue adapter that happens to have type extraction"; not "a metadata exporter sitting on top of TS". A genuine compiler-grade semantic graph with first-class representations of the constructs TypeScript actually has.

`@verter/typeinfo` is the public foundation for that. It is consumed by:

- Vue's component-meta adapter.
- Future Svelte and React adapters (STOP-gated; see §9).
- LSP / MCP / TSGO-integrated diagnostics.
- General-purpose TypeScript consumers (Zod, JSON Schema, docs, type explorer, IDE refactoring tools, codemods).

The graph design answers four questions:

- **What is this type, structurally**, with full TypeScript semantic fidelity — declaration merging, module augmentation, ambient namespaces, `this`-types, variance, overload sets, type predicates, assertion functions, narrowing, contextual typing, weak types, exact-optional semantics, every other meaning-affecting TS construct.
- **Where did it come from**, via a normative origin / derivation layer.
- **Is X assignable to Y, and why** — a relation engine exposed as a public query, not re-implemented by every consumer.
- **What can be safely projected from it** — to Zod, JSON Schema, controls, documentation, display text — with exactness and degradation made typed.

If the graph cannot answer these, no projection can. Heuristic recovery — name suffixes, regex over display text, source slicing, format-then-reparse — is forbidden across the full pipeline (the Typed-IR-Only Resolver Rule extends in full to typeinfo projections).

## 0.2 Document Layout

- §2 — non-negotiable architecture rules. Every rule has a named guard or discriminating regression test in the same phase that introduces it; each is registered in `CRITICAL_RULE_GUARDS` (see §8.0).
- §3 — semantic graph model. Includes the full proto rewrite (§3.0), InternedName carrier spec (§3.7), QueryErrorDto lossless lowering (§3.8), cycle-id stability rule (§3.9), variance/const/NoInfer producer chain (§3.10), FlowNarrowing producer chain (§3.11).
- §4 — cache topology. Includes substitutions in the slot key (§4.1.1), reused-cache R21 audit table (§4.2), concurrent cold-collapse design (§4.3), multi-candidate dimension audit (§4.4).
- §5 — public API surface. Mandatory `ProjectionReductionContext` and `DisplayPolicy`; typed `TypeInfoRequestError`; structured `EvaluateTypeExpressionGraphRequest`.
- §6 — framework surface adapter contract. Closed enums; no callback-prop heuristic.
- §7 — typeinfo projections.
- §8 — phase plan with the full §0.5 existing-state survey, §0.6 native-API name alignment, §0.7 documentation-update obligations, §8.0 CRITICAL_RULE_GUARDS registry plan, §8.x ignored-test lift schedule.
- §9 — risks, STOP-gate concrete checklists, cross-file MergedDeclaration completion, wire-compat risk.
- §10 — audit infrastructure integration, full enumeration (KindBit, accessor, batch counter, structured event, footprint miner, TS bindings, TLS propagation).
- §11 — RFC for `parse_type_annotation` second exception (resolved: no second exception is taken; see C2 / §5.6).
- §14 — final invariants table.
- PART A / PART B divider — structural marker for the doc split at landing time.
- §0.5 — existing-state survey (every codebase artifact touched by the plan, with disposition).
- §0.6 — native-API name alignment (every plan-side reference mapped to its real codebase symbol).
- §0.7 — design principle (one-paragraph statement of the foundational contract).
- Rounds 3-15 Commitments Compendium (A.1-A.24) — cross-section consolidated commitments from orchestration rounds 3-15, organized by topic.

---

## 1. Crate Ownership

- **`verter_audit`** owns: `RequestAuditRecord`, `RequestKind`, `RequestKindPayload`, `StructuredAuditEvent`, `AuditObserver`. Every new audit variant in this plan lands here. The existing `verter_audit::origin_graph::OriginEdgeKind` (which includes `SharedLoadReuse`) is the wire/audit mirror of the semantic taxonomy; see §2.15 for the reconciliation rule.
- **`verter_semantic`** owns: typed IR (`TypeExpr`, analyses, lowering, the type solver arena).
- **`verter_session`** owns: type-resolution orchestration, semantic query memoisation (`SemanticGraphStore`, `ProjectSemanticDispatch::execute`), `ProjectTypeStore`, all caches in the project-global cache architecture, the new `CompletionFence` publication adapter (added in Phase 0b/1 at `crates/verter_session/src/typeinfo/completion_fence.rs`; see §0.6), `HostAuditRuntime`, `ResolvedDeclSlotIdentity` / `VersionedDeclIdentity`, the fallthrough / root inheritance resolver, the relation engine memo, every new `SemanticQueryKey` variant introduced for declaration merging / module augmentation / ambient namespaces / overload sets / enums / flow narrowing / contextual typing.
- **`verter_protocol`** owns: transport-facing DTOs that cross FFI / WASM boundaries. The wire schemas for the typeinfo graph payload, the framework surface payload, and audit-record extensions are all defined here as `prost`-compatible Rust structs derived from `proto/verter/v1/*.proto`. The corresponding TypeScript surface is generated from the same `.proto` files through the existing protobuf path under `packages/proto/src/gen/verter/v1/` (which already produces `component_meta_pb.ts` and `selective_component_meta_pb.ts`); typeinfo graph wire DTOs add `typeinfo_pb.ts` to the same generator. `ts-rs` is NOT used on wire-side typeinfo DTOs — that derive is reserved for the audit envelope under `crates/verter_audit` (see §0.6).
- **`verter_ffi`** owns: the thin adapter layer between the native runtime and NAPI / WASM.
- **`@verter/typeinfo`** owns: public TypeScript graph DTOs (generated via `ts_rs` from `verter_protocol`), decode helpers, generic projection packages (Zod, JSON Schema, Storybook controls, docs, display text), the `legacy TypeDescriptor` projection.
- **`@verter/component-meta`** owns: the Vue framework-surface adapter — Vue macro extraction, surface discovery (props, events, slots, models, exposed, fallthrough), Vue-specific projections. It is a consumer of `@verter/typeinfo`, never a parallel semantic implementation.

The dependency direction is one-way and machine-checked by the `dependency_direction_one_way` static guard:

```
verter_audit ───────────────────────────────► verter_span
verter_semantic ────────────────────────────► verter_span
verter_protocol ────────────────────────────► verter_semantic, verter_audit
verter_session ─────────────────────────────► verter_semantic, verter_audit, verter_protocol, verter_workspace
verter_ffi ─────────────────────────────────► verter_session, verter_protocol
@verter/typeinfo ───────────────────────────► verter_protocol (DTOs via ts_rs)
@verter/component-meta ─────────────────────► @verter/typeinfo
```

`@verter/typeinfo` MUST NOT import `@verter/component-meta`, any framework adapter, or any framework parser. `@verter/component-meta` and any future framework adapter MUST NOT define semantic graph node kinds, edge kinds, projection policies, or relation logic.

---

## 2. Non-Negotiable Architecture (CRITICAL)

Every rule below is registered in `CRITICAL_RULE_GUARDS` (§8.0) with at least one named guard. Each guard is **discriminating**: it must FAIL on the pre-tree (before its rule lands) and PASS on the post-tree (after its rule lands). Empty test bodies, `assert!(true)`, characterization tests that do not discriminate, and "real body deferred" patterns are forbidden per CLAUDE.md Stub Prevention.

### 2.1 The Five Query Modes Are Explicit

Every reusable type-resolution call carries an explicit `ProjectionReductionContext { mode, demand }`. The `mode` is one of `Identity | Navigate | Shallow | Expanded | Skeleton`; the `demand` is `Published | StructuralTransit`. There is no implicit mode and no default mode. Omitting the mode from a public typeinfo request is `TypeInfoRequestError::MissingProjectionContext`, returned to the caller as a typed error before any semantic execution.

Mode cascades along a path: intermediate hops in `ProjectPath` run `Navigate`; only the terminal hop runs the caller's requested mode. Sibling members and unrelated branches are never materialised by a path query. Guard: `path_projection_mode_cascade`.

`Skeleton` is a separate policy slot. It does not alias `Navigate` or any other mode unless a typed equivalence proof and a regression test justify a backfill edge.

### 2.2 Backfill Rule

For the same mode-erased semantic shape (operation + base identity + path / projection + substitutions + scope + version root + env hashes), broader successful results may backfill narrower modes:

- `Expanded` may satisfy `Shallow`, `Navigate`, `Identity`.
- `Shallow` may satisfy `Navigate`, `Identity`.
- `Navigate` may satisfy `Identity`.
- `Skeleton` is isolated.

Narrower successful results MUST NOT claim broader work is cached. A whole-surface projection may backfill per-member or per-indexed-access caches for the members it actually materialised; a narrow member result must not claim sibling-member or whole-surface caches.

### 2.3 Five-Way Env Hash Split (R21)

Cache identity uses the five orthogonal dimensions from R21:

| Dimension | Captures |
|---|---|
| `parse_env_hash` | parser / SFC / compiler feature flags |
| `resolve_env_hash` | `base_url`, `paths`, workspace aliases, module resolution mode, package `exports`/`imports`, default extension order |
| `type_env_hash` | TS semantic options (`strict`, `noImplicitAny`, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`, etc.) |
| `lib_env_hash` | TS built-in lib selection, `types`, `typeRoots`, registered ambient libs, global / module-augmentation corpus identity |
| `project_identity` | project root, tsconfig path, provider root, workspace root, membership / owner selection |

A single bundled `project_config_hash` is forbidden. Every meaning-affecting cache key composition in this plan lists which of these five dimensions it includes; the `r21_no_bundled_config_hash` guard rejects any new cache key that bundles them.

### 2.4 Two Cache Families

Every cache in this plan is exactly one of:

- **Content-addressed artifact caches** — keyed on `content_hash` (or `parse_stable_hash`). Cosmetic-edit invariance lives in the key for caches that lower from `IndexedReady` semantic shape.
- **Query-identity caches** — keyed on content-free slot identity. Version rooting (`VersionedDeclIdentity` + `ReadSetSignature.facts` + `self_root_canonicals`) lives in the cached value. Concurrent variants coexist as bounded multi-candidate slots (R20).

Cache keys NEVER include `fact_dep_signature` or content / version hashes for query-identity caches. The `cache_key_no_value_fields` guard enforces this.

### 2.5 Publication Fence (CompletionFence)

Every top-level live-host result — `resolveSymbolGraph`, `evaluateTypeExpressionGraph`, `projectPathGraph`, `relate`, `getFrameworkSurfaces` — publishes through the new `CompletionFence` adapter (added in Phase 0b/1 at `crates/verter_session/src/typeinfo/completion_fence.rs`; see §0.6). The adapter wraps the existing `InflightTable` substrate at `crates/verter_session/src/semantic_query_memo/inflight.rs` (the `MAX_INFLIGHT_RETRIES: usize = 3` constant at line 226 is the canonical retry budget; the adapter does NOT introduce a second retry constant). Its public shape:

```rust
// crates/verter_session/src/typeinfo/completion_fence.rs (NEW in Phase 0b/1)
pub struct CompletionFence {
    inflight: Arc<InflightTable<TypeInfoGraphSlotKey>>,
    publish_lock: parking_lot::RwLock<()>,
}

pub struct PublishAttempt<V> {
    pub value: V,
    pub read_set: ReadSetSignature,
    pub self_root_canonicals: Arc<[(Arc<str>, FileWholeHash)]>,
}

impl CompletionFence {
    /// Build → revalidate → publish, retrying at most `MAX_INFLIGHT_RETRIES` (3)
    /// times when `revalidate` observes a torn read-set. On exhaustion returns
    /// `TypeInfoRequestError::UnstableState { attempts: 3 }`.
    pub fn publish_with_retry<V, Build, Reval>(
        &self,
        build: Build,
        revalidate: Reval,
    ) -> Result<Arc<V>, TypeInfoRequestError>
    where
        Build: FnMut() -> Result<PublishAttempt<V>, TypeInfoRequestError>,
        Reval: FnMut(&V, &ReadSetSignature) -> bool;
}
```

Behavior:

1. Collect every touched dependency fact through `ResolverContext::with_fact_tracer(|| { ... })`, finalised via `FactReadSet::finalise() -> FactReadSetFinalise` and lifted into a `crate::cache_runtime::SignatureAdmission` via `SignatureAdmission::from_finalise(finalise)`; the cache-admission contract distinguishes `Ok(facts)` (lift to `ReadSetSignature::new(facts)`) from `Overflow` (route through the non-cacheable arm).
2. Revalidate the recorded `ReadSetSignature.facts` against the live `StoreView` immediately before publish (the `Reval` callback).
3. On conflict, retry at most `MAX_INFLIGHT_RETRIES = 3` times.
4. On exhaustion, return `TypeInfoRequestError::UnstableState { attempts: 3 }` (the caller surfaces this through the `Opaque(QueryErrorDto::UnstableState { attempts: 3 })` graph node). Never warm a torn provisional result.

Cancelled, superseded, interrupted, budget-exceeded, partial, unstable, miss results may be returned to the caller, but are admitted only into the typed `DegradedResultStore`, never into the warm query-identity cache. Guards: `publication_fence_revalidates_before_publish`, `typeinfo_graph_publication_fence_3_retries`, `completion_fence_uses_max_inflight_retries_constant` (asserts the adapter does NOT define its own retry constant — must consume `semantic_query_memo::inflight::MAX_INFLIGHT_RETRIES`).

### 2.6 Warm-Cache Exactness Contract

`TypeInfoGraphResultDb` admits ONLY payloads whose every node has `ExpansionStatus::ExactResolved` or `ExpansionStatus::ExactSymbolic` AT THE REQUESTED COMPLETENESS BOUNDARY (within the closure the caller asked for). Any payload containing a node with status `BudgetExceeded`, `Partial`, `Unsupported`, `Cycle`, `UnstableState`, `Miss`, or `UnresolvedGeneric` returns through `ComputeAdmission::ReturnOnly`:

- it is returned to the caller for surfacing via `diagnostics`,
- it is admitted (if at all) to the typed `DegradedResultStore` under a different slot,
- it is NEVER stored as a warm `TypeInfoGraphResultDb` candidate.

Per-node degradation does not "partially admit" the surrounding payload — the rule is whole-payload exact-only. Guards: `degraded_payload_never_warm_admitted`, `typeinfo_graph_no_partial_admission`, `degraded_store_never_serves_complete_admission`.

`Cycle { cycle_id, entry }` is an exception to this rule WHEN AND ONLY WHEN the cycle is a structural recursive type representation (e.g. `type Tree = { children: Tree[] }`). In that case the `Cycle` node is `ExactResolved` for the recursive back-edge slot itself; the surrounding payload is admitted. See §3.9 for the cycle-id stability rule that makes this safe.

### 2.7 `Arc`-Published Immutable Payloads

Final payload caches hand out `Arc<TypeInfoGraphPayload>` values. The JS bridge serialises from the `Arc` snapshot; it never mutates or re-resolves.

### 2.8 Bounded Retention

Every new query-identity cache in this plan is bounded by the existing `verter_session::bounded_query_retention` substrate:

- `GlobalRetentionBudget<K>` — FIFO insertion-ordered total-size cap.
- `BoundedCandidateMap<K, D, V>` — per-slot candidate list (cap = 4, FIFO eviction) plus global budget (default 512 for owner-keyed caches, 2048 for structural caches, 4096 for `SemanticNodeId`-keyed caches).

`TypeInfoGraphResultDb` uses `BoundedCandidateMap` with per-slot cap 4 and global budget 512 (same as `ComponentMetaResultDb`). The relation cache (already on `SemanticGraphStore`) keeps its existing 4096 global budget.

### 2.9 Fact-Signature Validation

Every reusable cache write records a `ReadSetSignature.facts` from the path-precise fact tracer. Every reusable cache warm read revalidates the signature against the live `StoreView` before returning. Warm-hit validation stays under 50µs p99 with zero allocation per hit (R24). Guard: `typeinfo_graph_warm_hit_zero_alloc`.

### 2.10 IndexedReady Authority

Every reusable derivation in this plan reads from `IndexedReady` (the canonical post-parse artifact) or downstream caches. No code path may rescan raw source bytes to rediscover symbols, nor reparse display strings. Guard: `graph_export_reads_only_from_indexed_ready`.

### 2.11 Typed-IR-Only Resolver Rule (extended to typeinfo)

Every typeinfo projection (Zod, JSON Schema, Storybook controls, docs, display, legacy TypeDescriptor) reads semantic decisions exclusively from the typed graph nodes. No projection may parse `rawType`, display strings, identifier suffixes, path substrings, or any text-bearing semantic recovery hook. The `typeinfo_projection_no_raw_or_display_string_semantics` guard scans every projection package for forbidden substrings (`rawType`, `display`, `name.ends_with`, `path.contains("/node_modules/")`, `starts_with("Pick<")`, `looksLike`, `extract*`, `format!.*parse_type_annotation`).

`parse_type_annotation` is reserved for JSDoc tag-type payloads. The typeinfo resolver / projector / registry / policy / materialiser / projection / compat pipeline NEVER calls `parse_type_annotation` and never synthesises-then-reparses (`format!(...).parse_type_annotation(...)`). The `EvaluateTypeExpressionGraphRequest` accepts only the structured `StructuredTypeExpression` DTO (see §3.5 + §5.6); raw user expression text is not part of the public API. Guard: `evaluate_type_expression_does_not_call_parse_type_annotation`.

### 2.12 No Role Inference From Name Suffix

A type's role is determined by which framework macro consumed it (Vue: `defineProps` / `defineEmits` / `defineSlots` / `defineModel` / `withDefaults` / `defineExpose`; Svelte / React: equivalent surface discovery), not by identifier suffix. `name.ends_with("Props") | "Emits" | "Events" | "Model" | "Slots"` is banned across the resolver / projector / registry / policy / materialiser / compat pipeline AND across typeinfo projections. Guard: `no_role_inference_from_suffix`.

### 2.13 SymbolSpace Has Three Variants

`SymbolSpace = Type | Value | Namespace`. `BothTypeValue` is forbidden. A `class Foo` declaration emits two facts (`Export("Foo", Type)` and `Export("Foo", Value)`); a `namespace Foo` declaration that also has a value side emits three. Guards: `symbol_node_preserves_type_value_namespace_spaces`, `class_dual_space_emits_two_symbols`.

### 2.14 Relation Engine Is Public

`relate(source, target) -> RelationResult` is a public typeinfo query. Every consumer that needs "is X assignable to Y?" calls `relate`; no projection re-implements assignability, narrowing, or subtyping locally. The relation engine lives on `SemanticGraphStore` (the full-identity `Relate` `SemanticQueryKey` variant — source / target / relation kind / policy / source freshness / inference context / env+substitution+projection-reduction context, memoised under the matching `RelateMemoKey`), is memoised with `ReadSetSignature.facts` fencing, and is exposed through the public typeinfo session API. Guard: `typeinfo_exposes_relate_query`.

### 2.15 Origin-Edge Taxonomy Is Normative

Two `OriginEdgeKind` enums exist in the codebase, with a deliberate one-way relationship:

- `verter_session::semantic_query::OriginEdgeKind` — the canonical semantic taxonomy. Nine kinds: `Instantiate | SubstituteTypeParam | ConditionalSelect | InferBind | ProjectMember | ProjectIndex | ProjectPath | Normalize | AliasResolve`.
- `verter_audit::origin_graph::OriginEdgeKind` — the wire/audit mirror. Same nine kinds plus `SharedLoadReuse` (an audit-only edge emitted when a joiner attaches to a winner's in-flight artifact via scheduler dedup).

The public `verter_protocol::typeinfo::graph::OriginEdgeKind` DTO mirrors the `verter_audit` variant (ten kinds including `SharedLoadReuse`). New edge kinds require an RFC + the `origin_edge_taxonomy_locked` guard to be updated. Guard body asserts the three enums are pairwise consistent (the audit/protocol enums are supersets of the semantic enum by exactly `SharedLoadReuse`).

### 2.16 Symbol Identity Is Slot-Based

Public symbol identity uses `ResolvedDeclSlotIdentity` (content-free) plus `VersionedDeclIdentity` (carried on cached values, never in keys). `(canonical_id, name, symbol_space)` alone is insufficient — merged declarations across files share one `merged_symbol_name`. Guard: `symbol_node_preserves_resolved_decl_slot_identity`.

### 2.17 Declaration Merging, Module Augmentation, Ambient State Are First-Class Graph State

Declaration merging (interfaces, namespaces, classes + namespaces, enums + namespaces), function overload sets, module augmentation, ambient namespaces, `declare global`, UMD globals, `export as namespace` are first-class graph nodes with contributor provenance. The graph EXPLICITLY models them (see §3).

Semantic resolution for these constructs lives in `verter_session::semantic_query::ProjectSemanticDispatch` via the following new `SemanticQueryKey` variants. Env hashes (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`, `resolver_version`) flow through `ResolverContext` at the dispatch boundary — they are embedded in the `SemanticGraphStore::execute_cooperative` key composition uniformly, the same way `Instantiate { base, args, context }` carries its context today. No variant carries env fields directly on its struct (the env flows in alongside the discriminant via `context`, mirroring existing convention):

- `SemanticQueryKey::ResolveMergedDeclaration { canonical, name, symbol_space }` → `SemanticNodeData::MergedDeclaration`.
- `SemanticQueryKey::ResolveModuleAugmentation { target: AugmentationTargetKey }` → `SemanticNodeData::ModuleAugmentation`.
  - `AugmentationTargetKey` itself already embeds `{ project_identity, resolve_env_hash, lib_env_hash, target }` per the existing `FileArtifactStore::augmentation_index` schema, so this variant is consistent without restating.
- `SemanticQueryKey::ResolveAmbientNamespace { canonical, name }` → `SemanticNodeData::AmbientNamespace`.
- `SemanticQueryKey::ResolveOverloadSet { decl: ResolvedDeclSlotIdentity }` → ordered `Arc<[SignatureRef]>`.
- `SemanticQueryKey::ResolveEnum { decl: ResolvedDeclSlotIdentity }` → first-class enum representation.
- `SemanticQueryKey::FlowNarrowingAt { canonical, span }` → `TypeNode::FlowNarrowing` payload (§3.11).
- `SemanticQueryKey::ContextualTypeAt { canonical, span }` → `TypeNode::ContextualType` payload (§3.11).

Env composition is uniform across all seven variants — the cache key produced by `SemanticGraphStore::execute_cooperative` for variant `V(payload)` is `(variant_tag, payload, resolve_env_hash, type_env_hash, lib_env_hash, project_identity, resolver_version)` for query-identity layers (`parse_env_hash` flows through `parse_stable_hash` on cross-file dependencies, never enters the key directly). Guard: `new_semantic_query_keys_uniform_env_composition` (Phase 0a) asserts none of the seven new variants names an env-hash field on its struct (consistency check).

The graph exporter (§8 Phase 2) is a PURE LOWERING pass. It does NOT recover declaration-merge / augmentation / namespace structure from `IndexedReady` itself — it calls these `SemanticQueryKey` variants and lowers their results into `TypeNode::MergedDeclaration` / `TypeNode::ModuleAugmentation` / `TypeNode::AmbientNamespace` / `TypeNode::Class` (with overloads) / `TypeNode::Enum` DTOs. Cross-file declaration merging (e.g. `interface Foo` in file A + `interface Foo` in file B → one `MergedDeclaration` with `parts.len() == 2`) lands in the same phase as the lowering, not deferred. Guards: `merged_declarations_are_public_graph_state`, `overload_sets_are_public_graph_state`, `module_augmentation_is_public_graph_state`, `ambient_namespaces_are_public_graph_state`, `exporter_dispatches_resolve_merged_declaration_query`.

### 2.18 Framework Adapter Boundary

Framework adapters select surface roots and attach `TypeNodeId`s. They MUST NOT:

- Introduce new node kinds, edge kinds, projection policies, or relation logic.
- Define semantic graph variants in their own DTOs.
- Re-resolve type meaning through display text or `rawType`.
- Compute fallthrough / root inheritance themselves — that resolver lives in `verter_session`.
- Override TypeScript semantics (optionality, readonly, call/construct signatures, index signatures, exactness) inside `FrameworkSurfaceMember`. Those originate from graph nodes only.
- Reclassify callback props as events based on naming conventions (`on*`, `on{Name}`) or any other heuristic. See §6.3.3.

The `FrameworkSurfaceMember.kind` field stays CLOSED via `FrameworkSurfaceKind` (closed enum: `Prop, Event, Slot, Model, Exposed, Ref, Children, Snippet, Export, AcceptedProp, AcceptedEvent`). Framework identity itself is OPEN through `FrameworkAdapterId` (interned canonical string id, e.g., `"vue"`, `"svelte"`, `"react"`, `"solid"`) plus a session-time `FrameworkAdapterRegistry` (§6.4). Adding a new SURFACE KIND still requires a schema bump; adding a new FRAMEWORK only requires registry registration. Guards: `framework_surface_member_enum_is_closed`, `framework_surface_request_accepts_open_adapter_id`, `framework_adapter_id_canonicalization_rejects_case_alias`, `framework_adapter_registry_rejects_unknown_surface_kind_discriminant`, `framework_surface_member_does_not_override_optionality`, `framework_adapter_does_not_recompute_fallthrough`, `framework_adapter_does_not_reclassify_callback_props_as_events`.

### 2.19 Progressive Expansion Is A Semantic Query

A client asking "give me more graph around node N" dispatches a semantic query (`SemanticQueryKey::ProjectPath` or `SemanticQueryKey::Instantiate` with the broader mode), not a JS-side follow-up resolver call. The native side returns a fresh `TypeInfoGraphPayload` whose `GraphQueryIdentity` carries the request's full identity (the original root, the requested expansion target's `ResolvedDeclSlotIdentity`, mode, closure, env hashes). Snapshot node IDs are stable within one payload; cross-payload matching uses `SymbolId` + `ResolvedDeclSlotIdentity`. Guards: `progressive_graph_expansion_dispatches_semantic_query_key`, `progressive_expansion_routes_through_typeinfo_graph_db`.

### 2.20 No Heuristic Cache Semantics (R30 / R31)

Every dimension that can change the returned type, published members, exactness, or completeness must appear as one of:

- a typed cache-slot key dimension;
- a per-mode/per-policy entry inside a shared cache family;
- cached-value validation metadata (fact signature, self-root set, store-view constraint);
- explicit result state (`Exact`, `SurfaceOnly`, `Unsupported`, `BudgetExceeded`, `Cycle`, `Unstable`).

No numeric caps, no "better shape" scoring, no rendered-text heuristics. Guard: `cache_identity_has_no_heuristic_dimensions`.

### 2.21 Closed-Enum Discipline (Wire-Compat Policy: Option A — Version-Bumped Closed Enums)

All public typeinfo enums (`PrimitiveKind`, `MemberNameKind`, `IndexKeyKind`, `MappedModifier`, `Variance`, `BranchSelection`, `ConditionalResolution` variants, `OriginEdgeKind`, `SymbolKind`, `SymbolSpace`, `FrameworkTag`, `FrameworkSurfaceKind`, `GraphOperation`, `GraphClosurePolicy`, `ExpansionStatus`, `MissPrecondition`, `UnsupportedConstruct`, `BudgetKind`, `UnstablePrecondition`, `CyclePrecondition`, `RelationUnknownReason`) are closed: no `Custom(...)` escape hatch, no `UnknownVariant { tag, raw }` carrier on the wire.

Wire-version skew is handled by an explicit two-stage protocol:

1. **Handshake at session open.** When a TS-side `TypeInfoSession` first establishes its NAPI/WASM/in-process connection to the host, the host responds with `TypeInfoSessionHandshake { server_schema_version: u32, supported_versions: Vec<u32> }`. The client compares its compiled-in `client_schema_version` against `supported_versions`:
   - Overlap exists → session uses the highest mutual version; the client sets `negotiated_schema_version`.
   - No overlap → session fails to open with `TypeInfoRequestError::UnknownSchemaVersion { client_version, server_versions }`. The client SDK surfaces an upgrade-or-fallback hint to the caller. The session does NOT half-open with stub bodies.

2. **Per-request echo.** Every request DTO and every response payload carries the negotiated `schema_version` in its `GraphQueryIdentity`. A decoder receiving a payload whose `schema_version` differs from the handshake-negotiated version returns `TypeInfoRequestError::MalformedPayload { detail: "schema_version mismatch" }` — this guards against host-side regressions where a binary upgrade silently bumps the version mid-session.

Adding a new variant bumps `schema_version` and is announced to clients via `supported_versions`. Old clients fail closed at handshake; new clients downgrade by selecting the highest mutual version. SDK consumers receive `UnknownSchemaVersion` as a typed error they can switch on — they are not forced into a panic path.

```protobuf
message TypeInfoSessionHandshake {
  uint32 server_schema_version = 1;
  repeated uint32 supported_versions = 2;
  string server_version_string = 3;                    // for logging only
}
```

Guards: `framework_tag_enum_is_closed`, `framework_surface_member_enum_is_closed`, `no_custom_string_escape_in_typeinfo_dtos`, `decoder_returns_typed_error_on_unknown_variant`, `typeinfo_session_handshake_emits_supported_versions` (Phase 0a — asserts the handshake response carries non-empty `supported_versions`), `typeinfo_session_rejects_unknown_schema_version_at_handshake` (Phase 0b/1 — asserts a synthetic mismatch surfaces `UnknownSchemaVersion`).

### 2.22 Closed-Enum Fallback Reasons

Every `Opaque` / degraded result carries a closed enum naming the precondition that failed, not a free-form `InternedName`:

- `MissPrecondition { NoSymbolMatch | NoIndexedReady | AugmentationNotFound | OverloadResolutionFailed | ... }` — closed enum, full variant list locked in Phase 0a §3.8.
- `UnsupportedConstruct { Decorator | UmdGlobal | LegacyTypeguard | ... }` — closed enum, full variant list locked in Phase 0a §3.8.
- `BudgetKind { Nodes | Depth | RelationSteps | CycleEntries }` — closed enum (4 variants).
- `UnstablePrecondition { ReadSetMutation | StoreViewSupersession | InflightCancellation }`
- `CyclePrecondition { AliasCycleEntry | RecursiveBackEdge | RelationCycleEntry }`
- `RelationUnknownReason { UnboundTypeParameter | UnresolvedConditional | CycleEntry }`

**Exhaustive `BudgetDomain → BudgetKind` mapping** (from the real `BudgetDomain` at `crates/verter_session/src/resolver_core/shallow_file_state.rs:238`):

| Native `BudgetDomain` (6 variants) | Wire `BudgetKind` |
|---|---|
| `BudgetDomain::LocalClosure` | `BudgetKind::Nodes` |
| `BudgetDomain::Frontier` | `BudgetKind::RelationSteps` |
| `BudgetDomain::BuilderExpansion` | `BudgetKind::Depth` |
| `BudgetDomain::SolverResolveSteps` | `BudgetKind::RelationSteps` |
| `BudgetDomain::SolverArenaNodes` | `BudgetKind::Nodes` |
| `BudgetDomain::SolverInstantiationDepth` | `BudgetKind::Depth` |

The mapping function is `pub fn budget_kind_from_domain(d: BudgetDomain) -> BudgetKind` — implemented as an exhaustive `match` so adding a new `BudgetDomain` variant compile-breaks the lowering. Guard: `budget_exceeded_failure_maps_all_domains` asserts the mapping covers every `BudgetDomain` variant via compile-time exhaustiveness.

Each variant has at least one audit counter (per §10) and at least one discriminating regression test. No producer can emit `Opaque(Miss)` or `ExpansionStatus::Miss` without naming one of the enum values. Guards: `degraded_results_use_closed_enum_reasons`, `budget_exceeded_failure_maps_all_domains`.

### 2.23 SDK Audit Test For Intrinsics

Intrinsic dispatch routes through `IntrinsicRegistry::lookup` — the SDK audit test asserts every `= intrinsic` declaration in `lib*.d.ts` has a registry entry. Unimplemented intrinsics emit `TypeNode::Opaque(QueryErrorDto::UnsupportedIntrinsic { name })` with the `name` interned. Guard: existing `sdk_audit_unsupported_intrinsic` (extended).

---

## 3. Semantic Graph Model

### 3.0 Proto Schema (Full Rewrite)

> SUPERSEDED: the A0a-landed wire uses the `Graph*`-prefixed names (`GraphTypeNode` / `GraphMergedDeclaration` / …), a single `TypeInfoGraphRequest` envelope (7-arm oneof), an 11-variant `TypeInfoRequestError`, and `PredicateSubjectName` (the `PredicateSubjectIdentifier` rename is landed). The bare-`TypeNode` / per-request-message naming and the 8-variant error below are superseded (unified plan §2.2). The `GraphFlowNarrowing` / `GraphContextualType` arms move OUT of `GraphTypeNode` into a sibling `ProgramAnalysisGraph` at U8 (unified plan §2.2 / §3 cross-export-session U8).

The current `crates/verter_protocol/proto/verter/v1/typeinfo.proto` (94 lines) is replaced in full as part of Phase 0a (no field renumbering — proto3 cannot renumber once shipped, so the wire-compat table below documents which field numbers carry over and which are introduced fresh). The plan rewrite normatively specifies:

**Envelope:**

```protobuf
syntax = "proto3";
package verter.v1;

// SemanticTypeGraph — top-level wire envelope for every typeinfo
// graph query response. Producers MUST set schema_version.
message SemanticTypeGraph {
  uint32 schema_version = 1;                            // bumped on incompatible add
  GraphQueryIdentity query = 2;
  repeated TypeNode nodes = 3;                          // index = TypeNodeId
  repeated SymbolNode symbols = 4;                      // index = SymbolId
  repeated Signature signatures = 5;                    // index = SignatureRef
  repeated OriginEdge edges = 6;
  repeated uint32 root_ids = 7;                         // TypeNodeId list
  repeated NodeStatus exactness = 8;                    // (TypeNodeId, ExpansionStatus) pairs
  repeated TypeInfoDiagnostic diagnostics = 9;
  repeated NodeIdMapEntry node_id_map = 10;
  repeated SymbolIdMapEntry symbol_id_map = 11;
  StringTable strings = 12;                             // interned name carrier (§3.7)
}
```

**Wire-compat table — current → new:**

| Current message (94-line schema) | New disposition | Notes |
|---|---|---|
| `EvaluateTypeExpressionRequestDto` | DELETE wholesale | replaced by `EvaluateTypeExpressionGraphRequest` carrying `StructuredTypeExpression` (§5.6). The 5 field numbers (1-5) are reserved and never reused. |
| `ImportSpecDto`, `NamedImportDto`, `DefaultImportDto`, `NamedBindingDto`, `NamespaceImportDto` | PRESERVE | re-homed into the new schema; consumers continue to encode named imports for `StructuredTypeExpression::ExtraImports`. |
| `SymbolEntryDto`, `SymbolEntryListDto` | PRESERVE | `listSymbols` continues to return these; the SymbolNode of the new graph is distinct. |
| `string mode = 4` on `EvaluateTypeExpressionRequestDto` | REPLACE | Mode is now a closed enum `ProjectionMode` with explicit tag numbers. No more stringly-tagged mode. |

**Reserved field numbers (must never be reused):**

```protobuf
message EvaluateTypeExpressionRequestDto {
  reserved 1, 2, 3, 4, 5;                              // deleted in this schema version
  reserved "scope_canonical", "expression", "extra_imports", "mode", "cacheable";
}
```

**Oneof discriminant strategy for TypeNode:**

```protobuf
message TypeNode {
  oneof kind {
    PrimitiveNode primitive = 1;
    LiteralNode literal = 2;
    UniqueSymbolNode unique_symbol = 3;
    UnionNode union = 4;
    IntersectionNode intersection = 5;
    ObjectNode object = 6;
    ArrayNode array = 7;
    TupleNode tuple = 8;
    ReferenceNode reference = 9;
    AliasInstantiationNode alias_instantiation = 10;
    TypeParameterNode type_parameter = 11;
    KeyOfNode keyof = 12;
    IndexedAccessNode indexed_access = 13;
    ConditionalNode conditional = 14;
    MappedNode mapped = 15;
    TemplateLiteralNode template_literal = 16;
    TypeOfNode typeof_node = 17;                       // field name avoids Rust/TS keyword
    SatisfiesNode satisfies_node = 18;                 // field name avoids Rust/TS keyword
    ClassNode class_node = 19;                         // field name avoids Rust/TS keyword
    ThisTypeNode this_type = 20;
    MergedDeclarationNode merged_declaration = 21;
    AmbientModuleNode ambient_module = 22;
    ModuleAugmentationNode module_augmentation = 23;
    AmbientNamespaceNode ambient_namespace = 24;
    GlobalAugmentationNode global_augmentation = 25;
    FlowNarrowingNode flow_narrowing = 26;
    ContextualTypeNode contextual_type = 27;
    RelationProofNode relation_proof = 28;
    InferNode infer_node = 29;                          // field name avoids prost name collision
    EnumNode enum_node = 30;                            // field name avoids Rust/TS keyword
    OpaqueNode opaque = 31;
    CycleNode cycle = 32;
    // reserved 33 to 100 for forward additions; bumping schema_version on add.
  }
}
```

The Rust-side prost type names (`Class`, `TypeOf`, `Satisfies`, `Enum`, `Infer`) remain semantic; the field selector renames (`class_node`, `typeof_node`, `satisfies_node`, `enum_node`, `infer_node`) avoid `prost_build`'s default conversion producing Rust reserved-word identifiers (`pub typeof:`, `pub enum:`). Guard `proto_field_names_avoid_rust_keywords` (Phase 0a) statically scans the generated Rust for any `pub r#typeof` / `pub r#enum` / `pub r#class` / `pub r#satisfies` / `pub r#infer` patterns and rejects them.

The `oneof` covers every `TypeNode` variant; field numbers 33+ are reserved for additive growth. The `node_taxonomy_complete` guard asserts every Rust `TypeNode` variant, every proto `TypeNode.kind` arm, and every TS DTO discriminant are pairwise equal sets.

Unknown variant tags returned by a producer running a newer schema reach a decoder running an older schema as proto3's unknown-field machinery: the decoder surfaces `TypeInfoRequestError::UnknownSchemaVersion { wire_version: <producer>, expected_version: <decoder> }` and refuses to decode. There is NO `UnknownVariant { tag, raw }` carrier node — that escape hatch is rejected (§2.21).

**`StructuredTypeExpression` schema (replaces raw-text expressions — closed enum, all variants enumerated):**

```protobuf
message StructuredTypeExpression {
  oneof kind {
    ExprReference reference = 1;                       // type alias / interface / class by name
    ExprUnion union = 2;
    ExprIntersection intersection = 3;
    ExprIndexedAccess indexed_access = 4;
    ExprKeyOf keyof = 5;
    ExprTypeOf typeof_expr = 6;                        // field-name avoids Rust/TS keyword
    ExprTuple tuple = 7;
    ExprArray array = 8;
    ExprObject object_literal = 9;
    ExprMapped mapped = 10;
    ExprConditional conditional = 11;
    ExprLiteral literal = 12;
    ExprPrimitive primitive = 13;
    ExprTemplateLiteral template_literal = 14;
    ExprInfer infer_expr = 15;                         // REQUIRED — Promise<infer U> inside conditional extends
    ExprFunction function_expr = 16;                   // call/construct signature expressions
    ExprClass class_expr = 17;                         // class structural form expressed as type
    ExprThisType this_type = 18;
    ExprSatisfies satisfies_expr = 19;
    ExprUniqueSymbol unique_symbol = 20;
    ExprNoInfer no_infer = 21;                         // NoInfer<T> wrapper
    ExprLocalTypeRef local_type_ref = 22;              // R3: references mapped-type binder (binder_id)
    // reserved 23 to 100 for additive growth — bump schema_version on add.
  }
}

message ExprReference {
  string scope_canonical = 1;
  string name = 2;
  repeated StructuredTypeExpression type_arguments = 3;
  repeated ImportSpecDto extra_imports = 4;
}
message ExprUnion          { repeated StructuredTypeExpression members = 1; }
message ExprIntersection   { repeated StructuredTypeExpression members = 1; }
message ExprIndexedAccess  { StructuredTypeExpression object = 1; StructuredTypeExpression index = 2; }
message ExprKeyOf          { StructuredTypeExpression operand = 1; }
message ExprTypeOf         { string value_root_canonical = 1; repeated string path = 2; }
message ExprTuple {
  repeated TupleElementExpr elements = 1;
  bool readonly = 2;
}
message TupleElementExpr {
  optional string label = 1;
  StructuredTypeExpression value = 2;
  bool optional_element = 3;
  bool rest = 4;
}
message ExprArray { StructuredTypeExpression element = 1; bool readonly = 2; }
message ExprObject {
  repeated ObjectMemberExpr members = 1;
  repeated IndexSignatureExpr index_signatures = 2;
  repeated ExprFunction call_signatures = 3;             // R14: ordered (overload order is meaning-affecting)
  repeated ExprFunction construct_signatures = 4;        // R14: ordered
}
message ObjectMemberExpr {
  string name = 1;
  MemberNameKind name_kind = 2;                          // R10: closed enum, not uint32
  StructuredTypeExpression value = 3;
  bool optional_member = 4;
  bool readonly = 5;
}
message IndexSignatureExpr {
  IndexKeyKind key_kind = 1;                             // R10: closed enum, not uint32
  StructuredTypeExpression value = 2;
  bool readonly = 3;
}
// R3: mapped binder identity — name_remap/value_type reference binder_id
message MappedTypeParamExpr {
  string binder_id = 1;
  string name = 2;
  StructuredTypeExpression constraint = 3;
}

message ExprLocalTypeRef {
  string binder_id = 1;                                 // resolves to a MappedTypeParamExpr.binder_id in scope
}

message ExprMapped {
  MappedTypeParamExpr type_param = 1;
  optional StructuredTypeExpression name_remap = 2;
  StructuredTypeExpression value_type = 3;
  MappedModifier readonly_modifier = 4;                 // R10: closed enum, not uint32
  MappedModifier optional_modifier = 5;                 // R10: closed enum, not uint32
}
message ExprConditional {
  StructuredTypeExpression check = 1;
  StructuredTypeExpression extends_type = 2;
  StructuredTypeExpression true_branch = 3;
  StructuredTypeExpression false_branch = 4;
}
message ExprLiteral        { LiteralValue value = 1; }
message ExprPrimitive      { PrimitiveKind kind = 1; }  // R10: closed enum, not uint32
message ExprTemplateLiteral {
  repeated string quasis = 1;
  repeated StructuredTypeExpression expressions = 2;
}
message ExprInfer {
  string name = 1;
  optional StructuredTypeExpression constraint = 2;     // `infer X extends C` carries Some(C)
}
message ExprFunction {
  repeated TypeParameterExpr type_parameters = 1;
  optional FunctionParameterExpr this_param = 2;          // R14: `this`-param
  repeated FunctionParameterExpr parameters = 3;
  FunctionReturnExpr return_expr = 4;                     // R14: oneof type/predicate/assertion
  SignatureKind signature_kind = 5;                       // R11: Call | Construct | AbstractConstruct
}

message FunctionReturnExpr {
  oneof kind {
    StructuredTypeExpression type = 1;
    TypePredicateExpr predicate = 2;
    AssertionEffectExpr assertion = 3;
  }
}

message TypePredicateExpr {
  PredicateSubject parameter = 1;
  StructuredTypeExpression predicate_type = 2;
  bool asserts = 3;
}

message PredicateSubject {
  oneof kind {
    PredicateSubjectIdentifier identifier = 1;
    PredicateSubjectThis this_subject = 2;
  }
}

message PredicateSubjectIdentifier { uint32 name = 1; }
message PredicateSubjectThis {}

message AssertionEffectExpr {
  oneof kind {
    AssertionEffectIdentifier identifier = 1;
    AssertionEffectThis this_assert = 2;
    AssertionEffectCondition condition = 3;
  }
}

message AssertionEffectIdentifier { uint32 name = 1; optional StructuredTypeExpression predicate = 2; }
message AssertionEffectThis       { optional StructuredTypeExpression predicate = 1; }
message AssertionEffectCondition  {}

// R11: SignatureKind closed enum — Call | Construct | AbstractConstruct
enum SignatureKind {
  SIGNATURE_KIND_CALL = 0;
  SIGNATURE_KIND_CONSTRUCT = 1;
  SIGNATURE_KIND_ABSTRACT_CONSTRUCT = 2;
}

// R4/R7: helper messages referenced from ExprFunction / ExprClass
message TypeParameterExpr {
  string name = 1;
  optional StructuredTypeExpression constraint = 2;
  optional StructuredTypeExpression default_type = 3;
  Variance variance = 4;
  bool is_const = 5;
}

message FunctionParameterExpr {
  string name = 1;
  StructuredTypeExpression type_ref = 2;
  bool optional = 3;
  bool rest = 4;
  InferencePolicy inference_policy = 5;
}

// R9: closed proto enums replacing prior uint32 fields
enum Variance {
  VARIANCE_INDEPENDENT = 0;
  VARIANCE_IN = 1;
  VARIANCE_OUT = 2;
  VARIANCE_IN_OUT = 3;
}

enum InferencePolicy {
  INFERENCE_POLICY_NORMAL = 0;
  INFERENCE_POLICY_NO_INFER = 1;
}

enum MappedModifier {
  MAPPED_MODIFIER_NONE = 0;
  MAPPED_MODIFIER_ADD = 1;
  MAPPED_MODIFIER_REMOVE = 2;
}

enum Accessibility {
  ACCESSIBILITY_NONE = 0;
  ACCESSIBILITY_PUBLIC = 1;
  ACCESSIBILITY_PROTECTED = 2;
  ACCESSIBILITY_PRIVATE = 3;
}

enum OptionalSemantics {
  OPTIONAL_SEMANTICS_REQUIRED = 0;
  OPTIONAL_SEMANTICS_MISSING_ONLY = 1;
  OPTIONAL_SEMANTICS_MISSING_OR_UNDEFINED = 2;
}

enum MemberNameKind {
  MEMBER_NAME_KIND_IDENTIFIER = 0;
  MEMBER_NAME_KIND_STRING_LITERAL = 1;
  MEMBER_NAME_KIND_NUMERIC_LITERAL = 2;
  MEMBER_NAME_KIND_UNIQUE_SYMBOL_REF = 3;
}

enum IndexKeyKind {
  INDEX_KEY_KIND_STRING = 0;
  INDEX_KEY_KIND_NUMBER = 1;
  INDEX_KEY_KIND_SYMBOL = 2;
  INDEX_KEY_KIND_TEMPLATE_PATTERN = 3;
}

enum PrimitiveKind {
  PRIMITIVE_KIND_ANY = 0;
  PRIMITIVE_KIND_UNKNOWN = 1;
  PRIMITIVE_KIND_NEVER = 2;
  PRIMITIVE_KIND_VOID = 3;
  PRIMITIVE_KIND_NULL = 4;
  PRIMITIVE_KIND_UNDEFINED = 5;
  PRIMITIVE_KIND_STRING = 6;
  PRIMITIVE_KIND_NUMBER = 7;
  PRIMITIVE_KIND_BOOLEAN = 8;
  PRIMITIVE_KIND_BIGINT = 9;
  PRIMITIVE_KIND_SYMBOL = 10;
  PRIMITIVE_KIND_OBJECT = 11;
}

enum SignatureOrigin {
  SIGNATURE_ORIGIN_FUNCTION_DECLARATION = 0;
  SIGNATURE_ORIGIN_METHOD_DECLARATION = 1;
  SIGNATURE_ORIGIN_CONSTRUCTOR = 2;
  SIGNATURE_ORIGIN_CALL_SIGNATURE = 3;
  SIGNATURE_ORIGIN_CONSTRUCT_SIGNATURE = 4;
  SIGNATURE_ORIGIN_INDEX_SIGNATURE = 5;
  SIGNATURE_ORIGIN_GETTER_ACCESSOR = 6;
  SIGNATURE_ORIGIN_SETTER_ACCESSOR = 7;
}

enum ParameterVariancePolicy {
  PARAMETER_VARIANCE_POLICY_STRICT = 0;
  PARAMETER_VARIANCE_POLICY_BIVARIANT = 1;
}

enum EqualityKind {
  EQUALITY_KIND_STRICT = 0;
  EQUALITY_KIND_LOOSE = 1;
  EQUALITY_KIND_NULLISH = 2;
}
message ExprClass {
  optional string class_name = 1;
  repeated TypeParameterExpr type_parameters = 2;
  repeated ObjectMemberExpr instance_members = 3;
  repeated ObjectMemberExpr static_members = 4;
}
message ExprThisType {}                                  // empty body — only encodes the discriminant
message ExprSatisfies {
  StructuredTypeExpression value = 1;
  StructuredTypeExpression constraint = 2;
}
message ExprUniqueSymbol { string decl_canonical = 1; string name = 2; }
message ExprNoInfer      { StructuredTypeExpression inner = 1; }
```

The `oneof StructuredTypeExpression.kind` is the CLOSED, COMPLETE set of expression forms `EvaluateTypeExpressionGraphRequest.expression` may carry. The `node_taxonomy_complete` guard extends to assert the StructuredTypeExpression set matches the SemanticQueryKey dispatch table in §5.6 (one row per variant). New variant adds bump `schema_version`. Guard: `structured_type_expression_dto_is_closed` (Phase 0a) statically asserts no `// ...` or open-end placeholder remains in the schema source.

Discriminating fixture: `crates/verter_session/tests/typeinfo_graph_fixtures/conditional_infer_promise_unwrap.rs` (Phase 0b/1) — builds `Promise<infer U>` inside a conditional `extends` via `StructuredTypeExpression::Conditional { extends: ExprReference("Promise") with [ExprInfer { name: "U", constraint: None }] }`, dispatches through `evaluate_type_expression_graph_with_audit`, asserts the returned `TypeNode::Conditional.extends` contains a `TypeNode::Infer { name: "U" }` reachable through the reference's `type_arguments`. Without this fixture, the round-trip cannot be discriminated.

`StructuredTypeExpression` is a closed tree. Producers (LSP, MCP, tests) build it by walking their own AST or by constructing it directly. They never serialise raw text. The decoder maps `StructuredTypeExpression` 1:1 to a `SemanticQueryKey` and dispatches through `ProjectSemanticDispatch::execute`. Guard: `evaluate_type_expression_does_not_call_parse_type_annotation`.

### 3.0.1 AuditedResult Carrier

Every `_with_audit` entry-point in §5.3 returns `AuditedResult<T, E>` — a typed carrier that pairs the computation outcome with its `RequestAuditRecord`:

```rust
// crates/verter_protocol/src/typeinfo/audited_result.rs (new in Phase 0a)
pub struct AuditedResult<T, E> {
    pub value: Result<T, E>,
    pub record: RequestAuditRecord,
}
```

The wrapper is intentionally NOT `Result<T, E>` — even on error the caller wants `record` to surface footprint metrics, cache-hit counters, and degraded-reason audit. Callers extract the record unconditionally via `result.record`; the value via `result.value`. Audit nesting (`expandGraphAround` opening a child record under the caller's record) attaches through the record chain inside `record`, not through the wrapper.

TS bridge encoding: `interface AuditedResult<T, E> { value: { ok: T } | { err: E }; record: AuditRecord }`. The TS decoder always produces a `record`; absence implies a decode error (`TypeInfoRequestError::MalformedPayload`).

Guard: `audited_result_carries_record_on_error` — every `_with_audit` error path emits an audit record (not None).

### 3.1 Snapshot Identity

```rust
// verter_protocol::typeinfo::graph
pub struct SemanticTypeGraph {
    pub schema_version: u32,
    pub query: GraphQueryIdentity,
    pub nodes: Vec<TypeNode>,
    pub symbols: Vec<SymbolNode>,
    pub signatures: Vec<Signature>,                    // signatures interned, referenced by SignatureRef = u32
    pub edges: Vec<OriginEdge>,
    pub root_ids: Vec<TypeNodeId>,
    pub exactness: Vec<NodeStatus>,
    pub diagnostics: Vec<TypeInfoDiagnostic>,
    pub node_id_map: Vec<NodeIdMapEntry>,
    pub symbol_id_map: Vec<SymbolIdMapEntry>,
    pub strings: StringTable,
}

pub struct GraphQueryIdentity {
    pub operation: GraphOperation,
    pub roots: Vec<DeclSlotRef>,
    pub path: Vec<TypePathSegment>,
    pub closure: GraphClosurePolicy,
    pub context: ProjectionReductionContext,           // mandatory; absence is a typed request error
    pub display_policy: DisplayPolicy,                  // mandatory
    pub substitutions: Vec<SubstitutionBinding>,
    pub solver_options: SolverOptionsHash,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,
    pub project_identity: Hash16,
    pub resolver_version: u32,
    pub include_provenance: bool,
    pub include_diagnostics: bool,
    pub include_projection: Vec<TypeInfoProjectionKind>,
}
```

Snapshot node IDs are query-local indices. Cross-payload matching uses `SymbolId` resolved through `symbol_id_map` to the stable `ResolvedDeclSlotIdentity`.

### 3.2 Node Taxonomy

The taxonomy is a discriminated union with explicit DTO shapes for every variant. The Rust definition lives in `verter_protocol::typeinfo::graph::TypeNode`. The `node_taxonomy_complete` architecture guard enumerates them against the proto `oneof TypeNode.kind` and the TS DTO discriminant union.

#### 3.2.1 Primitives And Literals

```rust
pub enum TypeNode {
    Primitive(PrimitiveKind),
    UniqueSymbol { decl: SymbolId },
    Literal(LiteralValue),
    // ...
}

pub enum PrimitiveKind {
    Any, Unknown, Never, Void, Null, Undefined,
    String, Number, Boolean, BigInt, Symbol, Object,
}

pub enum LiteralValue {
    String(InternedName),
    Number(F64Bits),                                  // bit-pattern so NaN literals are distinct
    Boolean(bool),
    BigInt(InternedName),
}
```

#### 3.2.2 Algebraic

```rust
TypeNode::Union { members: Vec<TypeNodeId> }
TypeNode::Intersection { members: Vec<TypeNodeId> }
```

Empty union normalises to `Never`; empty intersection to `Unknown`. Guard: `algebraic_normalisation_rules`.

#### 3.2.3 Aggregates

```rust
TypeNode::Object {
    members: Vec<ObjectMember>,
    index_signatures: Vec<IndexSignature>,
    call_signatures: Vec<SignatureRef>,
    construct_signatures: Vec<SignatureRef>,
    flags: ObjectFlags,
}

pub struct ObjectMember {
    name: MemberName,
    name_kind: MemberNameKind,
    value: TypeNodeId,
    optional: bool,
    readonly: bool,
    accessibility: Accessibility,                     // Public | Protected | Private | None
    static_side: bool,
    declaration: Option<SymbolId>,
    jsdoc: Option<JsdocMeta>,
}

pub struct IndexSignature {
    key_kind: IndexKeyKind,
    value: TypeNodeId,
    readonly: bool,
}

bitflags ObjectFlags {
    const FRESH_LITERAL    = 0b0001;
    const WEAK             = 0b0010;
    const EXCESS_LIKELY    = 0b0100;
    const JSX_ATTRIBUTES   = 0b1000;
}

TypeNode::Array { element: TypeNodeId, readonly: bool }
TypeNode::Tuple { elements: Vec<TupleElement>, readonly: bool }

pub struct TupleElement {
    label: Option<InternedName>,
    value: TypeNodeId,
    optional: bool,
    rest: bool,
}
```

#### 3.2.4 Signatures (interned)

Signatures live in a separate arena (`signatures: Vec<Signature>`, indexed by `SignatureRef = u32`) because one Object can contribute many overloads and one signature can appear in multiple intersections.

```rust
pub struct Signature {
    type_parameters: Vec<TypeParameterRef>,
    this_param: Option<ThisParameter>,
    parameters: Vec<SignatureParameter>,
    return_type: TypeNodeId,
    return_predicate: Option<TypePredicate>,
    asserts: Option<AssertionEffect>,
    overload_index: u16,
    is_construct: bool,
    is_implementation: bool,
    is_abstract: bool,
    flags: SignatureFlags,
}
// ... ThisParameter, TypePredicate, AssertionEffect, SignatureFlags as in prior draft
```

Overload sets are an ordered `Vec<SignatureRef>` on the Object. `ReturnType<typeof overloaded>` last-overload behaviour is preserved by ordering. Guard: `signature_last_overload_return_type`.

#### 3.2.5 Aliases, Type Parameters, Generic Application

```rust
TypeNode::Reference { symbol: SymbolId }

TypeNode::AliasInstantiation {
    alias: SymbolId,
    type_arguments: Vec<TypeNodeId>,
    target: TypeNodeId,
    display_ref: Option<TypeNodeId>,
}

TypeNode::TypeParameter {
    symbol: SymbolId,
    decl: DeclSlotRef,
    param_index: u16,
    name: InternedName,
    constraint: Option<TypeNodeId>,
    default: Option<TypeNodeId>,
    variance: Variance,
    is_const: bool,
    no_infer: bool,
    binding: TypeParameterBinding,
}

pub enum TypeParameterBinding {
    Unbound,
    Concrete { type_ref: TypeNodeId },
    Inferred { type_ref: TypeNodeId, source: OriginRef },
    Substituted { type_ref: TypeNodeId, from: TypeNodeId },
}

pub enum Variance { In, Out, InOut, Independent }
```

Guard: `type_parameter_constraint_preserves_symbol_ref`.

#### 3.2.6 Operators

```rust
TypeNode::KeyOf { base: TypeNodeId }
TypeNode::IndexedAccess { object: TypeNodeId, index: TypeNodeId }

TypeNode::Conditional {
    check: TypeNodeId,
    extends: TypeNodeId,
    true_branch: TypeNodeId,
    false_branch: TypeNodeId,
    distributive: bool,
    resolution: ConditionalResolution,
}

pub enum ConditionalResolution {
    SelectedTrue { proof: RelationProofRef },
    SelectedFalse { proof: RelationProofRef },
    UnresolvedGeneric { blockers: Vec<TypeNodeId>, reason: RelationUnknownReason },
    Distributed { cases: Vec<DistributedConditionalCase> },
    Unsupported { reason: UnsupportedConstruct },
}

TypeNode::Mapped {
    key_type: TypeNodeId,
    source: TypeNodeId,
    name_remap: Option<TypeNodeId>,
    value_type: TypeNodeId,
    readonly_modifier: MappedModifier,                // None | Add | Remove
    optional_modifier: MappedModifier,
}

pub enum MappedModifier { None, Add, Remove }

TypeNode::TemplateLiteral { quasis: Vec<InternedName>, expressions: Vec<TypeNodeId> }

TypeNode::TypeOf {
    value_root: ValueRootRef,
    path: Vec<InternedName>,
}

TypeNode::Satisfies { value: TypeNodeId, constraint: TypeNodeId }

TypeNode::Infer {
    name: InternedName,
    constraint: Option<TypeNodeId>,
}
```

Guards: `conditional_distributive_flag_matches_tuple_wrap`, `infer_only_inside_conditional_extends`.

#### 3.2.7 Class Hierarchy

```rust
TypeNode::Class {
    symbol: SymbolId,
    type_parameters: Vec<TypeParameterRef>,
    heritage: Vec<HeritageClause>,
    members: Vec<ObjectMember>,
    static_members: Vec<ObjectMember>,
    construct_signatures: Vec<SignatureRef>,
    flags: ClassFlags,
}
// ... HeritageClause, ClassFlags as in prior draft
```

#### 3.2.8 `this`-Type

```rust
TypeNode::ThisType { decl: SymbolId }
```

#### 3.2.9 Declaration Merging, Module Augmentation, Ambient State

```rust
TypeNode::MergedDeclaration {
    merged: SymbolId,
    parts: SmallVec<[DeclarationPart; 2]>,
}

pub struct DeclarationPart {
    part_symbol: SymbolId,
    canonical: InternedName,
    fingerprint: Hash16,
    kind: DeclPartKind,
}

TypeNode::AmbientModule {
    specifier: InternedName,
    augmenters: Vec<SymbolId>,
    body: TypeNodeId,
}

TypeNode::ModuleAugmentation {
    target: AugmentationTargetRef,
    contributors: Vec<AugmenterContributor>,
    effective_export_set: EffectiveExportSetRef,
}

pub struct AugmenterContributor {
    canonical: InternedName,
    parse_stable_hash: Hash16,
    added_names: Vec<(InternedName, SymbolSpace)>,
}

TypeNode::AmbientNamespace {
    symbol: SymbolId,
    members: Vec<ObjectMember>,
    type_members: Vec<TypeNodeId>,
    nested: Vec<SymbolId>,
}

TypeNode::GlobalAugmentation {
    target_key: AugmentationTargetRef,
    contributors: Vec<AugmenterContributor>,
}

TypeNode::Enum {
    symbol: SymbolId,
    members: Vec<EnumMember>,
    is_const: bool,
}

pub struct EnumMember {
    name: InternedName,
    value: EnumMemberValue,                           // String | Number | Computed
    declaration: SymbolId,
}
```

#### 3.2.10 Flow / Narrowing / Contextual Typing

> SUPERSEDED: flow narrowing and contextual typing are NOT `TypeNode` arms. A.11 (the authoritative revision) moves them into a sibling `ProgramAnalysisGraph`; U8 performs the wire move (re-home the two arms under `ProgramAnalysisGraph`, `reserved` the vacated `GraphTypeNode` tags 26/27) and produces the `TypeInfoGraphPayload { graph, program_analysis }` shape (unified plan §2.2 / cache-export-session U8). Read the shapes below as the program-analysis payload, not `TypeNode` variants.

See §3.11 for the producer chain. The graph EXPOSES these as typed query payloads that callers consult on demand; the typeinfo graph payload always includes the variants in the DTO (even when unpopulated) so the wire shape is stable.

```rust
TypeNode::FlowNarrowing {
    base: TypeNodeId,
    narrowed: TypeNodeId,
    cause: NarrowingCause,
    span: SpanRef,
}

pub enum NarrowingCause {
    TypeofGuard { target: PrimitiveKind, negated: bool },
    InGuard { property: InternedName, negated: bool },
    InstanceofGuard { ctor: SymbolId, negated: bool },
    EqualityGuard { against: TypeNodeId, kind: EqualityKind },
    TruthinessGuard { negated: bool },
    UserPredicate { signature: SignatureRef, negated: bool },
    AssertionEffect { signature: SignatureRef },
    AssignmentFlow { new_type: TypeNodeId },
    OptionalChainNullish,
    DiscriminantUnion { discriminant: InternedName, selected: TypeNodeId },
}

TypeNode::ContextualType {
    contextual: TypeNodeId,
    target_position: ContextPosition,
    inference_bindings: Vec<TypeParameterBinding>,
}
```

#### 3.2.11 Relation Proofs

```rust
TypeNode::RelationProof {
    source: TypeNodeId,
    target: TypeNodeId,
    result: RelationOutcome,
    inference_bindings: Vec<TypeParameterBinding>,
    reason: ProofReason,
}
// ... RelationOutcome, ProofReason as in prior draft
```

#### 3.2.12 Sentinel / Degraded States

```rust
TypeNode::Opaque(QueryErrorDto)
TypeNode::Cycle { cycle_id: u64, entry: TypeNodeId }
```

See §3.8 for the lossless `QueryErrorDto` lowering table. See §3.9 for the `cycle_id` stability rule.

The retired `RecursiveRef` graph node from prior wire schemas remains supported through `QueryErrorDto::RecursiveRef { name: InternedName }` for the legacy boundary (existing `crates/verter_protocol/src/graph/builder.rs::GraphNode::RecursiveRef` callsites are migrated to consume the new `Cycle` representation; the legacy `GraphNode` enum is deleted in Phase 0b/1 per §0.5.1 — the same phase that introduces `verter_protocol::typeinfo::graph::TypeNode`, satisfying the same-phase legacy-deletion rule).

### 3.3 Symbol Node

```rust
pub struct SymbolNode {
    id: SymbolId,
    slot: ResolvedDeclSlotIdentityDto,
    version: Option<VersionedDeclIdentityDto>,
    kind: SymbolKind,
    name: InternedName,
    symbol_space: SymbolSpace,
    declarations: Vec<DeclarationPartRef>,
    merged_parts: Vec<DeclarationPartRef>,
    overloads: Vec<SignatureRef>,
    augmentation_contributors: Vec<AugmenterContributor>,
    type_only: bool,
    ambient: bool,
    export_name: Option<InternedName>,
    re_export_chain: Vec<ReExportHop>,
    target_type_ref: Option<TypeNodeId>,
    span: Option<SpanRef>,
    jsdoc: Option<JsdocMeta>,
}
// ... SymbolKind, SymbolSpace, ReExportHop as in prior draft
```

### 3.4 Origin / Derivation Edges

```rust
pub struct OriginEdge {
    result: TypeNodeId,
    kind: OriginEdgeKind,
    sources: Vec<TypeNodeId>,
    meta: OriginMeta,
}

pub enum OriginEdgeKind {
    Instantiate, SubstituteTypeParam, ConditionalSelect, InferBind,
    ProjectMember, ProjectIndex, ProjectPath, Normalize, AliasResolve,
    SharedLoadReuse,                                   // mirrors verter_audit
}

pub enum OriginMeta {
    None,
    Branch(BranchSelection),
    MemberName(InternedName),
    Index(IndexKeyDto),
    Path(Vec<TypePathSegment>),
    SubstitutedParam(InternedName),
}
```

Guard: `origin_edge_taxonomy_locked` asserts equality across `verter_session::semantic_query::OriginEdgeKind` (nine kinds), `verter_audit::origin_graph::OriginEdgeKind` (ten kinds; +`SharedLoadReuse`), `verter_protocol::typeinfo::graph::OriginEdgeKind` (ten kinds), proto enum, and TS DTO union.

### 3.5 Exactness And Degradation

```rust
pub enum ExpansionStatus {
    ExactResolved,
    ExactSymbolic { reason: SymbolicReason },
    UnresolvedGeneric { blockers: Vec<TypeNodeId>, reason: RelationUnknownReason },
    Miss { reason: MissPrecondition, origin: Vec<OriginRef> },
    Partial { diagnostics: Vec<TypeInfoDiagnostic> },
    Unsupported { construct: UnsupportedConstruct, diagnostics: Vec<TypeInfoDiagnostic> },
    Cycle { cycle_id: u64, entry: TypeNodeId },
    BudgetExceeded { budget: BudgetKind, diagnostics: Vec<TypeInfoDiagnostic> },
    UnstableState { attempts: u8, diagnostics: Vec<TypeInfoDiagnostic> },
}
```

Rules per §2.6 — only `ExactResolved` / `ExactSymbolic` are warm-admitted (with the structural `Cycle` exception from §2.6). All other variants flow through `DegradedResultStore`.

### 3.6 Required Popover Shape (worked example)

For:

```ts
type PopoverMode = "click" | "hover";
type SlotProps<M extends PopoverMode> =
  [M] extends ["hover"] ? { close: undefined } : { close: () => void };
```

A graph payload for `SlotProps<M>` with `M` unbound must contain:

- A `SymbolNode` for `SlotProps` (`kind: TypeAlias`, `symbol_space: Type`).
- A `SymbolNode` for `PopoverMode` (`kind: TypeAlias`, `symbol_space: Type`).
- A `TypeNode::AliasInstantiation { alias: SlotProps.symbol, type_arguments: [M_param], target: <conditional_node> }`.
- A `TypeNode::TypeParameter { symbol: M_symbol, constraint: Some(PopoverMode_ref), binding: Unbound, variance: Independent, is_const: false, no_infer: false }`.
- A `TypeNode::Reference { symbol: PopoverMode.symbol }` reachable from `M.constraint`.
- A `TypeNode::Union { members: [Literal("click"), Literal("hover")] }` reachable as `PopoverMode.target_type_ref`.
- A `TypeNode::Conditional { check: <Tuple [M]>, extends: <Tuple ["hover"]>, true_branch: <Object {close: undefined}>, false_branch: <Object {close: () => void}>, distributive: false, resolution: UnresolvedGeneric { blockers: [M_ref], reason: RelationUnknownReason::UnboundTypeParameter } }`.
- Both branch bodies fully present as graph nodes.

A graph payload for `SlotProps<"hover">` must contain a `ConditionalSelect` origin edge with `Branch(True)` and the false branch is NOT walked. Guard: `popover_slot_props_unresolved_keeps_both_branches`, `popover_slot_props_hover_selects_true_only`, `popover_slot_props_hover_and_click_are_distinct_cache_candidates`.

### 3.7 InternedName Carrier Specification

`InternedName` in `verter_protocol::typeinfo::graph::*` DTOs is wire-carried via an explicit string-table envelope. The schema decision is: **interned (Option B)**.

```rust
// verter_protocol::typeinfo::graph
pub struct InternedName(pub u32);                     // index into SemanticTypeGraph.strings

pub struct StringTable {
    pub strings: Vec<String>,                         // index = InternedName.0
}
```

Proto representation:

```protobuf
message StringTable { repeated string entries = 1; }  // indexed by InternedName u32

// InternedName fields are encoded as uint32 in every message that
// references them. Decoders resolve through the SemanticTypeGraph.strings
// table.
```

In-process Rust callers may use `InternedName::to_str(&strings) -> &str` for display. Across NAPI/WASM the string table travels with the payload (cost amortised — large payloads benefit from interning). JS consumers see `interface InternedName { __id: number }` with a helper `getString(graph, name)`.

The host-side `verter_semantic::facts::registry::InternedName` remains a separate type (the host symbol table is independent from the wire string table). The exporter (§8 Phase 2) populates the wire `StringTable` during snapshot. Guard: `interned_name_wire_format_uses_string_table`.

### 3.8 QueryErrorDto Lossless Lowering

`verter_session::semantic_query::QueryError` is the authoritative producer-side error enum (`Miss | UnsupportedIntrinsic { name } | BudgetExceeded(BudgetExceededFailure) | UnstableState { attempts } | AliasCycle { chain } | RecursiveRef { name } | Other(Arc<str>) | DeclPlaceholder { canonical_id, name, whole_hash }`).

The real `BudgetExceededFailure` at `crates/verter_session/src/resolver_core/shallow_file_state.rs:225` carries `{ domain: BudgetDomain, limit: usize, actual: u64, context: String }` — NO `detail_name` field exists. The lowering preserves all four fields losslessly.

The wire `QueryErrorDto` is a lossless mirror:

```rust
pub enum QueryErrorDto {
    Miss { reason: MissPrecondition, origin: Option<OriginRef> },
    UnsupportedIntrinsic { name: InternedName },
    Unsupported { construct: UnsupportedConstruct },
    BudgetExceeded {
        budget: BudgetKind,        // closed enum mapped from failure.domain via §2.22 table
        limit: u32,                 // failure.limit narrowed (usize → u32; overflow saturates)
        actual: u64,                // failure.actual preserved verbatim
        context: InternedName,      // failure.context interned through the StringTable
    },
    UnstableState { attempts: u8 },
    AliasCycle { chain: Vec<InternedName> },
    Cycle { cycle_id: u64, entry: TypeNodeId },
    RecursiveRef { name: InternedName },
    DeclPlaceholder { canonical_id: InternedName, name: InternedName, whole_hash: Hash16 },
    Other { message: InternedName },
}

pub enum MissPrecondition {
    NoSymbolMatch,
    NoIndexedReady,
    AugmentationNotFound,
    OverloadResolutionFailed,
    ImportNotResolvable,
    DeclPlaceholderRequiresInstantiate,
}

pub enum UnsupportedConstruct {
    Decorator,
    UmdGlobal,
    LegacyTypeguard,
    JsxIntrinsicHostElement,
    LegacyConstAssertOutsideExpression,
}
```

Lowering table:

| Native `QueryError` | Wire `QueryErrorDto` |
|---|---|
| `Miss` | `Miss { reason: MissPrecondition::NoSymbolMatch, origin: None }` (the producer classifies the specific precondition before invoking the lowering; the catch-all `NoSymbolMatch` is used only when no producer-side classification fired). |
| `UnsupportedIntrinsic { name }` | `UnsupportedIntrinsic { name }` |
| `BudgetExceeded(failure)` | `BudgetExceeded { budget: budget_kind_from_domain(failure.domain), limit: failure.limit as u32, actual: failure.actual, context: intern(&failure.context) }` |
| `UnstableState { attempts }` | `UnstableState { attempts }` |
| `AliasCycle { chain }` | `AliasCycle { chain }` |
| `RecursiveRef { name }` | `RecursiveRef { name }` |
| `Other(msg)` | `Other { message: msg }` |
| `DeclPlaceholder { canonical_id, name, whole_hash }` | `DeclPlaceholder { canonical_id, name, whole_hash }` |

The lowering preserves enough information for a caller to act on the failure (e.g. retry through `Instantiate` for `DeclPlaceholder`; render the cycle chain for `AliasCycle`; surface the over-budget domain, limit, and context for `BudgetExceeded`). Guard: `queryerror_dto_is_lossless` round-trips every native `QueryError` variant through `QueryErrorDto` and back, asserting field-by-field equality (including the four `BudgetExceeded` fields). Guard: `budget_exceeded_dto_uses_real_failure_fields` statically scans the lowering for any reference to a `detail_name` field on `BudgetExceededFailure` and rejects it.

### 3.9 Cycle ID Stability

`Cycle { cycle_id: u64, entry: TypeNodeId }` represents either:

- a structural recursive type (`type Tree = { children: Tree[] }`) — `cycle_id` keys the back-edge,
- a cycle precondition in degraded resolution — `cycle_id` keys the cycle participants.

`cycle_id` is computed as `stable_hash64(ResolvedDeclSlotIdentity)` of the declaration the cycle re-enters. Across distinct queries that hit the same recursive type, the `cycle_id` is identical. Zod's `z.lazy(() => ...)` memoisation keys on `cycle_id` and binds the lazy reference once per cycle root. Guards: `cycle_id_stable_across_queries`, `zod_recursive_emits_lazy`.

### 3.10 Variance / Const / NoInfer Producer Chain

OXC AST → shallow analysis → typed IR:

| OXC AST node | Shallow analysis output | Typed IR | DTO |
|---|---|---|---|
| `TSTypeParameter.in: true` (modifier `in` keyword) | `AnalyzedTypeParameter::variance = Variance::In` | `TypeExpr::TypeParam.variance = Variance::In` | `TypeNode::TypeParameter.variance = Variance::In` |
| `TSTypeParameter.out: true` (modifier `out` keyword) | `AnalyzedTypeParameter::variance = Variance::Out` | `TypeExpr::TypeParam.variance = Variance::Out` | `TypeNode::TypeParameter.variance = Variance::Out` |
| `TSTypeParameter.in: true && out: true` | `AnalyzedTypeParameter::variance = Variance::InOut` | `TypeExpr::TypeParam.variance = Variance::InOut` | `TypeNode::TypeParameter.variance = Variance::InOut` |
| no `in`/`out` modifier | `Variance::Independent` | `Variance::Independent` | `Variance::Independent` |
| `TSTypeParameter.const: true` (`const T extends string`) | `AnalyzedTypeParameter::is_const = true` | `TypeParam.is_const = true` | `TypeNode::TypeParameter.is_const = true` |
| `NoInfer<T>` wrapper in the source | shallow analysis records the wrapping decl, sets `AnalyzedTypeParameter::no_infer = true` on T | `TypeParam.no_infer = true` | `TypeNode::TypeParameter.no_infer = true` |

The producer side lives in `verter_semantic::analysis::type_parameter_analysis` (extended in §8 Phase 1). Each OXC node maps mechanically; no text parsing, no name-based detection. Guard: `variance_annotations_lowered_through_oxc` exercises each fixture (`type C<in T, out U> = ...`, `function f<const T>(x: T)`, `type N<T> = NoInfer<T>`) and asserts the typed IR carries the right `variance` / `is_const` / `no_infer` flags BEFORE any DTO conversion.

### 3.11 FlowNarrowing / ContextualType Producer Chain

Flow narrowing and contextual typing are not part of the default `resolveSymbolGraph` payload — they require a dedicated query. Env hashes flow through `ResolverContext` per the uniform convention (§2.17); the variants themselves only carry semantic identity:

- `SemanticQueryKey::FlowNarrowingAt { canonical, span }` → returns `TypeNode::FlowNarrowing` for the given expression position.
- `SemanticQueryKey::ContextualTypeAt { canonical, span }` → returns `TypeNode::ContextualType`.

`TypeInfoSession.evaluateFlowNarrowingAt(canonical, span)` and `.evaluateContextualTypeAt(canonical, span)` are public typeinfo methods. They are admitted to the typeinfo graph payload only when `closure: GraphClosurePolicy::ProjectionRequired { projection: FlowNarrowing | ContextualType }`. Default closure does NOT include them (keeps default payload size manageable). Guard: `flow_narrowing_only_when_projection_required`.

The producer side lives in `verter_session::flow_analysis` (extended to surface results through a `SemanticQueryKey` variant — analysis already exists internally for diagnostics; this surfaces it).

---

## 4. Cache Topology

### 4.1 New Caches In This Plan

#### 4.1.1 `TypeInfoGraphResultDb`

Final-result cache for top-level typeinfo graph queries.

```rust
// crates/verter_session/src/typeinfo_graph_db.rs
pub struct TypeInfoGraphResultDb {
    entries: BoundedCandidateMap<
        TypeInfoGraphSlotKey,
        VersionedTypeInfoGraphIdentity,
        TypeInfoGraphCandidate,
    >,
    inflight: InflightTable<TypeInfoGraphSlotKey>,    // §4.3 — concurrent cold collapse
    retention_gate: RetentionGate,
    completion_fence: Arc<CompletionFence>,
    audit: Arc<HostAuditRuntime>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct TypeInfoGraphSlotKey {
    pub operation: GraphOperation,
    pub roots: Arc<[ResolvedDeclSlotIdentity]>,
    pub relation_operands: Arc<[ResolvedDeclSlotIdentity]>, // empty unless operation == Relate
    pub path: Arc<[PathSegment]>,
    pub substitutions: Arc<[SubstitutionKey]>,         // type-param slot → concrete (or symbolic) binding
    pub context: ProjectionReductionContext,
    pub closure: GraphClosurePolicy,
    pub display_policy: DisplayPolicy,
    pub include_provenance: bool,
    pub include_diagnostics: bool,
    pub include_projection: Arc<[TypeInfoProjectionKind]>,
    pub solver_options: SolverOptionsHash,
    pub parse_env_hash: Hash16,
    pub resolve_env_hash: Hash16,
    pub type_env_hash: Hash16,
    pub lib_env_hash: Hash16,                         // always included — TS intrinsics depend on lib data
    pub project_identity: Hash16,
    pub resolver_version: u32,
}

> SUPERSEDED: `SubstitutionConcrete` is retired; `CanonicalSubstitutionValueKey` (A.10) is the single substitution-value carrier (unified plan §2.2). The `SubstitutionKey` below carries the A.10 carrier in place of the `concrete` field shown.

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct SubstitutionKey {
    pub type_param_slot: ResolvedDeclSlotIdentity,
    pub concrete: SubstitutionConcrete,
}

pub enum SubstitutionConcrete {
    Node { decl: ResolvedDeclSlotIdentity },           // resolved to a declaration
    Literal(LiteralValue),                              // bound to a literal type
    Symbolic { tag: InternedName },                    // open / unbound binding (e.g. caller's own T)
}

// `Symbolic.tag` MUST be a canonical-form interned string. The canonicalisation
// rule, applied at intern time:
//   1. Trim surrounding whitespace.
//   2. Internal whitespace runs collapse to a single space.
//   3. Case is preserved (TypeScript identifiers are case-sensitive).
//   4. Surrounding parentheses on the unparsed source representation are stripped.
// Two semantically-identical tags ("  T  " and "T") MUST intern to the same
// `InternedName.0` and therefore produce one cache candidate, NOT two.
// Guard: `substitution_symbolic_tags_canonicalised`.

pub struct VersionedTypeInfoGraphIdentity {
    pub root_versions: SmallVec<[VersionedDeclIdentity; 2]>,
    pub solver_options_hash: SolverOptionsHash,
}

pub struct TypeInfoGraphCandidate {
    pub payload: Arc<TypeInfoGraphPayload>,
    pub version: VersionedTypeInfoGraphIdentity,
    pub read_set: ReadSetSignature,
    pub self_root_canonicals: Arc<[(Arc<str>, FileWholeHash)]>,
}
```

The slot key composition satisfies R7 (content-free), R20 (multi-candidate dimension is the env-hash tuple), R21 (five env hashes + project_identity + resolver_version explicit; no bundled config hash). Per-slot capacity 4, global budget 512.

The `substitutions` field is mandatory: `SlotProps<"hover">` and `SlotProps<"click">` are distinct cache candidates because their `Arc<[SubstitutionKey]>` differ. Guard: `popover_slot_props_hover_and_click_are_distinct_cache_candidates`.

Warm-hit validation:

1. Clone the candidate `Arc` under shared read.
2. Validate `self_root_canonicals` against the live `StoreView` via `crate::fact_signature_helpers::validate_fact_signature_with_self_roots`.
3. Validate `read_set.facts` against the live `StoreView`.
4. On conflict, mark the candidate stale (FIFO-eligible) and fall through to cold compute.
5. On success, return `Arc<TypeInfoGraphPayload>` with zero allocation.

Cold publish routes through `CompletionFence` with `MAX_INFLIGHT_RETRIES = 3` (§0.6). On exhaustion, return `Opaque(QueryErrorDto::UnstableState { attempts: 3 })`; do NOT admit.

Guards: `typeinfo_graph_warm_hit_zero_alloc`, `typeinfo_graph_warm_hit_revalidates_self_root`, `typeinfo_graph_no_partial_admission`, `typeinfo_graph_publication_fence_3_retries`.

#### 4.1.2 `DegradedResultStore`

A separately keyed store for partial / budget-exceeded / unstable / cancelled / unsupported results. Callers explicitly opt in to read from it (`include_degraded: true` on the request). Callers that don't opt in never see degraded answers as warm-cached complete results. Keyed on `(TypeInfoGraphSlotKey, DegradationReason)`. Bounded by a small LRU (256 entries global). Guard: `degraded_store_never_serves_complete_admission`.

#### 4.1.3 `RelationEngineMemo` (reuses existing `SemanticGraphStore::relation_memo`)

Already on `SemanticGraphStore` with dep-signature fencing and 4096 global budget. No new cache — only a public API surface change (§5.2).

### 4.2 Reused Caches (R21 mechanical audit)

Every reused cache's key composition is explicit. Each row enumerates which of the five env-hash dimensions appears in the key, plus rationale for any "excluded":

| Cache | Family | Key struct + env dimensions |
|---|---|---|
| `FileArtifactStore` | Content-addressed | `FileArtifactKey { canonical, content_hash, parse_env_hash, parser_version }`. `resolve_env_hash`/`type_env_hash`/`lib_env_hash`/`project_identity` EXCLUDED — parsing is invariant under resolver/type/lib/project options (the file is parsed once per content+parser feature flag). |
| `FileArtifactStore::augmentation_index` | Content-addressed | `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }`. `parse_env_hash`/`type_env_hash` EXCLUDED — augmentation target identity depends on which lib + which resolver paths see the augmenter, not on parsing or strictness options. |
| `ResolvedImportFacts` | Content-addressed | `ResolvedImportFactsKey { canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version }`. `type_env_hash`/`lib_env_hash`/`project_identity` EXCLUDED — import facts depend on resolver paths, not type semantics or project identity. |
| `RouteDb` per-name | Query-identity | `RouteByNameKey { provider_canonical, exported_name, symbol_space, resolve_env_hash, lib_env_hash, resolver_version, project_identity }`. `parse_env_hash`/`type_env_hash` EXCLUDED — routes depend on resolver + lib + project (lib augments routes); parsing/strictness do not change route identity. |
| `RouteDb` effective barrel surface | Query-identity | `EffectiveBarrelKey { provider_canonical, resolve_env_hash, lib_env_hash, resolver_version, project_identity }`. Same justification. |
| `MaterializeStructureDb` | Query-identity | `MaterializationCacheKey { decl: ResolvedDeclSlotIdentity, projection_path, projection_mode, normalized_type_args, options_hash, resolve_env_hash, type_env_hash, lib_env_hash, project_identity }`. `parse_env_hash` EXCLUDED via `parse_stable_hash` invariance (cosmetic edits don't change materialised shape). |
| `RefCycleResultDb` | Query-identity | `RefCycleKey { root_slot: ResolvedDeclSlotIdentity, resolve_env_hash, type_env_hash, lib_env_hash, project_identity }`. `parse_env_hash` EXCLUDED via `parse_stable_hash` invariance on the self-root set. |
| `SemanticGraphStore` query nodes | Query-identity | One key struct per `SemanticQueryKey` variant — see §2.17 / §3.11 for explicit struct definitions of the seven new variants. Env-hash inclusion convention: every variant flows `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`, `resolver_version` through `ResolverContext` and embeds them at the dispatch boundary (mirroring the existing `Instantiate { base: DeclIdentity, args, context: ResolverContext }` pattern); `parse_env_hash` EXCLUDED via `parse_stable_hash` invariance. The seven new variants follow the same convention — no variant lists env fields directly on its struct. |
| `SemanticGraphStore::relation_memo` | Query-identity | `RelationKey { source_node: SemanticNodeId, target_node: SemanticNodeId, resolve_env_hash, type_env_hash, lib_env_hash, project_identity, resolver_version }`. The node IDs themselves encode their parse/version state (`parse_env_hash` flows through `SemanticNodeId`). |
| `ComponentMetaResultDb` | Query-identity | `ComponentMetaSlotKey { owner: OwnerIdentity, resolve_env_hash, type_env_hash, lib_env_hash, project_identity, resolver_version, fallthrough_policy, prepared_surface_policy, materializer_options_hash }`. `parse_env_hash` EXCLUDED via `parse_stable_hash` invariance on the owner identity. Owner identity is content-free (`ResolvedDeclSlotIdentity`-based). No ellipsis — every field is named here. |
| `MemberSemanticFactStore` | Content-addressed | `(canonical, parse_stable_hash, parse_env_hash, exporter, name, space, resolve_env_hash, lib_env_hash)`. `type_env_hash` EXCLUDED — semantic member facts don't depend on strictness flags. `project_identity` EXCLUDED — facts are per-canonical, project-isolated through project membership at the store level. |
| `MemberDisplayFactStore` | Content-addressed | `(canonical, content_hash, parse_env_hash, exporter, name, space, type_env_hash, lib_env_hash)`. Display depends on strictness flags. |
| `ModuleAugmentationIndex` (on `FileArtifactStore`) | Content-addressed | `AugmentationTargetKey` (same as `augmentation_index` above). |

Guard: `r21_lib_env_hash_inclusion` enumerates every cache layer above and asserts the lib_env_hash inclusion is correct per R21. Guard: `r21_no_bundled_config_hash` scans every cache key struct for fields named `*_config_hash` and rejects them.

### 4.3 Concurrent Cold-Collapse Design

Concurrent identical typeinfo graph requests dispatch into one cold compute via `cooperative_admit_with_post_publish` (existing helper at `crates/verter_session/src/cooperative_admission.rs:896`). The real signature carries 10 arguments — `TypeInfoGraphResultDb::get_or_compute` binds every callback:

```rust
pub fn get_or_compute(
    &self,
    key: TypeInfoGraphSlotKey,
    request: &TypeInfoGraphRequest,
) -> Result<Arc<TypeInfoGraphPayload>, TypeInfoRequestError> {
    let outcome = cooperative_admit_with_post_publish::<
        TypeInfoGraphSlotKey,
        TypeInfoGraphCandidate,
        Arc<TypeInfoGraphPayload>,
        _, _, _, _, _, _,
    >(
        &self.entries,
        &self.inflight,
        key.clone(),
        /* validate          */ |entry| self.peek_validated(&key, entry, request),
        /* compute           */ || self.cold_export(request),
        /* project           */ |candidate| candidate.payload.clone(),
        /* revalidate (post) */ |candidate| self.revalidate_against_store_view(candidate),
        /* removal_cleanup   */ |k, candidate| self.audit.emit_evict(k, candidate),
        /* post_publish      */ |candidate, _k| self.publication_fence.fire(candidate),
        /* publish_fence     */ Some(&self.publish_fence_lock),
    );
    match outcome {
        Some(payload) => Ok(payload),
        None          => Err(TypeInfoRequestError::UnstableState { attempts: 3 }),
    }
}
```

Callback roles:

- `validate` — warm-hit gate. Reads the candidate's `read_set` and `self_root_canonicals` against the live `StoreView`. Returns `Some(payload)` only when both gates pass.
- `compute` — cold export. Routes through `CompletionFence::publish_with_retry` (§2.5) which itself uses `MAX_INFLIGHT_RETRIES = 3`. Yields a `ComputeAdmission<V, Entry>`.
- `project` — final value extraction from the published `Entry` (`Arc<TypeInfoGraphCandidate>` → `Arc<TypeInfoGraphPayload>` via `Arc::clone` on the inner payload).
- `revalidate_after_compute` — joiner-view revalidation. A follower joining the winner's in-flight build runs `revalidate` against ITS OWN view; on failure the follower forks and cold-recomputes for its view.
- `removal_cleanup` — invoked on stale-entry eviction (live counter / reverse index symmetry; audit `emit_evict`).
- `post_publish` — runs AFTER the entry is admitted to the map; fires the publication fence's reverse-dep recording.
- `publish_fence` — the `parking_lot::RwLock<()>` instance held across publish; serialises with concurrent reads of the candidate map.

Return shape: `Option<V>`. `None` after 3 retries surfaces as `TypeInfoRequestError::UnstableState { attempts: 3 }` (never panic, never warm-admit). Joiners attached to the winner's in-flight entry emit `OriginEdgeKind::SharedLoadReuse` in their per-request audit record. Guards: `typeinfo_graph_concurrent_cold_collapses_one_exporter_run` (N=32 concurrent identical requests produce exactly one exporter invocation and N waiters; the audit shows 1 winner + 31 `SharedLoadReuse` entries), `typeinfo_graph_unstable_returns_typed_error` (3-retry exhaustion path returns `TypeInfoRequestError::UnstableState { attempts: 3 }` not panic).

### 4.4 Multi-Candidate Dimension Audit

`BoundedCandidateMap<K, D, V>` holds up to 4 candidates per slot keyed by `D` (the "candidate distinguishing" dimension). For `TypeInfoGraphResultDb`, `D = VersionedTypeInfoGraphIdentity { root_versions, solver_options_hash }`. Two candidates coexist in the same slot when:

- They share `TypeInfoGraphSlotKey` (same operation, roots-slot, path, substitutions, context, closure, display_policy, projection set, env hashes, resolver_version).
- They differ on `VersionedTypeInfoGraphIdentity` — i.e. the same logical query computed against different `VersionedDeclIdentity` versions of the same root.

The candidate distinguishing dimension is the SOLE source of multi-candidate divergence. Cache keys that bundle version into the key (instead of the value) violate R20 and are rejected by `cache_key_no_value_fields`.

Guard: `multi_candidate_distinguishing_dimension_is_value_only` walks every `BoundedCandidateMap` instance and asserts the key type is `Hash + Eq` and the distinguishing-dimension type is on the value side.

### 4.5 Cache-Hit Walk-Through (Popover Worked Example)

A first request:
`resolveSymbolGraph(canonical="popover.vue", name="SlotProps", { context: { mode: Shallow, demand: Published }, closure: RootOnly, displayPolicy: <default>, ... })`

1. Slot key built. All five env hashes populated.
2. `TypeInfoGraphResultDb::get_or_compute(slot_key, compute)` → `peek_validated` returns miss → cooperative admission gates the cold path.
3. `ProjectSemanticDispatch::execute(SemanticQueryKey::ResolveDecl(...))` runs cold. Touches `IndexedReady`, walks the type body, materialises the conditional shell with both branches as references.
4. Exporter snapshots reachable nodes from the root within `closure: Shallow`, populates `read_set` via `with_fact_tracer`.
5. Capture `self_root_canonicals` — `popover.vue` + the canonical of `PopoverMode`'s declaration.
6. `CompletionFence::publish_with_retry` revalidates → admit.
7. Joiners (if any) attach to the same `Arc<TypeInfoGraphPayload>`; their audit records carry `SharedLoadReuse`.

A second request, same key, same files unchanged:
1. Warm peek → candidate.
2. Validate `self_root_canonicals` → OK. Validate `read_set.facts` → OK.
3. Return `Arc<TypeInfoGraphPayload>` (zero allocation).

A third request after a cosmetic edit (whitespace in `popover.vue`):
1. `parse_stable_hash` unchanged → `MemberSemanticFactStore` entries unchanged.
2. `read_set.facts` validates OK.
3. Return cached payload.

A fourth request after an edit that adds `"press"` to `PopoverMode`:
1. New `MemberPresence(PopoverMode, "press")` fact emitted.
2. `read_set.facts` contained `MemberShape(PopoverMode)` → live fingerprint differs → revalidation fails → candidate stale.
3. Cold recompute through fence → new candidate admitted.

Guard: `popover_request_cache_walkthrough` in `tests/typeinfo_graph_cache_walkthrough.rs`.

---

## 5. Public API Surface

### 5.1 Wire Format

```
proto/verter/v1/typeinfo.proto
    │
    ├── prost-build (build.rs) ─► verter_protocol::verter::v1::*  (generated Rust)
    ├── protobuf-ts toolchain    ─► packages/proto/src/gen/verter/v1/typeinfo_pb.ts  (generated TypeScript)
    └── manual NAPI / WASM bridge ─► verter_ffi::typeinfo::*
```

Three FFI surfaces (NAPI, WASM, in-process Rust) all share the prost-derived DTOs. NAPI/WASM payloads cross as binary protobuf `Buffer` / `Uint8Array`. Decoder lives in `packages/typeinfo/src/decode.ts` — pure mechanical mapping with zero semantic recovery. Guard: `typeinfo_decoder_is_pure_mechanical`.

### 5.2 TypeScript API

```ts
// packages/typeinfo/src/session.ts
export interface TypeInfoSession {
  listSymbols(canonicalId: string): SymbolEntry[]

  resolveSymbolGraph(request: ResolveSymbolGraphRequest): TypeInfoGraphPayload

  evaluateTypeExpressionGraph(request: EvaluateTypeExpressionGraphRequest): TypeInfoGraphPayload

  projectPathGraph(request: ProjectPathGraphRequest): TypeInfoGraphPayload

  relate(request: RelateRequest): RelationPayload

  evaluateFlowNarrowingAt(request: FlowNarrowingRequest): TypeInfoGraphPayload

  evaluateContextualTypeAt(request: ContextualTypeRequest): TypeInfoGraphPayload

  expandGraphAround(request: ExpandGraphAroundRequest): TypeInfoGraphPayload

  getFrameworkSurfaces(request: FrameworkSurfaceRequest): FrameworkSurfacePayload
}

// Every request type has context + displayPolicy as REQUIRED fields,
// not `options?:`. Omitting them returns TypeInfoRequestError before
// any semantic execution runs.
export interface ResolveSymbolGraphRequest {
  canonicalId: string
  name: string
  context: ProjectionReductionContext               // REQUIRED
  closure: GraphClosurePolicy                       // REQUIRED
  displayPolicy: DisplayPolicy                      // REQUIRED
  includeProvenance: boolean
  includeDiagnostics: boolean
  includeProjection?: TypeInfoProjectionRequest[]
  includeDegraded?: boolean                          // opt-in to DegradedResultStore reads
}

export interface EvaluateTypeExpressionGraphRequest {
  scopeCanonical: string
  expression: StructuredTypeExpression              // structured; no raw text — see §5.6
  extraImports: ImportSpec[]
  context: ProjectionReductionContext
  closure: GraphClosurePolicy
  displayPolicy: DisplayPolicy
  includeProvenance: boolean
  includeDiagnostics: boolean
  includeProjection?: TypeInfoProjectionRequest[]
}

export interface ProjectionReductionContext {
  mode: "identity" | "navigate" | "shallow" | "expanded" | "skeleton"
  demand: "published" | "structuralTransit"
}

export type GraphClosurePolicy =
  | { kind: "rootOnly" }                                              // bound: ≤ 1 node (the root)
  | { kind: "path"; path: TypePathSegment[] }                        // bound: ≤ path.length + 1 nodes
  | { kind: "oneLevel" }                                              // bound: ≤ 1 + (root.direct_members + root.direct_constituents) nodes
  | { kind: "expanded"; nodeBudget: number; depthBudget: number }   // REQUIRED — no default
  | { kind: "projectionRequired"; projection: TypeInfoProjectionKind }   // bound: closure derived from the named projection's required edges

export type TypeInfoRequestError =
  | { kind: "MissingProjectionContext" }
  | { kind: "MissingDisplayPolicy" }
  | { kind: "InvalidMode"; received: string }
  | { kind: "MissingClosurePolicy" }
  | { kind: "UnknownSchemaVersion"; wireVersion: number; expectedVersion: number }
  | { kind: "MalformedPayload"; detail: string }
  | { kind: "OmittedRoots" }
  | { kind: "MalformedStructuredExpression"; detail: string }
```

`closure.kind: "expanded"` requires explicit `nodeBudget` and `depthBudget` — there is no defaulting. Every OTHER closure variant has a structural bound derived from the request, not a user-supplied number:

- `rootOnly` — exactly the root node and its `SymbolNode`. No transitive walk.
- `path { path }` — exactly the nodes along `path` (one node per `TypePathSegment`). Sibling members are NOT included.
- `oneLevel` — root + its direct structural members / union arms / intersection arms / type arguments (one BFS hop).
- `projectionRequired { projection }` — the closed set of edges the named projection MUST observe, computed from the projection's static-declared `requiredEdges` (each projection declares which `OriginEdgeKind`s it consumes).

Over-budget sub-trees produce `TypeNode::Opaque(QueryErrorDto::BudgetExceeded { budget, ... })` at the over-budget node; the surrounding payload IS degraded (status `Partial`) and is admitted only to `DegradedResultStore`, NOT to the warm cache (§2.6). Guard: `every_closure_variant_has_concrete_resource_bound` asserts each non-`expanded` closure variant has a declared bound function and rejects implicit unbounded closures.

All nine request DTOs are defined here (no name referenced without a schema):

```ts
// 1. listSymbols(canonicalId): scalar — uses parameter directly, no Request DTO.
//    EXEMPTION: `listSymbols` is a directory operation (inventory of names),
//    not a graph operation. It returns `SymbolEntry[]`, not a SemanticTypeGraph.
//    No closure / displayPolicy applies. Guard `list_symbols_is_scalar` enforces
//    that the public API signature stays `listSymbols(canonicalId: string)` —
//    accidentally bundling it into a Request DTO regresses the exemption.

// 2. ResolveSymbolGraphRequest — defined above.
// 3. EvaluateTypeExpressionGraphRequest — defined above.

// 4.
export interface ProjectPathGraphRequest {
  canonicalId: string
  name: string
  path: TypePathSegment[]                              // REQUIRED — empty path is a typed error
  context: ProjectionReductionContext
  closure: GraphClosurePolicy
  displayPolicy: DisplayPolicy
  includeProvenance: boolean
  includeDiagnostics: boolean
  includeProjection?: TypeInfoProjectionRequest[]
  includeDegraded?: boolean
}

// 5.
export interface RelateRequest {
  source: TypeNodeRef
  target: TypeNodeRef
  context: ProjectionReductionContext
  displayPolicy: DisplayPolicy
  // EXEMPTION: `closure` is not carried — the relation pair (source, target)
  // is its own closure. Adding a closure dimension would force callers to
  // express "include all derivation edges starting from source" which is
  // already the relation engine's contract. Guard
  // `relate_has_no_closure_field` enforces.
}

// 6.
export interface FlowNarrowingRequest {
  canonicalId: string
  span: SpanRef                                         // expression position
  context: ProjectionReductionContext
  closure: { kind: "projectionRequired"; projection: "flow_narrowing" }
  displayPolicy: DisplayPolicy
  includeProvenance: boolean
  includeDiagnostics: boolean
}

// 7.
export interface ContextualTypeRequest {
  canonicalId: string
  span: SpanRef                                         // expression position
  context: ProjectionReductionContext
  closure: { kind: "projectionRequired"; projection: "contextual_type" }
  displayPolicy: DisplayPolicy
  includeProvenance: boolean
  includeDiagnostics: boolean
}

// 8.
export interface ExpandGraphAroundRequest {
  parentGraph: GraphHandle                              // opaque, from prior payload
  target: TypeNodeRef                                   // node to expand around
  context: ProjectionReductionContext                   // widened mode
  closure: GraphClosurePolicy                           // widened closure
  displayPolicy: DisplayPolicy
  includeProvenance: boolean
  includeDiagnostics: boolean
}

// 9.
export interface ComponentSelector {
  canonicalId: string
  exportName?: string
  frameworkAdapterId: string   // R7-EXT: open canonical adapter id (e.g., "vue", "svelte", "react", "solid", "my-corp-fw");
                                // matches §6.4 FrameworkAdapterRegistry; host interns at receive.
}

export interface FrameworkSurfaceRequest {
  selector: ComponentSelector
  context: ProjectionReductionContext
  closure: GraphClosurePolicy
  displayPolicy: DisplayPolicy
  includeProvenance: boolean
  includeDiagnostics: boolean
  includeProjection?: TypeInfoProjectionRequest[]
  schemaVersion: number
}

export interface RelationPayload {
  graph: SemanticTypeGraph
  result: RelationOutcome
  proof: RelationProof
  inferenceBindings: TypeParameterBinding[]
  diagnostics: TypeInfoDiagnostic[]
}
```

The two exemptions (`listSymbols`, `relate`) are NAMED with explicit rationale; the seven other entry-points all carry `context + closure + displayPolicy` as MANDATORY fields. Guards: `every_typeinfo_request_carries_context_or_is_exempted_with_rationale`, `list_symbols_is_scalar`, `relate_has_no_closure_field`.

### 5.3 Native API Surface (Rust)

```rust
// crates/verter_session/src/typeinfo/session.rs
impl TypeInfoSession {
    pub fn resolve_symbol_graph_with_audit(&self, request: ResolveSymbolGraphRequest) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError>;
    pub fn evaluate_type_expression_graph_with_audit(&self, request: EvaluateTypeExpressionGraphRequest) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError>;
    pub fn project_path_graph_with_audit(&self, request: ProjectPathGraphRequest) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError>;
    pub fn relate_with_audit(&self, request: RelateRequest) -> AuditedResult<Arc<RelationPayload>, TypeInfoRequestError>;
    pub fn evaluate_flow_narrowing_at_with_audit(&self, request: FlowNarrowingRequest) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError>;
    pub fn evaluate_contextual_type_at_with_audit(&self, request: ContextualTypeRequest) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError>;
    pub fn expand_graph_around_with_audit(&self, request: ExpandGraphAroundRequest) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError>;
    pub fn get_framework_surfaces_with_audit(&self, request: FrameworkSurfaceRequest) -> AuditedResult<Arc<FrameworkSurfacePayload>, TypeInfoRequestError>;
}
```

Every entry-point goes through the audit runtime (§10) and returns `Arc<...>` (R7). Request-validation paths return `TypeInfoRequestError` BEFORE any semantic execution (no `todo!()` is reachable; see §8.1 step 1).

### 5.4 Compatibility Surface

The legacy `resolveSymbol(...)` / `evaluateTypeExpression(...)` callsites are migrated in the SAME phase as their replacement lands (§8.4 Phase 3). After migration these functions do not exist; consumers call the graph methods + the relevant projection (`toTypeDescriptor(payload)`). There is no permanent legacy wrapper — the legacy `TypeDescriptor` shape continues to exist as a projection target (§7.5), but the legacy entry-point names are removed.

### 5.5 Graph Handles And Progressive Expansion

`GraphHandle` is an opaque token carrying `GraphQueryIdentity` + `snapshotEpoch`. `expandGraphAround(request)` issues a fresh `resolveSymbolGraph` with the new target + widened mode; the native side memoises through `TypeInfoGraphResultDb` so identical re-expansions reuse the cached payload. No server-side state. Guards: `progressive_graph_expansion_dispatches_semantic_query_key`, `progressive_expansion_routes_through_typeinfo_graph_db`.

### 5.6 StructuredTypeExpression — replacing raw-text expressions

`EvaluateTypeExpressionGraphRequest.expression` is a `StructuredTypeExpression` — a closed-enum tree, NOT a raw type-text string. Each variant maps 1:1 to a `SemanticQueryKey`:

| `StructuredTypeExpression` variant | Dispatch target |
|---|---|
| `Reference { scope_canonical, name, type_arguments, extra_imports }` | `SemanticQueryKey::ResolveDecl` (or `Instantiate` if `type_arguments` non-empty) |
| `Union { members }` | recurse on each; algebraic combination through `SemanticGraphStore` |
| `Intersection { members }` | same |
| `IndexedAccess { object, index }` | `SemanticQueryKey::ProjectPath` with single `Index` segment |
| `KeyOf { operand }` | recurse on operand; `TypeExpr::KeyOf` lowering |
| `TypeOf { value_root, path }` | `SemanticQueryKey::ResolveValueRoot` then path |
| `Tuple { elements }` | recurse |
| `Array { element, readonly }` | recurse |
| `ObjectLiteral { members, index_signatures }` | recurse; synthesise `TypeExpr::Object` |
| `Mapped { ... }` | recurse |
| `Conditional { check, extends, true_branch, false_branch }` | recurse |
| `Literal { value }` | leaf |
| `Primitive { kind }` | leaf |
| `TemplateLiteral { quasis, expressions }` | recurse |

`parse_type_annotation` is NEVER called inside this path. Producers (LSP hover, MCP tools, test fixtures) build the `StructuredTypeExpression` from their own AST walks. Test fixtures gain a `structured_type_expr!` builder macro for convenience.

Guard: `evaluate_type_expression_does_not_call_parse_type_annotation` is a static file-walk over the resolver / projector / registry / policy / materialiser / projection / compat / typeinfo crate paths that asserts no callsite mentions `parse_type_annotation` other than the dedicated JSDoc path at `crates/verter_session/src/jsdoc/parse_type.rs`.

---

## 6. Framework Surface Adapter Contract

### 6.1 Payload

```rust
// verter_protocol::typeinfo::framework
pub struct FrameworkSurfacePayload {
    pub kind: PayloadKind,
    pub framework_adapter_id: FrameworkAdapterId,     // R7-EXT: OPEN canonical id via FrameworkAdapterRegistry
    pub adapter: InternedName,
    pub component: ComponentIdentity,
    pub graph: SemanticTypeGraph,
    pub surfaces: FrameworkTypeSurfaces,
    pub diagnostics: Vec<TypeInfoDiagnostic>,
    pub projections: Option<FrameworkProjectionBundle>,
}

// R7-EXT: FrameworkAdapterId is an OPEN canonical interned id (registry-driven).
// FrameworkSurfaceKind stays CLOSED (Prop, Event, Slot, Model, Exposed, Ref, Children, Snippet, Export, AcceptedProp, AcceptedEvent).
// Adding a framework = registering an adapter; adding a surface kind = schema bump.
pub struct FrameworkAdapterId(pub InternedName);

pub struct FrameworkAdapterDescriptor {
    pub adapter_id: FrameworkAdapterId,
    pub display_name: InternedName,
    pub version: u32,
    pub supported_surfaces: BTreeSet<FrameworkSurfaceKind>,
}

impl FrameworkAdapterId {
    /// R7: canonical-form normalization rejects case aliases.
    /// Guard: framework_adapter_id_canonicalization_rejects_case_alias.
    pub fn try_new(raw: &str) -> Result<Self, RegistryError> { /* lowercases + validates charset; rejects "Vue" after "vue" */ unimplemented!() }
    pub fn canonicalize(raw: &str) -> Self { /* normalises to canonical form */ unimplemented!() }
}

pub struct FrameworkTypeSurfaces {
    pub props: Vec<FrameworkSurfaceMember>,
    pub events: Vec<FrameworkSurfaceMember>,
    pub slots: Vec<FrameworkSurfaceMember>,
    pub models: Vec<FrameworkSurfaceMember>,
    pub exposed: Vec<FrameworkSurfaceMember>,
    pub refs: Vec<FrameworkSurfaceMember>,
    pub children: Vec<FrameworkSurfaceMember>,
    pub snippets: Vec<FrameworkSurfaceMember>,
    pub exports: Vec<FrameworkSurfaceMember>,
    pub accepted_props: Vec<FrameworkSurfaceMember>,  // fallthrough
    pub accepted_events: Vec<FrameworkSurfaceMember>,
    pub framework_specific: BTreeMap<FrameworkSurfaceKind, Vec<FrameworkSurfaceMember>>,
    pub public_instance: Option<TypeNodeId>,          // Vue-specific carve-out
}

pub enum FrameworkSurfaceKind {                       // CLOSED
    Prop, Event, Slot, Model, Exposed, Ref, Children, Snippet, Export,
    AcceptedProp, AcceptedEvent,
    VuePublicInstance,
    SvelteSnippet,
    ReactRefForwarded,
}

pub struct FrameworkSurfaceMember {
    pub name: InternedName,
    pub kind: FrameworkSurfaceKind,
    pub type_ref: TypeNodeId,
    pub display_ref: Option<TypeNodeId>,
    pub authored_ref: Option<TypeNodeId>,
    pub required: Option<bool>,                       // mechanically copied from graph member.optional
    pub readonly: Option<bool>,                       // mechanically copied from graph member.readonly
    pub default: Option<DefaultMeta>,
    pub jsdoc: Option<JsdocMeta>,
    pub framework_meta: Option<FrameworkMemberMetaDto>,
    pub origin: Vec<OriginRef>,
    pub status: ExpansionStatus,
}
```

Guards: `framework_tag_enum_is_closed`, `framework_surface_member_enum_is_closed`, `framework_surface_member_does_not_override_optionality`.

### 6.2 Ownership Boundary

`required` and `readonly` on `FrameworkSurfaceMember` are MECHANICALLY copied from the underlying graph member's `optional` / `readonly` fields. The adapter MUST NOT override them. `accepted_props` / `accepted_events` are populated by `verter_session`'s inheritance resolver. Guard: `framework_adapter_does_not_recompute_fallthrough`.

### 6.3 Adapter Implementations

#### 6.3.1 Vue Adapter

| Macro | Surface |
|---|---|
| `defineProps<T>` | props from `T` |
| `withDefaults(defineProps<T>(), defaults)` | props + `default` metadata |
| `defineEmits<T>` | events from `T` payload |
| `defineSlots<T>` | slots from `T` |
| `defineModel<T>` | models + `update:foo` events |
| `defineExpose<T>` | exposed from `T` |
| Default-export `setup() { return { ... } }` | exposed from the return |

Guard: `vue_model_open_generic_surface_is_consistent`.

#### 6.3.2 Svelte Adapter (STOP-Gated — see §9.6)

Out of scope until parser + scheduler producers exist.

#### 6.3.3 React Adapter (STOP-Gated — see §9.7)

Out of scope. **Callback props (`on*`, `on{Name}`) are members of the props surface of `kind: Prop`.** Their value is a callable `TypeNode` (a `TypeNode::Object` with `call_signatures` or a function-typed `Reference`). The adapter MUST NOT re-classify them as `kind: Event` based on naming conventions or any heuristic. Event surfaces in React (if/when introduced) must originate from a structurally distinct surface root selected by the adapter, not from a name-based prop-to-event reclassification. Guard: `framework_adapter_does_not_reclassify_callback_props_as_events`.

---

## 7. TypeInfo Projections

Generic projections live under `@verter/typeinfo` as separate import paths. Each projection:

- Imports ONLY `@verter/typeinfo` DTOs.
- Reads semantic decisions from `TypeNode` / `SymbolNode` / `OriginEdge` only.
- Never inspects `rawType`, display strings, identifier suffixes, or path substrings.
- Returns a projection result with `sourceNodeIds`, `exactness`, `diagnostics`, `policy`, `version`.

Guards: `typeinfo_projection_imports_no_framework_adapters`, `typeinfo_projection_no_raw_or_display_string_semantics`.

### 7.1 Zod

Variant-by-variant emission table (from the prior draft; preserved verbatim with the following corrections):

- `Conditional { resolution: UnresolvedGeneric }` with `policy.kind = bind` → substitute bindings, dispatch `expandGraphAroundWithAudit` natively, retry.
- `Cycle { cycle_id }` → `z.lazy(() => ...)` with `cycle_id`-keyed memoisation per §3.9.

Guard: `zod_recursive_emits_lazy`.

### 7.2 JSON Schema

`$defs` for shared sub-trees, `$ref` for cycles, `additionalProperties: false` / `unevaluatedProperties: false` / `true` policy options.

### 7.3 Storybook Controls

Standard mapping per the prior draft.

### 7.4 Docs / Type Explorer

Typed view object exposing `display`, `origin`, `exactness`, `blockers`, `conditionalBranches`, `substitutionPlayground`.

### 7.5 Legacy `TypeDescriptor`

`@verter/typeinfo/projections/type-descriptor` mechanically derives a `TypeDescriptor` from a `TypeInfoGraphPayload`. `@verter/type-ir` keeps the DTO definition; the projection logic lives in `@verter/typeinfo`.

### 7.6 Display Projection

```ts
export interface DisplayPolicy {
  preserveAliasIdentity: boolean
  expandIndexedAccess: "always" | "never" | "ifPathPrecise"
  conditionalBranchDisplay: "selected" | "both" | "symbolic"
  truncateUnion: number | null
}
```

Display is a text projection. It does NOT inform any other projection.

### 7.7 SharedLoadReuse Edges Are Audit-Terminal For Projections

`OriginEdgeKind::SharedLoadReuse` is the wire/audit-side edge emitted when a joiner attaches to a winner's in-flight artifact via scheduler dedup (§2.15). It is an **audit-only** edge — it records the dedup event for observability, not a semantic derivation step. Every projection — Zod, JSON Schema, Storybook controls, docs, display, legacy TypeDescriptor, and any future projection that lands in `packages/typeinfo/src/projections/*` — MUST skip `SharedLoadReuse` edges when walking the derivation chain. Treating `SharedLoadReuse` as a semantic derivation step is a defect (it would attribute the join event as if it changed type meaning).

Concrete rule: every projection's edge handler maintains a `SKIP_KINDS` set containing at least `OriginEdgeKind::SharedLoadReuse`. The integration test `projections_treat_shared_load_reuse_as_audit_terminal` (in `packages/typeinfo/src/__tests__/projection_origin_handling.test.ts`) enumerates every exported projection in `@verter/typeinfo/projections/*` and asserts each one's edge walker either declares `SharedLoadReuse` in its skip-list OR never touches `OriginEdge` (display projection, for instance, may not need provenance traversal at all). Guard: `projections_treat_shared_load_reuse_as_audit_terminal`.

---

## 8. Phase Plan

Each phase has: changes, legacy deletions, documentation updates, verification, named architecture guards. Documentation updates land IN THE SAME PHASE as the implementation they describe; §12 is verification only, not first-time documentation.

### 8.0 CRITICAL_RULE_GUARDS Registry Plan

Every new `(CRITICAL)` section introduced by this plan in CLAUDE.md or `.claude/skills/*/SKILL.md` is registered in `crates/verter_session/tests/critical_rules_have_guards.rs::CRITICAL_RULE_GUARDS` as part of the phase that introduces the rule. The meta-guard `every_critical_rule_in_docs_has_registered_guard` walks both files and asserts every CRITICAL section has at least one registered guard name; the inverse meta-guard `every_critical_rule_guard_entry_still_exists` asserts every registered name still resolves to a real `#[test]` function or integration-test filename.

Registry entries added by this plan (phase that adds them in parens):

| Title (CRITICAL section heading) | Guard names | Phase |
|---|---|---|
| `The Five Query Modes Are Explicit` | `path_projection_mode_cascade`, `typeinfo_request_validates_mode_present` | 0a |
| `Five-Way Env Hash Split (R21)` | `r21_no_bundled_config_hash`, `r21_lib_env_hash_inclusion` | 0a |
| `Two Cache Families` | `cache_key_no_value_fields`, `multi_candidate_distinguishing_dimension_is_value_only` | 0a |
| `Publication Fence (CompletionFence)` | `publication_fence_revalidates_before_publish`, `typeinfo_graph_publication_fence_3_retries`, `typeinfo_graph_unstable_returns_typed_error`, `completion_fence_uses_max_inflight_retries_constant`, `typeinfo_warm_hit_emits_no_structured_payload`, `typeinfo_graph_degraded_emits_structured_event` | 0b/1 |
| `Warm-Cache Exactness Contract` | `typeinfo_graph_no_partial_admission`, `degraded_payload_never_warm_admitted`, `degraded_store_never_serves_complete_admission` | 0b/1 |
| `Arc-Published Immutable Payloads` | `typeinfo_graph_warm_hit_zero_alloc` | 0b/1 |
| `Fact-Signature Validation` | `typeinfo_graph_warm_hit_revalidates_self_root`, `typeinfo_graph_warm_hit_zero_alloc` | 0b/1 |
| `IndexedReady Authority` | `graph_export_reads_only_from_indexed_ready` | 0a |
| `Typed-IR-Only Resolver Rule (extended to typeinfo)` | `typeinfo_projection_no_raw_or_display_string_semantics`, `evaluate_type_expression_does_not_call_parse_type_annotation` | 0a |
| `No Role Inference From Name Suffix` | `no_role_inference_from_suffix` (already extant; extended scope) | 0a |
| `SymbolSpace Has Three Variants` | `symbol_node_preserves_type_value_namespace_spaces`, `class_dual_space_emits_two_symbols` | 0a |
| `Relation Engine Is Public` | `typeinfo_exposes_relate_query` | 1 |
| `Origin-Edge Taxonomy Is Normative` | `origin_edge_taxonomy_locked` | 0a |
| `Symbol Identity Is Slot-Based` | `symbol_node_preserves_resolved_decl_slot_identity` | 0a |
| `Declaration Merging, Module Augmentation, Ambient State Are First-Class Graph State` | `merged_declarations_are_public_graph_state`, `overload_sets_are_public_graph_state`, `module_augmentation_is_public_graph_state`, `ambient_namespaces_are_public_graph_state`, `exporter_dispatches_resolve_merged_declaration_query` | 1 |
| `Framework Adapter Boundary` | `framework_surface_member_enum_is_closed`, `framework_tag_enum_is_closed`, `framework_surface_member_does_not_override_optionality`, `framework_adapter_does_not_recompute_fallthrough`, `framework_adapter_does_not_reclassify_callback_props_as_events` | 0a + 5 |
| `Progressive Expansion Is A Semantic Query` | `progressive_graph_expansion_dispatches_semantic_query_key`, `progressive_expansion_routes_through_typeinfo_graph_db` | 3 |
| `No Heuristic Cache Semantics (R30 / R31)` | `cache_identity_has_no_heuristic_dimensions` | 0a |
| `Closed-Enum Discipline (Wire-Compat Policy)` | `no_custom_string_escape_in_typeinfo_dtos`, `decoder_returns_typed_error_on_unknown_variant` | 0a + 2 |
| `Closed-Enum Fallback Reasons` | `degraded_results_use_closed_enum_reasons` | 0b/1 |
| `SDK Audit Test For Intrinsics` | `sdk_audit_unsupported_intrinsic` (existing, extended scope) | 1 |
| `AuditedResult Carrier` (§3.0.1) | `audited_result_carries_record_on_error` | 0a |
| `StructuredTypeExpression DTO Is Closed` (§3.0 / §5.6) | `structured_type_expression_dto_is_closed`, `proto_field_names_avoid_rust_keywords` | 0a |
| `SemanticQueryKey Env Composition Is Uniform` (§2.17) | `new_semantic_query_keys_uniform_env_composition` | 0a |
| `Request DTO Uniformity` (§5.2) | `every_typeinfo_request_carries_context_or_is_exempted_with_rationale`, `list_symbols_is_scalar`, `relate_has_no_closure_field`, `every_closure_variant_has_concrete_resource_bound` | 0a |
| `SharedLoadReuse Is Audit-Terminal For Projections` (§7.7) | `projections_treat_shared_load_reuse_as_audit_terminal` | 0a + 4 |
| `Closed-Enum Discipline (Wire-Compat Policy)` (extended) | `typeinfo_session_handshake_emits_supported_versions`, `typeinfo_session_rejects_unknown_schema_version_at_handshake` | 0a + 0b/1 |
| `BudgetExceededFailure Lowering Uses Real Fields` (§3.8) | `budget_exceeded_dto_uses_real_failure_fields`, `budget_exceeded_failure_maps_all_domains` | 0a + 0b/1 |
| `Substitution Symbolic Tag Canonicalisation` (§4.1.1) | `substitution_symbolic_tags_canonicalised` | 0b/1 |
| `Cross-File MergedDeclaration Completeness` (§9.5) | `merged_declarations_carry_contributor_identity`, `parts_len_is_not_sole_assertion` | 0b/1 |
| `Ignored-Test Lift Schedule Has Mechanical Backing` (§8.x) | `typeinfo_tests_unignore_manifest_complete`, `typeinfo_tests_unignored_count_by_phase`, `every_typeinfo_test_ignore_has_named_reason` | 0a (manifest) + per-phase thresholds |

Phase 0a fails CI if any new CRITICAL section is missing its registry entry — the meta-guard catches it before any later phase ships.

### 8.1 Phase 0a — Schema Scaffold

**No public typeinfo function is introduced with `todo!()`, `unimplemented!()`, panic stubs, empty bodies, `assert!(true)`, or always-`Opaque`/`Unknown` behavior. Phase 0a adds only executable shape guards, DTO/proto schema definitions, request-validation paths that return `TypeInfoRequestError` before semantic execution, and `CRITICAL_RULE_GUARDS` registry entries.**

**Changes:**

1. Extend the existing protobuf build pipeline so the new typeinfo graph wire DTOs compile alongside the existing `component_meta.proto` / `selective_component_meta.proto` / `typeinfo.proto` sources. The wire surface is protobuf-authoritative; no `ts-rs` derive is added to `verter_protocol`. TypeScript bindings are produced from the same `.proto` files under `packages/proto/src/gen/verter/v1/` through the existing protobuf-ts pipeline (identical to how `component_meta_pb.ts` is generated today).
2. Extend `proto/verter/v1/typeinfo.proto` with the graph wire contracts per §3.0 (new envelope, all `TypeNode` variants in the `oneof`, `StructuredTypeExpression`, reserved field numbers for the deleted `EvaluateTypeExpressionRequestDto` fields 1-5). Preserve the existing import/symbol DTOs (`ImportSpecDto`, `NamedImportDto`, `DefaultImportDto`, `NamedBindingDto`, `NamespaceImportDto`, `SymbolEntryDto`, `SymbolEntryListDto`) re-homed alongside the new schema so current NAPI/WASM callers do not break.
3. Add the typed Rust DTO surface in `crates/verter_protocol/src/typeinfo/` (`graph.rs`, `structured_expression.rs`, `framework.rs`, `request_error.rs`, `audited_result.rs`). The Rust API re-exports the generated `prost` types and adds typed convenience constructors where the proto's structural shape is unergonomic. No `#[ts(export, ...)]` derives appear on these wire DTOs.
4. Add `crates/verter_protocol/src/typeinfo/framework.rs` — `FrameworkSurfacePayload`, closed `FrameworkTag`, closed `FrameworkSurfaceKind`, `FrameworkSurfaceMember`.
5. Add `crates/verter_protocol/src/typeinfo/structured_expression.rs` — `StructuredTypeExpression` Rust DTO.
6. Add `crates/verter_protocol/src/typeinfo/request_error.rs` — `TypeInfoRequestError` typed enum.
7. Add `crates/verter_audit/src/record.rs` `RequestKind::TypeInfoGraph` + `RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayloadAudit)`.
8. Add `crates/verter_audit/src/config.rs` `KindBit::TypeInfoGraph` for `AuditConsumerFilter`.
9. Add `crates/verter_audit/src/structured_event.rs` `StructuredAuditEvent::TypeInfoGraphPublished { layer, audit }`.
10. Add request validation entrypoints in `crates/verter_session/src/typeinfo/request_validation.rs` — every public typeinfo function has a request-validation path that returns `TypeInfoRequestError` BEFORE any semantic execution. The function shapes are `pub fn validate_resolve_symbol_graph_request(r: &ResolveSymbolGraphRequest) -> Result<(), TypeInfoRequestError>` (and similar per request kind). These are CALLABLE and execute real validation logic; they are not stubs.
11. Add `CRITICAL_RULE_GUARDS` registry entries per §8.0 for every Phase 0a-scope rule.
12. Add `crates/verter_session/src/typeinfo/typeinfo_tests_unignore_plan.md` — the complete unignore manifest. Every `#[ignore]`'d test under `crates/verter_session/src/typeinfo/typeinfo_tests/**` is listed with `(file, fn_name, target_phase, unblocked_by)` columns. The manifest is the input to the `typeinfo_tests_unignore_manifest_complete` + `typeinfo_tests_unignored_count_by_phase` guards.
13. Add the architecture guards listed below — every guard body is non-trivial, every guard is discriminating (FAILS on pre-Phase-0a tree, PASSES on post-Phase-0a tree).

**Architecture guards added in Phase 0a:**

| Guard | File |
|---|---|
| `dependency_direction_one_way` | `crates/verter_session/tests/architecture_guards.rs` |
| `node_taxonomy_complete` | `crates/verter_session/tests/typeinfo_graph_taxonomy.rs` |
| `origin_edge_taxonomy_locked` | `crates/verter_session/tests/typeinfo_graph_taxonomy.rs` |
| `framework_surface_member_enum_is_closed` | `crates/verter_session/tests/architecture_guards.rs` |
| `framework_tag_enum_is_closed` | `crates/verter_session/tests/architecture_guards.rs` |
| `framework_surface_member_does_not_override_optionality` | `crates/verter_session/tests/architecture_guards.rs` |
| `framework_adapter_does_not_recompute_fallthrough` | `crates/verter_session/tests/architecture_guards.rs` |
| `framework_adapter_does_not_reclassify_callback_props_as_events` | `crates/verter_session/tests/architecture_guards.rs` |
| `r21_no_bundled_config_hash` | `crates/verter_session/tests/cache_key_invariants.rs` |
| `r21_lib_env_hash_inclusion` | `crates/verter_session/tests/cache_key_invariants.rs` |
| `cache_key_no_value_fields` | `crates/verter_session/tests/cache_key_invariants.rs` |
| `multi_candidate_distinguishing_dimension_is_value_only` | `crates/verter_session/tests/cache_key_invariants.rs` |
| `cache_identity_has_no_heuristic_dimensions` | `crates/verter_session/tests/architecture_guards.rs` |
| `typeinfo_request_validates_mode_present` | `crates/verter_session/tests/typeinfo_request_validation.rs` |
| `typeinfo_request_validates_display_policy_present` | `crates/verter_session/tests/typeinfo_request_validation.rs` |
| `typeinfo_request_validates_closure_policy_present` | `crates/verter_session/tests/typeinfo_request_validation.rs` |
| `typeinfo_decoder_is_pure_mechanical` | `packages/typeinfo/src/__tests__/decoder.test.ts` |
| `typeinfo_projection_no_raw_or_display_string_semantics` | `crates/verter_session/tests/architecture_guards.rs` + `packages/typeinfo/src/__tests__/projection_no_text_semantics.test.ts` |
| `typeinfo_projection_imports_no_framework_adapters` | `packages/typeinfo/src/__tests__/architecture_guards.test.ts` |
| `no_role_inference_from_suffix` | existing — extended scope |
| `evaluate_type_expression_does_not_call_parse_type_annotation` | `crates/verter_session/tests/architecture_guards.rs` |
| `no_custom_string_escape_in_typeinfo_dtos` | `crates/verter_session/tests/architecture_guards.rs` |
| `symbol_node_preserves_type_value_namespace_spaces` | `crates/verter_session/tests/symbol_node_invariants.rs` |
| `symbol_node_preserves_resolved_decl_slot_identity` | `crates/verter_session/tests/symbol_node_invariants.rs` |
| `graph_export_reads_only_from_indexed_ready` | `crates/verter_session/tests/architecture_guards.rs` |
| `infer_only_inside_conditional_extends` | `crates/verter_session/tests/typeinfo_graph_taxonomy.rs` |
| `interned_name_wire_format_uses_string_table` | `crates/verter_session/tests/architecture_guards.rs` |
| `audited_result_carries_record_on_error` | `crates/verter_session/tests/typeinfo_audited_result.rs` |
| `structured_type_expression_dto_is_closed` | `crates/verter_session/tests/architecture_guards.rs` |
| `proto_field_names_avoid_rust_keywords` | `crates/verter_session/tests/architecture_guards.rs` |
| `new_semantic_query_keys_uniform_env_composition` | `crates/verter_session/tests/architecture_guards.rs` |
| `every_typeinfo_request_carries_context_or_is_exempted_with_rationale` | `crates/verter_session/tests/architecture_guards.rs` |
| `list_symbols_is_scalar` | `crates/verter_session/tests/architecture_guards.rs` |
| `relate_has_no_closure_field` | `crates/verter_session/tests/architecture_guards.rs` |
| `every_closure_variant_has_concrete_resource_bound` | `crates/verter_session/tests/typeinfo_request_validation.rs` |
| `substitution_symbolic_tags_canonicalised` | `crates/verter_session/tests/typeinfo_graph_substitutions.rs` |
| `typeinfo_warm_hit_emits_no_structured_payload` | `crates/verter_session/tests/typeinfo_graph_warm_hit_audit.rs` |
| `typeinfo_tests_unignore_manifest_complete` | `crates/verter_session/tests/typeinfo_tests_unignore_manifest.rs` |
| `typeinfo_tests_unignored_count_by_phase` | `crates/verter_session/tests/typeinfo_tests_unignore_manifest.rs` |
| `typeinfo_session_handshake_emits_supported_versions` | `crates/verter_session/tests/typeinfo_session_handshake.rs` |
| `completion_fence_uses_max_inflight_retries_constant` | `crates/verter_session/tests/typeinfo_graph_publication_fence.rs` (Phase 0b/1 — gated until the file lands) |
| `projections_treat_shared_load_reuse_as_audit_terminal` | `packages/typeinfo/src/__tests__/projection_origin_handling.test.ts` |
| `parts_len_is_not_sole_assertion` | `crates/verter_session/tests/architecture_guards.rs` |
| `budget_exceeded_dto_uses_real_failure_fields` | `crates/verter_session/tests/architecture_guards.rs` |
| `budget_exceeded_failure_maps_all_domains` | `crates/verter_session/tests/typeinfo_graph_degraded_reasons.rs` |

Specifically:

- `node_taxonomy_complete` enumerates every Rust `TypeNode` variant, every proto `TypeNode.kind` arm, and every TS DTO discriminant; asserts the three sets are pairwise equal.
- `origin_edge_taxonomy_locked` asserts `verter_session::OriginEdgeKind` (9 variants) ⊂ `verter_audit::OriginEdgeKind` (10 variants; +`SharedLoadReuse`) ⊂ `verter_protocol::typeinfo::graph::OriginEdgeKind` (10 variants).
- `evaluate_type_expression_does_not_call_parse_type_annotation` is a static file walk over `crates/verter_session/src/typeinfo/**`, `crates/verter_session/src/component_meta_*.rs`, `crates/verter_session/src/host_resolve/**`, `crates/verter_session/src/resolver_core/**`, `crates/verter_session/src/typeinfo/projections/**`, `packages/typeinfo/**`, `packages/component-meta/**` — asserts zero references to `parse_type_annotation` outside the JSDoc path.
- `typeinfo_decoder_is_pure_mechanical` is a TS-side file walk asserting the decoder source contains zero references to text-bearing semantic helpers.

**Legacy deletions:** none yet — pure scaffolding.

**Documentation updates (same phase):**

- `.claude/skills/audit-infrastructure/SKILL.md` — add `RequestKind::TypeInfoGraph`, `RequestKindPayload::TypeInfoGraph`, `KindBit::TypeInfoGraph`, `StructuredAuditEvent::TypeInfoGraphPublished`.
- `.claude/skills/type-cache-architecture/SKILL.md` — add the `TypeInfoGraphResultDb` row (shape only at this phase; bodies in 0b/1).
- `.claude/skills/type-resolution/SKILL.md` — add the new `SemanticQueryKey` variant names (with shape only; dispatch comes in Phase 1).
- `CLAUDE.md` — add a "TypeInfo Graph Is The Foundation" CRITICAL section that lists the §2 rules introduced in 0a.

**Verification:**

```bash
cargo test --workspace --tests --verbose
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
pnpm vitest --run packages/typeinfo packages/component-meta
```

Expected: every guard listed above passes; pre-existing tests unchanged; `cargo test architecture_guards` shows ≥27 new tests passing.

### 8.2 Phase 0b/1 — Native Graph Exporter + Real Bodies

**Changes:**

1. Add `SemanticQueryKey` variants per §2.17 (`ResolveMergedDeclaration`, `ResolveModuleAugmentation`, `ResolveAmbientNamespace`, `ResolveOverloadSet`, `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt`). Each variant routes through `ProjectSemanticDispatch::execute` (existing memo at `crates/verter_session/src/semantic_query_memo`).
2. Extend `SemanticNodeData` with the corresponding payload variants: `MergedDeclaration`, `ModuleAugmentation`, `AmbientNamespace`, `Class`, `Enum`. Producer logic for each lives in `verter_semantic::analysis` (extended namespace/merge/class analysis) and `verter_session::semantic_query`.
3. Implement `TypeInfoGraphExporter` at `crates/verter_session/src/typeinfo/exporter.rs`. The exporter is a PURE LOWERING PASS:
   - Dispatch root semantic queries through `ProjectSemanticDispatch::execute`.
   - Snapshot reachable `SemanticNodeData` to `TypeNode` (lowering table per §3.2).
   - Build `node_id_map` / `symbol_id_map` / `signatures` arena / `strings` table.
   - Capture `read_set` via `with_fact_tracer`.
   - Capture `self_root_canonicals` (root canonical + every observed cross-file dependency).
   - Stamp `exactness` per snapshot node.
   - Serialise into the prost wire format.

   The exporter does NOT recover declaration-merge / augmentation / namespace structure from `IndexedReady` directly — it consults the `SemanticQueryKey` variants above. Guard: `exporter_dispatches_resolve_merged_declaration_query`.

4. Implement `TypeInfoGraphResultDb` (cold path + cooperative admission per §4.3, warm hit + revalidation per §4.1.1).
5. Implement `DegradedResultStore`.
6. Wire `CompletionFence::publish_with_retry` (the helper used elsewhere in the session; rename within this phase if its current name differs — see §0.6) around `TypeInfoGraphResultDb` admission.
7. Implement the lossless `QueryError → QueryErrorDto` lowering per §3.8. The lowering is a single `From<QueryError> for QueryErrorDto` impl with a discriminating round-trip test.
8. Extend `verter_audit`:
   - Populate `RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayloadAudit)` from the exporter.
   - Emit `StructuredAuditEvent::TypeInfoGraphPublished { layer, audit }` on cold publish.
   - Extend `BatchAuditAggregator` (`crates/verter_audit/src/batch.rs`) row counter for `TypeInfoGraph`.
   - Extend `crates/verter_audit/src/record.rs` typed accessor `as_typeinfo_graph()`.
   - Extend `crates/verter_session/src/component_meta_audit/footprint_miner.rs` so `TypeInfoGraph` records contribute footprint observations.
9. Add the behavioral architecture guards listed below — every guard body is discriminating against the real exporter bodies landed in this phase. No `todo!()` is reachable.

**Behavioral architecture guards added in Phase 0b/1:**

| Guard | File |
|---|---|
| `typeinfo_graph_warm_hit_zero_alloc` | `crates/verter_session/tests/canary_warm_hit_zero_alloc.rs` |
| `typeinfo_graph_warm_hit_revalidates_self_root` | `crates/verter_session/tests/typeinfo_graph_self_root.rs` |
| `typeinfo_graph_no_partial_admission` | `crates/verter_session/tests/typeinfo_graph_no_partial_admission.rs` |
| `degraded_payload_never_warm_admitted` | `crates/verter_session/tests/typeinfo_graph_no_partial_admission.rs` |
| `degraded_store_never_serves_complete_admission` | `crates/verter_session/tests/typeinfo_graph_no_partial_admission.rs` |
| `degraded_results_use_closed_enum_reasons` | `crates/verter_session/tests/typeinfo_graph_degraded_reasons.rs` |
| `publication_fence_revalidates_before_publish` | `crates/verter_session/tests/typeinfo_graph_publication_fence.rs` |
| `typeinfo_graph_publication_fence_3_retries` | `crates/verter_session/tests/typeinfo_graph_publication_fence.rs` |
| `typeinfo_graph_concurrent_cold_collapses_one_exporter_run` | `crates/verter_session/tests/typeinfo_graph_concurrent.rs` |
| `popover_slot_props_unresolved_keeps_both_branches` | `crates/verter_session/tests/typeinfo_graph_fixtures/popover_slot_props_unbound.rs` |
| `popover_slot_props_hover_selects_true_only` | `crates/verter_session/tests/typeinfo_graph_fixtures/popover_slot_props_hover.rs` |
| `popover_slot_props_hover_and_click_are_distinct_cache_candidates` | `crates/verter_session/tests/typeinfo_graph_substitutions.rs` |
| `queryerror_dto_is_lossless` | `crates/verter_session/tests/queryerror_dto_lossless.rs` |
| `cycle_id_stable_across_queries` | `crates/verter_session/tests/typeinfo_graph_cycle_id.rs` |
| `variance_annotations_lowered_through_oxc` | `crates/verter_semantic/tests/variance_lowering.rs` |
| `algebraic_normalisation_rules` | `crates/verter_session/tests/typeinfo_graph_algebraic.rs` |
| `conditional_distributive_flag_matches_tuple_wrap` | `crates/verter_session/tests/typeinfo_graph_conditionals.rs` |
| `type_parameter_constraint_preserves_symbol_ref` | `crates/verter_session/tests/typeinfo_graph_generics.rs` |
| `signature_last_overload_return_type` | `crates/verter_session/tests/typeinfo_graph_overloads.rs` |
| `class_dual_space_emits_two_symbols` | `crates/verter_session/tests/symbol_node_invariants.rs` |
| `merged_declarations_are_public_graph_state` | `crates/verter_session/tests/typeinfo_graph_merging.rs` |
| `overload_sets_are_public_graph_state` | `crates/verter_session/tests/typeinfo_graph_overloads.rs` |
| `module_augmentation_is_public_graph_state` | `crates/verter_session/tests/typeinfo_graph_augmentation.rs` |
| `ambient_namespaces_are_public_graph_state` | `crates/verter_session/tests/typeinfo_graph_ambient.rs` |
| `exporter_dispatches_resolve_merged_declaration_query` | `crates/verter_session/tests/typeinfo_graph_merging.rs` |
| `typeinfo_exposes_relate_query` | `crates/verter_session/tests/typeinfo_graph_relate.rs` |
| `flow_narrowing_only_when_projection_required` | `crates/verter_session/tests/typeinfo_graph_flow.rs` |
| `decoder_returns_typed_error_on_unknown_variant` | `packages/typeinfo/src/__tests__/decoder.test.ts` |
| `typeinfo_graph_unstable_returns_typed_error` | `crates/verter_session/tests/typeinfo_graph_publication_fence.rs` |
| `typeinfo_graph_degraded_emits_structured_event` | `crates/verter_session/tests/typeinfo_graph_audit_branches.rs` |
| `typeinfo_session_rejects_unknown_schema_version_at_handshake` | `crates/verter_session/tests/typeinfo_session_handshake.rs` |
| `merged_declarations_carry_contributor_identity` | `crates/verter_session/tests/typeinfo_graph_merging.rs` |
| `completion_fence_uses_max_inflight_retries_constant` | `crates/verter_session/tests/typeinfo_graph_publication_fence.rs` |
| Phase 0b/1 `typeinfo_tests_unignored_count_by_phase` threshold | the existing guard checks `lifted ≥ N1` at this phase boundary |

**Legacy deletions:**

1. Delete `crates/verter_protocol/src/graph/builder.rs::GraphNode` and the surrounding `GraphBuilder` / `GraphConditionalFrame` / `GraphTupleElement` / `GraphObjectMember` / `GraphFunctionParam` types — they are replaced wholesale by the new `verter_protocol::typeinfo::graph::TypeNode`. Every call site in `crates/verter_protocol/src/component_meta.rs` (the lowering pass for component metadata) is migrated to consume the new `TypeNode` taxonomy in the same diff. The component-meta payload's `TypeGraph` proto message keeps its existing identity; only its node-shape lowering is rewritten.
2. Delete `crates/verter_protocol/src/graph/schema/*` legacy proto re-exports referenced from `crates/verter_protocol/src/graph/mod.rs:7`.
3. Delete the legacy `crates/verter_protocol/proto/verter/v1/typeinfo.proto` 94-line schema — replaced by §3.0. Field numbers 1–5 of `EvaluateTypeExpressionRequestDto` are reserved permanently.
4. Delete `crates/verter_session/src/typeinfo/{evaluate_type_expression.rs, raise.rs, resolve_named_symbol.rs, scratch_cache.rs, symbol_inventory.rs, types.rs, tests.rs}` legacy stubs that the prior text-based evaluator carried. The `typeinfo_tests/` directory is preserved (lifted per §8.x).

**Documentation updates (same phase):**

- `.claude/skills/type-resolution/SKILL.md` — add the new `SemanticQueryKey` variants with full dispatch contract.
- `.claude/skills/type-cache-architecture/SKILL.md` — add `TypeInfoGraphResultDb` and `DegradedResultStore` to the canonical store table; update the per-cache-layer key composition.
- `.claude/skills/component-meta/SKILL.md` — note that `MergedDeclaration` / `ModuleAugmentation` / `AmbientNamespace` are first-class graph state.
- `.claude/skills/audit-infrastructure/SKILL.md` — document `TypeInfoGraphPayloadAudit` field semantics + footprint-miner extension.
- `docs/arch/typeinfo-graph.md` — first version, the SemanticTypeGraph contract.
- `docs/arch/fact-based-cache.md` — extended with the `TypeInfoGraphResultDb` row.

**Verification:**

```bash
cargo test --package verter_session --test typeinfo_graph_exporter --verbose
cargo test --workspace --tests --verbose
cargo test --package verter_audit --tests
node scripts/gen-corpus-audit-tests.mjs
```

Fixtures in `crates/verter_session/tests/typeinfo_graph_fixtures/` cover: `popover_slot_props_unbound.rs`, `popover_slot_props_hover.rs`, `theme_alias_display.rs`, `editor_drag_handle_indexed_access.rs`, `content_search_intersection.rs`, `merged_interfaces.rs`, `module_augmentation.rs`, `function_overload_set.rs`, `class_with_this_type.rs`, `mapped_with_modifiers.rs`, `mapped_with_remap.rs`, `template_literal_infer.rs`, `infer_extends.rs`, `variance_annotations.rs`, `const_type_param.rs`, `no_infer.rs`, `type_predicate.rs`, `assertion_function.rs`, `unique_symbol.rs`, `satisfies_value.rs`, `flow_narrowing_typeof.rs`, `recursive_object_member.rs`, `package_backed_alias.rs`, `enum_first_class.rs`.

### 8.3 Phase 2 — FFI Surface And TypeScript Decoder

**Changes:**

1. Add NAPI bindings in `crates/verter_napi/src/typeinfo.rs` — one per `_with_audit` method. Payloads cross as binary protobuf `Buffer`. The legacy NAPI typeinfo entries identified during the §0.5 survey are deleted in this phase.
2. Add WASM bindings in `crates/verter_wasm/src/typeinfo.rs` — `Uint8Array` parallel.
3. Add TypeScript decoder at `packages/typeinfo/src/decode.ts` — pure mechanical proto-to-DTO walker.
4. Add `packages/typeinfo/src/session.ts` `TypeInfoSession` implementation: wraps the native bindings, decodes the wire format, returns typed `TypeInfoGraphPayload`. The session API is the §5.2 contract — every request type has `context` + `displayPolicy` + `closure` as REQUIRED fields.
5. Add `packages/types/audit.generated.ts` regenerated via `ts_rs` — adds `RequestKindPayload::TypeInfoGraph`, `KindBit::TypeInfoGraph`, `StructuredAuditEvent::TypeInfoGraphPublished`.
6. Extend the existing `crates/verter_session/tests/architecture_guards.rs::wave_3_entry_points_propagate_tls` function (line 7641) with typeinfo entry-point drivers verifying TLS observer propagation. (No new file — the function is inside `architecture_guards.rs`.)

**Legacy deletions:**

1. Delete `packages/component-meta/src/type-graph.ts`, `type-graph-core.ts`, `type-graph-decode.ts`, `type-graph-proto-decode.ts`, `type-graph.test-utils.ts`, `type-expr-bridge.ts`. Their last consumers are migrated in the same diff.
2. Delete `packages/typeinfo/src/{session.ts (legacy version), types.ts, descriptor-to-native.ts, native-to-descriptor.ts, native-type-expr.ts}` legacy files identified during §0.5 survey. The new `session.ts` (this phase) is a fresh file at the same path.
3. Delete legacy NAPI/WASM typeinfo entries listed in §0.5.

**Documentation updates (same phase):**

- `.claude/skills/audit-infrastructure/SKILL.md` — TLS propagation note for typeinfo entry-points.
- `.claude/skills/testing/SKILL.md` — note new TS decoder test pattern (binary protobuf round-trip).
- `docs/contributing/typeinfo-cutover-deletions.md` — first version with the file list.

**Verification:**

```bash
pnpm vitest --run packages/typeinfo packages/types
cargo test --package verter_napi --tests
cargo test --package verter_wasm --tests
cargo test --workspace --tests --verbose
```

Expected: wire round-trip tests pass for every node kind; TS decoder rejects malformed payloads with `TypeInfoRequestError::UnknownSchemaVersion` / `MalformedPayload`; audit record carries `RequestKindPayload::TypeInfoGraph` for every entry-point; TLS observer propagation test passes.

### 8.4 Phase 3 — Public TypeInfo Session API And Compat Migration

**Changes:**

1. Wire every `TypeInfoSession` method through the NAPI / WASM bindings.
2. Migrate every legacy consumer of `resolveSymbol(...)` / `evaluateTypeExpression(...)` to the new graph API + `toTypeDescriptor(payload)` projection. Legacy entry-point function names are DELETED in this phase (no permanent wrappers).
3. Add native-side audit integration: `TypeInfoSession` opens a `RequestAuditRecord` per call, populates `RequestKindPayload::TypeInfoGraph`, emits `StructuredAuditEvent::TypeInfoGraphPublished` on cold publish, threads the audit handle through every internal subquery. `expandGraphAround` uses nested-record semantics — the parent record opens; the inner `expandGraphAround` invocation records as a child entry under the same outer audit envelope.
4. Add typeinfo-side graph helpers (TS):
   - `getNode(payload, id)` — pure local lookup.
   - `getSymbol(payload, id)` — pure local lookup.
   - `resolveAliasInstantiation(payload, id)` — pure local walk.
   - `substitute(payload, typeRef, bindings)` — **native call** via `expandGraphAroundWithAudit`.
   - `evaluateConditional(payload, typeRef, bindings)` — **native call**.
   - `projectPath(payload, typeRef, path)` — **native call** via `projectPathGraphWithAudit`.
   - `collectDependencies(payload, typeRef)` — pure local BFS over `edges`.
   - `explain(payload, typeRef)` — pure local origin-edge walk.
5. Add `@verter/typeinfo/src/__tests__/api_contract.test.ts` integration suite.

**Behavioral guards added in Phase 3:**

| Guard | File |
|---|---|
| `typeinfo_helpers_substitution_routes_native` | `packages/typeinfo/src/__tests__/api_contract.test.ts` |
| `progressive_graph_expansion_dispatches_semantic_query_key` | `crates/verter_session/tests/typeinfo_graph_progressive.rs` |
| `progressive_expansion_routes_through_typeinfo_graph_db` | `crates/verter_session/tests/typeinfo_graph_progressive.rs` |
| `audit_record_carries_typeinfo_graph_kind` | `crates/verter_session/tests/architecture_guards.rs::wave_3_entry_points_propagate_tls` |
| `expand_graph_around_records_nested_audit_entry` | `crates/verter_session/tests/typeinfo_graph_audit_nested.rs` |

**Legacy deletions:**

1. Delete every `resolveSymbol(...)` / `evaluateTypeExpression(...)` consumer migration's prior entry-point sites by name. No permanent wrappers.

(`packages/typeinfo/src/descriptor-to-native.ts` / `native-to-descriptor.ts` are deleted in Phase 2 per §0.5.5 — they do NOT carry into Phase 3. The same-phase deletion rule forbids conditional survival language across phase boundaries; if a file was scheduled for Phase 2 deletion and the Phase 2 implementer left it behind, that is a Phase 2 implementation failure, not a Phase 3 cleanup step.)

**Documentation updates (same phase):**

- `.claude/skills/type-resolution/SKILL.md` — add `TypeInfoSession` public surface.
- `.claude/skills/component-meta/SKILL.md` — note that compat consumers route through `toTypeDescriptor(payload)`.

**Verification:**

```bash
pnpm vitest --run packages/typeinfo
cargo test --workspace --tests --verbose
```

### 8.5 Phase 4 — Typeinfo Projections

**Changes:**

1. Implement projections in order: Display → TypeDescriptor → JSON Schema → Zod → Storybook Controls → Docs.
2. Each projection lives at `packages/typeinfo/src/projections/<name>/index.ts` with `__tests__/` sibling.
3. Mechanical mapping tables per §7. No semantic recovery; no display-string parsing.
4. Cycle handling uses `cycle_id` matching per §3.9.

**Legacy deletions:**

1. Delete `@verter/component-meta/src/compat/` semantic recovery hooks (text parsers, `looksLike*`, `extract*`). Replaced by typeinfo projection calls. Every file listed in the `typeinfo_projection_no_raw_or_display_string_semantics` guard's pre-Phase-0a scan must now be absent.
2. Delete `packages/type-ir/src/parse/` if a type-annotation reparser is present.

**Documentation updates (same phase):**

- `.claude/skills/component-meta/SKILL.md` — note projection-only compat layer.
- `docs/arch/typeinfo-graph.md` — projections section.

**Verification:**

```bash
pnpm vitest --run packages/typeinfo packages/component-meta
cargo test --workspace --tests --verbose
```

### 8.6 Phase 5 — Vue Framework Surface Adapter

**Changes:**

1. Rebuild `@verter/component-meta` as a thin Vue adapter producing `FrameworkSurfacePayload`.
2. Rebuild `@verter/component-meta/compat` as a typeinfo-projection wrapper.
3. Fix the four known mismatches:
   - **Popover `SlotProps<M>`** via `resolveSymbolGraph` → §3.6 shape.
   - **Theme alias display vs expandable** via `display_ref` + `target`.
   - **EditorDragHandle `Button["variants"]["color"]`** via `projectPathGraph`.
   - **ContentSearch inherited / intersection slots** via `TypeNode::Intersection`.

**Legacy deletions:**

1. Delete the component-meta-local "graph" type in `packages/component-meta/src/types.ts` if still present.
2. Delete every remaining compat semantic-recovery hook.

**Documentation updates (same phase):**

- `.claude/skills/component-meta/SKILL.md` — finalised Vue-adapter contract.

**Verification:**

```bash
pnpm vitest --run packages/component-meta
cargo test --workspace --tests --verbose
node scripts/gen-corpus-audit-tests.mjs
pnpm test:e2e:vscode
```

### 8.7 Phase 6 — Client-Facing Zod And Schema Helpers

**Changes:** as in the prior draft (`@verter/typeinfo/zod`, `/json-schema`, framework-surface helpers).

**Documentation updates (same phase):** `.claude/skills/component-meta/SKILL.md`, `.claude/skills/type-resolution/SKILL.md`.

**Verification:** `pnpm vitest --run packages/typeinfo`.

### 8.8 Phase 7 — LSP / MCP / Playground Integration

**Changes:** LSP hover → graph + display projection; LSP completion → framework-surface; MCP `typeinfo.*` tools; MCP `component-meta.*` tools; playground type explorer.

**Legacy deletions:** LSP / MCP code paths calling legacy `evaluateTypeExpression` directly.

**Documentation updates (same phase):** `.claude/skills/host-session/SKILL.md`, `.claude/skills/architecture/SKILL.md`.

**Verification:** `pnpm test:e2e:vscode`, MCP integration tests.

### 8.9 Phase 8 — Verification Sweep

**This phase is verification only — no first-time documentation, no new APIs.**

**Changes:**

1. Run every architecture guard from Phases 0a–7 in a single `cargo test --workspace --tests --verbose` + `pnpm test` invocation.
2. Add the acceptance suite from the prior draft (Phase 8 acceptance table).
3. Final `find` over the repository asserts zero matches for the forbidden patterns:

```bash
find crates packages -type f \( -name '*.ts' -o -name '*.rs' \) | xargs grep -l 'rawType\|parse_type_annotation\|looksLike\|extract_pick_slot_bindings\|prefer_alias_for_string_intrinsics\|repairOpaque' | grep -v -- '__tests__\|jsdoc' | wc -l
# must output 0
```

**Verification:**

```bash
cargo test --workspace --tests --verbose
pnpm vitest --run
pnpm test
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
node scripts/gen-corpus-audit-tests.mjs
pnpm install --frozen-lockfile
```

### 8.x Existing Ignored Test Lift Schedule (384 tests across `crates/verter_session/src/typeinfo/typeinfo_tests/`)

> **SUPERSEDED (see the top-of-file banner):** this per-file phase schedule and the `typeinfo_tests_unignore_plan.md` doc-manifest are historical. The authoritative ledger is the 363-row `block_id` partition in [`native-typeinfo-parity.md`](./native-typeinfo-parity.md) §10.4.1, backed by the in-repo `.rs` two-table manifest (`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`); the binding count is the parser-derived **363** (not 384 — that was the raw `#[ignore]` line count), tracked by `block_id` rather than phase.

The 384 `#[ignore]`'d tests describe future contracts that this plan satisfies. The schedule below assigns each FILE to a phase; per-test target details for the highest-coupling files live in the follow-up artifact `crates/verter_session/src/typeinfo/typeinfo_tests_unignore_plan.md` (created in Phase 0a as a planning doc; the schedule below summarises by phase).

**Phase 0b/1 (foundational shapes — exporter + lowering + new SemanticQueryKey variants land):**

- `basic.rs`, `support.rs`, `mod.rs` — baseline plumbing.
- `mode_boundary_invariants.rs`, `demand_boundary.rs` — mode/demand contract.
- `cross_file.rs` — host-backed cross-file resolution.
- `cache_invalidation.rs`, `flow_invalidations.rs` — fact-signature fencing.
- `expansion_boundaries.rs`, `wide_deep.rs`, `deep_path.rs` — closure budgets.
- `relation_semantics.rs` — relation engine public surface.
- `enums.rs` — `SemanticQueryKey::ResolveEnum`.
- `module_features.rs` — `ResolveModuleAugmentation`, `ResolveAmbientNamespace`.
- `function_advanced.rs`, `call_resolution.rs` — `ResolveOverloadSet`.
- `generic_defaults.rs`, `const_type_param.rs`, `no_infer.rs` — `TypeParameter.{variance, is_const, no_infer}` lowering chain (§3.10).
- `branded_types.rs`, `apparent_types.rs` — structural / intersection shape.
- `template_literal_inference.rs` — template literal node + infer-bind.
- `recursive_conditional.rs`, `recursive_union.rs` — `Cycle` placeholder + `cycle_id` stability.
- `mapped_modifiers.rs`, `mapped_template.rs` — `MappedModifier`, `name_remap`.
- `index_signatures.rs`, `indexed_utilities.rs`, `union_key_access.rs` — `IndexSignature`, `IndexedAccess`, `KeyOf`.
- `tuple_labels.rs`, `variadic_tuples.rs` — `TupleElement.label`, `rest`.
- `unique_symbol.rs` — `UniqueSymbol`.
- `utility_top_bottom.rs`, `utility_edge.rs`, `utility_composition.rs` — TS built-in utility typing.
- `substitution_types.rs`, `conditional_infer.rs` — `SubstitutionConcrete`, `Infer`.

**Phase 0b/1 + Phase 3 (flow narrowing + contextual typing — gated by `closure: ProjectionRequired`):**

- `contextual_typing.rs` — `ContextualType` payload.
- `narrow_typeof.rs`, `narrow_instanceof.rs`, `narrow_equality.rs`, `narrow_in_operator.rs`, `narrow_discriminated_union.rs`, `narrow_truthiness.rs` — `FlowNarrowing` + `NarrowingCause` variants.
- `flow_return_*.rs` (catalog, edge_catalog, parity_contracts, path_contracts — high-coupling subgroup) — `FlowNarrowing` against function returns; per-test mapping lives in `typeinfo_tests_unignore_plan.md`.
- `value_inference.rs` — `ContextualType` + `inference_bindings`.

**Phase 3 (public TypeInfoSession + nested audit semantics):**

- `footprint.rs` — audit footprint miner extension.

**Phase 4 (projections):**

- `typescript_rules.rs`, `class_features.rs` — `Class` node + projections.
- `modern_ts_features.rs` — utility coverage in projections.
- `jsx.rs` — JSX projection rules.
- `decorators.rs` — UnsupportedConstruct::Decorator + diagnostic projection.

**Phase 5 (Vue surface adapter):**

- `table_like.rs`, `menu_like.rs`, `message_list_like.rs` — corpus-shaped fixtures.

**Test-count target:** of the 384 `#[ignore]` annotations, at least 90% (≥346) are lifted by Phase 5. The remaining ≤10% (≤38) MUST each be re-`#[ignore]`'d with an explicit `IGNORE-REASON:` comment naming either (a) a STOP-gated future adapter (Svelte/React) or (b) a typed unsupported construct.

**Manifest landing rule:** the file `crates/verter_session/src/typeinfo/typeinfo_tests_unignore_plan.md` IS PART OF PHASE 0a (NOT a forward commitment). The Phase 0a deliverable includes a complete manifest naming every `#[ignore]`'d test, its target lift phase (0b/1, 3, 4, or 5), and a one-line "what implementation makes this pass" entry. A planner-reviewer reading the Phase 0a tree can mechanically verify which of the 384 ignored tests will become live in each phase by reading the manifest.

Backing guards:

- `typeinfo_tests_unignore_manifest_complete` (Phase 0a) — walks `crates/verter_session/src/typeinfo/typeinfo_tests/**` for every `#[ignore]` annotation; asserts each `(file, fn_name)` pair appears in the manifest with `target_phase` and `unblocked_by` columns.
- `typeinfo_tests_unignored_count_by_phase` (Phase 0a creates with thresholds; each phase asserts the threshold met) — `Phase 0a: ≥ 0` lifted, `Phase 0b/1: ≥ N1` (where `N1` is derived from the manifest as the count of rows with `target_phase = "0b/1"`), `Phase 3: ≥ N1 + N3`, `Phase 4: ≥ N1 + N3 + N4`, `Phase 5: ≥ 346 (90% of 384)`.
- `every_typeinfo_test_ignore_has_named_reason` — every surviving `#[ignore]` carries an `IGNORE-REASON:` comment.

The mechanically-verifiable manifest is the SDK-reviewer's escape hatch: a Phase-0b/1 reviewer can `wc -l` against the `target_phase` column rather than reading 384 individual tests.

---

## 9. Risks And STOP Gates

### 9.1 Cache-Key Drift

Mitigation: `TypeInfoPolicyKey::canonicalize()` is the single canonical-form producer. Guard: `typeinfo_policy_key_has_single_canonicaliser`.

### 9.2 Wire Format Churn

Mitigation: `SemanticTypeGraph.schema_version: u32` envelope plus `TypeInfoSessionHandshake` negotiation (§2.21). At session open the client and host negotiate the highest mutually-supported `schema_version`; per-request payloads carry the negotiated version. Decoders fail-closed with `TypeInfoRequestError::UnknownSchemaVersion { client_version, server_versions }` — a typed error consumers can switch on, not a panic. No `UnknownVariant` carrier — closed-enum discipline per §2.21.

### 9.3 Snapshot Bloat

Mitigation: `nodeBudget` and `depthBudget` are required (no defaults). Over-budget sub-trees produce `TypeNode::Opaque(QueryErrorDto::BudgetExceeded { budget })` and the surrounding payload routes to `DegradedResultStore`, NOT the warm cache (§2.6).

### 9.4 Declaration Merging Performance

Mitigation: merge memoised in `SemanticGraphStore` via `SemanticQueryKey::ResolveMergedDeclaration`; cosmetic edits invariant under `parse_stable_hash`; augmentation index provides inverse lookup (R29).

### 9.5 Cross-File MergedDeclaration Completeness

Cross-file declaration merging lands in the same phase as the lowering (Phase 0b/1) — there is no "deferred" cross-file merge; the contract is base-correct at landing time. The `merged_interfaces_across_files` regression test (`crates/verter_session/tests/typeinfo_graph_merging.rs`) asserts FIVE discriminating properties, each named so a reviewer can verify the test catches each defect class:

1. **Contributor identity** — `parts[i].part_symbol` equals the `ResolvedDeclSlotIdentity` of file `i`'s contribution. A test that only asserts `parts.len() == 2` does NOT catch a swap-and-relabel defect; the identity check does.
2. **Member projection union with provenance** — for `interface Foo { a }` in A + `interface Foo { b }` in B, the merged `members` set contains both `a` and `b`; each member's `declaration: Option<SymbolId>` resolves through `symbol_id_map` back to file A or file B (not both, not neither).
3. **Stable ordering** — `parts` are sorted by `(canonical_path, declaration_offset)`. The test runs the same query 5 times across cold/warm boundaries; ordering is byte-identical.
4. **Symbol-space separation** — when `interface Foo` (Type) + `namespace Foo` (Value + Namespace) merge in the same file, three `SymbolNode` entries emit (Type Foo, Value Foo, Namespace Foo) sharing one `merged_symbol_name` and one `merged_decl` reference. The test asserts each `SymbolNode.symbol_space` is distinct and `class_dual_space_emits_two_symbols` extends to namespace-fold cases.
5. **2 → 3 invalidation** — adding a third file C with `interface Foo { c }` invalidates the cached 2-part candidate; the new query returns a candidate with `parts.len() == 3`. The 2-part candidate must NOT be returned to a caller whose `StoreView` includes file C. Augmentation interaction: if a fourth file declares `declare module "owner" { interface Foo { d } }` augmenting the merged interface, the augmenter contributes via `augmentation_contributors` and the merged set's members include `d` — but the augmenter's part_symbol is separate from the 3 direct contributors (test enforces a 3-part `parts` + 1-augmenter `augmentation_contributors` separation).

A test asserting only count (1) is paper-applied; the suite must hold all five properties. Guard: `merged_declarations_carry_contributor_identity` is the discriminating test; `parts_len_is_not_sole_assertion` is a static scan over `typeinfo_graph_merging.rs` rejecting any test body that asserts only `.len()`.

### 9.6 Svelte Adapter STOP Gate

Entry criteria (every item must be true; checked by gate test before §9.6 work starts):

1. Svelte parser landed at `crates/verter_svelte_parser/` with `IndexedReady` production for `.svelte` files.
2. Scheduler integration: `FileArtifactStore` produces `IndexedReady` for `.svelte` files via the same shared host path.
3. ≥15 passing tests in `verter_semantic` covering `export let`, `$$Props`, `<script generics>`, `$$Slots`, `$$Events` (file: `crates/verter_semantic/tests/svelte_surfaces.rs`).
4. `no_phase_archaeology_in_production_code` passes for every new Svelte file.

Gate file: `crates/verter_session/tests/svelte_adapter_stop_gate.rs::svelte_adapter_entry_criteria_met` — `#[ignore]` until all four entry criteria are satisfied; `cargo test` does NOT exercise the body.

### 9.7 React Adapter STOP Gate

Entry criteria:

1. `resolve_react_component_shape` query exists in `verter_session`.
2. Correctly identifies props from `(props: P) => ...`, `React.FC<P>`, `React.forwardRef<R, P>`, `React.memo<P>`.
3. ≥10 passing tests covering generic components, `forwardRef`, default props (`crates/verter_semantic/tests/react_surfaces.rs`).

Gate file: `crates/verter_session/tests/react_adapter_stop_gate.rs::react_adapter_entry_criteria_met` — `#[ignore]` until satisfied.

### 9.8 Concurrent Session Poisoning

Mitigation: multi-candidate substrate (R20, cap 4 per slot). Guard: `concurrent_sessions_isolated_via_multi_candidate`.

### 9.9 Wire-Compat Risk

Per claude REQUIRED #14: external consumers of the existing 94-line `typeinfo.proto` schema (NAPI, WASM, MCP, future SDK consumers) must be migrated in the same phase that lands the new schema. Field-number reservation prevents accidental reuse. The §0.5 existing-state survey enumerates every consumer.

### 9.10 Phase Archaeology In Source

`no_phase_archaeology_in_production_code` scans for `phase 0/1/2/.../9`, `cutover`, `post-typeinfo`, `pre-Phase`. Test files exempt; source files in `crates/*/src/**` are not.

---

## 10. Audit Infrastructure Integration

Every public typeinfo entry-point opens a `RequestAuditRecord` with `RequestKind::TypeInfoGraph` and populates `RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayloadAudit)`. Files extended in Phase 0a + Phase 0b/1:

| File | Change |
|---|---|
| `crates/verter_audit/src/record.rs` | Add `RequestKind::TypeInfoGraph` + `RequestKindPayload::TypeInfoGraph(TypeInfoGraphPayloadAudit)` + typed accessor `as_typeinfo_graph()`. |
| `crates/verter_audit/src/config.rs` | Add `KindBit::TypeInfoGraph` for `AuditConsumerFilter`. |
| `crates/verter_audit/src/batch.rs` | Extend `BatchAuditAggregator` row counter for `TypeInfoGraph`. |
| `crates/verter_audit/src/structured_event.rs` | Add `StructuredAuditEvent::TypeInfoGraphPublished { layer: &'static str, audit: TypeInfoGraphPayloadAudit }` and `StructuredAuditEvent::TypeInfoGraphDegraded { layer: &'static str, audit: TypeInfoGraphPayloadAudit, reason: DegradationReasonTag }`. The warm-hit path emits no structured event (only `emit_counter(CounterKind::CacheHit, "typeinfo_graph")`); see §10.1. |
| `crates/verter_session/src/component_meta_audit/footprint_miner.rs` | Add a new `TypeInfoGraphFootprintCell` struct (fields: `snapshot_node_count: u32`, `snapshot_edge_count: u32`, `exactness_summary: ExactnessSummary`, `merged_decl_count: u32`, `augmentation_count: u32`, `overload_signature_count: u32`) and a new `FootprintAccumulator.typeinfo_graph_cells: SmallVec<[TypeInfoGraphFootprintCell; 4]>` field. The contribution function is `fn contribute_typeinfo_graph(record: &TypeInfoGraphPayloadAudit) -> TypeInfoGraphFootprintCell`. The miner does not switch on `RequestKind` — it receives an already-typed `TypeInfoGraphPayloadAudit` from the record observer. |
| `packages/types/audit.generated.ts` | Regenerated via `ts-rs`. |
| `crates/verter_session/tests/architecture_guards.rs::wave_3_entry_points_propagate_tls` (function at line 7641 inside `architecture_guards.rs`) | Extend with typeinfo entry-point drivers verifying TLS observer propagation. There is NO sibling `wave_3_entry_points_propagate_tls.rs` file — the function lives inside `architecture_guards.rs`. |

> SUPERSEDED: the nine `exactness_*: u32` scalar fields below are replaced by one `exactness_counts: BTreeMap<ExactnessTag, u32>` map field (reconcile-#5 / CF-2; unified plan §2.2). This applies to every `TypeInfoGraphPayloadAudit` listing in this doc.

```rust
pub struct TypeInfoGraphPayloadAudit {
    pub operation: GraphOperationTag,
    pub mode: ProjectionModeTag,
    pub demand: ReductionDemandTag,
    pub roots_count: u32,
    pub closure: GraphClosurePolicyTag,
    pub snapshot_node_count: u32,
    pub snapshot_edge_count: u32,
    pub snapshot_symbol_count: u32,
    pub exactness_exact_resolved: u32,
    pub exactness_exact_symbolic: u32,
    pub exactness_unresolved_generic: u32,
    pub exactness_partial: u32,
    pub exactness_miss: u32,
    pub exactness_unsupported: u32,
    pub exactness_budget_exceeded: u32,
    pub exactness_unstable: u32,
    pub exactness_cycle: u32,
    pub cache_hit: bool,
    pub publication_retries: u8,
    pub merged_decl_count: u32,
    pub augmentation_count: u32,
    pub overload_signature_count: u32,
    pub relation_check_count: u32,
    pub origin_edges_emitted: u32,
    pub display_projection_emitted: bool,
    pub zod_projection_emitted: bool,
    pub json_schema_projection_emitted: bool,
    pub storybook_projection_emitted: bool,
    pub docs_projection_emitted: bool,
    pub type_descriptor_projection_emitted: bool,
}

pub enum GraphOperationTag {
    ResolveSymbol, EvaluateExpression, ProjectPath, Relate,
    FrameworkSurfaces, ExpandAround, FlowNarrowingAt, ContextualTypeAt,
}

pub enum GraphClosurePolicyTag {
    RootOnly, Path, OneLevel, Expanded, ProjectionRequired,
}
```

### 10.1 Per-Entry-Point Audit Registration (Pseudocode)

Every typeinfo entry-point follows a three-branch pattern. The branches differ on whether the result was a cold publish, a warm hit, or a degraded return — and only ONE of the three emits a `StructuredAuditEvent`. This is normative: the warm-hit branch MUST NOT emit `TypeInfoGraphPublished` (the `typeinfo_warm_hit_emits_no_structured_payload` guard enforces this; emitting on every hit would flood the structured-event bus).

```rust
impl TypeInfoSession {
    pub fn resolve_symbol_graph_with_audit(
        &self,
        request: ResolveSymbolGraphRequest,
    ) -> AuditedResult<Arc<TypeInfoGraphPayload>, TypeInfoRequestError> {
        // 1. Validate request — typed error before semantic execution. The
        //    `?` propagates a `TypeInfoRequestError` into `AuditedResult.value`;
        //    `record` still exists (carries the validation-failure footprint).
        let record = self.audit.open_record(RequestKind::TypeInfoGraph);
        if let Err(e) = validate_resolve_symbol_graph_request(&request) {
            record.populate(TypeInfoGraphPayloadAudit::from_validation_error(&e));
            return AuditedResult { value: Err(e), record };
        }

        // 2. Compute under fact tracer. `get_or_compute` returns an enum
        //    distinguishing the three outcome paths.
        let (outcome, fact_read_set): (GetOrComputeOutcome<Arc<TypeInfoGraphPayload>>, FactReadSet) =
            self.resolver_context().with_fact_tracer(|| {
                self.type_info_graph_db.get_or_compute(slot_key, &request)
            });
        let admission = SignatureAdmission::from_finalise(fact_read_set.finalise());

        // 3. Three audit branches — only cold publish emits a structured event.
        match outcome {
            GetOrComputeOutcome::ColdPublish(payload) => {
                record.populate(TypeInfoGraphPayloadAudit::from(
                    &payload,
                    /* cache_hit */ false,
                ));
                self.audit.emit(StructuredAuditEvent::TypeInfoGraphPublished {
                    layer: "typeinfo_graph",
                    audit: record.payload().as_typeinfo_graph().unwrap().clone(),
                });
                AuditedResult { value: Ok(payload), record }
            }

            GetOrComputeOutcome::WarmHit(payload) => {
                record.populate(TypeInfoGraphPayloadAudit::from(
                    &payload,
                    /* cache_hit */ true,
                ));
                // Counter-only — no StructuredAuditEvent emitted on warm hits.
                // Guard `typeinfo_warm_hit_emits_no_structured_payload` enforces.
                self.audit.emit_counter(CounterKind::CacheHit, "typeinfo_graph");
                AuditedResult { value: Ok(payload), record }
            }

            GetOrComputeOutcome::Degraded(error) => {
                record.populate(TypeInfoGraphPayloadAudit::from_degraded(&error));
                self.audit.emit(StructuredAuditEvent::TypeInfoGraphDegraded {
                    layer: "typeinfo_graph",
                    audit: record.payload().as_typeinfo_graph().unwrap().clone(),
                    reason: error.degradation_reason_tag(),
                });
                AuditedResult { value: Err(error), record }
            }
        }
    }
}
```

For `expand_graph_around_with_audit`, the inner record opens as a CHILD of the caller's outer record (via `audit.open_child_record(parent_record_id, ...)`). The audit-bridge test `expand_graph_around_records_nested_audit_entry` asserts the parent-child relationship.

Cold-publish branch — emit `TypeInfoGraphPublished` (full structured payload). Warm-hit branch — emit a counter only (`CacheHit`); zero structured payload (the warm-hit guard rejects any structured emit on this path). Degraded branch — emit `TypeInfoGraphDegraded` carrying the closed-enum reason (e.g. `BudgetExceeded`, `UnstableState`, `Unsupported`).

Guards: `audit_record_carries_typeinfo_graph_kind`, `typeinfo_warm_hit_emits_no_structured_payload`, `expand_graph_around_records_nested_audit_entry`, `typeinfo_graph_degraded_emits_structured_event`.

---

## 11. parse_type_annotation Second Exception RFC — Resolution

Per claude #17: The plan resolves this by adopting `StructuredTypeExpression` (Alt A in the synthesis dossier). `parse_type_annotation` is NOT given a second exception. There is no synthesise-then-reparse path inside the typeinfo pipeline. CLAUDE.md's existing rule ("parse_type_annotation is reserved for JSDoc tag-type payloads") stands unchanged.

The static guard `evaluate_type_expression_does_not_call_parse_type_annotation` enforces this. The CLAUDE.md rule does not need amendment.

---

## 12. Backward Compatibility

`@verter/type-ir`'s `TypeDescriptor` DTO is preserved as a projection target. The projection logic lives in `@verter/typeinfo/projections/type-descriptor`. Legacy entry-point function names (`resolveSymbol`, `evaluateTypeExpression`) are NOT preserved — consumers migrate to the graph API in Phase 3. Vue `vue-component-meta`-compatible output remains byte-identical for the corpus benchmark.

---

## 13. Documentation Updates Map

Per CX5: documentation lands in the same phase as the implementation. The table below summarises which owning skill / doc is updated by which phase:

| Skill / Doc | Updated in phase(s) |
|---|---|
| `.claude/skills/type-resolution/SKILL.md` | 0a (variants names), 0b/1 (dispatch contract), 3 (TypeInfoSession surface), 6 |
| `.claude/skills/type-cache-architecture/SKILL.md` | 0a (shapes), 0b/1 (DBs + key composition), 8 (final table) |
| `.claude/skills/component-meta/SKILL.md` | 0a (registry), 0b/1 (graph state), 3 (compat-via-projection), 4 (projections), 5 (Vue adapter), 6 |
| `.claude/skills/audit-infrastructure/SKILL.md` | 0a (registry), 0b/1 (payload semantics + footprint), 2 (TLS), 3 (nested records) |
| `.claude/skills/host-session/SKILL.md` | 7 (LSP integration) |
| `.claude/skills/architecture/SKILL.md` | 7 (MCP/playground) |
| `.claude/skills/testing/SKILL.md` | 2 (TS decoder pattern) |
| `CLAUDE.md` | 0a (CRITICAL section + skill pointer) |
| `docs/arch/typeinfo-graph.md` | 0b/1 (first version), 4 (projections), 8 (final) |
| `docs/arch/fact-based-cache.md` | 0b/1 (new DBs) |
| `docs/contributing/typeinfo-cutover-deletions.md` | 2 (first version), updated each phase |

Phase 8 is verification only — it does NOT introduce first-time documentation.

---

## 14. Final Invariants Table

One row per §2.x invariant (per claude #18). The plan's acceptance criterion is: every row has at least one named guard, every guard is registered in `CRITICAL_RULE_GUARDS`, every guard has a discriminating regression test.

| §2.x | Invariant | Registered guard(s) | Owning skill section | Discriminating test |
|---|---|---|---|---|
| 2.1 | Five Query Modes Explicit | `path_projection_mode_cascade`, `typeinfo_request_validates_mode_present` | `/type-resolution` | `semantic_path_cascade.rs::path_projection_mode_cascade` |
| 2.2 | Backfill Rule | `backfill_rule_holds_across_modes` | `/type-resolution` | `crates/verter_session/tests/backfill_rules.rs::backfill_rule_holds_across_modes` |
| 2.3 | Five-Way Env Hash Split | `r21_no_bundled_config_hash`, `r21_lib_env_hash_inclusion` | `/type-cache-architecture` | `cache_key_invariants.rs::r21_no_bundled_config_hash` |
| 2.4 | Two Cache Families | `cache_key_no_value_fields`, `multi_candidate_distinguishing_dimension_is_value_only` | `/type-cache-architecture` | `cache_key_invariants.rs::cache_key_no_value_fields` |
| 2.5 | Publication Fence | `publication_fence_revalidates_before_publish`, `typeinfo_graph_publication_fence_3_retries`, `typeinfo_graph_unstable_returns_typed_error`, `completion_fence_uses_max_inflight_retries_constant`, `typeinfo_warm_hit_emits_no_structured_payload` | `/type-resolution` | `crates/verter_session/tests/typeinfo_graph_publication_fence.rs` + `crates/verter_session/tests/typeinfo_graph_warm_hit_audit.rs` |
| 2.6 | Warm-Cache Exactness Contract | `typeinfo_graph_no_partial_admission`, `degraded_payload_never_warm_admitted`, `degraded_store_never_serves_complete_admission` | `/type-cache-architecture` | `typeinfo_graph_no_partial_admission.rs` |
| 2.7 | Arc-Published Immutable Payloads | `typeinfo_graph_warm_hit_zero_alloc` | `/type-cache-architecture` | `canary_warm_hit_zero_alloc.rs` |
| 2.8 | Bounded Retention | `bounded_query_retention_per_slot_cap`, `bounded_query_retention_global_budget` | `/type-cache-architecture` | `crates/verter_session/tests/bounded_query_retention_tests.rs` |
| 2.9 | Fact-Signature Validation | `typeinfo_graph_warm_hit_revalidates_self_root` | `/type-cache-architecture` | `typeinfo_graph_self_root.rs` |
| 2.10 | IndexedReady Authority | `graph_export_reads_only_from_indexed_ready` | `/type-resolution` | `architecture_guards.rs` |
| 2.11 | Typed-IR-Only Resolver Rule | `typeinfo_projection_no_raw_or_display_string_semantics`, `evaluate_type_expression_does_not_call_parse_type_annotation` | `/component-meta`, `/type-resolution` | `architecture_guards.rs` + `projection_no_text_semantics.test.ts` |
| 2.12 | No Role Inference From Suffix | `no_role_inference_from_suffix` | `/component-meta` | `architecture_guards.rs::no_role_inference_from_suffix` |
| 2.13 | SymbolSpace Three Variants | `symbol_node_preserves_type_value_namespace_spaces`, `class_dual_space_emits_two_symbols` | `/type-resolution` | `symbol_node_invariants.rs` |
| 2.14 | Relation Engine Public | `typeinfo_exposes_relate_query` | `/type-resolution` | `typeinfo_graph_relate.rs` |
| 2.15 | Origin-Edge Taxonomy Normative | `origin_edge_taxonomy_locked` | `/type-resolution`, `/audit-infrastructure` | `typeinfo_graph_taxonomy.rs::origin_edge_taxonomy_locked` |
| 2.16 | Symbol Identity Slot-Based | `symbol_node_preserves_resolved_decl_slot_identity` | `/type-resolution` | `symbol_node_invariants.rs` |
| 2.17 | Merge/Augmentation/Ambient First-Class | `merged_declarations_are_public_graph_state`, `module_augmentation_is_public_graph_state`, `ambient_namespaces_are_public_graph_state`, `overload_sets_are_public_graph_state`, `exporter_dispatches_resolve_merged_declaration_query` | `/component-meta`, `/type-resolution` | `typeinfo_graph_merging.rs`, `typeinfo_graph_augmentation.rs`, `typeinfo_graph_ambient.rs`, `typeinfo_graph_overloads.rs` |
| 2.18 | Framework Adapter Boundary | `framework_surface_member_enum_is_closed`, `framework_tag_enum_is_closed`, `framework_surface_member_does_not_override_optionality`, `framework_adapter_does_not_recompute_fallthrough`, `framework_adapter_does_not_reclassify_callback_props_as_events` | `/component-meta` | `architecture_guards.rs` (each guard) |
| 2.19 | Progressive Expansion Is A Semantic Query | `progressive_graph_expansion_dispatches_semantic_query_key`, `progressive_expansion_routes_through_typeinfo_graph_db` | `/type-resolution` | `typeinfo_graph_progressive.rs` |
| 2.20 | No Heuristic Cache Semantics | `cache_identity_has_no_heuristic_dimensions` | `/type-cache-architecture` | `architecture_guards.rs` |
| 2.21 | Closed-Enum Discipline (Version-Bumped) | `no_custom_string_escape_in_typeinfo_dtos`, `decoder_returns_typed_error_on_unknown_variant` | `/component-meta` | `architecture_guards.rs` + `decoder.test.ts` |
| 2.22 | Closed-Enum Fallback Reasons | `degraded_results_use_closed_enum_reasons` | `/component-meta`, `/type-cache-architecture` | `typeinfo_graph_degraded_reasons.rs` |
| 2.23 | SDK Audit Test For Intrinsics | `sdk_audit_unsupported_intrinsic` | `/type-resolution` | `crates/verter_session/tests/sdk_audit_unsupported_intrinsic.rs::sdk_audit_unsupported_intrinsic` |

---

## PART A / PART B DIVIDER

PART A (final-state architecture: §1–§7, §10–§12, §14) reads as if no phase ever existed.
PART B (implementation plan: §0, §0.5, §0.6, §0.7, §8, §9, §13, plus the Rounds 3-15 Commitments Compendium below) is the executor's playbook.
This divider is structural — at landing time, PART A lives at `docs/arch/typeinfo-graph.md`, PART B at `docs/contributing/typeinfo-implementation-plan.md`.
Guard: `part_a_carries_no_phase_archaeology` (Phase 0a static scan rejects "Phase", "Revision", "formerly", "moved", "previously", "retired" tokens inside PART A sections).

Note on PART A residency: §0 (Identity / Why typeinfo Exists / Document Layout) sits at the top of this combined document for reader orientation but is PART B material at landing time. §8 (Phase Plan) and §9 (Risks And STOP Gates) appear in this combined document at lines ~1934 and ~2415 between §7 and §10; they are PART B material at landing time and move to the implementation-plan doc on split. The structural ordering of this combined file is reader-friendly, not landing-canonical.

---

## 0.5 Existing State Survey

This section enumerates every existing codebase artifact touched by the plan, with disposition (`preserve | rename | delete | migrate-to`). The implementer agent must reference this table when executing each phase; the `typeinfo_cutover_deletions_complete` guard in Phase 8 walks this table and asserts every deletion is realised.

### 0.5.1 verter_protocol — Rust DTOs and proto schema

| Artifact | Disposition | Landing phase | Notes |
|---|---|---|---|
| `crates/verter_protocol/src/graph/builder.rs::GraphNode` (24 variants including `RecursiveRef`) | DELETE | 0b/1 | replaced by `verter_protocol::typeinfo::graph::TypeNode`. `RecursiveRef` migrates to `QueryErrorDto::RecursiveRef` for legacy boundary; recursive structural types migrate to `TypeNode::Cycle`. |
| `crates/verter_protocol/src/graph/builder.rs::GraphBuilder, GraphConditionalFrame, GraphTupleElement, GraphObjectMember, GraphFunctionParam, ExprMemoKey` | DELETE | 0b/1 | replaced by exporter at `crates/verter_session/src/typeinfo/exporter.rs`. |
| `crates/verter_protocol/src/graph/schema/*` + `crates/verter_protocol/src/graph/mod.rs` re-exports | DELETE | 0b/1 | superseded by `verter_protocol::typeinfo::graph::*`. |
| `crates/verter_protocol/src/component_meta.rs` GraphBuilder call sites (lines ~38, 69, 222, 243, 261, 290, 309, 332, 349, 373, 388, 402, 421+) | MIGRATE | 0b/1 | rewritten to consume `verter_protocol::typeinfo::graph::TypeNode`. The component-meta payload's `TypeGraph` proto message preserves its outer identity; only the node-shape lowering is rewritten in the same diff. |
| `crates/verter_protocol/proto/verter/v1/typeinfo.proto` (94 lines, current schema) | DELETE+REPLACE | 0a | replaced by §3.0 full rewrite. Field numbers 1-5 of `EvaluateTypeExpressionRequestDto` permanently reserved. `ImportSpecDto`, `NamedImportDto`, `DefaultImportDto`, `NamedBindingDto`, `NamespaceImportDto`, `SymbolEntryDto`, `SymbolEntryListDto` PRESERVED (re-homed into the new schema). |
| `crates/verter_protocol/Cargo.toml` deps | UNCHANGED | 0a | `verter_protocol` keeps its existing `prost` / `prost-build` / `protoc-bin-vendored` deps. The wire surface is protobuf-authoritative; `ts-rs` derives are NOT added on wire DTOs (the audit envelope's `ts-rs` lives in `verter_audit`). TypeScript bindings flow from `proto/verter/v1/*.proto` through the existing protobuf-ts pipeline under `packages/proto`. |

### 0.5.2 verter_session — typeinfo internals

| Artifact | Disposition | Landing phase | Notes |
|---|---|---|---|
| `crates/verter_session/src/typeinfo/evaluate_type_expression.rs` (text-evaluator entry-point) | DELETE | 0b/1 | replaced by `crates/verter_session/src/typeinfo/session.rs::evaluate_type_expression_graph_with_audit`. |
| `crates/verter_session/src/typeinfo/raise.rs` | DELETE | 0b/1 | the raising logic that "raises" a `TypeExpr` from a `SemanticNodeId` is moved into the exporter's lowering pass (`crates/verter_session/src/typeinfo/exporter.rs`). |
| `crates/verter_session/src/typeinfo/resolve_named_symbol.rs` | DELETE | 0b/1 | replaced by `resolve_symbol_graph_with_audit`. |
| `crates/verter_session/src/typeinfo/scratch_cache.rs` | DELETE | 0a | scratch-cache mechanism for text-evaluator is replaced by `StructuredTypeExpression`'s direct dispatch (no scratch state needed). |
| `crates/verter_session/src/typeinfo/symbol_inventory.rs` | RENAME → `crates/verter_session/src/typeinfo/list_symbols.rs` | 0b/1 | `listSymbols` continues to exist; the file is renamed to disambiguate from the old text-evaluator infrastructure. |
| `crates/verter_session/src/typeinfo/types.rs` | DELETE+REPLACE | 0a | replaced by `crates/verter_protocol/src/typeinfo/*.rs` Rust DTOs. |
| `crates/verter_session/src/typeinfo/tests.rs` (top-level) | DELETE | 0b/1 | the new typeinfo tests live under `crates/verter_session/tests/typeinfo_graph_*.rs` (integration) + `crates/verter_session/src/typeinfo/typeinfo_tests/` (unit lift schedule, §8.x). |
| `crates/verter_session/src/typeinfo/typeinfo_tests/` (384 ignored tests) | LIFT PER §8.x | 0b/1 onwards | see §8.x. |

### 0.5.3 verter_napi / verter_wasm — FFI entries

| Artifact | Disposition | Landing phase | Notes |
|---|---|---|---|
| Existing typeinfo NAPI exports in `crates/verter_napi/src/*` consuming `EvaluateTypeExpressionRequestDto` | DELETE | 2 | replaced by new `_with_audit` exports at `crates/verter_napi/src/typeinfo.rs`. |
| Existing typeinfo WASM exports in `crates/verter_wasm/src/*` | DELETE | 2 | replaced by new exports at `crates/verter_wasm/src/typeinfo.rs`. |

### 0.5.4 verter_audit — audit envelope extensions

| Artifact | Disposition | Landing phase |
|---|---|---|
| `crates/verter_audit/src/record.rs::RequestKind` enum | EXTEND (add `TypeInfoGraph`) | 0a |
| `crates/verter_audit/src/record.rs::RequestKindPayload` enum | EXTEND (add `TypeInfoGraph(TypeInfoGraphPayloadAudit)`) | 0a |
| `crates/verter_audit/src/config.rs::KindBit` | EXTEND (add `TypeInfoGraph`) | 0a |
| `crates/verter_audit/src/structured_event.rs::StructuredAuditEvent` | EXTEND (add `TypeInfoGraphPublished`) | 0a |
| `crates/verter_audit/src/batch.rs::BatchAuditAggregator` | EXTEND (row counter for `TypeInfoGraph`) | 0b/1 |
| `crates/verter_audit/src/origin_graph.rs::OriginEdgeKind` (existing 10-variant enum) | PRESERVE | — | the `SharedLoadReuse` variant remains as the audit-only edge. Mirror in `verter_protocol::typeinfo::graph::OriginEdgeKind`. |

### 0.5.5 @verter/typeinfo — TypeScript package

| Artifact | Disposition | Landing phase | Notes |
|---|---|---|---|
| `packages/typeinfo/src/session.ts` (legacy) | DELETE+REPLACE | 2 | new session implementation at the same path. |
| `packages/typeinfo/src/types.ts` (legacy DTO surface) | DELETE | 2 | replaced by generated TypeScript at `packages/proto/src/gen/verter/v1/typeinfo_pb.ts`, sourced from `crates/verter_protocol/proto/verter/v1/typeinfo.proto` via the existing protobuf path (the same path that already generates `component_meta_pb.ts` and `selective_component_meta_pb.ts`). The wire DTOs are protobuf-authoritative; `ts-rs` is NOT used for typeinfo wire DTOs (it is reserved for the audit envelope under `crates/verter_audit`). |
| `packages/typeinfo/src/descriptor-to-native.ts` | DELETE | 4 | consumers migrate to the `toTypeDescriptor` projection. |
| `packages/typeinfo/src/native-to-descriptor.ts` | DELETE | 4 | same. |
| `packages/typeinfo/src/native-type-expr.ts` | DELETE | 2 | replaced by `TypeInfoGraphPayload`. |
| `packages/typeinfo/src/index.ts` | KEEP, REWRITE | 2 | single re-export point for the package. |

### 0.5.6 @verter/component-meta — Vue adapter package

| Artifact | Disposition | Landing phase |
|---|---|---|
| `packages/component-meta/src/type-graph-core.ts` | DELETE | 2 |
| `packages/component-meta/src/type-graph-proto-decode.ts` (currently consumes legacy `RecursiveRef` at line ~198) | DELETE | 2 |
| `packages/component-meta/src/type-graph.test-utils.ts` | DELETE | 2 |
| `packages/component-meta/src/type-expr-bridge.ts` | DELETE | 2 |
| `packages/component-meta/src/compat/*.ts` semantic-recovery hooks | DELETE | 4 |
| `packages/component-meta/src/types.ts` local "graph" type if present | DELETE | 5 |

### 0.5.7 packages/types

| Artifact | Disposition | Landing phase |
|---|---|---|
| `packages/types/audit.generated.ts` | EXTEND (regenerated via ts-rs) | 2 |
| New `packages/typeinfo/src/generated/graph.generated.ts` | ADD | 0a |

The `typeinfo_cutover_deletions_complete` guard (Phase 8) walks every row above and asserts the disposition column matches the tree state.

---

## 0.6 Native API Name Alignment

The plan refers to types and functions whose names must match the actual codebase. The table below maps every plan-side reference to its real codebase symbol. Phase 0a includes a name-alignment audit step that asserts every reference resolves; phase-by-phase implementations bind to the real names.

| Plan-side reference | Actual codebase symbol | Location |
|---|---|---|
| `CompletionFence::publish_with_retry` | `CompletionFence::publish_with_retry` (NEW in Phase 0b/1) at `crates/verter_session/src/typeinfo/completion_fence.rs` — wraps the existing `InflightTable` substrate; consumes the existing `MAX_INFLIGHT_RETRIES = 3` constant. No second retry constant is introduced. The plan-revision-3 path `crates/verter_session/src/host_resolve/completion_fence.rs` does NOT exist; the adapter lands at the new path. | `crates/verter_session/src/typeinfo/completion_fence.rs` (NEW) |
| `MAX_INFLIGHT_RETRIES` | `MAX_INFLIGHT_RETRIES: usize = 3` constant (existing) | `crates/verter_session/src/semantic_query_memo/inflight.rs:226` |
| `ResolverContext::fact_read_set.snapshot()` | `ResolverContext::with_fact_tracer(|| ...) -> (T, FactReadSet)` then `FactReadSet::finalise() -> FactReadSetFinalise`, lifted into a cache admission via `crate::cache_runtime::SignatureAdmission::from_finalise(finalise)` and into a path-precise read signature via `crate::fact_signature_helpers::ReadSetSignature::new(facts)` | `crates/verter_session/src/resolver_core/resolver_context.rs`, `crates/verter_session/src/resolver_core/fact_read_set.rs:171`, `crates/verter_session/src/cache_runtime/node.rs:95` |
| `validate_fact_signature_with_self_roots` | `crate::fact_signature_helpers::validate_fact_signature_with_self_roots` | existing helper at `crates/verter_session/src/fact_signature_helpers.rs` |
| `SemanticGraphStore::execute_cooperative` | `ProjectSemanticDispatch::execute` (the project-wide dispatch entrypoint) | `crates/verter_session/src/semantic_query_memo/*` |
| `cooperative_admit_with_post_publish` | `crate::cache_runtime::singleflight::cooperative_admit_with_post_publish` | `crates/verter_session/src/cache_runtime/singleflight.rs:852` |
| `BoundedCandidateMap` | `verter_session::bounded_query_retention::BoundedCandidateMap` | existing |
| `GlobalRetentionBudget` | `verter_session::bounded_query_retention::GlobalRetentionBudget` | existing |
| `OriginEdgeKind` (semantic side, 9 variants) | `verter_session::semantic_query::OriginEdgeKind` | `crates/verter_session/src/semantic_query.rs:826` |
| `OriginEdgeKind` (audit side, 10 variants incl. `SharedLoadReuse`) | `verter_audit::origin_graph::OriginEdgeKind` | `crates/verter_audit/src/origin_graph.rs:193` |
| `QueryError` (producer side) | `verter_session::semantic_query::QueryError` (variants: `Miss`, `UnsupportedIntrinsic`, `BudgetExceeded(BudgetExceededFailure)`, `UnstableState`, `AliasCycle`, `RecursiveRef`, `Other(Arc<str>)`, `DeclPlaceholder { canonical_id, name, whole_hash }`) | `crates/verter_session/src/semantic_query.rs:991` |
| `BudgetExceededFailure` | existing type with fields `{ domain: BudgetDomain, limit: usize, actual: u64, context: String }` — there is NO `detail_name` field. The plan-revision-3 reference to `failure.detail_name` was incorrect; the lossless lowering uses `failure.context`. | `crates/verter_session/src/resolver_core/shallow_file_state.rs:225` |
| `BudgetDomain` | existing enum, 6 variants (`LocalClosure`, `Frontier`, `BuilderExpansion`, `SolverResolveSteps`, `SolverArenaNodes`, `SolverInstantiationDepth`) | `crates/verter_session/src/resolver_core/shallow_file_state.rs:238` |
| `cooperative_admit_with_post_publish` real arity | 10 args: `(map, inflight, key, validate, compute, project, revalidate_after_compute, removal_cleanup, post_publish, publish_fence: Option<&parking_lot::RwLock<()>>)` returning `Option<V>` | `crates/verter_session/src/cache_runtime/singleflight.rs:852` |
| `wave_3_entry_points_propagate_tls` | a function (line 7641) inside an existing file, NOT a separate file at `tests/wave_3_entry_points_propagate_tls.rs` | `crates/verter_session/tests/architecture_guards.rs:7641` |
| `CRITICAL_RULE_GUARDS` | `const CRITICAL_RULE_GUARDS: &[(&str, &[&str])]` | `crates/verter_session/tests/critical_rules_have_guards.rs:78` |
| Meta-guard `every_critical_rule_in_docs_has_registered_guard` | existing | `crates/verter_session/tests/critical_rules_have_guards.rs:368` |
| `IntrinsicRegistry::lookup` | existing | per CLAUDE.md |
| `FileArtifactStore::augmentation_index` | existing inverse-lookup keyed by `AugmentationTargetKey` | per CLAUDE.md |
| `MemberShapeCacheDb` | retired; replaced by `ShapeCacheDb` — the per-member graph-native materialiser cache is now a slot inside `ShapeCacheDb` indexed by `ShapeSubject::SemanticNode` and built via `ShapeCacheKey::semantic_node_whole(scope, semantic_node_id, mode)`. `ProjectTypeStore` owns the unified `shape_cache_db: ShapeCacheDb`. The static guard at `crates/verter_session/tests/block_6i_static_guards.rs::shape_cache_db_replaces_split_caches` asserts `pub struct MemberShapeCacheDb` MUST NOT exist anywhere in the tree. | `crates/verter_session/src/component_meta_caches.rs` (`pub struct ShapeCacheDb`, `pub enum ShapeSubject`, `ShapeCacheKey::semantic_node_whole`) |
| `MemberSemanticFactStore`, `MemberDisplayFactStore` | existing | per CLAUDE.md |
| `with_fact_tracer` helper | `ResolverContext::with_fact_tracer<F, R>(&self, f: F) -> (R, FactReadSet)` | `crates/verter_session/src/resolver_core/resolver_context.rs:1265` |
| `finalise_signature_or_empty` | retired (it collapsed Overflow into Empty, which is a correctness-class defect); replaced by the explicit two-step `FactReadSet::finalise() -> FactReadSetFinalise` (`Ok(facts)` / `Overflow`) plus `crate::cache_runtime::SignatureAdmission::from_finalise(finalise)` for cache admission (Overflow routes through the non-cacheable arm, distinct from Empty). The `compile_fact_emission` module retains the helper-side surface; cache producers use `SignatureAdmission::from_finalise`. Compile-cache producers go through `crate::compile_fact_emission` directly. | `crates/verter_session/src/resolver_core/fact_read_set.rs:171`, `crates/verter_session/src/cache_runtime/node.rs:95`, `crates/verter_session/src/compile_fact_emission.rs` |
| `ReadSetSignature` | existing — carries `facts: ...` field | `crates/verter_session/src/`... |
| `host_resolve/virtual_file_pipeline.rs` reference pattern (`with_fact_tracer` + `SignatureAdmission::from_finalise(fact_read_set.finalise())`) | the canonical usage pattern other entry-points follow | `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1066` |
| `RetentionGate` | existing on every project-scoped query DB | per CLAUDE.md |
| `HostAuditRuntime` | existing | per CLAUDE.md |
| `RequestAuditRecord` | existing | per `/audit-infrastructure` skill |
| `BatchAuditAggregator` | existing | `crates/verter_audit/src/batch.rs` |
| `AuditConsumerFilter` | existing | `crates/verter_audit/src/config.rs` |
| `StoreView` | existing | per CLAUDE.md |
| `VersionedDeclIdentity`, `ResolvedDeclSlotIdentity` | existing | per `/type-resolution` skill |
| `IndexedReady` | existing canonical post-parse artifact | per CLAUDE.md |

Where a plan-side name does not yet exist (e.g. `TypeInfoGraphResultDb`, `TypeInfoSession`, the new `SemanticQueryKey` variants), the plan introduces it in the listed phase and the §0.6 row above documents the dependency. No invented references survive Phase 0a's name-alignment audit.

---

## 0.7 Design Principle

> Typeinfo answers: **What is this type, where did it come from, what symbols and substitutions define it, what relations can be proved over it, what can be resolved now, what remains unresolved, and what can a client safely derive from it?**
>
> Framework adapters answer: **Which framework surfaces exist, and which typeinfo graph nodes describe their types?**
>
> Rendered strings answer only: **How should this type be shown for this projection?**

The graph is the foundation. Projections are derivations. Framework adapters are selectors. The relation engine is public. Heuristic recovery is forbidden. Cache identity is typed. Publication is fenced. Every degraded state is observable. This is the foundation for replacing TypeScript. It is also the foundation for every framework adapter the project will ever write. The same shared optimized codebase serves both.

---

## Rounds 3-15 Commitments Compendium

This compendium consolidates every architectural commitment landed across orchestration rounds 3-15. Each section is normative final-state — phase numbers appear only in PART B cross-references. The earlier sections (§1-§14, §0.5-§0.7) carry the inline edits applied across rounds; this compendium is the consolidated source-of-truth for items spanning multiple sections. It is PART B reference material at landing time.

Items are organized by topic (A.1-A.24) rather than by round, because the same topic was often revisited across rounds and the final commitment supersedes earlier discussion. Where a single topic touches multiple §-sections of the main plan, the compendium row is the authoritative cross-section statement.

### A.1 StructuredTypeExpression — Final 22-Arm Closed Schema

The `oneof StructuredTypeExpression.kind` is closed at **22 arms**: `Reference`, `Union`, `Intersection`, `IndexedAccess`, `KeyOf`, `TypeOf`, `Tuple`, `Array`, `Object`, `Mapped`, `Conditional`, `Literal`, `Primitive`, `TemplateLiteral`, `Infer`, `Function`, `Class`, `ThisType`, `Satisfies`, `UniqueSymbol`, `NoInfer`, `LocalTypeRef`. Reserved range 23-100 for additive growth (bump `schema_version` on add). §5.6 dispatch table maps EVERY arm 1:1 — no ellipses. Guard `structured_type_expression_dispatch_table_complete` (Phase 0a) statically asserts cardinality equality between the proto source and the dispatch table source.

### A.2 §5.6 Dispatch Table — Field-Complete

Every `StructuredTypeExpression` proto field is consumed by §5.6 dispatch. Key dispatch rows (R15 final form):

| Variant | Dispatch behavior |
|---|---|
| `Reference { scope_canonical, name, type_arguments, extra_imports }` | Resolve `name` in `scope_canonical`; recurse `type_arguments`; resolve `extra_imports`. |
| `Union { members }` / `Intersection { members }` | Recurse every member. |
| `IndexedAccess { object, index }` | Recurse `object`, then `index`. |
| `KeyOf { operand }` | Recurse `operand`. |
| `TypeOf { value_root_canonical, path }` | Resolve value declaration; navigate `path`. |
| `Tuple { elements, readonly }` | Recurse each element's `value`. |
| `Array { element, readonly }` | Recurse `element`. |
| `Object { members, index_signatures, call_signatures, construct_signatures }` (R14): Recurse `members[].value`; recurse `index_signatures[].value`; recurse every `call_signatures[]` ExprFunction in source order (preserves overload order); recurse every `construct_signatures[]` ExprFunction in source order. |
| `Mapped { type_param, name_remap, value_type, readonly_modifier, optional_modifier }` (R3 binder): Bind `type_param.binder_id`; recurse `type_param.constraint`; recurse `name_remap` if Some; recurse `value_type`. `LocalTypeRef { binder_id }` in `name_remap`/`value_type` references the bound `binder_id`. |
| `Conditional { check, extends_type, true_branch, false_branch }` | Recurse all four. Open conditionals distribute path; closed conditionals reduce immediately. |
| `Literal { value }` | Encode `LiteralValue`. |
| `Primitive { kind }` | Encode `PrimitiveKind`. |
| `TemplateLiteral { quasis, expressions }` | Recurse each expression. |
| `Infer { name, constraint }` | Bind `name`; recurse `constraint` if Some. |
| `Function { type_parameters, this_param, parameters, return_expr, signature_kind }` (R14 final): Recurse `type_parameters` (constraint + default); recurse `this_param` if Some; recurse `parameters[].type_ref`; dispatch `return_expr.kind`: `Type(t)` → lower to `Signature.return_type`, `Predicate(p)` → populate `Signature.return_predicate`, `Assertion(a)` → populate `Signature.asserts`. `signature_kind` discriminates Call / Construct / AbstractConstruct (R11). |
| `Class { class_name, type_parameters, instance_members, static_members }` | Recurse type parameters and each member. |
| `ThisType {}` | Encode `TypeNode::ThisType`. |
| `Satisfies { value, constraint }` | Recurse both. |
| `UniqueSymbol { decl_canonical, name }` | Resolve declaration. |
| `NoInfer { inner }` | Recurse `inner`. |
| `LocalTypeRef { binder_id }` | Resolve `binder_id` in current binder scope (set up by enclosing `Mapped`). |

Guard `structured_type_expression_dispatch_table_field_coverage` (R15) — static-scan asserts every proto field on every `StructuredTypeExpression` message is consumed by the dispatch logic.

### A.3 §4.3 CompletionFence + GetOrComputeOutcome + cooperative_admit_with_post_publish

`TypeInfoGraphResultDb::get_or_compute(key, request) -> GetOrComputeOutcome<Arc<TypeInfoGraphPayload>>` where:

```rust
pub enum GetOrComputeOutcome<V> {
    ColdPublish(V),
    WarmHit(V),
    Degraded { error: TypeInfoRequestError, partial: Option<V> },
}
```

Cold path calls `self.completion_fence.publish_with_retry(build, revalidate)` (the struct field is `completion_fence: CompletionFence`, NOT `publication_fence`). The cooperative substrate routes through `cooperative_admit_with_post_publish` with `ComputeAdmission::Cacheable | WarmHit | ReturnOnly { value, error } | Failed`. Degraded recovery uses store-backed `degraded_store.peek_exact_only_partial(&key, request, &error)` (defined in §4.1.2). No `AtomicU8 cold_outcome_flag`, no `last_degraded_error()`, no `degraded_partial_from_fence` helper.

The fact-signature path in §10.1: `let (outcome, fact_read_set) = ctx.with_fact_tracer(|| db.get_or_compute(slot_key, &request)); let admission = SignatureAdmission::from_finalise(fact_read_set.finalise());`. Per `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1066`.

### A.4 TypeInfoRequestError — Unified Closed Union

```ts
type TypeInfoRequestError =
  | { kind: "MissingProjectionContext" }
  | { kind: "MissingDisplayPolicy" }
  | { kind: "InvalidMode"; mode: string }
  | { kind: "MissingClosurePolicy" }
  | { kind: "UnknownSchemaVersion"; clientVersion: number; serverSupportedVersions: number[] }
  | { kind: "MalformedPayload"; detail: string }
  | { kind: "OmittedRoots" }
  | { kind: "UnstableState"; attempts: number }
```

Rust mirror: `client_version: u32, server_supported_versions: Vec<u32>`. Canonical shape (per round 4 reconciliation). Guard `typeinfo_request_error_union_is_consistent_across_sections` extends to scan documentation sections as well.

### A.5 Schema Version — Negotiation + Per-Request Echo + Downlevel Encoding

Every public request DTO carries `schemaVersion: number` (required, must match handshake-negotiated version). Servers advertise `supported_versions = [N, N-1, N-2]` ONLY for versions backed by a registered encoder. `SchemaVersionCapabilities::validated_supported_versions()` returns the validated set. Guard `server_supported_versions_have_encoders` asserts `capabilities.supported == capabilities.validated_supported_versions()`. When a session negotiates version V < current, the server emits via `encode_typeinfo_payload_for_version(V, payload)`. Newer-only variants project to compatible substitutes (e.g., post-V `ExpansionStatus::ExactOpenGeneric` may project to `ExactSymbolic { reason: GenericPreserved }` for v(N-1) consumers). If V supports `UnsupportedConstruct::DowngradedFromNewerSchema`, emit that; else downgrade to older fallback (`UnsupportedConstruct::Unsupported { construct: "schema_skew" }`); else degrade to `Opaque(Miss { reason: SchemaSkew })`. Guard `downgrade_encoder_never_emits_variant_unknown_to_target_version` validates each encoder against the `KNOWN_VARIANTS_AT_VERSION[target_version]` table (NOT `encoder.version`).

### A.6 KNOWN_VARIANTS_AT_VERSION — Cumulative Exhaustive Sets

The table is the truth; proto `since_schema_version` annotations must match. Each row is an EXHAUSTIVE enumeration (not cumulative deltas):

```rust
pub const KNOWN_VARIANTS_AT_VERSION: phf::Map<u32, &'static [VariantId]> = phf::phf_map! {
    1u32 => &[
        VariantId::PrimitiveNode, VariantId::LiteralNode, VariantId::UnionNode,
        VariantId::IntersectionNode, VariantId::ObjectNode, VariantId::ArrayNode,
        VariantId::TupleNode, VariantId::ReferenceNode, VariantId::AliasInstantiationNode,
        VariantId::TypeParameterNode, VariantId::KeyOfNode, VariantId::IndexedAccessNode,
        VariantId::ConditionalNode, VariantId::MappedNode, VariantId::TemplateLiteralNode,
        VariantId::TypeOfNode, VariantId::SatisfiesNode, VariantId::ClassNode,
        VariantId::ThisTypeNode, VariantId::MergedDeclarationNode, VariantId::AmbientModuleNode,
        VariantId::ModuleAugmentationNode, VariantId::AmbientNamespaceNode,
        VariantId::GlobalAugmentationNode, VariantId::RelationProofNode, VariantId::OpaqueNode,
        VariantId::UniqueSymbolNode, VariantId::InferNode, VariantId::UnresolvedGeneric,
        VariantId::EnumNode, VariantId::CycleNode,
        VariantId::MemberKindField, VariantId::MemberKindMethod, VariantId::MemberKindGetter,
        VariantId::MemberKindSetter,
        VariantId::UnsupportedDecorator, VariantId::UnsupportedUmdGlobal,
        VariantId::UnsupportedLegacyTypeguard, VariantId::UnsupportedLegacyConstAssertOutsideExpression,
        VariantId::SignatureKindCall, VariantId::SignatureKindConstruct,
        VariantId::FunctionReturnType, VariantId::FunctionReturnPredicate, VariantId::FunctionReturnAssertion,
        VariantId::PredicateSubjectIdentifier, VariantId::PredicateSubjectThis,
        VariantId::AssertionEffectIdentifier, VariantId::AssertionEffectThis, VariantId::AssertionEffectCondition,
        VariantId::RelationFailureParameterContravariance, VariantId::RelationFailureMissingProperty,
        VariantId::RelationFailureIncompatibleReturn, VariantId::RelationFailureIncompatibleConstructSignature,
        VariantId::RelationFailureIncompatibleIndexSignature, VariantId::RelationFailurePrivateProtectedMismatch,
        VariantId::RelationFailureExcessProperty, VariantId::RelationFailureReadOnlyMismatch,
        VariantId::RelationFailureOptionalityMismatch,
    ],
    2u32 => &[
        // v1 entries (re-listed in full) PLUS:
        VariantId::NoInferNode, VariantId::JsxIntrinsicElementNode,
        VariantId::MemberKindAutoAccessor, VariantId::UnsupportedSchemaSkew,
        VariantId::SignatureKindAbstractConstruct,
    ],
    3u32 => &[
        // v1 + v2 entries (re-listed in full) PLUS:
        VariantId::UnsupportedDowngradedFromNewerSchema, VariantId::ExactOpenGeneric,
    ],
};
```

`VariantId` covers ONLY closed oneof arms: `TypeNode`, `StructuredTypeExpression`, `ExpansionStatus`, `UnsupportedConstruct`, `MemberKind`, `NarrowingCause`, `ContextPosition`, `FunctionReturnExpr`, `PredicateSubject`, `AssertionEffectExpr`, `RelationFailureReason`. Plain proto enum values (e.g., new `PrimitiveKind` arm) require a separate registration mechanism if ever needed — for v1..v3 this is not required (R14 narrows generator scope; the `enum_value_since_schema_version` claim is removed).

Generator: `scripts/gen-known-variants-table.rs` reads proto AST with per-arm `[(verter.since_schema_version) = N]` custom options and emits the inverse map. Guard `known_variants_at_version_rows_are_cumulative_exact_sets` (R12). Guard `known_variants_table_matches_proto_at_version` regenerates the table at CI and asserts byte-identical match. Every non-v1 proto oneof arm carries `[(verter.since_schema_version) = N]` annotation; missing annotations default to v1.

### A.7 MappedTypeParam + NoInfer + TypeOperand + JSX + OptionalSemantics + DiagnosticDirective

`MappedTypeParam { name, constraint, symbol }` — binder identity carried on every mapped node. `name_remap` and `value_type` reference `type_param.symbol`. Fixture `mapped_remap_uses_bound_key_in_value_and_template.rs`.

`NoInfer` is OCCURRENCE-LOCAL (on parameter occurrence), NOT on type parameter declaration. `TypeNode::NoInfer { inner: TypeNodeId }`. `SignatureParameter.inference_policy: InferencePolicy { Normal, NoInfer }`. The `no_infer: bool` is removed from `TypeParameter`. Fixture `no_infer_is_occurrence_local_on_overload_set.rs`, `no_infer_ambient_declaration_defaults_normal.rs`.

`TypeOperand` sum type for `relate` / `projectPath`:

```ts
type TypeOperand =
  | { kind: "node"; graph: GraphHandle; node: TypeNodeId }
  | { kind: "symbol"; canonicalId: string; name: string; symbolSpace: SymbolSpace; typeArguments?: StructuredTypeExpression[] }
  | { kind: "expression"; scopeCanonical: string; expression: StructuredTypeExpression; extraImports?: ImportSpec[] }
```

JSX intrinsic elements are first-class semantic queries: `SemanticQueryKey::ResolveJsxIntrinsicElement { namespace, tag }`, `SemanticQueryKey::ResolveJsxAttribute { element, name }`. `TypeNode::JsxIntrinsicElement { namespace, tag, attributes, children }`. `UnsupportedConstruct::JsxIntrinsicHostElement` is removed. Fixtures `jsx_intrinsic_element_attrs_resolve_from_provider.rs`, `jsx_intrinsic_empty_project_returns_miss_not_unsupported.rs`.

`OptionalSemantics { Required | MissingOnly | MissingOrUndefined }` on `ObjectMember.optional` (replaces `optional: bool`). CORRECT classification (R4): `prop: T` → Required; `prop: T | undefined` → Required with value union (NOT MissingOrUndefined); `prop?: T` under `exactOptionalPropertyTypes: false` → MissingOrUndefined; `prop?: T` under `exactOptionalPropertyTypes: true` → MissingOnly. Fixture `optional_required_undefined_is_not_missing.rs`.

`DiagnosticDirective { kind: TsExpectError | TsIgnore, span, applies_to: Option<DiagnosticId>, consumed: bool }` lives PAYLOAD-ONLY on `TypeInfoGraphPayload.diagnostic_directives` — NOT inside `SemanticTypeGraph` (R7 separation; R8 reaffirmation). `DiagnosticId(u32)` indexes `TypeInfoGraphPayload.diagnostics` (NOT `SemanticTypeGraph.diagnostics` — diagnostics are payload-only, R8 C3 Option A). Guard `diagnostics_only_on_typeinfo_graph_payload`. Unused `TsExpectError` projects to synthetic `TypeInfoDiagnostic { code: 2578, severity: Error }`; unused `TsIgnore` emits NO diagnostic. Fixture `ts_expect_error_unused_projects_ts2578_diagnostic.rs`. Wire round-trip guard preserves `applies_to` index-range validation and `consumed` round-trip (proto3 cannot distinguish empty-vs-absent — that assertion is removed).

### A.8 ObjectMember + Signature — TS-Replacement-Grade Surface

```rust
pub enum MemberKind {
    Field,
    Method { signatures: Vec<SignatureRef> },
    Getter { return_type: TypeNodeId },
    Setter { param_type: TypeNodeId },
    AutoAccessor { read_type: TypeNodeId, write_type: TypeNodeId },
}

pub struct ObjectMember {
    name: InternedName,
    name_kind: MemberNameKind,
    kind: MemberKind,
    value: TypeNodeId,
    optional: OptionalSemantics,
    readonly: bool,
    accessibility: Accessibility,
    static_side: bool,
    declaration: Option<SymbolId>,
    jsdoc: Option<JsdocMeta>,
}

pub enum SignatureOrigin {
    FunctionDeclaration,
    MethodDeclaration,
    Constructor,
    CallSignature,
    ConstructSignature,
    IndexSignature,
    GetterAccessor,
    SetterAccessor,
}

pub struct Signature {
    type_parameters: Vec<TypeParameter>,
    parameters: Vec<SignatureParameter>,
    return_type: TypeNodeId,
    this_param: Option<SignatureParameter>,
    return_predicate: Option<TypePredicate>,
    asserts: Option<AssertionEffect>,
    origin: SignatureOrigin,
    parameter_variance_policy: ParameterVariancePolicy,
}

pub enum ParameterVariancePolicy {
    Strict,
    Bivariant,
}
```

**Variance producer-mapping rules (TS-correct, R8 + R9 + R10 final form):**

| SignatureOrigin | strictFunctionTypes | ParameterVariancePolicy |
|---|---|---|
| FunctionDeclaration | true | Strict |
| FunctionDeclaration | false | Bivariant |
| MethodDeclaration | true | Bivariant (TS leaves method syntax bivariant under strict — R8 C1 correction) |
| MethodDeclaration | false | Bivariant |
| Constructor | true | Bivariant (TS 2.6 excludes constructors with methods — R9 C1 correction) |
| Constructor | false | Bivariant |
| CallSignature (function-property) | true | Strict |
| CallSignature (function-property) | false | Bivariant |
| ConstructSignature | true | Strict |
| ConstructSignature | false | Bivariant |

Fixtures: `method_vs_function_property_variance_under_strict_function_types.rs`, `constructor_bivariance_exception_under_strict_function_types.rs`, `strict_function_types_contravariant_parameters.rs`, `method_bivariance_exception.rs`, `incompatible_call_signature_intersection.rs`.

Proto wire shape (R8 C6):

```protobuf
message ObjectMemberNode {
  string name = 1;
  MemberNameKind name_kind = 2;
  oneof kind {
    MemberKindField field = 3;
    MemberKindMethod method = 4;
    MemberKindGetter getter = 5;
    MemberKindSetter setter = 6;
    MemberKindAutoAccessor auto_accessor = 7;
  }
  uint32 value = 8;
  OptionalSemantics optional = 9;
  bool readonly = 10;
  Accessibility accessibility = 11;
  bool static_side = 12;
  optional uint32 declaration = 13;
}
message MemberKindField {}
message MemberKindMethod          { repeated uint32 signatures = 1; }
message MemberKindGetter          { uint32 return_type = 1; }
message MemberKindSetter          { uint32 param_type = 1; }
message MemberKindAutoAccessor    { uint32 read_type = 1; uint32 write_type = 2; }

message UnsupportedConstruct {
  oneof kind {
    UnsupportedDecorator decorator = 1 [(verter.since_schema_version) = 1];
    UnsupportedUmdGlobal umd_global = 2 [(verter.since_schema_version) = 1];
    UnsupportedLegacyTypeguard legacy_typeguard = 3 [(verter.since_schema_version) = 1];
    UnsupportedLegacyConstAssertOutsideExpression legacy_const_assert = 4 [(verter.since_schema_version) = 1];
    UnsupportedDowngradedFromNewerSchema downgraded_from_newer_schema = 5 [(verter.since_schema_version) = 3];
    UnsupportedSchemaSkew schema_skew = 6 [(verter.since_schema_version) = 2];
  }
}
message UnsupportedDowngradedFromNewerSchema {
  uint32 added_in_version = 1;
  uint32 current_negotiated_version = 2;
}
```

### A.9 RelationOutcome — Closed Reason With Deterministic Priority

```rust
pub enum RelationFailureReason {
    ParameterContravariance { parameter_index: u32 },
    MissingProperty { name: InternedName },
    IncompatibleReturn,
    IncompatibleConstructSignature,
    IncompatibleIndexSignature,
    PrivateProtectedMismatch,
    ExcessProperty { name: InternedName },
    ReadOnlyMismatch,
    OptionalityMismatch,
}

pub enum RelationOutcome {
    Assignable,
    NotAssignable {
        primary_reason: RelationFailureReason,
        secondary_reasons: Vec<RelationFailureReason>,
    },
    Unknown,
}
```

R10 C4 — `primary_reason` is deterministic by priority order: `PrivateProtectedMismatch > MissingProperty > IncompatibleConstructSignature > IncompatibleIndexSignature > IncompatibleReturn > ParameterContravariance > ReadOnlyMismatch > OptionalityMismatch > ExcessProperty`. `secondary_reasons` preserves all observed failures. Fixture `relation_failure_reason_priority_is_stable.rs`. Proto mirror: `RelationOutcomeNode.NotAssignable { primary_reason: RelationFailureReason; secondary_reasons: repeated RelationFailureReason }`.

### A.10 cycle_id + canonicalize_substitutions + CanonicalSubstitutionValueKey + LiteralValueKey + structural_object_fingerprint

```rust
pub fn cycle_id(
    decl: &ResolvedDeclSlotIdentity,
    substitutions: &[SubstitutionKey],
) -> Result<u64, QueryErrorDto>;

pub struct SubstitutionKey {
    pub type_param_slot: ResolvedDeclSlotIdentity,
    pub value: CanonicalSubstitutionValueKey,
}

pub enum CanonicalSubstitutionValueKey {
    Symbol { decl: ResolvedDeclSlotIdentity, type_arguments: Arc<[CanonicalSubstitutionValueKey]> },
    Literal { value: LiteralValueKey },
    Primitive { kind: PrimitiveKind },
    UniqueSymbol { decl: ResolvedDeclSlotIdentity },
    Union { members: Arc<[CanonicalSubstitutionValueKey]> },
    Intersection { members: Arc<[CanonicalSubstitutionValueKey]> },
    TypeParam { id: TypeParameterId },
    StructuralObject { fingerprint: Hash16 },
}

pub fn canonicalize_substitutions(
    substitutions: &[SubstitutionKey],
) -> Result<Arc<[CanonicalSubstitutionPair]>, CanonicalizationError>;

pub struct CanonicalSubstitutionPair {
    pub type_param: TypeParameterId,
    pub value: CanonicalSubstitutionValueKey,
}

pub enum CanonicalizationError {
    ConflictingBindings { type_param: TypeParameterId },
}

/// R14 C1 — HOST-STABLE identity. NOT a typedef of wire LiteralValue.
/// Wire InternedName(u32) is a payload-local string-table index; using it in cache keys
/// causes warm-cache aliasing across semantically-different types.
pub enum LiteralValueKey {
    String(Arc<str>),       // host-stable string identity
    Number(F64Bits),        // bit-pattern stable (NaN canonicalized)
    Boolean(bool),
    BigInt(Arc<str>),       // host-stable string identity for arbitrary-precision values
}

pub fn structural_object_fingerprint(obj: &CanonicalStructuralObject) -> Hash16;

pub struct CanonicalStructuralObject {
    pub members: Vec<CanonicalStructuralMember>,
    pub index_signatures: Vec<CanonicalIndexSignature>,
    pub call_signatures: Vec<CanonicalSignatureKey>,     // R14 C2: PRESERVE source order (overload order is meaning-affecting)
    pub construct_signatures: Vec<CanonicalSignatureKey>, // R14 C2: PRESERVE source order
}

pub struct CanonicalSignatureKey {
    pub overload_ordinal: u16,                          // R14: explicit index in declaration order
    pub origin: SignatureOrigin,
    pub parameter_variance_policy: ParameterVariancePolicy,
    pub type_parameters: Vec<(TypeParameterId, Option<CanonicalSubstitutionValueKey>)>,
    pub parameters: Vec<CanonicalParameterKey>,
    pub return_type: CanonicalSubstitutionValueKey,
}
```

`CanonicalizationError::ConflictingBindings` maps to `QueryErrorDto::UnstableState { attempts: 0 }`. Fixtures:
- `cycle_id_propagates_canonicalization_conflicts.rs`
- `recursive_generic_cycle_distinguishes_type_arguments.rs` (`Box<string>` vs `Box<number>` produces distinct `cycle_id`)
- `substitution_canonicalization_distinguishes_nested_generic_arguments.rs`
- `structural_object_fingerprint_overload_order_matters.rs`
- `structural_object_fingerprint_signature_parameter_type_matters.rs`

Guards: `literal_value_key_is_independent_of_wire_string_table` (property-test: `LiteralValueKey::String(Arc::from("hello"))` produces consistent hashes across distinct `SemanticTypeGraph` payload instantiations), `structural_object_fingerprint_is_member_sensitive`. `CanonicalSubstitutionValueKey` is the single carrier — `SubstitutionConcrete` is retired (R13 C2). Sweep `SubstitutionValueKey` (without "Canonical" prefix) replaced consistently (R14 CX2).

### A.11 ProgramAnalysisGraph + FlowNarrowing + ContextualType

Per R7 P2-2 — `FlowNarrowing` and `ContextualType` move OUT of `TypeNode` into a sibling `ProgramAnalysisGraph` carrier on `TypeInfoGraphPayload`. `TypeNode` carries only type values (R7 guard `type_node_contains_only_type_values`):

```rust
pub struct TypeInfoGraphPayload {
    pub graph: SemanticTypeGraph,
    pub program_analysis: Option<ProgramAnalysisGraph>,
    pub diagnostics: Vec<TypeInfoDiagnostic>,
    pub diagnostic_directives: Vec<DiagnosticDirective>,
}

pub struct ProgramAnalysisGraph {
    pub flow_narrowings: Vec<FlowNarrowing>,
    pub contextual_types: Vec<ContextualType>,
}
```

`ProgramAnalysisGraph` is populated only when the request's closure is `GraphClosurePolicy::ProjectionRequired { projection: FlowNarrowing | ContextualType }`. Guard `program_analysis_graph_gated_by_projection_required`.

Proto messages (R13 C5):

```protobuf
message FlowNarrowing {
  uint32 base = 1;
  uint32 narrowed = 2;
  NarrowingCause cause = 3;
  SpanRef span = 4;
}

message NarrowingCause {
  oneof kind {
    TypeofGuard typeof_guard = 1;
    InGuard in_guard = 2;
    InstanceofGuard instanceof_guard = 3;
    EqualityGuard equality_guard = 4;
    TruthinessGuard truthiness_guard = 5;
    UserPredicate user_predicate = 6;
    AssertionEffectCause assertion_effect = 7;
    AssignmentFlow assignment_flow = 8;
    OptionalChainNullish optional_chain_nullish = 9;
    DiscriminantUnion discriminant_union = 10;
  }
}

message TypeofGuard           { PrimitiveKind target = 1; bool negated = 2; }
message InGuard               { uint32 property = 1; bool negated = 2; }
message InstanceofGuard       { uint32 ctor = 1; bool negated = 2; }
message EqualityGuard         { uint32 against = 1; EqualityKind kind = 2; }
message TruthinessGuard       { bool negated = 1; }
message UserPredicate         { uint32 signature = 1; bool negated = 2; }
message AssertionEffectCause  { uint32 signature = 1; }
message AssignmentFlow        { uint32 new_type = 1; }
message OptionalChainNullish  {}
message DiscriminantUnion     { uint32 discriminant = 1; uint32 selected = 2; }

message ContextualType {
  uint32 contextual = 1;
  ContextPosition target_position = 2;
  repeated TypeParameterBinding inference_bindings = 3;
}

message ContextPosition {
  oneof kind {
    JsxAttributePos jsx_attribute = 1;
    CallArgumentPos call_argument = 2;
    ObjectLiteralPropertyPos object_literal_property = 3;
    ArrayLiteralElementPos array_literal_element = 4;
    ReturnExpressionPos return_expression = 5;
  }
}

message JsxAttributePos {}
message CallArgumentPos             { uint32 idx = 1; }
message ObjectLiteralPropertyPos    { uint32 name = 1; }
message ArrayLiteralElementPos      { uint32 idx = 1; }
message ReturnExpressionPos {}
```

`SpanRef` and `TypeParameterBinding` are imported from `verter/v1/common.proto` (R14 CX4). TS API methods (R12 C2 Option A): both `evaluateFlowNarrowingAt(...)` and `evaluateContextualTypeAt(...)` return `Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>`. The payload includes both `graph: SemanticTypeGraph` and `program_analysis: Option<ProgramAnalysisGraph>`. SDK consumers resolve `TypeNodeId`s through `payload.graph.nodes`.

### A.12 ExactOpenGeneric — Warm-Cacheable

```rust
pub enum ExpansionStatus {
    ExactResolved,
    ExactSymbolic { reason: SymbolicPreservationReason },
    ExactOpenGeneric { blockers: Vec<Blocker>, faithful: bool },    // R7-EXT P1-2: warm-cacheable
    Partial { diagnostics: Vec<TypeInfoDiagnostic> },
    BudgetExceeded { kind: BudgetKind },
    UnstableState,
    Unsupported { construct: UnsupportedConstruct },
    Opaque { error: QueryErrorDto },
}
```

`ExactResolved`, `ExactSymbolic`, `ExactOpenGeneric` are warm-admissible to the publication fence (R7-EXT 5-gate contract). Others route to `DegradedResultStore`. Guard `popover_slot_props_unresolved_warm_admits_as_exact_open_generic`. `VariantId::ExactOpenGeneric` is a SINGLE arm (R11 C3) — the `faithful: bool` discriminator is content within the variant, not a separate `VariantId`.

### A.13 SDK Parser — `@verter/typeinfo/parse`

Per R7-EXT P1-3 — SDK-side text-to-`StructuredTypeExpression` parser. NOT the resolver. Producer of typed DTOs only. R9 CX4 — return both versions for compatibility check:

```ts
export interface ParseStructuredTypeExpressionRequest {
  typeText: string;
  scopeCanonical: string;
  extraImports?: ImportSpec[];
  schemaVersion: number;
}

export interface ParseStructuredTypeExpressionResult {
  expression: StructuredTypeExpression | null;
  diagnostics: ParseDiagnostic[];
  producerSchemaVersion: number;       // R9 CX4: SDK's compiled-in schema version
  requiredSchemaVersion: number;       // R9 CX4: minimum version emitted DTO requires
}

export type ParseDiagnosticCode =
  | "syntax_error"
  | "unsupported_construct"
  | "schema_version_skew"
  | "unknown_identifier"
  | "invalid_reference";

export interface ParseDiagnostic {
  message: string;
  span: { start: number; end: number };
  severity: "Error" | "Warning";
  code: ParseDiagnosticCode;
}

export function parseStructuredTypeExpression(
  request: ParseStructuredTypeExpressionRequest,
): ParseStructuredTypeExpressionResult;

export function assertStructuredTypeExpressionCompatible(
  expression: StructuredTypeExpression,
  negotiatedSchemaVersion: number,
): { ok: true } | { ok: false; error: SchemaVersionSkewError };
// Compat check: requiredSchemaVersion <= negotiatedSchemaVersion
```

Guards: `parser_lives_in_sdk_not_resolver` (static scan of `verter_session::typeinfo::*` for parser calls — must be zero), `sdk_parser_result_carries_schema_version`. Fixtures: `sdk_parser_round_trips_via_evaluate.rs`, `sdk_parser_schema_version_skew_reports_warning.rs`. Example call (R7 CX3) at §5.7:

```ts
const payload = await session.evaluateTypeExpressionGraph({
  expression: parsed.expression,
  scopeCanonical: request.scopeCanonical,
  context: { mode: "expanded", demand: "published" },
  closure: { kind: "expanded", nodeBudget: 4096, depthBudget: 64 },
  displayPolicy: { preserveAliasIdentity: true, expandIndexedAccess: "ifPathPrecise", conditionalBranchDisplay: "selected", truncateUnion: null },
  schemaVersion: SESSION_SCHEMA_VERSION,
});
```

### A.14 .proto as Single Wire Source of Truth

Per R7-EXT P2-1 — `.proto` at `crates/verter_protocol/proto/verter/v1/typeinfo.proto` is the SINGLE wire source of truth. Rust DTOs at `crates/verter_protocol/src/typeinfo/generated/*.rs` are GENERATED by `prost-build` (per `build.rs`). TypeScript DTOs at `packages/typeinfo/src/generated/graph.generated.ts` are GENERATED by `protoc-gen-ts` (or equivalent `ts-proto`) from `.proto`, NOT from Rust struct via `ts-rs`. `ts-rs` annotations for typeinfo wire-payload path are removed — they may persist only for non-wire types (audit records, internal carriers). §5.1 wording: "Wire DTOs are generated directly from proto; ts-rs is not used for typeinfo wire payloads." (R11 CX1). Guards: `wire_dtos_generated_only_from_proto` (file header includes `// @generated-do-not-edit`), `ts_rs_not_applied_to_wire_dtos` (static scan: zero `#[ts(...)]` attributes on wire DTOs), `proto_closed_enums_declared_not_raw_uint32` (R9 C3), `proto_no_duplicate_enum_declarations` (R10 C3).

### A.15 FrameworkAdapterRegistry — Open Identity + Closed Surfaces

Per R7-EXT P1-1 (§6.4 of the plan):

```rust
pub struct FrameworkAdapterRegistry {
    descriptors: BTreeMap<FrameworkAdapterId, FrameworkAdapterDescriptor>,
}

impl FrameworkAdapterRegistry {
    pub fn register(&mut self, wire: FrameworkAdapterDescriptorWire) -> Result<(), RegistryError> {
        // R7 C2 Option A — validate RAW wire discriminants BEFORE typed conversion
        for raw in &wire.supported_surface_discriminants {
            FrameworkSurfaceKind::try_from_discriminant(*raw)
                .ok_or(RegistryError::UnknownSurfaceKindDiscriminant { wire_value: *raw })?;
        }
        // ... typed conversion ...
        Ok(())
    }
}

pub enum RegistryError {
    DuplicateAdapter { id: FrameworkAdapterId },
    UnknownSurfaceKindDiscriminant { wire_value: u32 },
    CaseAlias { canonical: FrameworkAdapterId, attempted: String },
}
```

Guard `framework_adapter_registry_rejects_unknown_surface_kind_discriminant`. Fixture submits a wire descriptor with `wire_value: 9999`, asserts `RegistryError::UnknownSurfaceKindDiscriminant`.

### A.16 TS API — Typed Errors via AuditedResult

Per R11 CX3 — every public `TypeInfoSession` method returns `Promise<AuditedResult<T, TypeInfoRequestError>>`:

```ts
export interface TypeInfoSession {
  listSymbols(canonicalId: string): SymbolEntry[];
  resolveSymbolGraph(req: ResolveSymbolGraphRequest): Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>;
  evaluateTypeExpressionGraph(req: EvaluateTypeExpressionGraphRequest): Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>;
  projectPathGraph(req: ProjectPathGraphRequest): Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>;
  relate(req: RelateRequest): Promise<AuditedResult<RelationPayload, TypeInfoRequestError>>;
  evaluateFlowNarrowingAt(req: FlowNarrowingRequest): Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>;
  evaluateContextualTypeAt(req: ContextualTypeRequest): Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>;
  expandGraphAround(req: ExpandGraphAroundRequest): Promise<AuditedResult<TypeInfoGraphPayload, TypeInfoRequestError>>;
  frameworkSurface(req: FrameworkSurfaceRequest): Promise<AuditedResult<FrameworkSurfacePayload, TypeInfoRequestError>>;
}
```

Matches native `_with_audit` API at §5.3. SDK consumers can mechanically switch on `result.value.kind === "ok"` vs `"err"` and discriminate `TypeInfoRequestError` variants.

### A.17 §10 Audit Carrier — TypeInfoGraphPayloadAudit Field List

```rust
pub struct TypeInfoGraphPayloadAudit {
    // existing fields ...
    pub program_analysis_emitted: bool,             // R7 CX1
    pub exactness_exact_open_generic: u32,          // R7 CX1 — count of ExactOpenGeneric variants in payload
    pub schema_skew_miss_count: u32,                // R9 CX1 — count of SchemaSkewMiss degradations
    // ... (other exactness counters)
}

pub enum DegradationReasonTag {
    BudgetExceeded,
    UnstableState,
    Unsupported,
    Miss,
    Partial,
    Cycle,
    SchemaSkewMiss,                                  // R9 CX1
}

pub enum StructuredAuditEvent {
    TypeInfoGraphPublished { layer: AuditLayerName, audit: TypeInfoGraphPayloadAudit },
    TypeInfoGraphDegraded  { layer: AuditLayerName, audit: TypeInfoGraphPayloadAudit, reason: DegradationReasonTag },
    // ... existing variants
}
```

§10.1 audit pseudocode dispatches on `GetOrComputeOutcome`:
- `ColdPublish(payload)` → emit `TypeInfoGraphPublished { layer, audit }`.
- `WarmHit(payload)` → emit `TypeInfoGraphPublished { layer, audit }` (audit reflects warm-hit metadata).
- `Degraded { error, partial }` → emit `TypeInfoGraphDegraded { layer, audit, reason: classify_error(&error) }`.

Per R9 — drop the side-channel `audit_tag: "SchemaSkewMiss"` string; use the typed `DegradationReasonTag::SchemaSkewMiss` instead.

### A.18 §0.5 / §0.6 Disposition Rows — Visibility / Phase / Drift

- `crates/verter_session/src/semantic_query_memo/inflight.rs:226` → EXTEND visibility of `MAX_INFLIGHT_RETRIES` from `pub(super) const` to `pub(crate) const` so the new `TypeInfoSession`-owned `CompletionFence` adapter at `crates/verter_session/src/typeinfo/completion_fence.rs` can consume the canonical constant. Guard `completion_fence_uses_max_inflight_retries_constant` requires this visibility. Phase 0b/1.
- `crates/verter_session/src/.../semantic_query.rs:833` (line drift from R4 cite: was `:826`) — `with_fact_tracer` pattern unchanged.

### A.19 §7 Projections — Cycle Row Consumes Resolved Payload (No Native Call)

Per R14 CX1 — §7.1 (Zod) and §7.2 (JSON Schema) projections consume the RESOLVED `Cycle { cycle_id }` and `Opaque(UnstableState)` nodes — they do NOT call `cycle_id(...)` from TS:

- `Cycle { cycle_id }` → emit `z.lazy(() => memo.get(cycle_id))` (Zod) / `{ "$ref": "#/$defs/" + cycle_id }` (JSON Schema).
- `Opaque(QueryErrorDto::UnstableState { attempts })` → emit `z.unknown()` with diagnostic `unstableCycle` (Zod) / `{}` with same diagnostic (JSON Schema).

Normative mapping tables live IN §7 (R10 C5) — references to a non-existent "prior draft" are eliminated.

### A.20 Phase Plan — Phase 4 Deletes Descriptor Bridge

Per R5 C1 Option A — `descriptor-to-native.ts` / `native-to-descriptor.ts` are deleted in Phase 4. Phase 3 implements `@verter/typeinfo/projections/type-descriptor` BEFORE migrating consumers; Phase 4 deletes the legacy bridge files. R3 CL3 phase mis-cite at §8.4 line 2058 ("Phase 2") → "Phase 4". §8.1 Phase 0a step 9 adds:

```
9. Add crates/verter_audit/src/structured_event.rs:
   - StructuredAuditEvent::TypeInfoGraphPublished { layer, audit }
   - StructuredAuditEvent::TypeInfoGraphDegraded { layer, audit, reason }
   - pub enum DegradationReasonTag { BudgetExceeded, UnstableState, Unsupported, Miss, Partial, Cycle, SchemaSkewMiss }
```

`RetentionGate` field type at §4.1.1 → `parking_lot::RwLock<()>` (matching `component_meta_caches.rs:2714` pattern).

§8.2 step 2b (R11 C2): proto `Signature` adds `SignatureOrigin origin` and `ParameterVariancePolicy parameter_variance_policy` (closed proto enum types, NOT `uint32`).

### A.21 Declaration-Merging Fixtures (R7 CX7)

Added to §9.5 + §8.2 fixture list:
- `class_namespace_merge_preserves_three_spaces.rs` (class + namespace)
- `enum_namespace_merge_preserves_value_namespace.rs` (enum + namespace)

### A.22 Phase Archaeology Sweep (R8 C4 + R9 C4 + R10 C5)

PART A (final-state architecture sections §1-§7, §10-§14) contains NONE of: `formerly`, `moved`, `migration`, `replaces prior`, `retired carrier`, `NO LONGER`, `Effective version N`, `Revision N`, `previously`, `prior draft`, `from the prior draft`, `per the prior draft`, `retired`, `prior wire schemas`, `legacy boundary`, `deleted in Phase 0b/1` (when not in §8 phase plan). PART B (implementation playbook: §0, §0.5, §0.6, §0.7, §8, §9, §13) may reference phases.

Specific line corrections applied:
- §3.1 — strip "Revision-9 cleanup: prior duplicated carrier..." comment; replace with neutral "diagnostic_directives lives at the payload envelope level (not inside SemanticTypeGraph) because directives are span-scoped pragmas, not type-graph state."
- §3.2.1 / §3.2.10 — strip "NO LONGER", "moved", "Effective version 8" residuals.
- §5.1 — replace "ts-rs annotations are RETIRED" with "Wire DTOs are generated directly from proto; ts-rs is not used for typeinfo wire payloads."
- §3.2 / §6.1 — strip `unimplemented!()` body from production-shaped snippets; replace with exhaustive match sketch or explicit pseudocode marker comment.

### A.23 Final Invariants Table — Additions

Append to §14 final invariants:

| Invariant | Guard | Phase added |
|---|---|---|
| `StructuredTypeExpression` is exactly 22 closed arms | `structured_type_expression_dispatch_table_complete` | 0a |
| `StructuredTypeExpression` dispatch consumes every proto field | `structured_type_expression_dispatch_table_field_coverage` | 0b/1 |
| `cycle_id` is substitution-aware | `cycle_id_propagates_canonicalization_conflicts` | 0b/1 |
| `LiteralValueKey` is host-stable (not wire `InternedName`) | `literal_value_key_is_independent_of_wire_string_table` | 0a |
| Overload order preserved in `structural_object_fingerprint` | `structural_object_fingerprint_is_member_sensitive` | 0b/1 |
| TS API methods return `AuditedResult<T, E>` | `ts_api_methods_return_audited_result` | 3 |
| Diagnostics live payload-only | `diagnostics_only_on_typeinfo_graph_payload` | 0a |
| `TypeNode` carries only type values | `type_node_contains_only_type_values` | 0a |
| `ProgramAnalysisGraph` gated by projection-required closure | `program_analysis_graph_gated_by_projection_required` | 0b/1 |
| Closed proto enums declared (not `uint32`) | `proto_closed_enums_declared_not_raw_uint32` | 0a |
| No duplicate proto enum declarations | `proto_no_duplicate_enum_declarations` | 0a |
| `KNOWN_VARIANTS_AT_VERSION` is cumulative exhaustive | `known_variants_at_version_rows_are_cumulative_exact_sets` | 0a |
| Downgrade encoder checks `target_version` | `downgrade_encoder_never_emits_variant_unknown_to_target_version` | 0b/1 |
| Server-advertised versions all have encoders | `server_supported_versions_have_encoders` | 0b/1 |
| SDK parser carries `producerSchemaVersion` + `requiredSchemaVersion` | `sdk_parser_result_carries_schema_version` | 3 |
| SDK parser lives outside the resolver | `parser_lives_in_sdk_not_resolver` | 3 |
| `FrameworkAdapterId` accepts open canonical ids | `framework_surface_request_accepts_open_adapter_id` | 0a |
| `FrameworkAdapterId::canonicalize` rejects case aliases | `framework_adapter_id_canonicalization_rejects_case_alias` | 0b/1 |
| Framework adapter registry rejects unknown surface-kind discriminants | `framework_adapter_registry_rejects_unknown_surface_kind_discriminant` | 0b/1 |
| Constructor variance is `Bivariant` regardless of `strictFunctionTypes` | `constructor_bivariance_exception_under_strict_function_types` | 0b/1 |
| Method variance is `Bivariant` regardless of `strictFunctionTypes` | `method_vs_function_property_variance_under_strict_function_types` | 0b/1 |
| `RelationFailureReason` priority is deterministic | `relation_failure_reason_priority_is_stable` | 0b/1 |
| PART A carries no phase archaeology | `part_a_carries_no_phase_archaeology` | 0a |
| `CompletionFence` consumes canonical `MAX_INFLIGHT_RETRIES` | `completion_fence_uses_max_inflight_retries_constant` | 0b/1 |
| Wire DTOs generated only from proto | `wire_dtos_generated_only_from_proto` | 0a |
| `ts-rs` not applied to wire DTOs | `ts_rs_not_applied_to_wire_dtos` | 0a |
| `TypeInfoRequestError` shape uniform across plan + Rust + TS | `typeinfo_request_error_union_is_consistent_across_sections` | 0a |
| `UnknownSchemaVersion` shape uniform across plan | `unknown_schema_version_shape_uniform_across_plan` | 0a |
| Every public typeinfo request DTO carries `schemaVersion` | `every_typeinfo_request_carries_schema_version` | 0a |
| `known_variants_table` regenerated from proto | `known_variants_table_matches_proto_at_version` | CI |

### A.24 Production-Ready Bar (Unchanged Across Rounds)

- Every phase lands code callable in production, audited, exercised by un-`#[ignore]`'d tests with discriminating assertions.
- Cache layers fully validated on warm hits. Bundled `project_config_hash` FORBIDDEN. Five env-hash dimensions on every cache key or wrapper.
- Every fallback path needs a named, observable, audited precondition.
- Audit observability first-class per phase.
- Performance contract: per-cold-path single materialisation; concurrent cold collapse; warm-cache reentry tests per phase.
- Documentation in OWNING skill in the same change.
- Long-horizon goal: gradually replace TypeScript itself. The TS-completeness commitments (binder identity, occurrence-local NoInfer, optional semantics, JSX first-class, method/constructor bivariance, this-params, type predicates, assertion functions, callable object overload order, ExactOpenGeneric warm-cacheable) directly serve this trajectory.

---

End of Rounds 3-15 Commitments Compendium.

---

End of Verter TypeInfo Semantic Graph Plan — Revision 17.
