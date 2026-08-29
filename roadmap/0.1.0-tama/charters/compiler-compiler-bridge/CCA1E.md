<!-- unified-charter-v2
id=CCA1E
name=Vue FrameworkSemanticAuthority backend
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1D
owner=compiler.compiler-bridge:Vue FrameworkSemanticAuthority implementation
conflict_domains=semantic_authority,vue_product
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
charter=charters/compiler-compiler-bridge/CCA1E.md
max_production_loc=600
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1E — Vue FrameworkSemanticAuthority backend

## Independently acceptable outcome and rollback boundary

Implement Vue `FrameworkSemanticAuthority<FrameworkEpoch>` for position-preserving `eval_source`, template facts, and Vue-owned semantic interpretation without moving consumers. Reverting removes only the unused Vue authority registration.

## Concrete surfaces and APIs

- Surfaces: `framework_common/vue_bridge.rs`, Vue semantic helpers, and catalog registration.
- Owns Vue `eval_source`, `template_data`, framework facts, provenance, and typed semantic identity.
- Consumes the CCA1D parse artifact; owns no projection bytes, runtime output, session route, or style preprocessor authority.

## Exact predecessor contract

- **CCA1D:** implemented ledger row for “Registered frontend convergence join”; its CCA1D1/CCA1D2 ancestors separately own parse execution and complete-only publication.

## Acceptance and evidence

- Pinned Vue fixtures preserve eval-source bytes/offsets, template facts, spans, diagnostics, provenance, and ordering.
- Shared semantic/type services remain the only lower resolver/analyzer; no parse re-entry occurs.
- No production consumer uses the new authority in this node.

## Deletions, budgets, and aborts

- Delete no current consumer method; forbid projection/runtime/session/style-continuation work.
- Ceiling: 600 LOC, 5 files, 1 crate; rescope on consumer migration or a second resolver.
- Abort on semantic/fact divergence or framework branches required in generic session code.

## Verification and review

Run focused Vue eval/template-fact tests, compiler/semantic suites, and `targeted-domain`. Apply `architecture-3`; add only CCA1E's ledger row.
