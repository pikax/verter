<!-- unified-charter-v2
id=CCA1N3A
name=Bound-arm host-backed framework execution inputs
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=repair
semantic_role=delivery
class=compiler
predecessors=CCA1N3
owner=compiler.compiler-bridge:host-backed framework execution-input preparation and its refusal placement
conflict_domains=compiler_execution
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=high
review_effort_default=high
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1N3A.md
max_production_loc=200
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N3A — Bound-arm host-backed framework execution inputs

## Independently acceptable outcome and owners

On the host-backed compile route, framework-specific execution-input preparation and the refusal it can raise run in generic route orchestration ABOVE the bound dispatch: the route observes the bound request only to decide whether to run one framework's cross-file semantic producer, drives that framework's dependency synchronization from the result, refuses the whole compile on that framework's dependency diagnostics before any backend is called, and then builds that framework's execution-input carrier for every carrier — including carriers that ignore it and can never reach the refusal. The generic path therefore carries framework semantics, and one framework's arm is unreachable from another framework's request even though both are supposed to be peers of the same dispatch.

After this node, that work is arm-local: the bound dispatch happens first, and each bound framework arm prepares the execution inputs its own backend consumes, restates the compiled file's dependency/semantic axis with the transitive dependencies its own preparation observed, and refuses on its own dependency diagnostics. A carrier cannot reach another framework's producer, refusal, or execution-input carrier, because the generic route has no framework execution-input value to construct or hand over. Current ownership is the generic route; final ownership is the bound framework arm. Reverting restores the pre-dispatch preparation on the generic route only.

## Exact production population and boundary

- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` — the host-backed route retains its generic preconditions (carrier-artifact presence, registered-grammar validity), its product-demand booleans, and its publication tail; it constructs and refuses on no framework execution input.
- `crates/verter_session/src/host_resolve/compile_request_build.rs` — the bound execution dispatch gains arm-local execution-input preparation: the framework producer, the dependency-axis restatement, the dependency-diagnostic refusal, and the execution-input carrier live inside the arm that consumes them.
- `crates/verter_session/src/host_resolve/mod.rs` — the module-layout note that names what the bound execution owns, corrected to include arm-local execution-input preparation.
- The bound execution's signature no longer accepts a framework execution-input value, so the generic route cannot supply one to any arm. That absence is the structural boundary, not a comment.
- The one backend call per request, the demand constructors, the typed refusal codes, the admission/execution pairing, the assembled payloads, and both frameworks' byte-parity oracles are unchanged.
- Excluded: the runtime-render lane (already arm-local), the outer stale/last-known-good publication policy, main-module payload assembly, request DTOs, and any change to which cross-file semantic work a framework performs.

## Exact predecessor contract

- **CCA1N3:** the host-backed route already consumes the request-scoped bound host request and executes through the registered framework host backend, so an arm-local dispatch exists to move this preparation into; this node corrects only where that preparation sits relative to the dispatch.

## Invariants and acceptance

- The bound dispatch occurs before any framework execution-input preparation; the generic route holds no framework producer call, no framework refusal, and no framework execution-input construction.
- Every bound framework arm restates the compiled file's dependency/semantic axis exactly once per host-backed compile, and does so BEFORE its own dependency refusal, so a compile refused for an unresolvable cross-file type still records the dependencies whose repair must invalidate it. The axis is replaced, not merged, so an arm whose preparation observes no cross-file dependency still restates it with that empty contribution.
- A request refused on a generic precondition reaches no framework preparation at all and therefore leaves the dependency axis unwritten. This is the one characterized behavior change: previously such a refusal ran the cross-file producer and restated the axis. Skipping a restatement is safe here only because the bound host request exists exactly when the carrier artifact does, so the skipped path is reachable only with no artifact, where the previous behavior restated the axis with an empty contribution; retention is therefore a superset of what was written and can only add reverse-dependency edges. Skipping a restatement is NOT conservative in general — a skipped write that would have added an edge is under-invalidation — so any future precondition that can refuse after a producer has run must restate the axis rather than rely on this reasoning. No product is published on either side of the change.
- Exactly one binding and one backend execution remain per request; refusal atomicity, maps, publication, and fresh/warm/incremental equivalence are unchanged. Diagnostic content and ordering are unchanged per refusal kind; because the framework dependency refusal now follows the generic grammar precondition, a carrier that both carries an unserviceable grammar profile and has unresolvable cross-file types reports the grammar refusal rather than the dependency refusal.

## Deletions, budget, and verification

Delete the pre-dispatch framework preparation, its refusal, and the framework execution-input parameter in place; no shim, second path, or flag remains. Ceiling: 200 production LOC, 3 production files, 1 crate; abort if a fourth production surface, a demand/refusal-code change, or a runtime-render change enters. Evidence: a discriminating test that a generic-precondition refusal performs no dependency synchronization (failing before this change), a per-arm rail that every bound arm synchronizes exactly once, the existing dependency-axis and route-guard suites, both frameworks' product-surface and conformance oracles, and `targeted-domain`. The native host-integration convergence join consumes the corrected ownership shape.
