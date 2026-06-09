---
name: component-meta
description: "Component metadata: native vs compat boundary, fallthrough/root inheritance, resolver rules, cache-owned hydration, registry publication"
---

# Component Meta

## Audit & footprint

Per-request semantic footprint observability splits across two crates:

- **`verter_audit`** (substrate) — owns `RequestAuditRecord` envelope, `RequestKind` / `RequestKindPayload` discriminants, all per-kind payload structs (`ComponentMetaPayload`, `TypeResolutionPayload`, `SemanticAnalysisPayload`, `CompilePayload`, `WorkspacePayload`, `LspRequestPayload`, `McpToolPayload`, `BundlerBatchPayload`), producer-side `AuditObserver` trait + `AuditEvent` counter hook, `StructuredAuditEvent` enum + variant payloads (in `verter_audit::origin_graph`), `AuditConfig` + `AuditConsumerFilter`, trivial `NoOpObserver`.
- **`verter_session`** — owns concrete `HostAuditRuntime`, `AuditRecordsStore`, `AuditRequestRegistration` lifecycle (Active / Noop arms, RAII drop), per-request `RequestContext` + TLS observer guard, accumulator, footprint miner, host-owned peak-RSS sampler thread (native only), structured-trace macros, `AuditedRequest` test harness.

Opt-in via `HostConfig::audit_enabled + footprint_capture` (and `audit_timing_capture` for timing surface).

Component-meta store counters and solver counters live on `ComponentMetaPayload` (paired with `RequestKind::ComponentMeta`). `RequestFootprintAudit::loaded_files()` is the exact-read answer; `declared_dependency_files()` is the broader dependency-closure answer. Audit endpoints (`why_loaded` / `why_instantiated`) read from the audit accumulator; TS helpers only render JSON.

NAPI + WASM + LSP consumers route through the shared session-layer materialiser at `crates/verter_session/src/component_meta_materialize.rs` (`materialize_component_meta_structure` entry), backed by graph-native policy predicates in `meta_resolve.rs` (`extract_route_root_identity_node`, `ref_root_reaches_transitive_cycle_node`, `component_meta_ref_resolves_to_package_node`). Benchmark correctness validated by `packages/benchmark/src/audit-validator.ts` against `packages/benchmark/audit-specs/component-meta/*.json`.

### Audited entry-points on `VerterHost`

Component-meta consumers should always go through an audited entry-point so the `RequestAuditRecord` publishes into the host's records store:

| Method                                              | When to use                                                                            |
| --------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `get_component_meta_with_audit(canonical_id)`       | Audited `getComponentMeta`. Drives the cold/warm cache flow and publishes a record.    |
| `get_component_meta_with_resolution(canonical_id)`  | Same producer used by the test harness; returns `(analysis, resolution, record)`.      |
| `take_audit_record(request_id)`                     | Drain a published record by id.                                                        |

Sibling audited entry-points (`resolve_type_with_audit`, `compile_with_audit`, `analyze_with_audit`, `audit_workspace_op`, `lsp_audit_begin`, `audit_mcp_tool_call`) follow the same pattern when the component-meta layer is driven from another surface (LSP, MCP, bundler, workspace ops).

For full architecture, API reference, and debug workflows see [`docs/audit-footprint/`](../../docs/audit-footprint/) and the `/audit-infrastructure` skill.

## Final-Result Cache (post-rewrite)

`get_component_meta(owner)` consults `ProjectTypeStore::component_meta_results()` before running the resolver. The cache is typed `ComponentMetaResultDb<ComponentMetaAnalysis>` and keyed by `(owner_canonical, owner_whole_hash, ComponentMetaQueryKind, options_fingerprint)`.

Flow per call:

1. Look up `shallow_file_state(owner)` for the current whole-hash.
2. Build `ComponentMetaResultKey` with `component_meta_options_fingerprint(&ComponentMetaOptions::default())` — xxh3-128 over a manually-versioned encoding (schema + `compat` + `include_fallthrough`).
3. Try `component_meta_results().get_with_view(&key, &store_view)`. On hit, the entry's `ReadSetSignature.facts` (path-precise fact-tracer observation set) revalidates against the live `StoreView` — `get_with_view` counts a warm hit only when validation passes and the value is returned. Stable signatures return `Arc<ComponentMetaAnalysis>` with zero resolver work.
4. On miss or stale signature, run the existing resolver and publish the result with the transitive fact signature (`ReadSetSignature.facts`).

Cache eviction is automatic: `host.upsert(...)` calls `project_type_store.evict_canonical(owner)`, which `invalidate_owner`s every key for the changed canonical. Workspace-shape shifts (tsconfig / SDK / project-graph) call `bump_project_generation_and_evict`, clearing all result entries.

Direct owner imports take the `OwnerImportSurfaceDb` path via `resolve_owner_direct_import`; no legacy `resolve_imported_type_root` caller survives for direct-owner resolution, though the helper remains the authority for transitive chain walks inside route/barrel code.

## Native Vs Compat (CRITICAL)

The official/native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.

