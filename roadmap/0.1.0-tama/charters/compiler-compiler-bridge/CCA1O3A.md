<!-- unified-charter-v2
id=CCA1O3A
name=WASM fixture-tool host-request migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O3,CCA1O1A,CCA1O3C,CCA1O3D,CCA1O3E
owner=compiler.compiler-bridge:direct WASM carrier-fixture capture typed host requests
conflict_domains=compiler_execution,host_service_graph,public_protocol
resource_class=ts-heavy
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
charter=charters/compiler-compiler-bridge/CCA1O3A.md
max_production_loc=200
max_production_files=1
max_related_packages=1
rescope_loc=500
rescope_files=3
rescope_unrelated_packages=2
-->

# CCA1O3A — WASM fixture-tool host-request migration

## Independently acceptable outcome and rollback boundary

Move the direct WASM carrier-fixture capture tool to CCA1O3's typed request while the binding-local legacy profile remains available. Reverting changes only the capture tool; the typed adapter and local compatibility DTO remain installed.

## Concrete surfaces and APIs

- The sole production/tooling surface is `packages/playground/scripts/capture-wasm-carrier-fixtures.mjs`.
- Owns the direct `wasm-bindgen` calls that produce the committed IDE/public-API fixture: a source-only `upsert` that states no compile demand, followed by one typed `compileRequest` in place of the `ensureIdeCompiled`/`getIde` pair.
- The fixture sources, canonical IDs, source order, output bytes/maps, SFC-absolute span meaning, JavaScript UTF-16 offsets, and committed fixture schema remain unchanged. Equivalent output does not require golden regeneration.

## Exact predecessor contract

- **CCA1O3:** implemented ledger row for “WASM typed host-request adapter”.
- **CCA1O1A:** implemented ledger row for “Canonical Svelte custom-element prop-type admission”; the Svelte custom-element prop-type slot has its final shape, so no request this tool builds encodes a superseded closed vocabulary.
- **CCA1O3C:** implemented ledger row for “Execution-proven WASM JS-boundary gate”; the browser boundary refusals this consumer relies on are proven by execution rather than by compilation alone.
- **CCA1O3D:** implemented ledger row for “WASM typed host-request callable route”; the browser host object exposes one callable typed compile entry on its generated JavaScript surface, so this consumer has a reachable typed route to move onto.
- **CCA1O3E:** implemented ledger row for “Live-WASM carrier-fixture freshness rail”; the committed carrier fixture is regenerated from the current browser artifact and checked against it, so this migration’s byte-equivalence evidence is read against a current baseline.

## Acceptance and evidence

- The script contains no legacy WASM profile call. Each fixture registers its source once and then issues exactly one typed Vue IDE `compileRequest` — no separate ensure, no cached read, no extra WASM call, and no source copied into the request.
- `node --check`, WASM conversion fixtures, and the existing fixture freshness/consumer guards prove shape and output equivalence.

## Deletions, budgets, and aborts

- Delete no compatibility type, binding decode, capture case, fixture field, or committed output.
- Ceiling: 200 production/tooling LOC, 1 production/tooling file, 1 related package; rescope above 500 LOC, 3 files, 2 unrelated packages, or if playground runtime routing enters.
- Abort on fixture-byte drift, changed coordinate encoding, duplicate WASM execution, or a required golden rewrite unrelated to request shape.

## Verification and review

Run `node --check`, WASM request-conversion tests, fixture freshness/consumer guards, and `targeted-domain`. Add only CCA1O3A's ledger row and apply `public-3`.
