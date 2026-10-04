# Decision: extract a parser-free engine over one shared owned-input boundary

**Status:** Approved design for implementation in one PR. Boundary preparation precedes the extraction commits. AC1–AC5 remain unchanged.

## Context

The engine cannot be extracted by relocating the charter’s directories alone. Its current dependency closure includes parser-backed semantic definitions, workspace receipts, scheduler primitives, session input records, framework execution, and concrete stores.

The preceding ports work provides useful request-bound seams, but their return types and attachments still cross the intended boundary. Completing that boundary is part of this PR.

The inspected checkout differs from the stated baseline only by the extraction-prerequisite decision document. Its production code therefore matches the stated baseline. This ruling is based on source inspection; builds and tests were not run during this read-only review. The supplied file and line totals are planning inputs, not independently certified counts.

## Decision

**Extend `verter_session_query` into the canonical parser-free owned-data boundary; extract the scheduler-independent execution primitives into `verter_execution`; keep evaluated state and semantic execution in `verter_type_engine`; keep retained syntax and source lowering in `verter_semantic_source`.**

Do not introduce a second semantic-core crate alongside `verter_session_query`. Their responsibilities would substantially overlap, and splitting prepared declarations, flow vocabulary, and fact receipts between them would create an unnecessary dependency-order problem.

The boundary crate may contain deterministic operations over its owned data: hashing, receipt composition, structural flow planning, and existing parser-free semantic helpers. It must not contain host loading, project routing, retained ASTs, evaluated-node stores, or another query-time resolver.

### Intent contract

The extraction preserves:

- One `ProjectTypeStore` assembly and the existing shared store instances.
- One semantic query engine and one workspace resolution authority.
- Existing cache identities, candidate selection, admission, budgets, evaluation order, and synchronization.
- Source-demand laziness, retained-snapshot leases, and exact observed-content identity.
- Request/view isolation, completion fences, fact tracing, cancellation ownership, and cross-layer wait-cycle detection.
- The existing test population, with an explicit old-to-new test identity map.

There must be no new virtual dispatch or allocation for each evaluated node. Existing request-port calls remain at their existing demand boundaries. Graph access, interning, memo lookup, relation evaluation, and signature operations retain concrete implementations.

Breaking Rust module paths and construction APIs are authorized. Semantic behavior changes are not.

### 1. Final crate graph

The following table specifies allowed repository dependencies for the affected layers. Existing third-party utility dependencies remain permitted subject to the same transitive restrictions.

| Crate | Allowed production dependencies and responsibility |
|---|---|
| `verter_span` | Existing non-OXC leaf dependencies. Owns Verter spans and canonical paths. |
| `verter_execution` **new** | Standard library, synchronization/collection utilities, `verter_debug_assert`, and leaf observability if required. Owns scheduler-independent task identity, wait graph, cancellation, and opaque request-context propagation. No parser, semantic engine, workspace, protocol, or concrete scheduler. |
| `verter_session_query` **extended** | `verter_type_expr`, `verter_span`, `verter_language`, `verter_macro_dto`, `verter_no_typeexpr`, `verter_audit`, `verter_debug_assert`, and `verter_execution` where needed. Owns shared inputs, facts, identities, source outcomes, and pure operations over them. |
| `verter_semantic` | `verter_session_query`, existing syntax/front-end and leaf dependencies. Retains AST analysis/builders and its remaining front-end services. Never depends on engine or semantic source. |
| `verter_semantic_source` **new** | `verter_semantic`, `verter_session_query`, `verter_execution`, parser/OXC/`verter_type_expr_oxc`, and required leaves. No engine, session, workspace, protocol, providers, compiler, or concrete scheduler. |
| `verter_type_engine` **new** | `verter_session_query`, `verter_execution`, and the approved parser-free leaves above. No `verter_semantic`, semantic source, parser, OXC, compiler, session, workspace, protocol, providers, or concrete scheduler. |
| `verter_scheduler` | `verter_execution` plus its existing concrete scheduling/front-end dependencies. No engine dependency. |
| `verter_workspace` | `verter_session_query`, `verter_execution`, and its existing workspace/front-end dependencies. Owns live resolution and publication. No engine dependency. |
| `verter_session` | Assembles engine, source, semantic, workspace, scheduler, protocol, and existing host integrations. |
| `verter_protocol` | Retains wire-schema ownership. May consume neutral boundary vocabulary where already appropriate; engine and source never consume protocol. |