- Fix missing/incorrect metadata in the shared/native owner layer first. Compat should only remap representation when the native payload is already correct enough.
- Rust is the component-meta semantic authority. Resolution, declaration routing, recursion handling, graph construction, payload shaping, and the full Verter API response belong on the native side.
- `@verter/component-meta` must issue one async native request per query and receive the full Verter API response for that query. Do not introduce JS/native follow-up calls to progressively resolve missing types, missing graph nodes, or deferred declarations.
- The only intentional off-spec difference versus `vue-component-meta` is that Verter's native boundary is async instead of sync. Apart from that, compat should behave like a projection over one completed native response, not a coordinator of extra native work.
- Rust should send the smallest semantically complete payload possible. Prefer compact symbolic graph structure, shared arenas/tables, and explicit recursion nodes over oversized expanded trees, raw-text-only fallbacks, or `unknown` when a graph-backed representation is possible.
- JS may decode, reconstruct recursive descriptors, memoize shared graph nodes, and adapt the native response into compat output, but that work must stay mechanical. JS may transform structure but must not recover meaning that native code failed to provide.
- Do not let JS become a second resolver, expander, or semantic authority. Compat must not reinterpret unresolved symbols, invent fallback semantics, or silently repair native payload gaps.
- Do not weaken native TypeScript meaning to imitate Volar formatting. Example: keep native `boolean` as `boolean`; any `true | false` expansion belongs only in compat-specific display/schema logic if we choose to support it.
- Indexed-access members may be resolved/expanded when that improves real type fidelity. Targeted compat expansion such as `Alert['variants']['color']` to the concrete color union is acceptable; blanket ref flattening is not.
- Compat `exposed` parity should derive from a shared cached public-instance surface (e.g. a `ComponentPublicInstance` extraction owned by the host/public-instance path). Do not redefine native `exposed` to mean public-instance unless the public API is deliberately expanded.
- Native-only extensions such as `models`, `acceptedProps`, `acceptedEvents`, `acceptedSurfaceCompleteness`, `rootReachability`, and `fallthroughSurface` are part of Verter's official API. Benchmark them separately from Volar-surface parity instead of treating them as regressions.
- Component-meta type recovery must stay cache-owned. When changing `verter_session`, `verter_session::resolver_core`, or `packages/component-meta` type paths, rely on cached lookup/eval state and expand only on demand; do not rewalk AST/source as a fallback to recover missing types.
- Component-meta registry publication must stay shallow. Publish only the symbols demanded by the current query path; do not eagerly materialize unrelated owner/package helpers just to populate the registry.
- Component-meta companion/file-target selection must stay shallow too. Choosing between runtime and declaration companions may probe cached raw source existence, but must not build export analysis, snapshots, or eval envs just to decide the target file.
- Imported component-meta hydration must stay cache-owned too. Once shallow imported dependency state exists, later alias/registry/fallthrough resolver stages must read only from that cache-owned state and must not jump back to raw snapshot/source builders for imported files.
- Component-meta resolvers must deepen in exactly one place per requested symbol/query path. Do not let a file-level helper widen into sibling symbols/files not on the active declaration route.
- Component-meta metadata/fallthrough projection must stay query-scoped. Reuse the resolved state plus captured `HostStoreView`/session view; do not re-enter a fresh top-level meta/fallthrough query when a resolved query already exists.
- Imported symbol collection for component-meta must stay single-path and lazy. Do not introduce eager collection modes or reparsing fallbacks from stored source text; selected imported symbols must be hydrated through the host-owned cache and resolved via the solver only.
- Resolver, solver, recursion tracking, and graph interning should share one stable semantic identity model: defining symbol identity plus type arguments and any relevant conditional context. Do not let cache keys drift by layer.
- Component-meta output must be deterministic for a coherent snapshot/query. The same inputs should produce the same graph identities, metadata shape, and compat-visible meaning.
- If native code truly cannot represent part of a type, encode that explicitly as a structured unsupported/HardStop-style result with diagnostics/provenance sidecars. Do not silently degrade a representable type to raw text or `unknown`.
- The native response contract must remain explicit and versioned. If the component-meta payload shape changes, do it as a deliberate schema/API cutover rather than compat-side drift.
- Host-owned resolver artifacts, graph artifacts, and encoded payload caches must share one invalidation story. Do not add a second ownership path for the same component-meta query state.
- Raw graph cycles reaching JS without explicit recursion nodes are native bugs, not a normal compat fallback path.

## Component-Meta Heuristic Prevention (CRITICAL)

Component-meta may use heuristics only as meaning-preserving performance gates. It must not use heuristics as semantic policy, result repair, or compatibility recovery.

Forbidden patterns:

- String parsing, regex, source-slice parsing, rendered-type splitting, `rawType` inspection, or display-text matching to recover type meaning in native, bridge, or compat code.
- Compatibility paths that keep old heuristic behavior alive. If native payloads are missing meaning, fix the native producer, extend `@verter/type-ir`, or return a structured unsupported result with diagnostics.
- Shape-scoring a candidate as "better" unless the result carries explicit exactness/provenance proving which candidate is authoritative.
- Using package-boundary, shallow/deep, recursion, cycle, or budget decisions as hidden local predicates. These decisions must be explicit projection-plan policy and must appear in semantic identity or cached-value validation metadata when they can affect output.
- Publishing `unknown` or raw text for a TypeScript construct the typed IR can represent or should be extended to represent.
- Encoding semantic miss, unsupported, cycle, unstable-state, or budget information inside `Unknown.raw`, descriptor `rawType`, or string prefixes. These states are API facts and must be typed.

Allowed performance gates must fail closed. A gate may decide "do not expand yet", "do not cache this degraded result", or "enter the shared resolver"; it must not invent members, flatten aliases, hide unresolved branches, or silently replace representable structure with raw text.

Best-architecture target for component-meta:

- Native Rust owns semantic completeness. The TS bridge and compat layer perform mechanical schema adaptation only.
- Published metadata derives from typed `TypeExpr` / `TypeDescriptor` structures plus explicit exactness/provenance, never from display strings.
- Projection planning is explicit: shallow publication, path walking, package-backed object preservation, cycle handling, and fuse behavior are modeled as policy data, not scattered predicates.
- Unsupported cases are visible API states with diagnostics/provenance, not fallback strings.
- TS adapters may be lossy only through explicit unsupported/partial output. They must not silently erase typed structure to `unknown`, prefer raw display text over typed descriptors, or turn structured degraded state back into strings.

## Component-Meta Completeness Contract (CRITICAL)

Component-meta completeness is part of the public API surface.

