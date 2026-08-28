# C1 input-resolution budget authority

## Dispatch record

- Input ID: `C1-INPUT-RESOLUTION-BUDGET-POLICY-RATIFICATION-2026-08-26-01`
- Model: `gpt-5.6-sol`
- Reasoning effort: `xhigh`
- Transport: Codex CLI `codex exec`, read-only sandbox
- Exit status: `0`
- Reviewed candidate: `c0d1d4108b856832dcd84b346c1907ebdca84c41`
- Reviewed tree: `44abbd2259231505adcf2900ebcf21214c916a40`
- Prompt SHA-256: `be6a2463206d505d1678c76f8832d26497b1c4f54d7e2db519ff838c2858a4d4`
- Raw output SHA-256: `b42975247cb6135e7bd325888edbcf55eb6f7c735e05cee12f017c89c713d402`

The decision below is the complete final answer in the raw output, with its machine-local budget-envelope link normalized to a portable artifact label.

The evidence is sufficient. I ratify option C as the complete C1 input-resolution budget policy.

The requested commit/tree identities are exact and the worktree is clean. The APM-001 production blobs are unchanged since the blind review. The external budget-envelope report hashes reproduce, its v2 instrumentation records preflight immediately before loader invocation, and the raw data independently reproduces:

- Successful maximum: 5 attempts, 66 unique keys, 2,950 bytes, 4 accepted waves, 1 restart.
- A6: 4,344 identical driver rows at 3 attempts, 14 keys, 361 bytes; 16,584 world rows with zero retries/churn.
- Compatibility surface: 49 explicit and 10 rest-default public option literals.

Option C is selected because it preserves the existing observable 256-attempt ceiling while adding meaningful hard bounds with substantial downstream headroom. A or B would lower that existing ceiling without a representative downstream corpus.

## Ratified policy

| Meter | Type | Inclusive maximum | Charged action |
|---|---:|---:|---|
| Attempts | `u32` | 256 | Each kernel invocation |
| Unique keys | `u32` | 1,024 | First occurrence of a complete `InputKey` identity |
| Input bytes | `u64` | 1,048,576 B | First-key spelling plus every flight reservation and returned metadata |
| Driver depth | `u32` | 64 | Successfully loaded, basis-compatible `NeedInputs` waves |
| Churn | `u32` | 8 | Basis/conditional-commit restarts |

All maxima are inclusive: prospective `<= maximum` is admitted; `> maximum` is rejected before the action or unrestricted I/O. Projections use widened checked arithmetic; overflow is a breach.

The existing failure scalar carries the consumed value before the rejected action. The rejected prospective value and configured maximum are recorded in the budget audit event, but are not added to the ledger. Rejected unresolved keys are the normalized rejected delta; attempt/churn failures carry the last pending unresolved delta.

`InputKey` identity is its complete `Eq`/`Ord` identity and persists across basis changes. Supported spelling charges are UTF-8 byte lengths of `PathProbe.path`, `RealPath.path`, and `PackageManifest.directory`; two different variants with the same spelling are two keys and charge twice.

The byte charge is:

- first-seen key spelling, once per operation;
- every flight’s authoritative payload-plus-metadata reservation, including retries;
- one byte for a `PathProbe` result;
- zero or the returned realpath UTF-8 length;
- raw `package.json` byte length, zero for stable missing;
- every returned directory-observation spelling and the manifest path.

Reservations are consumed immediately before loading and never refunded. Actual capture must be complete and no larger than its reservation.

## Depth, churn, and precedence

Driver depth is the accepted-wave ordinal. Explicit frontier ancestry is rejected for C1: `LoadSet` has no ancestry carrier, and adding one would broaden the semantic/API contract without evidence. The separate path-local project-reference recursion fuse remains 256, retains its sibling-deferral behavior, and neither consumes nor substitutes for driver depth.

The ledger starts once per independent import-resolution operation, outside outer world/commit retry loops. It persists across kernel waves, transient retries, basis resets, and conditional-commit retries. A later independent request starts fresh.

Initial basis acquisition and successful commit are free. An incompatible basis or permitted failed-commit restart consumes one churn unit exactly once. Provider-specific commit exhaustion remains `InputCommitConflictExceeded`; it is never renamed as churn. If that provider cap refuses a restart, it wins before the driver considers another churn charge.

Failure precedence is:

1. Attempt limit before another kernel invocation.
2. A terminal result from an allowed kernel invocation.
3. `InputCommitConflictExceeded`, then churn, before a restart.
4. No-progress for an empty unchanged delta.
5. Unsupported-loader capability.
6. Driver depth, unique keys, then bytes.
7. Reservation/actual integrity failure during bounded loading.

Thus a supported delta simultaneously breaching depth, unique keys, and bytes reports `InputResolutionDepthLimit`.

## Ownership, ingress, and loader seam

`verter_semantic::resolver_core::InputResolutionBudgets` is the sole immutable policy carrier and owns `RATIFIED`/`Default`. Its five fields are validated in `1..=RATIFIED`; overrides may tighten the policy but cannot disable or raise a limit. The workspace owns only the operation ledger.

Do not add fields to `FilesystemOptions` or `MemoryOptions`: that would break the 49 explicit literals and unknown downstream users. Preserve both existing `new(options)` constructors, delegating to the default policy, and add additive whole-value override constructors:

- `FilesystemWorkspace::new_with_input_resolution_budgets`
- `MemoryWorkspace::new_with_input_resolution_budgets`

Both pass the same semantic-owned value into `Engine`. No `HostConfig`, FFI, NAPI, semantic-fuse, or second-table duplicate is permitted.

