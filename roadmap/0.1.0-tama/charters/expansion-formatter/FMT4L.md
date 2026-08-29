<!-- unified-charter-v2
id=FMT4L
name=Formatter LSP adapter contribution
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT4P
owner=expansion.formatter:full-document LSP formatter edit conversion adapter
conflict_domains=formatter_lsp_adapter
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
charter=charters/expansion-formatter/FMT4L.md
max_production_loc=550
max_production_files=6
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4L — Formatter LSP adapter contribution

## Independently acceptable outcome and rollback

Land a dormant, directly testable adapter for the existing standard LSP full-document formatting route. It converts FMT4P's SFC-absolute result edits to negotiated-encoding `TextEdit` values. It is not called by `handle_formatting` and does not change capability registration. Reverting removes only the dormant adapter.

The sole owner is **full-document LSP formatter edit conversion adapter**.

## Surfaces and boundaries

- Exact production mutation surfaces: `crates/verter_lsp/src/features/mod.rs` for formatter-adapter module registration and `crates/verter_lsp/src/features/formatter_adapter/**`; `crates/verter_protocol/src/formatter/**` is a consumed, unchanged API.
- Owns `FormatterLspAdapter`, validation of `DocumentFormattingParams` against the exact open document revision, one full-document FMT4P request, and `Span`→LSP `Range` conversion for returned `TextEdit` values through that document's negotiated `LineIndex`.
- Consumes FMT4P DTOs and conversions; it never serializes private authored/formatted wrappers or uses generated-TSX `Generated*` coordinates.
- The existing LSP formatter surface is full-document only. This node adds no `DocumentRangeFormattingParams`, `document_range_formatting_provider`, range-formatting handler, cursor request, or cursor result.
- `handle_formatting`, `format_document`, `document_formatting_provider`, route selection/deletion, and non-LSP adapters are excluded. `document_on_type_formatting_provider` and `handle_on_type_formatting` are retained markup tag-auto-close surfaces outside this train and are also excluded.

## Exact predecessor contract and acceptance

- **FMT4P:** supplies versioned full-document DTO use, SFC-absolute `Span` edits, and complete cursor-free public service outcomes.

Internal testable subblocks are full-document request validation, one service invocation, edit result conversion, error/cancellation translation, and dormant call-site enforcement. Identity includes document URI/canonical source identity, source revision, negotiated encoding, protocol version, translated config provenance, and formatter-policy provenance; mismatched basis fails rather than publishing.

- **FMT4L-AC1:** UTF-8/UTF-16/UTF-32 negotiated edit conversion is exact at Unicode, CRLF, EOF, and edit-range boundaries.
- **FMT4L-AC2:** edits map from SFC-absolute `Span` against the exact authored document `LineIndex`; applying returned `TextEdit` values reconstructs the direct formatter output.
- **FMT4L-AC3:** malformed/out-of-bounds/mixed-revision conversion fails without 0:0, nearest-token, or encoding fallback.
- **FMT4L-AC4:** one adapter request invokes FMT4P/FMT3C once and meets FMT0 adapter overhead/cancellation/zero-work limits.
- A call-site guard proves the live `handle_formatting` still invokes the current whitespace route after this node; FMT3 owns the switch. Capability inspection proves range formatting remains unadvertised and the on-type tag-auto-close route remains unchanged.

## Deletion, budgets, and consumers

- Delete nothing. Add no dual live route, capability toggle, handler branch, fallback, or shadow comparison.
- Ceiling: 550 LOC, 6 files, 1 mutated crate, including `features/mod.rs`. Abort if range/cursor capability, route/capability mutation, FFI policy, public schema redefinition, or a second mutated package enters.
- Verify focused line-index/encoding and dormant full-document-adapter integration tests, `cargo nextest run -p verter_lsp -p verter_protocol`, and `targeted-domain`.
- Unlocks FMT3. Add only FMT4L's ledger row.
