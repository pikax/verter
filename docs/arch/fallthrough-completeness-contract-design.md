# Fallthrough completeness / budget-uniformity / result-bounding — binding design

Status: BINDING DESIGN — CTO layer-2 APPROVED. This is the codex-architect-approved unified design for the contract carved out in `fallthrough-completeness-contract.md`, ratified at CTO layer-2 with decision (b) adopted (see §5). It supersedes the per-site patch directions in that debt row's "(y) plan" (those were non-exhaustive candidate starting points). One sole writer; FOREGROUND codex/cargo/gate; binding 3-leg review (1 adversarial claude + 1 claims-aware codex + 1 unprimed codex); land on `mom/stage9-impl` before the §5d ff-merge — the CTO PRE-LAND layer-2 gates the land (no land before it).

## Binding implementation constraints (CTO layer-2)

These four are contract-level and the 3-leg explicitly verifies them:

1. Completeness is execution METADATA ONLY. It gates ADMISSION, never IDENTITY. It must NEVER enter any cache key, any lane identity, or any wire DTO (completeness-in-key would fragment the cache), and must never enter the GENERIC query rendezvous value `V` — the defaulted hooks keep every non-fallthrough generic query byte-identical. It DOES ride on the component-meta resolved value (`ResolvedComponentMetaState`/`ResolutionTemplate`) as a value FIELD — value-side admission metadata only. `ResultCompleteness` derives `Hash`/`Eq`, but that derive is UNCONSUMED: no cache key, lane identity, or wire field uses it (the value carriers it rides on derive only `Debug`/`Clone`), and none ever must. Confine its admission role to the rendezvous carriers + the TLS scope.
2. The defaulted hooks (`capture_completeness` = `Complete`, `fold_follower_completeness` = no-op) MUST keep every non-fallthrough generic query BYTE-IDENTICAL — proven via the full surface (no other query's value or `RequestSource` attribution shifts).
3. The follower's fold MUST be sequenced BEFORE ANY warm-admission in its path (no admission site between join and fold); the leader's `(value, completeness)` publish stays ATOMIC under the `flights`→`inner` lock. The concurrency review leg verifies BOTH windows.
4. ONE `RequestBudget` for #2 (no second fuse); install-if-none everywhere for #1 (no double-install).

Provenance: design consult run unprimed/neutral, repo-aware, gpt-5.5/xhigh, foreground-blocking, against the frozen B2e CORE code state (`2838d7e`, == worktree HEAD code). `CODEX framing=neutral-verified,best-not-lowest-effort-explicit`. Prompt + verbatim output: `.feedback/completeness-block/codex-design-consult-PROMPT.md` / `-OUT.txt` (verdict + 6 findings, `__DONE__`). The load-bearing structural claims were manager-verified by direct code read (trait/carrier shape, budget helper, `test_flight_strong_count` seam, leader-publish + same-thread-reentry anchors).

## The unified contract (one sentence)

Carry COMPUTE COMPLETENESS as first-class execution metadata THROUGH the shared `run_stable_request` singleflight rendezvous so a coalescing follower folds the leader's partiality into its own thread-local suppress state BEFORE it can warm any cache; arm the projection budget UNIFORMLY across every public result surface (outer install-if-none helper) with the fallthrough choke as an internal backstop; and bound the dynamic-root walker's RESULT cardinality during the walk — all INTERNAL, no key/`V`/wire/sealed/cap change.

Three rejected placements (codex, explicit): completeness must NOT live in the GENERIC query rendezvous value `V`, NOT in any cache key, NOT in any public/wire DTO. (It DOES ride on the component-meta resolved value `ResolvedComponentMetaState`/`ResolutionTemplate` as a value FIELD — admission metadata, never identity.)

## 1. Singleflight completeness protocol (Finding #3 — centerpiece)

The defect: `ColdComputeCompletenessScope` is thread-local (`request_context.rs:75`); a `RequestSource::Flight` follower joins the leader's value gated only on `stable` (`mod.rs` ~952), receives `flight.value.value.clone()` (~973), and never folds the leader's partiality — so a follower warms a leader's budget-partial child surface as Complete into the shared node cache + its own owner/payload caches (concurrent `get_component_meta_batch`, shared `NoOverrides` child).

Design — completeness becomes execution metadata on the rendezvous carriers:

1. Add `completeness: ResultCompleteness` to `StableExecutionValue<V>` (`resolver_core/mod.rs:695`) and `RequestRunResult<V>` (`mod.rs:722`). `ResultCompleteness` (`semantic_query.rs:1839`) is `Copy` + already the project completeness lattice — reuse it; do NOT invent a parallel type.
2. Add TWO default hooks to the `StableRequestExecutor<K,V>` trait (`mod.rs:728`), both with bodies so EVERY existing executor is unaffected (the generic-query zero-cost guarantee):
   - `fn capture_completeness(&self) -> ResultCompleteness { ResultCompleteness::Complete }` — the leader's post-compute completeness snapshot. Default = `Complete` (a generic query has no partial notion). The fallthrough executor overrides it to read `current_cold_compute_completeness()`.
   - `fn fold_follower_completeness(&self, _joined: ResultCompleteness) {}` — invoked on a follower join. Default = no-op. The fallthrough executor overrides it to fold the joined partiality into the follower's thread-local scope (see step 4).
3. LEADER records its fold: in `run_stable_request` (`mod.rs:803`), the winner snapshots `executor.capture_completeness()` AFTER `compute`/`store_stable` and BEFORE the retained `StableExecutionValue` is published as `FlightInner::Done` under the `flights`→`inner` lock (`mod.rs` ~2170). Because the completeness is stored INTO the `StableExecutionValue` before the closure returns and the publish is the same critical section that makes the lane `Done`, there is no torn-metadata window: any follower that observes `Done` observes the completeness with it.
4. FOLLOWER folds on join: the `RequestSource::Flight` joiner, after cloning `flight.value.value` (~973), calls `executor.fold_follower_completeness(flight.value.completeness)` BEFORE returning. The fallthrough override calls the new `request_context::fold_result_completeness(joined)` (added next to `mark_request_materialization_cache_suppress`, `request_context.rs:143`) which folds the EXACT partial reason into the follower's TLS scope (rather than blanket-re-marking a generic `PROPAGATED`), so the follower returns with its suppress state already partial before it reaches `store_node` (`host_manage.rs:1089`) or any owner/payload promotion.
5. `RequestRunResult.completeness` carries the same value out to the top-level caller so the surface-level promotion gates can read it without re-deriving.

Race / interleaving closure (all four must hold; codex-confirmed against the code):
- Leader-still-computing vs leader-done: a follower only ever reads `FlightInner::Done`, which is published with completeness already set under the lock — no read of in-flight partial state.
- Same-thread re-entry (`mod.rs` ~2245 recursion sentinel, `Running { owner } if owner == current_thread`): this is a NESTED LEADER compute, not a follower join — it already runs inside the same TLS scope, so NO cross-thread fold is applied (applying one would be a double-count). The fold is follower-join-only.
- Cross-thread follower: folds via step 4 before any warm-admission site.
- Follower that itself becomes a leader for a downstream child: it has already folded the upstream partiality into its TLS before it begins the child compute, so its own `capture_completeness()` for the child correctly reflects the inherited partiality.

## 2. Budget uniformity (Finding #1 — corrected by codex)

The defect: the projection-budget `RequestContext` spans resolve+extract on the direct analysis entry / audited with-resolution entry / payload surface, but `get_component_meta` (view-aware) and `get_component_meta_with_resolution` (session) run the fallthrough extract context-free — the only install-if-none guard (`component_meta_methods.rs:156`) drops before the extract, so the `[P0]` op-budget fuse is inert there.

Codex CORRECTION to the debt-row candidate: `compute_fallthrough_surface_from_resolved_state` (`host_manage/fallthrough.rs:253`) is the right FALLTHROUGH choke and the right place for an internal install-if-none BACKSTOP, but it is NOT sufficient alone — it only scopes the fallthrough sub-compute, not the whole public resolve+extract body. The budget must span the FULL request.

Design:
- Add ONE shared helper that installs `RequestContext::with_kind_timing_and_projection_budget` (`request_context.rs:1029`) install-if-none around the FULL public resolve+extract body, and apply it to:
  - the view-aware cold path around `component_meta_entry.rs:430`,
  - the session with-resolution path around `component_meta_entry_resolution.rs:276`,
  - reuse for the payload path `meta.rs:826` (already correct — keep).
- ALSO install-if-none at the fallthrough choke `compute_fallthrough_surface_from_resolved_state` (`fallthrough.rs:253`) as the INTERNAL backstop. It must NOT replace an outer full-request scope (install-if-none, never install-always) — so a public surface that already armed an outer context keeps that one and the choke is a no-op; a direct internal caller with no outer context still gets the fuse.
- The budget context and the #3 completeness scope co-scope at the same boundaries; both are install-if-none so neither double-installs when an outer surface already armed one.

## 3. Dynamic-root result bound (Finding #2)

The defect: `collect_dynamic_root_candidates_from_node_inner` (`fallthrough_value_eval.rs:394`) memoizes a CONCATENATED `Vec` and the `Union` arm `flat_map`s it; on a content-interned-DAG diamond the cached vector concatenates into both parents → O(2^depth) ALLOCATION even though node COMPUTATION is memo-bounded; the op-budget charges per node VISIT, never per result element, so it never trips.

Design:
- Memoize a DEDUPLICATED candidate SET, not a concatenated `Vec`. Derive/use `Hash` on the dynamic-root candidate type (codex cites `DynamicRootCandidate`, `resolver_core/fallthrough.rs:95` — implementer confirms exact type/site), store with `IndexSet` / Fx hashing, and emit a sorted `Vec` using the EXISTING ordering at `host_manage/fallthrough.rs:798` (so observable output order is unchanged).
- Charge the SAME `RequestBudget` per newly-inserted UNIQUE candidate (cardinality charge). Do NOT create a second fuse — unify with the Finding-#1 budget.
- Correctness invariant: for non-diamond inputs the dedup is a no-op on the result set and the sorted emit reproduces the prior order, so output is unchanged; only the exponential duplication is removed.

## 4. Auxiliary (folded into the contract)

- `FallthroughComputeOutcome { resolution, completeness }` — centralize fallthrough compute scoping into one internal outcome carrier; replace the ad-hoc caller scopes at `component_meta_extract.rs:1059`, `component_meta_extract.rs:1144`, and `host_manage/fallthrough.rs:151`. Rationale (codex): prevents a stale partial from a DISCARDED retry (completion-fence revalidate-and-retry) tainting a later complete attempt — the outcome carrier is the PRIMARY/intended travel path for the completeness it describes. The landed `compute_fallthrough_outcome_from_resolved_state` still bubbles its cold-compute scope implicitly and the callers fold it explicitly per attempt (idempotent — callers fold per-attempt and discard prior attempts, so a discarded retry's partial does not taint a later complete attempt). A capture-and-discard tightening so completeness travels SOLELY via the carrier is a tracked residual (not yet landed).
- `mirror_cached_fallthrough_arc` (`host_manage/fallthrough.rs:1127`) — add a self-gate (refuse partial) so the mirror does not rely on caller discipline. Currently caller-safe ([P3]); belt-and-suspenders, but the mirror is a promotion site and should self-gate.
- Keep the existing fallthrough node gate (`fallthrough_resolver.rs:193`) unchanged.

## 5. Surface / blast radius — INTERNAL-ONLY

Codex verdict: NO proto / FFI / wire-DTO / `HOT_TERMINAL_SINK` / architecture-guard cap / sealed-trait change. The materialize fence (`hot_path_never_calls_materialize_type_expr`, enabled and green at zero offenders) stays untouched — a separate workstream. This is an internal `resolver_core` change.

Visibility (`StableExecutionValue` / `RequestRunResult` / the rendezvous are currently `pub` in `resolver_core`, itself `pub`; adding a field is technically a Rust-API addition, NOT a user-facing wire/API break).

DECISION (CTO layer-2): **(b)** — add the `completeness` field AND narrow the stable-request carriers/rendezvous to `pub(crate)`, GATED on a CONSUMER-GREP at implementation start. The narrow is the clean end-state (removes the external-API surface); the grep makes it safe. Fall back to **(a)** (add the field, leave visibility `pub`) ONLY if the grep finds an external crate that consumes them. The implementer runs the grep across the workspace (every crate outside `verter_session`) FIRST, records the (a)/(b) outcome in the implement report, and proceeds accordingly. No guard contract shifts under either.

## 6. Discriminating test plan (red→green; no stubs)

1. CONCURRENT-FOLLOWER POISON (centerpiece). Batch B/C share a budget-tripping `NoOverrides` child A. Park A's leader with the established `test_flight_strong_count` seam (the existing concurrency parking probe — deterministic, not timing-based), wait until the follower has coalesced onto the in-flight lane, release the leader, then assert: the follower's observed `RequestRunResult.completeness.is_partial()` is TRUE, AND nothing partial warmed `child_surface_key(A, NoOverrides)` in the node cache, NOR the owner result, NOR the payload cache. MUST FAIL with the fold reverted (today the follower warms it Complete).
2. RESULT-SIZE DIAMOND. Extend `fallthrough_value_eval_recursion_tests.rs` (~:90) with a dynamic-root shared-subtree union diamond; assert produced `candidates.len()` is bounded by unique leaves (e.g. `== 1` for a single shared leaf), NOT `2^depth`. MUST FAIL against the current `flat_map` (which yields 2^depth).
3. PER-ANALYSIS-SURFACE BUDGET PARITY. Low-budget fallthrough-only fixture through `ComponentMetaSession::get_component_meta` (view-aware) AND `get_component_meta_with_resolution` (session); set `resolved.synthesis_should_suppress == false` so the partiality is fallthrough-only (isolates the budget fuse from the synthesis gate); assert the result is budget-partial and is NOT admitted to any cache. MUST FAIL while those surfaces extract context-free.

Keep the CORE's existing positive controls GREEN (a `LowerBound` accepted-surface shape with a COMPLETE compute STILL warms — gates key on COMPUTE completeness, never `accepted_surface_completeness`), and the over-gating negative controls.

## 7. Implementation ordering (single sole-writer)

1. Carriers + trait hooks + `fold_result_completeness` (the rendezvous plumbing; default hooks keep all generic queries byte-identical) → add the `run_stable_request` generic-attribution regression tests FIRST and prove them green (no generic regression) before wiring fallthrough.
2. Fallthrough executor overrides (`capture_completeness` / `fold_follower_completeness`) + the follower-poison test (red→green).
3. `FallthroughComputeOutcome` centralization + mirror self-gate.
4. Budget outer-helper + choke backstop + the budget-parity test (red→green).
5. Dynamic-root dedup-set + cardinality charge + the diamond test (red→green).

Each step is TDD red→green; the whole lands as ONE clean conventional commit (no phase/stage vocab) after the 3-leg review.

## 8. Strongest residual risk

A generic-query regression from adding metadata to the shared carriers. Mitigation (codex): default `Complete` capture + no-op follower-fold hooks, no key change, no `V` change, and dedicated `run_stable_request` tests proving cache/leader/follower/fallback SOURCE ATTRIBUTION (`RequestSource`) and value are unchanged for a generic executor. The reviewer must read those attribution tests and confirm they discriminate (a regression in attribution would fail them).

## 9. Component-meta admission gates on one merged completeness signal

Component-meta admission keys on ONE merged completeness signal, not a source-enumerated list. This is the settled state of the admission boundary.

### The single signal

`extract_component_meta_from_resolved` (and its with-facts sibling) enters ONE `ColdComputeCompletenessScope` spanning the WHOLE extract body — from before the pre-choke macro-DTO read (`resolver_component_meta_resolved_macros` → `vue_macro_dtos_with_ctx`) through the fallthrough cold compute and the publication policy. Both functions return the internal `ComponentMetaExtractOutcome { analysis, fallthrough_fact_versions, completeness }`; `completeness` is the scope's observed partiality — the union of EVERY extract-phase compute partial, no longer only the fallthrough's. The carrier is internal to `verter_session`: completeness is admission metadata — it never enters a cache key, lane identity, the GENERIC query rendezvous value `V`, or any wire DTO, but it DOES ride on the component-meta resolved value (`ResolvedComponentMetaState`/`ResolutionTemplate`) as a value FIELD. `ResultCompleteness` derives `Hash`/`Eq`, but that derive is UNCONSUMED — no cache key, lane identity, or wire field uses it (the value carriers it rides on derive only `Debug`/`Clone`), and none ever must.

Each publishing caller computes the one admission signal:

```
final_completeness = resolved.completeness.merge(extract_scope_completeness)
```

`resolved.completeness` is the resolve-phase compute completeness (dispatch / materializer / projector / slot-binding / resolve-macro partials); `extract_scope_completeness` is the full-extract scope. The merge is a lattice join in which `Partial` dominates, and it is load-bearing: neither operand alone suffices — the resolve phase and the extract phase each observe partials the other does not (a fallthrough-only extract-phase partial is invisible to the resolve-phase completeness on a path where the resolve completed and only the fallthrough tripped; a resolve-phase slot-binding partial is invisible to the extract scope). The pre-choke macro-DTO path is COUPLING-PREVENTED in production — the resolve-phase props projector also reads the DTO, so a budget-tripped DTO folds into `resolved.completeness` too — and the extract-scope operand is observable for that source in isolation only via the `#4` white-box decoupling test (defense-by-construction of the convergent gate).

Every owner-result and payload promotion gates on the single `final_completeness.is_partial()` check. The three former source-enumerated gates — the two owner-result sites in `component_meta_entry.rs` and the payload site in `meta.rs` — no longer enumerate `synthesis_should_suppress || fallthrough_completeness`; each reads only the merged signal. Because the scope boundary equals the result boundary, no extract-phase partiality source can escape by construction: a partiality source added anywhere inside the extract is captured without touching the gate, so the contract does not regress as the extract grows.

### Scope of the no-poison guarantee

This block delivers the no-poison guarantee on the FINAL `ComponentMetaResultDb` + payload admission via the merged-signal gate. The intermediate resolved-meta scalar-lane cache (the `None`-fixed-view lane in `resolver_core/component_meta_request.rs`) carries a SEPARATE pre-existing latent poison bug, tracked as the `RESOLVED_META_SCALAR_NO_POISON` follow-up in `semantic-db-overhaul-unified-remaining-plan.md`.

Residual: `materialization_cache_suppress` is sticky + monotonic across a discarded→retried-complete attempt, but its readers run in the resolve/materialize phase (before the extract fallthrough), so the blast radius is a possible MISSED-WARM (perf), never poison.

### `synthesis_should_suppress` is subsumed

`synthesis_should_suppress` is the boolean projection of `resolved.completeness.is_partial()` — its sole producer sets `synthesis_should_suppress` to exactly that predicate. Every admission case it suppressed is therefore already carried by the `resolved.completeness` operand of the merge. It is demoted to a compatibility/audit projection: it remains on the resolved state for output and span diagnostics, but it is not an admission-gate term. The merged signal covers it, and a synthesis-suppressed (resolve-partial) result with a complete extract scope is still refused warm admission through the `resolved.completeness` operand.

### Surface shape never suppresses admission

The gate keys on COMPUTE completeness (`final_completeness`), never on accepted-surface shape. `AcceptedSurfaceCompleteness::LowerBound` describes a surface shape, not a partial compute; a `LowerBound` accepted surface produced by a COMPLETE compute still warms. Surface shape is never an admission-suppression term.

### Dynamic-root result bound: halt on trip

The dynamic-root candidate walker (`collect_dynamic_root_candidates_from_node_inner` / `insert_dynamic_root_candidate_charged`) charges the shared projection budget per newly-inserted UNIQUE candidate BEFORE the insert, and HALTS the merge on a trip: a tripping candidate is not inserted, the walk stops unioning and recursing, and a partial is folded. Deduplication keeps a content-interned diamond's duplicate re-insertions free (the result-size charge stays per-unique), so a shared subtree does not re-expand; the halt additionally bounds the wide-UNIQUE cardinality (a union of N distinct roots under a budget below N yields a bounded partial set, not an N-element set). Non-trip output order is unchanged — the sorted emit is preserved.