The authorized minimal loader change replaces the unrestricted closure with:

- `WorkspaceRead::preflight_resolution_inputs_bounded`, returning a reservation batch bound to the exact normalized keys and `ResolutionBasis`;
- `WorkspaceRead::load_preflighted_resolution_inputs`, accepting that reservation and returning the same key/basis identity plus actual payload and metadata counts.

Preflight may perform bounded metadata/stat work but may not read or parse unrestricted payloads. Package reads are capped at the reserved raw length; metadata collection is bounded by its reservation. The driver verifies reservation sums before loading and verifies key, basis, completeness, and `actual <= reserved` afterward.

Add typed `InputLoadIntegrity` failure with reasons for key-set mismatch, basis mismatch, actual-over-reservation, and incomplete bounded capture. It is terminal and non-cacheable. This public enum widening must be documented as an intentional API migration, not hidden.

Only `PathProbe`, `RealPath`, and `PackageManifest` are supported in this C1 loader. Before preflight, reject the first unsupported normalized key without charging or I/O:

- `FileContent` → `ObservationUnavailable(WholeHash)`
- `DeclBody(Type|Value)` → `ObservationUnavailable(TypeDecl|ValueDecl)`
- `ModuleAugmentationIndex` → its corresponding observation kind
- `FlowFunctionSkeleton` → `ObservationUnavailable(FunctionBodySkeleton)`

Default bounded methods for downstream `WorkspaceRead` implementations must fail this way; they must never fall back to the old unrestricted methods or assign a zero-byte charge.

Retryable load failure keeps the key unfulfilled and may retry only while another attempt and byte reservation fit. Its unique spelling remains charged once; every retry recharges its reservation. Non-retryable unavailability returns `InputLoadUnavailable`. Any five-limit breach, unsupported capability, or integrity failure receives no same-operation retry.

Every limit calls `note_resolution_budget_exhausted` before escape. Limit results remain `NonCacheable(BudgetExceeded)`: no positive or stable-negative candidate, reverse-index entry, fact signature, or persistence admission. Existing facades may continue mapping limit failure to `Ok(None)` only while retaining that non-admission. A fresh request recomputes cold.

## Required tests and mutations

Implementation requires independent inclusive-boundary and breach tests for all five limits, plus:

- cumulative and single-wave unique/byte cases;
- repeated-flight charging and reservation-not-refunded;
- loader-not-called on depth/unique/byte/churn rejection;
- oversized manifest rejection before file read/parse;
- reservation identity, actual-over-reservation, incomplete-capture, and arithmetic-overflow failures;
- all four unsupported `InputKey` families, including a mixed supported/unsupported delta with zero I/O;
- ledger persistence across basis and commit restarts, and reset on a new operation;
- accepted-wave depth independent of project-reference recursion;
- exact failure precedence for multi-breach deltas;
- default and tightening-only override ingress while all 49 explicit literals remain compiling;
- cache/reverse-index non-admission and a later cold request succeeding;
- transient retry success and terminal budget non-retry.

Each production check needs a mutation that removes or moves that exact check and makes its named test red. Additional mutations must catch post-I/O-only byte checking, ledger reset on basis change, reservation refund, unsupported-zero charging, integrity-to-stable-missing conversion, a second budget table, and a duplicate `HostConfig` ingress.

## Operative acts

1. Trunk must add `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-INPUT-RESOLUTION-BUDGET-AUTHORITY.md` containing this decision.
2. Register it as `RULING-2026-08-26-C1-INPUT-RESOLUTION-BUDGET-AUTHORITY` with its exact SHA-256 and append that ID to the existing single C1 authorization. Do not create or replace an authorization or disturb the A6/GAP-3 successor fields.
3. Append the policy/implementation obligation to C1 program state, keep C1 `IN_PROGRESS` and all reviews `PENDING`, and do not pre-stamp a candidate identity.
4. After the candidate inherits that trunk act byte-exactly by clean rebase, APM-001 implementation may resume under TDD within the scope above.
5. Re-pin the implemented candidate SHA/tree, refresh freeze and evidence, run the mutations and canonical gate, and rerun exact A6 evidence and all three exact-candidate reviews. This production change voids the current exact-subject wall waiver unless a new measured result passes or receives new authority.
6. APM-002 and APM-003 remain separate unresolved review findings; this ruling does not dispose of them.

```text
LANE: c1-input-resolution-budget-policy-ratification
REVIEWED_SHA: c0d1d4108b856832dcd84b346c1907ebdca84c41
VERDICT: PASS
POLICY: Option C; inclusive maxima attempts=256 invocations, unique_keys=1024 complete InputKey identities, bytes=1048576 B charged as first-key spelling plus every bounded-flight reservation and evidence metadata, depth=64 accepted NeedInputs waves, churn=8 basis/commit restarts; only tightening whole-value overrides
BLOCKERS: none
OPERATIVE_ACTS: 1. Land this ruling on trunk. 2. Digest-register it and append it to the sole C1 authorization. 3. Record implementation-pending policy authority in C1 program state. 4. Rebase and inherit byte-exactly. 5. Implement the bounded preflight/ledger policy test-first. 6. Re-pin, remeasure, mutate, gate, freeze, and rerun all exact-candidate reviews.
RATIONALE: Exact v2 and A6 evidence establishes maxima of 5 attempts, 66 keys, 2950 B, 4 accepted waves, and 1 restart; Option C preserves the existing attempt ceiling and supplies 15.5x–355x new-limit headroom while remaining finite, pre-I/O enforced, cache-ineligible on breach, compatible with 49 explicit option literals, and separate from semantic recursion.
```
