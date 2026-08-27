# C1 final review round 3 — comprehensive fix evidence

## Subject and scope

- Starting candidate: `ef07e5f6189311ce939da39de2466de8e724eead`.
- Corrective candidate: the commit containing this report; record its immutable
  SHA/tree from Git after commit.
- Scope: the eight unique blockers in the three round-3 reports, within C1's
  ratified resolver contract.
- Excluded: C2/C4 implementation, C-train redesign, performance optimization,
  performance measurement, canonical final gate, integration changes, and
  authority/ledger edits.

No performance benchmark, check, or measurement was run in this fix round.

## Finding closure

| Finding class | Current implementation | Discriminating evidence |
|---|---|---|
| Prohibited source scanner | Deleted `c1_fact_value_move_source_absence.rs` and its registration. Fact values and the input-resolution policy are enforced through cross-crate nominal identity, semantic dependency direction, and the sealed observation interface. | `fact_value_ownership` compares the exact semantic/workspace nominal types and constructs the workspace through the semantic-owned whole policy value. |
| Stale evidence | `ac-map.md`, `landing-subject.md`, both A6 receipts, and the round-2 report explicitly mark old identities, implementation descriptions, performance results, and authority applicability as historical. | The operative map names the current candidate as unmeasured/unaccepted and does not transfer exact-subject performance dispositions. |
| Explicit-overlay fail-open | All three explicit-overlay adapters return the view-authoritative materializer result directly. A materialization refusal returns `None`/unknown and cannot call a base fallback or inherit base publication status. | Adapter behavior tests count fallback calls: explicit refusal observes zero; an unmasked request observes one. Sibling adapters were audited for the same branch shape. |
| Source archaeology | Resolver/session comments state current invariants. The request-view test module is `request_view_reuse_tests.rs`; numbered phase, review, migration, stale fallback, and historical fix narration was removed from the reported class. | Targeted source audit plus the renamed request-view suite. No vocabulary scanner was added. |
| Parsed-edge retry ledger | Parsed-edge and parsed-edge-plus-exact recorders retain one `InputResolutionLedger` across capture and conditional-commit restarts. The private `0..8` outer policy is gone; every restart charges canonical inclusive churn. | Tightened churn runs two attempts and rejects the second restart without stale parsed-edge admission. Default churn runs the ninth attempt and rejects the ninth restart with the exact `8 -> 9` meter event; a fresh operation succeeds. |
| Kernel construction bound | `ResolverAttemptView` carries the semantic-owned policy into every candidate frontier. Frontiers accumulate unique keys incrementally, reject growth at the canonical unique-key maximum before returning an oversized `LoadSet`, path targets are produced lazily, and `AttemptOutput` retains first-seen unique output. | A real kernel with excessive matching targets terminates at the tightened maximum. A real kernel with 2,048 duplicate targets produces the same output as one target. Retained-frame normalization reuse stays green. |
| Operation-local shared writes | Bounded loaders return parsed payload/manifest entries without publishing snapshot/package-index state. The operation ledger stages entries across input waves, clears them on restart, and publishes them only after complete integrity and final cacheable commitment. Resolution output/reverse-edge publication remains behind the same final fence. | Positive/negative late-integrity, later unique-key-limit, native-payload integrity, and positive/negative outer-churn tests all keep the inspected shared stores cold; reverse edges and resolution candidates remain absent after terminal failure. |
| Policy structural discriminator | Deleted the nominal-mismatch compile-fail fixture. Workspace re-exports the exact `verter_semantic::resolver_core::InputResolutionBudgets` type, and behavior consumes that whole value at every operation ingress. | Reversible production plant: add a second private `Engine` budget field at defaults and route both parsed-edge recorders through it. The tightened parsed-edge test turned RED with hook calls `9` instead of `2`; after exact restoration it returned GREEN, while the nominal identity control also remained GREEN. |

## TDD and mutation receipt

- Overlay adapter test first failed to compile because the fail-closed selector
  did not exist; the production selector then made explicit refusal and base
  fallback behaviors green.
- Real-kernel budget tests first failed to compile because the resolver attempt
  view had no budget-aware constructor; budget propagation and incremental
  frontier construction made the tests green.
- Workspace staging/retry tests first failed to compile because the parsed-edge
  pre-commit hook and operation-local commit rail did not exist; the shared
  ledger and staged commit implementation made them green.
- The required planted second internal budget table was a real production
  mutation, not a source token or nominal fixture. Command:
  `cargo test -p verter_workspace --lib parsed_edge_commit_restarts_share_one_churn_ledger_and_discard_stale_edges`.
  RED: assertion observed `left: 9`, `right: 2`. The plant was removed exactly;
  rerunning the command passed 1/1. The nominal control
  `input_resolution_policy_is_the_exact_semantic_owned_nominal_value` passed
  1/1 after restoration.

## Verification

The completed non-performance verification was:

- focused resolver/kernel, parsed-edge, overlay, request-view, nominal-identity,
  and mutation controls: all passed, including the plant `RED` described above
  and restored controls of 1/1 each;
- `verter_semantic`, deterministic single-thread library suite: 1,998 passed;
- `verter_workspace`, deterministic single-thread library suite: 830 passed;
- `verter_workspace` integration/compile-fail suite: 12 passed;
- `verter_session`, deterministic single-thread library suite: 5,884 passed,
  537 ignored, zero failed (6,421 total);
- `verter_source_policy_gate`: 187 passed;
- workspace nextest iteration: 25,498-test universe across 86 binaries, with
  588 listed as skipped; failure-only output reported exactly two failures,
  both unchanged `verter_compiler` `trybuild` stderr fixtures outside C1:
  `pending_nav_request_is_unreachable_outside_vapor` and
  `segmented_overwrite_authority_is_unreachable_outside_the_crate`. The final
  nextest summary buffer was not returned by the execution session, so this
  report does not invent a final passed count. Exact isolated runs with
  `--features bench` reproduced both stale privacy-diagnostic expectations;
  neither fixture was modified;
- `cargo check --workspace --all-targets`: passed;
- `cargo fmt --all -- --check`: passed;
- literal `cargo clippy --workspace --all-targets -- -D warnings`: after fixing
  every changed-file warning, stopped only on three pre-existing
  `clippy::unnecessary_lazy_evaluations` findings in unchanged
  `resolution_snapshot.rs`; the established workspace strict lane with the six
  recorded baseline exclusions and `-D warnings` passed;
- live Rev11 program-state validation: 69 blocks validated; and
- diff/source-policy hygiene: passed after the final explicit staging pass.

The canonical final gate and every performance benchmark, check, and
measurement remain intentionally unrun in this fix round.

## Acceptance boundary

This report is implementation evidence, not a review, verifier, performance
disposition, canonical-gate, landing, or acceptance receipt. All such gates
must bind the immutable commit containing this report.
