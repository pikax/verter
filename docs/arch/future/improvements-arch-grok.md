# Architecture improvements — Grok review (`perf/consolidation` vs `main`)

**Status:** open backlog from an architect review of consolidation direction vs
`main` (Mar 2026 baseline). Not a plan block and not a release gate. Strategic
verdict was **direction good→best; do not reverse toward main** — these rows are
finish/tighten items, not rewrites.

**Scope of the review:** crate ownership, single type engine, fact-based cache,
framework adapters, project-bound external TS, LSP/provider readiness policy,
and residual dual-path / provisional surfaces.

---

## North star (keep)

These choices are correct and must not be weakened:

1. Exactly one query-time type-resolution engine (`ProjectSemanticDispatch` +
   `SemanticGraphStore`, five modes). OXC is syntax/lowering only.
2. Shallow index + lazy declaration bodies (`IndexedReady`, `DeclBodyMemo`).
3. Fact-based cache validity + single `ProjectTypeStore` (R21 env split,
   content-addressed vs query-identity, ReturnOnly admission).
4. Project-bound external TypeScript (`BoundProject`; no inferred production
   fallback).
5. Framework adapters plan/normalize only — never a second resolver.
6. Clean cutovers with legacy deletions (no dual-path shims).
7. Fail-closed mapping for virtual locations / unmapped provider edits.

---

## Improvements

### P0 — LSP readiness must not join background coverage

**Problem.** Interactive navigation (definition / type-definition / related)
must not wait on “dependency walk finished” / `DependencyReady` / bulk import
publication. That turns an optional accelerator into a global correctness
barrier. A deadline is a watchdog, not a readiness protocol.

**Required direction.**

- Gate only **request-local safety**: bound project still current; provider
  epoch current; current carrier’s exact IDE surface revision acknowledged;
  returned virtual locations map with exact content/map snapshot.
- Do **not** gate on: “all reachable deps published”, “scanner done”, “overlay
  fully warm”.
- Run Verter-native routes without waiting on provider background work.
- Background publication stays best-effort and priority-aware (must not steal
  interactive priority by default).

**Related in-tree notes:** codex opinion on import publication (local feedback),
store-backed provider lifecycle + nonblocking IDE work on the consolidation tip,
[`shared-tsgo-speculative-carrier-publication.md`](./shared-tsgo-speculative-carrier-publication.md)
(unread speculative publish cost).

**Acceptance sketch.**

- Definition/hover under a slow or stuck dependency publisher still answers
  from the current surface + native path within interactive SLOs.
- No interactive handler path joins a workspace-wide readiness handle.
- Gate/tests prove absence of the join, not only “faster under ideal load”.

---

### P0 — Kill speculative / unread provider publication on routes that never inject it

**Problem.** Shared-tsgo (and similar) can compile and publish every carrier
into overlay caches that nothing injects on that route — pure cost.

**Required direction.**

- Produce only what a live consumer will inject: open carriers, import fan-in
  the request actually needs, interactive-lane records.
- Route-specific producers: do not share “publish everything” with a route that
  only consumes demand-scoped inject.

**Owner lead:** LSP / external-TS publish coordinator + workspace scanner.
See [`shared-tsgo-speculative-carrier-publication.md`](./shared-tsgo-speculative-carrier-publication.md).

---

### P1 — Close provisional off-store framework caches (U10)

**Problem.** `FrameworkSurfaceStore` / `FrameworkScriptCaches` are
fact-validated but live on registry rows, **not** on `ProjectTypeStore`. That
re-opens the dual-path / incomplete-singleflight class the rest of the host
closed.

**Required direction.**

- Consolidate onto `ProjectTypeStore` with true singleflight and the same
  fact-rail admission as other query-identity DBs.
- Extend off-store absence guards so reintroduction fails the gate.

**Tracks:** `docs/arch/next/semantic-db-overhaul-unified-remaining-plan.md` (U10)
and CLAUDE Framework Adapter Substrate provisional note.

---

### P1 — Finish framework-surface wire finality (U8)

**Problem.** `FrameworkSurfacePayload` / embedded graph shape is provisional;
schema retag and version bump still owed after early landing.

**Required direction.**

- Retag graph carrier to the typeinfo payload shape.
- Bump `SemanticTypeGraph.schema_version`; reserve old field numbers per the
  Typeinfo Wire Contract.
- Keep validation-first executor; no bare-inner-request entry.

---

### P1 — Graph-native residual body readers (CLOSED)

**Problem — CLOSED at the terminal partition.** The authored-shape and
graph-backed-pending reader classes are EMPTY (the heritage/closedness cluster
went fact-native; the value-annotation, imported-registry, and locator-native
arms landed). The residual body-reader ledger stands at 1 migrated anchor + 6
`ProducerLowering` (permanent transient ingress, not on a path to zero) + 5
`GraphFreeDto` (below-graph, content-free facts/locators — except:
`route_closure`'s lease-reborrowed key-source mint, TRANSIENT ingress
(authored contributor bodies passed as `&[TypeExpr]` to
`produce_key_source_fact`,
`crates/verter_session/src/resolver_core/shallow_file_state.rs:2609` →
`crates/verter_semantic/src/facts/route_facts.rs:220`; never stored), and the
two external-frontier rows' read of the STORED
`LoweredTypeDecl.type_parameters` pocket — the terminal-storage violation
owned by the type-parameter-bound confinement block). No reverse
`HotTypeRef → TypeExpr → semantic decision` bridge exists.