- `acceptedSurfaceCompleteness`, expansion exactness, bridge-depth state, unsupported operators, unresolved branches, and budget exits must remain explicit through Rust payloads, protocol conversion, the TS bridge, native component-meta, and compat adapters.
- Missing native data is a native bug or a structured unsupported result, not a compat repair opportunity.
- `Complete` / `Exact` means all required semantic inputs were current and all required branches represented. Stale cache data, missing analysis, unavailable providers, bridge truncation, unsupported operators, and budget exits must produce partial/degraded state and must not warm final-result caches as exact.

## Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo type resolver — analyzer (`verter_semantic::analysis::macros`) → projector (`meta_resolve::projectors`) → registry (`resolver_core::component_meta_registry`) → policy (`component_meta_resolution_policy`) → materialiser (`meta_resolve::materialize`) → JS compat (`@verter/component-meta/compat`) — drives semantic decisions exclusively from the typed IR. Source slicing, regex against type text, hand-rolled type-text splitters, `starts_with("Pick<")` shape sniffing, `path.contains("/node_modules/")` classification, and the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`) are all forbidden inside that pipeline.

**Producer contract** — OXC AST is lowered once during shallow analysis:

- `lower_ts_type(ts_type, source)` (in `verter_semantic::analysis::type_expr_lower`) is the single allowed lowering call. The analyzer takes the OXC `TSType` AST node it already has in scope and stores the resulting `TypeExpr` on the analyzed struct. No source-slice + reparse step.
- `Analyzed*Field` carries the typed form alongside the raw display string:
  - `AnalyzedPropField.type_expr: Option<TypeExpr>` (raw text on `type_annotation`)
  - `AnalyzedEmitField.payload_expr: Option<TypeExpr>` (raw text on `payload_type`)
  - `AnalyzedSlotFieldBinding.binding_expr: Option<TypeExpr>` (raw text on `type_annotation`)
  - `AnalyzedSlotField.return_expr: Option<TypeExpr>` (raw text on `return_type`)
  - `ResolvedLocalType.type_expr` MUST be populated whenever `expanded` is non-empty.
  - `AnalyzedMacro.parsed_type_argument` is populated via `lower_ts_type(first, source)` directly on the OXC AST node, not via source slice + `parse_type_annotation`.
- `ProjectedMacroSurfaces` holds typed `*_expr` fields; the string-keyed `*_type` / `*_annotation` fields are display-only passthroughs and consumers do not parse them back.

**Consumer contract** — every downstream stage walks the typed form:

- The projector reads `field.r#type` (already typed). The "raw-annotation fallback" branch in `reduce_published_field_types` is removed once the producer guarantees the typed form.
- The registry's `collect_component_meta_registry_public_field_refs` drives route extraction from the typed expression; `component_meta_registry_public_indexed_access_route` already takes `&TypeExpr`.
- Policy helpers (`raw_restoration::restore_props_suffix_from_raw`, `slot_preservation::slot_binding_should_preserve_symbolic_raw_type`) take `Option<&TypeExpr>` rather than `Option<&str>`. `parse_indexed_access_from_raw` was deleted — the policy reads the typed source annotation (`PropAnalysis::raw_type_expr` / `SlotBindingAnalysis::raw_type_expr` / `AcceptedPropAnalysis::raw_type_expr`, populated by the analyzer's `lower_ts_type` pass) directly.
- Materialise / cold-resolver consumers (`synthesize_define_props_shape_from_known_surface_with_authority`, `slot_field_function_type_expr`, `resolve_component_meta_parts`) construct `TypeExpr::Function`/`TypeExpr::Object` directly from typed inputs — no `format!("(props: { … }) => RT")` synthesise-and-reparse.
- The JS compat layer (`@verter/component-meta/compat/checker.ts`) walks `prop.type` (`TypeDescriptor` from `@verter/type-ir`) for every semantic decision. `prop.rawType` is display passthrough only — it does not feed any `looksLike*`, `extract*`, `normalize*`, `split*`, `strip*`, `prefer*`, `shouldPrefer*`, `compat*ToString`, or `repairOpaque*` branch. Operator splits use union/intersection tag matching on `TypeDescriptor`, not hand-rolled string operator parsers.

**Workspace classification** — substring tests on canonical paths are banned:

- Use `ResolverContext::workspace_is_workspace_owned(canonical_id)` and `workspace_is_package_backed(canonical_id)`.
- `path.contains("/node_modules/")` and `path.contains("\\node_modules\\")` are forbidden in production source.
- The `workspace_is_*` API is path-agnostic (handles symlinked / pnpm-hoisted / Windows-backslash / workspace-linked-package cases the substring approach silently mishandled).

**Type-role classification is structural, not nominal** — a type's role in a Vue SFC (prop / emit / model / slot) is determined by which macro consumes it: `defineProps` / `withDefaults` / `defineModel` for props, `defineEmits` for emits, `defineSlots` for slots, etc. The structural fact is recorded on `AnalyzedMacro` (`kind`, `parsed_type_argument: Option<Arc<TypeExpr>>`, `type_references: Vec<String>`) and propagated through `resolved.snapshot.macros` / `resolved.snapshot.macro_type_deps`. Identifier-name suffix heuristics (`name.ends_with("Props")` / `"Emits"` / `"Events"` / `"Slots"` / `"Model"`) are forbidden inside the resolver — they are Vue community naming conventions, not type-system facts. Walk `AnalyzedMacro` to compute the macro-participation closure of any ref; do not test the ref's identifier text. Architecture guard `no_role_inference_from_name_suffix` enforces this.

**Single allowed exception** — JSDoc tag-type payloads (`{Type}` text inside `@type`, `@param`, `@returns`, …) are inherently text. They are parsed via `verter_semantic::analysis::jsdoc` / `host_manage::jsdoc_resolve::resolve_jsdoc_tag_type` only. Treating any other text as JSDoc-like to dodge this rule is itself a bug.

**Why** — the resolver was specified as a typed pipeline: read each canonical file once, lower OXC `TSType` once, cache the typed form, walk it. Every time a downstream stage needs to "look at the type" through a regex on a stored string, that round-trip drops generic substitutions / negative literals / brand information / function-param metadata / readonly modifiers / qualified-name segments `lower_ts_type` already preserved. The hand-rolled string parsers (`split_top_level_*`, `find_top_level_char`, `extract_pick_slot_bindings`, `splitTopLevelTypeOperator`) duplicate OXC's TS parser, drift from it as TS evolves, and re-introduce bugs OXC already fixed.

**Diagnosing a violation** — `parse_type_annotation` callers, `format!(...).parse_*` patterns, `starts_with("Pick<") | starts_with("Omit<") | starts_with("Required<") | starts_with("Partial<")` shape sniffing, and `path.contains("/node_modules/")` substring tests are caught by architecture-guard tests in `crates/verter_session/tests/architecture_guards.rs`. The compat-side equivalent (no `prop.rawType` reads inside `buildCompat*` / `looksLike*` / `extract*`) is enforced by an ESLint rule / Vitest assertion in `packages/component-meta`.

**Fixing a violation** — fix the producer (lower the right OXC node, store the right typed field), or extend `@verter/type-ir` with a missing variant. Do not paper over by adding another reparse fallback or another regex.

## Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

**Public contract** (on `ComponentMetaAnalysis` / `FfiComponentMeta` / `ComponentMeta`):

- `props` / `events` -- declared component surface only (unchanged)
- `acceptedProps` / `acceptedEvents` -- computed call-site acceptance surface (declared + inherited)
- `acceptedSurfaceCompleteness` -- `Exact` or `LowerBound` (if any branch is partial/unresolved)
- `rootReachability` -- structural root classification before inheritance resolution
- `fallthroughSurface` -- branch-structured inherited surface after host resolution

**Semantic rules**:

- `inheritAttrs: false` -- no inherited surface
- Unconditional multi-root (fragment) -- no inherited surface
- Root `v-for` -- no inherited surface
- Single native root -- intrinsic attrs/listeners minus declared props/events minus consumed root bindings
- Single component root -- recursive propagation through the child's full public surface
- Conditional single-root branches -- exact union of branch surfaces
- Cycles -- terminate safely as unresolved branches, no invented members
- Unsupported roots (`<component :is>`, `<slot>`, Vue built-ins) -- unresolved branches
- `class` and `style` are never consumed (Vue always merges them)
- `@click` and `:onClick` normalize to the same canonical listener name (`click`)
- Declared props/events always take precedence over inherited names

**Authority chain**: analysis extracts `RootReachability` -- host resolves `FallthroughResolution` -- `get_component_meta()` populates `accepted_*` and `fallthrough_surface` -- FFI maps to JSON -- TS consumes.

**Compat**: mapping-only. Flat Volar `props/events` stay on declared surfaces. Branch-structured inherited data is on `_verter`.

**Key files**:

| File | Purpose |
| --- | --- |
| `crates/verter_semantic/src/analysis/component_meta.rs` | Types + root extraction |
| `crates/verter_semantic/src/analysis/html_intrinsics.rs` | Native intrinsic catalog |
| `crates/verter_session/src/host_manage.rs` | Resolver + cache |
| `crates/verter_protocol/src/types.rs` | Schema DTOs |
| `crates/verter_ffi/src/convert.rs` | Adapter conversion |
| `packages/component-meta/src/types.ts` | TS types |

## Shallow-By-Default Rule (CRITICAL)

**Architectural rule:** types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + `reduce_field_type_expr_with_mode`) enforces.

