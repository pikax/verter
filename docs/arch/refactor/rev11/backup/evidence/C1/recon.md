I'll start from the C1 charter as ground truth, then verify every claim against the current tree with concrete citations.Charter is loaded. Next I’ll verify its file counts, line ranges, and module declarations against the current tree.Charter line numbers already look drifted. I’ll lock current `mod` declarations and keep enumerating files without writing anything.Line numbers have drifted: the VerterHost impl is at 850, not 817. I’ll keep verifying the rest of the charter against the live tree.Charter line numbers keep drifting. Next I’ll map call sites, I/O in `ProjectResolver`, and the counter producers.C1 recon against the **current tree**. The charter is still the right shape of work; several of its concrete coordinates have drifted, and a few coupling facts make a literal “move these files” cutover harder than written.

---

# 1. Ground truth vs the charter

## Item 1 — relocate `resolver_core/**` (minus three carve-outs)

**File count: still 59 `.rs` files.** 39 top-level + `component_meta/` (6) + `component_meta_query_engine/` (14). That matches the charter.

**`verter_session` module declarations have drifted.**

| Charter | Current |
|---|---|
| `mod project_semantic_dispatch` at `lib.rs:332` | `pub(crate) mod project_semantic_dispatch` at `crates/verter_session/src/lib.rs:336` |
| `pub mod resolver_core` / `mod resolver_store` at `lib.rs:341-344` | `pub mod resolver_core` at `:345`; `mod resolver_store` at `:346` |

Line 332 is now `mod parse;`. Line 341 is `mod public_api_batch_fixed_view_tests;`.

**The VerterHost impl block has drifted and is not the range `:817-1343`.**

| Charter | Current |
|---|---|
| `impl ResolverContext for VerterHost` at `:817-1343` | Starts at `resolver_context.rs:850`, ends at `:1304` |
| “VerterHost-specific `sealed::Sealed` / `RequestBoundSealed` impls” inside that range | `impl sealed::Sealed for crate::VerterHost` is **outside** it, at `:778`. `RequestBoundSealed` is **never** implemented for `VerterHost` (comment at `:820-821`). `:821-836` is `RequestBoundSealed` + `RequestBoundResolverContext` for the two wrappers only |

`:817-836` today is the request-bound marker impls for `HostResolverContext` / `SessionResolverContext`, which **must stay with the adapters**, not travel with a “delete this range” cut.

**The three named carve-outs are incomplete.** The seal guard already exempts a **fourth** production `&VerterHost` file:

```3652:3657:crates/verter_session/tests/cases/architecture_guards.rs
                // All four are seal-bridge exemptions per sub-plan
                // §10a.0.A.
                n == "resolver_context.rs"
                    || n == "session_resolver_context.rs"
                    || n == "host_resolver_context.rs"
                    || n == "request_store_view.rs"
```

`request_store_view.rs` takes `&crate::VerterHost` in production (`complete_canonical` at `:499-505`, `complete_canonical_with_session_view` at `:529-537`, `complete_canonical_inner` at `:583-588`). The charter never names it.

## Item 2 — relocate `verter_workspace::resolver::ProjectResolver`

**Line count matches: 2122 lines.** File ends at `crates/verter_workspace/src/resolver.rs:2122` (`mod resolver_tests;`). `pub struct ProjectResolver` is still at `:122`.

This is **not** pure computation. `resolve_with_reader` at `:321-352` takes `&dyn crate::traits::WorkspaceRead`. `resolve_tracked` at `:354-361` takes `&crate::engine::TrackedResolutionCapability` plus `&crate::resolution_currency::TransactionReader`. Path probing is live I/O (`probe_path` `:1193`, `probe_path_for_context` `:1225`, `reader.realpath` `:1265`, package-manifest probe `:1659`). C1-AC-9 is real work, not a no-op move.

## Item 3 — delete `verter_semantic → verter_workspace` and the exception row

**Cargo edge is still there:** `crates/verter_semantic/Cargo.toml:28` (`verter_workspace = { path = "../verter_workspace" }`).

**Exception row has drifted:** charter cites `workspace_dependency_layers.rs:118-127`. Today `ratified_upward_exceptions()` is at `:139-148`; `"verter_semantic"` is inserted at `:145`. The test `workspace_production_closures_never_cross_upward_except_the_recorded_exception` is at `:338`. `RATIFIED_ROOT_CRATES` still lists `"verter_semantic"` at `:335`.

