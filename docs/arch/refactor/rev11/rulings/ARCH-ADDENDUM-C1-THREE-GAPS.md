---
ruling_id: "C1-THREE-GAPS-ADDENDUM"
type: "architecture-ruling"
date: "2026-08-20"
date_source: "file-mtime (in-document: 'Source: bounded architecture challenge... run against program/architecture-lock at 8c2189389', no calendar date stated)"
binds: ["C1"]
source_file: "ARCH-ADDENDUM-C1-THREE-GAPS.md"
summary: "Addendum resolving three execution gaps in the C1 charter left open by C1-FOUR-FORKS: (1) cross-crate trait sealing is impossible as drafted — seal semantic-owned snapshots, no foreign implementations; (2) the proposed file/directory relocation does not close the dependency cut set as-is — enumerates the full move/stay/split disposition per module; (3) the exhaustive-impl 'AttemptOutcome full coverage' proof is invalid under stable Rust — replaced with one closed, non-overridable inherent gateway (TypeInfoCore::attempt over a closed NonFlowOperation enum)."
supersedes: []
superseded_by: []
contradicts: []
notes: "Explicitly states baseline preserved: the C1 charter and all four ARCH-RULING-C1-FOUR-FORKS rulings 'remain binding and were not reopened' — this is additive gap-filling, not a supersession. States no accepted ADR, DAG edge, or program outcome changes under any of the three gap resolutions."
---

# C1 architecture addendum — three execution gaps

**Source:** bounded architecture challenge, read-only, run against `program/architecture-lock`
at `8c2189389`. Commissioned by the maintainer when C1 was held before ratification, scoped to
exactly three discovered execution gaps and forbidden from re-scoping C1 smaller.

**Baseline preserved:** the C1 charter, its research, and all four prior architecture rulings
(`ARCH-RULING-C1-FOUR-FORKS`) remain binding and were not reopened.

**Amendment determination:** no accepted ADR, no DAG edge, and no program outcome changes under any
of the three gap resolutions, so no AMD is required. The extraction into `verter_semantic` and full
behavioural `AttemptOutcome` coverage both remain required.

---

## GAP 1 — Cross-crate trait sealing

Use the repository’s existing idiom exactly:

```rust
// verter_semantic/src/type_info/observations.rs
mod sealed {
    pub trait Sealed {}
}

pub trait ObservationRead: sealed::Sealed {
    fn lookup(&self, key: &ObservationKey) -> ObservationValueRef<'_>;
}

impl sealed::Sealed for ObservationSnapshot {}
impl ObservationRead for ObservationSnapshot { /* ... */ }
```

The private module is the sealing token. Only code inside `verter_semantic` can name `sealed::Sealed`; a foreign `impl ObservationRead for CompileTypeInfo` fails because `CompileTypeInfo: sealed::Sealed` cannot be satisfied, and attempting the marker impl fails with private-module error E0603. This is the same shape used by:

- `ResolverContext`: private marker at `crates/verter_session/src/resolver_core/resolver_context.rs:82`, supertrait at `:161`, sanctioned marker impls at `:745`.
- Public cross-crate `AttachOwnership`: `crates/verter_tsgo_api/src/attach.rs:85`, `:98`, `:113`.
- `NegativeEvidence`: `crates/verter_semantic/src/analysis/framework_facts/mod.rs:35`, `:45`; its sanctioned implementation is at `framework_facts/svelte.rs:178`.
- The existing foreign-implementation compile failure: `crates/verter_session/tests/cases/compile-fail/deferred_callable_is_sealed_to_its_two_consumers.rs:21-38`.

Therefore the legal implementors are semantic-owned types only: production `ObservationSnapshot`, and—if useful—a test-only type whose implementations are written inside `observations.rs`. `verter_compiler`, `verter_session`, integration tests, and external mocks consume an `ObservationSnapshot`; they do not implement the trait. They construct it through a semantic-owned builder from immutable data. No callbacks, closures, host references, or trait objects belong in the snapshot.

`CompileTypeInfo` must compose an `ObservationSnapshot` and call the kernel. If it genuinely must implement the interface, the interface cannot remain sealed: Rust has no friend-crate exception. The same applies to external test doubles. An exhaustive external double and a private-supertrait seal are directly incompatible.

