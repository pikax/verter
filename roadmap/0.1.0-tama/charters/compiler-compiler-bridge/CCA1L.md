<!-- unified-charter-v2
id=CCA1L
name=Svelte RuntimeCompilerBackend implementation
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1G
owner=compiler.compiler-bridge:Svelte runtime backend over the canonical standalone compiler core
conflict_domains=compiler_execution,svelte_product
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
charter=charters/compiler-compiler-bridge/CCA1L.md
max_production_loc=700
max_production_files=6
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1L — Svelte RuntimeCompilerBackend implementation

## Independently acceptable outcome and rollback boundary

Implement `RuntimeCompilerBackend<SvelteEpoch>` over the canonical typed `CompileRequest` → `standalone.rs` parsed core → atomic `ArtifactSet` path, without moving production consumers. Reverting removes only the unused Svelte runtime adapter/catalog row.

## Concrete surfaces and APIs

- Surfaces: `crates/verter_compiler/src/standalone.rs`, `compile_request`, Svelte parsed runtime helpers, and catalog registration.
- Owns Svelte typed runtime request/options, coarse target plan, runtime bytes/maps/diagnostics, and truthful refusal.
- The legacy `CarrierCompiler::compile_bundle` may later delegate to this backend in CCA1M2, but it is not an implementation substrate or authority here.
- Preserves current output adapters; `CompileArtifactSet`/assembly are CCA2A/CCA2B and direct/host-backed delegation is CCA1M1/CCA1M2.

## Exact predecessor contract

- **CCA1G:** implemented ledger row for “Framework semantic consumer convergence”.

## Acceptance and evidence

- Svelte runtime corpora preserve bytes, maps, diagnostics, targets, options, and deterministic ordering.
- One request shares prerequisites across targets with no duplicate parse/semantic/plan/emit pass.
- No production caller uses the new backend in this node.

## Deletions, budgets, and aborts

- Delete no consumer route; forbid Vue, session, host, public schema, or staged assembly work.
- Ceiling: 700 LOC, 6 files, 1 crate; rescope if consumer migration enters.
- Abort on hidden option defaults, dual authorities, or output/performance divergence.

## Verification and review

Run focused Svelte runtime/conformance tests, `cargo nextest run -p verter_compiler`, and `targeted-domain`. Apply `public-3`; add only CCA1L's ledger row.
