<!-- unified-charter-v2
id=CCA1O3D
name=WASM typed host-request callable route
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1O1B,CCA1O3,CCA1O3C
owner=compiler.compiler-bridge:browser callable typed host compile-request route
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
charter=charters/compiler-compiler-bridge/CCA1O3D.md
max_production_loc=450
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O3D — WASM typed host-request callable route

## Independently acceptable outcome and rollback boundary

The durable problem: the browser binding's typed host-request decoders are plain Rust functions with no binding annotation. They are reachable from Rust tests and from nothing else — the generated JavaScript surface exports no typed compile entry, and the package wrapper declares only the legacy profile-bearing pair. The typed request therefore has no JavaScript caller, and its second parameter is a Rust reference that has no browser-binding representation at all, so it could not be exported as written. Every browser consumer told to move onto the typed request is blocked on an entry point that does not exist, and the existing decode tests prove decoding inside Rust rather than callability from JavaScript.

Outcome: the browser host object exposes one callable typed compile entry on the generated JavaScript surface, and the package wrapper declares and forwards it. A JavaScript caller registers a source once and then executes one typed request against it in a single call. Reverting removes the exported route and its wrapper declaration; the decode boundary, the legacy profile route, and every consumer stay as they are.

## Concrete surfaces and APIs

- `crates/verter_wasm/src/lib.rs`: a bound method on the exported browser host object, named `compileRequest` on the JavaScript side. The annotation belongs on that host method, never on the existing Rust decode helper, whose signature has no binding representation.
- The method takes the canonical id and the typed request as separate arguments, decodes the request exactly once through the existing decode boundary, converts it once to the canonical request, and hands it to the session's typed execution entry. It must not convert the request back into a compile profile.
- `packages/wasm/src/index.ts`: the binding-function type, its entry in the host binding interface, and the wrapper method that forwards it, alongside the untouched legacy declarations.
- `docs/api/wasm.md`: the typed route's documentation.
- Inspection of the generated browser surface, proving the method is present on the generated declarations and on the generated JavaScript object rather than only in Rust.
- Invocation tests driven from real JavaScript values through the generated entry, registered in the existing browser boundary module that the canonical gate lane executes.
- The response is the session's typed envelope, serialised for JavaScript with the existing canonical id, diagnostic, output, source-map, and offset meaning. A refusal throws; it never returns a partial result, a null, or an ensure boolean.
- Consumer migration, the legacy profile decoders and their deletion, and the native binding are excluded. The legacy profile pair remains fully intact here and is deleted only by its own declared node.

## Exact predecessor contract

- **CCA1O1B:** implemented ledger row for “Canonical typed host-request execution seam”; the session executes a caller-supplied canonical request and returns the typed envelope, so this binding has something to call and never has to rebuild a profile.
- **CCA1O3:** implemented ledger row for “WASM typed host-request adapter”; the browser-local request decode boundary and the exact conversion to the shared schema exist at this surface.
- **CCA1O3C:** implemented ledger row for “Execution-proven WASM JS-boundary gate”; the canonical gate executes this surface's JavaScript-boundary tests, so the new route's invocation evidence runs rather than merely compiles.

## Acceptance and evidence

- The generated browser surface exports the typed compile method on the host object: it is present in the generated declarations and callable on the generated object, and a test invokes it from JavaScript rather than from Rust.
- A JavaScript caller performs one source-only registration and then one typed call, and receives the requested products for both Vue and Svelte carriers; the run makes no additional compile call and copies no source into the request.
- Output bytes, source maps, diagnostics, canonical ids, and JavaScript UTF-16 offsets are equivalent to the legacy route for the same demand.
- An unknown or cross-framework property is refused at the JavaScript boundary on this route exactly as on the existing decode boundary, and the refusal names the offending property where the schema names it.
- A refusal throws rather than returning a partial result, a null, or a boolean, and no path on this route constructs a compile profile from the typed request.
- The legacy profile-bearing methods keep their existing declarations and behavior, and their tests still pass unchanged.

## Deletions, budgets, and aborts

- Delete nothing. The legacy profile decoders and wrapper methods on this surface belong to a later deletion node and must survive this change intact.
- Planning guidance: roughly 450 LOC across 4 files in 2 related crates/packages. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when a consumer migration or the native binding enters.
- Abort on annotating the existing Rust decode helper instead of the host method, on converting the typed request back into a profile, on a second decode path, on evidence that proves callability only from Rust, or on any legacy deletion.

## Verification and review

Add the invocation tests first, inspect the generated surface, run the browser binding and package suites, the canonical gate's JavaScript-boundary lane, and `targeted-domain`. Apply `public-3`; add only CCA1O3D's ledger row.
