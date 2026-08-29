<!-- unified-charter-v2
id=FMT0
name=Full formatter implementation lock
phase=expansion
train=expansion.formatter
product=formatter
kind=lock
semantic_role=delivery
class=successor
predecessors=FMK0
owner=expansion.formatter:formatter contract, inventory, ownership, and numeric budget lock
conflict_domains=program_authority,performance_evidence
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-formatter/FMT0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT0 — Full formatter implementation lock

## Independently acceptable outcome and owner

Ratify one immutable formatter compatibility matrix, exact current-route inventory, successor ownership table, coordinate/provenance laws, and numeric performance envelope before implementation starts. The sole owner is **formatter contract, inventory, ownership, and numeric budget lock**.

FMT0 is documentation and authority data only. It changes no production file, route, capability, formatter implementation, parser, adapter, or deletion population. Reverting FMT0 removes only the ratified formatter contract and makes every formatter implementation successor unready.

## Concrete authority surfaces

- Authority surfaces: this charter, `authority/dag/expansion-formatter.toml`, `program.md`, `catalogs/formatter-compatibility-v1.toml`, `catalogs/formatter-performance-v1.toml`, and the hermetic files rooted at `fixtures/formatter/**`.
- Contract artifacts: admitted compatibility cells, formatter options and divergences, exact route inventory, successor owner assignment, performance fixture identities/units/thresholds, the complete churn action manifest, and acceptance identifiers.
- Production mutation boundary: none.
- Internal coordinates remain private UTF-8 byte domains. `AuthoredFormatByteOffset`/`AuthoredFormatByteRange` index the complete authored carrier source; `FormattedByteOffset`/`FormattedByteRange` index rendered output. There is no implicit or `From` conversion between domains and no generated-TSX `Generated*` reuse.

## Exact predecessor contract

- **FMK0:** supplies the ratified formatter ownership/composition vocabulary and compatibility classification scheme. FMT0 fills that scheme with exact cells, routes, owners, and budgets; it does not reinterpret FMK0.

## Mandatory lock artifacts

FMT0 must materialize all three authority populations below in its own candidate before its `[[implemented]]` ledger row can be added or accepted. A charter-only row, an empty placeholder, a network-resolved corpus, or prose that promises later population fails FMT0.

`catalogs/formatter-compatibility-v1.toml` has one literal `schema = 1`, one literal `prettier_release` package version, and these complete row families:

- `[[option]]`: `name`, `value_kind`, `default_json`, `applicable_languages`, and `unsupported_values`. The rows are the complete accepted option vocabulary; omitted options are not silently defaulted.
- `[[corpus]]`: `id`, hermetic repository-relative `path` under `fixtures/formatter/corpora/**`, `sha256`, `language`, and `purpose`.
- `[[cell]]`: `id`, `language`, `corpus_id`, `option_set`, `recovery_mode`, `request_mode`, `classification`, and `expected_outcome`. `classification` is exactly `prettier-exact`, `verter-default`, or `unsupported`; `expected_outcome` names the exact checked-in output/error fixture or literal unsupported reason. Every language/option/recovery/request cell is present exactly once.

`catalogs/formatter-performance-v1.toml` has one literal `schema = 1` and these complete fields:

- `[calibration]`: literal `seed`, `iterations`, `rotate_left_bits`, `xor_constant`, `add_constant`, `pseudocode`, ordered `operation_schedule`, `warmup_samples`, `measured_samples`, `sample_order`, and `reported_statistic`. The pseudocode and operation schedule are normative and leave no implementation-selected operation, seed, or sampling order.
- `[[fixture]]`: `id`, hermetic repository-relative `path` under `fixtures/formatter/performance/**`, `sha256`, `authored_utf8_bytes`, `carrier_blocks`, `authored_view_records`, and `mode_tags`. The exact files, not generated shapes, define `small`, `medium`, `large`, `incremental`, `range-cursor`, and `project-lifecycle` identities.
- `[[churn_action]]`: `ordinal`, `source_fixture_id`, `action`, `delta_removed_bytes`, `delta_inserted_bytes`, `profile_id`, `config_id`, `policy_id`, `cancellation_observation`, `expected_cache_disposition`, and `ceiling_mode`. The artifact contains exactly 10,000 contiguous ordinals and is the complete action manifest; the harness never invents actions from a seed at run time.