**`verter_compiler` Cargo.toml:59 is stale.** That line is the `test-support = []` feature comment. `verter_semantic = { path = "../verter_semantic" }` is at `crates/verter_compiler/Cargo.toml:75`.

`verter_workspace` currently has **no** `verter_semantic` dependency. Moving `fact_registry` down forces a **new** inward `workspace → semantic` edge (allowed) or a split of fact types.

## Item 4 — `AttemptOutcome` / `NeedInputs` / `LoadSet`

**Confirmed absent in Rust.** `NeedInputs|LoadSet` has **zero hits under `crates/`**. The types exist only as the contract sketch in `docs/arch/refactor/rev11/contracts/input-loading.md:12-16`.

## Item 5 — collapse Host/Session delegation

The duplication is real; **line numbers drifted ~4–8**.

| Method | Charter host vs session | Current host vs session |
|---|---|---|
| `is_request_bound` | `:193-195` vs `:187-189` | `:193` vs `:187` — match |
| `request_completion_overlay` | `:221-224` vs `:234-237` | `:222` vs `:238` |
| `store_view` | `:321-324` vs `:508-511` | `:325` vs `:512` |
| `aggregate_basis_seed` | `:326-329` vs `:513-516` | `:330` vs `:517` |
| `dispatch` | `:494-500` vs `:674-680` | `:498` vs `:678` |
| `resolve_imported_type_root` | `:343-372` vs `:530-559` | `:347` vs `:534` |
| `resolve_type_declaration_for_dep` | `:431-450` vs `:616-634` | `:435` vs `:619` |
| constructor trio | `:96-149` vs `:103-150` | host `new` is **cfg-gated** (`:97-107`); production ctors are `from_current` `:118` / `from_cold_seed` `:140`. Session `new` is **cfg-gated** (`:104-116`); production ctor is `from_cold_seed` `:139` |

Session-specific overrides still exist, at drifted lines: `complete_canonical` inherent `:171-180`, `shallow_file_state` `:283-291`, `authoritative_current_content_hash` starts `:307`, `observe_materialize_scope` `:361`, `indexed_for_current_content` `:425`, `resolve_type_dependency_canonical` `:595`, `active_session_view` `:716-718`.

## Item 6 — delete bare `impl ResolverContext for VerterHost`

The impl is still present (`:850-1304`). Panic-in-production arms still exist (`:866-869` pattern). Guard `crates/verter_session/tests/cases/g_misc1/no_bare_host_resolver_shims.rs:1-37` **requires** that impl and those panic bodies to remain.

Production still compiles `ProjectSemanticDispatch::new(self)` on a bare `VerterHost` via `VerterHost::semantic_dispatch` (`host_construction.rs:662-665`) — documented as a test-bridge but **not** `#[cfg(test)]`. Deleting the impl is a compile break for that accessor and every `ProjectSemanticDispatch::new(host)` test (dozens), unless they wrap first.

I did **not** find a production resolver-tier entry that still *needs* the bare rail: live entries use `HostResolverContext::from_current` / `from_cold_seed` (e.g. `typeinfo/shallow_surface.rs:95`, `typeinfo/raise.rs:111`, `host_manage/prepared_decl.rs:128`) and `SessionResolverContext::from_cold_seed` (e.g. `host_manage/component_meta_entry.rs:383`).

## Item 7 — `routes.rs` six `WorkspaceRead` sites

**Exact match:** `crates/verter_semantic/src/analysis/routes.rs:196,251,661,672,869,1120`.

| Line | Function |
|---|---|
| 196 | `detect_routing_framework` (live `read_file`) |
| 251 | `discover_router_configs` (`file_exists`) |
| 661 | `extract_file_based_routes` (`is_dir`) |
| 672 | `extract_file_routes_recursive` (`read_dir`) |
| 869 | `discover_layouts` (`is_dir` / `read_dir`) |
| 1120 | `build_route_analysis` (orchestrator) |

Pure extractors: `detect_routing_framework_from_json` at `:209` (charter `:208-233`); `extract_programmatic_routes` at `:272` (charter `:266-289`). Close enough.

## Item 8 — move `fact_registry` vocabulary

Still at `crates/verter_workspace/src/fact_registry.rs`. Types: `FactDomain` `:227`, `FactKey` `:269`, `Fact` `:557`, plus `FactRegistry` `:606`. Semantic re-export is still one line: `crates/verter_semantic/src/facts/registry.rs:3` (`pub use verter_workspace::fact_registry::*;`). The `From<crate::analysis::MacroKind>` impl at `:5-18` is real analysis-dependent logic that must stay in semantic when the re-export dies.

