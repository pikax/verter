# C1 final review round 2 — comprehensive fix evidence

> Superseded by `final-review-round3-comprehensive-fix.md`. In particular, the
> round-2 `syn` name/path scanner and the nominal-mismatch policy compile-fail
> fixture described below were rejected as nondiscriminating/prohibited and
> are deleted in round 3. Round-2 identities, commands, and green totals are
> historical and do not restamp the current candidate.

## Subject and scope

- Reviewed candidate: `70f0253481637b92e81169909429902c15142b52`
- Reviewed tree: `7cfd97423f933c38738cd7628408743423f1c009`
- Branch: `block/module-resolver-core`
- Initial worktree: clean
- Fix ownership: C1 only. No integration/AMD-024 worktree, authority
  registry/ruling, `performance-gates.toml`, or C-train file was changed.
- Final canonical gate, performance benchmark, reviews, verifier, and landing
  were deliberately not run in this implementation round.

The three final-review reports were treated as one inventory. There was no
authority conflict and no finding was waived.

## Finding-to-fix map

| Class | Production correction | Discriminating proof |
|---|---|---|
| Outer churn ceiling | Removed the independent eight-iteration engine cap. Both importer and explicit-project entry points now let the operation-owned `InputResolutionLedger` apply the ratified inclusive churn maximum; the provider commit-conflict path and its precedence remain separate. | `ratified_outer_churn_boundary_runs_the_ninth_attempt_and_rejects_only_its_restart`; the existing conditional-commit/reset case remains exact. |
| Superseded output | Every world-refresh/currentness rejection discards the attempt's `result`; no `last_result` survives a whole restart. | `explicit_project_churn_rejection_discards_the_superseded_result` plus strengthened importer conditional-churn assertions, including reverse-edge absence. |
| APM-001 structure/arithmetic/admission | Replaced source token scans with nominal compiler/coherence/visibility assertions, negative compile fixtures, an exhaustive `HostConfig` destructure, direct checked-sum helpers, and reverse-index non-admission assertions. | `parallel_input_resolution_policy_cannot_enter_workspace`, `host_config_shape_has_no_module_resolution_budget_ingress`, `reservation_and_actual_batch_byte_aggregation_reject_checked_arithmetic_overflow`, and the oversized-manifest cold-retry test. |
| Final-state language/evidence | Audited semantic/workspace/session production and test-module comments for stale resolver, pre-transition, phase, and direct-host wording. Corrected the frozen-report and APM evidence claims invalidated by this fix. | Zero-result source audit recorded below; historical deliberation artifacts remain historical. |
| Immutable fact/value MOVE | Deleted workspace's duplicate `FactVersionRef` graph and resolution identity/value graph. `verter_semantic::facts::{version,resolution}` owns the nominal values; workspace retains cache/versioning/publication policy and lawfully re-exports the semantic types. Distinct MIRROR rows such as `DirEntry`/`RouteDirEntry` were not merged. | Cross-crate `TypeId` identity proofs for every moved row plus a `syn` item-declaration guard proving complete positive ownership and source absence at both old workspace homes. |
| ProjectMembership MOVE | Moved `ProjectMembership` to `verter_workspace::membership` and re-exported that same nominal type at `verter_workspace` root; deleted the old private module. | Positive assignment across both ruled paths and compile-fail rejection of `verter_workspace::project_membership`. |
| Request-bound construction | Production `ResolverContext` requires the private request-bound seal. Direct `VerterHost` sealing, implementation, and dispatcher access compile only under `test`/`test-support`; production call sites construct `HostResolverContext`/`SessionResolverContext` or use the scoped base-context helper. Both request lifecycles clone the already bound fixed view when an owned view is required; they do not reopen a raw host snapshot. | Production `assert_not_impl_any!(VerterHost: ResolverContext)`, positive request-context coercions, production-only library check, template-class branch tests, the default-build test-support fence, and fixed-view O(1)/publication-fence suites. |
| Retryability taxonomy | `InputLoadUnavailable` is permanent/terminal. New `TransientInputLoadFailure` retries from a fresh kernel attempt only after the next attempt and known reservation fit. Transient bounded-preflight failure is separately retryable with zero payload reservation until a reservation exists; permanent preflight/load failures are immediate and non-cacheable. | Independent transient preflight/load success and permanent preflight/load terminal tests, with exact attempt/preflight/load counts. |
| Native bounded preflight | Native bounded path/realpath preflight uses live metadata only and never consults or populates the directory index. Manifest preflight reserves from live file length; bounded load caps the payload. The recorder delegates to the bounded backend and records only returned scalar observations. | A real native directory containing 1,024 entries proves zero `read_dir`, zero directory-index refresh, and zero retained directory entries during bounded preflight. |