`fixtures/formatter/**` contains every authored input, expected output/error, option set, transition payload, and churn edit payload referenced by either catalog. FMT0 validation fails on a missing path, hash mismatch, duplicate/omitted cell, incomplete option vocabulary, non-contiguous churn ordinal, any churn count other than 10,000, or any path outside the repository.

## Exact current-route inventory and successor ownership

The ratified source inventory names these current LSP surfaces:

- `crates/verter_lsp/src/features/formatting.rs::format_document` is the whitespace-only formatter implementation and displaced route body.
- `crates/verter_lsp/src/server/mod.rs::LanguageServer::formatting` dispatches standard `textDocument/formatting` requests to `handle_formatting`.
- `crates/verter_lsp/src/server/aux_features.rs::handle_formatting` is the live document-formatting handler.
- `crates/verter_lsp/src/capabilities.rs::document_formatting_provider` is existing capability truth and is not deleted by the formatter train.
- `crates/verter_lsp/src/server/mod.rs::LanguageServer::on_type_formatting` dispatches standard `textDocument/onTypeFormatting` requests to `handle_on_type_formatting`.
- `crates/verter_lsp/src/server/aux_features.rs::handle_on_type_formatting` is the live markup tag-auto-close handler.
- `crates/verter_lsp/src/capabilities.rs::document_on_type_formatting_provider` advertises the `>` trigger for that tag-auto-close handler.

The on-type capability/handler is not a general formatter route. It remains owned by the existing markup auto-close feature and is retained unchanged outside this train. No formatter successor may call, replace, broaden, delete, or claim promotion of it.

Ownership is exclusive:

- **FMT1P–FMT1E:** private crate, provenance, renderer, edit, map, range, and cursor substrate; no live route mutation. FMT1 and FMT1A–FMT1E each have an explicitly empty deletion population. Discovery of any candidate prototype requires a pre-mutation STOP and an FMT0 amendment naming its exact path/symbol and sole owner; no class member conditionally absorbs it.
- **FMT2/FMT2T/FMT2X/FMT2TX, FMTC0/FMTCS0/FMTCL0, FMTH0, FMTV0, FMTS0:** private view/printer contributions; their deletion populations are explicitly empty unless this lock is amended with exact prototype paths/symbols and sole owners before mutation; no live route mutation.
- **FMT3C:** private carrier service composition; no live route mutation.
- **FMT4P/FMT4L/FMT4F/FMT4N/FMT4W/FMT4M:** versioned protocol and public-boundary adapters; FMT4L lands dormant beside the current LSP route.
- **FMT3:** sole owner of switching `handle_formatting` to the FMT4L adapter and deleting the displaced whitespace-only `format_document` body and its route-only helpers/tests.
- **FMT4:** proof-only cross-surface conformance and product promotion; no implementation, conversion, route switch, or deletion.

Discovery of another live formatter route requires an FMT0 amendment naming its exact path/symbol and exactly one later owner before any successor mutates it.

## Compatibility and behavior lock

- Pin the exact Prettier release, option vocabulary, and Vue/Svelte/HTML/JavaScript/TypeScript/JSX/TSX/CSS/SCSS/Less corpora.
- Classify every admitted cell as `prettier-exact`, `verter-default` with a pre-ratified divergence, or `unsupported`; unsupported cells never return success.
- Lock malformed/recovery behavior, edit minimality, map boundary bias, safe range expansion, cursor affinity, newline policy, and idempotence.
- Production formatting consumes existing parser/source artifacts and performs zero second semantic parses.

Public capability truth is per surface; a private formatter capability does not imply a protocol capability:

