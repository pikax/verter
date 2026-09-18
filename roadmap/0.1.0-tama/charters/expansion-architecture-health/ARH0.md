<!-- unified-charter-v2
id=ARH0
name=Live responsibility and debt inventory
predecessors=A6,ORC0
phase=expansion
train=expansion.architecture-health
product=architecture_health
kind=constitution
semantic_role=delivery
class=successor
owner=expansion.architecture-health:Repository responsibility boundaries, deletion and complexity evidence; existing semantic owners survive
conflict_domains=host_service_graph,semantic_authority,test_maintenance
resource_class=docs-light
gate_profile=docs-domain
review_profile=architecture-3
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=high
review_effort_default=high
verification_effort_min=low
verification_effort_default=low
confirmation_effort_min=medium
confirmation_effort_default=medium
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-architecture-health/ARH0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=600
rescope_files=6
rescope_unrelated_packages=3
-->

# ARH0 — Live responsibility and debt inventory

Status: proposed; pending execution. Normative companion: `contracts/web-product-expansion-v1.md` and the existing Tama authority, review, resource and successor-charter contracts. Scheduling continues to use the existing trusted implementation ledger, not hashes or tool receipts.

## Independently acceptable outcome

Re-inventory the implementation candidate, separating production/test/generated code; map fan-in, fan-out, mutable state, change coupling and responsibilities to named surviving owners and deletion candidates.

## Sole owner and integration boundary

**Owner:** Repository responsibility boundaries, deletion and complexity evidence; existing semantic owners survive.

- Current-candidate responsibility inventory
- Narrow owner interfaces
- Before/after behavior and work evidence

This constitution inventories concrete production consumers, exact versions/sources and the full required capability matrix. It changes contract bytes only; implementation and deletion are bound to the named successor nodes in this train plan.

## Exact predecessor contracts

- **A6:** Implementation Lock Record.
- **ORC0:** Trusted implementation-ledger cutover.

Predecessors are required contracts, not authority transferred to this node. Existing node meanings are those of the supplied export plus explicit amendments; new parent definitions are in this delta. Completion is determined by the normal trusted-ledger policy. Merely finding a source file or a planned node is not readiness evidence.

## Bounded implementation and cutover scope

Deliver the outcome above through the named owner, using canonical source/profile/build identity and typed contribution interfaces. Consume existing parsing, types, resolution, maps, query scheduling, public result envelopes and edit transactions; no local substitute or silent fallback. The train plan identifies later consumers and its terminal; do not implement their independently acceptable work here.

Before mutation, inventory affected consumers and superseded paths on the actual implementation candidate. Assign each route exactly one surviving owner and a concrete deletion/cutover disposition. An empty deletion set is valid for a genuinely additive boundary, with a short explanation; do not invent old APIs to delete. Breaking changes are allowed, but affected current consumers, public docs and examples migrate together. No indefinite dual authority or compatibility adapter with no retirement owner.

## Acceptance IDs and discriminating evidence

- **ARH0-AC1 — required outcome and ownership:** demonstrate the precise outcome and every in-scope consumer using the sole owning interface. A compile-time/import boundary is preferred where it actually enforces the contract.
- **ARH0-AC2 — discriminating counterexample:** A large file alone or a previously split module must not be declared a god module without responsibility evidence. Use or extend existing evidence that would fail for this defect; this sentence is not an instruction to duplicate tests already proving it.
- **ARH0-AC3 — identity, incremental and work correctness:** for touched state/query/map boundaries, verify fresh versus incremental equivalence, edit/revert, cancellation, stale/partial rejection and deterministic ordering under perturbed discovery or scheduling. For untouched concerns, record a precise not-applicable rationale.
- **ARH0-AC4 — usable public delivery:** update the relevant VIM/DX capability and exact host/profile evidence, DOC1-tested public examples/reference, permissions, uncertainty and migration notes. A contract-only node specifies those obligations for its producers; a product terminal checks real completed artifacts, not templates or screenshots.
- **ARH0-AC5 — bounded cost and maintainability:** bind any new latency/work/allocation/RSS budget to the existing ratified performance methodology before measuring; prove obsolete paths retired and capabilities narrowed where refactoring is in scope. Do not win by removing required work or by reporting lower LOC alone.

Runtime/code-executing features require explicit trust, real engine/host evidence, teardown and source/config/build/session provenance. Static-only features do not spawn runtimes. Negative, partial, conditional and unsupported results are truthful, not routes for hiding unfinished required scope.

## Forbidden designs

- No generic service container, mega-context or second query engine
- No source-name tombstones, arbitrary LOC targets or recursive governance
- No second semantic authority, unqualified cache, regex recovery for semantic identity, whole-context helper or hidden cross-project state.
- No fabricated conformance, completion, source revision, speedup or supported-version claim.
- No qualifying an upstream feature as Verter support without the owned authored integration and public operation evidence.

## Abort and completion

Stop before mutation when current code contradicts the assumed owner, prerequisites lack accepted ledger completion, a reversible authored map is unavailable for a required operation, or scope exceeds one review context. Resolve by explicit charter/split change, not weakened semantics. Only an actual implemented and reviewed patch may transition this node's pending ledger row. This charter supplies no implementation evidence or completion date.

## Roadmap review 2026-09-18 — web-product delta

Constitution node, reviewed from the delta. Inventory constitution; kept.

Sizing: size M, 0 production LOC across 0 files, rescope at 600/6; efforts implementation medium, review high, verification low, confirmation medium; architecture-3 review, docs-domain gate, docs-light scheduling. Effective predecessors: A6, ORC0. The delta proposed uniform budgets (850 LOC / 8 files, implementation effort high) for its 431 nodes; this review sized each node from its actual owned outcome and split nodes carrying more than one independently acceptable outcome.
