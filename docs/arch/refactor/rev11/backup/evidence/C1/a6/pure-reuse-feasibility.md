# C1 pure-reuse feasibility diagnostic

## Verdict

`RESULT NO_PATH`

The remaining C1 A6 regression cannot be closed from about `+9.214%` retired instructions to
`<= +3%` solely by the currently authorized request-local pure-geometry, existing-`Arc` identity,
or scratch-capacity reuse. The committed `0c2295382` cleanup is correct, local, and worth retaining,
but it already captures the material storage/identity wins that do not alter the retry semantics.
The remaining hot work is the ordered semantic candidate traversal repeated by the mandatory
whole-operation restart after each `NeedInputs` result.

No further production change is recommended under current C1 authority. Closing the gap requires
architecture authority to change the input-loading restart and attempt-output discard contracts by
introducing a typed, snapshot-revalidated semantic continuation. That is the path the governing
frontier-resume consult already ruled outside C1. Cross-request caching, an unvalidated cursor,
threshold changes, and scope expansion are not alternatives considered by this report.

## Scope and evidence

- Integration base: `d1f3d50a948597f036868543b9bb21acacd730ff`.
- Candidate production commit: `0c22953821f57eedd32b812b1478a449a976f964`.
- Inspected worktree HEAD: `d352a54e022c35f2c65acc3743143d9eda6532b9`; the only commit after the
  production candidate is the docs-only wall-blocker receipt.
- Existing valid protocol-5 comparison:
  `/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/metrics-protocol5.tsv`,
  `/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/aggregates-protocol5.tsv`, and
  `/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/SHA256SUMS.txt`.
- Existing current-candidate recovery artifacts:
  `/tmp/c1-a6-perf-fix.8RC0ee/disabled-diagnostic-0c2295382.txt`,
  `/tmp/c1-a6-perf-fix.8RC0ee/disabled-diagnostic-scope-memo.txt`,
  `/tmp/c1-a6-perf-fix.8RC0ee/enabled.tsv`, and
  `/tmp/c1-a6-perf-fix.8RC0ee/profile-current.sample.txt`.
- Existing base/candidate sampling profiles:
  `/tmp/c1-a6-wall-diagnostic.Sd7Hyz/raw/profile-base.sample.txt` and
  `/tmp/c1-a6-wall-diagnostic.Sd7Hyz/raw/profile-candidate.sample.txt`.
- Governing rulings: `contracts/input-loading.md`,
  `docs/arch/refactor/rev11/evidence/C1/a6/frontier-resume-architecture-consult.md`, the C1
  charter/addendum, Stage-2 cutover ruling, landing ruling, A6 receipt, protocol-5 wall report, and
  recovery receipt.

The formal ABBA gate was not rerun. Under the granted user-exclusive-host/Rust-lock waiver, the
only new execution was bounded LLDB function-entry counting on the already-built unchanged base and
candidate binaries. It was used for operation counts, not timing or acceptance.

## Exact instruction ledger

| Quantity | Retired instructions | Meaning |
| --- | ---: | --- |
| Valid protocol-5 base | `52,680,501,178` | Landing denominator |
| Current `0c2295382` candidate diagnostic | `57,534,528,973` | Remaining candidate work |
| Current excess | `4,854,027,795` | `+9.21409%` |
| Maximum at `+3%` | `54,260,916,213` | Integer floor of the A6 ceiling |
| Excess allowed by A6 | `1,580,415,035` | May remain |
| Instructions that still must be removed | `3,273,612,760` | `67.4412%` of the current excess; `5.68982%` of candidate work |

The earlier fast candidate measured `57,950,855,937` instructions. `0c2295382` therefore recovered
`416,326,964` instructions (`0.7236%` of current candidate work, `7.899%` of the earlier excess, or
`12.717%` of the still-required A6 reduction). That is the demonstrated removable budget of this
lawful optimization class, and it is already committed.

As a deliberately generous sanity bound, repeating the entire `0c2295382` instruction win a second
time would leave `57,118,202,009` instructions (`+8.4238%`) and would still miss the cap by
`2,857,285,796`. There is no second change of comparable size in the permitted surface. The one
remaining measured pure request-local experiment, recovery-scope hashing, moved the wrong way:
`+308,689,003` instructions (`+0.5365%`) and `99.90 -> 100.14 ms`; it was correctly removed.

