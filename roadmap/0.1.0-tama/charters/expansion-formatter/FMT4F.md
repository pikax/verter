<!-- unified-charter-v2
id=FMT4F
name=Formatter FFI UTF-16 conversion foundation
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT4P
owner=expansion.formatter:strict bidirectional NAPI/WASM formatter UTF-16 conversion
conflict_domains=formatter_ffi_adapter
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
charter=charters/expansion-formatter/FMT4F.md
max_production_loc=450
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4F — Formatter FFI UTF-16 conversion foundation

## Independently acceptable outcome and owner

Land one shared, dormant FFI conversion layer that strictly converts NAPI/WASM UTF-16 formatter requests into FMT4P/private source geometry and converts complete results back to UTF-16. Reverting removes only this conversion layer.

The sole owner is **strict bidirectional NAPI/WASM formatter UTF-16 conversion**.

## Surfaces, predecessor, and laws

- Exact production mutation surfaces: `crates/verter_ffi/src/lib.rs` for formatter-module registration and `crates/verter_ffi/src/formatter/**`; `crates/verter_protocol/src/formatter/**` and `crates/verter_session/src/formatter/protocol.rs` are consumed unchanged.
- Owns `FormatterFfiRequest`, `FormatterFfiResult`, and `FormatterFfiAdapter`: UTF-16 request ranges convert against the exact authored source revision to checked SFC-absolute `Span`; a UTF-16 request cursor first passes the same strict conversion and then constructs FMT4P's `PrivateFormatterAdapterRequest` with a checked zero-width authored `Span`; result edit `Span` values convert against that same authored source; the private formatted cursor converts against the exact `PrivateFormatterAdapterResult` output text.
- Formatter conversion uses new checked operations under `crates/verter_ffi/src/formatter/**`. It must not call the existing permissive `utf16_to_byte_offset()`, `byte_offset_to_utf16()`, or clamping `OffsetIndex` methods. Those utilities remain unchanged for unrelated established FFI callers.
- **FMT4P** supplies versioned DTOs, checked source geometry, and the non-Serde session-private request/result carriers; FMT4F is the sole constructor of a cursor-bearing private request and couples any private `FormattedByteOffset` result to the exact formatted output it indexes.
- Private formatter wrappers do not serialize; generated-TSX coordinates and LSP encoding policy are forbidden.

Internal testable subblocks are strict UTF-16 request range conversion, strict UTF-16 request cursor conversion, authored `Span` result conversion, formatted cursor result conversion against the coupled output, error propagation, and boundary/performance evidence. Identity includes source revision, exact authored/formatted text basis, and protocol version; mismatched basis never publishes.

## Acceptance, deletion, and budgets

- **FMT4F-AC1:** Unicode/CRLF/EOF request ranges/cursors and result edits/cursors round-trip exactly between UTF-16 and the correct authored/formatted byte domain; the cursor-bearing path constructs exactly one FMT4P `PrivateFormatterAdapterRequest` only after strict conversion succeeds.
- **FMT4F-AC2:** a UTF-16 offset beyond encoded length, an offset splitting a surrogate pair, an inverted range, a byte offset beyond text length, or a byte offset inside a UTF-8 code point fails closed. Planted authored/formatted source swaps, byte-as-UTF16 shortcuts, overflow, and clamp/nearest-boundary fallbacks fail.
- **FMT4F-AC3:** conversion preserves FMT4P completeness/errors and performs no parse/render/map work.
- **FMT4F-AC4:** allocation/latency/cancellation/zero-work overhead stays within FMT0.
- Delete nothing and export no NAPI/WASM endpoint; FMT4N/FMT4W own those contributions.
- Ceiling: 450 LOC, 5 files, 1 mutated crate, including `src/lib.rs`. Abort if NAPI/WASM-specific export policy, LSP conversion, route work, permissive global-converter semantic changes, or a second mutated package enters.
- Verify FFI conversion tests, `cargo nextest run -p verter_ffi -p verter_protocol`, and `targeted-domain`.
- Unlocks independent FMT4N and FMT4W. Add only FMT4F's ledger row.
