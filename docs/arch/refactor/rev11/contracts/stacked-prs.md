# Stacked Pull Request, Restack, and Atomic Landing Contract

**Status:** Normative delivery contract.  
**Architecture authority:** `program-dag.toml`, never PR topology.

# 1. Decision

Use a single PR when one bounded reviewable candidate is enough. Use a short stack when dependency ordering or an atomic cutover is materially easier to review in layers.

A stack is transport. It cannot add, remove, or reorder program predecessors, split a program acceptance unit silently, or make an incomplete cutover releasable.

# 2. Stack window and immutable snapshot

Before creating branches, copy `templates/stack-window.template.toml`, resolve it, and validate it with `tools/validate_stack_window.py`.

The mutable window records `program_state_basis_digest`, the digest of the last validated program state **before** that stack is attached. `previous_stack_snapshot_digest` links a restacked successor to the prior immutable snapshot, or is `NOT_APPLICABLE` for the first window. These one-way references avoid hash cycles.

Every layer has a unique `layer_id`; `block_id` names the program block whose charter/acceptance unit owns it. Every review and CI event binds to an immutable **StackSnapshotId**, defined as the SHA-256 of the fully resolved validated stack-window file. The file contains every exact base/head/tree, patch, generated-output, and evidence digest. Any changed input creates a new snapshot and invalidates affected approval.

After a snapshot is attached, current `program-state.toml` stores the stack ID, StackSnapshotId, and the program block's final/current layer index. Same-block private sublayers remain detailed in the stack window rather than becoming duplicate program-ledger blocks; an explicit program block such as `D1` retains its own `PRIVATE_CHECKPOINT` state. The pre-stack basis file is never rewritten. For `ACTIVE`, `FROZEN_FOR_REVIEW`, and `LAND_READY` windows, `tools/validate_stack_window.py --current-program-state ...` cross-validates the mutable ledger against the immutable snapshot. A PR label or web UI is not sufficient state.

A stack-window snapshot is not the landing receipt. Actual landing is recorded by the block's validated landing-equivalence proof and the subsequent program-state transition. A used or invalidated stack snapshot is retained as evidence and replaced by a new window when work remains.

# 3. Modes

## 3.1 `LANDABLE`

Use only when every layer is independently safe and releasable. The layers form one connected dependency path; true DAG siblings use separate windows. `acceptance_block_id` is empty and each `block_id` appears once.

Each layer:

- maps to one accepted block charter;
- has every external semantic predecessor accepted and may depend on an unaccepted predecessor only when that predecessor is a lower layer in the same validated snapshot;
- preserves one production implementation and every current Supported/Stable contract;
- passes its own charter checks on the cumulative tree;
- contains its own required deletions and compatibility handling;
- may land bottom-up without leaving a transitional public state.

An upper layer may be `READY`, `IN_PROGRESS`, or `REVIEW` while a lower in-window predecessor is unaccepted. It may not become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until every semantic predecessor is formally `ACCEPTED`/`PRIVATE_CHECKPOINT`, the lower landing has occurred, the upper layer has been restacked on the new base, and affected checks/reviews have revalidated the new exact candidate.

An upper layer is never accepted merely because the top of the stack is green.

## 3.2 `ATOMIC_REVIEW`

Use when several review-sized diffs collectively form one indivisible clean cutover.

Rules:

- top-level `acceptance_block_id` names the sole program block that may become accepted/landed from this window;
- all layers belong to that atomic acceptance unit and have unique `layer_id` values;
- private layers may repeat the acceptance block's `block_id` as internal checkpoints, or name an explicit `foundational-private-checkpoint` predecessor such as `D1`;
- intermediate layers target a private integration branch, remain draft, are marked `NON_MERGEABLE_PRIVATE_LAYER`, and are unreachable from production entry points;
- no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor, except an explicit program checkpoint such as D1 whose `PRIVATE_CHECKPOINT` state is valid only for the final acceptance block;
- exactly one final mergeable layer routes every consumer, deletes the displaced path/support machinery, and becomes the reviewed candidate;
- the complete combined tip receives the block's required conformance, architecture, and adversarial/performance review;
- landing preserves the exact reviewed candidate delta on the recorded landing base.

`D1`/`D2` is the canonical case: D1 is a private checkpoint; D2 is the sole acceptance and landing unit.

## 3.3 Parallel disjoint work

Parallelism is represented by separate `LANDABLE` or `ATOMIC_REVIEW` windows, not one artificial stack. Each window declares shared owners/files/generated artifacts and integration tests. A newly discovered ownership overlap stops or serializes the affected work.

# 4. Size and lifetime

Default maximum: **four open review layers per stack**.

A6 locks a value from two through six based on reviewer capacity, CI latency, restack frequency, and repository tooling. More than six requires an ADR amendment. A program-wide or fifty-block stack is prohibited.

Split or land a stack when:

- the lowest layer is independently acceptable;
- owner or concern changes;
- a block boundary is crossed without independent acceptance;
- lower-layer churn repeatedly invalidates upper proof;
- the review scope cone no longer fits one bounded invariant;
- the final atomic cutover is no longer understandable as one candidate.

# 5. Branch/worktree ownership

