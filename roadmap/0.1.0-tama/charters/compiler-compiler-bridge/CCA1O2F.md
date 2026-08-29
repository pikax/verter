<!-- unified-charter-v2
id=CCA1O2F
name=Native session contract-vocabulary migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2
owner=compiler.compiler-bridge:session-facing native host request vocabulary
conflict_domains=host_service_graph,public_protocol
resource_class=docs-light
review_profile=public-3
gate_profile=docs-domain
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
charter=charters/compiler-compiler-bridge/CCA1O2F.md
max_production_loc=80
max_production_files=2
max_related_packages=1
rescope_loc=200
rescope_files=3
rescope_unrelated_packages=2
-->

# CCA1O2F — Native session contract-vocabulary migration

## Independently acceptable outcome and rollback boundary

Update the bounded session-facing explanatory vocabulary from the native legacy profile name to CCA1O2's typed host-request contract without changing executable behavior. Reverting restores only comments and test explanation text while both request routes remain installed.

## Concrete surfaces and APIs

- Surfaces are exactly `crates/verter_session/src/host_compile.rs` and `crates/verter_session/src/runtime_render_lane_tests.rs`.
- Owns only references that explain how JavaScript/native request fields project into session compile options. Types, functions, assertions, fixtures, algorithms, defaults, and runtime-render behavior are excluded.
- The durable wording names the framework-discriminated host request and preserves the distinction between its general product options and the runtime-render profile.

## Exact predecessor contract

- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”.

## Acceptance and evidence

- The two named files contain no `HostCompileProfile` reference and their diff is comment/documentation-only.
- Rust formatting and existing session tests remain unchanged in behavior; no test name, assertion, or fixture gains roadmap vocabulary.

## Deletions, budgets, and aborts

- Delete only stale explanatory references to the retired native type name.
- Ceiling: 80 documentation LOC, 2 production/test files, 1 related crate; rescope above 200 LOC, 3 files, 2 unrelated packages, or if executable code changes.
- Abort on a required API/algorithm change or discovery of another independent documentation population.

## Verification and review

Use bounded diff inspection, targeted `rg`, Rust formatting, and `docs-domain`. Add only CCA1O2F's ledger row and apply `public-3`.
