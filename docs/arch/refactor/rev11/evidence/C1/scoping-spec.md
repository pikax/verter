# C1 scoping spec — ModuleResolverCore and non-flow TypeInfoCore convergence

Authority: `docs/arch/refactor/rev11/charters/C1.md`, digest-bound in
`docs/arch/architecture-lock/ledger/authority-registry.toml` (`block = "C1"`, commit
`3a5a49fb6`). Binding ruling: `ARCH-RULING-C1-FOUR-FORKS.md`. This spec does not
relitigate either — it turns their already-decided positions into a concrete,
file-by-file execution plan, and records five ADOPT-NOW corrections a fresh
recon+disposition pass found the charter's own research got wrong against the current
tree (tip `3a5a49fb6`, branch `block/module-resolver-core`).

Inputs this spec is built from (read them before touching code, in this order):
1. `docs/arch/refactor/rev11/charters/C1.md` — the charter (ground truth for scope,
   acceptance IDs, forbidden outcomes).
2. `docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-FOUR-FORKS.md` — the four forks.
3. `/tmp/recon-c1-output.md` (grok recon, read-only pass over current tree) — every
   line number, file count, and coupling claim below that differs from the charter
   text comes from this pass. Re-verify anything you rely on; the tree may have moved
   again since.
4. `/tmp/c1-disposition-output.md` (codex xhigh disposition consult) — dispositions
   F1-F6 below are its verdicts, not mine. All five deviations were **ADOPT-NOW**;
   none reached the charter's Abort/rescope bar.

**If you discover a SIXTH deviation from the charter during implementation** (a new
factual error, a new coupling, a genuinely new fork), do not silently improvise a fix.
Stop, disposition it the same way (ADOPT-NOW / DEFER / REJECT per CLAUDE.md → Fix
Quality), and only continue once dispositioned. Default to another Codex xhigh
consult rather than guessing.

## 0. Five corrections to the charter, ADOPTED NOW

These change the literal Legacy Deletions / convergence-map text. Treat this section
as amending that text, not as optional commentary.

**F1 — Fourth carve-out file.** `resolver_core/request_store_view.rs` stays in
`verter_session`, alongside the three the charter already names
(`host_resolver_context.rs`, `session_resolver_context.rs`, the `impl ResolverContext
for VerterHost` block). It holds `&VerterHost` in production
(`CanonicalCompletionOverlay::complete_canonical`/`complete_canonical_with_session_view`/
`complete_canonical_inner`) and the existing seal-bridge guard in
`architecture_guards.rs` already exempts it as a fourth file — update that guard's
target-path list, do not fight it. Relocate only the dependency-neutral `StoreView`
value types it wraps; the completion machinery itself does not move.

**F2 — `component_meta/` + `component_meta_query_engine/` split by authority, not
wholesale.** The charter's claim that both subdirectories are dependency-neutral is
false — recon found production (non-test) imports of `crate::semantic_query`,
`crate::meta_resolve`, `crate::typeinfo::{surface,raise,framework_surface}`,
`crate::component_meta_materialize`, `crate::component_meta_caches`, `crate::
fact_signature_helpers`, `crate::cache_runtime`, `crate::request_context`, `crate::
structural_carrier_producer`, `crate::types`, `crate::host_executor`. None of those
are in the charter's move set. Moving the two subdirectories as-is creates a fresh
`verter_semantic -> verter_session` edge, which fails C1-AC-2 outright.

