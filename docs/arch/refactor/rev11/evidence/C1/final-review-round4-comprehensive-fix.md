# C1 final review round 4 — comprehensive fix evidence

## Subject and scope

- Starting candidate: `5b27759f7b73a95b01a1a59ac064de93377cef93`.
- Corrective candidate: the commit containing this report; obtain its immutable
  SHA and tree from Git after commit.
- Scope: the six unique blockers in the three round-3 reviews, within the
  ratified C1 resolver contract.
- Performance: no benchmark, performance check, or measurement was run.

## Corrective behavior

1. `final-squash-message.md` and `suite-results.md` are independently marked
   historical and superseded. They explicitly prohibit transferring their
   exact-subject measurements, waivers, or landing instructions to this
   production-changed candidate. Other independently consumable landing-facing
   artifacts were checked for the same stale-authority failure.
2. A real `SessionView` refusal now crosses `SessionResolverContext`,
   `SessionQueryHostPort`, component-meta capture, and routed-shallow lookup
   while a usable base artifact exists. Every explicit-overlay route returns
   the refusal; the base artifact remains usable only through a base request.
3. Cumulative resolver source comments under `crates/`, `packages/`, and
   `scripts/` were diff-audited and rewritten as present-tense ownership or
   discrimination invariants. No vocabulary scanner was added.
4. Each filesystem import or parsed-edge operation creates one canonical input
   ledger outside discovery/replay and all frozen-evidence retries. Discovery
   payloads are discarded without resetting charges; each failed outer
   validation charges one inclusive churn restart. No private fixed-attempt
   loop competes with that canonical operation policy.
5. Matching workspace aliases are deduplicated and bounded before mapping
   geometry or probe arrays are built, on both frame and non-frame kernel
   paths. Completed-miss output merges use hash-backed first-seen sets and the
   same semantic `unique_keys` allowance, eliminating quadratic `Vec::contains`
   growth and refusing oversized fully observed witnesses.
6. Frozen evidence is revalidated inside the Engine publication lock, after
   computation and immediately before any shared mutation. Normal candidates,
   decisions, lazy/reverse edges, parsed/exact edges, loaded payloads, package
   manifests, and positive/negative package-index entries therefore remain
   cold when final validation refuses.

## Discriminating tests and mutations

- Overlay production-site plant: append
  `.or_else(|| host.ensure_indexed_ready_serve(canonical_id))` at
  `overlay_priority::ensure_indexed_ready_serve_with_view`. The real
  request-bound test turns RED at the `SessionResolverContext` assertion;
  restore returns GREEN.
- Final-transaction plant: bypass the Engine's `final_validate()` guard. The
  filesystem import test turns RED after only the discovery/replay pair rather
  than the two charged retries and would expose premature shared publication;
  restore returns GREEN.
- Alias-bound plant: bypass `alias_limit_exceeded` before the frame frontier.
  The real-kernel test turns RED because a mapping candidate is evaluated
  before terminal refusal; restore returns GREEN.
- Completed-miss plant: replace `AttemptOutput::merge_bounded` with unbounded
  `merge` in the miss arm. The distinct fully observed miss test turns RED
  because it completes instead of returning the canonical unique-key terminal;
  restore returns GREEN.
- Ledger reset control: moving either ledger construction into the filesystem
  attempt loop removes the one `Churn { consumed: 1, prospective: 2 }` event
  asserted by the wrapper-level test. Moving the churn charge after `continue`
  has the same discriminating failure.
- Parsed/exact transaction controls: moving final validation after the Engine
  mutation makes the snapshot, package index, forward/reverse edges, or exact
  result non-cold in the combined wrapper-level test.

## Verification

The final production tree passed:

- discriminating targeted regressions: 27 passed (13 semantic-kernel rows,
  two filesystem outer-transaction rows, 11 resolution concurrency-contract
  rows, and one real session/request overlay-refusal row);
- `verter_semantic` library: the four-thread run reported 1,998 passed and two
  failures caused by the shared test-only `NORMALIZE_CALLS` counter; each named
  failure passed in exact isolation, for 2,000 independently completed tests
  with zero reproducible failures;
- `verter_semantic` integration/compile-fail: 3 passed;
- `verter_workspace` library: 832 passed;
- `verter_workspace` integration/compile-fail: 12 passed;
- `verter_session` library: 5,885 passed, 537 ignored, zero failed (6,422
  total);
- `verter_source_policy_gate`: 187 passed;
- `cargo check --workspace --all-targets`: passed;
- `cargo fmt --all -- --check`: passed; and
- the workspace/all-targets clippy lane with `-D warnings` passed after fixing
  every changed-file warning and applying only the five existing C1 baseline
  exclusions (`result_large_err`, `large_enum_variant`,
  `manual_is_multiple_of`, `manual_saturating_arithmetic`, and
  `too_many_arguments`). The literal no-exclusion command stops on those 84
  already-recorded workspace warnings; it reports no changed-file warning.

Before every affected-package, check, and clippy compilation, the Grok J1
worktree was checked for Cargo, rustc, nextest, and Node/pnpm test descendants;
each check found a quiet window. The canonical final gate, workspace-wide
25,498-test iteration, and every performance benchmark, check, and measurement
were intentionally excluded from this fix round.
