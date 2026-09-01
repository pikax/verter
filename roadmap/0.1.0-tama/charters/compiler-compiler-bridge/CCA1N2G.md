<!-- unified-charter-v2
id=CCA1N2G
name=Registered grammar selection
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1J
owner=compiler.compiler-bridge:catalog-derived carrier grammar selection at source ingestion
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
charter=charters/compiler-compiler-bridge/CCA1N2G.md
max_production_loc=300
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N2G — Registered grammar selection

## Independently acceptable outcome and owners

Carrier grammar selection at the shared source-ingestion stage derives from the immutable compiler catalog row of the file's carrier identity instead of a Vue-else-Svelte framework fallthrough. This stage serves every parse — including parses no compile request triggers — so its authority is the registered identity row, never a request-scoped binding. Current ownership is the hardcoded fallthrough in the source-ingestion stage; final ownership is the immutable compiler catalog's registered grammar column. Reverting restores only that fallthrough.

## Concrete surfaces and boundary

- Surfaces: the grammar-selection branches in `crates/verter_session/src/host_executor.rs` and `crates/verter_session/src/host_manage/overlay_materialize.rs` (the overlay source-registration site carries the same displaced fallthrough class), the registered grammar column on the immutable compiler catalog row, and focused tests.
- Grammar identity is registered carrier geometry: its sole authority is the `CarrierFrontend` registration row in the immutable compiler catalog. The session consumes that fact through catalog lookup (a session-side descriptor may project it but is never an alternative authority).
- The lookup is a registered-identity fact read available to every parse; it must not construct, forge, store, or widen any request-scoped binding, and it must not add a framework predicate — an unregistered or grammar-less row fails closed with a typed outcome instead of defaulting to another framework's grammar.
- Compile-lane routing, request construction, admission, and both outer `compile_bundle` calls are excluded.

## Exact predecessor contract

- **CCA1J:** implemented ledger row for “IDE projection route convergence”; registered catalog identity rows are the live per-carrier authority this node extends with grammar facts.

## Invariants and acceptance

- No Vue-else-Svelte grammar fallthrough remains at any source-registration site (host ingestion and overlay registration); a third registered adapter never receives another framework's grammar: it carries its registered grammar fact where the representation admits one and otherwise fails closed with a typed outcome; opening the grammar representation to further adapters is owned by the framework-adapter substrate.
- Grammar for Vue and Svelte carriers is byte-identical to the current behavior; parse output, diagnostics, and ordering are unchanged.
- An unregistered carrier or a row without grammar facts fails closed with a typed outcome; no silent cross-framework grammar substitution.
- The change adds no parse, resolve, or copy work; the lookup is a registered-fact read.

## Deletions, budget, and verification

Delete only the displaced grammar fallthrough. Ceiling: 300 production LOC, 6 files, 2 crates; abort if compile-lane routing or binding work enters. Run source-ingestion/parse suites for both frameworks and `targeted-domain`. The native host-integration convergence join consumes this fact.
