<!-- unified-charter-v2
id=CCA1O2C
name=Native SSR casing and attribute tool migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2
owner=compiler.compiler-bridge:SSR casing input and attribute diagnostic script requests
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
charter=charters/compiler-compiler-bridge/CCA1O2C.md
max_production_loc=350
max_production_files=7
max_related_packages=0
rescope_loc=700
rescope_files=9
rescope_unrelated_packages=1
-->

# CCA1O2C — Native SSR casing and attribute tool migration

## Independently acceptable outcome and rollback boundary

Move the bounded SSR casing/input/attribute diagnostic-script population to CCA1O2's typed native request while the legacy profile remains available. Reverting restores only these seven scripts' request construction.

## Concrete surfaces and APIs

- Tooling surfaces are exactly `scripts/ssr-baseline/_test-casing.mjs`, `scripts/ssr-baseline/_test-casing2.mjs`, `scripts/ssr-baseline/_test-casing3.mjs`, `scripts/ssr-baseline/_test-input-value.mjs`, `scripts/ssr-baseline/_test-vbind-attrs.mjs`, `scripts/ssr-baseline/_test-vbind-attrs2.mjs`, and `scripts/ssr-baseline/_test-vbind-attrs3.mjs`.
- Owns only the Vue SSR `getVirtualFile` request shape in those files. Fixture discovery, official Vue compilation, printed diagnostics, and normalization remain untouched.
- Each script preserves filename, SSR, JavaScript coercion, source-map, and requested-main-product intent exactly.

## Exact predecessor contract

- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”.

## Acceptance and evidence

- No named script contains a legacy `compileProfile` request; each contains one typed Vue runtime-server/main request and makes no additional native call.
- `node --check` covers every script. External fixture execution is optional supplemental evidence and is never required by the hermetic default gate.

## Deletions, budgets, and aborts

- Delete no script, fixture, binding type, converter, or comparison output.
- Ceiling: 350 production/tooling LOC, 7 tooling files, 0 related packages; rescope above 700 LOC, 9 files, 1 unrelated package, or if helper/model scripts enter.
- Abort on corpus access changes, output normalization changes, duplicate native execution, or option drift.

## Verification and review

Run `node --check` over the exact seven-file list, applicable native request-conversion fixtures, and `targeted-domain`. Add only CCA1O2C's ledger row and apply `public-3`.