---

# 2. Real owner-module declarations

## `verter_semantic` today (`crates/verter_semantic/src/lib.rs:23-33`)

```
pub mod analysis;
pub mod analyzers;
pub mod db;
pub mod extract;
pub mod facts;
pub mod input;
pub mod profile;
pub mod query;
pub mod refs;
pub mod revision;
pub mod snapshot;
```

Nothing named `resolver_core`, `project_semantic_dispatch`, `ModuleResolverCore`, or `TypeInfoCore` exists. `analysis/mod.rs:56` already has `pub mod project_resolver`.

**After C1, `verter_semantic/src/lib.rs` needs to declare** (exact intra-crate layout is an implementation judgment the charter left open):

- the relocated `resolver_core` tree (minus adapters)
- `project_semantic_dispatch`
- immutable observation types currently in `resolver_store`
- `ModuleResolverCore` (relocated `ProjectResolver`)
- `facts/registry.rs` as the owner of `FactKey`/`FactDomain`/`Fact`, not a re-export
- the new observation interface + `AttemptOutcome`/`LoadSet`

`analysis/project_resolver.rs` stays as a module path; only its `:16-25` re-export half is deleted.

## `verter_session` today

```
336: pub(crate) mod project_semantic_dispatch;
345: pub mod resolver_core;
346: mod resolver_store;
349: pub mod semantic_query;   // NOT in C1 move set
```

`pub use crate::project_semantic_dispatch::locator_shape::LocatorShapeCtx` at `lib.rs:214` is a **public** name on a `pub(crate)` module.

`pub use resolver_store::{ dump_from_host_call_sites, reset_from_host_call_sites, ... }` at `:424-427`.

**After C1, session keeps** `host_resolver_context.rs`, `session_resolver_context.rs`, the VerterHost impl (or deletes it), `request_store_view.rs` (missed carve-out), and `StoreViewManager` (`resolver_store.rs:3889`). It stops declaring the moved kernel. `semantic_query` at `:349` is **not** in the charter’s move set, but the moving code depends on it heavily (see §7).

`resolver_core/mod.rs:39,42,46` currently declares the adapter modules:

```
pub(crate) mod host_resolver_context;
pub(crate) mod resolver_context;
pub(crate) mod session_resolver_context;
```

After the split, session still needs those three (plus `request_store_view`).

## `verter_workspace` today (`lib.rs:103`, re-export `:183-188`)

```
pub mod resolver;
pub use resolver::{ ..., ProjectResolver, NativeProjectResolver, ... };
```

That `pub mod resolver` / `pub use resolver::{...}` is what C1 deletes. Downstream currently reaches the same types via:

- `verter_workspace::resolver::*` / `verter_workspace::ProjectResolver`
- `verter_semantic::analysis::project_resolver::*` (re-export)
- `verter_lsp::project_resolver` (`crates/verter_lsp/src/project_resolver.rs:1` = `pub use verter_semantic::analysis::project_resolver::*;`)

The LSP shim is **not** in the charter. Deleting only `:16-25` of semantic’s `project_resolver.rs` **breaks** `verter_lsp::project_resolver::{NativeProjectResolver, IdeProjectConfig, ...}` unless those names are re-exported from the new semantic owner.

---

# 3. Call sites into what’s moving

## `verter_session::resolver_core::*` from **outside** `verter_session`

**No production crate `use`s the module.** Hits outside session are:

- **benches:** `crates/verter_bench/benches/fact_validation_hot_path.rs:29`, `fact_tracer_warm_hit_zero_alloc.rs:79`
- **comments only:** `verter_workspace` (`engine.rs:4440`, `module_resolution.rs:7,16,48`, `ambient_parse.rs:6`, `fact_registry.rs:139`), `verter_type_runtime` (`lib.rs:13-26`, `backend.rs:7,149`), `verter_audit` (`structured_event.rs:26-28,405`, `observer.rs:351`, `payloads/type_resolution.rs:4,13`, `footprint.rs:632`)
- **generated TS comments:** `packages/types/audit.generated.ts:2375,2759,3288`

Inside session, `pub mod resolver_core` is the in-crate API. Hundreds of `crate::resolver_core::` uses stay as same-crate imports until the module moves, then they become `verter_semantic::…` (or a session re-export). That is the real blast radius, not cross-crate `use`s.

## `verter_session::resolver_store::*` / `project_semantic_dispatch::*` from outside session

