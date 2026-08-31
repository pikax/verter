<!-- unified-charter-v2
id=CCA1O2H
name=NAPI own-property closedness repair
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=repair
semantic_role=delivery
class=compiler
predecessors=CCA1O2
owner=compiler.compiler-bridge:native host-request own-property materialization
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
charter=charters/compiler-compiler-bridge/CCA1O2H.md
max_production_loc=200
max_production_files=2
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O2H — NAPI own-property closedness repair

## Independently acceptable outcome and rollback boundary

The durable problem: the native binding materializes a JS request through the generic `serde_json::Value::from_napi_value`, which drops an own property whose value is `undefined` before serde sees it. `{ framework: "vue", …, runes: undefined }` therefore decodes clean instead of refusing the cross-framework key, while the browser binding materializes every own key and refuses the same payload. The two bindings disagree on a fail-closed rule that is qualified by neither “unknown” nor “cross-framework”, and the published native declaration's claim that every request object is closed is false for that whole class of input.

Outcome: the native binding refuses an own unknown or cross-framework key regardless of its value, so both bindings observe the same rule. Reverting restores the generic materialization and the documented gap.

## Concrete surfaces and APIs

- `crates/verter_napi/src/host_compile_request.rs`: replace the generic value materialization with a recursive NAPI object materializer that enumerates every own enumerable string key and represents an `undefined`-valued key as JSON `null`. The module's own explanation of the surviving gap goes with it.
- Focused NAPI boundary tests registered in the existing `crates/verter_napi/tests/` harness, driving real JS values rather than Rust fixtures. A Rust model of a JS graph is supporting evidence only; the acceptance evidence runs through a non-shipping fixture Node addon that puts a live V8 object graph in front of the real `FromNapiValue`. The fixture reaches neither the shipped addon nor `@verter/native`'s published declarations.
- The materializer enumerates OWN enumerable string keys only. This is a deliberate NARROWING of the replaced conversion, which read properties through `napi_get_property_names` and therefore walked the prototype chain: an inherited enumerable key used to reach the schema and be refused, and no longer does. Own keys are what the browser binding's `Object.entries` enumerates and what a caller wrote.
- Rules the materializer must preserve: a known optional property carrying `undefined` still decodes as absent-equivalent; an unknown or cross-framework key stays present so `deny_unknown_fields` refuses it whatever its value; there is no binding-local allowed-key list, because serde remains the sole closed-shape authority.
- The materializer additionally refuses two graphs JSON cannot represent, because a JS caller can produce both cheaply and either kills the process unbounded: a value nested past a fixed depth, which is also how a graph referring back to itself is refused; and an array whose DECLARED length exceeds a fixed element budget, checked before any capacity is reserved.
- The FFI and protocol schemas, the browser binding, the legacy profile route, published TypeScript declarations, and product/option semantics are excluded.

## Exact predecessor contract

- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”; the tagged native request, its decode function, and its conversion to the FFI schema exist at this boundary.

## Acceptance and evidence

- An unknown key whose value is `undefined` is refused at the top level, inside a framework options object, and inside a requested product.
- A cross-framework option key carrying `undefined` is refused in both framework arms.
- A known optional property set to `undefined` still decodes as absent-equivalent and compiles exactly as when the key is omitted, so an optional slot reads `null` as absent.
- No allowed-key list exists in the binding; every refusal is serde's own. Unknown-field, unknown-tag and missing-field refusals name the offending field. A slot given the wrong KIND of value does not, in either direction: `{ ssr: 5 }` and `{ ssr: undefined }` both report `invalid type: …, expected a boolean`. This division is serde's own and predates the repair; the repair moves a stated `undefined` out of the missing-field class into it, which is what treating the key as present requires — the outermost `#[serde(tag = "framework")]` buffers the payload into serde's private content representation before the variant is deserialized, so no deserializer-side path tracker survives it. Naming that class means changing how the framework tag is dispatched, which is a change to this decode's shape and is not this node's.
- An inherited enumerable key is ignored rather than refused: `Object.create({ runes: true })` carrying an otherwise valid Vue body is ACCEPTED, where the replaced conversion refused it. This prototype-chain narrowing is the SOLE ratified exception to the abort on changing which payloads are accepted outside the `undefined` class; that abort otherwise still applies in full, scoped to the own-property graph. Required evidence is a real-JS positive control asserting the inherited-key payload is accepted and decodes identically to the payload without the prototype.
- The native and browser bindings accept and refuse the same payloads for the `undefined` class. The claim is scoped to that class and is not a general convergence: JS values the request schema cannot carry reach the two schemas by different routes and are not converged here.
- Materialization preserves value shape for every JS type the request schema uses — objects, arrays, strings, numbers, booleans, and `null` — and a payload containing no `undefined` decodes to the same request as today.
- Evidence is TDD boundary tests for top-level and nested unknown and cross-framework keys carrying `undefined`, a known optional-`undefined` control that both decodes and compiles as the omitting payload does, the inherited-key positive control, and refusals for a declared-oversize array and a self-referential graph. The acceptance set runs through the fixture addon over real JS objects, and restoring the generic materialization must turn it red.

## Deletions, budgets, and aborts

- Delete the generic materialization call and the source text describing the gap it caused. Delete no schema, no converter, and no legacy route; no second decode path may survive beside the new one.
- Planning guidance: roughly 200 LOC across 2 files in 1 crate. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when the browser binding or another consumer enters.
- Abort on a binding-local key allowlist, on a second decode path, on a refusal that stops naming the offending field WITHIN the classes serde names (unknown field, unknown tag, missing field), on a change to which payloads are accepted outside the `undefined` class, or on decode cost beyond one traversal of the supplied object graph.
- The prototype-chain narrowing recorded above is the SOLE ratified exception to that acceptance abort. It is scoped to inherited enumerable keys; any other change to which own-property payloads are accepted still aborts. Refusing a graph with no JSON representation — unbounded nesting, a self-reference, an array declaring more elements than the budget — is not an acceptance change: those payloads previously killed the process rather than being accepted.

## Verification and review

Use TDD at the NAPI boundary with real JS values, run the NAPI and native package suites and `targeted-domain`. Apply `public-3`; add only CCA1O2H's ledger row.
