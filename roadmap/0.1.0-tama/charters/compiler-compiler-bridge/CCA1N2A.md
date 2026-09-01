<!-- unified-charter-v2
id=CCA1N2A
name=Native host binding substrate
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1N1,CCA1N2
owner=compiler.compiler-bridge:sealed request-scoped BoundNativeHostRequest binding substrate
conflict_domains=compiler_execution,host_service_graph
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1N2A.md
max_production_loc=600
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N2A — Native host binding substrate

## Independently acceptable outcome and rollback boundary

A sealed, unused, request-scoped binding substrate maps an authoritative registered artifact/catalog identity to exactly one framework-specific host binding without changing any production route. Reverting removes only this unused substrate.

## Concrete surfaces and APIs

- Surfaces: a new session-side binding module (expected under `crates/verter_session/src/host_resolve/`) plus bounded reads of catalog identity from `verter_compiler`; focused unit tests.
- Named boundary: `BoundNativeHostRequest` as a sealed sum over the registered framework host variants (Vue, Svelte), constructed only through one guarded constructor from the exact adapter + carrier-language + framework-epoch + host-epoch catalog row of one immutable source snapshot.
- The binding carries ONLY: the opaque framework-specific host binding; immutable source snapshot/revision identity; framework and host epoch/lifecycle witnesses; and structured catalog/audit identity needed to attribute the request. "Opaque" is operational: the framework-specific content is invocable only at the single by-value consumption point and is never fetchable as a service from the binding. It is not a service locator and must not expose or carry frontend, semantic, projection, runtime, host, store, audit-runtime, or cancellation services, nor any general capability bag.
- Type discipline: not `Clone`, not `Copy`, not serializable, never cached or stored beyond the request; designed for exactly-once by-value consumption.
- Typed binding-unavailable outcomes for an unregistered identity, a mismatched epoch, or a stale snapshot; construction guards make identity/variant mismatch (for example a Svelte identity in the Vue variant) structurally impossible or a typed failure.
- Excluded: production selector activation, host-backed or runtime-render execution, `CompileAdmission` issuance, audit policy, outer-call deletion, public DTO changes, cache publication, and framework request topology.

## Exact predecessor contracts

- **CCA1N1:** the Vue host backend exists with demand-specific issuance, giving the Vue variant a real framework-specific target.
- **CCA1N2:** the Svelte host backend exists symmetrically for the Svelte variant.

## Acceptance and evidence

- Exactly one binding constructor exists; it derives framework identity solely from registered catalog identity for one immutable source generation, never from path text, extension sniffing, or a lane-supplied flag.
- Negative evidence: cross-framework variant construction, stale-generation construction, and epoch-mismatched construction fail closed with typed outcomes.
- Structural evidence proves no production route consumes the substrate in this node and the binding type is non-Clone, non-serializable, and unstored.

## Deletions, budgets, and aborts

- Delete nothing; forbid route activation and admission issuance.
- Ceiling: 600 LOC, 6 files, 2 crates; rescope if execution, publication, or DTO work enters.
- Abort if the binding cannot be expressed without a general capability/service bag or without persistent token storage.

## Verification and review

Use TDD for constructor/guard/unavailability boundaries, run session host-binding unit suites and `targeted-domain`. Apply `public-3`; add only CCA1N2A's ledger row.
