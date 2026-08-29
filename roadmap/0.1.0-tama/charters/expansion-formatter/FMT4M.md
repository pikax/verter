<!-- unified-charter-v2
id=FMT4M
name=Formatter MCP adapter contribution
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT4P
owner=expansion.formatter:full/range cursor-free MCP formatter adapter and tool result conversion
conflict_domains=formatter_mcp_adapter
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
charter=charters/expansion-formatter/FMT4M.md
max_production_loc=400
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4M — Formatter MCP adapter contribution

## Independently acceptable outcome and owner

Expose one versioned MCP formatter adapter over FMT4P's `Span`-based DTOs and complete service result. The MCP surface is independently testable and usable without NAPI/WASM/LSP promotion. Reverting removes only the MCP adapter/tool contribution.

The sole owner is **full/range cursor-free MCP formatter adapter and tool result conversion**.

## Surfaces and predecessor

- Exact production mutation surfaces: `crates/verter_mcp/src/formatter.rs`, `crates/verter_mcp/src/tools/mod.rs` module integration, and the single `#[tool_router] impl VerterMcpServer` in `crates/verter_mcp/src/server.rs`. The consumed `crates/verter_protocol/src/formatter/**` API is unchanged.
- Owns `FormatMcpRequest`, cursor-free `FormatMcpResult`, MCP tool registration, `Span`-based request-range/edit serialization, and error/cancellation translation.
- **FMT4P:** supplies the sole versioned DTO, internal-to-`Span` conversion, and shared formatter service call.
- LSP positions, UTF-16 conversion, NAPI/WASM exports, and route cutover are excluded.
- MCP supports full-document and authored-range formatting. It does not accept or return cursor projection and never constructs or serializes `PrivateFormatterAdapterRequest`, `PrivateFormatterAdapterResult`, or `FormattedByteOffset`.

Internal testable subblocks are MCP request validation, one shared service call, `Span` result/error conversion, tool registration, and boundary/performance evidence. Identity includes protocol version, source revision, canonical source identity, config provenance, and formatter-policy provenance; partial or mismatched results never publish as success.

## Acceptance, deletion, and budgets

- **FMT4M-AC1:** MCP full/range output, edits, and errors agree with the public Rust DTO and preserve SFC-absolute `Span` geometry; its capability/tool schema contains no cursor cell.
- **FMT4M-AC2:** malformed/out-of-bounds/mixed-revision or partial results fail without fabricated 0:0/current-file fallback.
- **FMT4M-AC3:** one call reaches the shared formatter once and performs no second parse/render/map/conversion authority.
- **FMT4M-AC4:** FMT0 adapter overhead, cancellation, retained memory, and zero-work limits hold.
- Delete no shared route, legacy LSP code, protocol DTO, or other adapter.
- Ceiling: 400 LOC, 4 files, 1 mutated crate. Abort if LSP/FFI policy, DTO redefinition, cursor exposure, route work, or a second mutated package enters.
- Verify MCP protocol/tool tests and `targeted-domain`. Add only FMT4M's ledger row; FMT4 later promotes cross-surface maturity.
