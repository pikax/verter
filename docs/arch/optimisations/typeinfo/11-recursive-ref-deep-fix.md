# 11 — RecursiveRef indexed-access deep fix (implemented; composes with the wire-kind publication)

**Status:** implemented and green on branch `fix/recursive-ref-indexed-access` @ `213c1ddfe`
(base `fd21791c0` = the perf-combined tip; measurement machine, not pushed). 22 files, +324/−17.
**Relation to `codex/release-consolidation-raw`:** that branch publishes self-referential indexed
access as a first-class `recursiveRef` WIRE KIND (publication-contract solution) — the corpus passes
there without this fix. THIS fix addresses the complementary half: demand-time projection when a
consumer actually WALKS INTO `Identity['member']`. The identity payload on the sentinel is the shared
substrate both halves need. Landing this on the consolidation branch is recommended but optional.

## Root cause (instrumented, corpus-exact)

`Link.vue` props `href?: NuxtLinkProps['to']`: the `is_instantiate_active` zero-arg-head back-edge
(`project_semantic_dispatch/carrier.rs:471-484`) mints `Opaque(QueryError::RecursiveRef { name })`
while `Instantiate(NuxtLinkProps)` lowers its own body, and the sentinel is baked into the completed
surface as the IndexedAccess OBJECT. The walker (`project_semantic_dispatch/walk.rs`) had a mid-walk
re-entry arm ONLY for `Opaque(DeclPlaceholder)` (~line 2091); every other `Opaque(_)` fell to the
terminal-miss arm (~2336) → `Opaque(Miss)` → unknown-materializing classification
(`raise/shape_engine/node_domain.rs:1317`) → `structural_member_value_source` None →
`UnrepresentableRequiredMemberValue` at the output sink.

## Design (as landed)

1. **Sentinel identity:** `QueryError::RecursiveRef { name }` → `{ canonical_id: Arc<str>, name:
   Arc<str> }` (semantic_query.rs ~2636 + PartialEq/Hash arms) — mirrors `DeclPlaceholder` minus
   `whole_hash` (the walker re-derives the live slot via `type_slot_for`; no content hash rides the
   sentinel; R6-clean — node VALUE, never a query key). Six mint sites threaded:
   `project_semantic_dispatch/mod.rs:1474` (memo same-path sentinel), `build.rs:1076` + `build.rs:2471`,
   `carrier.rs:478`, `locator_view.rs:968`, and `walk.rs:1221` (the depth-budget fuse — EMPTY canonical
   by contract, never re-entered).
2. **Verdict-neutral consumers unchanged:** raw spelling stays `recursiveRef({name})` (surface.rs:829);
   raise stays a name-only `TypeExpr::RecursiveRef` leaf; RefCycle detection stays name-keyed
   (graph_predicates.rs:703). Relation/conditional oracles need NO change (`RecursiveRef` already
   excluded from `is_error_type()`).
3. **Walker re-entry:** new arm directly after the DeclPlaceholder arm, guarded
   `!canonical_id.is_empty()`: dispatch `Instantiate(type_slot_for(canonical, name), [],
   structural_transit())` with the same intermediate-hop demand demotion as the model arm. By walk
   time the outer instantiation has completed, so the memo serves the completed surface and
   `Identity['member']` terminates. Termination rides EXISTING rails only: `expanded == current`
   no-progress fail-close (`type A = A`), frame-level `visited_nodes`, memo same-path detection
   (`T = T['x']` re-dispatches the identical ProjectPath key and sentinels out), and the armed
   projection-op budget. A RecursiveRef at REST (no remaining segments) stays a leaf — shallow-by-default
   publication of recursive types is preserved.

## Tests

- `self_referential_indexed_access_prop_resolves_present` (meta_tests.rs) — RED on base, GREEN
  post-fix. Fixture gotcha (documented in the feedback file): reproducing the corpus shape requires
  BOTH builtin-utility heritage on the self-referential interface AND heritage-reached consumption.
- `pathological_self_indexed_access_without_base_terminates` — genuine-infinite case terminates within
  budget (mpsc recv_timeout watchdog, 32 MiB stack).
- The three pinned fail-closed rails + `recoverable_shallow_prop_values_still_complete_as_present`
  stay green. Full `verter_session --lib`: 4215/0. Clippy delta vs base: zero. fmt clean.

## Corpus effect (on the fd-base combined tree)

Fixed Link.vue, Button.vue, Input.vue, InputMenu.vue; ZERO new failures. Residuals diagnosed
byte-identical on base and NOT this bug's family: DashboardSidebarCollapse props[23] (theme-chain
Computed-Mapped partial from `Instantiate(Button.vue::Button)`) and Separator (a mis-spaced locator —
`TypeBodySlot { reka-ui index4.d.ts#_default$79, space: Type }` targeting a `declare const`, i.e.
VALUE-space, failing `raise_body_slot`) — both smaller follow-ups.
