# Fallthrough completeness / budget-uniformity / result-bounding contract

Status: TRACKED active block (not a deferral past the current overhaul stage). This block MUST close findings #1/#2/#3 below BEFORE the fallthrough/B2e work is fast-forward-merged to the integration branch. Never ff-merge a known cache-poison hole; on the block branch (`mom/stage9-impl`) the open state is an acceptable in-progress condition. This is design-first: the block opens with an architecture design consult, not implementation.

## Why this block exists

The Vue component-meta "fallthrough / attribute-inheritance" path was hardened across four commits (node-domain conversion + a recursion-compute bound + override-bearing-fallthrough made wholesale uncacheable + a sequential no-poison completeness rail + a payload-surface projection budget). The CORE is sound and committed. But an adversarial 3-leg review (1 claude + 2 codex, each leg found a DIFFERENT defect) proved the per-surface / per-site patch approach NON-CONVERGENT: it surfaced three distinct structural gaps, the deepest of which (the concurrent-follower hole, #3) is a foundational pre-existing concurrency gap in the completeness mechanism — a redesign, not a patch. The contract is therefore carved out as one coherent design-first unit.

## CORE state (committed, verified)

- B2e CORE = commit `2838d7e` on `mom/stage9-impl` (the code state is frozen there; this doc lands as a later docs-only commit, so the CORE is `HEAD~1` once this doc is committed).
- Verified clean (manager, full foreground): `cargo test -p verter_session --tests` → lib 4025/0, integration sole failure = the materialize fence (RED at exactly 10 at that verification — a separate cache-correctness tracker, NOT this block; the fence has since gone GREEN at zero offenders); `node scripts/gate.mjs` → both surfaces, sole failure = the fence, 0 tolerated, typeinfo byte-pin genuine+clean; `cargo fmt --all --check` clean; `cargo clippy -p verter_session --tests -- -D warnings` clean.
- What the CORE correctly delivers (all three review legs concur): the node-domain fallthrough value-eval + registry structural materialization; a per-call `SemanticNodeId` memo bounding walker COMPUTATION (no re-traversal / no hang); override-bearing fallthrough wholesale `Uncacheable` (poison-free by construction, off-lane — immune to #3); the SEQUENTIAL no-poison completeness rail (every fallthrough-bearing promotion gated on typed `ResultCompleteness` — `ComponentMetaResultDb` ×3, the fallthrough node cache, the legacy `cached_fallthrough` mirror, and the `cached_meta_payload` payload cache); the payload-surface (scalar + batch) projection budget. Over-gating is correctly avoided (gates key on COMPUTE completeness, never on `accepted_surface_completeness`; a `LowerBound` surface with a complete compute still warms — proven by positive-control tests).

## The three findings to close (file:line + verified analysis)

Evidence: `.feedback/b2e-3leg/codex-reattack3-focused-OUT.txt`, `codex-reattack3-blind-OUT.txt`, and the claude leg report (agentId `a13cab6f8c241552c`). All three independently manager-verified by direct code read.

### #1 — Budget-parity is missing on the view-aware / session ANALYSIS surfaces (both codex legs)

The projection-budget context is installed spanning resolve+extract on the direct host analysis entry (`crates/verter_session/src/host_manage/component_meta_entry.rs:159`), the audited with-resolution entry (`component_meta_entry_resolution.rs:83`), and (newly) the payload surface (`meta.rs` `resolve_one_payload_item`). But `ComponentMetaSession::get_component_meta` → `get_component_meta_via_view_inner` (resolve `component_meta_entry.rs:430`, extract `:440`, publish `:454`) and session `get_component_meta_with_resolution` → `get_component_meta_with_resolution_via_view` (extract `component_meta_entry_resolution.rs:297`) run the fallthrough extract CONTEXT-FREE: the only install-if-none guard lives inside `resolve_component_meta_with_view_and_fixed` (`component_meta_methods.rs:156`) and DROPS before the extract. The walker budget check is conditional on `current_request_budget()` (`fallthrough_value_eval.rs:71`), so the [P0] op-budget fuse is inert on these public analysis surfaces.

- NOT a poison hole (the promotion gates are present; context-free ⇒ no budget partial can occur ⇒ nothing partial to warm; semantic partials are still folded + caught). It is the budget/[P0]-fuse PARITY gap (Shared Optimized Codebase: a pathological component can over-compute on these surfaces where it would be budget-truncated elsewhere).
- The per-surface context-install is the non-convergent shape. CONVERGENT FIX: install ONE budget context (install-if-none-active) at the SHARED CHOKE POINT every surface funnels through — `compute_fallthrough_surface_from_resolved_state` (`host_manage/fallthrough.rs:218`, called from every `extract_component_meta_from_resolved*`) — so analysis / view-aware / session / payload / batch all inherit it uniformly and no future surface can miss it.

### #2 — Dynamic-root walker bounds VISITS but not RESULT SIZE → O(2^depth) allocation (blind codex)

`collect_dynamic_root_candidates_from_node` (`crates/verter_session/src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs:394`): `enter_node` returns `NodeWalkStep::Cached(<memoized Vec>)` on a re-visit (`:401-405`), and the `Union` arm `flat_map`s (concatenates) each arm's Vec (`:412-419`). On a shared-subtree diamond the cached vector is concatenated into BOTH parents, doubling output per level → O(2^depth) ALLOCATION even though node COMPUTATION is memo-bounded. The op-budget charges per node VISIT (memo-bounded), NOT per result element, so it does not trip; the caller dedup (`host_manage/fallthrough.rs:798`) runs AFTER the exponential vector is built → OOM risk on the hot path. Distinct from `known_spread_keys_from_node` whose Union arm INTERSECTS (`fallthrough_value_eval.rs:311-343`) and is therefore bounded.

- FIX: bound the RESULT during the walk — dedup-during-walk (memo stores a de-duplicated set / per-node dedup of the flat_map output) and/or charge the op-budget per produced result element (cardinality), so a diamond cannot allocate exponentially. The existing diamond test only asserts budget-delta + non-emptiness, so it would pass with 2^n duplicates — a new result-SIZE test is required.

### #3 — Concurrent singleflight FOLLOWER no-poison hole (claude leg) — the deepest / foundational

The `ColdComputeCompletenessScope` is THREAD-LOCAL (`request_context.rs`). The fallthrough child resolution coalesces through the singleflight rendezvous (`run_stable_request`, `resolver_core/mod.rs`). A FOLLOWER joins the leader's value WITHOUT folding the leader's compute-partiality:

- `StableExecutionValue { value, stable, computed }` (`resolver_core/mod.rs:695`) and `RequestRunResult { value, source, attempts }` (`mod.rs:722`) carry NO completeness field.
- `run_stable_request` retains/shares a result to followers gated on `|sev| sev.stable` (`mod.rs:952`) — stability only, completeness-BLIND. The follower receives `flight.value.value.clone()` (`mod.rs:974`).
- The follower arm (`host_manage/fallthrough.rs:173-222`) only bumps provenance counters; it never calls `mark_request_materialization_cache_suppress`, so the leader's partiality never folds into the follower's thread-local scope.

Concrete poison (concurrent batch path, owners B+C share a `NoOverrides` child A whose fallthrough trips the budget): leader B folds partial → correctly refuses A's surface + B's result; follower C joins B's truncated value under a Complete scope → `store_node(child_surface_key(A), <partial>)` (`host_manage.rs:1088`) admits the partial into the SHARED child-surface node cache, AND C's owner result publishes / payload-warms. Reachable via `get_component_meta_batch` (concurrent on the HostCpuPool, child lane coalesces).

- NOT a regression (pre-cluster there was no fallthrough completeness gate at all; this cluster strictly improves the sequential path and leaves only the concurrent-follower edge) — but it IS a known cache-poison hole, so it blocks the ff-merge.
- FIX (foundational): carry completeness as a first-class part of the singleflight rendezvous — add a completeness/partial signal to `StableExecutionValue` / `RequestRunResult`; the leader RECORDS its fold result into the shared rendezvous (not just TLS); the follower (`RequestSource::Flight`) FOLDS it on join (`mark_request_materialization_cache_suppress` when the joined value was partial). This is a core-machinery / concurrency change ⇒ design-first + CTO layer-2 before implementation.

Lower-severity (fold into the block): [P3] `mirror_cached_fallthrough_arc` (`host_manage/fallthrough.rs:1127`) has no self-gate (latent — currently safe because its only fresh-compute caller `cache_fallthrough_result` gates; a self-gate at the choke would be belt-and-suspenders); [P3] a minor over-gate of the reusable intrinsic-surface cache within a partial owner compute (negligible).

## (y) design-first plan (the block)

1. FIRST ACTION (fresh manager): an UNPRIMED codex architect DESIGN consult (demand-best, effort-neutral) for the contract AS A UNIT — the completeness-through-the-singleflight-rendezvous redesign (#3) as the centerpiece, plus the shared-choke-point budget context (#1) and the result-bounding walker (#2). Treat the three as one coherent no-poison/budget/result contract, not three patches.
2. Binding design written to `docs/arch/fallthrough-completeness-contract-design.md`.
3. SURFACE to the CTO for layer-2 BEFORE any implementation (foundational core-machinery / concurrency change — anti-rogue: no implementation ahead of the design sign-off).
4. Sole-writer implement (single writer; foreground codex/cargo/gate) → binding 3-leg review (1 claude + 2 codex) → land on `mom/stage9-impl` BEFORE the §5d ff-merge.

### Mandatory discriminating tests (red→green; not stubs)

- CONCURRENT-FOLLOWER POISON: two owners share a `NoOverrides` child whose fallthrough trips the budget, resolved concurrently (batch); assert the follower's result is observed Partial AND nothing partial warms the shared `child_surface_key` node cache / owner result / payload cache. Must FAIL if the rendezvous does not carry+fold completeness.
- RESULT-SIZE DIAMOND: a shared-subtree union diamond drives `collect_dynamic_root_candidates_from_node`; assert the produced candidate count is bounded (not 2^depth) — must FAIL against the current flat_map.
- PER-ANALYSIS-SURFACE BUDGET PARITY: a budget-tripping owner through `get_component_meta` (view-aware) and `get_component_meta_with_resolution` (session) yields a budget-partial that is NOT admitted — must FAIL while those surfaces extract context-free.

## Guard rails (do not regress)

The materialize fence (`hot_path_never_calls_materialize_type_expr`) is enabled and GREEN at zero offenders — it tracks reverse-materialization SITES (a separate workstream), not this cache-correctness contract; this block must keep it green (no new hot materialize-then-decide sites). The §7 HOT_TERMINAL_SINKS accounting is the audited-pure-one-shot-sinks rail and is unaffected (no new sinks). Keep the CORE's sequential no-poison gates, the override-Uncacheable path, and the over-gating positive-control tests green.