This also falsifies C1-AC-4’s proposed relocated `ResolverContext`: `VerterHost` being local makes the foreign-trait impl orphan-legal, but it cannot implement the foreign trait’s private semantic supertrait (`docs/arch/refactor/rev11/charters/C1.md:149`). Keep the lifecycle `ResolverContext`/`RequestBoundResolverContext` seal and its adapters in `verter_session`; extract their semantic operations behind the semantic-owned snapshot/gateway. This changes the charter’s file-level split at `C1.md:186`, not the kernel extraction or C1 outcome.

Mechanical gates:

- A compile-fail fixture in `verter_semantic/tests/compile-fail/` attempts both foreign marker and observation-interface impls.
- `rg 'impl (sealed::Sealed|ObservationRead)' crates/verter_semantic/src/type_info` has only the sanctioned implementations.
- Compiler/session tests use the public builder or semantic-owned test fixture, never a foreign `impl`.

## GAP 2 — Complete extraction dependency closure

Disproved as a literal file/directory relocation.

The complete direct session-module cut set reached by the named members is:

`build_toolchain_fingerprint`, `cache_runtime`, `capture_token`, `component_meta_audit`, `component_meta_caches`, `component_meta_materialize`, `decl_body_memo`, `decl_lowering`, `fact_emission`, `fact_signature_helpers`, `file_artifact_store`, `flow_return_audit`, `flow_slice_content`, `hash`, `host_executor`, `host_manage`, `host_resolve`, `identity_interner`, `instant`, `intrinsic_registry`, `invalidation_domain`, `locator_identity`, `loop5_instrumentation`, `mapper_binder_registry`, `meta_resolve`, `project_type_store`, `request_context`, `resolved_import_facts`, `semantic_query`, `semantic_query_memo`, `session_view`, `store_view_roots`, `structural_carrier_producer`, `typeinfo`, `types`, and `VerterHost`.

Representative roots are mechanically visible in `project_semantic_dispatch/mod.rs:62-71`, `resolver_core/resolver_context.rs:68-78`, `resolver_store.rs:1-2`, and the module declarations at `crates/verter_session/src/lib.rs:274-369`. The dispositions are:

- Move into `verter_semantic`: `semantic_query`, `semantic_query_memo`’s graph/value core, `identity_interner`, `intrinsic_registry`, `locator_identity`, `mapper_binder_registry`, `flow_slice_content`, and the pure algorithm/value portions of `component_meta_materialize`, `decl_body_memo`, `fact_emission`, `fact_signature_helpers`, `meta_resolve`, `project_type_store`, `structural_carrier_producer`, and non-flow `typeinfo`.
- Stay in `verter_session`, with captured data passed down: `VerterHost`, `host_*`, lifecycle `ResolverContext`, artifact/project/store managers, `session_view`, `store_view_roots`, `resolved_import_facts` storage, lowering services, cache admission/singleflight/retention, request context/cancellation, audit and instrumentation. The relevant live accesses appear at `resolver_context.rs:188-196,261-266,700-717`, `project_semantic_dispatch/lower.rs:403-418`, and `project_semantic_dispatch/flow_return.rs:2016-2078`.
- Split rather than move whole: `cache_runtime`, `component_meta_caches`, `component_meta_materialize`, `decl_body_memo`, `fact_signature_helpers`, `file_artifact_store`, `project_type_store`, `types`, and `resolver_store`. Neutral keys/values/algorithms move; capture, retention, publication, TLS, and live stores stay.

Every higher-layer edge is:

1. **`verter_compiler`.** `template_class_facts.rs:6` imports `RawTemplateData`, and `:121-128` accepts it. Invert: compiler converts it to a semantic-owned immutable template-class input DTO. Moving the import creates `verter_semantic ↔ verter_compiler`, because compiler already depends on semantic (`crates/verter_compiler/Cargo.toml:51-59`).

2. **`verter_protocol`.** `resolver_core/component_meta/mod.rs:98-115` stores `ResolvedJsdocTypeOutput`. Keep wire conversion in session/protocol and store a neutral semantic JSDoc result below. Otherwise protocol’s existing semantic dependency (`crates/verter_protocol/Cargo.toml:12-20`) creates a direct cycle.

