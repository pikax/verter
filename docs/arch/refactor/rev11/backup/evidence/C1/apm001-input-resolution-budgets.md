# C1 APM-001 input-resolution budget implementation

> Round-3 correction: the nominal-mismatch compile-fail fixture and the
> name/path/AST source scanner referenced by older sections are deleted. The
> current rails are exact cross-crate nominal identity, the semantic-owned
> whole-value constructor, dependency direction, boundary behavior, and the
> real internal-second-budget-table production mutation recorded in
> `final-review-round3-comprehensive-fix.md`.

## Subject and authority

- Authority base: `23a0b7c539286d1a11f4097b8f6c223772d22b6d`
  (`fbb5cecfdf9e65e042021569afa84bc771eb0415`)
- Clean-rebase result before implementation:
  `ed4d747ff0f780136a49aa92ced848c079b3040f`
  (`6ee8791d372b5e31211cd40a8dfbe2d338460282`)
- Implementation commit: `94d2538fdc39ae42693d92456195b70a2cca223d`
  (`50305c5d8542473f30b40d572ac77f467da8f960`)
- Operative ruling blob inherited from trunk: SHA-256
  `0c67a42b367728b148b68a3456adb20b2cd51c8490081e82c249e63bcad879cc`
- Scope: APM-001 only. This change does not implement, dispose, or waive
  APM-002/APM-003.

The pre-implementation rebase completed without conflicts or manual resolution.
The candidate delta digest before/after rebase was
`3f8e6cfccace8b500d4ad7bd2b4eea164b6e208b95fa3c9bef21a0d327af40c7`.
The inherited authority-registry, program-state, and ruling blobs were
respectively `8667b7ff09d69be600435ccfc98c5cc5582e7f2d`,
`429f0f60257c5243283ee9da2645d33791f71d2a`, and
`d0ae1e02ad65e91607dfbbae1d5962e1fe64767f`.

## RED to GREEN

The first semantic carrier test was written before the carrier existed.

```text
cargo test -p verter_semantic \
  resolver_core::attempt_outcome_tests::ratified_input_resolution_budgets_are_the_default_and_inclusive \
  --no-run
RED: E0432, unresolved imports InputResolutionBudgets/InputResolutionBudgetMeter
```

The minimum implementation then introduced the semantic-owned immutable carrier,
the workspace operation ledger, and the exact bounded preflight/load seam. The
final focused results are:

```text
cargo test -p verter_semantic ratified_input_resolution_budgets --quiet
PASS: 1 passed

cargo test -p verter_workspace resolution_driver_tests --quiet
PASS: 21 passed

cargo test -p verter_workspace \
  conditional_commit_restarts_share_one_churn_ledger_and_a_new_request_resets_it \
  --quiet
PASS: 1 passed

cargo test -p verter_workspace --tests --quiet
PASS: 818 unit tests + 8 integration/compile-fail tests

cargo check --workspace --all-targets
PASS; all existing explicit MemoryOptions/FilesystemOptions literals compile

cargo fmt --all -- --check
PASS
```

Focused clippy passed with warnings denied after explicitly excluding five
non-semantic lint substitutions:

```text
cargo clippy -p verter_semantic -p verter_workspace --all-targets -- \
  -D warnings \
  -A clippy::unnecessary_lazy_evaluations \
  -A clippy::needless_update \
  -A clippy::result_large_err \
  -A clippy::large_enum_variant \
  -A clippy::manual_saturating_arithmetic \
  -A clippy::manual_is_multiple_of
PASS
```

The first two exclusions are pre-existing warnings outside APM-001. The result
and enum-size suggestions would add boxing/allocation to the request-local loader
carrier and therefore cross into the separately unresolved APM-002 finding. The
saturating suggestion was not adopted because the authority requires explicit
checked projections and overflow-as-breach. The `is_multiple_of` suggestion is
new-toolchain-only test syntax above the repository's declared Rust 1.86 floor.

## Authority coverage

