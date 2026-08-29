<!-- unified-charter-v2
id=CCA2E
name=Legacy custom-block descriptor deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA2EH
owner=compiler.compiler-bridge:terminal RuntimeCustomBlock and ambiguous descriptor deletion
conflict_domains=compiler_execution,source_lineage
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
charter=charters/compiler-compiler-bridge/CCA2E.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2E — Legacy custom-block descriptor deletion

## Independently acceptable outcome and owners

Delete `RuntimeCustomBlock` and any remaining ambiguous `CustomBlock`/`VerterCustomBlock` compatibility shape or export whose consumers now use `CustomBlockDescriptor`. Current residual ownership is the legacy neutral descriptor bucket; final ownership is the source-backed descriptor produced by Vue and consumed through neutral/session paths. Reverting restores only unused compatibility declarations.

## Exact deletion population and boundary

- `crates/verter_compiler/src/framework_common/carrier_compiler.rs` and `framework_common/mod.rs` — delete `RuntimeCustomBlock` and its export.
- Bounded compile-type surfaces — delete only superseded ambiguous descriptor declarations/conversions proven to have zero consumer; retain parse-owned facts still needed to construct `CustomBlockDescriptor`.
- Focused fixtures/tests use durable source-backed descriptor terminology and behavior.

Do not alter descriptor schema, Vue production, neutral/session consumption, staged relations, custom-block semantics, provider behavior, or public wire APIs.

## Exact predecessor contract and binding laws

- **CCA2EH:** neutral bundle, Vue assembly, and session virtual-node consumers use complete source-backed descriptors directly; conversion/reconstruction helpers are absent.
- Every legacy declaration must have zero production consumer before deletion. A live consumer reopens CCA2EV or CCA2EH; no compatibility fallback or dual descriptor may be retained here.
- The current Svelte runtime custom-block producer cell remains bounded inapplicable and zero-work. This deletion must not create a Svelte migration to justify removing shared code.

## Acceptance, performance, aborts, and verification

- **CCA2E-AC1:** structural/type evidence finds no `RuntimeCustomBlock`, superseded ambiguous descriptor, export, constructor, conversion, or fallback.
- **CCA2E-AC2:** descriptor fields, identities, provenance, spans, content states, virtual-node behavior, and deterministic order remain equivalent.
- **CCA2E-AC3:** fresh/incremental/cancellation/complete-only evidence from CCA2EV/CCA2EH remains green.
- **CCA2E-AC4:** deletion adds no work; unknown/absent/Svelte-inapplicable cells remain zero semantic/runtime/provider work.

Ceiling: 300 production LOC, 3 production files, 1 crate. Abort on a live legacy consumer, transform/plugin/provider work, invented Svelte production, descriptor-schema mutation, or a fourth production file. Run structural scans plus compiler/session custom-block/assembly/host/span suites and `targeted-domain`. CCA2F consumes the legacy-free descriptor boundary.