This is a production graph, including build dependencies and optional production features. A `test-support` feature must not become a back door to shell dependencies.

#### Remove the OXC span edge completely

Delete `verter_span`’s `oxc_span` dependency and both OXC `From` implementations.

At AST-consuming call sites, use the existing Verter constructors with the original `start` and `end` values. These conversions are coordinate-preserving field transfers; do not introduce normalization, offset adjustment, or a new allocation.

Do not feature-gate the old implementations. An optional OXC feature would still violate AC2.

Do not attempt to relocate the same `From<oxc_span::Span>` implementation into an adapter crate: neither type would be local there. Explicit conversion at the syntax boundary avoids that orphan-rule problem and does not justify another crate.

#### Move the parser-free semantic closure into `verter_session_query`

Use stable domain modules rather than one undifferentiated `types.rs`:

| Existing owner | Canonical destination |
|---|---|
| `verter_semantic/analysis/type_eval.rs`: owned declaration/value vocabulary and parser-free operations | `verter_session_query::declarations` |
| `analysis/type_solver/{prepared,host,builtin,arena}` parser-free definitions and helpers | `verter_session_query::type_solver` |
| `analysis/decl_headers.rs`: header/index records | `verter_session_query::declarations::headers` |
| `analysis/flow/mod.rs`: skeletons, binding identities, prepared-body records | `verter_session_query::flow::skeleton` |
| `analysis/flow/{flow_ir,flow_graph,peeker,hashing,lower,binding}.rs` | `verter_session_query::flow`, preserving current algorithms |
| `analysis/function_program.rs`: index, entry, locator, and structural record definitions | `verter_session_query::function_program` |
| `facts/*`: shared fact keys, versions, receipts, and supporting vocabulary | `verter_session_query::facts` |
| `resolver_core`: environment, project, resolution-context, and resolution-fact vocabulary | `verter_session_query::resolution` |
| Shared analysis snapshots, framework/script facts, template-class records | `verter_session_query::analysis` and `framework` |
| AST-free enum constants and scalar projection | `verter_session_query::enum_constant` |

The AST builders in mixed files stay in `verter_semantic`, importing the canonical definitions. Split mixed files during boundary preparation, before the move-only commits.

Apply the same rule to supporting DTOs currently owned by the parser or compiler:

- The routing inventory carried by `ShallowInputRecord` cannot remain a `verter_parser` type.
- Engine-consumed `RawTemplateData`/component-usage records must become canonical neutral input records.
- AST-backed framework artifacts must be split into their immutable query-facing data and their syntax-owned backing.
- Move `js_number_to_string` to the existing parser-free type-expression utility layer, preserving its implementation and tests. Engine and compiler import that single implementation.

Do not copy these structures into “engine versions.” Producers must construct the canonical types.

### 2. Workspace and scheduler primitives

#### Workspace facts move; workspace authority stays

Move the parser-free implementation of:

- `fact_cache.rs`: `ReadSetSignature`, `FactVersionValidator`, aggregate basis/generation vocabulary, and associated validation/composition helpers.
- `fact_read_set.rs`: collection, compaction, and finalization.
- Shared project IDs, environment identities, and resolution-fact keys.

Their destination is `verter_session_query::{facts,resolution}`.

Several apparently workspace-owned types already originate in `verter_semantic`: notably receipt/version vocabulary, including `ResultReceipt` and `StrictSelfRootWorld`. Move their original definitions once and update consumers directly.

Keep in workspace:

- `PublishedRoot` and `WorkspaceSnapshot`.
- Live resolution indexes and resolution-currency publication machinery.
- Generation producers and view selection.
- Concrete implementations of validation and publication authority.

A port returns the existing admitted answer, its receipt, and its refusal/admission state in neutral carriers. It must not expose a workspace publication service.

`OperandEnvEpoch` currently compares a retained root by `Arc::ptr_eq` plus generation. Preserve that identity rule. Represent the retained root as an opaque identity handle that retains the original allocation, without exposing its contents. Do not replace it with a content hash, newly allocated wrapper identity, or generation alone.