3. **`VerterHost` and scheduler.** Component-meta escapes through the context to `host.scheduler` at `resolver_core/component_meta/mod.rs:251-288`; native props calls a host projection at `component_meta/native_props.rs:113-139`; `ResolverContext` names scheduler cancellation at `resolver_context.rs:188-196`. Cut these paths and supply captured language, prepared-declaration, surface, and cancellation-state observations. The host/native/wire portions of those files stay behind, so `component_meta/` cannot move as an unmodified wildcard despite `C1.md:19-24`.

4. **Store and cache infrastructure.** `HostStoreView` is not a dependency-neutral value: its snapshot contains session `StoreViewRoots` (`resolver_store.rs:1385-1411`), and those roots retain scheduler, workspace, artifact-store, project-store, and resolution-store handles (`store_view_roots.rs:385-418`). Its capture reads `VerterHost` and workspace generations (`resolver_store.rs:638-655,792-816`). Keep `HostStoreView`, its roots, and manager in session; move `StoreViewValidationToken`-like scalar DTOs and introduce the semantic-owned observation snapshot. This changes the member boundary claimed at `C1.md:187`.

5. **Workspace fact vocabulary.** `resolver_core/fact_read_set.rs:1-5`, `resolver_core/reuse.rs:46-51`, `resolver_core/request_store_view.rs:81-94`, and `resolver_core/mod.rs:669-672` import workspace-owned fact/cache types. Move the neutral fact identities, read-set values, populations, and validation-result vocabulary into semantic; workspace and session then import them downward. Live fact registries, published roots, transactions, and generation capture remain workspace/session.

6. **`ProjectResolver`.** Its pure algorithm depends on workspace DTOs and membership (`verter_workspace/src/resolver.rs:11-17,74-114`), but its resolution entry accepts live `WorkspaceRead` (`:321-365`), path resolution calls `probe_path`/`realpath` (`:1251-1267`), and package resolution calls live manifest reads (`:1654-1663`). Move the algorithm, request/result/manifest DTOs, and immutable membership predicate into semantic. Replace `WorkspaceRead` with snapshot lookups whose missing keys produce `NeedInputs`. Keep `resolve_tracked`, `TrackedResolutionCapability`, and `TransactionReader` in workspace (`:354-384`).

7. **Provider/tsgo.** `resolver.rs` has no direct tsgo call; its current higher-layer closure comes from being housed in workspace, whose manifest depends unconditionally on scheduler and target-conditionally on tsgo (`crates/verter_workspace/Cargo.toml:35-49`). `ProjectStableKey` also imports scheduler’s `Hash16` and workspace payloads (`project_key.rs:11-15`): move the key representation/hash down and keep `from_project` above. Deleting `verter_semantic → verter_workspace` at `crates/verter_semantic/Cargo.toml:24-27` removes the presently ratified transitive scheduler/tsgo reach (`workspace_dependency_layers.rs:108-126`).

8. **Flow.** The directory contains `flow_return` and routes it through the shared dispatcher (`project_semantic_dispatch/mod.rs:103-143,2186-2195`), while its implementation reaches session cache/project-store machinery (`flow_return.rs:2016-2078`). Its architectural home is genuinely ambiguous because C1 excludes flow behavior while requiring the entire dispatcher to relocate. To preserve the stated member scope, move the flow query/value and pure evaluation code unchanged; leave cache-node admission, retention, audit, and capture above. Flow remains exempt from C1’s `AttemptOutcome` conversion.

`ConfiguredMembership` is the other ambiguous member: it mixes an immutable predicate with workspace construction/cache machinery (`membership.rs:13-17,189-256`). The executable split is a semantic immutable membership snapshot, populated by the workspace builder.

Cycles to prevent:

- `semantic ↔ compiler`: `template_class_facts.rs:6` versus compiler `Cargo.toml:59`.
- `semantic ↔ protocol`: component-meta `:115` versus protocol `Cargo.toml:18`.
- `semantic ↔ session`: any retained host/store/session reference versus session `Cargo.toml:122`.
- `semantic ↔ workspace`: workspace must call the relocated resolver; semantic’s current workspace edge at `Cargo.toml:27` must be deleted first.

