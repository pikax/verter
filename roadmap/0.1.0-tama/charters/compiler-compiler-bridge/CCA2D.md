<!-- unified-charter-v2
id=CCA2D
name=Unqualified style boundary deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA2DV,CCA2DS
owner=compiler.compiler-bridge:terminal unqualified-style input and DTO deletion
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
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA2D.md
max_production_loc=500
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2D — Unqualified style boundary deletion

## Independently acceptable outcome and owners

Delete the now-unused unqualified supplied/prepared-style transport, `RuntimeStyleBlock`, and compatibility helpers after Vue and Svelte independently consume the stage-qualified boundary. Current residual ownership is the legacy neutral DTO/input bucket; final style handoff ownership is CCA2D0's typed continuation. Reverting restores only unused compatibility declarations.

## Exact deletion population and boundary

- `crates/verter_compiler/src/framework_common/carrier_compiler.rs` and `framework_common/mod.rs` — delete `RuntimeStyleBlock` and its exports.
- `crates/verter_compiler/src/style_planner.rs`, `standalone.rs`, and bounded request/transport surfaces — delete only unqualified supplied/prepared-style fields and helpers with zero remaining consumer.
- Focused fixture/test construction is retargeted to qualified style artifacts using durable style identity/stage wording.

Do not alter `StyleStage`, `QualifiedStyleResult`, the qualified continuation/artifact, CSS preprocessing/semantics, framework-specific output behavior, or facade/host publication.

## Exact predecessor contracts and binding laws

- **CCA2DV:** all Vue style consumers use the qualified continuation; Vue-side unqualified fields/adapters are absent.
- **CCA2DS:** all Svelte style consumers use the qualified continuation; Svelte-side unqualified fields/adapters are absent.
- Every legacy declaration must have zero production consumer before deletion. Discovery of a live consumer reopens the owning migration; no compatibility fallback or dual DTO may be retained here.

## Acceptance, performance, aborts, and verification

- **CCA2D-AC1:** repository-wide structural/type evidence finds no unqualified style input, `RuntimeStyleBlock`, export, constructor, helper, or fallback.
- **CCA2D-AC2:** Vue/Svelte CSS bytes, qualified maps, diagnostics, provenance, stage/basis, source order, scoped/modules/global behavior remain equivalent.
- **CCA2D-AC3:** fresh/incremental/cancellation/complete-only evidence from both migrations remains green.
- **CCA2D-AC4:** deletion adds no work; one qualified continuation remains per applicable style and absent/inapplicable style stays zero-work.

Ceiling: 500 production LOC, 5 production files, 2 crates. Abort on a live legacy consumer, CSS semantic/preprocessor change, qualified-boundary mutation, or a sixth production file. Run structural scans plus Vue/Svelte style/preprocessor/host/map suites and `targeted-domain`. CCA2F consumes the legacy-free qualified boundary.
