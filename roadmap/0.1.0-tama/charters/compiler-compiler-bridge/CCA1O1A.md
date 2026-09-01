<!-- unified-charter-v2
id=CCA1O1A
name=Canonical Svelte custom-element prop-type admission
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=repair
semantic_role=delivery
class=compiler
predecessors=CCA1O1
owner=compiler.compiler-bridge:single canonical Svelte custom-element prop-type admission authority
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
charter=charters/compiler-compiler-bridge/CCA1O1A.md
max_production_loc=300
max_production_files=4
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O1A — Canonical Svelte custom-element prop-type admission

## Independently acceptable outcome and rollback boundary

The durable problem: the Svelte custom-element prop-type vocabulary is decided in three places. The wire schema declares its own closed five-value enum and refuses a sixth spelling at decode; the FFI converter carries a second exhaustive spelling map; and the compiler's Svelte execution path repeats the same five-string allowlist. The canonical descriptor itself carries `Option<String>` and `SvelteOptionAttempt::into_request` admits it unchecked, so an invalid prop type rides inside an admitted request until emission. Adding a sixth canonical spelling therefore does not produce the compile error the wire comment claims: the external bindings stay tighter than the canonical request until their own list changes, and the refusal fires at a different authority and a different stage depending on the entry point.

Outcome: one admission authority, at canonical request construction. `into_request` converts the attempted spelling into a closed `SvelteCustomElementPropType`, the admitted `SvelteCompileRequest` carries that enum, and an invalid admitted request is unrepresentable. Transports forward the caller's string and own no membership. Reverting restores the wire enum, the converter spelling map, and the execution-side allowlist.

## Concrete surfaces and APIs

- `crates/verter_compiler/src/compile_request/svelte.rs`: the attempted descriptor keeps `Option<String>`; `into_request` admits exactly ten spellings — `string`, `boolean`, `number`, `array`, `object` and their capitalised forms `String`, `Boolean`, `Number`, `Array`, `Object` — into `SvelteCustomElementPropType`. Each lowercase/capitalised pair maps to the same variant. Every other casing, including `STRING` and `nUmBeR`, is refused; there is no general case-normalisation rule, only this closed ten-spelling set. An owner-local method such as `SvelteCustomElementPropType::as_svelte_name()` always renders the capitalised Svelte backend spelling.
- `crates/verter_compiler/src/standalone.rs`: `normalize_svelte_custom_element_descriptor` performs no membership check and cannot emit a prop-type refusal; it only renders the canonical enum.
- `crates/verter_protocol/src/types.rs`: `FfiSvelteCustomElementProp.prop_type` becomes `Option<String>`; the closed wire enum is removed.
- `crates/verter_ffi/src/convert/input.rs`: the conversion forwards the string unchanged.
- Focused conversion, admission, and refusal tests in those crates.
- Vue options, the remaining Svelte option vocabularies, the native and browser adapters, published TypeScript declarations, product routing, and legacy profile deletion are excluded.

## Exact predecessor contract

- **CCA1O1:** implemented ledger row for “Typed FFI host compile-request schema”; the framework-discriminated FFI request and its exact converter exist and carry the Svelte custom-element descriptor through to the canonical request.

## Acceptance and evidence

- Exactly one membership decision over the prop-type vocabulary exists in the workspace, and it is the canonical request constructor's. Neither the transport nor the execution path can refuse a prop type.
- A `SvelteCompileRequest` value cannot be constructed carrying an unadmitted prop type, so no later stage can observe one.
- An unrecognised spelling is refused as a malformed custom-element props-type option carrying the offending value, from direct canonical construction and from every transport route. The refusal identity and offending value are preserved; only the stage moves, from decode or emission to request construction.
- The admitted set is exactly those ten spellings, and every other casing is refused. Neither existing public entry narrows: the transport route additionally accepts the five capitalised forms, the direct canonical route additionally accepts the five lowercase forms, and the aggregate public vocabulary does not widen because it is the union the two entries already accept between them.
- The rendered Svelte backend spelling is byte-identical for every admitted spelling, so custom-element output and emitted prop metadata are unchanged.
- Adding a sixth canonical prop type — both of its spellings — requires editing exactly one match.
- Evidence is TDD admission/refusal fixtures across both entry paths plus the existing Svelte runtime-backend custom-element output tests, unchanged.

## Deletions, budgets, and aborts

- Delete the wire prop-type enum, the FFI spelling map, and the execution-side string allowlist. Delete no legacy profile route and no other option vocabulary.
- Planning guidance: roughly 300 production LOC across 4 files in 3 related crates. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when another binding or option vocabulary enters.
- Abort on a second membership check surviving anywhere, on a request type that can still carry an unadmitted spelling, on a changed backend spelling, or on an admitted set that is not exactly those ten spellings — that is, on either public entry narrowing, on the aggregate public vocabulary widening past the union those entries already accept, or on a general case-normalisation rule admitting any further casing.

## Verification and review

Use TDD at the admission boundary, run the compiler, protocol, and FFI suites and `targeted-domain`. Apply `public-3`; add only CCA1O1A's ledger row.
