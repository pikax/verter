<!-- unified-charter-v2
id=CCA1O3C
name=Execution-proven WASM JS-boundary gate
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=repair
semantic_role=delivery
class=compiler
predecessors=CCA1O3
owner=compiler.compiler-bridge:canonical gate lane executing wasm-target JavaScript-boundary tests
conflict_domains=compiler_execution,host_service_graph,public_protocol
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
charter=charters/compiler-compiler-bridge/CCA1O3C.md
max_production_loc=350
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O3C — Execution-proven WASM JS-boundary gate

## Independently acceptable outcome and rollback boundary

The durable problem: the browser binding's real-JS boundary tests compile but never run. They exist because a deserializer is free to visit only the fields a schema declares and thereby never reach the closed-shape refusal at all, so an unknown or cross-framework key must be proven refused where a browser caller meets it. The canonical gate executes no wasm-target test, and continuous integration only builds and lints `wasm32-unknown-unknown`. A regression at that boundary passes every required check, and the tests disclose the gap in their own source rather than failing.

Outcome: one execution-proven lane inside the canonical gate runs the workspace's wasm-target boundary tests on every real gate invocation, so those refusals are proven by execution and a missing prerequisite is a loud failure. Reverting removes the lane and its provisioning; the tests remain, unexecuted, as they are today.

## Concrete surfaces and APIs

- `scripts/gate.mjs` and `scripts/gate-internals.mjs`: the lane, its discovery, its share of the run's resource ceilings, and its terminal result. The lane belongs to the canonical gate; it is not a separate continuous-integration-only job and not a second primary test authority.
- `.github/workflows/ci.yml`: provisions `wasm32-unknown-unknown` and the pinned `wasm-bindgen-test-runner` for the job that already invokes the canonical gate exhaustively. The pin tracks the workspace's `wasm-bindgen` dependency version and must equal it — the runner and the library are one ABI, so a version skew fails at run time inside the generated glue rather than at compile time, which is exactly the quiet failure this lane exists to remove. The existing wasm build job must not acquire duplicate ownership of the lane.
- `crates/verter_wasm/src/host_compile_request_tests.rs`: remove the source text stating that the workspace gate does not execute wasm-target tests.
- Required on every real gate invocation, bare and exhaustive, and not path-filtered inside the gate. The prepare-only invocation is not a gate run.
- Failure conditions: a missing target, a missing or unpinned runner, zero discovered wasm-target boundary cases, a skipped lane, or an absent terminal result.
- Discovery is tree- or tool-derived. A hand-maintained list of the currently existing filenames is forbidden.
- Rewriting the boundary tests themselves, adding new binding behavior, and migrating consumers are excluded, with one recorded exception: the browser binding's closed-shape refusal must also be proven for an own key stated as `undefined`. No `serde_json::Value` fixture can express that value — such a key is simply absent after the round trip — so the existing boundary fixtures cannot state the case at all, and the claim that both bindings converge on it is currently held by reading the source. This node adds that one case to the existing boundary module, built against a real JavaScript object graph rather than a serialised fixture, because it owns both that file and the lane that would execute it. The exception is exactly this: it is not a licence to add binding behaviour, to restate per-option conversion, or to widen the module further.

## Exact predecessor contract

- **CCA1O3:** implemented ledger row for “WASM typed host-request adapter”; the typed browser request, its JavaScript entry point, and the boundary tests the lane executes exist.

## Acceptance and evidence

- A bare gate run and an exhaustive gate run both execute the lane and report a terminal per-lane result; neither can report success while the lane was skipped or produced no result.
- The lane reports a non-zero discovered case count derived from the tree, and zero discovered cases is a failure.
- A newly added wasm-target boundary test in a file the lane has never seen is executed without editing any list.
- The lane is proven to discriminate: a deliberate weakening of a refusal at the JavaScript boundary reddens the run, and the mutation is shown to be present, unique, and new before that red run is trusted.
- A missing target or runner fails loudly naming the exact missing prerequisite, rather than degrading to a skip or a silent pass.
- The run's memory and parallelism ceilings still bound the whole invocation with the lane present.
- Continuous integration provisions the target and the pinned runner for the existing canonical-gate job without duplicating lane ownership.

## Deletions, budgets, and aborts

- Delete only the stale in-source statement about gate coverage. Delete no test, no job, and no existing lane.
- Planning guidance: roughly 350 infrastructure and test LOC across 4 files touching 2 related surfaces. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when the lane is asked to own a second test authority.
- Abort on a lane that can be skipped silently, on a filename list, on path-filtering inside the gate, on a second primary test authority, or on moving the lane out of the canonical gate.

## Verification and review

Run the canonical gate bare and exhaustively on a clean tree and again against a planted boundary mutation, and run `targeted-domain`. Apply `public-3`; add only CCA1O3C's ledger row.