This evidence does not pretend to assign exact retired instructions to individual symbols: the
available `sample(1)` profiles are sampling profiles, not instruction-attributed profiles. It does
establish an exact irreducible **call/operation** budget under the current contracts and an exact
`3,273,612,760`-instruction closure requirement for which no remaining lawful work-elision mechanism
exists. Capacity reuse can change storage churn inside those calls; it cannot remove those calls.

## Per-request and per-wave operation comparison

The benchmark components all import the same existing absolute target, `/bench/types.ts`. The base
and candidate call stacks for this request are:

```text
base
Engine::resolve_import_outcome_in_published
  -> ProjectResolver::resolve_tracked
  -> resolve_with_reader / resolve_source_id
  -> probe_path_for_context / probe_path
  -> resolve_existing_path
  -> TransactionReader::probe_path
  -> TransactionReader::realpath

candidate
Engine::resolve_import_outcome_in_published
  -> resolver::resolve_tracked
  -> drive_attempt
  -> ResolveFrame::attempt
  -> resolve_frame_with_reader / resolve_source_id_frame
  -> evaluate_probe_candidates
  -> PriorityFrontierState::push
  -> ResolverAttemptView::path_probe / real_path
```

Because the target already has an extension, both algorithms construct the ordered target itself
plus 12 `index.*` candidates. The legacy resolver can observe the live reader and short-circuit on
the first candidate. The candidate starts with an empty immutable observation snapshot and must
batch every same-basis missing sibling before retrying.

| Operation | Base per request | Candidate wave 0 | Candidate wave 1 | Candidate wave 2 | Candidate total |
| --- | ---: | ---: | ---: | ---: | ---: |
| Whole resolver/attempt entry | `1` | `1` | `1` | `1` | `3` |
| Ordered `path_probe`/frontier pushes | `1` | `13` | `13` | `1` | `27` |
| Attempt-view `real_path` lookup | `1` live read | `0` | `1` missing | `1` present | `2` |
| Snapshot `real_path` lookup including completion replay | n/a | `0` | `1` | `2` | `3` |
| Newly loaded keys | live read | `13` path probes | `1` realpath | `0` | `14` |
| Outcome | complete | `NeedInputs` | `NeedInputs` | complete | complete |

Wave details:

1. Wave 0 starts at candidate zero with a fresh output. All 13 path observations are absent from the
   request snapshot, so the priority frontier evaluates all ordered siblings and unions 13 missing
   keys. The attempt returns only the `LoadSet`; its output is discarded.
2. Wave 1 starts again at candidate zero with a fresh output. Candidate zero is a file, but its
   realpath observation is missing, so it blocks. The frontier must still walk the 12 bounded
   lower-priority siblings to discover any additional missing inputs. They are now known misses;
   their observation/recovery outputs are constructed and then discarded because the higher
   candidate is blocked. The attempt returns the realpath `LoadSet` only.
3. Wave 2 starts again at candidate zero with a fresh output. Its path probe and realpath are now
   present; it records the current attempt's ordered path, realpath, and recovery witness, completes,
   and only then replays that output into the transaction.

The bounded entry trace confirms the static count rather than inferring it from allocations:

| Trace | Base | Candidate | Ratio |
| --- | ---: | ---: | ---: |
| 1-file corpus, warm-up + one measured run: top-level resolver/attempt entries | `44` | `132` | exactly `3.0x` attempts |
| Same trace: live/base probe vs candidate attempt-view path probes | `44` | `1,188` | exactly `27.0x` probes |
| Same trace: base realpath vs candidate snapshot realpath lookup/replay | `44` | `132` | exactly `3.0x` snapshot reads |
| 40-file corpus, warm-up + one measured run: top-level resolver/attempt entries | `1,448` | `4,344` | exactly `3.0x` attempts |

The 40-file trace contains two `run_once` passes, so one pass has 724 base resolver requests versus
2,172 candidate attempts. Applying the independently confirmed per-request shape gives 724 versus
19,548 ordered path-probe/frontier operations per pass: **18,824 extra path observations**, plus
1,448 extra whole attempt traversals, are required by the current retry state machine.

These counts also explain why allocation count is not an adequate diagnosis. In the comparable
protocol-5 sampling profiles, allocator/free/reallocator top-of-stack samples were 693/1,355
(`51.1%`) for base and 692/1,369 (`50.5%`) for candidate: allocation-stack occupancy did not rise
with the instruction regression. Current enabled counters do show residual allocation churn
(`912,537` whole-run allocations versus base `839,015`), but `0c2295382` reduced counts without
reducing bytes, and the required semantic call multiplicity remained exactly unchanged.

## Why the permitted reuse surface cannot close A6

The remaining permitted categories are valuable only inside the mandatory calls:

- Candidate strings and lists are already retained in `SourceResolutionGeometry` and its lazy
  per-project/node-modules/imports geometry.
- Pure normalized/joined/parent/probe/package spellings are already request-local in
  `ResolutionStringMemo`, basis-cleared, and pre-seeded from retained geometry.
- `0c2295382` removes the duplicate request/project spelling from `ResolveFrameOperation`, reuses
  the `InputKey` `Arc` in observation metadata, retains the manifest path, reuses requested/delta
  capacity, moves `LoadSet`s, and defers terminal-only key copies.
- Recovery-scope spelling/hash reuse was measured and rejected because it increased instructions.
- Further `Vec` capacity reuse would target small candidate/output vectors. It cannot remove the
  3x attempt entries, 27x ordered path observations, snapshot lookups, hash probes, result matching,
  or frontier state transitions. The Rust performance guidance also treats capacity reuse on such
  small vectors as normally below measurement noise; the repository's own measurement here is
  stronger, because the broader lawful cleanup recovered only 0.416B instructions.

Skipping the 18,824 repeated ordered observations per benchmark pass is the only concrete work
elision large enough to address the `3.274B` closure requirement. It is not pure geometry or scratch
reuse. A completed miss depends on observation values and carries ordered witness output. Retaining
it, a cursor, or an observation-derived prefix across `NeedInputs` would violate:

- `contracts/input-loading.md` section 4: retry the whole operation against the new immutable
  snapshot and restart at step 1;
- fresh `AttemptOutput` ownership: `NeedInputs`/`Terminal` discard all attempt output and only
  `Complete` transfers it;
- priority-frontier rules 3 and 9: retain only the `LoadSet` at the first block and discard all
  branch/frontier output on a non-complete result;
- the current ordered consumed-witness/replay contract.

Even a cursor without retained output would need a new proof that the skipped observation-derived
prefix remains valid in the later snapshot, plus a rule for reconstructing its ordered witness.
Basis equality and request isolation do not provide that proof.

## Assessment of `0c2295382`

The cleanup is correct within its stated boundary and should be retained.

- Replacing three string-keyed metadata maps with one `HashMap<InputKey, SnapshotInput>` preserves
  the input kind in the key and reuses the exact canonical `Arc`; enum matching fails closed if the
  stored metadata kind and observation kind disagree.
- Capturing the canonical manifest path at load time preserves the exact spelling paired with the
  loaded fingerprint and avoids replay-time recomputation.
- Replacing the request `BTreeSet` plus per-wave `LoadSet::delta` allocation with retained
  `requested`/`delta` vectors preserves `LoadSet` order. `LoadSet` is already sorted/deduplicated,
  and the filter is stable. Terminal-only unresolved vectors are still copied at the exact terminal
  boundary where their owned diagnostic payload is required.
- Moving each `LoadSet` into `last_load_set` changes ownership churn only. Basis mismatch,
  no-progress, retry, limit, load-failure, and final-output paths retain their prior decisions.
- Making `ResolveFrameOperation` a fieldless discriminator is valid because request/project start
  and specifier spellings already live in `SourceResolutionGeometry`; provider projection and
  unowned-package confirmation read those same retained values.
- The focused driver/frame tests pin `Arc` identity, one reusable delta allocation, zero successful
  terminal-key copies, and the fieldless discriminator. The recovery receipt additionally reports
  locked enabled counts/digest and focused tests green.

Its measured value is modest but real: `416,326,964` fewer instructions than the preceding fast
candidate, semantic-dispatch allocations `602,596 -> 593,263`, and whole-run allocations
`918,449 -> 912,537`. It does not change wave count, candidate count, ordered observation count, or
result/witness semantics, which is both why it is correct and why it cannot close A6.

## Required authority

There is no additional exact permitted C1 change with a positive, sufficient instruction budget.
The minimum authority needed for a fix is a new architecture ruling over all of the following:

1. amend the whole-operation restart rule to permit a typed request-local semantic continuation;
2. define continuation identity against the immutable snapshot and exact basis;
3. define revalidation/invalidation of every skipped observation-derived prefix;
4. define ownership, discard, and later publication of ordered `AttemptOutput` witness data;
5. amend priority-frontier rules 3/9 and the F24 witness/replay obligations accordingly;
6. authorize the required private API/state-machine change and its acceptance tests.

Without that authority, the exact permitted action is to retain `0c2295382`, make no further C1
production mutation, and report A6 as blocked by the ratified restart/discard semantics.

`RESULT NO_PATH`
