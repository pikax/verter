<!-- unified-charter-v2
id=FMT4N
name=Formatter NAPI adapter contribution
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT4F
owner=expansion.formatter:NAPI formatter adapter and export
conflict_domains=formatter_napi_adapter
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
charter=charters/expansion-formatter/FMT4N.md
max_production_loc=400
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4N — Formatter NAPI adapter contribution

## Independently acceptable outcome and owner

Expose one versioned NAPI formatter adapter over FMT4F's UTF-16 conversion and FMT4P's complete service result. The NAPI surface is independently testable and usable without WASM/MCP/LSP promotion. Reverting removes only the NAPI export.

The sole owner is **NAPI formatter adapter and export**.

## Surfaces and predecessor

- Exact production mutation surfaces: `crates/verter_napi/src/formatter.rs`, bounded module/export integration in `crates/verter_napi/src/lib.rs`, and the manually maintained `VerterHost` formatter declarations in `packages/native/index.ts`. The consumed `crates/verter_ffi/src/formatter/**` API is unchanged.
- `packages/native/index.js` is inspected and remains unchanged: the formatter is a `VerterHost` prototype method whose request references an already-upserted canonical source/revision, so it has no source-byte payload requiring this wrapper's string-to-`Buffer` coercion and no free-function export wiring. A design that adds either requirement aborts for an inventory/budget amendment before mutation.
- Owns `FormatNapiRequest`, `FormatNapiResult`, Rust export registration, `@verter/native` declarations, error/cancellation translation, and no coordinate conversion beyond FMT4F.
- **FMT4F:** supplies the sole shared UTF-16 conversion and transitive versioned formatter protocol/service.

Internal testable subblocks are request validation, one shared service call, NAPI result/error translation, export registration, and boundary/performance evidence. Identity includes protocol version, source revision, config provenance, and formatter-policy provenance; partial or mismatched results never export as success.

## Acceptance, deletion, and budgets

- **FMT4N-AC1:** NAPI full/range/cursor requests and output/edits/cursor/errors agree with the private service/public Rust DTO as applicable and exact FMT4F UTF-16 fixtures.
- **FMT4N-AC2:** unsupported/cancelled/partial results never become successful JavaScript objects.
- **FMT4N-AC3:** one call reaches the shared formatter once; no parse/render/map or conversion fork exists.
- **FMT4N-AC4:** FMT0 adapter overhead, retained memory, cancellation, and zero-work limits hold.
- Delete no shared route, legacy LSP code, FFI conversion, or other adapter.
- Ceiling: 400 LOC, 4 files, 2 related packages (`verter_napi`, `@verter/native`). Abort if WASM/MCP/LSP policy, shared conversion changes, an unbudgeted `packages/native/index.js` wrapper, or a third package enters.
- Verify NAPI binding tests, `@verter/native` declaration/type checks, exact export inventory, and `targeted-domain`. Add only FMT4N's ledger row; FMT4 later promotes cross-surface maturity.
