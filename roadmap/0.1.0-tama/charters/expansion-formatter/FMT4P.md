<!-- unified-charter-v2
id=FMT4P
name=Formatter Rust protocol and conversion foundation
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT3C,PUB0,PER0
owner=expansion.formatter:cursor-free Rust formatter DTOs, internal-to-Span conversion, and private cursor request/result carriers
conflict_domains=formatter_protocol_adapter
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-formatter/FMT4P.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4P — Formatter Rust protocol and conversion foundation

## Independently acceptable outcome and rollback

Land versioned Rust formatter request/result DTOs and one checked adapter between the private FMT3C service and SFC-absolute public source geometry. The API is usable by Rust callers and later surface adapters without switching the current LSP route. Reverting removes only the DTO/conversion foundation.

The sole owner is **cursor-free Rust formatter DTOs, internal-to-Span conversion, and private cursor request/result carriers**.

## Surfaces and named boundaries

- Exact production mutation surfaces: `crates/verter_protocol/src/lib.rs` for `pub mod formatter`, `crates/verter_protocol/src/formatter/**`, `crates/verter_session/src/formatter/mod.rs` for protocol-module integration, and `crates/verter_session/src/formatter/protocol.rs`.
- Owns versioned `FormatRequest`, `FormatResult`, `FormatTextEdit`, `FormatterProtocolAdapter`, capability/error/completeness outcomes, and explicit checked conversion between `AuthoredFormatByteRange` and SFC-absolute `Span`.
- Serialized authored request ranges and result edit ranges use `Span` only. `FormatRequest` supports full-document or one optional authored `Span`; `FormatResult` contains formatted output, `FormatTextEdit` values, completeness, and typed errors, but deliberately has no cursor field.
- The session-side, non-Serde `PrivateFormatterAdapterRequest` is exactly one public `FormatRequest` plus `Option<Span>` for an authored cursor. A present cursor span must be zero-width, belong to the request's exact canonical source and source revision, be checked against that revision's exact authored text, be in bounds, and lie on a UTF-8 character boundary before conversion to FMT3C's private authored cursor domain. Its fields are private; it never appears in `verter_protocol`, and ordinary Rust, MCP, and LSP calls use the cursor-free public request path.
- The session-side, non-serialized `PrivateFormatterAdapterResult` couples one `FormatResult` with `Option<FormattedByteOffset>`. Its checked constructor proves the offset is in bounds and on a UTF-8 character boundary of that exact `FormatResult.output`; accessors never return the cursor detached from the borrowed output text. The type has private fields, implements neither serde trait, and never appears in a `verter_protocol` field.
- At this foundation landing, no production caller constructs a cursor-bearing `PrivateFormatterAdapterRequest` or consumes `PrivateFormatterAdapterResult`; local negative tests exercise their checked constructors. FMT4F alone owns later activation after strict UTF-16 conversion and owns the repository-wide sole-call-site guard. Public Rust callers remain on cursor-free `FormatRequest`/`FormatResult`.
- LSP `Position`/`Range`, UTF-16 offsets, NAPI/WASM/MCP exports, capability advertisement, and route changes are excluded.

## Exact predecessor contracts

- **FMT3C:** supplies complete private formatter requests/results and typed source-revision identity.
- **PUB0:** supplies versioning, capability-truth, error, completeness, and serialization laws.
- **PER0:** supplies cancellation, request identity, bounded work, and zero-work semantics.

Internal testable subblocks are versioned DTO definition, checked authored-range/`Span` conversion, private authored-cursor request coupling, complete service-result adaptation, private formatted-output/cursor coupling, and serialization/performance evidence. Request/result identity includes wire version, canonical source, source revision, exact authored/formatted text basis, carrier/profile, config provenance, and formatter-policy provenance; partial or mismatched-basis results cannot serialize as success.

## Acceptance and position laws

- **FMT4P-AC1:** every authored private range converts to the same source revision's checked SFC-absolute `Span`; inverted/out-of-bounds/mixed-revision conversions fail.
- **FMT4P-AC2:** round-tripping admitted authored ranges through `Span` preserves exact UTF-8 bytes at Unicode/CRLF/EOF boundaries; a planted direct integer/implicit conversion fails.
- **FMT4P-AC3:** result completeness/errors/cancellation remain truthful; no partial result or private coordinate serializes as success, and compile-time/serde evidence proves the public Rust/MCP DTO has no cursor result.
- **FMT4P-AC4:** foundation-local negative tests prove a non-zero-width, wrong-source, wrong-revision, wrong-text, out-of-bounds, or non-character-boundary authored cursor cannot construct `PrivateFormatterAdapterRequest`; production call-site inspection proves the dormant foundation has no cursor-bearing request caller.
- **FMT4P-AC5:** foundation-local negative tests prove a detached, out-of-bounds, non-character-boundary, or wrong-output cursor cannot construct `PrivateFormatterAdapterResult`; production call-site inspection proves the dormant foundation has no private-result consumer. FMT4F owns later sole-caller/consumer evidence.
- **FMT4P-AC6:** adapter overhead meets FMT0 counters/allocation/latency and performs zero parse/render/map work beyond one FMT3C service call.

## Migration, deletion, budgets, and consumers

- This node adds DTOs/conversion beside existing routes; it changes no LSP handler/capability and deletes no route or public façade.
- Delete only an exact superseded private protocol prototype under the two owned surfaces if FMT0 is amended to name it; current deletion population is empty.
- Forbid private wrapper serialization, a public Rust/MCP cursor claim, a cursor detached from its exact output basis, generated-TSX coordinates, implicit conversion, LSP/UTF-16 policy, duplicate service invocation, and route activation.
- Ceiling: 800 LOC, 8 files, 2 crates, including both explicit module-root integrations; split before exceeding an M-node quality ceiling.
- Abort if adapter-specific positions, a third package, or route/capability mutation is required.
- Verify protocol serialization/negative coordinate fixtures, `cargo nextest run -p verter_protocol -p verter_session`, and `targeted-domain`.
- Unlocks FMT4L, FMT4F, and FMT4M. Add only FMT4P's ledger row.
