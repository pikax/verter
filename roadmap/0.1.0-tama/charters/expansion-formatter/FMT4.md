<!-- unified-charter-v2
id=FMT4
name=Formatter cross-surface conformance and promotion
phase=expansion
train=expansion.formatter
product=formatter
kind=terminal
semantic_role=delivery
class=successor
predecessors=FMT3,FMT4N,FMT4W,FMT4M
owner=expansion.formatter:cross-surface formatter conformance evidence and product promotion
conflict_domains=capability_catalog,performance_evidence
resource_class=docs-light
review_profile=architecture-3
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
release_gating=product
external_requirements=
charter=charters/expansion-formatter/FMT4.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT4 — Formatter cross-surface conformance and promotion

## Independently acceptable outcome and owner

Ratify cross-surface parity and promote formatter product maturity only after the Rust protocol, live LSP route, NAPI, WASM, and MCP contributions are independently accepted. This proof-only terminal changes no production code, route, adapter, conversion, capability implementation, or deletion population.

The sole owner is **cross-surface formatter conformance evidence and product promotion**.

## Authority surfaces and predecessor contracts

- Authority/evidence surfaces: formatter capability snapshots, conformance matrices, benchmark receipts, product/user documentation, and this roadmap authority. Production LOC is zero.
- **FMT3:** supplies the single live LSP route and proof that the whitespace/shared route was deleted.
- **FMT4N:** supplies the accepted NAPI adapter/export and transitive FFI/Rust protocol service.
- **FMT4W:** supplies the accepted WASM adapter/export and transitive FFI/Rust protocol service.
- **FMT4M:** supplies the accepted MCP adapter/export and transitive Rust protocol service.

FMT4 consumes but cannot redefine FMT4P's `Span` laws, FMT4L's negotiated LSP conversion, FMT4F's UTF-16 conversion, any printer, or service behavior.

Internal proof subblocks are cross-surface semantic parity, coordinate/encoding parity, capability truth, FMT0 performance receipts, and reviewed dogfood. Evidence identity binds the exact implemented predecessor set, corpus/config versions, benchmark calibration, and candidate tree; mixed or stale evidence cannot promote.

Promotion is per-surface rather than fictitious lowest-common-denominator parity:

| Surface | Promoted formatter cells | Coordinate/result truth |
| --- | --- | --- |
| Rust | full + authored range | SFC-absolute `Span` request/edit geometry; no cursor result |
| LSP | full document only | standard `DocumentFormattingParams`/`TextEdit` with negotiated `LineIndex`; no range capability or cursor result |
| NAPI | full + authored range + cursor | strict FMT4F UTF-16 request/result conversion |
| WASM | full + authored range + cursor | strict FMT4F UTF-16 request/result conversion |
| MCP | full + authored range | SFC-absolute `Span` request/edit geometry; no cursor result |

The pre-existing LSP on-type tag-auto-close capability is retained outside the formatter product and is not promoted here.

## Conformance and promotion acceptance

- **FMT4-AC1 — common cross-surface parity:** one locked corpus proves Rust/LSP/NAPI/WASM/MCP reconstructed output bytes, authored edits, unsupported/errors, and cancellation agree for every cell shared by those surfaces. Cursor parity is proven only for private/NAPI/WASM, and range parity only for Rust/NAPI/WASM/MCP.
- **FMT4-AC2 — coordinate truth:** LSP full-document edits use negotiated `LineIndex` conversion; NAPI/WASM request ranges/cursors and result edits/cursors use strict FMT4F UTF-16 conversion; MCP/Rust request/edit geometry uses SFC-absolute `Span`; public Rust/MCP/LSP results contain no cursor; and no private/generated coordinate serializes.
- **FMT4-AC3 — capability truth:** each surface advertises only its independently landed cells; CLI remains unavailable until CLIF0.
- **FMT4-AC4 — performance promotion:** FMT0 cold/first-warm/repeated-warm/incremental/edit-revert/applicable range-cursor/transition/project-open-close/long-churn/large/cancellation/zero-work receipts pass with the absolute result/cache capacity bounds and no unexplained retained growth.
- Differential/idempotence dogfood produces a finite reviewed diff and routes any semantic failure back to its exact printer/service/adapter owner; this terminal cannot patch it.

## Deletions, rollback, budgets, and verification

- Delete nothing. No route switch, public façade removal, adapter implementation, coordinate conversion, semantic fix, or capability implementation is permitted.
- Rollback removes only promotion/evidence/documentation state; independently accepted adapters and the live route remain unchanged.
- Target ceiling: 0 production LOC, 0 production files, 0 related production packages.
- Abort and reopen the exact predecessor if common-cell parity, per-surface coordinate/capability truth, or performance evidence fails.
- Verify all predecessor gates/receipts, strict DAG validation, roadmap tests, `git diff --check`, and `docs-domain`.
- Unlocks CLIF0 and formatter product release gating. Add only FMT4's ledger row.