## TDD RED → GREEN receipts

| Area | RED command/result | GREEN command/result |
|---|---|---|
| Inclusive churn + stale output | Candidate behavior was captured by the new discriminators: an eight-attempt outer plant failed the ninth-attempt assertion in run `2745bed1-067c-4a5c-82f5-b1feeecbbf85`; stale-result plants failed both entry-point assertions in run `0eaa1771-eadc-4ace-9c30-4ecff92f83ce`. | `cargo nextest run -p verter_workspace -E 'test(ratified_outer_churn_boundary_runs_the_ninth_attempt_and_rejects_only_its_restart) | test(explicit_project_churn_rejection_discards_the_superseded_result)'` — run `fcc733fd-a3ec-4a25-93f8-f569efafca2f`, 2 PASS. Provider-conflict precedence plus retry cases passed in `9e52324e-7835-42bf-8d01-6c02076d6f68`. |
| Permanent unavailability | `cargo nextest run -p verter_workspace -E 'test(permanent_input_load_unavailable_is_immediate_terminal_and_not_retried)'` with a permanent-retry plant — RED: permanent failure was retried and replaced by a five-limit result. | Final retry matrix run `128dd077-bbf9-40c0-bd55-10b55a9f4211`, 4 PASS; final provider/retry run `9e52324e-7835-42bf-8d01-6c02076d6f68`, 5 PASS. |
| Transient preflight | `cargo nextest run -p verter_workspace -E 'test(transient_preflight_failure_retries_and_completes) | test(permanent_preflight_unavailable_is_immediate_terminal_and_not_retried)'` — run `1bad694b-5caa-4225-aa4c-636f666c2422`, exit 100: permanent case PASS, transient case RED with `TransientInputLoadFailure`. | `cargo nextest run -p verter_workspace -E 'test(transient_preflight_failure_retries_and_completes) | test(permanent_preflight_unavailable_is_immediate_terminal_and_not_retried) | test(transient_load_failure_retries_and_completes) | test(permanent_input_load_unavailable_is_immediate_terminal_and_not_retried)'` — run `128dd077-bbf9-40c0-bd55-10b55a9f4211`, 4 PASS. |
| Membership old path | `cargo nextest run -p verter_workspace --test main -E 'test(project_membership_old_module_path_is_absent)'` first wrote `wip/project_membership_old_path_is_absent.stderr` and failed because the expected diagnostic had not yet been accepted. | The diagnostic was inspected and accepted; combined compile-fail/ownership run `a6505669-f758-466b-8a95-fb8700d4a809`, 4 PASS. |
| Scalar policy ingress | `cargo nextest run -p verter_workspace --test main -E 'test(parallel_input_resolution_policy_cannot_enter_workspace)'` first wrote its expected diagnostic and failed because the expected stderr had not yet been accepted. | The diagnostic was inspected and accepted; combined compile-fail/ownership run `a6505669-f758-466b-8a95-fb8700d4a809`, 4 PASS. |
| Request-view O(1) regression found during closure | `cargo nextest run -p verter_semantic -p verter_workspace -p verter_session` — run `6f57677f-014d-43d3-9a44-b1e64ae57d80`, RED after 6,038 executed: `warm_public_api_batch_from_host_calls_are_o1_not_per_item` observed 85 view reads. Focused run `2f288617-ffe5-4ebe-ab70-ab2cc31f0c4c` reproduced 85; intermediate run `cda4f53b-1bb5-41f5-b48d-a87a4c4e4b42` reduced but remained RED at 13. | Fixed-view propagation through shallow-state/materialization/fallthrough paths passed both original discriminators in `8ad09d3c-4fe5-4d49-af0c-d67e91441e29`; the final expanded fixed-view/fence suite passed 12/12 in `a3fe669d-ed69-468a-9c6d-d608c2850e36`. |

## Reversible mutation receipts

Every plant below changed production code, ran the named discriminator, and
was restored before the final green verification.