- One writable worktree belongs to one branch and one worker.
- A worker may read but not mutate another worker's worktree.
- Only the orchestrator changes shared stack topology, rebases/restacks shared branches, or resolves cross-layer conflicts.
- Generated files, lockfiles, protocol schemas, central manifests, and dependency-firewall configuration have one active writer lease.
- Branch/PR metadata includes stack ID, mode, layer index, block ID, exact base/head/tree, charter digest, and snapshot digest.
- Accepted evidence never depends on uncommitted or untracked changes.

# 6. CI and proof

Every mergeable layer runs:

1. layer-specific tests/static checks;
2. every charter check applicable to the cumulative tree from stack root through that layer;
3. non-vacuous execution and generated-file cleanliness proof;
4. relevant performance/memory/work gates;
5. dependency, architecture, compatibility, and failure checks required by its block.

The top `LANDABLE` layer additionally runs the declared stack-integration suite. The final `ATOMIC_REVIEW` layer runs full atomic-cutover proof, including one-production-path and deletion assertions.

`LAND_READY` means all mergeable layers are green on the named immutable snapshot and the one currently eligible landing block is `ACCEPTANCE_RECOMMENDED`: the bottom layer for `LANDABLE`, or the final acceptance block for `ATOMIC_REVIEW`. Green upper `LANDABLE` layers remain `REVIEW`, not accepted in advance.

Evidence from an older base, snapshot, toolchain, profile, or corpus is not silently reused.

# 7. Lower-layer change and cascading restack

When a lower layer changes:

1. fix the layer where the defect belongs; never hide it in an upper workaround;
2. restack bottom-to-top;
3. record old/new base SHA/tree, canonical patch digest, range-diff, candidate tree, generated diff, evidence digest, and every manual conflict resolution;
4. set `previous_stack_snapshot_digest` to the replaced snapshot and mint a new StackSnapshotId;
5. mark all affected upper candidates/reviews `INVALIDATED`/revalidation-required;
6. rerun required CI on every new cumulative tree;
7. obtain impact-bounded reattestation from every required review mandate on the new exact candidate/snapshot.

No approval transfers automatically. Tree/patch equivalence can make reattestation small, but the new exact identity must be named.

# 8. Restructuring

Inserting, dropping, folding, reordering, or unstacking a layer requires:

- clean worktrees;
- no affected layer queued/merging;
- updated validated stack window and program state;
- predecessor and mergeability revalidation;
- regenerated PR descriptions/context packets where affected;
- invalidation of changed cumulative candidates, CI, and reviews.

A transport-only linear relation between true DAG siblings must not be created. Separate sibling stacks are required.

# 9. Landing

Legal modes:

- **Bottom-up:** land only the lowest `LANDABLE` layer. Then invalidate/restack every remaining upper layer on the actual accepted base and issue a successor snapshot. If one layer remains, continue as an ordinary single PR.
- **Atomic final only:** land only the final `ATOMIC_REVIEW` candidate; private layers never reach trunk independently.

Before landing, record the reviewed base/candidate SHA/tree and the predicted landing base/target identity. Branch protection and required checks remain binding. A merge queue is preferred where available, but queue admission does not replace exact review.

`candidate_sha/tree` remains the exact cumulative candidate reviewers inspected. `accepted_sha/tree` records the actual landed commit and full repository tree and may differ after a reviewed rebase, squash, merge commit, or merge-queue base advance. A validated `landing_equivalence_digest` proves that the canonical binary Git delta from reviewed base to reviewed candidate exactly equals the delta from accepted base to accepted commit, that generated-output digests match, that no manual conflict resolution occurred after review, and that required post-landing checks passed. If the delta differs, re-freeze and re-review; do not call it equivalent.

A single accepted program block must not be co-batched with unrelated changes in the same landing delta. Foundational or atomic candidates receive a dedicated merge-group/queue position where the repository supports it.

After landing:

- validate `templates/landing-equivalence.template.toml` against the actual repository objects;
- record the proof file's digest in the post-landing program state;
- run required post-merge smoke/consistency checks;
- record actual accepted SHA/tree;
- clear or invalidate every unlanded upper block's old stack binding/review state;
- create a successor stack snapshot from the new validated program-state basis when multiple upper layers remain;
- retain evidence before pruning branches/worktrees.

A stack snapshot itself never transitions a block to accepted. A merged PR is not automatically an accepted block.

# 10. Tooling independence

GitHub native stacks, ordinary dependent PRs, or another reviewed tool may implement this contract. Native stack UI and CLI behavior are operational conveniences, not architecture dependencies.

# 11. Prohibited patterns

- one stack spanning the complete program;
- merging a private replacement checkpoint to trunk;
- an upper-layer workaround for a lower-layer defect;
- two writers force-pushing or editing one branch;
- preserving approval after an unrecorded restack;
- relying only on top-of-stack CI for independently mergeable lower layers;
- merging a layer that leaves two selectable production paths;
- using stack position as semantic predecessor authority;
- allowing an upper block to reach acceptance before its semantic predecessor lands;
- treating full-tree inequality after a base advance as automatic failure or automatic equivalence instead of proving exact candidate-delta equivalence;
- hiding cross-stack dependencies or an unaccepted competing PR.