| Surface | Full document | Authored range | Formatted cursor result | Public coordinate/result representation |
| --- | --- | --- | --- | --- |
| private `FormatterService` | supported | supported | supported | non-serializable authored/formatted UTF-8 wrappers |
| Rust `verter_protocol` | supported | supported | unavailable | request/edit geometry is SFC-absolute `Span`; `FormatResult` has no cursor field |
| LSP | supported | unavailable | unavailable | standard `DocumentFormattingParams` and `TextEdit`; returned edit ranges use negotiated `LineIndex` encoding |
| NAPI | supported | supported | supported | strict UTF-16 request ranges/cursor and result edits/cursor through FMT4F |
| WASM | supported | supported | supported | strict UTF-16 request ranges/cursor and result edits/cursor through FMT4F |
| MCP | supported | supported | unavailable | request/edit geometry is SFC-absolute `Span`; tool result has no cursor field |

LSP range formatting is deliberately not advertised: the live server has neither a `document_range_formatting_provider` nor a `textDocument/rangeFormatting` handler. LSP cursor projection is likewise unavailable because standard document formatting returns `TextEdit[]`, not a cursor result. Adding either cell requires a separately ratified, independently landable capability/route node before mutation; no existing formatter node may infer it from private range/cursor support.

## Numeric performance contract

All counters use these units:

- `A_in`: authored UTF-8 bytes in the immutable source presented to the request before formatter work starts.
- `B_out`: formatted UTF-8 bytes in a complete result.
- `B_emit`: UTF-8 bytes appended exactly once to the unique final formatted-output sink. For a complete request `B_emit = B_out`; cancellation counts only bytes appended to that sink before observation/stop and never invents a complete result.
- `B_copy`: cumulative UTF-8 bytes copied between formatter-owned intermediate buffers or from an intermediate buffer into the final sink. Repeated copies of the same bytes count repeatedly; appending newly produced bytes directly to the final sink increments `B_emit` but not `B_copy`.
- `T_out`: total UTF-8 replacement payload bytes across all emitted edits.
- `C_in`: carrier blocks in the immutable pre-request block inventory.
- `V_in`: source-backed authored-view records in the immutable pre-traversal view inventory; it is counted once before rendering and is not a visit counter.
- `V_visit`: actual formatter visits to authored-view records, including revisits.
- `D_out`: `Doc` nodes constructed for the request.
- `D_visit`: actual renderer node visits plus group-decision operations.
- `E_out`: emitted non-overlapping edits in the complete result.
- `M_out`: emitted position-map segments in the complete result.
- `Q_in`: range, cursor, or map queries submitted after the result map is built.
- `Delta_in`: authored UTF-8 bytes replaced by an incremental edit, counting removed plus inserted bytes.
- A parser invocation means constructing a new semantic parser over formatter input. Existing parse-artifact lookup is not a parser invocation.
- Allocated bytes are cumulative allocator bytes during the measured operation; retained bytes are live formatter-owned bytes after the result and required cache entries are admitted.
- Normalized latency is measured in release mode, one formatter request at a time, with filesystem/provider work excluded. Before each sample set, the runner executes exactly the literal seed/constants/pseudocode/operation schedule and sampling policy in `catalogs/formatter-performance-v1.toml`; its `iterations` is 10,000,000 and the ratification reference score is 25,000,000 ns. `normalized_ms = observed_ms * 25,000,000 / calibration_ns`. The raw and normalized values, CPU model, build profile, calibration ns, and performance-catalog hash are retained.

Required fixtures:

| Fixture | Shape |
| --- | --- |
| `small` | 4 KiB, at least 3 carrier blocks, Unicode and CRLF boundaries |
| `medium` | 100 KiB, at least 10 blocks, 10,000 authored syntax/trivia records, malformed/recovery islands |
| `large` | 1 MiB, at least 40 blocks, mixed admitted languages |
| `incremental` | `medium` plus one in-block replacement of at most 64 authored bytes |
| `range-cursor` | `medium` plus 1,000 deterministic range/cursor/map queries at retained, inserted, deleted, Unicode, CRLF, and EOF boundaries |
| `project-lifecycle` | 128 distinct `medium` sources with fixed canonical identities; open is initially cache-empty and close begins with two admitted identities per source, subject to the global cap |
| `long-churn` | 10,000 requests over the 128 open `project-lifecycle` sources using a checked-in fixed-seed manifest of exact warm, `Delta_in <= 64`, edit-revert, profile/config transition, policy transition, and cancellation actions |

