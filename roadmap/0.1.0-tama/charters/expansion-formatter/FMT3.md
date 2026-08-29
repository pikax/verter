<!-- unified-charter-v2
id=FMT3
name=Shared formatter route-adapter cutover and deletion
phase=expansion
train=expansion.formatter
product=formatter
kind=cutover
semantic_role=delivery
class=successor
predecessors=FMT4L
owner=expansion.formatter:shared and LSP formatter route-adapter cutover and deletion
conflict_domains=formatter_route_cutover
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
charter=charters/expansion-formatter/FMT3.md
max_production_loc=500
max_production_files=6
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT3 — Shared formatter route-adapter cutover and deletion

## Independently acceptable outcome and rollback

Atomically switch the live LSP document-formatting handler from the whitespace-only implementation to the already-landed FMT4L adapter, then delete the displaced whitespace-only/shared route body. The LSP capability remains truthful and continuous. Reverting restores the old handler/body while leaving the dormant adapter and private service intact.

The sole owner is **shared and LSP formatter route-adapter cutover and deletion**.

## Exact mutation surfaces and deletion population

- `crates/verter_lsp/src/server/aux_features.rs::handle_formatting`: replace the call to the old `format_document` route with exactly one `FormatterLspAdapter` call.
- `crates/verter_lsp/src/features/formatting.rs::format_document`: delete the displaced whitespace-only implementation and its route-only helpers/tests; remove the module if no non-route symbol remains.
- `crates/verter_lsp/src/features/mod.rs::formatting`: remove the old `pub mod formatting` declaration when the displaced module is deleted; retain FMT4L's independent `formatter_adapter` module registration.
- `crates/verter_lsp/src/capabilities.rs::document_formatting_provider`: verify it remains truthful; this node does not add, remove, or broaden capability cells.
- `crates/verter_lsp/src/capabilities.rs::document_on_type_formatting_provider` and `crates/verter_lsp/src/server/aux_features.rs::handle_on_type_formatting`: verify the markup tag-auto-close capability/handler remain unchanged and reachable outside this train.
No session, printer, private geometry, protocol DTO, `LineIndex`/UTF-16 conversion, range-formatting capability/handler, cursor result, on-type implementation, NAPI/WASM/MCP adapter, or public promotion mutation belongs here.

## Exact predecessor contract

- **FMT4L:** supplies a complete dormant LSP adapter. Its FMT4P→FMT3C ancestry guarantees versioned DTOs, legal boundary conversion, private service composition, and all carrier printers before the route moves.

## Cutover laws and acceptance

- Characterize the current handler/capability and whitespace output before mutation.
- One patch switches the handler and removes the old body; no flag, fallback, shadow output, dual call, or compatibility branch may land.
- A post-switch source search proves the old formatter symbol/body is unreachable or absent and every formatting request reaches the shared service once.

- **FMT3-AC1:** LSP formatting remains continuously advertised and requests reach exactly one FMT4L→FMT4P→FMT3C path.
- **FMT3-AC2:** HTML/Vue/Svelte full-document output reconstructed from standard LSP `TextEdit` values agrees with direct FMT4L fixtures; private range/cursor behavior is not an LSP capability claim.
- **FMT3-AC3:** the old whitespace-only output and route symbol are unreachable; a planted retained handler call fails.
- **FMT3-AC4:** cutover adds no duplicate work and meets FMT0 cold/warm/incremental/cancellation/zero-work thresholds.
- **FMT3-AC5:** LSP range formatting remains unadvertised, and the on-type tag-auto-close dispatch/capability remains byte-for-byte behaviorally unchanged and does not call the formatter service.

## Budgets, aborts, verification, and consumers

- Ceiling: 500 LOC, 6 files, 1 crate, including `features/mod.rs`. Any additional adapter/deletion population requires an FMT0 amendment and split before mutation.
- Abort if the live route inventory differs, if FMT4L is not independently callable, if capability continuity requires a second route, if on-type behavior would change, or if printer/protocol/conversion semantics need changes.
- Verify focused LSP automated tests, old-symbol/call-site deletion proof, mixed-carrier full-document formatting, on-type tag-auto-close regression coverage, `cargo nextest run -p verter_lsp -p verter_session`, and `targeted-domain`.
- Unlocks FMT4. Add only FMT3's ledger row.