Corrected rule (apply Fork 1's ownership list, not a wildcard):
- Query-time type/name/projection ALGORITHMS inside these two subdirectories move to
  `verter_semantic` (they are what "TypeInfoCore" means).
- Session-only machinery they currently reach into for LIFECYCLE/PUBLICATION reasons
  — request TLS, host executor data, cache admission/singleflight bookkeeping,
  `ProjectTypeStore` publication, final component-meta result caching — stays in
  `verter_session`. Do not drag it down.
- Where a currently-imported session module (`semantic_query`, `meta_resolve`,
  `typeinfo::{surface,raise,framework_surface}`, `structural_carrier_producer`) is
  ITSELF query-time algorithm rather than lifecycle glue, its algorithmic core also
  relocates (do not leave TypeInfoCore split across two crates); the parts of those
  modules that are genuinely session lifecycle glue stay.
- A `ComponentMetaQueryEngine`-shaped facade may remain in `verter_session` ONLY as
  pure lifecycle/publication glue over the relocated kernel — it must own zero
  independent query semantics. If, mid-implementation, you find a piece of it that
  is NOT reducible to a thin call into `ProjectSemanticDispatch`/the relocated
  kernel, that is the sixth-deviation stop condition above — it may be evidence of a
  genuine second query-time resolver, which is the specific Abort trigger this
  disposition explicitly left open. Do not resolve that discovery unilaterally.
- Do the split file-by-file; do not attempt one mechanical `git mv` of the two
  directories. Expect this to be the single largest and highest-risk piece of work
  in the block.

**F3 — Do NOT relocate the current `ResolverContext` trait body as a unit.** The
charter's "trait + sealed module relocate with the kernel" sentence is wrong: the
production trait directly names `crate::VerterHost`
(`host_for_fact_tracer_install`), `verter_scheduler::cancellation::*` (default
`cancellation_token`), `crate::host_manage::prepared_decl::IndexedReadyServe`
(`ensure_indexed_ready_serve`), `Arc<ProjectTypeStore>` (`project_type_store`), and
`HostConfig` (`config`) — none of those are inside the three-carve-out region. This
is precisely the trait shape Fork 3 says the new observation interface must NOT
inherit by subtraiting.

Corrected mechanism:
- The EXISTING `ResolverContext` trait (blocking, host-capable, all the members
  above) STAYS DEFINED in `verter_session`. It is not split across crates as a
  single trait; it does not relocate.
- A NEW, separate sealed trait — the observation interface — is defined FROM SCRATCH
  in `verter_semantic`. Every method returns `AttemptOutcome<T>`. It does not extend
  `ResolverContext` and cannot name `VerterHost` or any scheduler type (this is
  enforced structurally: `verter_semantic`'s dependency closure cannot reach those
  types once F4/the Cargo edge deletion lands — see Authority/fallback order in the
  charter).
- `HostResolverContext`/`SessionResolverContext` keep implementing (session-side)
  `ResolverContext` for blocking callers. Whatever new I/O-free lifecycle C1 or C2
  needs implements the new semantic-side observation trait instead — it is a
  DIFFERENT type, not a marker layered on the old one.
- The kernel code that currently takes `&dyn ResolverContext` (now living in
  `verter_semantic` per F2) must be re-typed to take the NEW observation interface
  instead, since it can no longer name the old trait once the crate boundary closes.
  This is the actual mechanism by which C1-AC-5 (full `AttemptOutcome` coverage)
  gets satisfied: kernel functions stop taking `&dyn ResolverContext` and start
  taking `&dyn <NewObservationInterface>`, returning `AttemptOutcome<T>` throughout.
- `sealed::Sealed`/`RequestBoundSealed` as currently written are `ResolverContext`'s
  seal — they stay with that trait in `verter_session`. The new observation trait
  gets its OWN seal module in `verter_semantic`, sealed the same way (private marker
  trait, `pub(crate)` or narrower), not a shared seal.
- Delete the bare `impl ResolverContext for VerterHost` (see F6 / C1-AC-4) only after
  confirming (at implementation time, by grep + compile) that no production call
  site still needs it — recon found none, but re-verify, since
  `VerterHost::semantic_dispatch` (`host_construction.rs:~662`) and integration
  tests call it and must be repointed to construct via `HostResolverContext` first.
- Correct C1-AC-1/4/6's wording (they currently read as if the relocated interface is
  another `ResolverContext` implementor) when you write the landing record — the
  charter text is wrong on this point per Fork 3's own ruling; do not implement the
  wrong text just because it is written down.

**F4 — `ProjectResolver` is embedded, not a leaf; workspace gains a downward edge to
semantic (intended, not accidental).**
`WorkspaceSnapshot.resolver: ProjectResolver` (`workspace_snapshot.rs:46`),
constructed in `engine.rs`/`project_graph.rs`. `resolve_tracked` additionally takes
`&TrackedResolutionCapability` + `&TransactionReader` — workspace-engine-coupled
types that do NOT relocate.