`CanonicalPath` already has a leaf owner in `verter_span`; import it there directly.

#### Move execution primitives without reimplementing them

Move these scheduler modules into `verter_execution`, retaining their implementations:

- `tasks.rs`: `ProducerIdentity`, `ExecutionTask`, `TaskRegistry`, scopes, producer registrations, wait edges, and cycle refusal.
- `cancellation.rs`: tokens, owner registrations, and current-job cancellation scope.
- `request_context.rs`: opaque context carrier, TLS slots, installation/restoration, and clear hooks.

Concrete queues, worker pools, admission, priorities, and scheduling remain in `verter_scheduler`.

**The scheduler and semantic engine must use the same `TaskRegistry` instance where their producers can wait on one another.** Moving the type while creating separate registries would silently break cycle detection.

Preserve the cancellation selector’s current behavior: checkpoint-time selection after cooperative job entry, including registered-owner handling. Capturing one token at request construction is not equivalent.

Move the process-wide retention account and RAII charge implementation to `verter_session_query::retention`. Leave the workspace-specific `ResolutionRetentionAccount` adapter with session/workspace integration. There remains one process-local account, with unchanged atomics, limits, and release behavior.

### 3. Resolver context and the six ports

Put the six service contracts in:

```text
verter_type_engine/src/resolver_core/request_ports.rs
verter_type_engine/src/resolver_core/fact_validation_port.rs
```

They remain:

- `IndexedInputs`
- `OwnedLowering`
- `RouteLookup`
- `FactValidation`
- `Cancellation`
- `ExecutionSubmission`

These are engine-owned capabilities. Their reusable input/output vocabulary lives in `verter_session_query`. `ExecutionSubmission::attach_engine` may return engine-owned `EngineBinding`; that is precisely why this trait should not live in the neutral DTO crate.

Keep `QueryHostPort` and its authored-body serve vocabulary in `verter_session_query`; the engine lowering capability consumes that existing contract rather than defining a second version.

Replace the cross-crate sealed `ResolverContext` trait with an **engine-owned concrete request context** containing borrowed references to the selected services and the captured request identity. Construct it once at the request boundary. Hot code receives that concrete context; each service operation makes at most its existing service-boundary indirect call.

This avoids a forwarding `dyn ResolverContext` layered over six additional trait objects.

Session retains:

```text
resolver_core/host_resolver_context.rs
resolver_core/session_resolver_context.rs
resolver_core/request_store_view.rs
resolver_core/owned_lowering_port.rs
```

These implement the engine contracts against the existing captured views, completion overlays, and source leases.

The private lifecycle constructors remain session-owned. `VerterHost` does not implement production engine services directly.

**Sealing limitation:** Rust has no friend-crate visibility. The current private seal cannot both remain private to engine and be implemented by session. The replacement protects construction and lifetime through the concrete context and explicit service contracts; it does not claim that arbitrary downstream code cannot implement a public backend trait. Compile-contract tests must protect the actual supported boundary, including rejection of direct-host production construction.

#### Signature substitutions

| Current exposed type | Replacement |
|---|---|
| `IndexedInputRecord`, `IndexedInputServe`, `PreparedInputRecord`, `ShallowInputRecord` | Canonical owned records in `verter_session_query::inputs` |
| `FileAnalysisSnapshot` | Its neutral immutable analysis payload; compiler/host backing remains outside the record |
| `session_view::EnvHashes`, `ProjectIdentity`, `SourceEnvIdentity`, artifact keys | Canonical boundary identities |
| Prepared declarations and preparation outcomes | Boundary-owned declarations and explicit failure/admission vocabulary |
| `IndexedExpressionDemand` | Neutral demand identity/request data; session resolves it against retained source leases |
| `EnginePolicy` | Engine-owned immutable value, explicitly constructed by session from `HostConfig` |
| `EngineBinding` | Engine-owned attachment containing engine resources only |
| Scheduler cancellation/task types | Their identical definitions in `verter_execution` |
| `PublishedRoot` | Opaque retained identity where identity is needed; otherwise neutral data |
| Contributor/augmentation answers | Neutral ordered records plus their existing population/fact evidence |

