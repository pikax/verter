<!-- unified-charter-v2
id=CCA1O1
name=Typed FFI host compile-request schema
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1N
owner=compiler.compiler-bridge:protocol and FFI framework-discriminated host compile request
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
charter=charters/compiler-compiler-bridge/CCA1O1.md
max_production_loc=500
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O1 — Typed FFI host compile-request schema

## Independently acceptable outcome and rollback boundary

Add an unused framework-discriminated FFI host compile-request schema and exact conversion to the canonical compiler request alongside the legacy profile. Reverting removes only the unused protocol/FFI path.

## Concrete surfaces and APIs

- Surfaces: `crates/verter_protocol/src/types.rs` and `crates/verter_ffi/src/convert/input.rs` with focused conversion tests.
- Named boundary: `FfiHostCompileRequest::{Vue,Svelte}` carrying framework-owned options plus typed requested products; unknown/cross-framework fields fail closed.
- Conversion constructs exactly one canonical `CompileRequest` and preserves product, option, identity, and refusal semantics without string targets or defaults.
- NAPI, WASM, TypeScript packages, unplugin, playground, public route changes, and legacy deletion are excluded.

## Exact predecessor contract

- **CCA1N:** implemented ledger row for “Native host-integration route convergence join”; CCA1N4/CCA1N3 cut the runtime-render and host-backed lanes over to request-scoped bound framework host backends and removed both bundle selectors.

## Acceptance and evidence

- Exhaustive conversion fixtures cover both frameworks, every product kind, unknown fields, malformed values, and unsupported capability refusal.
- Structural evidence proves no production binding consumes the new schema yet and the old profile remains unchanged.

## Deletions, budgets, and aborts

- Delete nothing; forbid binding or consumer migration.
- Ceiling: 500 LOC, 5 files, 2 crates; rescope if a public binding or TypeScript consumer enters.
- Abort on silent defaults, cross-framework fields, or a second request authority.

## Verification and review

Use TDD for conversion/refusal boundaries, run protocol/FFI tests and `targeted-domain`. Apply `public-3`; add only CCA1O1's ledger row.