- `resolver_store`: no external `use`. Private `mod` (`lib.rs:346`).
- `project_semantic_dispatch`: trybuild compile-fail tests (`tests/cases/compile-fail/output_projector_not_impl_outside_crate.rs:23-27`, `locator_reducing_lowerer_not_nameable.rs:8`); comments in `verter_audit/src/footprint.rs:506` and `verter_semantic/src/analysis/type_solver/builtin.rs:6,86`. The public name `LocatorShapeCtx` is re-exported at session `lib.rs:214`.

## `verter_workspace::resolver::*` from outside workspace

Production (non-test) users:

| Crate | What |
|---|---|
| `verter_semantic/src/analysis/project_resolver.rs:16-21` | re-exports the whole resolver + helpers |
| `verter_tsc/src/checker.rs:1861,1930` | `is_relative_specifier` |
| `verter_session` `file_artifact_store.rs:3848,3902`, `host_cache_runtime.rs:248,424`, `host_manage/fallthrough.rs:234`, `external_ts/identity_resolver.rs:43`, `external_ts/resolver.rs:21`, `typeinfo/vue_macro_codegen/tsc_projection.rs:1172` | path helpers / `path_is_carrier` / `collapse_path` |
| `verter_lsp` `server/provider_state.rs:409+`, `server_utils.rs:313`, `config.rs:528+`, `tsgo/project_binding.rs:27`, `tsgo/composite.rs:40`, `external_ts/carrier_sync.rs:440` | `path_is_carrier` / `normalize_canonical_id` |
| `verter_lsp/src/project_resolver.rs:1` | full semantic re-export of workspace resolver types |
| `verter_workspace` itself | `engine.rs:2277`, `project_graph.rs:172`, `workspace_snapshot.rs:22,46`, `snapshot_builder.rs:14,430` — **engine/snapshot own a `ProjectResolver` field** |

`WorkspaceSnapshot` **embeds** `pub resolver: ProjectResolver` (`workspace_snapshot.rs:46`). Relocating the type into semantic means this snapshot DTO either depends on semantic (new inward edge) or the field type is replaced. The charter does not mention this embedding.

## `verter_workspace::types::*` re-exported from semantic `project_resolver.rs:22-25`

```
ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase,
ResolveRequest, ResolveRequestKind, ResolveResult
```

Those live in workspace `types` and are crate-re-exported at `verter_workspace/src/lib.rs:199-203`. Semantic’s `:22-25` is the analysis-facing alias. Napi/wasm do **not** import these via that path; they only call the two functions.

## napi / wasm callers — coordinates drifted

| Charter | Current |
|---|---|
| `verter_napi/src/lib.rs:2095,2117` | **fn** at `:2093` / `:2111`; **call** at `:2102` / `:2124` |
| `verter_wasm/src/lib.rs:640,667` | **fn** at `:631` / `:649`; **call** at `:640` (match) / `:667` (match) |

Module path `verter_semantic::analysis::project_resolver::*` is still correct. If only the re-export half is deleted, these two functions stay put and those callers do not need repointing.

---

# 4. Existing second-implementation risk for `AttemptOutcome`

**Do not reuse an existing type as `AttemptOutcome`.** Nothing in `crates/` is that enum.

Adjacent vocabulary (different jobs):

| Shape | Where | Why it is not `AttemptOutcome` |
|---|---|---|
| Contract sketch `AttemptOutcome::{Complete, NeedInputs, Terminal}` | `docs/arch/refactor/rev11/contracts/input-loading.md:12-16` | docs only |
| `verter_semantic::query::QueryResult` with `Completeness::{Complete, Partial, Unavailable}` and `missing_inputs: Vec<SemanticDependency>` | `crates/verter_semantic/src/query.rs:12-37` | public query **envelope**, not a kernel attempt outcome; `Partial` is a result, not “load these keys and retry” |
| `EnumMemberValue::Deferred` | `verter_semantic/src/analysis/type_eval.rs:305` | enum-literal lowering, unrelated |
| `SelectedSignature::Deferred` | `project_semantic_dispatch/dispatch_txn.rs:664` | overload selection |
| `ProvenanceNeed` | `semantic_query/demand.rs:339` | provenance demand, not I/O |
| `FlightInner::Pending` | `resolver_core/mod.rs:2617` | singleflight lane state (blocking wait) |
| `PendingExternalSymbol` | `resolver_core/mod.rs:120` re-export from `external_type_frontier` | frontier worklist |
| `ResolutionOutcome` | `verter_workspace` resolution-currency | fact-admission product, not NeedInputs |
| `CacheRead` / `ReturnOnly` / `SignatureAdmission::NonCacheable` | session cache runtime | warm-admission, not missing-bytes |