| Plant | Exact discriminator/result |
|---|---|
| Re-export the deleted `project_membership` module path. | `cargo nextest run -p verter_workspace --test main -E 'test(project_membership_old_module_path_is_absent)'` — RED because the compile-fail subject unexpectedly compiled. |
| Add a workspace-local duplicate `FactAttribution` and route workspace uses through it. | `cargo nextest run -p verter_workspace --test main -E 'test(fact_version_value_graph_is_semantic_owned_and_workspace_reexported)'` — RED on distinct `TypeId`s. |
| Add `enum FactVersionRef { Plant }` to the old workspace `fact_cache.rs` source while retaining the lawful re-export. | `cargo nextest run -p verter_source_policy_gate -E 'test(moved_fact_value_graph_is_declared_only_by_semantic_at_the_ruled_sources)'` — run `c5bab366-f1c9-433f-b2a9-ab467635b78e`, RED with `the old workspace source redeclared moved values: {"FactVersionRef"}`. |
| Remove the production cfg fences from direct-host sealing/implementation. | `cargo check -p verter_session --lib` — RED `E0283` at the production negative coherence assertion (and the cfg-only method boundary). |
| Add a differently named input-resolution scalar to `HostConfig` and its default. | `cargo check -p verter_session --all-targets` — RED `E0027`: the exhaustive `HostConfig` destructure did not mention the planted field. |
| Replace checked actual-byte addition with saturation. | `cargo nextest run -p verter_workspace -E 'test(reservation_and_actual_batch_byte_aggregation_reject_checked_arithmetic_overflow)'` — RED: `Some(u64::MAX)` instead of `None`. |
| Route native bounded preflight back through ordinary `WorkspaceRead::probe_path`. | `cargo nextest run -p verter_workspace -E 'test(bounded_resolution_preflight_never_enumerates_or_retains_native_directory_entries)'` — RED: native `read_dir_count` was 1 instead of 0. |
| Retry permanent `InputLoadUnavailable` at the payload boundary. | `cargo nextest run -p verter_workspace -E 'test(permanent_input_load_unavailable_is_immediate_terminal_and_not_retried)'` — RED: a five-limit result replaced permanent unavailability. |
| Delete the explicit transient-preflight retry arm. | `cargo nextest run -p verter_workspace -E 'test(transient_preflight_failure_retries_and_completes)'` — run `28ea8194-29a6-49bc-9806-7c9bb56e10d9`, exit 100, RED with the first transient failure returned. |
| Widen the transient-preflight retry arm to include permanent `InputLoadUnavailable`. | `cargo nextest run -p verter_workspace -E 'test(permanent_preflight_unavailable_is_immediate_terminal_and_not_retried)'` — run `573c8918-9428-4d35-a02e-cd097ef1f76e`, exit 100, RED because the exact permanent result/counters were replaced by retries. |
| Reintroduce an eight-capture outer return before the ninth importer attempt. | `cargo nextest run -p verter_workspace -E 'test(ratified_outer_churn_boundary_runs_the_ninth_attempt_and_rejects_only_its_restart)'` — run `2745bed1-067c-4a5c-82f5-b1feeecbbf85`, RED: captured attempts were 8 instead of 9. |
| Return the invalidated attempt's result from importer and explicit-project churn rejection branches. | `cargo nextest run -p verter_workspace -E 'test(ratified_outer_churn_boundary_runs_the_ninth_attempt_and_rejects_only_its_restart) | test(explicit_project_churn_rejection_discards_the_superseded_result)'` — run `0eaa1771-eadc-4ace-9c30-4ecff92f83ce`, 2 RED on the stale-result assertions. |

## Focused final GREEN receipts

- `cargo nextest run -p verter_workspace --test main -E 'test(project_membership_old_module_path_is_absent) | test(parallel_input_resolution_policy_cannot_enter_workspace) | test(fact_version_value_graph_is_semantic_owned_and_workspace_reexported) | test(resolution_identity_value_graph_is_semantic_owned_and_workspace_reexported)'`
  — run `a6505669-f758-466b-8a95-fb8700d4a809`, 4 PASS.
- `cargo nextest run -p verter_workspace -E 'test(stay_class_definitions_remain_owned_by_the_workspace_crate) | test(workspace_ingress_uses_only_the_semantic_owned_whole_policy_value) | test(reservation_and_actual_batch_byte_aggregation_reject_checked_arithmetic_overflow) | test(bounded_resolution_preflight_never_enumerates_or_retains_native_directory_entries) | test(ratified_outer_churn_boundary_runs_the_ninth_attempt_and_rejects_only_its_restart) | test(explicit_project_churn_rejection_discards_the_superseded_result) | test(conditional_commit_restarts_share_one_churn_ledger_and_a_new_request_resets_it) | test(permanent_input_load_unavailable_is_immediate_terminal_and_not_retried)'`
  — run `c00565b5-2c31-4562-b801-d8e0229465ce`, 8 PASS.
- `cargo nextest run -p verter_session -E 'test(host_config_shape_has_no_module_resolution_budget_ingress) | test(indexed_present_template_class_lane_binds_a_request_bound_context) | test(cold_seed_template_class_lane_binds_a_request_bound_context)' && cargo check -p verter_session --lib`
  — run `bcb2ae87-0c79-4042-9f7d-594edf3e22ba`, 3 PASS; production-only library check PASS.
