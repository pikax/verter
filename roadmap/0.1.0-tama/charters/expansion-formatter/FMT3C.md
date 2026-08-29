<!-- unified-charter-v2
id=FMT3C
name=Private formatter service composition
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMTH0,FMTV0,FMTS0
owner=expansion.formatter:private carrier routing, composition, and formatter-result aggregation
conflict_domains=formatter_service
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
charter=charters/expansion-formatter/FMT3C.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT3C — Private formatter service composition

## Independently acceptable outcome and rollback

Install a private `FormatterService` that selects exactly one accepted outer-carrier contribution, composes embedded-language results once, and aggregates private output/edits/maps/range/cursor/recovery outcomes. It is directly usable by in-process tests and later adapters but changes no live or public route. Reverting removes only the private service.

The sole owner is **private carrier routing, composition, and formatter-result aggregation**.

## Surfaces and named boundaries

- Production surfaces: `crates/verter_formatter/src/lib.rs` for service-module registration, `crates/verter_formatter/src/service/**`, `crates/verter_session/src/lib.rs` for formatter-module registration, and `crates/verter_session/src/formatter/**`, including its `mod.rs`. Both crate roots and the formatter module root count against the eight-file ceiling.
- Owns `PrivateFormatRequest`, `PrivateFormatResult`, `FormatterService`, typed carrier/profile selection, embedded demand/composition, result aggregation, and the sole complete-result cache/admission policy bounded by FMT0's per-source/global entry and byte caps.
- Consumes FMTH0/FMTV0/FMTS0 printer outputs plus FMT1B–FMT1E geometry unchanged.
- Public `FormatRequest`/`FormatResult` DTOs, `Span` conversion, LSP/FFI/MCP adapters, capabilities, and live route switching are excluded.

## Exact predecessor contracts

- **FMTH0:** complete private neutral-HTML view/printer contribution.
- **FMTV0:** complete private Vue carrier contribution with no route.
- **FMTS0:** complete private Svelte carrier contribution with no route.

## Binding architecture and acceptance

- Each request binds one source revision, carrier/profile, configuration provenance, and cancellation token.
- Exactly one outer carrier runs. Embedded regions are disjoint, source-ordered, and formatted at most once.
- Result completeness is explicit; unsupported, cancelled, stale, or partial results cannot be admitted as complete.
- `FormatProvenanceId` and all authored/formatted private coordinates remain unchanged and non-serializable.

Internal testable subblocks are carrier/profile selection, embedded demand planning, result aggregation, recovery/unsupported propagation, complete-only cache admission/eviction, and incremental/cancellation/lifecycle/performance evidence. Service identity is canonical source + source revision + carrier/profile + FCFG0 config provenance + formatter-policy provenance; only complete results for that exact basis may publish or warm.

- **FMT3C-AC1:** every admitted carrier selects exactly one printer; duplicate/missing selection fails closed.
- **FMT3C-AC2:** embedded demand and aggregation preserve source order, provenance, edit non-overlap, map completeness, and recovery truth.
- **FMT3C-AC3:** incremental equals fresh for the same source/config/policy basis; cancelled/stale/partial results never warm, and deterministic eviction maintains at most two identities per open source, 64 entries, and 96 MiB.
- **FMT3C-AC4:** FMT0 cold/warm/incremental/edit-revert/transition/project-open-close/long-churn/cancellation/zero-work counters and thresholds hold with no duplicate parse or embedded formatting.

## Migration, deletion, budgets, and consumers

- No live handler, formatter route, capability, public DTO, public export, or route adapter changes. In particular, LSP document/range/on-type capability and handler surfaces remain untouched.
- Delete only a source-confirmed private composition prototype under the two owned surfaces after private consumers migrate; FMT0 currently names none, so the default deletion population is empty.
- Forbid dual service authority, public conversion, printer reimplementation, double formatting, whole-file fallback, and route shadowing.
- Ceiling: 800 LOC, 8 files, 2 crates; split before exceeding an M-node quality ceiling.
- Abort if composition requires a third package, public/wire work, capability mutation, or a live route change.
- Verify mixed HTML/Vue/Svelte workspace and incremental fixtures, `cargo nextest run -p verter_formatter -p verter_session`, and `targeted-domain`.
- Unlocks FMT4P. Add only FMT3C's ledger row.