`input-loading.md` is the spec to implement, not an existing type to reuse.

The closest **behavioral** analog is the blocking load loop: `ensure_loaded` (`host_lifecycle.rs:954`) → `scheduler.wait_or_drive` (`:1013`). C1 adds a sibling outcome; it does not refactor this loop.

---

# 5. Blocking-wait audit

## `SingleflightGroup` — charter line range is stale; wait is real

| Charter | Current |
|---|---|
| `run`/`run_retaining` at `resolver_core/mod.rs:2116-2214,2595-2639` | `struct SingleflightGroup` `:2117`; `run` `:2377`; `run_retaining` `:2404`; **`Condvar::wait` at `:2617-2618`** (inside `run_retaining`’s follower loop) |

`run` is a thin wrapper over `run_retaining` (`:2389`). Any NeedInputs-capable caller that still hits `run`/`run_retaining` **parks the thread**.

`route_db_singleflight.rs:70-146` is still the function; it **calls** `run_retaining` at `:141-146`. That is a Host/Session-reachable blocking path today (RouteDb is used from resolver-tier resolution). For I/O-free coverage it needs a peek-and-decline arm, not just “it lives in session.”

## `prepared_decl.rs` `build_gate`

Field is at `:37` (`parking_lot::Mutex<()>`). Lock sites `:962`, `:1177`. Comment `:28-30` says waiters block cooperatively. This serializes **cold prepared-decl builds**. Reachable from `ResolverContext::prepared_*` on Host/Session. An I/O-free observation interface that still calls these methods will block.

## `ensure_loaded` / `wait_or_drive`

Charter `host_lifecycle.rs:953,1012` → current `:954` (`ensure_loaded`) and `:1013` (`self.scheduler.wait_or_drive`). Trait methods: `resolver_context.rs:294-299`; Host override `:267/:285`; Session override `:243/:270` (session `ensure_loaded` goes through `host_manage::overlay_priority::ensure_loaded_with_view` at `:271`).

**Classification:**

| Primitive | Blocking-lifecycle only? | NeedInputs path? |
|---|---|---|
| `ensure_loaded` → `wait_or_drive` | Yes, if only Host/Session call it | Must **not** be called from the observation interface; that is the Fork-3 ruling |
| `SingleflightGroup::run_retaining` Condvar wait | Used by RouteDb, which the kernel uses | **Not** confined to adapters. Full coverage requires a non-blocking peek |
| `prepared_decl` `build_gate` | Used by kernel prepared-decl | Same: peek-or-NeedInputs, or the slot machinery stays session-side |
| `route_db_singleflight` | Kernel RouteDb | Same |

`host_for_fact_tracer_install` (`resolver_context.rs:750`) is the host escape hatch. Charter cited `:780-782,261-266,700-717`. Current: method at `:750`; default `normalized_analysis_canonical` at `:725-731` **calls it**; `cancellation_token` default at `:188-197` names `verter_scheduler`. Those defaults cannot move into `verter_semantic` as written.

---

# 6. Test surface that must stay green / sibling-relocate

## In-tree `resolver_core` tests

- `bare_name_resolve_namespace_tests.rs`, `bracketed_generation_tests.rs`, `prepared_decl_tests.rs`, `surface_projector_tests.rs`
- `component_meta/tests.rs`, `component_meta/native_props_rehome_contract_tests.rs`
- `component_meta_query_engine/tests.rs`, `component_meta_query_engine/fallthrough_value_eval_recursion_tests.rs`

`resolver_core/mod.rs:31-32` wires `surface_projector_tests`.

## Dispatch invariants

`crates/verter_session/src/lib.rs:224`: `mod project_semantic_dispatch_invariants_tests;` → `project_semantic_dispatch_invariants_tests.rs`. Relocates with the module (C1-AC-3).

Many `project_semantic_dispatch/*_tests.rs` files sit next to production code (`call_resolve_tests.rs`, `cycle_gate_tests.rs`, `tests.rs`, …). They move with the tree.

## Authority-uniqueness scanner

`architecture_guards.rs:3384` **`mod resolver_context_seal`** — charter line is current.

Scan roots at `:3666-3674`:

- `crates/verter_session/src/resolver_core`
- `…/meta_resolve`
- `…/project_semantic_dispatch`
- plus `component_meta_caches.rs`, `component_meta_materialize.rs`