Every successor contribution must preserve these structural ceilings:

- semantic parser invocations: `0`;
- authored-view visits: `V_visit <= V_in + 2C_in`;
- produced document nodes: `D_out <= 4V_in + 64C_in`;
- renderer work: `D_visit <= 3D_out + 64`;
- final-sink emission: `B_emit = B_out` for a complete result; a cancelled attempt reports the directly observed `B_emit` and never assigns a complete `B_out`;
- intermediate/final copying: `B_copy <= 3A_in + 3B_emit + 64 KiB`;
- edit comparisons: at most `4(A_in + B_emit) + 64`; no unbounded diff fallback;
- map segments: `M_out <= 2D_out + 2E_out + 8C_in`;
- one map query: at most `2 * ceil(log2(max(M_out, 1))) + 8` indexed probes, and `Q_in` queries perform at most `Q_in` times that ceiling;
- native call-stack growth: at most 64 frames on adversarial nesting; explicit renderer work-stack entries: at most `2D_out + 64`.

Retention and cache capacity are absolute, not deltas:

- Define one completed-result ceiling `R_entry = A_in + B_out + T_out + 64E_out + 32M_out + 32C_in + 256 KiB`. It covers formatter-owned authored-basis retention, formatted output, edit payload/storage, map storage, carrier/result metadata, and allocator slack for one complete result identity.
- One returned complete result occupies at most `R_entry` formatter-owned live bytes before caller handoff. One cache entry stores exactly one complete result identity and occupies at most `R_entry`. If `R_entry > 96 MiB`, the complete result may be returned within its result ceiling but is not cache-admitted.
- The formatter cache retains at most two complete identities per open canonical source, at most 64 entries process-wide, and at most 96 MiB process-wide. All three limits apply simultaneously. Admission evicts deterministic least-recently-used complete entries until both count and byte caps hold; a single entry that still cannot fit is not admitted.
- With one result still owned by the formatter, post-request retained memory is at most `R_entry + 96 MiB`. After result handoff/release, formatter-owned retained cache memory is at most 96 MiB. A closed, unopened, disabled, or unsupported source retains zero formatter cache entries.
- Cancelled, stale, partial, unsupported, or mismatched source/profile/config/policy results are never cache-admitted. Cache identity is exact canonical source + source revision + carrier/profile + configuration provenance + formatter policy provenance.

The complete formatter must meet these normalized p95 thresholds:

| Mode | Latency | Cumulative allocation | Peak/retained memory |
| --- | ---: | ---: | ---: |
| cold `medium`, no matching cache identity | <= 40 ms | <= `12(A_in+B_out) + 2 MiB` | peak RSS delta <= 24 MiB; absolute retained limits above hold |
| first warm `medium`, exact identity | <= 20 ms | <= `8(A_in+B_out) + 1 MiB` | at most 2 source entries, 64 global entries, and 96 MiB cache |
| repeated warm `medium`, runs 2–10 | <= 12 ms/run | <= `6(A_in+B_out) + 1 MiB`/run | run-10 retained bytes <= run-2 + 256 KiB and absolute caps hold |
| `incremental`, `Delta_in <= 64` | <= 8 ms | <= `4(A_in+B_out) + 512 KiB` | retained delta <= changed result bytes + 256 KiB and absolute caps hold |
| edit-revert pair | <= 18 ms total | <= twice the incremental ceiling | final retained bytes <= pre-edit + 256 KiB |
| `range-cursor`, 1,000 queries | <= 5 ms total | <= 512 KiB total | zero retained query state |
| profile/config transition then first `medium` request | <= 40 ms | <= cold `medium` ceiling | old-identity result is never returned; absolute cache caps hold |
| formatter-policy/capability transition then first `medium` request | transition itself <= 2 ms; first request <= 40 ms | transition <= 64 KiB; first request <= cold `medium` ceiling | old-policy result is never returned; absolute cache caps hold |
| external provider transition | inapplicable: zero formatter callback/work | 0 formatter bytes | provider identity is outside formatter inputs; cache contents and next-request warm eligibility are unchanged |
| project open, 128 unopened sources | <= 5 ms total | <= 64 KiB total | zero parser/view/render/edit/map work and zero formatter cache entries |
| project close, 128 sources | <= 5 ms total | <= 64 KiB total | zero parse/view/render/edit/map work; at most 128 source-index lookups and 64 cache-entry removals, then zero entries for every closed source |
| `long-churn`, 10,000 requests | completed-request p95 <= 20 ms | total <= the sum of the manifest's 10,000 action-specific allocation ceilings; every request obeys its own ceiling | request 10,000 retained <= request 1,000 + 1 MiB; at most 64 entries/96 MiB; zero stale or partial admissions |
| cold `large` | <= 300 ms | <= `12(A_in+B_out) + 8 MiB` | peak RSS delta <= 96 MiB; absolute retained limits above hold |
| cancellation | <= 5 ms from observation to formatter stop | applicable mode allocation before observation + <= 64 KiB teardown | no partial/stale admission; absolute cache caps hold |
| disabled, unsupported, unopened, or capability-masked request | <= 2 ms | <= 64 KiB request envelope | zero parser/view/render/edit/map work and zero retained entry for the request identity |