Do not use associated types for semantic records the engine inspects. That would hide the vocabulary without resolving ownership.

Use opacity only for genuinely opaque identity or lifetime retention. No `Any` downcasts to recover host, workspace, parser, or compiler services.

Preserve existing projection memoization and lease tables: an `Arc` projection is constructed at the existing artifact boundary and reused. Port calls must not clone inventories or reconstruct snapshots.

### 4. Engine-to-session calls

#### Move engine-driven framework execution into the engine

Split the following before extraction:

```text
typeinfo/framework_surface/vue_exec/
typeinfo/framework_surface/svelte_exec.rs
typeinfo/framework_surface/{plan,results,scope,resolved_surface_access}.rs
meta_resolve/{callable_view,dispatch_helpers,slot_binding_graph,...}
```

Move the semantic execution closure into engine. Host entry methods that choose views and construct requests remain in session.

`resolve_vue_macro_surface_with_ctx` is a direct semantic dependency of dispatch. It therefore moves with dispatch. The Svelte semantic execution paths follow the same rule.

Engine-driven framework execution receives neutral script/framework facts through the existing input and lowering services. It continues using the same dispatch, task, budget, tracer, and stores.

Keep wire encoding, protocol graph export, request routing, and host lifecycle in session/protocol.

Internal framework discriminants currently imported from protocol become canonical neutral vocabulary. Protocol maps to its transport representation explicitly. Do not move generated protocol types into the engine or make internal execution depend on wire schema.

#### Keep framework semantic stores concrete

Move `FrameworkSurfaceStore` and its internal semantic payloads into engine after removing their protocol dependencies.

`EngineBinding` retains direct `Arc` references to those stores. `ProjectTypeStore` still owns and supplies the single Vue and Svelte instances.

#### Separate final component payloads from generic memo machinery

`ComponentMetaResultDb<P>` is already generic. Use that separation.

- Move the generic store, candidate/admission machinery, and generic `MemoRead`/`MemoPublish` implementation into engine.
- Keep `CachedComponentMetaResult`, protocol-bearing projections, host cache-key construction, and terminal payload assembly in session.
- Remove `component_meta_results` from `EngineBinding`.
- Change the dispatch’s final-component memo methods to accept a borrowed `ComponentMetaResultDb<P>` and return the existing scoped read/publish capabilities.

Session passes the same store instance owned by `ProjectTypeStore`. This is statically dispatched generic code, with no erased payload, extra cache, or extra runtime lookup.

#### Other calls

| Existing dependency | Ruling |
|---|---|
| `emit_dispatch_dep_signature_facts` | Move its receipt propagation/tracing implementation into engine; retain host fact acquisition behind the fact service. Preserve observation order and suppression behavior. |
| `host_manage` counters and `RelationHostKnobs` | Extract the exact shared counters/knobs into neutral or engine-owned observers; attach existing instances. No callback per increment. |
| Global contributor collection/classification | Keep workspace-aware collection in session. Return canonical classifications, ordered contributors, and population fingerprints through `OwnedLowering`. Engine performs existing semantic filtering. |
| `fact_signature_helpers` | Split neutral receipt operations, engine tracing, and host fact acquisition by those owners. Do not move the entire mixed file blindly. |
| Session `request_context.rs` | Move engine execution state and its hot accessors into engine. Keep concrete host audit registration in session behind a narrow finalization capability. Move engine derivation accumulation with engine; retain existing TLS installation/restoration through `verter_execution`. |

`EngineBinding` remains an attachment to engine resources, never a service locator or a container for `ProjectTypeStore`.

#### Preserve output capability fences

The sealed output-materialization machinery currently registers session-local sink types. It cannot move unchanged across crate boundaries.

Move the relevant terminal semantic sink implementations and their private capability constructors into engine. Expose complete terminal projection operations to session. Do not expose a public capability constructor, raw unrestricted `TypeExpr` materializer, or externally implementable replacement for the sealed projector.

This is a required boundary-preparation change, with compile-fail tests.

### 5. Extraction layout and co-moving modules

Retain the existing main module names:

```text
verter_type_engine/src/
  project_semantic_dispatch/
  semantic_query.rs
  semantic_query/
  semantic_query_memo/
  semantic_execution.rs
  signature_kernel/
```

