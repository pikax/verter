<!-- unified-charter-v2
id=CCA1A
name=Typed compiler capability catalog
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA0
owner=compiler.compiler-bridge:typed compiler capability traits and immutable catalog schema
conflict_domains=compiler_execution,capability_catalog
resource_class=rust-mixed
review_profile=architecture-3
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
charter=charters/compiler-compiler-bridge/CCA1A.md
max_production_loc=500
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1A — Typed compiler capability catalog

## Independently acceptable outcome and rollback boundary

Land the five typed capability interfaces and one immutable catalog schema without routing any production request through them. This node is independently reviewable because reverting it removes only unused type/catalog infrastructure; parse, projection, compile, publication, and host behavior remain on their current routes.

The sole owner is **typed compiler capability traits and immutable catalog schema**. This node does not mark the capability migration complete.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework_common`, `crates/verter_compiler/src/lib.rs`.
- Owned APIs: `CarrierFrontend`, `FrameworkSemanticAuthority<FrameworkEpoch>`, `ProjectionBackend`, `RuntimeCompilerBackend<FrameworkEpoch>`, `FrameworkHostIntegrationBackend<FrameworkEpoch, HostEpoch>`, and the immutable capability catalog rows that associate them.
- Compile-time truth must distinguish absent capabilities; a tooling-only carrier is representable without a runtime backend stub.
- `CompileArtifactSet` is excluded. CCA2A is its sole schema owner; CCA2B–CCA2F own the ordered migrations and CCA2 is only their convergence join.

## Exact predecessor contract

- **CCA0:** implemented ledger row for “Compiler authority, policy, demand, and admission constitution”.

## Acceptance IDs and discriminating evidence

- **CCA1A-AC1 — capability truth:** compile-time fixtures represent frontend-only, projection-only, and runtime-capable registrations without placeholder implementations.
- **CCA1A-AC2 — immutable identity:** duplicate framework/epoch/host identities fail construction and catalog iteration is deterministic.
- **CCA1A-AC3 — dependency direction:** structural compile/dependency evidence proves the generic catalog does not import framework-private semantic types or host/session owners.
- **CCA1A-AC4 — zero-route change:** bounded call-site inspection plus existing compiler/session tests proves no production request consults the new catalog in this node.

Test homes: `crates/verter_compiler/tests` and compile-fail fixtures owned by `verter_compiler`.

## Deletions and forbidden designs

- Delete no current route, registry, option type, or output adapter here.
- Forbid `Any`-erased artifacts, optional methods that pretend missing capabilities exist, runtime capability probes inside per-node loops, mutable process-global registration, and a `CompileArtifactSet` definition.

## Budgets and mandatory rescope

- Target ceiling: 500 production LOC, 5 production files, 1 related crate/package.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if route migration/output schema work enters the diff.
- Correctness budget: zero identity aliasing or false-positive capability claims.
- Performance budget: catalog construction occurs once; steady-state compile work and allocations may increase by 0 because no route uses it yet.

## Abort conditions

- Abort if a capability cannot be represented without importing a framework-private or host-owned type.
- Abort if current-source inventory reveals a sixth authority rather than a capability of one of the five ratified owners.
- Abort if the patch must change observable compiler bytes, maps, diagnostics, cache behavior, or publication.

## Verification and review

1. Run the smallest compile/type fixtures that discriminate the capability matrix.
2. Run `cargo nextest run -p verter_compiler`.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Before squash/review, add only CCA1A's `[[implemented]]` row to `authority/state/implemented.toml`; no sibling row is implied.
