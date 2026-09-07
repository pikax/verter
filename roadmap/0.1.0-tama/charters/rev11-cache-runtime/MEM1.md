<!-- unified-charter-v2
id=MEM1
name=Aggregate semantic retention admission and pressure handling
predecessors=MEM0,E4,G1,G2,G3
phase=rev11
train=rev11.cache-runtime
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.cache-runtime:process-local aggregate retention reservations and admission policy
conflict_domains=semantic_cache_store
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-cache-runtime/MEM1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# MEM1 — Aggregate semantic retention admission and pressure handling

## Independently acceptable outcome

Implement one process-local aggregate retained-byte admission policy shared across projects, combining existing family caps with MEM0's finite byte limits. E4 supplies allocation charges and safe physical reclamation; G1/G2/G3 supply result contracts, singleflight and bounded execution.

## Concrete surfaces and binding architecture

Expected owners are ProjectTypeStore/cache admission in verter_session, `DeclLoweringService`/`SnapshotShard` retained-parse worker ownership in verter_session/src/decl_lowering.rs and E4's semantic storage charge handles. Bind exact files and symbols before mutation. Implement reserve/commit/release ownership against the one aggregate account, not per-consumer parallel quotas. Existing semantic query identity, fact validity and scheduler authority are unchanged. Follow contracts/resource-and-finalization.md for active, retained, shared and external pin accounting.

## Acceptance and discriminating proof

- **MEM1-AC1 — sole-owner outcome:** concurrent cache admissions and many distinct keys never exceed the configured retained-byte ceiling; every shared store participates and per-family caps still hold. Test aggregate exhaustion with individually legal keys.
- **MEM1-AC2 — positive contract:** a complete oversized or pressure-rejected result returns uncached within the active budget; exhausted active work returns the typed resource outcome. Neither path fabricates completeness or uses a stale candidate.
- **MEM1-AC3 — incremental equivalence:** cancellation, failed builds, ownership transfer and eviction release each reservation exactly once; held readers stay valid, closed projects stop new demand, and final release makes bytes reclaimable.
- **MEM1-AC4 — bounded work:** normal and pressure workloads preserve fresh/incremental answers and zero warm partial admission. Measure charge accounting and admission overhead using the MEM0 metric rows; characterize cold-work changes separately from semantic correctness.

## Cutover, verification and abort

Characterize existing admissions, connect charge handles and the aggregate reservation policy, then remove independent bypasses in the same candidate. No second cache-validity oracle or query engine is permitted. Run focused cache/scheduler/lifetime concurrency tests, the actual normal/pressure fixture cases and targeted-domain. G4 owns final cache population closure; L1 owns the long-run soak. Abort if an unaccounted allocation or unsafe pinned-reader eviction is required.

## Review and completion

Apply the node's fresh review profile and the bound final gate; affected findings and evidence are rerun after material changes. Transition only this node's predeclared implementation row inside its own implementation patch before review. Commit message, approximate date and optional PR are locator hints only. This charter amendment leaves the node pending.