- Retry preflight/load matrix — run
  `128dd077-bbf9-40c0-bd55-10b55a9f4211`, 4 PASS.
- Restored churn/stale-output pair — run
  `fcc733fd-a3ec-4a25-93f8-f569efafca2f`, 2 PASS.
- Provider commit-conflict/reset plus final transient/permanent retry cases — run
  `9e52324e-7835-42bf-8d01-6c02076d6f68`, 5 PASS.
- Semantic fact-value source owner/absence guard and its item-vs-mention
  discriminator — run `6dcb4f9d-5e2e-46b4-9380-5e2f989c4466`, 2 PASS.
- Request-bound fixed-view/O(1)/publication-fence matrix — run
  `a3fe669d-ed69-468a-9c6d-d608c2850e36`, 12 PASS.

## Structural source audit

The final audit command is:

```sh
rg -n '(^|[^[:alnum:]_])ProjectResolver(::|[^[:alnum:]_])|NativeProjectResolver|pre-transition|pretransition|phase archaeology|bare-host|bare host' \
  crates/verter_semantic/src crates/verter_workspace/src crates/verter_session/src -g '*.rs'
```

It returns no production/test-module matches. Deleted-name compile-fail fixtures
and historical deliberation records intentionally retain historical names.

## Broader verification

- First affected closure:
  `cargo nextest run -p verter_semantic -p verter_workspace -p verter_session`
  — run `6f57677f-014d-43d3-9a44-b1e64ae57d80`; 6,038 executed,
  6,037 PASS, one O(1) RED, 5,015 not run because fail-fast was active. This
  was the TDD discovery described above.
- Non-fail-fast affected closure after that fix:
  `cargo nextest run -p verter_semantic -p verter_workspace -p verter_session --no-fail-fast`
  — run `92cff7b1-1657-47a9-936b-1e242f47d951`; all 11,053 selected tests
  executed, 11,050 PASS, two source guards RED, and one unrelated large
  trybuild lane timed out at 360 seconds under suite contention. All semantic
  and workspace tests passed, including the new C1 compile-fail cases.
- The two guard failures were intentionally resolved, not allowlisted away:
  request lifecycles now clone the fixed request view rather than opening a
  production raw-view escape, and the single-spec inventory follows
  `SessionRequestLifecycle` behind the public alias. Run
  `f9893a76-5229-4156-8360-37e707bcd101`: 2 PASS.
- The timed-out trybuild lane was rerun alone:
  `cargo nextest run -p verter_session -E 'test(hot_materialize_and_script_fact_structural_rails_smoke)' --no-capture`
  — run `cfd60993-e095-45f3-aa62-651a23b5d38f`, PASS in 76.277 seconds;
  all ten fixtures passed.
- Final-state session closure:
  `cargo nextest run -p verter_session --no-fail-fast` — run
  `ad8d5d0d-2f18-42ff-954f-b4dfecb8a583`, 8,218/8,218 PASS, 570 skipped.
- Full source policy:
  `cargo nextest run -p verter_source_policy_gate --no-fail-fast` with a
  temporary Git index reflecting the two pending file deletions — run
  `2262aff8-4196-497f-9ec7-9b91a39e0619`, 189/189 PASS. A temporary index was
  necessary because the sandbox refused the real worktree `index.lock`; the
  policy's tracked-path scan otherwise sees deleted paths until staging.
- Exact prospective-index portability check: all modifications, additions, and
  deletions were staged into a temporary index/object store, followed by
  `git diff --cached --check` and
  `cargo nextest run -p verter_source_policy_gate -E 'test(tracked_files_contain_no_machine_specific_path_markers)'`
  — diff check PASS; run `b2c2ebf0-af95-4125-a61b-4d27b4e8c762`, PASS. The
  real index and repository object store were not mutated.
- `cargo check --workspace --all-targets` — PASS in 54.41 seconds.
- `cargo clippy --workspace --all-targets -- -D warnings -A clippy::unnecessary_lazy_evaluations -A clippy::needless_update -A clippy::result_large_err -A clippy::large_enum_variant -A clippy::manual_saturating_arithmetic -A clippy::manual_is_multiple_of`
  — PASS in 1 minute 33 seconds. The six `-A` entries are the documented
  existing workspace exclusions; no new exclusion was added.
- `cargo fmt --all -- --check` — PASS.

The canonical gate, performance benchmark, final reviews, verifier, and landing
remain out of scope for this round and were not run.

## Commit state

The verified implementation was committed as
`9ce73638645a2884f18ec902372f6e6b89dabb83`. The earlier worktree-index lock
denial occurred during pre-commit source-policy validation; final staging and
commit creation succeeded. No authority ruling blocks the fix.