Do not adopt `flow/evaluate/relation/query/memo/runtime/signature` in this PR. That layout is not needed to establish ownership and would obscure the extraction.

Move the following engine-owned closure after neutralization:

- `cache_runtime`: singleflight, publication, admission, node operations, and flow stores.
- `bounded_query_retention`, `identity_interner`, `intrinsic_registry`, `mapper_binder_registry`.
- Engine portions of `capture_token`, `graph_walk`, `cache_schema`, and instrumentation.
- `resolver_core::{bare_name_resolve,scope_shadowing,reuse}`.
- Semantic declaration preparation and metadata consumers.
- `component_meta_query_engine` and its semantic cache operations.
- `component_meta_caches`.
- The generic final-component store described above.
- Engine-driven framework execution, semantic surface stores, and their private projection helpers.
- Engine-specific request state, observation scopes, and audit accumulation.

Other pieces go lower:

- `Hash16`, shared provenance counters, source completion vocabulary, and common identities → neutral boundary or their existing leaf owner.
- `flow_completion_inventory` shared vocabulary and structural operations → boundary.
- Flow function keys, `BoundFlowGraph`, and immutable graph bundles consumed by source → boundary.
- Shared preparation DTOs and outcomes → boundary.
- Source-only helpers → semantic source or semantic AST analysis.

Do not treat the directed min-cut as an ownership manifest. In particular, `types.rs`, `meta_resolve.rs`, `framework/mod.rs`, and `host_resolve/mod.rs` are mixed modules or wiring. Moving their entire descendant trees would pull shell ownership into engine.

Host-backed proof producers remain in session. A pure proof-store implementation can move, including a correctly gated test-support implementation, without moving its host producer.

### 6. Semantic source

Preserve the charter’s source module names under `verter_semantic_source`:

```text
decl_body_memo.rs
decl_body_memo/
decl_lowering.rs
flow_slice_content.rs
flow_slice_content_branches.rs
flow_slice_content_class.rs
```

Also move `parsed_eval_program.rs`, which `DeclLoweringService` uses to retain and reborrow parsed programs. Keep its ownership and worker-affinity discipline intact.

Extract from `flow_slice_content.rs` into `verter_session_query::flow::slice`:

- All shared `Slice*` records and enums.
- `SliceContent`, selection and nested-context records.
- Source-demand and capture-authority vocabulary.
- Parser-independent operations over those records.

Keep AST visitors, AST-backed helper types, and content-lowering functions in semantic source.

Move `FlowGap` and `FlowReturnPolicy` to the boundary. Preserve discriminants, defaults, hashes, and policy construction.

**Keep `CallValue` in engine.** The inspected source reference is a documentation link to the evaluator’s sink type, not a production field or import requiring relocation.

The calls at `locator_deref.rs`’s enum-body paths invoke `enum_scalar_type_expr`, a pure conversion. Move the canonical conversion to the boundary’s enum module and use it from source and engine. Preserve its exact scalar parsing and degradation behavior.

Further source dependencies requiring separation:

- Extract `RouteLens`/`ShallowLens` records and source-facing operations from `fact_emission`.
- Move source-local `collect_typeof_roots` and rune-environment construction helpers to source/semantic ownership.
- Keep source demand failures explicit. Move shared non-cacheable observation vocabulary and propagation support below engine where source must signal it.
- Keep `SnapshotLease` and worker/service internals in semantic source; neutral `SnapshotKey` and demand identities belong to the boundary.
- Pass the existing retention account and provenance instances into source construction.
- Relocate source tests that currently construct engine flow stores, or make them consume prebuilt neutral graph bundles. Do not give source a test-only engine dependency.

Source workers produce owned lowering results. They receive no dispatch callback, engine handle, graph interner, or resolver capability.

### 7. Tests and AC1/AC5

The 69 host-driven test files are **session integration tests**, regardless of their current location.

Move them to a dedicated session-owned test subtree. Preserve each test body, assertion, fixture, ignore disposition, and scheduling requirement. Preserve existing module suffixes where practical and record the precise identity mapping where they change.

Use an engine `test-support` feature to expose only the constructors, inspectors, fault controls, and operations those tests actually require.

Rules:

- The feature contains engine implementation only.
- It introduces no shell dependency.
- Test support may inspect private engine state; ordinary production APIs do not become public merely to satisfy `super::`.
- Session test wrappers use the real host and the real extracted engine.
- No test file is compiled into both crates.
- No cross-crate `include!` of production implementation to recover private access.

Engine-local tests move with engine and use neutral inputs. AST/allocator-driven tests move to source or session unless the allocator is incidental and removable without changing the test’s purpose.

Capture nextest identities as a multiset including package, binary, test name, and ignored state. Every baseline test must map to exactly one final test. New boundary and guard tests are an explicit additive set.

AC5 is proved by building and linking engine tests independently, with no session/workspace/provider/compiler/parser/concrete-scheduler dependencies in their compiled test closure.

### 8. Dependency guards

Place the policy and tests in:

```text
crates/verter_source_policy_gate/tests/cases/
```

Use structured `cargo metadata`, not parsed `cargo tree` text:

```text
cargo metadata --format-version 1 --locked --all-features
```

Do not use `--no-deps` or `--filter-platform`. Traverse package IDs through `resolve.nodes[].deps[].dep_kinds`, following normal and build edges. Resolve renamed dependencies by package ID. Cargo metadata includes target-platform dependencies by default and identifies dependency kinds explicitly. See [Cargo metadata documentation](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html).

Guard these closures:

1. Engine: reject all OXC packages and all forbidden shell/front-end/provider packages.
2. Semantic source: reject engine, session, workspace, protocol, providers, and concrete scheduler.
3. Neutral query and execution crates: enforce their lower-layer dependency allowlists.
4. Engine tests: separately prove shell-free test construction.

Use an allowlist of permitted internal crates for engine/query/execution, in addition to explicit forbidden-family diagnostics. A newly named provider must not bypass the rule merely because it was absent from a historical denylist.

Remove the current query-boundary guard’s sanctioned OXC-span subtree exception.

All-features checks should be supplemented by default and no-default-feature checks. Production features may not activate shell dependencies; test support remains structurally shell-free. Fail closed when metadata resolution fails.

#### Planted violations

Use hermetic temporary Cargo workspaces/manifests and run actual metadata resolution through the same guard implementation.

Include a separate rejected plant for:

- OXC, explicitly including `oxc_span`.
- Parser.
- Compiler.
- Session.
- Workspace.
- Protocol.
- Provider.
- Concrete scheduler.
- Semantic source → engine.

Also cover transitive, renamed, optional-feature, build-dependency, and target-specific routes, plus a permitted control graph. Do not test only fabricated JSON adjacency lists.

A real planted back-edge may make Cargo reject a cycle before traversal. Report that as a dependency-resolution failure; additionally use an acyclic fixture to prove the policy diagnostic for that forbidden owner.

## Consequences

This is a substantial boundary refactor followed by extraction. The five engine sets and source set are not the complete ownership closure. The mixed semantic modules, request state, framework execution, private capability registrations, and host-driven tests make preparation materially larger than the min-cut’s neutral-file count.

The design preserves direct engine data access and avoids turning evaluated nodes into port calls. It also preserves the single instances of stores, the task wait graph, and the retention account.

The most sensitive changes are:

- Cross-crate request construction and output-capability privacy.
- Content-pinned inputs and root identity.
- Fact-tracer TLS nesting and restoration.
- Cancellation selection inside shared jobs.
- Source lease lifetime and worker affinity.
- Feature-sensitive test discovery.
- Compiler/protocol records currently embedded in engine-facing data.

These require targeted evidence. Passing the existing suite alone does not establish them.

**No inspected dependency requires a semantic algorithm change.** The enum source call is a movable pure helper; framework execution can move with its semantic dependencies; scheduler primitives can retain their implementation. Cross-crate visibility, trait construction, and mixed-file separation do require structural code changes, so the entire PR cannot honestly be called moves-only.

AC1 applies to the designated extraction commits. Boundary commits must not conceal changes to cache keys, synchronization, budgets, or evaluation order. If implementation requires one of those changes, it has exceeded this behavior-preserving ruling; it is not justified by relabeling the commit “preparation.”

Performance improvement is not claimed without measurement. Crate separation can affect inlining, code size, and compilation even when algorithms are unchanged.