Concrete contract:

- Plain alias references (`type Foo = ...`) — the published prop type stays as `TypeExpr::Ref { name: "Foo", type_arguments: [] }`. Consumers re-resolve `Foo` through the registry on demand. **The projector does not eagerly inline the alias body.**
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) are recognised as shortcuts and behave identically to a userland implementation that referenced the same keys.
- **Open enumeration domain ⇒ shallow carrier (L1).** The `Pick`/`Omit` object-filter carrier-stop. An object-filter utility (`Pick`/`Omit`) whose enumeration domain (argument 0, the source) is OPEN/undecidable STAYS a shallow carrier under `Navigate` rather than materialising the source — materialising an open source degenerates into full cross-file generic expansion (the `ChatMessages.vue` `Pick<PropsBase<T>, …>` over `generic="T"` hang). Mapped utilities (`Partial`/`Required`/`Readonly`) are guarded NOT by L1 but by their `MappedType` deferred shell (fail-closed when source/keyspace unavailable); `Record` is an index-signature key domain (not finite enumeration) and falls back to a deferred mapped carrier. The predicate `raise.rs::utility_enumeration_domain_is_open_or_unknown` runs a bounded typed-IR (`SemanticNodeData`) walk of the domain: OPEN ⇒ reaches an unsubstituted `TypeParam`, an open conditional (`check`/`extends` still depends on a `TypeParam`), an open `IndexedAccess`/`KeyOf`/`Mapped`, an instantiation over an open arg, an unresolved or open-bodied `DeclRef` alias chain, an `Opaque`, or exhausts the node budget; CLOSED ⇒ a finite object surface / concrete instantiation / finite union/intersection of those, reached without crossing an open node (a bounded alias chain `Foo→Bar→{bar:string}` resolves CLOSED, and an `InstantiationRef` is CLOSED only when its target decl exists with satisfied arity/defaults, every arg/default is closed, AND the prepared body is closed under those bindings). `infer` is a conditional-inference binding placeholder, NOT an unbound generic — it does NOT open the domain (a decidable `extends UIMessage<infer M, …>` over a concrete check stays closed). Closed `Pick<Foo,'bar'>` and `Pick<SimpleBox<string>,'icon'>` (a concrete object-bodied generic instantiation) still materialise path-precisely. (A chained-conditional-bodied concrete source such as `Pick<PropsBase<UIMessage[]>,'icon'>` is NOT carrier-stopped either, but currently yields `semanticMiss` downstream — a separate conditional-reduction gap tracked as a follow-up, not an L1 concern.) Typed-IR only — no string matching. The L1 carrier-stop is the PRIMARY defense for the open-generic class on the `Navigate` projector reduce route: a finite-large legitimate published surface decidable as CLOSED terminates on its own merits and is published. The per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000); the projection keys plus `Instantiate`/`Conditional` count toward it; an armed fuse that trips returns `BudgetExceeded` as a genuine partial (refused warm admission — the no-poison invariant). L1 currently guards the `Navigate` projector reduce route only — open-generic surfaces that recurse on the structural decl-body-lowering route (e.g. a deep `extends`-heritage like `Table.vue`'s `Omit<CoreOptions<T>, …>`) are NOT yet L1-stopped and rely on the armed budget as the runaway backstop (they resolve degraded) pending a route/mode-independent L1 carrier-stop (a tracked follow-up). R6-registry guards: `chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`, `closed_pick_sources_still_materialize_path_precisely`. Co-located unit guard (inline in `raise.rs`, not an R6-registry entry): `utility_enumeration_domain_open_for_unbound_generic_closed_for_concrete`.
- `Omit<Foo, "bar">` — keeps `bar` shallow (excluded from the surface) and materialises the others.
- `Foo['a']['b']` — path-precise: only the `a` and `b` hops are loaded. Other Foo keys never enter the published surface.
- Recursive aliases (`type Self = Pick<Self>`) — TRUE recursive types are NOT supported. The published surface stays as the bare `Ref { name: "Self" }`; the resolver does not attempt unbounded expansion.
- Imported alias names (workspace-owned OR package-backed) — stay shallow regardless of where they live. The rule is the same for `node_modules`-imported aliases as for project-local aliases: the consumer drives any expansion through subsequent lookups.

The projector's reduction step fires only when the input expression carries an operator-shape node (`IndexedAccess`, `KeyOf`, `TypeOf`, `Conditional`, `Mapped`, `Infer`) OR is a bare `Ref` whose declaration body would carry a non-object top-level surface. This is the discriminating boundary between "shallow publication" and "operator collapse"; bare `Ref` to an object alias stays shallow even when the body is fully known.

The projector pipeline is the sole post-projection authority for finalising published field types — no eager per-field materialisation runs at publication time.

**Negative tests** that lock the contract live in `crates/verter_session/src/meta_tests.rs`:
- `published_bare_alias_ref_stays_shallow`
- `pick_materialises_only_named_keys_others_stay_shallow`
- `omit_excludes_named_keys_others_materialise`
- `nested_indexed_access_publishes_only_terminal_path`

## Synthetic Carrier Typed-IR Rule (CRITICAL)

**Architectural rule:** synthetic slot-binding / `defineSlots` binding carriers are minted exclusively as the typed-IR variant `TypeExpr::SyntheticSlotBinding(Arc<SyntheticCarrierKey>)`. The variant's identity is the FULL `(scope_canonical_id, surface_kind, slot_name, binding_name, value_node)` tuple — intrinsic and structurally distinct from any real workspace alias of the same name. There is NO sidecar provenance table, NO host-owned verdict cache, and NO name-only short-circuit. The variant identity IS the carrier-skip signal at every consumer.

Concrete contract:

- The slot-binding graph publisher's no-parser branch (`publish_merged_bindings` in `crates/verter_session/src/meta_resolve/slot_binding_graph.rs`) mints exactly one shape: `TypeExpr::SyntheticSlotBinding(Arc<SyntheticCarrierKey>)`. Parser-path bindings publish the OXC-lowered `binding_expr` and NEVER mint a synthetic carrier.
- Consumers MUST NOT resolve a synthetic carrier's `binding_name` through `TypeRegistry` — the name is intrinsic, not a workspace alias. Reducing through the resolver would re-enter registry collection looking for a type that does not exist (the "same-name poisoning" risk).
- Explicit deepening of a synthetic carrier routes through `ShapeCacheKey::semantic_node_whole(scope, SemanticNodeId(carrier.value_node), mode)` — the same identity used for any regular member-shape route. The carrier itself stays shallow; the value-node hops are reachable via the normal graph identity. Zero production consumers exercise this route today; the positive-proof test at `crates/verter_session/tests/synthetic_carrier_explicit_deepen_proof.rs` proves the cache-key identity round-trip is well-defined for any future consumer that needs it.
- The retired R22 substrate — `CarrierVerdictDb`, `CarrierVerdictSlot`, `CarrierIdentity`, `CarrierVerdict`, `CarrierProvenance`, `CarrierProvenanceTable`, `CarrierValueNodeId`, the `carrier_provenance_table` field, the `carrier_verdicts` accessor on `ProjectTypeStore`, and the `crate::carrier_verdict_db` module — MUST NOT be reintroduced. The static guard at `crates/verter_session/tests/no_carrier_verdict_db.rs` enforces this.

**Architecture guards:** (1) `no_carrier_verdict_db` (file basename) at `crates/verter_session/tests/no_carrier_verdict_db.rs` — walks `crates/*/src/**` and asserts every retired R22 identifier is absent from production source; the same file also hosts `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`, which bans bare `SemanticNodeId(<ident>.value_node)` constructions outside the legitimate cache-route call (narrowing exempts the rustfmt-broken legitimate shape via an upstream-window check). Paired self-tests prove both scanners discriminate. (2) `synthetic_carrier_explicit_deepen_proof` (file basename) at `crates/verter_session/tests/synthetic_carrier_explicit_deepen_proof.rs` — positive executable proof of the explicit-deepen cache route: constructs a `SyntheticCarrierKey`, admits a deep `TypeExpr` under `ShapeCacheKey::semantic_node_whole(scope, SemanticNodeId(carrier.value_node), mode)`, and asserts the legitimate lookup returns the deep type AND that distinct `value_node` / `scope` / `ProjectionMode` discriminate the cache identity.

## Component-Meta Resolver Rules

Canonical component-meta resolver rules. They govern how the shared cross-file resolver operates when serving component-meta queries.

**Resolver ownership rule:** host-backed component-meta and analysis must share one cross-file resolver. Do not build separate resolver logic for script-setup macros, Options API metadata, compat wrappers, or consumer-specific adapters.

**IndexedReady target rule:** the foundational per-file input for component-meta is the post-parse `IndexedReady` artifact, not a separate request-local type-ready layer. Component-meta should start from canonical imports/exports and the owned lowered shallow symbol representation stored in that artifact, then deepen only on demand for the active symbol route.

**Shared-expansion rule:** when component-meta expands a shallow symbol, it must populate the same host-owned route, prepared-declaration, owner-import, and projection caches used by other resolver consumers. Do not create a component-meta-only expansion cache or helper path when the shared resolver can own the work.

**Path-independent cache rule:** component-meta must benefit from and contribute to the same shared semantic caches regardless of entry path. If a projected member or expanded symbol was already computed from another consumer path, component-meta should reuse it. If component-meta computes a reusable result successfully, it should populate the shared cache for later callers.

**Backfill rule:** broader successful results may backfill narrower caches they actually satisfied, but narrower results must not pretend broader work is cached. Cancelled, partial, or budget-exceeded work must not be promoted as warm shared cache entries.

**Two-signal suppression model (A2).** The cache-suppression decision splits into TWO distinct signals carried on `CacheRead` / `QueryBuildOutput` / the in-flight joiner state — keying the warm gate on the wrong one is the bug class:

- `result_is_partial` — the result is itself a PARTIAL (budget exhaustion, cancellation, same-path recursion, walker fatal/pathological). This is the SOLE authority the component-meta final-result cache (`synthesis_should_suppress`) and the `ShapeCacheDb` / `MaterializeStructureDb` warm gates key on. Set at the budget exits, the three walker fatal/pathological paths, and the recursion sentinel. The WHOLE `execute_type_node` cold-build class folds it so a genuinely-incomplete nested subquery cannot surface as a complete-looking `Value` past the warm gate: (a) the mapper-utility surfaces in `build_builtin_utility` (`Partial`/`Required`/`Readonly`/`Record` + the shared `keyof source` reification) thread a folded `result_is_partial` out through the builder's tuple; (b) `source_members_for_published_projection` returns its `ProjectPath` read's partiality, folded by `build_mapped_type` (source-members + the K-independent value hoist) and `build_key_of` (Intersection/Union keyspace enumeration); (c) `build_class_surface` folds the composed instance/static side read; (d) the path walker routes EVERY intermediate-hop / terminal-carrier re-dispatch through `PathWalker::execute_read_folding_partial`, which ORs the nested read's partiality into `self.result_is_partial` (drained by `build_project_path`); (e) the node-returning helpers that cannot carry a `QueryBuildOutput` — the deferred-shell evaluator (`evaluate.rs`), the per-K mapped-member materialisers, and the `.vue` default-instance synthesis — fold via `observe_component_meta_read_suppress`, which raises the request-scoped sticky flag ONLY on this signal. Also folded through the macro-payload nested reads (`build_resolve_macro_payload`), the reducer (`raise_and_reduce`), and the slot-binding graph.
- `cache_suppress` — INNER-MEMO non-cacheability: the value is complete but THIS memo entry can't be admitted (ReturnOnly / signature-overflow / unrootable self-root). Blocks only that inner memo; it MUST NOT suppress a complete component-meta result. All three `component_meta_materialize.rs` ReturnOnly arms — Tainted, unrootable self-root, AND signature-overflow — set `cache_suppress=true` UNCONDITIONALLY (a ReturnOnly is non-cacheable by construction). The Tainted / self-root arms carry `result_is_partial` from the request's genuine accumulated partiality; the signature-overflow arm is a benign non-cacheable COMPLETE outcome carrying `result_is_partial=false` (so the two-signal shape is `cache_suppress=true, result_is_partial=false`) — a complete materialised outcome that merely could not be admitted STILL warms the final cache. This matches the synthetic benign-overflow shape in `component_meta_no_cache_promotion_tests.rs` and the real-path Discrimination #4/#5 assertions in `component_meta_materialize.rs`.

A budget-tripped partial can surface as a COMPLETE `QueryResult::Value` (a `ProjectPath` shallow-walking an `InstantiationRef` whose nested `Instantiate` trips the budget — the walker catches the error and returns a `Value` shell). A value-kind gate (`matches!(value, Error | Recursive)`) MISSES this by construction; the explicit `result_is_partial` field is the only correct authority. A complete-but-non-cacheable result (open `Pick` carrier riding a non-cacheable sub-read) carries `cache_suppress` WITHOUT `result_is_partial` and MUST still warm the component-meta result. Guards: the discrimination proofs in `crates/verter_session/src/component_meta_no_cache_promotion_tests.rs` (`warm_gate_keys_on_result_is_partial_not_value_kind_or_cache_suppress`, the budget-trip fresh-request replays) and `component_meta_pick_omit_tests.rs` (`chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier` warms; `chatmessages_budget_tripped_value_partial_does_not_warm_final_result_cache` does not).

**Navigation-not-expansion rule:** component-meta should navigate intermediate type paths as narrowly as possible. When resolving a path like `A['c']['full']['bar']`, intermediate hops should stay in navigation or shallow projection mode; only the terminal requested projection should expand unless limited normalization is required to continue.

**Generic-instantiation rule:** component-meta navigation and expansion must respect generic substitutions. Member projection or indexed access on instantiated generic types must operate on the substituted meaning, and the shared cache keys must include the relevant type arguments or substitution environment.

**Navigator-boundary rule:** component-meta path walkers are not allowed to become a private resolver. They may do non-owning normalization and choose the next hop, but any operation that could recurse, cross files, instantiate a new semantic node, or populate reusable caches must enter through the shared semantic query API.

**Component-meta rule:** all metadata-producing macro and Options API surfaces must go through the shared resolver in `Expanded` mode. That includes props, emits, slots, data, computed, and expose-style members.

**Traversal rule:** only follow the import graph reachable from the requested type's declaration graph. Unrelated imports in the same file are out of scope.

**Caching rule:** when parsing a `.ts` / `.js` / declaration file for type resolution, cache discovered symbol name to canonical location mappings. Cache direct re-exports, barrelled exports, and any discovered `export *` hops too, because repeated wildcard-barrel scanning is expensive.

**Component-meta cache rule:** when changing `verter_session`, `verter_session::resolver_core`, `verter_semantic`, or `packages/component-meta` type paths, use cached lookup/eval state as the only source of truth after the cache-owning pass. Do not rewalk AST/source as a fallback to recover or expand types.

**Component-meta publication rule:** keep registry publication shallow and demand-driven. Publish and expand only the symbols required by the active metadata query; do not eagerly materialize unrelated owner-local or package-local helpers.

**Component-meta target-selection rule:** keep runtime/declaration companion selection shallow. Canonicalization may probe cached raw source existence, but must not build export analysis, snapshots, or eval envs just to choose a target file.

**Component-meta imported-state rule:** after shallow imported dependency seeding, resolver stages must consume only the imported dependency cache for imported file snapshots/envs/analysis. Do not bounce from alias or registry hydration back into raw snapshot/source builders for imported files.

**Component-meta analysis interop rule:** LSP or other consumers that request richer analysis but still need shallow type retrieval should build on the same `IndexedReady` and shared resolver artifacts. Analysis is an additive layer over the indexed base; it must not bypass the component-meta/type cache path for shallow symbol expansion.

**Component-meta deepening rule:** resolve one requested symbol/query path at a time. Do not let a file-level resolver widen into unrelated sibling symbols/files while chasing a single metadata request.

**Component-meta imported-file rule:** imported files stay shallow-first and symbol-directed. After an imported canonical file is read/processed for the current version, consume only its shallow/export surface first. Do not navigate into its imports unless the requested symbol is present on a direct route from that shallow state, or the file is acting as a barrel and the symbol was not found locally.

**Component-meta barrel BFS rule:** when the current imported file is a barrel and the requested symbol is not present in its shallow/export surface, follow wildcard barrel exports breadth-first by layer. Shallow all barrel children in the current layer, check each for the requested symbol, and only then continue to the next barrel layer. Do not descend one barrel branch depth-first ahead of same-layer siblings, and do not open unrelated imported files while searching that symbol route.

**Component-meta projection rule:** when projecting metadata/fallthrough from an already-resolved query, reuse that resolved state plus the captured store/session view. Do not bounce back out to a fresh top-level fallthrough/meta query.

**Component-meta collection rule:** imported-eval collection must stay lazy/BFS over the active symbol route. Do not add eager collector modes or source-text reparsing fallbacks in shared resolver code.

## Semantic Dispatch Integration (Post Phase-D)

All type expansion for component-meta goes through `ProjectSemanticDispatch::execute(SemanticQueryKey::...)` against the shared `SemanticGraphStore` (see `/type-resolution` for the full authority contract). The component-meta layer is a pure `SemanticQueryApi` consumer — it does not own a separate solver, relation engine, or lowering path.

**Call-site pattern** (plan §9 appendix — canonical migration shape):

```rust
// Retired: owner_engine.solve_scoped(host, scope, &expr) / .solve(&expr)
// Post-cutover:
let base = dispatch.shallow_lower_type_expr(&expr, &env, &scope_node, &name_resolution, &mut substitutions);
let path: Arc<[PathSegment]> = Arc::from([]);
dispatch.execute(SemanticQueryKey::ProjectPath { base, path, context: ProjectionReductionContext::published(Expanded) });
// Then `semantic_node_to_type_expr(host, result)` to round-trip to TypeExpr.
```

`env` (type-parameter bindings) and `name_resolution` (import map) must both be preserved — dropping either causes bare-name misses in declaration-scoped resolution.

**Retired identifiers** (per plan §9 / §5.7 WIP-C):

- `owner_engine.solve_scoped(...)` / `.solve(...)` → `dispatch.execute(ProjectPath { ..., context: ProjectionReductionContext::published(Expanded) })`
- `owner_engine.project_expr_surface_as_type_expr(...)` → same as above
- `engine.solve_expr_type_expr(...)` → same as above
- `engine.project_expr_surface_shape(...)` → `dispatch.execute(ProjectPath { ..., context: ProjectionReductionContext::published(Shallow) })` + surface-shape reader helper
- `engine.expand_local_generic_ref_expr(...)` → `dispatch.execute(SemanticQueryKey::Instantiate { base, args, context })`
- `TypeSurfaceDb::{get, publish, evict_*}` → DELETED; identity lives in `SemanticGraphStore`'s node memo
- `TypeSolverHost`, `EvalEnvSolverHost`, `SessionSolverHost` traits/structs → DELETED; dispatch called directly
- `TypeQueryEngine` → DELETED; `ProjectSemanticDispatch::new(host)` replaces

**Phase 5 query-planner contract.** `ComponentMetaQueryEngine` no longer owns any resolver state. For every Vue macro call site (`defineProps`, `defineEmits`, `defineSlots`, `defineModel`, `defineExpose`, `defineOptions`, `withDefaults`) the engine builds a `SemanticQueryKey::ResolveMacroPayload { owner, macro_index, macro_kind, type_args, context }` (where `context` is a `MacroPayloadContext { resolve_env_hash, mode }`; the SOLE new variant added in Phase 5 §5.0) and dispatches through `ProjectSemanticDispatch::execute`. `ResolveMacroPayload` reuses the sidecar `AnalyzedMacro` (no AST re-walk per §A14) and lowers the body using the existing `Instantiate` / `NormalizeIntersection` / `Object` builders.

The 3 originally-proposed variants — `MaterializeSurface`, `ResolvePublicInstance`, `ResolveFallthroughSurface` — landed as **non-variant dispatch helpers** that compose existing `SemanticQueryKey` variants and read the `ComponentMetaResultDb<ComponentMetaAnalysis>` sidecar. They are not enum variants on `SemanticQueryKey`. The cache-shape rule ("every `SemanticQueryKey` variant dispatches through `SemanticGraphStore::execute_cooperative`") therefore still holds with one new variant added.

**Source-text fallbacks are guard-enforced (Phase 4 / 4b).** The pre-Phase-4 `host.read_source` callsites in `component_meta.rs` and the `DeclarationMetadataResolver::read_source` trait + the three text-projection helpers (`source_for_local_type_projection`, `project_macro_surfaces_from_expanded_text`, `project_macro_surfaces_from_source_type_name`) are deleted. Per-member JSDoc enrichment now flows through `enrich_projected_jsdoc` using `host.resolve_jsdoc_block` (graph-native). The architecture guards `no_read_source_in_component_meta`, `no_read_source_in_declaration_metadata`, `no_text_based_macro_surface_projection_helpers`, and `no_macro_string_heuristics_in_resolver_core` are un-ignored and mechanically enforce the no-fallback invariant on every commit.

**Key resolver files:**

| File | Purpose |
| --- | --- |
| `crates/verter_session/src/project_semantic_dispatch/` | `ProjectSemanticDispatch`, `SemanticQueryApi` impl, build/walk/relate/lower/substitute/evaluate/guards/enumerate sub-modules |
| `crates/verter_session/src/semantic_query_memo.rs` | `SemanticGraphStore` (node memo + relation memo) |
| `crates/verter_session/src/host_manage.rs` | `get_component_meta()` entry point, `HostNamedTypeCacheAdapter` (reads/writes `SemanticGraphStore` directly for Vue macro results) |
| `crates/verter_session/src/host_resolve.rs` | `HostFrontierAdapter`, cross-file type resolution |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/` | `ComponentMetaQueryEngine` — request-scoped query-planner. Builder of `SemanticQueryKey` lists; the engine asks the shared dispatch and assembles `ComponentMetaAnalysis` from the returned `CacheRead<T>` results. **Authority model lives in `mod.rs`'s file-level doc-comment** — read it before adding cache state to the engine. No private durable resolver/expander state; child modules `helpers` / `prepared_surface` / `registry_decl` / `route_keys` / `routed_expr` / `shallow_preserve` / `surface` provide focused method clusters. |

### Engine-internal authority model

The authoritative caches that survive across requests are `MaterializeMemoDb`, `ComponentMetaResultDb`, `SemanticGraphStore`, `RefCycleResultDb`, and `MaterializeStructureDb`. The engine sits above these and below the public component-meta API; it does not own any durable cache state. Per-request scratch (`prepared_surface_cache`, `routed_expr_surface_cache`, `prepared_member_cache`, type-param substitution maps, projection-chain scopes) dies when the engine drops and is never promoted. Cancelled, superseded, interrupted, budget-exceeded, and partial results MUST NOT be admitted to the authoritative caches. The full ownership-boundary contract lives in the file-level doc-comment of `crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs` — read it before introducing new cache state inside the engine.

## Component-Meta Perf / Debug Workflow

When debugging component-meta performance or validating resolver invariants, use the repository-owned benchmark and trace workflow first and treat trace as a correctness/attribution tool, not as the final latency source of truth.

### Tool order

1. Use no-trace benchmark runs to measure real request latency.
2. Use trace runs to validate resolver rules and identify which stage owns the time.
3. Use native profiling only after trace/no-trace runs identify a stable hotspot.

### Benchmark authority

- `query_ms_from_stdout` from `scripts/benchmark/trace-component-corpus.mjs --no-trace` is the best lightweight request-latency number for component-meta iteration work.
- `wall_ms` includes Node/process/bootstrap/teardown overhead and is useful for harness cost, not resolver-only cost.
- Traced runs add overhead; use them to understand route behavior and stage ownership, then confirm wins again with `--no-trace`.

### Trace interpretation

- `trace_resolve_ms` is the primary root `resolve_component_meta` span only.
- `trace_query_ms` is the sum of all root traced spans in the request. Use this when later imported-local work or extraction/fallthrough work is not inside the primary resolve span.
- Trace logs under `<trace-dir>/traces/` are the authority for route correctness:
  - imported files should stay shallow-first
  - imports should deepen only on the requested symbol route
  - wildcard barrels should stay BFS by layer
  - unrelated imported siblings should not be promoted/materialized

### Real-project component-meta commands

```bash
# Targeted no-trace timing for a real nuxt-ui component
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-notrace \
  --filter=Accordion.vue \
  --no-trace

# Targeted traced run for route validation and stage attribution
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-trace \
  --filter=Accordion.vue

# Full no-trace corpus run
node scripts/benchmark/trace-component-corpus.mjs \
  --output-dir=tmp/cm-full \
  --no-trace

# Validate traced output against strict rules / expected artifacts
npx tsx packages/benchmark/src/trace-check.ts \
  tmp/cm-trace \
  --batch "Accordion" \
  --strict \
  --check-expected
```

### Native profiling

For native hotspot attribution on a real `nuxt/ui` component, use the real-project profiler:

```bash
cargo run -p verter_bench --example profile_real_component_meta --release --features=hotpath -- Accordion
```

Useful environment variables:

- `VERTER_PROFILE_PROJECT_ROOT` - override the project root (defaults to `.integration-tests/repos/nuxt-ui`)
- `VERTER_PROFILE_REPEATS` - repeat the request multiple times
- `HOTPATH_METRICS_PORT` - choose a non-default hotpath port if another run is active
- `HOTPATH_METRICS_SERVER_OFF=1` - disable the HTTP metrics server when only local timing output is needed

Practical rule:

- use hotpath/native profiling for relative attribution inside one run
- use the no-trace benchmark as the final guard against real-world perf regressions

For repository-owned component-meta benchmark and profiling commands, see `/build-and-profiling`.
