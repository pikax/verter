# Q2 Structural-Body Cache — Population Deferral (TEMPORARY debt row)

**Status**: DEFERRED — cache + key + registry land dead-code-correct (byte-frozen);
POPULATION + the Q2 A-vs-B perf-consult + the deferred instrumentation counters defer
to a future cache-redesign block (Stage 7-ish, alongside the prepared-cache key migration).
**This is a TEMPORARY debt row** (per Rule-File Integrity) — it is cleared when that block
lands the sound cache, not before.

**Ruling source (codex-DEFER, binding)**: two converged neutral codex architecture legs —
`/tmp/mom/STAGE6/SLICE4/session8/codex/legA-OUT.txt` (finding 7) +
`legB-OUT.txt` (finding 4) — both `__DONE__`, exit 0, framing dispatcher-verified, ratified by
the CTO. The legs ruled Option B for the Stage-6 declaration-body storage flip and, as part of
that ruling, that the `PreparedStructuralBodyCache` must NOT be populated with resolving-lowerer
output this session.

## Why population is deferred (the soundness trap)

The Stage-6 declaration-body storage flip (Option B) mints `HotTypeRef` handles at the
graph-bearing dispatch producer `lower_decl_body_with_provenance`
(`crates/verter_session/src/project_semantic_dispatch/build.rs:2930`) by wrapping the
`SemanticNodeId` the **RESOLVING** lowerer `shallow_lower_type_expr_with_context`
(`crates/verter_session/src/project_semantic_dispatch/lower.rs:142`) already returns. The
resolving lowerer's output depends on **args / env / substitutions / name-resolution /
mode (demand) / scope payload**.

The `PreparedStructuralBodyCache`
(`crates/verter_session/src/resolver_core/structural_body_memo.rs`) key is
`StructuralBodyMemoKey { body_slot, provenance, merge_role }` — designed (per the module's own
doc) to partition a **STRUCTURAL-lowerer (env-independent template)** body by provenance +
merge-role. It does NOT carry the resolving lowerer's args/env/substitutions/name-resolution/
mode/scope dimensions. Caching a resolving-lowered body under that key would serve a body
resolved under one env/args to a different context ⇒ **UNSOUND** (a correctness defect, not a
perf nit).

## Why deferral is safe (no correctness gap)

The warm-state posture is already sound **without** this cache: the existing
`SemanticGraphStore` query memo handles warm reuse of the `Instantiate` key — the body is NOT
re-lowered on warm hits. So the per-`Instantiate`-cold-build cost is the correct floor, and the
Q2 cache is a future OPTIONAL optimization, not a correctness requirement. Both legs confirm
this.

## What lands now vs what defers

**Lands now (S4, Option B)** — dead-code-correct, byte-frozen, UN-wired:
- `PreparedStructuralBodyCache` + its registry (`PreparedStructuralBodySlotId` minting) +
  `StructuralBodyMemoKey` + the `get`/`insert`/`register`/`descriptor` surface.
- `PreparedDeclBundle.structural_body_cache` (built empty).
These stay `#[allow(dead_code)]`-correct with no production caller.

**Defers to the future cache-redesign block**:
- Cache POPULATION. The durable redesign is one of: (a) an **env-independent
  structural-template** cache (only sound for genuinely query-free template bodies), or (b) a key
  that **carries the missing resolving dimensions** (args/env/substitutions/name-resolution/mode/
  scope) — a separate design that must respect R6/R21 (no `HotTypeRef`/`SemanticNodeId`/version/
  content-hash in a derived query-identity key; version roots on the cached value's facts).
- The **Q2 A-vs-B MEASURED perf-consult** (Option A "context-keyed cache" vs Option B
  "recontextualizer"). It is **MOOT for S4**: both poles assumed the cache is populated, and there
  are no warm A-vs-B numbers because the cache stays un-wired. The A-vs-B decision defers to the
  cache-redesign block, not S4.
- The **3 deferred S4-bump instrumentation counters** (cold-build-ns / errors /
  direct-lower-bypass). They have no populated cache to instrument; they defer with the cache.

## Closure criterion

This debt row is cleared when the future cache-redesign block lands the sound cache (key carrying
the correct dimensions OR a genuinely env-independent structural-template surface), populates it,
runs the instrumentation, and resolves the Q2 A-vs-B perf-consult — at which point the
dead-code-correct substrate becomes live and this file is deleted.