Corrected scope:
- Reverse the Cargo edge: delete `verter_semantic -> verter_workspace`
  (`verter_semantic/Cargo.toml:28`), add `verter_workspace -> verter_semantic`. This
  direction is architecturally intended (`architecture.md:1121`: "managed engine
  depends on compiler/semantic, never the reverse") — it is not a workaround.
- `ModuleResolverCore` (the relocated `ProjectResolver`) lives in `verter_semantic`
  as pure computation over: (a) an owned immutable snapshot of whatever file-probe
  state it needs (mirroring the `RouteAnalysisInputs` pattern from Fork 2/F7 below),
  or (b) `AttemptOutcome::NeedInputs` when a probe result is not yet in that
  snapshot.
- `WorkspaceSnapshot.resolver` holds the relocated `ModuleResolverCore` value (or an
  `Arc` to it) directly now that the edge points the right way — no handle/id
  indirection is required by the evidence gathered so far; keep it as a value/Arc
  unless implementation surfaces a concrete reason otherwise.
- `TrackedResolutionCapability`, `TransactionReader`, transaction capture,
  publication, and resolution-currency enforcement all STAY in `verter_workspace`.
  `resolve_tracked` becomes a `verter_workspace`-owned adapter/wrapper that drives
  `ModuleResolverCore` through the `AttemptOutcome` boundary (workspace holds the
  live capability/reader; it feeds inputs to the kernel and interprets
  `NeedInputs`) — the algorithm itself does not stay behind in workspace merely to
  preserve the current method shape.
- Every direct I/O call inside `resolver.rs` today (`probe_path`,
  `probe_path_for_context`, `reader.realpath`, the package-manifest probe, anything
  taking `&dyn WorkspaceRead`) converts to the `AttemptOutcome`/`LoadSet` pattern —
  this is C1-AC-9, and per this disposition it is real, in-scope, non-optional work,
  not a "convert if trivial" carve-out.

**F5 — `verter_lsp::project_resolver` is a real consumer; migrate it explicitly.**
`crates/verter_lsp/src/project_resolver.rs:1` is `pub use verter_semantic::analysis::
project_resolver::*;`, re-exporting the FULL resolver surface
(`NativeProjectResolver`, `IdeProjectConfig`, etc.) via the same module whose
`:1-30` the charter says only loses its re-export half.

Corrected rule:
- Delete the `verter_semantic/src/analysis/project_resolver.rs:1-30` re-export
  SOURCE (`pub use verter_workspace::resolver::*` / `verter_workspace::types::*`) —
  the charter is right about that.
- Make the relocated `ModuleResolverCore` types themselves the canonical inhabitants
  of a semantic module path (this can be `analysis::project_resolver` itself, or a
  new path — this is the same "intra-crate layout" judgment call the charter already
  flags as non-forking; pick one and be consistent).
- Update `crates/verter_lsp/src/project_resolver.rs:1` to re-export from THAT real
  path so it keeps compiling with no behavior change.
- Grep for every other consumer of `verter_semantic::analysis::project_resolver::*`
  and every direct `verter_workspace::ProjectResolver`/`verter_workspace::resolver::*`
  import (recon's list in §3 of `/tmp/recon-c1-output.md` is a start, not
  necessarily complete — re-grep at implementation time) and repoint each in the
  same change. No compatibility re-export back to `verter_workspace` may survive.

**F6 — Confirmed, no action needed.** Exactly three production `sealed::Sealed`
implementors of `ResolverContext` exist (`VerterHost`, `HostResolverContext`,
`SessionResolverContext`). The charter's "fourth lifecycle" abort trigger does not
fire. `request_store_view.rs` (F1) is a fourth CARVE-OUT FILE, not a fourth
lifecycle implementor — do not confuse the two.

## 1. Final module layout

### `verter_semantic` gains (new, or moved-and-renamed)

- `resolver_core/` tree minus the four carve-out files (F1) — intra-crate submodule
  layout is your judgment call; keep the existing internal file boundaries unless
  there's a concrete reason to restructure, since this is a relocation, not a
  rewrite, for the code that isn't being algorithmically changed.
- `project_semantic_dispatch/` (the sole choke point:
  `execute_via_cold_build_helper_with_publication_capture`, currently
  `project_semantic_dispatch/mod.rs` around the `verter_audit::attribute_scope!
  (SemanticDispatch)` call — re-locate the file, verify the exact line at
  implementation time, and DO NOT let a second copy of this function or an
  equivalent choke point exist anywhere, in either crate, even transiently during
  the move).
- `resolver_store`'s immutable observation value types: `HostStoreView`,
  `StoreViewValidationToken`, and whatever they concretely depend on to compile as
  standalone values — recon found `HostStoreView` embeds
  `crate::resolver_core::StoreViewCompatToken` and `Arc<crate::store_view_roots::
  StoreViewMemo>`; `StoreViewValidationToken` embeds `crate::file_artifact_store::
  ProjectIdentity`. Trace and relocate (or replace with a dependency-neutral
  equivalent) everything these two types transitively need to be `Send + Sync`
  standalone values with no `&VerterHost`. `StoreViewManager` and all
  cache-retention policy stays in `verter_session`.
- `ModuleResolverCore` — the relocated `ProjectResolver` (2122 lines; verify current
  count at implementation time), per F4.
- The query-time algorithmic cores of `component_meta/` and
  `component_meta_query_engine/`, per F2.
- `facts/registry.rs` becomes the OWNER of `FactKey`/`FactDomain`/`Fact`/
  `FactRegistry` (moved down from `verter_workspace/src/fact_registry.rs`), not a
  re-export. The `From<crate::analysis::MacroKind>` impl currently at
  `facts/registry.rs:5-18` is real logic — keep it colocated with the types it
  converts, adjust only what the move requires.
- NEW: `AttemptOutcome<T>` / `LoadSet` per `contracts/input-loading.md` §2, §4 — does
  not exist anywhere in the tree today (recon confirmed zero `NeedInputs|LoadSet`
  hits under `crates/`). This is new code, not a refactor of an existing type —
  `verter_semantic::query::QueryResult`'s `Completeness`/`missing_inputs` shape is a
  DIFFERENT, pre-existing envelope for a different job (public query result
  reporting, not kernel retry signaling) and must not be repurposed or merged into
  `AttemptOutcome`.
- NEW: the capability-limited observation interface trait (F3) + its own seal
  module.

### `verter_session` keeps (or gains back)

- `resolver_core/host_resolver_context.rs`, `resolver_core/session_resolver_context.rs`,
  `resolver_core/request_store_view.rs` (F1), and the surviving parts of
  `resolver_core/resolver_context.rs`: the `ResolverContext` trait itself (F3), its
  existing `sealed` module, `RequestBoundResolverContext`, and — unless the
  implementation-time audit proves it dead — the bare `impl ResolverContext for
  VerterHost` block.
- `StoreViewManager` and cache-retention machinery from `resolver_store.rs`.
- `semantic_query`, `meta_resolve`, `component_meta_materialize`,
  `component_meta_caches`, `fact_signature_helpers`, `cache_runtime`,
  `request_context`, `structural_carrier_producer`, `types` — EXCEPT whatever
  algorithmic slices of these F2 determines must relocate with the query engine.
  Treat each as "stays unless proven to be query-time algorithm."
  `ensure_loaded`/`wait_or_drive` (`host_lifecycle.rs`), the blocking cross-file
  load-on-demand machinery, stays here unconditionally (charter's convergence map,
  unchanged).
- A thin `ComponentMetaQueryEngine`-shaped lifecycle/publication facade (F2), calling
  into the relocated kernel.
- The `verter_semantic -> verter_workspace` edge does NOT get replaced by a
  `verter_session -> verter_semantic` change of shape — session already depends on
  semantic; that edge is unaffected by C1.

### `verter_workspace` loses `resolver.rs`'s algorithm, keeps its transactional shell

- `pub mod resolver` / the `ProjectResolver`-family re-export at `lib.rs` deleted.
- `TrackedResolutionCapability`, `TransactionReader`, resolution-currency
  enforcement, and the new `resolve_tracked` adapter (F4) stay/land here.
- `WorkspaceSnapshot`, `engine.rs`, `project_graph.rs` update their `ProjectResolver`
  field/construction sites to the relocated `ModuleResolverCore` type, reached
  through the new `verter_workspace -> verter_semantic` edge.
- `module_resolution.rs`'s content-free vocabulary (`ModuleResolutionMode`,
  `SpecifierKind`) stays put; its doc comments currently point at
  `verter_session::resolver_core` and must be corrected to the new path in the same
  change (recon flagged this as stale-pointer risk, not a second resolver).
