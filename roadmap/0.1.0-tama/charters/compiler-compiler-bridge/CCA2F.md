<!-- unified-charter-v2
id=CCA2F
name=Compiler facade integration and retained-adapter ownership
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2C,CCA2D,CCA2E
owner=compiler.compiler-bridge:staged compiler facade adapters and named downstream API ownership
conflict_domains=style_semantics,compiler_execution,host_service_graph
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA2F.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2F — Compiler facade integration and retained-adapter ownership

## Independently acceptable outcome and owners

Expose the completed staged artifact, framework assembly handoff, qualified style continuation, and source-backed custom-block descriptor through the existing compiler/session facade while retaining only bounded behavior-preserving adapters whose downstream API owners are mapped in this charter. Current facade exposes SFC-shaped outputs; final bridge facade exposes the staged contracts consumed by later C2/CMP/performance work. Reverting restores only facade adapters.

## Concrete surfaces and boundary

- Production surfaces are `crates/verter_compiler/src/compile/mod.rs`, `standalone.rs`, `assembly/mod.rs`, `assembly/publish.rs`, `lib.rs`, and facade/handoff consumers in `crates/verter_session/src/host_compile.rs` and `host_resolve/virtual_file_pipeline.rs`.
- Own facade conversion to `CompileArtifactSet` and stable public-in-crate operations. Production comments may state only the durable API owner or invariant; they must not carry temporary-adapter/deletion-owner tables, roadmap IDs, phases, sequence, or deletion history.
- This node does not mutate downstream `C2`, `CMP1`, or `CPER1`. Those production-capable nodes own the named adapter deletions after their corresponding API authorities are established.

## Charter-local retained-adapter mapping

This table is roadmap coordination authority and must not be copied into production comments or tests.

| Retained bridge shape after CCA2F                                             | Intended downstream API authority                                                 | Production-capable deletion owner                                                                   |
| ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| SFC-shaped compile-result projection over `CompileArtifactSet`                | `C2` — `CompileAttempt`/sealed staged compile transaction                         | `C2` removes the projection as part of the sealed-facade cutover.                                   |
| Mixed request/policy compatibility conversion feeding the staged facade       | `CMP0` — typed `CompileRequest`, `CompilerPolicy`, demand and artifact qualifiers | `CMP1` removes the conversion when canonical demand-refined admission becomes the production route. |
| Legacy work-attribution view over staged parse/plan/emit/assembly/copy events | `CPER0` — `CompilerWorkLedger`/`WorkKind`/`OwnerPhase`                            | `CPER1` removes the view when the phase/owner-labeled ledger becomes the production authority.      |

No fourth retained adapter is authorized. Discovery of another required compatibility shape is a rescope, not an invitation to add a production ownership table.

## Exact predecessor contracts

- **CCA2C:** host lifecycle/publication consumes the staged artifact handoff.
- **CCA2D:** CCA2D0/DV/DS established the qualified style boundary and both framework migrations; terminal deletion removed unqualified style DTOs/inputs.
- **CCA2E:** CCA2E0/EV/EH established source-backed descriptors, Vue production, and neutral/session consumption; terminal deletion removed legacy descriptor shapes while the Svelte producer cell remained bounded inapplicable.

## Invariants and acceptance

- Existing compiler bytes/maps/diagnostics remain equivalent through adapters; text-only requests construct no native AST artifact.
- Generic session code contains no framework module topology or unqualified style continuation.
- Every retained adapter appears exactly once in the charter-local table, has one intended downstream API authority and one production-capable deletion owner, and carries no dual-running semantic authority; work counters permit no duplicate parse/plan/emit/assembly/copy.
- Source and tests use durable API/invariant wording only. A scan must find no temporary-adapter/deletion-owner table or roadmap coordination citation in production comments, diagnostics, test names, comments, fixtures, snapshots, or assertions.

## Deletions, budget, and verification

Delete only facade shapes whose complete consumers migrate here; retain exactly the three charter-mapped downstream adapters. Ceiling: 700 LOC, 7 files, 2 crates; abort if a fourth adapter, downstream semantic algorithm, public wire change, or in-code ownership/deletion table is required. Run compiler/session facade/host/map suites, targeted source/test vocabulary scans, and `targeted-domain`. CCA2 is the zero-production convergence join consumed by C2, CMP0, and CPER0.
