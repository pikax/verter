<!-- unified-charter-v2
id=CCA1GE
name=Eval-source semantic consumer cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1E,CCA1F
owner=compiler.compiler-bridge:eval-source authority route and combined-method deletion
conflict_domains=semantic_authority,compiler_execution
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
charter=charters/compiler-compiler-bridge/CCA1GE.md
max_production_loc=600
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1GE — Eval-source semantic consumer cutover

## Independently acceptable outcome, role, and owners

Route the session eval-source request through the registered framework's `FrameworkSemanticAuthority`, then delete only the displaced `CarrierCompiler::eval_source` declaration and Vue/Svelte implementations. Current ownership is the combined compiler method; final ownership is the framework semantic backend selected by immutable catalog identity. This lands and rolls back independently of template facts.

## Exact production population and APIs

- `crates/verter_session/src/parse.rs` — replace the sole production `.eval_source(...)` dispatch and preserve `carrier_eval_source_type`/snapshot behavior.
- `crates/verter_compiler/src/framework_common/carrier_compiler.rs` — delete the combined method and its trait-harness coverage only.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — delete the Vue combined-method implementation after route equivalence.
- `crates/verter_compiler/src/svelte/carrier.rs` — delete the Svelte combined-method implementation after route equivalence.
- Focused existing eval-source tests, including the Svelte conformance matrix, are evidence surfaces and do not enlarge the production-file budget.

The named boundary is source text plus immutable framework/parse identity to the backend-owned eval source and source kind. Parse publication, template facts, projection, runtime, assembly, and host routing are excluded.

## Exact predecessor contracts and binding laws

- **CCA1E:** the Vue `FrameworkSemanticAuthority` produces eval source from a registered parse artifact.
- **CCA1F:** the Svelte `FrameworkSemanticAuthority` produces eval source from a registered parse artifact.
- Lookup is once per request by framework/catalog epoch; no generic framework branch, second resolver, source reparse, or fallback to the combined trait is allowed.
- Eval source and its `FileLanguage`/source-kind classification remain bound to the same source revision and parse artifact. Cancelled, stale, or partial work publishes and warms nothing.

## Internal subblocks, migration, and deletions

1. Characterize Vue/Svelte fresh, preloaded, incremental, and refusal outcomes through existing tests.
2. Switch the production session dispatch atomically to the registered semantic backend.
3. Delete the combined trait method, both framework implementations, and method-only tests in the same candidate.

No shadow/dual read may survive the candidate. Delete no template-fact, IDE, runtime, host, registry, trait, option, or staged-artifact authority.

## Acceptance, performance, aborts, and verification

- **CCA1GE-AC1:** structural evidence finds no production `.eval_source(...)` combined-trait dispatch, declaration, or framework implementation.
- **CCA1GE-AC2:** Vue/Svelte eval bytes and source kind remain equivalent for fresh, preloaded, and incremental inputs; a planted hardcoded-framework selection fails.
- **CCA1GE-AC3:** stale/cancelled outputs cannot publish or warm, and edit-revert equals fresh.
- **CCA1GE-AC4:** one request performs one semantic-backend call with no duplicate parse, semantic pass, source copy, or retained candidate; absent/inapplicable work stays zero.

Ceiling: 600 production LOC, 5 production files, 2 crates. Abort on another production caller, semantic divergence, need for a second resolver, or any template-fact mutation. Run focused compiler/session eval-source and Svelte conformance evidence plus `targeted-domain`; CCA1G joins this result with CCA1GT.