- Gains a new dependency edge on `verter_semantic` (Cargo.toml).

## 2. Full worked file/consumer inventory (starting point — re-grep, do not trust as final)

Do not treat this table as exhaustive; the codebase moves under you and the charter's
own line numbers were already stale by the time recon ran. Re-run the greps noted
below before each phase starts.

| Surface | Where to re-verify | Disposition |
|---|---|---|
| `resolver_core/**` (59 files) minus 4 carve-outs | `find crates/verter_session/src/resolver_core -type f` | relocate, split component_meta*/ per F2 |
| `project_semantic_dispatch/**` | `crates/verter_session/src/project_semantic_dispatch/` | relocate whole (Preserve semantics unchanged) |
| `resolver_store.rs` | same file | split: value types relocate, manager stays |
| `verter_workspace/src/resolver.rs` (2122 lines) + `resolver_tests.rs` | `wc -l`, sibling test file | relocate both, convert I/O per F4 |
| `verter_session/src/lib.rs:332,341-346` (charter coords, drifted — recon found `:336,345,346`) | `grep -n 'mod resolver_core\|mod resolver_store\|mod project_semantic_dispatch' crates/verter_session/src/lib.rs` | narrow declarations to the 4 staying carve-out files |
| `verter_workspace/src/lib.rs:103,183-188` (charter coords) | `grep -n 'pub mod resolver\|pub use resolver' crates/verter_workspace/src/lib.rs` | delete |
| `verter_semantic/src/lib.rs:23-33` | same file | add new module declarations per §1 |
| `crates/verter_identity/tests/cases/workspace_dependency_layers.rs` `ratified_upward_exceptions()` (charter `:118-127`, recon found `:139-148`, row at `:145`) + `RATIFIED_ROOT_CRATES` (`:335`, recon-found — charter doesn't mention this list) | grep both symbol names | delete the `"verter_semantic"` map row AND its `RATIFIED_ROOT_CRATES` entry (C1-AC-2 requires the exception genuinely gone, not partially) |
| `crates/verter_session/tests/cases/architecture_guards.rs` `mod resolver_context_seal` (charter `:3384` — recon confirms current) + seal-bridge exemption list (recon found `:3652-3657`, 4 filenames) | grep `resolver_context_seal`, grep the exemption list | retarget scan roots to new paths; update exemption list if filenames change |
| `crates/verter_session/tests/cases/g_misc1/no_bare_host_resolver_shims.rs` | file exists per recon | rewrite or delete in the SAME change the bare-host impl is deleted in — never leave it failing |
| `crates/verter_session/tests/cases/compile-fail/{raw_resolver_entry_points_are_private,output_projector_not_impl_outside_crate,locator_reducing_lowerer_not_nameable}.rs` | grep for `verter_workspace::resolver::ProjectResolver` / `verter_session::project_semantic_dispatch::` | repoint paths, keep same negative-compile assertions |
| `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs` | hard-codes `ProjectSemanticDispatch` paths per recon | repoint |
| `crates/verter_audit/tests/cases/member_edge_provenance_arch_guard.rs:116-117` | cites `verter_session/src/project_semantic_dispatch/{mod.rs,raise.rs}` | repoint |
| `crates/verter_lsp/src/project_resolver.rs:1` | full re-export | repoint per F5 |
| `crates/verter_napi/src/lib.rs` (charter `:2095,2117`, recon found fn/call at `:2093/2102`, `:2111/2124`) | grep `project_resolver::` | no repoint needed — module path unchanged, only its re-export half dies |
| `crates/verter_wasm/src/lib.rs` (charter `:640,667` — recon confirms current) | same | same, no repoint needed |
| `crates/verter_tsc/src/checker.rs:1861,1930` (`is_relative_specifier`) | grep | repoint to new `verter_semantic` path |
| `verter_session` path-helper consumers of `verter_workspace::resolver::*` (recon's table: `file_artifact_store.rs`, `host_cache_runtime.rs`, `host_manage/fallthrough.rs`, `external_ts/{identity_resolver,resolver}.rs`, `typeinfo/vue_macro_codegen/tsc_projection.rs`) | grep `verter_workspace::resolver` across `crates/verter_session` | repoint (these want `path_is_carrier`/`collapse_path`-style helpers — confirm which of these are dependency-neutral enough to also relocate vs. which stay workspace-side utilities `verter_semantic` re-exports) |
| `verter_lsp` path-helper consumers (`server/provider_state.rs`, `server_utils.rs`, `config.rs`, `tsgo/{project_binding,composite}.rs`, `external_ts/carrier_sync.rs`) | grep `verter_workspace::resolver` across `crates/verter_lsp` | repoint |
| `WorkspaceSnapshot.resolver` field + `engine.rs`/`project_graph.rs`/`snapshot_builder.rs` constructors | grep `ProjectResolver` in `verter_workspace` | repoint to relocated type, per F4 |
| `verter_bench` benches referencing `verter_session::resolver_core` | `fact_validation_hot_path.rs:29`, `fact_tracer_warm_hit_zero_alloc.rs:79` | repoint |
| Comment-only references (`verter_workspace`, `verter_type_runtime`, `verter_audit`, `packages/types/audit.generated.ts`) | recon §3 list | fix comments in the same change, not a follow-up — stale doc comments pointing at the old crate are exactly the kind of drift that produced this spec's corrections |
| `SingleflightGroup::run`/`run_retaining` Condvar wait (`resolver_core/mod.rs`, recon found `struct` at `:2117`, `run` at `:2377`, `run_retaining` at `:2404`, `Condvar::wait` at `:2617-2618`) | grep | relocates with resolver_core; audit whether `route_db_singleflight.rs` / kernel callers of it need a peek-and-decline `NeedInputs` arm (see §3) |
| `route_db_singleflight.rs:70-146` | grep | RouteDb is kernel-reachable — needs non-blocking peek path per C1-AC-5, not just "lives in session" |
| `prepared_decl.rs` `build_gate: parking_lot::Mutex<()>` (recon found field `:37`, lock sites `:962,1177`) | grep | same — kernel-reachable via `ResolverContext::prepared_*`, needs peek-or-`NeedInputs` |

## 3. The `AttemptOutcome` full-coverage mechanism, concretely

Per C1-AC-5's own text, coverage is discharged STRUCTURALLY: every method on the new
observation-interface trait (F3) is typed to return `AttemptOutcome<T>` — never a
bare `T`, `Result<T, _>`, or a call that can block. This is the actual mechanism, not
a design choice you have room to skip:

1. Define the trait in `verter_semantic` with every resolver-tier operation the
   relocated kernel needs, each returning `AttemptOutcome<T>`.
2. Every relocated kernel function that currently takes `&dyn ResolverContext`
   (blocking) is re-typed to take the new trait instead, and its body is restructured
   so that wherever it currently calls something that can block —
   `ensure_loaded`/`wait_or_drive`, `SingleflightGroup::run`/`run_retaining`'s
   `Condvar::wait`, `route_db_singleflight`, `prepared_decl`'s `build_gate` — it
   instead: (a) attempts a non-blocking peek at already-available state, and (b) on a
   miss, returns/propagates `AttemptOutcome::NeedInputs(LoadSet)` describing what's
   missing, rather than parking the thread.
3. `HostResolverContext`/`SessionResolverContext` do NOT implement the new trait —
   they keep implementing the old blocking `ResolverContext`. Internally they may
   still call the relocated kernel functions and, on `NeedInputs`, immediately drive
   the blocking load loop (`ensure_loaded`/`wait_or_drive`) and retry — this is how
   the charter's "may become internal choke points" language cashes out. The
   BLOCKING lifecycles keep their existing single-pass, no-extra-allocation hot path
   (performance bound in the charter) — do not make every blocking call now
   materialize a `LoadSet` when nothing is missing; the `Complete(T)` arm must be the
   same-cost path as today's direct return.
4. The exhaustive-test-double gate from C1-AC-5: write one `impl <ObservationInterface>
   for TestDouble` in a test module — it must implement every trait method to
   compile. This is the actual coverage proof; do not additionally try to enumerate
   "every reachable operation" by hand or via a name-keyed scanner (forbidden by
   CLAUDE.md's landed-scanner bar).
5. `LoadSet` construction: normalized/sorted/deduplicated per `contracts/
   input-loading.md` §4. `NeedInputs` on an empty delta with no basis change is the
   typed `InputResolutionNoProgress` failure per §4.3-4.5 — never a silent retry
   loop. Write this as its own small, directly-tested unit before wiring it into the
   trait's return path everywhere, since every other conversion depends on it being
   correct.

## 4. Edge cases and ordering (explicit, since ambiguity here is where gaps happen)

- **Order of operations matters for staying buildable at each commit.** Recommended
  phase order (WIP commits within this order are fine per policy; squash happens at
  landing):
  1. Land `AttemptOutcome`/`LoadSet` as new, inert types in `verter_semantic`
     (§3 step 5) with their own unit tests — zero behavior change to anything else.
  2. Land the new observation-interface trait shape (empty-ish, just the seal +
     trait definition) — still zero behavior change.
  3. Relocate `resolver_store`'s value types + whatever they need (transitive
     closure) into `verter_semantic`, leaving `verter_session` re-exporting them
     from the new location until step 6 removes the re-export. This keeps
     `verter_session` compiling throughout steps 3-5.
  4. Relocate `project_semantic_dispatch` — the single choke point — verifying by
     grep immediately after that `execute_via_cold_build_helper_with_publication_
     capture` (or whatever it's named by then) exists in exactly one place in the
     whole workspace.
  5. Relocate `resolver_core/**` minus the four carve-outs, doing the F2 split
     file-by-file rather than as one wildcard move.
  6. Wire the relocated kernel functions onto the new observation-interface trait
     (§3 steps 1-3), removing the temporary re-exports from step 3.
  7. Relocate `ProjectResolver` -> `ModuleResolverCore` (F4), reverse the Cargo edge,
     update `WorkspaceSnapshot`/`engine`/`project_graph`.
  8. Delete the `verter_semantic -> verter_workspace` edge and the A5-DD1 exception
     rows (only possible once 3-7 are done and nothing in `verter_semantic` still
     names a workspace type).
  9. Delete the bare-host `ResolverContext` impl (F3/F6) if the audit confirms it's
     dead, and its now-obsolete guard test.
  10. Collapse the Host/Session delegation duplication (charter's "Duplicated
      lifecycle-adapter boilerplate" list) — do this LAST, after the trait shape has
      settled, so you are not repeatedly re-deriving the shared helper against a
      moving target.
  11. Fix every stale doc-comment / test path from the inventory table in §2.
  12. Write the C1-AC-1 characterization suite (bit-identical answers across
      lifecycles) — this can and should be written EARLY as a regression harness you
      run after every phase above, not only at the end; it is your primary defense
      against accidentally changing resolution semantics mid-move.
- **Never let step 4-5 produce a moment where two things both claim to be "the"
  query-time resolver.** If a phase boundary would leave both an old
  `verter_session::project_semantic_dispatch` AND a new
  `verter_semantic::project_semantic_dispatch` reachable from production code at
  once (even briefly, even behind a feature flag), that is the single-engine
  violation CLAUDE.md is most explicit about. Prefer relocating a module and fixing
  its call sites in the SAME commit over a multi-commit "add new, migrate callers,
  delete old" sequence for this specific piece.
- **`component_meta_query_engine`'s F2 split is where a second engine is most likely
  to appear by accident** (per the disposition consult's own warning). After
  splitting, grep for every remaining production call in `verter_session` into the
  relocated kernel and confirm each one is a thin pass-through with no independent
  branching/caching/dispatch logic of its own. If you find one that IS independent
  logic, stop and disposition it (§0's "sixth deviation" instruction) rather than
  leaving it as a shadow resolver.
- **Test files sibling-relocate with their production code** (charter's own rule,
  C1-AC-3) — `resolver_core/*_tests.rs`, `component_meta/tests.rs`,
  `component_meta_query_engine/*_tests.rs`, `project_semantic_dispatch/*_tests.rs`,
  `project_semantic_dispatch_invariants_tests.rs` all move with their modules.
  Assertions do not change in substance; only their module path and any
  `crate::`-relative imports that now cross the crate boundary.
- **Doc comments inside the 59 relocating files** that say `crate::resolver_core`,
  `crate::semantic_query`, etc. must be corrected to their new absolute paths in the
  same commit as the move — do not leave a doc comment describing where code used to
  live (CLAUDE.md's "No phase archaeology" rule extends naturally to "no crate
  archaeology" here: a moved file's comments describe where it lives now).
- **`VerterHost::semantic_dispatch`** (`host_construction.rs`, recon found
  `:662-665`) constructs `ProjectSemanticDispatch::new(self)` directly on a bare host
  and is NOT `#[cfg(test)]` — if the bare-host impl is deleted (F3/F6, step 9), this
  accessor must wrap `HostResolverContext::from_current`/`from_cold_seed` internally
  first. Find every test that calls it (recon flagged dozens, e.g.
  `tests/cases/g_misc0/cross_owner_materialise_reuse_production.rs:255`) and confirm
  they still compile against the wrapped form.
- **Route extraction snapshot (Fork 2 / charter's C1-AC-8)** — this is a SEPARATE,
  smaller, already-well-specified piece: `routes.rs`'s six `&dyn WorkspaceRead`
  call sites (charter's line numbers `196,251,661,672,869,1120` — recon confirms
  these are current) take an owned `RouteAnalysisInputs` snapshot instead; the
  workspace/session side builds the snapshot upward. Do this as its own bounded
  unit — it's a template for the SAME pattern F4 needs at larger scale for
  `ProjectResolver`, so doing it first can inform the `AttemptOutcome`/snapshot shape
  used later, or doing it after `AttemptOutcome` exists lets it reuse the same
  `LoadSet` vocabulary if that turns out cleaner. Either ordering is fine; just
  don't invent a THIRD snapshot shape distinct from both.

## 5. Acceptance ID -> concrete proof mapping

| ID | What "done" looks like concretely |
|---|---|
| C1-AC-1 | New characterization suite driving one fixed query corpus through `HostResolverContext` and `SessionResolverContext`, asserting structural `SemanticNodeId`-surface equality; write early, run after every phase (§4) |
| C1-AC-2 | `cargo test -p verter_identity workspace_dependency_layers` green with the `"verter_semantic"` row AND its `RATIFIED_ROOT_CRATES` entry both deleted (F1-corrected: charter only names the map row) |
| C1-AC-3 | `project_semantic_dispatch_invariants_tests.rs` + the five-row Authority-uniqueness contract green, unmodified in substance, at their new path |
| C1-AC-4 | Bare-host rail deleted (or its retention justified with a named production call site) — `RequestBoundResolverContext` becomes the sole production-constructible rail; corrected per F3, this is about the SESSION-side `ResolverContext`, not the new observation interface |
| C1-AC-5 | The exhaustive-test-double compile gate (§3 step 4) plus a runtime I/O-free harness exercising it with zero scheduler touches |
| C1-AC-6 | Diff proof: the ~10 duplicated delegation methods collapse to one shared implementation; do this at phase 10, not earlier |
| C1-AC-7 | `trybuild`/compile-fail fixture: a `&VerterHost`-holding type does not satisfy the new observation interface's bound — write this test FIRST, before the trait has real methods, so it's a genuine tripwire during F2/F4's split work |
| C1-AC-8 | `routes.rs` extractors take `&RouteAnalysisInputs`; zero `WorkspaceRead`-typed params remain anywhere in `verter_semantic` — grep-verify the zero, don't just check the six named sites |
| C1-AC-9 | Every I/O call inside relocated `ProjectResolver`/`ModuleResolverCore` is either pure computation or converted to `AttemptOutcome`/`LoadSet` — audited as part of the same sweep that proves C1-AC-5, per F4's corrected scope |

## 6. Forbidden outcomes checklist (re-read before declaring implementation done)

- A second `SemanticQueryApi::execute`/`execute_relate`/`shallow_lower_type_expr`, or
  a second struct owning a `RelationMemo`/the semantic node map.
- A converged kernel reading live, un-snapshotted host state at validation time.
- Any lifecycle-specific answer divergence for identical inputs.
- A blocking wait anywhere on the path the new observation interface uses to reach
  `NeedInputs`.
- The new observation interface presented as a marker/subtrait of `ResolverContext`
  (F3 makes this doubly explicit — it is not just forbidden, it is now known to be
  IMPOSSIBLE as the charter literally described it).
- Any non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2
  projection attempt left blocking-only.
- A new name-keyed source-tree scanner for any invariant this block introduces — all
  new confinement is type-level/crate-boundary-level (per the charter's own
  Structural confinement section).
