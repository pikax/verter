# Pinned compatibility data ingestion: evidence map

Maps each acceptance and each receiving case from the
[compatibility case table](../WBC0/cases.md) to the test that discriminates it.
Rust tests are in `crates/verter_session/src/compat/data/tests.rs` and run under
the canonical Rust gate. Generator tests are in
`scripts/web-compat/generate.test.mjs` and run under `test:scripts` and
`test:scripts:ci`.

## Acceptance

| Acceptance | Evidence |
| --- | --- |
| AC1: outcome and sole owner | Private `compat::data` with `pub(in crate::compat)` surface; `the_admitted_snapshot_answers_from_the_pinned_upstream_records`; `keeps the committed outputs bound to this recipe and the pinned inputs` |
| AC2: missing or mismatched stays unknown | `missing_data_stays_unknown_and_names_what_is_absent`; `a_schema_mismatch_answers_unknown_without_disturbing_its_neighbours`; `baseline_status_never_answers_for_a_target`; `an_undecidable_boundary_stays_unknown_at_the_target` |
| AC3: identity and determinism | See the applicability table below |
| AC4: public delivery | [Contract](../../WBC1/contract.md) specifies permissions, uncertainty and migration. No public operation exists yet; registration stays with the public delivery |
| AC5: cost and maintainability | See cost below. The deletion set is empty because no compatibility reader existed |

## Receiving cases

Browser-schema admission is covered by `rejects browser release data it cannot
trust rather than omitting it`: a valid multi-browser input is the control;
missing `engine_version` and malformed discarded `release_notes` reject before
projection. `enforces property dependencies and preserves schema dependencies`
distinguishes both dependency forms. The integrity and recipe tests bind the
verbatim browser schema and both BCD schema digests to the snapshot identity.

| Case | Test |
| --- | --- |
| Baseline is not support | `baseline_status_never_answers_for_a_target`; `web_feature_groupings_keep_baseline_apart_from_support` |
| Missing data stays unknown | `missing_data_stays_unknown_and_names_what_is_absent`; `a_schema_mismatch_answers_unknown_without_disturbing_its_neighbours`; `quarantines a schema mismatch at the narrowest record, never guessing a value` |
| Condition preservation | `flags_and_partial_limits_survive_as_distinct_facts`; `alternative_statements_are_evaluated_separately_and_all_retained`; `preserves every condition as a distinct fact` |
| Removal boundary | `removal_is_an_exclusive_upper_bound`; `alternative_statements_are_evaluated_separately_and_all_retained` |
| Flag provenance | `flags_and_partial_limits_survive_as_distinct_facts` |
| Admitted source inventory | `verifies integrity before ingestion and rejects changed bytes`; `keeps the committed outputs bound to this recipe and the pinned inputs`; `web_feature_groupings_keep_baseline_apart_from_support` |
| Offline determinism | `is independent of input key order`; `reproduces the committed fixture snapshot byte for byte`; `the_snapshot_input_state_is_never_an_empty_success`; `the_query_vocabulary_refuses_what_the_snapshot_cannot_answer` |
| Upgrade equality (data level) | `identity_follows_content_so_an_edit_and_its_revert_are_exact` |

Reproducing the real artifacts needs the network, so `generate.mjs --check` is an
explicit command and not part of a hermetic lane. Offline, the suites prove that
the committed snapshot's recipe, payload and identity digests match the current
generator and the pinned inputs, and the Rust suite reads every record of it.

## Identity, incremental and work applicability

| Concern | Disposition |
| --- | --- |
| Deterministic ordering under perturbation | Applicable. Reversing every input object's key order yields identical bytes. Records are sorted, and out-of-order or duplicate records are rejected |
| Edit and revert | Applicable. Snapshot identity derives from content, so an edit changes it and a revert restores it exactly. Answers follow the identity |
| Fresh versus warm equivalence | Applicable at this level. An answer after earlier reads equals one from a fresh load. There is no cache to diverge: each answer parses its record |
| Stale or partial rejection | Applicable. Flipped bytes, truncation, CRLF conversion, forged identity, other data, a future format, extra header fields, dropped or duplicated records and a bad browser record are each rejected |
| Cancellation | Not applicable. Admission and reads are synchronous, hold no lock, spawn nothing and publish nothing |
| Scheduling and query-graph invalidation | Not applicable. This adds no query, cache or scheduled work. `SnapshotState` is `Send + Sync`; the effective-target query key and its invalidation belong to target calculation |

## Cost

No latency, allocation or RSS budget is introduced, and none is gated. Binding
work counts: admission parses browser release data and no feature record, and an
answer parses one feature record plus its web-features records. The containment
test `records_are_parsed_on_demand_so_one_bad_record_is_contained` fails if
admission starts parsing feature records. The generated snapshot adds about 16.5
MiB to the working tree; the session retains it only if a host supplies it, and
no such host exists yet.