After the crate move, **those paths must be updated** (charter already says this). The four seal-bridge filenames at `:3654-3657` must keep matching whatever stays in session.

Related: `no_bare_host_resolver_shims.rs` greps `resolver_context.rs` for the VerterHost impl. If the impl is deleted, **this guard must be rewritten or deleted in the same change**, not left failing.

## Dependency-layers guard

`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`:

- `ratified_upward_exceptions()` `:139-148`
- `workspace_production_closures_never_cross_upward_except_the_recorded_exception` `:338`
- `RATIFIED_ROOT_CRATES` `:335` still includes `"verter_semantic"`

C1-AC-2 is: remove the `"verter_semantic"` map row **and** drop it from `RATIFIED_ROOT_CRATES` if it is no longer an exception root. The charter only names the map row.

## Other characterization that will break on a naive move

- `crates/verter_session/tests/cases/compile-fail/raw_resolver_entry_points_are_private.rs:7` (`use verter_workspace::resolver::ProjectResolver`)
- `compile-fail/output_projector_not_impl_outside_crate.rs` / `locator_reducing_lowerer_not_nameable.rs` (path `verter_session::project_semantic_dispatch::`)
- `verter_workspace/src/resolver_tests.rs` (sibling of `resolver.rs`; must move or keep testing a re-export)
- `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs` (hard-codes `ProjectSemanticDispatch` paths)
- `verter_audit/tests/cases/member_edge_provenance_arch_guard.rs:116-117` (`crates/verter_session/src/project_semantic_dispatch/mod.rs` / `raise.rs`)

---

# 7. Edge cases the charter implies but does not spell out

## `ResolverContext` cannot move as a unit into `verter_semantic`

The trait is `pub(crate) trait ResolverContext: sealed::Sealed` at `resolver_context.rs:161`. Production methods **name session- and scheduler-only types**:

- `fn host_for_fact_tracer_install(&self) -> &crate::VerterHost` (`:750`)
- default `cancellation_token` returns `verter_scheduler::cancellation::CancellationToken` and calls `verter_scheduler::cancellation::current_job_cancellation_token()` (`:188-197`)
- `ensure_indexed_ready_serve` returns `crate::host_manage::prepared_decl::IndexedReadyServe` (`:294-297`)
- `project_type_store` returns `&Arc<ProjectTypeStore>` (`:457`)
- `config` returns `&HostConfig` (`:459`)
- default `authoritative_current_content_hash` calls `self.host_for_fact_tracer_install()` (`:332-334`)

Fork 3 says the **observation interface** must not extend `ResolverContext` because it would inherit `ensure_loaded` / the host hatch. The charter still relocates **this same trait** into `verter_semantic`. That is only possible after stripping every unnameable type from the trait (or keeping the trait in session and putting a **different** sealed trait in semantic). The charter’s “trait + sealed module relocate; adapters stay” split is the right idea; the current trait body is not dependency-neutral.

## Fourth production `&VerterHost` file: `request_store_view.rs`

Not a fourth `ResolverContext` implementor (only three `Sealed` impls: `:778-782`). It **is** a fourth production host-holding file the move list missed. `CanonicalCompletionOverlay::complete_canonical` writes through host epoch / scheduler / `derived_raw_cache` / `project_type_store` (guard comment `:3645-3651`). If `resolver_core/**` moves wholesale, this file cannot go with it.

## `component_meta/` + `component_meta_query_engine/` are not dependency-neutral

Charter claim: neither subdirectory holds `&VerterHost` in production (true for fields/params). They **do** import the rest of `verter_session`:

`component_meta/mod.rs:236,314,361-474` — `crate::meta_resolve::SurfaceNodeIdentities`, `crate::typeinfo::framework_surface::vue_exec`, `crate::host_executor::HostSourceData`, `crate::typeinfo::framework_surface::svelte_exec`, `crate::request_context`.

`component_meta/native_props.rs:6-10,106,138` — `crate::semantic_query`, `crate::typeinfo::surface`, `crate::meta_resolve`, `crate::typeinfo::raise`.

`component_meta_query_engine/` production: `crate::project_semantic_dispatch` (would move), plus **staying** session modules: `semantic_query`, `meta_resolve`, `component_meta_materialize`, `component_meta_caches`, `fact_signature_helpers`, `cache_runtime`, `request_context`, `structural_carrier_producer`, `types`.