| Obligation | Enforced evidence |
|---|---|
| Sole immutable policy owner, RATIFIED/Default, tightening only | semantic carrier unit test; exact semantic/workspace nominal identity; whole-value workspace constructor type; tightened parsed-edge behavior; real duplicate internal policy-field mutation |
| Attempts 256 inclusive, before kernel | `attempt_budget_is_inclusive_and_rejects_before_the_next_kernel_invocation` |
| Unique keys 1024, full identity, cumulative and single-wave | `unique_key_budget_is_cumulative_inclusive_and_precedes_loading`; `all_unsupported_input_families_map_to_their_exact_observation_kind` |
| Bytes 1 MiB, spelling once, reservation every flight, no refund | `byte_budget_charges_key_and_reservation_before_loading_without_refund` |
| Driver depth 64 accepted waves and precedence | `depth_budget_is_inclusive_and_wins_a_multi_breach_before_loading`; retained project-reference-depth tests |
| Churn 8 across basis/commit retries, fresh independent request | `basis_change_mid_flight_restarts_cleanly_on_the_new_basis`; `conditional_commit_restarts_share_one_churn_ledger_and_a_new_request_resets_it` |
| Unsupported four-family fail-closed mapping and mixed zero-I/O | `all_unsupported_input_families_map_to_their_exact_observation_kind`; `unsupported_input_is_terminal_before_mixed_delta_preflight_or_charging` |
| Exact reservation key/basis before load | `reservation_identity_is_rejected_before_the_load_seam` |
| Actual-over, incomplete capture, key/basis mismatch, overflow | `bounded_loader_rejects_reservation_and_capture_integrity_failures`; `reservation_and_actual_batch_byte_aggregation_reject_checked_arithmetic_overflow` |
| Oversized manifest before payload read/parse; no cache admission; later cold success | `oversized_manifest_is_rejected_before_parse_or_cache_and_a_later_request_is_cold` |
| Retryable load retry and permanent/default unavailability | `transient_load_failure_retries_and_completes`; `permanent_input_load_unavailable_is_immediate_terminal_and_not_retried`; byte reservation retry/breach control |
| Default and tightened ingress without option-field breakage | `workspace_ingress_uses_only_the_semantic_owned_whole_policy_value`; workspace all-target check |
| Existing 24 converted cases and fact-witness replay | full workspace package suite, including the converted-case and resolution-currency modules |

`InputLoadIntegrity` is an intentional public `AttemptFailure` enum widening under
the ruling. Every budget/integrity/unsupported terminal marks the transaction
non-admissible before the Engine can publish a positive, stable negative,
decision/reverse edge, or persistent signature.

## Discriminating mutations

Every mutation below was applied alone, its named test returned exit `101`, and
the mutation was reverted before the final GREEN runs:

| Mutation | Red test / discriminating result |
|---|---|
| Disable attempts check | attempt boundary test reported non-attempt failure |
| Disable unique check | cumulative/single-wave unique test reported the wrong terminal |
| Disable depth check | multi-breach test lost required depth precedence |
| Disable pre-load reservation byte check | byte test called/advanced past the rejected load |
| Stop incrementing churn | churn boundary test no longer produced `InputResolutionChurnLimit` |
| Filter out unsupported-family rejection | mixed delta entered the unreachable preflight closure |
| Filter out loaded-capture integrity rejection | four-reason integrity test lost the exact typed failure |
| Disable reservation identity fence | reservation test observed one loader call instead of zero |
| Refund a failed flight reservation | retry test no longer breached at consumed byte value 3 |
| Reset churn in `charge_outer_restart` | commit-restart mutation ran 8 attempts instead of terminating after 2 |
| Add a second private `Engine` policy field at defaults and route parsed-edge recorders through it | tightened parsed-edge discriminator runs 9 hook calls instead of 2; exact restoration returns GREEN |
| Add a parallel resolution-budget scalar to `HostConfig` and its default | exhaustive compiled `HostConfig` pattern fails with the unaccounted field |

Round-3 correction supersedes the round-2 enforcement claim. The
compiler-visible nominal identity and semantic-owned whole policy value are the
structural rails; the planted private `Engine` field proves the behavioral
discriminator rejects a true second internal budget table. Full receipts are in
`final-review-round3-comprehensive-fix.md`.