**Required direction — landed / superseded.**

- Graph-native closedness / key-domain classifiers over `SemanticNodeData` —
  LANDED (`KeyDomainClosednessFact` minted at lazy decl-body lowering,
  evaluated dispatch-side over recipes + nodes).
- Explicit authored-shape surface — SUPERSEDED by the permanent-ingress ruling:
  producer boundaries transiently consume lease-only authored `TypeExpr` to
  mint graph, fact, or locator outputs and may not retain it.
- Residual inventory — landed as a permanent curated ratchet
  (`crates/verter_session/tests/cases/residual_type_expr_body_reader_inventory.rs`);
  do not expand the residual set. The open storage pockets (memoized
  `LoweredTypeDecl.type_parameters: Vec<TypeParam>`; `TypeParamBinding.constraint/default`
  `Arc<TypeExpr>` bounds in cached prepared decl bundles) are owned by the separate
  type-parameter-bound confinement block, not by this item.

**Tracks:** [`../authored-shape-graph-native-migration-deferral.md`](../authored-shape-graph-native-migration-deferral.md).

---

### P2 — Provider priority / document-feeding policy (finish the rescoped design)

**Problem.** Flood control without an active-priority scheduler regressed;
heartbeat-only and de-flood-alone were wrong fixes. Empirical proof exists;
implementation must stay on the rescoped design.

**Required direction.**

- Active-priority scheduler is the primary fix; de-flood is secondary and only
  behind that.
- Machine-adaptive backpressure only after proven IDE patterns — not speculative
  heartbeats as correctness.
- Interactive ops never share default priority with bulk background open/publish.

---

### P2 — Cognitive load of `verter_session` (structure without dual engines)

**Problem.** Session is the correct owner layer but is very large (~570 source
files). Risk is accidental second paths and review fatigue, not “split into
another resolver”.

**Required direction.**

- Prefer module extraction and sealed capability surfaces over new crates that
  reintroduce host↔session dual ownership.
- Keep structural confinement (`NoTypeExpr`, private DBs, absence guards)
  stronger than name-scanner guards for new rules.
- New query-identity caches only on `ProjectTypeStore`.

---

### P2 — Gate integrity proof (not attestation-only)

**Problem.** “(MANDATORY) Verification Must Prove Execution” is still ledgered
as incomplete; gates can pass without proving the intended surface ran.

**Required direction.**

- Tree-derived surface inventory + mutation/negative controls +
  `gate_contract_integrity`.
- Promote to `(CRITICAL)` only with a registered guard in the same change.

**Tracks:** [`../gate-integrity-ledger.md`](../gate-integrity-ledger.md),
[`../next/01-gate-integrity-block.md`](../next/01-gate-integrity-block.md).

---

### P3 — Cache admission class closure

**Problem.** One poison instance closed; the **class** of forgeable
cacheability remains open.

**Required direction.** Unforgeable `CacheabilityProbe` / audit-not-patch per
[`../next/cache-admission-closure-design.md`](../next/cache-admission-closure-design.md).

---

### P3 — Product honesty bounds (do not architectural-creep)

Keep these as explicit non-claims until their programs land:

- Full TypeScript-checker-grade typeinfo parity — multi-year, not “ledger green
  = parity”.
- Svelte SSR/hydration — not shipped; fail closed.
- Carrier IDE TS surface principle — promote to `(CRITICAL)` only with
  real-provider cross-region Vue/Svelte guards.
- Multi-claimant rename fail-closed stays until cross-project rename fan-out
  exists (no silent partial renames).

---

## Anti-goals (explicit)

Do **not**:

- Reintroduce a second query-time type resolver, OXC query-time resolve, or
  per-macro engine.
- Bring back inferred/config-less production external-TS projects.
- “Fix” interactive latency by lengthening deadlines or waiting harder on
  background completeness.
- Add off-`ProjectTypeStore` host caches “just for this surface”.
- Preserve dual paths behind flags “until the next branch lands”.
- Reverse crate layout toward `verter_host` / `verter_core` / `@verter/core`.

---

## Suggested execution order

1. **P0** interactive readiness predicate + stop unread speculative publish.
2. **P1** U10 store consolidation + U8 wire finality (can parallelize across
   owners). Graph-native residual body readers are CLOSED at the terminal
   partition (1 migrated anchor / 6 `ProducerLowering` permanent ingress /
   0 `AuthoredShape` / 5 `GraphFreeDto` named residual / 0 `GraphBackedPending`)
   — not outstanding work.
3. **P2** provider priority finish + session modularization discipline + gate
   integrity block.
4. **P3** cache-admission class + product-boundary promotions as evidence lands.

---

## Review snapshot (context)

| Axis | `main` (old) | consolidation (current direction) |
| --- | --- | --- |
| Semantic authority | host / analysis / core, 2 modes | single dispatch + graph, 5 modes |
| Cache | coarser host caches | facts + env dimensions + ReturnOnly |
| External TS | weaker / inferred-friendly | project-bound + fail-closed |
| Multi-framework | Vue-centric | adapter registry (Svelte experimental) |
| Dual paths | `@verter/core`, split engines | deleted + absence guards |
| Biggest live smell | n/a | background coverage as request safety |

**Overall:** strategic architecture is the right one for this product. Remaining
work is closing provisional surfaces and fixing the LSP readiness class — not
re-litigating the substrate.