Moving those subdirs into semantic **without** those owners creates `verter_semantic → verter_session`, which C1-AC-2 forbids. The charter’s “wildcard, not a carve-out” is the wrong cut unless `semantic_query` / `meta_resolve` / caches / typeinfo framework-surface also move (out of C1’s stated scope) or the query-engine stays in session.

## `HostStoreView` / `StoreViewValidationToken` are not crate-independent values

`HostStoreView` (`resolver_store.rs:1463-1525`) has no `&VerterHost` field (good), but fields include `crate::resolver_core::StoreViewCompatToken` and `Arc<crate::store_view_roots::StoreViewMemo>` (`:1465,1524`).

`StoreViewValidationToken` (`:428-558`) includes `crate::file_artifact_store::ProjectIdentity` (`:548`). Comments name `VerterHost::current_store_view_epoch`. Moving “just the structs” without `StoreViewMemo` / `ProjectIdentity` / snapshot types does not compile.

`from_host` in the charter is actually `HostStoreView::from_host_read` at `:1867` (takes `&VerterHost`, lives with the manager). Comment at `lib.rs:420` still says `HostStoreView::from_host`.

## `ProjectResolver` is embedded in workspace snapshots/engine

`WorkspaceSnapshot.resolver: ProjectResolver` (`workspace_snapshot.rs:46`). `engine.rs:2277` and `project_graph.rs:172` construct it. `resolve_tracked` (`resolver.rs:354`) is workspace-engine-coupled (`TrackedResolutionCapability`, `TransactionReader`). C1-AC-9 converting “any I/O” to NeedInputs has to replace **live `WorkspaceRead`**, not just six route extractors.

`verter_workspace/src/module_resolution.rs` is a **separate**, content-free vocabulary (`ModuleResolutionMode`, `SpecifierKind`) that already documents the walker as living in `verter_session::resolver_core` (`:7-17`). That is not a second resolver engine; it is a second module that will keep pointing at the old path after the move.

## Doc-comments / imports in the 59 files

Almost every relocating file uses `crate::…` session paths (`resolver_core`, `semantic_query`, `project_semantic_dispatch`, `host_manage`, `request_context`, `fact_signature_helpers`, `session_view`, `file_artifact_store`, `types`). Those become either:

- `verter_semantic::…` (if the target also moved), or
- an illegal upward edge, or
- a new session-side trait the kernel calls through `AttemptOutcome`.

This is the bulk of the mechanical work and is not listed as a file-by-file import rewrite in the charter.

---

# 8. What makes the charter impossible or materially harder

## Not a fourth `ResolverContext` implementor

Exactly three production `Sealed` impls (`resolver_context.rs:778-782`): `VerterHost`, `HostResolverContext`, `SessionResolverContext`. Abort condition “fourth production lifecycle” does **not** fire.

## The real abort-class problems

**A. The trait-as-specified cannot live in `verter_semantic`.** Default bodies and signatures name `VerterHost` and `verter_scheduler`. Putting that file in semantic either (1) reintroduces the upward edge C1 deletes, or (2) requires a trait split the charter under-specified: session keeps `ResolverContext` (blocking, host-capable); semantic gets a new observation trait with `AttemptOutcome` on every method (C1-AC-5). That split is the Fork-3 ruling; the “relocate the existing trait” sentence fights it.

**B. `component_meta_query_engine` + `semantic_query` coupling.** Moving the query engine without `semantic_query` / `meta_resolve` / caches creates `semantic → session`. Not moving it leaves TypeInfo/component-meta kernel in session, contradicting “TypeInfoCore lives in semantic.” This is the most likely way to accidentally grow a **second query-time path**: leave `ProjectSemanticDispatch::execute` in session *and* add a semantic-side facade, or leave `verter_semantic::query::QueryResult` as a parallel envelope. Today there is still one choke point: `execute_via_cold_build_helper_with_publication_capture` at `project_semantic_dispatch/mod.rs:2206-2214` (`verter_audit::attribute_scope!(SemanticDispatch)` at `:2214`). `SemanticQueryApi::execute` is at `:3240` and funnels here. Charter’s `:2206-2214` as “`execute`” is the wrong function name but the right choke point.

**C. `ProjectResolver` is not a leaf.** Snapshot/engine/LSP re-export/`resolve_tracked` bind it to workspace I/O. NeedInputs conversion is a protocol change for every `WorkspaceRead` probe (`probe_path`, `realpath`, package.json reads around `:1654-1663`). If that cannot be snapshotted, A5-DD1 cannot close without pulling `WorkspaceRead` down — which Fork 2 forbade.

