<!-- unified-charter-v2
id=CCA2D0
name=Stage-qualified external-style boundary
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA2A,J1
owner=compiler.compiler-bridge:additive stage-qualified external-style continuation contract
conflict_domains=style_semantics,compiler_execution
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
dispatchable=false
optional=false
release_gating=none
external_requirements=maintainer_cca2_style_contract_and_deletion_ownership_ruling
charter=charters/compiler-compiler-bridge/CCA2D0.md
max_production_loc=500
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2D0 — Stage-qualified external-style boundary

Dispatch is blocked pending `maintainer_cca2_style_contract_and_deletion_ownership_ruling`. The ruling must identify the ratified predecessor that supplies the typed completed-preprocessing result and partition legacy style-transport deletion between that style owner and CCA2D. This charter does not authorize either choice in advance.

## Independently acceptable outcome, role, and owners

Add an unused compiler-owned external-style continuation boundary that accepts one J-owned completed preprocessing result and can describe its CCA2A artifact without becoming a second preprocessor. Current supplied/prepared style values are unqualified; final input contract is qualified by exact `StyleStage`, source/style identity, revision, content basis, provenance, diagnostics, and map chain. Reverting removes only the additive types and tests.

## Expected production surfaces and named APIs

- `crates/verter_compiler/src/style_planner.rs` — define the continuation input/validation boundary over J-owned `QualifiedStyleResult`/`StyleStage` semantics.
- Bounded `crates/verter_compiler/src/assembly/fragment.rs`, `source_space.rs`, `source_unit.rs`, or `publish.rs` — represent the qualified style artifact/relation in `CompileArtifactSet`.
- Compiler exports expose only immutable typed construction and validation; no production Vue/Svelte/session caller is migrated here.

The boundary includes authored dialect, completed preprocessing stage, canonical source/style identity, source revision, input hash/basis, content bytes, qualified map, diagnostics, and complete/refused state. CSS parsing/preprocessing, selector semantics, formatting, framework consumption, and old-route deletion are excluded.

## Exact predecessor contracts and binding architecture

- **CCA2A:** the artifact schema can represent role, provenance, typed relations, content state, and source/generated map spaces.
- **J1:** CSS owner reconciliation identifies the sole preprocessing authority, but does not itself define the currently named `StyleStage`/`QualifiedStyleResult` contract. The maintainer ruling must identify the supplying boundary and amend this predecessor edge before implementation.
- Construction rejects wrong stage, stale or mismatched basis, source aliasing, duplicate continuation, unqualified maps, and partial results. It never guesses, reparses, preprocesses, or normalizes CSS independently.

## Internal subblocks, acceptance, and performance

1. Define immutable qualified identity and validation with no route call site.
2. Define deterministic conversion to a staged style artifact with complete-only admission.
3. Prove positive construction and planted wrong-stage, stale-basis, source-mismatch, duplicate, and unqualified-map failures.

- **CCA2D0-AC1:** the only new authority validates identity/stage/basis; it has zero production consumers and deletes nothing.
- **CCA2D0-AC2:** bytes, diagnostics, provenance, source-unit relation, and map spaces round-trip deterministically.
- **CCA2D0-AC3:** stale/cancelled/partial input cannot construct an admissible artifact.
- **CCA2D0-AC4:** construction performs bounded validation/metadata work, no CSS parse/transform/copy beyond declared artifact ownership, and absent/inapplicable input is zero-work.

Ceiling: 500 production LOC, 5 production files, 1 crate. Abort on a production route mutation, second preprocessor, framework-specific behavior, CSS semantic work, or a sixth production file. Run compiler style/artifact/map tests and `targeted-domain`; CCA2DV and CCA2DS consume this additive boundary independently.
