# C1 final review round 5 — kernel retention fix evidence

## Subject and scope

- Starting candidate: `b38c918117a95042a43a50660ba0f17d8bac832b`, tree
  `699a481743cd795086c57f86132f0139550d4ebb`.
- Corrective candidate: the commit containing this report; obtain its immutable
  SHA and tree from Git after commit.
- Scope: the six consolidated round-4 blocker classes under the ratified C1
  resolver contract and the kernel-retention authority ruling.
- Performance: no benchmark, performance check, or measurement was run.

## Corrective behavior

1. `unique_keys` is used only by the workspace operation ledger for the
   first-seen complete `InputKey` identity. Alias rows, candidate geometry,
   frontier output, and completed witnesses do not charge or emit that meter.
2. `InputResolutionBudgets` is the sole seven-field semantic policy carrier.
   Its five-field ingress selects ratified defaults for alias-geometry and
   completed-witness retention; the named seven-field ingress validates every
   nonzero tightening. The workspace operation ledger owns one shared live
   retention state across attempts, basis changes, and outer retries.
3. Workspace aliases are priority ordered before normalized-target
   deduplication. Executable candidate geometry is built lazily with one live
   bundle at a time, so wide alias and reachable-project tails cannot reject a
   fully observed higher-priority or late hit while live retention stays within
   the inclusive maximum.
4. `AttemptOutput` charges the exact tagged union of facts, ambient
   dependencies, and consumed resolution observations on every insertion,
   clone, and merge path. Duplicates charge zero distinct units; max is GREEN,
   max+1 returns the typed terminal and no partial output publishes.
5. Alias-geometry and completed-witness terminals carry retained, prospective,
   and maximum scalars. The workspace emits exactly one matching exhaustion
   audit event and maps each terminal to `NonCacheable(BudgetExceeded)` without
   same-operation retry or publication.
6. Filesystem base and explicit-overlay retry facades preserve churn-budget
   exhaustion as `BudgetExceeded`, including the final frozen-reader fence.
   Both real facade tests prove resolver caches, snapshots, package indices,
   and dependency edges remain cold.
7. Cumulative resolver-source prose is stated as current ownership,
   compatibility, or discrimination invariants. The cited conversion,
   ownership, and observation examples no longer narrate program/review
   history. No vocabulary scanner was added.

## Discriminating production mutations

- Priority-before-dedup: replaced `sorted_workspace_aliases(aliases)` with
  declaration order. The normalization-collision real-kernel test RED because
  `pkg/` evaluated instead of `pkg/special/`; restore returned GREEN.
- Unique-key independence: planted an alias-row-count check against
  `budgets.unique_keys()`. The wide-alias/high-priority-hit real-kernel test RED
  before the hit; restore returned GREEN.
- Inclusive completed-witness boundary: changed prospective `> maximum` to
  `>= maximum`. The tagged whole-output boundary test RED at retained 1,
  prospective 2, maximum 2; restore returned GREEN.
- Churn terminal identity: rewrote the two base-filesystem outer churn
  failures to `ResolutionRetryExhausted`. The real facade test RED with the
  exact reason mismatch while cold-state assertions remained load-bearing;
  restore returned GREEN. The same final implementation and test cover the
  explicit-overlay facade.
- Pinned alias retention: the production kernel is invoked with one bundle
  already live at max-1 and at max. The boundary evaluates candidates and
  reaches high-water max; max+1 returns the exact typed terminal before the
  candidate-evaluation hook fires.
- Completed-output composition: distinct witnesses are produced across
  separate live outputs and merged in canonical order under one operation
  state. Max is exact, max+1 is terminal, duplicate entries charge zero, and
  discarding an output releases its units for the next prospective charge.

## Verification

The final production tree passed:

- semantic resolver-core targeted lane: 259 passed;
- `verter_semantic`: 2,004 library tests plus 3 integration/compile-fail tests;
- workspace resolution-driver lane: 29 passed;
- filesystem base/overlay churn boundary lane: 2 passed;
- `verter_workspace`: 834 library tests plus 12 integration/compile-fail tests;
- `verter_source_policy_gate`: 187 passed;
- `cargo check --workspace --all-targets`: passed;
- `cargo fmt --all --check`: passed; and
- workspace/all-targets strict clippy with the five recorded baseline
  exclusions: passed.

The full semantic run initially exposed the shared test-only normalization
counter as non-hermetic under parallel tests. It is now thread-local; the full
2,004-test lane passes without isolation.

Before every compile/test/check lane, the Grok J1 worktree was checked for
Cargo, rustc, nextest, and Node/pnpm test descendants, and every lane started
in a quiet window. Grok restarted one targeted J1 test while the already-running
strict clippy lane was finishing; the correctness result remains usable and no
timing claim is derived from it. The canonical final gate, workspace-wide
25,498-test iteration, and every performance benchmark, check, and measurement
were intentionally excluded.