**D. Deleting the `:16-25` re-export without a replacement breaks `verter_lsp/src/project_resolver.rs:1`.** Napi/wasm functions are safe; the LSP type re-export is not mentioned.

**E. Second engine risk from `verter_semantic::query`.** `query.rs` already describes a pure, I/O-free semantic engine (`lib.rs:9-12`: “Queries do **not** perform I/O”). After C1, that module and relocated `ProjectSemanticDispatch` must be **one** engine, not two public resolve APIs. `ProjectSemanticDispatch::new` (`:507`) currently accepts `&dyn ResolverContext`, and the comment at `:500-505` still advertises `&host` upcast via the VerterHost impl. Deleting the impl is good for C1-AC-4; every remaining `new(host)` must become `new(&host_ctx)`.

**F. `VerterHost::semantic_dispatch` (`host_construction.rs:662-665`)** is a live compiled (non-cfg-test) constructor of dispatch on a bare host. Integration tests call it (`tests/cases/g_misc0/cross_owner_materialise_reuse_production.rs:255`). If the bare impl dies, this accessor must wrap `HostResolverContext` internally or be cfg-gated.

---

# 9. `A6_META_COMPILE_40_COLD_RUST`

Charter coordinates **match**:

- Cell: `performance-gates.toml:125` (`id = "A6_META_COMPILE_40_COLD_RUST"`, `owner = "verter_session"`)
- `session.semantic_cold_build.calls`: `:296` (absolute_max 1063)
- `session.semantic_dispatch.calls`: `:321` (absolute_max 4216)

**Who emits them** (must not change identity across the crate move):

Schema: `crates/verter_audit/src/attribution/schema.rs:254-255`

```
SemanticDispatch  => "session.semantic_dispatch"
SemanticColdBuild => "session.semantic_cold_build"
```

Producers, both in `project_semantic_dispatch/mod.rs`:

1. **Dispatch total** — `verter_audit::attribute_scope!(SemanticDispatch)` at **`:2214`**, inside `execute_via_cold_build_helper_with_publication_capture` (`:2206`). Comment `:2211-2213`: both `execute` and `execute_read` funnel here. That is the cell’s `session.semantic_dispatch.calls`.

2. **Cold builds** — after cooperative execute, `if is_cold { verter_audit::attribute!(SemanticColdBuild); }` at **`:2821-2822`**. Warm sibling is `SemanticWarmHit` (`:2824`), intentionally **not** gated (`performance-gates.toml:316-319`).

`record_dispatch_intent_counters` (`:965`) bumps **request-context hop/projection counters**, not the A6 attribution counters.

A crate move that keeps these two `attribute!` / `attribute_scope!` sites on the same control-flow will not itself change the counters. Changing `execute` so a blocking lifecycle extra-wraps `AttemptOutcome` on the hot path **would** (charter’s allocation bound). The cell owner string `owner = "verter_session"` (`:126`) will be a lie after the producer lives in `verter_semantic`; that is metadata, not the metric.

`HostResolverContext::dispatch` / `SessionResolverContext::dispatch` (`:498` / `:678`) only construct `ProjectSemanticDispatch::new(self)`. They do not bump A6 counters; `execute` does.

---

# Bottom line for implementers

Keep the charter’s **rulings** (extract-now, snapshot `WorkspaceRead` upward, observation interface ≠ `ResolverContext` subtrait, full NeedInputs coverage). Treat these as **stale or incomplete against HEAD**:

1. Almost every cited line number (lib.rs mods, VerterHost impl range, `execute` choke, SingleflightGroup, identity-test exception row, napi lines, compiler Cargo.toml:59, Host/Session method lines).
2. Carve-out list of three files — **add `request_store_view.rs`**.
3. “`component_meta*` is dependency-neutral” — false for session imports; true only for `&VerterHost` fields.
4. “Relocate the existing `ResolverContext` trait” — the current trait is not nameable in `verter_semantic`.
5. LSP `project_resolver` re-export and `WorkspaceSnapshot.resolver: ProjectResolver` are missing from the call-site/deletion set.
6. `NeedInputs` is still green-field in Rust; `verter_semantic::query::QueryResult` is a lookalike, not a substitute.

No fourth `ResolverContext` implementor. The single-engine choke point is still `execute_via_cold_build_helper_with_publication_capture` at `project_semantic_dispatch/mod.rs:2206-2214`. Protect that uniqueness harder than any file move.
