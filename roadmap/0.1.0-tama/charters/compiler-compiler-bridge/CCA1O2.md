<!-- unified-charter-v2
id=CCA1O2
name=NAPI typed host-request adapter
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1O1
owner=compiler.compiler-bridge:NAPI and native-package typed host request adapter
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
charter=charters/compiler-compiler-bridge/CCA1O2.md
max_production_loc=600
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O2 — NAPI typed host-request adapter

## Independently acceptable outcome and rollback boundary

Add the typed NAPI/native-package request adapter beside the legacy profile without moving unplugin or other production consumers. Reverting removes only the unused adapter and TypeScript type export.

## Concrete surfaces and APIs

- Surfaces: `crates/verter_napi/src/lib.rs`, `packages/native/host-types.ts`, `packages/native/index.ts`, and focused NAPI/native tests.
- Owns `NapiHostCompileRequest` plus the native TypeScript `HostCompileRequest` discriminated union and exact conversion to CCA1O1's FFI schema.
- Both framework variants reject unknown/cross-framework options; the legacy `NapiCompileProfile`/`HostCompileProfile` route remains unchanged until CCA1O4D after every independent native consumer migration.
- WASM, unplugin, playground, session internals, and shared FFI deletion are excluded.

## Exact predecessor contract

- **CCA1O1:** implemented ledger row for “Typed FFI host compile-request schema”.

## Acceptance and evidence

- NAPI/TypeScript shape and conversion fixtures preserve framework/product/options and fail closed on malformed or foreign fields.
- Structural evidence proves no production unplugin call moved and no legacy profile was deleted.

## Deletions, budgets, and aborts

- Delete nothing; forbid unplugin migration and WASM work.
- Ceiling: 600 LOC, 6 files, 2 related crates/packages; rescope if another consumer or binding enters.
- Abort on JS/Rust shape drift, hidden defaults, or duplicate compile execution.

## Verification and review

Use TDD at NAPI conversion boundaries, run NAPI/native suites and `targeted-domain`. Apply `public-3`; add only CCA1O2's ledger row.