Final verification is the existing Cargo-metadata production-closure gate (`workspace_dependency_layers.rs:1-18,316-350`) with the semantic exception at `:118-126` deleted. No source scanner substitutes for that gate.

## GAP 3 — Full-coverage `AttemptOutcome` proof

The charter’s exhaustive-`impl` mechanism is invalid.

- Adding a required method makes an existing impl incomplete, but adding a defaulted method does not. The repo demonstrates this directly: `ResolverContext::is_request_bound` has a default at `resolver_context.rs:179-181`, while the large `impl ResolverContext for VerterHost` beginning at `:817` omits it and compiles.
- Even a required method proves only that the double supplies a body. A new method returning bare `T` can be added, implemented with bare `T`, and the exhaustive impl compiles. The impl does not constrain the trait author’s return type.
- Generic methods, associated output types, and `async fn` do not improve this. They may make the trait non-dyn-compatible—the current code separately pins that at `resolver_context.rs:805-815`—but they do not force `AttemptOutcome`. An associated type can resolve to any type.
- A Rust type alias to `AttemptOutcome<T>` is type-identical and therefore valid. It only defeats textual/name-based checking, not Rust type equality.
- A foreign exhaustive double is forbidden by GAP 1’s seal. Only an in-module semantic test type can implement it, and it still suffers the default/return-shape defects above.
- “Sole entry surface” is currently false: ProjectResolver uses `WorkspaceRead` directly (`resolver.rs:321-365`), component-meta reaches the host/scheduler (`component_meta/mod.rs:251-288`), and dispatch lowering calls host materialization (`lower.rs:403-418`).

Replace the operation trait with one closed, non-overridable inherent gateway:

```rust
pub enum NonFlowOperation { /* every C2-reachable operation */ }
pub enum NonFlowValue { /* heterogeneous results */ }

impl TypeInfoCore {
    pub fn attempt(
        &self,
        observations: &ObservationSnapshot,
        operation: NonFlowOperation,
    ) -> AttemptOutcome<NonFlowValue>;
}
```

Keep the implementation modules and fields private. Match `NonFlowOperation` exhaustively with no wildcard. Adding a variant then forces the dispatcher to change, while every cross-crate operation necessarily returns the one inherent method’s `AttemptOutcome`. Compile-fail fixtures prove foreign code cannot implement a query/observation trait or access an alternate kernel operation. This is a mechanically valid proof of the closed cross-crate entry surface and its return shape.

It is not—and stable Rust cannot make it—a complete proof that every internal helper is I/O-free. `verter_semantic` can still compile `std::fs`, `std::net`, a blocking primitive, a global/TLS read, or an I/O-capable callback without naming `VerterHost` or scheduler. Cargo’s crate firewall cannot observe those effects. A dedicated `#![no_std]` capability crate could strengthen that proof, but the binding ruling requires the existing `verter_semantic` crate.

The honest mechanically checkable criterion is therefore:

- Every external C2 operation is a `NonFlowOperation` and enters through `TypeInfoCore::attempt`.
- All kernel internals are private to that module tree.
- The semantic Cargo closure contains only layers 1–3.
- Compile-fail tests pin sealing and private-entry unreachability.
- Exhaustive all-missing-observation tests cover every enum variant and require `NeedInputs` or `Terminal`, never blocking.
- Internal absence of direct I/O/blocking remains an audited/linted source invariant, not a type-system proof.

Accepted ADR: unchanged by all three gap resolutions.

Program DAG edge: unchanged; the planned dependency inversion remains higher layers → `verter_semantic`.

Program outcome: unchanged; full non-flow extraction and full behavioral `AttemptOutcome` coverage remain required.

GAP 1 VERDICT — Current cross-crate sealing design is impossible; seal semantic-owned snapshots and require compiler/session composition, not foreign implementations.

GAP 2 VERDICT — The proposed files do not close below layer 4 as-is; C1 is executable only through the listed splits, cuts, and capture inversions.

GAP 3 VERDICT — The exhaustive-`impl` proof is false; a closed inherent gateway proves boundary shape, but total internal I/O-freedom has no mechanically valid proof under the preserved existing-crate architecture.
