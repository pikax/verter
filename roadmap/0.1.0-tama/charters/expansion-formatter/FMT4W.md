<!-- unified-charter-v2
id=FMT4W
name=Formatter WASM adapter contribution
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT4F
owner=expansion.formatter:WASM formatter adapter and export
conflict_domains=formatter_wasm_adapter
resource_class=rust-mixed
review_profile=architecture-3
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
charter=charters/expansion-formatter/FMT4W.md
max_production_loc=400
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4W — Formatter WASM adapter contribution

## Independently acceptable outcome and owner

Expose one versioned WASM formatter adapter over FMT4F's UTF-16 conversion and FMT4P's complete service result. The WASM surface is independently testable and usable without NAPI/MCP/LSP promotion. Reverting removes only the WASM export.

The sole owner is **WASM formatter adapter and export**.

## Surfaces and predecessor

- Exact production mutation surfaces: `crates/verter_wasm/src/formatter.rs`, bounded module/export integration in `crates/verter_wasm/src/lib.rs`, and `packages/wasm/src/index.ts` updates to both `WasmHostBinding` and the public `Host` facade. The consumed `crates/verter_ffi/src/formatter/**` API is unchanged.
- Owns `FormatWasmRequest`, `FormatWasmResult`, wasm-bindgen export registration, the TypeScript facade, error/cancellation translation, and no coordinate conversion beyond FMT4F.
- **FMT4F:** supplies the sole shared UTF-16 conversion and transitive versioned formatter protocol/service.

Internal testable subblocks are request validation, one shared service call, WASM result/error translation, export registration, and boundary/performance evidence. Identity includes protocol version, source revision, config provenance, and formatter-policy provenance; partial or mismatched results never export as success.

## Acceptance, deletion, and budgets

- **FMT4W-AC1:** WASM full/range/cursor requests and output/edits/cursor/errors agree with the private service/public Rust DTO as applicable and exact FMT4F UTF-16 fixtures.
- **FMT4W-AC2:** unsupported/cancelled/partial results never become successful JavaScript objects.
- **FMT4W-AC3:** one call reaches the shared formatter once; no parse/render/map or conversion fork exists.
- **FMT4W-AC4:** FMT0 adapter overhead, retained memory, cancellation, and zero-work limits hold.
- Delete no shared route, legacy LSP code, FFI conversion, or other adapter.
- Ceiling: 400 LOC, 4 files, 2 related packages (`verter_wasm`, `@verter/wasm`). Abort if NAPI/MCP/LSP policy, shared conversion changes, or a third package enters.
- Verify host and `wasm32-unknown-unknown` binding checks, `@verter/wasm` facade/type checks, exact export inventory, and `targeted-domain`. Add only FMT4W's ledger row; FMT4 later promotes cross-surface maturity.
