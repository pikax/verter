# Snapshot-Consistent Batched Input Loading Contract

**Status:** Normative outer-orchestration contract.

# 1. Kernel rule

The compiler, resolver, TypeInfo, flow, and reusable query kernels consume one immutable observation view per attempt and perform no hidden filesystem/network/process/package-manager I/O.

# 2. Outcome

```rust
enum AttemptOutcome<T> {
    Complete(T),
    NeedInputs(LoadSet),
    Terminal(AttemptFailure),
}
```

`LoadSet` is normalized, sorted, deduplicated, and includes the resolution basis needed to load/commit safely. The kernel discovers all independently reachable missing observations it can identify without fabricating semantic answers.

# 3. Orchestration state

```text
attempt number
current snapshot identity
accumulated requested InputKeys
accumulated stable positive/negative observations
unique input count
loaded byte count
dependency depth/frontier
basis-change/churn count
```

# 4. Algorithm

1. Run the whole operation against snapshot `S`.
2. On `Complete`, validate observed facts against `S` and return/admit according to the query contract.
3. On `Terminal`, return typed failure and never admit complete state.
4. On `NeedInputs(L)`, normalize `L` and calculate `delta = L.keys - accumulated_requested`.
5. If `delta` is empty and neither the resolution basis nor any previously observed fact changed, return `InputResolutionNoProgress` with the unresolved set.
6. Check configured limits before I/O: attempts, unique keys, bytes, dependency depth, and basis-change/churn count.
7. Load `delta` through embedding-owned same-key I/O flights. Each result is `Present`, stable `Missing`, or transient `LoadFailure`, with typed metadata and basis. Public/external loader digests are hints: the committing authority verifies key/basis consistency and computes or verifies content/configuration fingerprints from captured data before publication.
8. Conditionally publish all validated commit-eligible observations as one coherent input batch only if the project/configuration basis remains compatible.
9. If conditional commit loses a race or the basis changed, capture the new coherent snapshot, increment churn budget, and restart. Do not splice data into the old attempt.
10. If committed, capture the new snapshot and restart from step 1.

# 5. Direct project mode

A direct/project `CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns `NeedInputs`; the caller may rebuild/extend the environment and retry. The same no-progress/resource rules apply to convenience orchestration APIs.

# 6. Observation trust boundary

- public environments/loaders cannot mint authoritative `ContentId`, `InputBasisId`, semantic read facts, or completeness evidence;
- source bytes, declared language/source type, canonical source identity, package/config metadata, lengths, and supplied digests are consistency-checked at capture/commit;
- the authoritative owner hashes/normalizes once per committed revision and reuses the typed IDs thereafter;
- a sealed first-party `EngineSnapshot` may carry prevalidated IDs because the same authority minted them;
- mismatched key/bytes/profile/basis is a typed integrity failure and is never cached as stable missing or complete.

# 7. Negative facts

A stable missing module/package/file may be observed and cached only with the complete resolution basis: parent/package boundaries, conditions, path/case/symlink/workspace policy, and relevant configuration. Transient permission/network/process/provider failure is not a stable negative semantic fact.

# 8. Resource failures

Distinct failures include:

```text
InputResolutionNoProgress
InputResolutionAttemptLimit
InputResolutionUniqueKeyLimit
InputResolutionByteLimit
InputResolutionDepthLimit
InputResolutionChurnLimit
InputLoadUnavailable
InputCommitConflictExceeded
```

They carry unresolved keys and consumed budget without exposing sensitive ambient paths beyond the product's diagnostic policy.

# 9. Tests

- multiple missing siblings loaded in one batch;
- transitive dependency waves;
- stable missing negative fact;
- dependency appears between attempts;
- project/config changes during load;
- repeated same `LoadSet` no progress;
- loader partial/transient failure;
- unique key/byte/depth/retry/churn limits;
- external observation digest/key mismatch rejected before commit;
- no semantic kernel I/O instrumentation;
- final result equals an equivalent fully preloaded clean run.