Cancellation must stop formatter-owned work within 5 normalized ms after cancellation observation, allocate no more than the applicable mode before observation plus 64 KiB teardown bookkeeping, and admit no partial result. Disabled, unsupported, unopened, and capability-masked requests perform zero parser/view/render/edit/map work, allocate at most 64 KiB of request-envelope bookkeeping, and retain zero cache entries for that request identity. Project open/close and external-provider transition are bounded inapplicable lifecycle rulings: they may perform only the explicitly listed cache/envelope operations and never invoke the formatter pipeline.

If the reference calibration, fixture identities, counters, or thresholds cannot be reproduced, the affected successor aborts and amends FMT0 before implementation. A real capability may consume the non-zero work above; blanket 0% regression language is forbidden.

## Acceptance and discriminating evidence

- **FMT0-AC1 — complete inventory:** bounded source inspection proves every current formatter route/capability symbol is listed and assigned once; a planted omitted/duplicate owner fails the inventory check.
- **FMT0-AC2 — complete compatibility matrix:** `catalogs/formatter-compatibility-v1.toml` names the literal Prettier release, complete option vocabulary/defaults, hermetic corpus paths/hashes, and every language/option/recovery/request cell with one classification and expected outcome.
- **FMT0-AC3 — numeric ratification:** `catalogs/formatter-performance-v1.toml` names exact fixture identities, literal calibration seed/constants/pseudocode/operation schedule/sampling, every required mode's units and thresholds, and exactly 10,000 explicit churn actions; removing any dimension fails validation.
- **FMT0-AC4 — zero production implementation:** the candidate diff contains zero production files, successor ledger rows, route edits, capability edits, or deletions. All mandatory catalogs and `fixtures/formatter/**` inputs are materialized and validated before the one required FMT0 implementation-ledger row is added; that row is authority metadata, not production work.

## Deletions, rollback, and forbidden designs

- FMT0 deletes nothing and accepts no production deletion.
- Rollback removes only contract/authority text; no production state exists to restore.
- Forbid post-implementation compatibility choices, vague route names, shared deletion ownership, a second semantic parser, external production formatting, untyped coordinate conversion, and successor implementation in this node.

## Budgets, aborts, verification, and consumers

- Target ceiling: 0 production LOC, 0 production files, 0 related production packages. Roadmap catalogs and hermetic fixture files are mandatory authority/evidence artifacts and do not consume the production budget.
- Abort if source inspection cannot identify an exact route or sole successor owner, if any numeric mode lacks a measurable unit/threshold, or if this node would need production mutation.
- Verify with strict DAG validation, roadmap tests, `git diff --check`, bounded source-route inspection, and `docs-domain`.
- Unlocks FCFG0 and FMT1P. Each successor consumes only the exact contract dimensions named in its charter and must amend FMT0 rather than silently relax them.

Apply `architecture-3`. Add only FMT0's trusted implementation-ledger row when this contract node itself is implemented.