## Ordered implementation plan

1. **Capture the baseline.**  
   Record the nextest population and ignored set, feature/profile configuration, representative semantic outputs, and cold/warm performance observations. Prepare a file/symbol ownership map and test identity map. Keep operational receipts outside production source.

2. **Remove OXC from `verter_span`.**  
   Update all conversions, remove the dependency, and establish the stricter neutral-boundary firewall. Preserve span-coordinate behavior.

3. **Create `verter_execution`.**  
   Move task, cancellation, and opaque request-context machinery. Update scheduler and callers directly. Prove shared wait-cycle detection, nested TLS restoration, and cancellation-owner behavior before proceeding.

4. **Complete `verter_session_query`.**  
   Split mixed semantic/parser/compiler files and move the canonical prepared, flow, completion, analysis, identity, and fact vocabulary. Move shared receipt and retention machinery. Remove original definitions and compatibility exports in the same preparation sequence.

5. **Neutralize source while it remains in session.**  
   Separate Slice IR, source demands, enum conversion, retained-program ownership, lenses, and source-local helpers. Remove all source-to-engine calls and types. Preserve lazy lowering and exact source pinning.

6. **Complete engine request construction and resource binding.**  
   Introduce the concrete request context, neutral signatures, explicit policy construction, and engine-only attachment. Preserve existing adapter lifetimes and store instances. Add discriminating tests for fenced serves, overlays, stale observations, and nested demands.

7. **Move semantic responsibility out of shell-facing code.**  
   Separate framework execution from host entrypoints and wire export. Separate generic component-result storage from session payloads. Relocate terminal output capabilities and compile-contract fixtures. Split tracing, audit finalization, counters, and contributor acquisition.

8. **Rehouse tests and finish API preparation.**  
   Move host-driven tests to session, add narrowly scoped engine test support, and prepare engine-local tests against neutral inputs. Verify the baseline test mapping has no losses or duplicates.

9. **Commit the source extraction as moves and wiring only.**  
   Create `verter_semantic_source`; relocate the prepared source closure. Permit only imports, visibility, module wiring, Cargo manifests, and necessary lockfile changes outside detected renames.

10. **Commit the engine extraction as moves and wiring only.**  
    Create `verter_type_engine`; move the prepared engine closure with its current internal module names. Update all consumers directly. Delete old module paths; add no compatibility re-exports.

11. **Land the final dependency guards and mutation fixtures.**  
    Guard logic and failing plants should already have been exercised during preparation. Register their final owners in the source-policy gate, remove the old OXC exception, and verify every required plant is rejected.

12. **Verify AC1 and AC3 explicitly.**  
    Evaluate each extraction commit with `git diff -M50%`, including the required ≥95% production-line rename accounting and inspection of every residual hunk. Compare normalized nextest populations. Confirm old definitions, paths, exports, and duplicate implementations are absent.

13. **Run the complete acceptance lanes.**

    ```text
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    node scripts/gate.mjs
    node scripts/compile-contracts.mjs
    cargo check --workspace --release
    cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings
    pnpm proto:check
    pnpm run build:native
    pnpm run build:ts
    pnpm --filter @verter/wasm build
    ```

    Require complete canonical-gate receipts, including shipped-cfg verification. Run the dedicated Svelte/provider lanes affected by the moved paths; they are not implied by the canonical gate.

14. **Prove build isolation and performance preservation.**  
    Independently link engine tests with their intended features. Warm a fixed build configuration, make a session-only implementation edit in a disposable verification checkout, and rebuild with identical flags/environment. Require both new crates’ relevant artifacts to report `Fresh`; ensure no build script watches the whole repository.

    Compare representative cold/warm, flow, relation, framework, and concurrent-demand workloads. Inspect allocations, source-work counts, cache hits, lock activity, and generated hot-path calls. Resolve regressions without introducing a second cache, per-node service dispatch, or changed synchronization.

15. **Update owning documentation.**  
    Update type-resolution, cache architecture, signature-kernel, host-session, component-meta, and testing references for their changed owners and APIs. Keep top-level repository guide changes limited to summaries and pointers. Deliver the single PR only when every acceptance obligation has its actual receipt.
