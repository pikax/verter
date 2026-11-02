# Representative CI change corpus v1

The versioned input for every before/after CI comparison: selection, build sharing, cache reuse and concurrency. It owns scenarios and collection definitions. Each consumer keeps its own results outside this repository. It is not a measured baseline, a new selector, or permission to dispatch workflows or publish releases.

- **Inputs:** [`scripts/ci-change-corpus/scenarios.v1.json`](https://github.com/pikax/verter/blob/main/scripts/ci-change-corpus/scenarios.v1.json). Every changed-file array, expectation, plant, stream and cell lives there, and nowhere else.
- **Replay:** [`scripts/ci-change-corpus.mjs`](https://github.com/pikax/verter/blob/main/scripts/ci-change-corpus.mjs) answers the scenarios through the existing authorities. Its [tests](https://github.com/pikax/verter/blob/main/scripts/ci-change-corpus.test.mjs) run in `test:scripts:ci`.
- **Commands, metrics and repetition:** the [collection protocol](ci-change-corpus-collection.v1.md).
- **What CI declares today:** the [inventory](ci-inventory/README.md), with its [workflows](ci-inventory/workflows.md) and [storage](ci-inventory/storage.md).

Cause: isolated passing lanes do not show that a change is covered, or where waiting moves when lanes change. Owner and pattern: `ci-impact.mjs`, `check-ci-aggregate.mjs`, `release-proof.mjs` and `ci-junit-metadata.mjs` stay authoritative; the replay only calls them. CI behavior does not change, except that the scripts lane runs the replay's tests.

## Identity and versioning

The corpus identity is `ci-change-corpus/1`. Each replay records the digest of the scenario file and of these two documents with line endings normalized, and `compare` refuses a pair whose digests differ. So:

- A change to a scenario, input, order, expectation or metric definition is a new version: `scenarios.v2.json`, `ci-change-corpus.v2.md`. Do not edit v1 in place.
- Consumers read v1. They never rewrite a scenario to fit what they collected.
- An experiment archives the scenario file it read, because a later version replaces the default path. `replay --corpus <file>` reads an archived copy.
- Results, refreshed counts and runtime identities are never committed. Each run's manifest records its head, tree and configuration digests as runtime facts about what was tested, never as durable proof.

## Scenario kinds

| Kind             | IDs            | Input                                                                   | What qualifies                                                                                                                                          |
| ---------------- | -------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `change`         | `CH01`–`CH17`  | An exact changed-file array, and a release kind where one applies       | The selection and crate closure meet the declared floor. A path array is a selector input only: it compiles nothing and proves no test result.          |
| `selector-plant` | `SP01`, `SP02` | A change, plus dependency edges removed from the real crate graph       | The expectation holds on the true graph and is violated on the planted one. A plant that leaves the answer intact fails as `plant-not-effective`.       |
| `fixture`        | `FX01`–`FX09`  | Existing named tests, unmodified                                        | Each name runs and passes. A name that selects zero work fails. `subject` records what the implementation does: a passing `reject` row refused a plant. |
| `plant`          | `PL01`–`PL04`  | A real failing change applied to a disposable tree, with its obligation | The marker is newly present once, and the obligation fails with the declared output. Passing, or failing for another reason, disqualifies the run.      |

An expectation is a floor (`mustSelect`, `mustImpact`), a few unaffected controls (`mustNotSelect`, `mustNotImpact`) and an answer class:

| Class         | Meaning                                                                                |
| ------------- | -------------------------------------------------------------------------------------- |
| `none`        | No gate runs.                                                                          |
| `narrow`      | Selection came from path filters and the crate closure.                                |
| `impact-full` | A build-wide input turned on every impact-bearing lane.                                |
| `everything`  | An input that changes what CI runs, or a path nobody classified, turned on every lane. |

A replay keeps the complete answer for each sample, so a narrowing within the floor shows up in `compare` as a difference rather than a failure.

## Required coverage

`validateCorpus` refuses a corpus that loses any of these dimensions.

| Dimension                | Entries                                                   | Local replay                                                                           |
| ------------------------ | --------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Passing changes          | `CH01`–`CH13`, `CH15`–`CH17`, passing `FX` rows           | Selection answers and fixtures. Whether the change goes green needs hosted runs.       |
| Failing changes          | `CH14`, `SP01`, `SP02`, `PL01`–`PL04`, `reject` `FX` rows | All of them.                                                                           |
| Concurrent pull requests | `CC01`, `CC02`                                            | Unavailable: see [concurrent streams](#concurrent-streams).                            |
| Superseded pushes        | `CC01`, `CC02`, `FX08`                                    | `FX08` only.                                                                           |
| Shared inputs            | `CH07`–`CH09`, `CH11`–`CH15`, `SP02`, `FX02`              | Selection answers and fixtures.                                                        |
| Platform and features    | `CH02`, `CH03`, `CH10`, `FX07`, every cell                | Selection answers and fixtures. Cells need hosted runs.                                |
| Release and tag          | `CH17`, `FX05`, `FX06`                                    | Selection answers and proof fixtures. A real tag or dry-run event needs authorization. |
| Cold and warm builds     | Cache conditions, producer cells                          | Unavailable: cache state is observed only on the job that restores it.                 |

### Release and tag

`CH17` and `FX06` cover the `release: v<version>` and `release(ide): v<version>` subjects and their `v<version>` and `ide/v<version>` tags. `FX05` covers the `releaseProof` boundary: the landed-tree proof, the release title, a successful rehearsal job and unexpired artifact families. A successful build alone does not exercise that boundary, so an ordinary pull-request replay cannot show a release still publishes. Never publish a version to run these scenarios: `FX05` answers from synthetic API responses.

### Platform and feature cells

Each cell names one job in the workflow that owns it, and `validateCorpus` fails if that job no longer exists. Compare like with like and report each cell separately: OS, architecture, target, profile and features come from the [inventory's](ci-inventory/workflows.md) expanded matrix for the run being measured.

- `ci.yml` runs on Linux only; Windows and macOS are reached only by the release workflows' matrices.
- A native pass is not a wasm32 result. Debug and release LSP results are not interchangeable. A feature-on run never replaces a feature-off or no-debug-assertions obligation.
- A platform with no available runner is a recorded limitation, never emulated as equivalent.

### Cache conditions

The four conditions in the scenario file (`cold`, `warm`, `mixed`, `unknown`) apply per cell and per cache layer. Record exact, partial or miss for each [declared layer](ci-inventory/storage.md) and each artifact download. Warm consumers still run the full required validation; a warm target beside a cold compiler cache is `mixed`. Never delete shared worker caches, force uploads from forks, relax trust or change a dependency pin to manufacture a condition. If a condition cannot be controlled or observed, keep the cell as `unknown`; never infer it from elapsed time.

### Concurrent streams

`CC01` and `CC02` are ordered by observed signals, never by delays. Each event fires on the signal it names; if that signal is never exposed, the row is `unavailable` rather than timed by a sleep. Record every submission, admission, start, cancellation, acknowledgment, ready and landing, including work that finishes after being superseded.

Report service order, admission counts and the maximum bypass count for each surviving pull request. No fairness or time bound is set here. A canceled or superseded push never counts as green, as zero latency, or as a landing, and its work stays in the runner-minute totals.

The local fixtures do not show that a future scheduler or cache adapter rejects these plants. A consumer that adds one replays them through it.

## Reuse of qualification work

The test-selection qualification corpora (DTEST) have not landed, so no DTEST product exists to reuse in this version. These are the parts that exist today:

- `FX07` reuses the JUnit metadata contract, and `FX09` reuses the perturbation and hermetic-candidate checks.
- `FX01` to `FX04` reuse the selector and aggregate fixtures. They are synthetic: `span <- parser <- compiler <- {napi, wasm, lsp}` are not real crates, and they do not measure the workspace.

The corpus's real-crate samples use the same mutation classes as the planned Rust qualification, so results can be mapped when it lands:

| Mutation class                        | Entries                |
| ------------------------------------- | ---------------------- |
| Crate change, with downstream callers | `CH05`, `CH06`         |
| Shared test helper                    | `CH09`, `SP02`         |
| Build script                          | `CH07`                 |
| Proc macro                            | `CH08`                 |
| Included or generated file            | `CH11`                 |
| Config, lockfile, toolchain or cfg    | `CH12`                 |
| Added, deleted or renamed file        | `CH15`                 |
| Interacting parent changes            | `CH16`                 |
| Dependency edge the selector missed   | `SP01`, `SP02`, `FX01` |
| JavaScript or TypeScript failure      | `CH04`, `PL04`         |

When a DTEST corpus lands, adopt it through a new corpus version with an explicit scenario mapping. This corpus does not replace it: selection answers alone never show that a selected subset agrees with the full suite.

## Coverage limitations

- **Only local evidence is delivered.** Hosted timings, spend, ready and landing events, real tag consumers, fork policy and fleet fairness need authorized external data. The inventory's unknown live required contexts stay unknown.
- **Selection is not execution.** `CH` and `SP` answers show which lanes would run, and `PL` runs one obligation. Neither shows that a full required run passes.
- **Plants exercise chosen obligations.** Each `PL` runs the narrowest command that rejects it, not its hosted lane. Never apply one to a branch a publishing workflow can reach.
- **Feature and shared-input mutations stay narrow.** `SP01` and `SP02` remove normal and dev edges. There is no build-edge plant, because the workspace's one build-dependency edge is also reached through other paths; `FX01` covers build edges synthetically. Hatches, features and generated inputs are tested only as answers, and planting them belongs to whoever extends the selector.
- **A deleted crate has a blind spot.** `CH15/deleted-crate-source-only` selects only the crate-wide lanes with an empty closure, because a deleted crate is absent from the new tree's metadata. Removing a crate also changes `Cargo.lock`, so a real deletion falls back to `impact-full` (`CH15/deleted-crate-with-workspace-files`). The narrow sample is kept as the before-answer for closure work over both old and new manifests.
- **Unavailable is not a pass.** Each row a local replay cannot answer is listed in the run's `unavailable` or `notRun` output, with its reason.

## Why each definition exists

| Definition                                    | Concern     | Prevents                                                                                 | Not covered because                                                |
| --------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Versioned inputs with a digest gate           | correctness | Likely false equivalence from comparing different graphs, inputs or metric definitions   | Independent passing lanes never compare the same experiment inputs |
| Floors and edge plants on the real graph      | correctness | A narrowed selector silently dropping a dependent lane                                   | `ci-impact.test.mjs` uses a synthetic graph for closure            |
| Failing plants with proof they applied        | correctness | A negative control reporting a pass because the mutation never landed                    | Exit status alone cannot tell a missed plant from correct code     |
| Release and stale-proof scenarios             | correctness | A tier change leaving a release unable to publish, or an old head proving a new tree     | Ordinary pull-request runs never call the release proof            |
| Concurrent streams                            | correctness | An older success satisfying current work, and hidden starvation under contention         | Serial per-pull-request fixtures never overlap                     |
| Cold and warm conditions, paired repetition   | performance | Mistaking cache state or host variance for an optimization, common in unpaired durations | Test verdicts do not measure compatible cache reuse                |
| Separate queue, execution and landing metrics | process     | Reporting faster green while waiting moves to the final landing queue                    | Job success omits admission and merge waiting                      |
