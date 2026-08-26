# C1 A6 disabled-wall diagnostic

## Result

**RESULT: REAL_REGRESSION.**

The final protocol-valid same-host comparison for measured subject
`1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e` is:

| metric | C1 base `d1f3d50a…` | candidate `1a4e41d5c…` | delta | locked result |
|---|---:|---:|---:|---|
| wall median of four invocation medians | 86.685 ms | 96.450 ms | +9.765 ms / **+11.264925%** | **FAIL** relative limit 3.0% |
| wall absolute | — | 96.450 ms | 3.550 ms below 100.000 ms | pass |
| peak RSS, maximum of four process maxima | 75,481,088 B | 75,661,312 B | +180,224 B / +0.238767% | pass |
| retired instructions, median of four process totals | 52,680,501,178 | 57,950,855,937 | +5,270,354,759 / **+10.004375%** | diagnostic hardware counter |
| cycles, median of four process totals | 17,833,603,914 | 19,164,061,484 | +1,330,457,570 / **+7.460397%** | diagnostic hardware counter |

The start/end baseline controls were 86.280/86.560 ms: +0.280 ms, **+0.324525% drift**, inside the
locked 3.0% invalidation fence. All ten invocations exited zero and each retained all 30 in-process
samples for 40 files, reported attribution OFF, and resolved the 40/40 warm-up corpus. No outlier was
removed. The exact order was start control, `A1 B1 B2 A2`, `A3 B3 B4 A4`, end control.

The candidate therefore passes the 100 ms product budget and both RSS bounds, but the cell is
conjunctive and its same-host wall relative condition fails by 8.264925 percentage points. Neither the
70.525 ms frozen historical baseline nor the old 91.205 ms diagnostic base is used as the comparison
control for this verdict.

## Prompt and authority metadata

- Task: `/root/c1_a6_wall_diagnostic`
- Sender: `/root`
- Date: `2026-08-26` (`Europe/Lisbon`)
- Diagnostic checkout: `/Users/carlosrodrigues/Documents/dev/verter-c1`
- Integration checkout held read-only: `/Users/carlosrodrigues/Documents/dev/verter`
- Repository HEAD at final report: `47b7b8b6cd214b224be0592f47486a856c7b014b`
- Requested/measured production subject: `1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e`
- Subject tree: `3cfc2f81b4b451519c3074ddfd165c6367048a5c`
- Current C1 base: `d1f3d50a948597f036868543b9bb21acacd730ff`
- Base tree: `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`
- Mode: author-independent, report-only; production code and thresholds unchanged

Initial prompt, verbatim:

> You are the fresh author-independent C1 A6 wall-time diagnostic. Do not modify
> /Users/carlosrodrigues/Documents/dev/verter-c1 or integration
> /Users/carlosrodrigues/Documents/dev/verter. Candidate is clean HEAD
> dd0a5e2f7f1c13ddfddefa3fbc0dd2a2bd58c853; measured code SHA 1a4e41d5c. Read CLAUDE.md,
> build/testing/rust-performance skills, C1 charter/addendum/Stage-2/landing ruling, A6
> lock/receipt/raw evidence, both recent diagnostics, and the exact harness runner. Resolve why
> disabled A6 latency is 100.255 ms while evidence references locked base 70.525 ms and an
> earlier same-host base 91.205 ms.
>
> Run only the exact locked A6 protocol needed to establish ground truth, serialized through
> rust-lock.sh and using isolated disposable base/candidate build/output locations; verify
> toolchain/harness Git blob/SHA-256/binary identities, sample completeness, thermal/co-resident
> workload conditions, ordering/ABBA requirements, and whether base and candidate were built
> equivalently. Do not weaken/recalibrate any threshold. If a fresh same-host base/candidate
> comparison is required by the locked protocol, run it. Do not edit production code. Classify:
> REAL_REGRESSION with exact attribution and a bounded C1-local fix target;
> INVALID_EVIDENCE/LOCK_MISMATCH with exact authority action required; or INCONCLUSIVE. No
> unrelated review.
>
> Persist exact prompt metadata, commands, raw paths, and full report at
> /Users/carlosrodrigues/Documents/dev/verter-c1/docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md
> (report-only write is allowed). Return compact: RESULT; base/candidate medians and deltas;
> protocol identity verdict; next lawful action.

Controlling execution update, verbatim:

> User authority update: this machine is exclusive to this task; rust-lock.sh is explicitly waived.
> Resume the exact C1 A6 wall verification. Record this user waiver/exclusive-host fact in the
> evidence. Use the committed performance subject code SHA
> 1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e and current integration base
> d1f3d50a948597f036868543b9bb21acacd730ff, with isolated build/output locations, exact repaired
> harness blob/SHA256/toolchain, complete 4x30 per arm in required A,B,B,A ordering plus start/end
> baseline controls and thermal/co-resident receipts. Do not modify production code or thresholds.
> Assess only from valid complete telemetry: PASS, REAL_REGRESSION with bounded C1-local attribution,
> or INCONCLUSIVE. Append/update wall-diagnostic.md with exact prompt metadata, commands, raw
> paths/digests, medians, delta, drift, RSS, identities and verdict.

Final background-process clarification, verbatim:

> Binding clarification: user explicitly waived RustDesk presence and ordered continuation even if
> RustDesk is running. Do NOT attempt launchctl or further termination. Record
> system/com.carriez.RustDesk_service and any respawned server CPU state as waived background. Start
> protocol3 immediately with BrowserStack absent and no cargo/rustc/gate/benchmark conflicts. Run full
> start/end controls + exact A,B,B,A 4x30, record thermal/process receipts and hardware counters,
> finalize report/verdict. No further pause for RustDesk.

These are execution waivers for this author-independent diagnostic. They do not edit
`performance-gates.toml`, change the A6 lock, or create a durable replacement for the missing
serializer/session runner.

## Governing material read

- `CLAUDE.md`
- `.claude/skills/build-and-profiling/SKILL.md`
- `.claude/skills/testing/SKILL.md`
- `.claude/skills/rust-performance/SKILL.md`
- `docs/arch/refactor/rev11/charters/C1.md`
- `docs/arch/refactor/rev11/rulings/ARCH-ADDENDUM-C1-THREE-GAPS.md`
- historical `docs/arch/refactor/rev11/evidence/C1/stage2-execution-plan.md`
- `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md`
- `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md`
- `performance-gates.toml`
- `docs/arch/refactor/rev11/evidence/A6/{implementation-lock-record,baseline-measurement,counter-reproduction,command-proofs}.md`
- `docs/arch/refactor/rev11/evidence/C1/a6/{receipt,memo-architecture-consult,unblock-architecture-consult,residual-244-diagnostic}.md`
- `.feedback/{c1-implementation-report,c1-recovery-implementer-receipt}.md`
- exact harness `crates/verter_bench/examples/attribution_baseline.rs`
- all committed C1 A6 wall/counter raw files

## Protocol identity and build equivalence

| identity | base | candidate | verdict |
|---|---|---|---|
| commit | `d1f3d50a948597f036868543b9bb21acacd730ff` | `1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e` | requested pair |
| tree | `2e7cf8637ec5c52b0fa04572d99672b052f1f85f` | `3cfc2f81b4b451519c3074ddfd165c6367048a5c` | recorded |
| harness Git blob | `efa9ea54a14772ecd87511d6bb07017aa33940ba` | same | exact match |
| harness SHA-256 | `5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632` | same | exact repaired lock match |
| `rust-toolchain.toml` blob | `27d6fdd1ac927948bafc650047c157b2369a4f9e` | same | exact match |
| root `Cargo.toml` blob | `56a3fc6759c2f0220529ce499b66a5e2d4ef7b74` | same | exact match |
| `.cargo/config.toml` blob | `fcee591a87b82fd1b7acd377d8d9c4fbc775fd92` | same | exact match |
| `Cargo.lock` blob | `bd3e5e3d632249eb8086a3b7d2febad9a6dc2ffc` | `93e01596405b953e91f5f08eda0331f28da03587` | only C1 workspace dependency membership differs; external versions/checksums do not |

The host matches the locked class: Darwin 25.6.0 arm64, Apple M3, 8 logical CPUs, 25,769,803,776
bytes, rustc `1.97.1 (8bab26f4f 2026-07-14)`, cargo `1.97.1
(c980f4866 2026-06-30)`. It remained on AC power with AC `lowpowermode 0`. Every pre/post receipt
reported no thermal warning and no performance warning.

The two binaries are arm64 Mach-O files with platform 1, minimum macOS 11.0, and SDK 26.5:

| arm | bytes | SHA-256 | Mach-O UUID |
|---|---:|---|---|
| base/default features | 22,372,176 | `20e743e1151345130fbe7cf6455067e79b4295017f64820e467a465fd68b2731` | `22A70E06-7D87-34CE-8FF7-C9F49EC5B90D` |
| candidate/default features | 22,501,856 | `1a4cb4556ecea4a1fec9b3ec01d07ef2eac7c9554c1cb81ee186df5d182a1fce` | `8E47A195-111E-3F35-B0C1-574B809317E1` |

Both were clean builds from `git archive` source exports into separate target directories, with the
same command, profile, default feature set, environment, root profile/config, toolchain, host, and
SDK. Outputs confirm `attribution: OFF` in every invocation.

**Protocol identity verdict: MATCH for this diagnostic under the explicit serializer and RustDesk
waivers.** Harness bytes, toolchain, host class, build shape, invocation, sample count, ordering,
controls, RSS capture, and all-sample policy match. The waivers do not amend the repository lock.

## Exact commands and locations

Evidence root: `/tmp/c1-a6-wall-diagnostic.Sd7Hyz` (outside both repositories).

```sh
git archive d1f3d50a948597f036868543b9bb21acacd730ff |
  tar -x -C /tmp/c1-a6-wall-diagnostic.Sd7Hyz/base-src
git archive 1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e |
  tar -x -C /tmp/c1-a6-wall-diagnostic.Sd7Hyz/cand-src

(cd /tmp/c1-a6-wall-diagnostic.Sd7Hyz/base-src &&
 CARGO_TARGET_DIR=/tmp/c1-a6-wall-diagnostic.Sd7Hyz/base-target \
 cargo build -p verter_bench --release --example attribution_baseline)
(cd /tmp/c1-a6-wall-diagnostic.Sd7Hyz/cand-src &&
 CARGO_TARGET_DIR=/tmp/c1-a6-wall-diagnostic.Sd7Hyz/cand-target \
 cargo build -p verter_bench --release --example attribution_baseline)

/usr/bin/time -l <base-or-candidate-binary> --files 40 --runs 30
```

Final order:

```text
20s idle; control-start(A)
20s idle; A1
20s idle; B1
20s idle; B2
20s idle; A2
20s idle; A3
20s idle; B3
20s idle; B4
20s idle; A4
20s idle; control-end(A)
```

Equal idle cadence is environmental control, not a changed sample, metric, statistic, order, or
threshold. Two complete immediate-cadence attempts exceeded the control fence and were discarded.

Principal raw paths:

```text
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/{identity.tsv,environment.txt,binary-identities.txt}
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/raw/{build-base.log,build-candidate.log}
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/protocol3-authority-and-preflight.txt
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/exclusive-host-cleanup-actions.txt
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/order-protocol5.tsv
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/metrics-protocol5.tsv
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/aggregates-protocol5.tsv
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/conditions-protocol5/
/tmp/c1-a6-wall-diagnostic.Sd7Hyz/raw/protocol5/{control-start,A1,B1,B2,A2,A3,B3,B4,A4,control-end}.txt
```

`/tmp/c1-a6-wall-diagnostic.Sd7Hyz/meta/SHA256SUMS.txt` contains 163 SHA-256 entries for the raw and
metadata evidence plus both binaries. Its SHA-256 is
`306745db699e0b7244d40176d3885bc30bad650bfcad040ebfa2687a8920289a`. An aborted, post-gate
allocation-profiler trace directory is deliberately excluded from that manifest and from every
verdict; its command/error receipt remains under `raw/` as disclosed diagnostic debris.

## Final raw telemetry

| invocation | arm | wall median (ms) | wall min (ms) | peak RSS (B) | retired instructions | cycles |
|---|---|---:|---:|---:|---:|---:|
| control-start | control | 86.28 | 83.96 | 75,628,544 | 52,693,639,047 | 17,823,694,316 |
| A1 | base | 86.88 | 85.37 | 74,858,496 | 52,682,445,503 | 17,806,761,045 |
| B1 | candidate | 96.56 | 93.91 | 75,661,312 | 57,939,923,205 | 19,168,123,332 |
| B2 | candidate | 96.34 | 94.33 | 75,087,872 | 57,936,592,438 | 19,161,989,261 |
| A2 | base | 86.60 | 83.35 | 74,891,264 | 52,718,391,577 | 17,860,446,782 |
| A3 | base | 86.41 | 84.01 | 75,218,944 | 52,678,556,852 | 17,865,022,381 |
| B3 | candidate | 95.84 | 93.09 | 75,153,408 | 57,961,788,669 | 19,058,167,757 |
| B4 | candidate | 96.94 | 94.36 | 74,973,184 | 57,969,625,961 | 19,166,133,706 |
| A4 | base | 86.77 | 85.00 | 75,481,088 | 52,655,172,084 | 17,767,454,428 |
| control-end | control | 86.56 | 84.82 | 76,185,600 | 52,715,192,224 | 17,928,963,158 |

Every row reports 41 corpus files, 30 runs, attribution OFF, warm-up 40/40, `files=40`, `runs=30`,
and `attribution=0`.

RSS passes against the frozen 74,850,304-byte reference: candidate arm maximum 75,661,312 bytes is
+1.083507%, below 4.952% and far below 256 MiB. Median process maxima are 75,120,640 bytes candidate
versus 75,055,104 bytes contemporary base (+0.087317%).

## Session validity and discarded attempts

| namespace | controls (ms) | drift | disposition | note |
|---|---:|---:|---|---|
| `protocol1` | one control only | N/A | aborted | zsh wrapper used read-only variable `status`; full session restarted |
| `protocol2` | 88.280 → 89.740 | +1.653829% | diagnostic-only | before final process-cleanup/RustDesk authority |
| `protocol3` | 84.050 → 87.700 | +4.342653% | **void** | above unchanged 3.0% fence |
| `protocol4` | 84.240 → 88.630 | +5.211301% | **void** | above unchanged 3.0% fence |
| `protocol5` | 86.280 → 86.560 | **+0.324525%** | **accepted** | equal idle cadence stabilized host |

No protocol3/4 statistic enters the verdict. They are retained to prove whole-run invalidation was
applied before choosing a direction.

For protocol5, 20 pre/post receipts all show AC power and no thermal/performance warning. BrowserStack
was absent in all 20. No cargo, rustc, nextest, gate, benchmark, sample, or xctrace process appeared.
The explicitly waived RustDesk processes were:

```text
PID 87009, UID 0,   /Applications/RustDesk.app/Contents/MacOS/service, 0.0% CPU
PID 84614, UID 501, /Applications/RustDesk.app/Contents/MacOS/RustDesk --server, 0.0–1.0% CPU
```

Their presence is disclosed. Accepted drift and ABBA bound session effects; the +10.004375%
per-process instruction delta independently demonstrates extra candidate work.

## Why 70.525, 91.205, and 100.255 differed

- **70.525 ms** is the frozen median of four historical medians on old implementation baseline
  `fb863297…`, not a current C1-session control. The transitive production closure changed.
- **91.205 ms** is the previous current-C1-base median. That run was `AAAA BBBB` without separate
  controls, so it is host history, not locked evidence.
- **100.255 ms** is a later candidate-only median. It lacked a contemporary base, ABBA, controls, and
  condition receipts. It cannot lawfully be divided by either old base.
- **96.450 versus 86.685 ms** is the complete comparison. Absolute latency moves with host/session
  state and currently passes 100 ms, while approximately ten-percent extra candidate work persists.
  The locked decision is the valid same-session +11.264925% relative failure.

## Attribution and bounded C1-local fix target

The valid disabled-arm counters localize the regression to candidate CPU work: retired instructions
rise **10.004375%**, close to wall's **11.264925%**; cycles rise **7.460397%** while RSS is flat. This
excludes RSS pressure, a changed corpus, missing samples, attribution instrumentation, and pure
co-resident delay as primary causes.

Existing locked enabled-arm evidence for this exact production subject supplies the owner boundary:

| counter | base | fast-path candidate | delta |
|---|---:|---:|---:|
| `workspace.normalize_canonical_id.calls` | 11,313 | 1,981 | -9,332; below ceiling |
| `session.semantic_dispatch.calls` | 4,216 | 4,216 | 0 |
| semantic-dispatch allocations | 477,396 / 103,677,354 B | 602,596 / 128,551,790 B | **+125,200 (+26.225607%) / +24,874,436 B (+23.992159%)** |
| whole-run unattributed allocations | 839,015 / 128,974,689 B | 918,449 / 144,440,993 B | **+79,434 (+9.467530%) / +15,466,304 B (+11.991736%)** |
| scheduler execute/wait/queue | 130/130/130 | 130/130/130 | 0 |
| component-meta digest | `7161214711717846280` | same | exact oracle |

Normalization is no longer the fix target: the measured fast path is below base while disabled wall
still retires ten percent more instructions. Top-level dispatch, scheduler work, and output are equal.
The remaining discriminator is request-path allocation/copy work inside semantic dispatch.

The source-local mechanism is the Stage-2 attempt driver:

- `crates/verter_workspace/src/resolver.rs:37` `ResolutionInputs` owns a snapshot and three maps;
- `resolver.rs:57` `attempt_view` materializes each kernel-wave view;
- `resolver.rs:61` `load_requested_inputs` clones keys/paths and stores directory vectors/manifests;
- `resolver.rs:158` `apply_attempt_output` replays consumed observations;
- `resolver.rs:214` `drive_attempt` owns set/vector state and repeats on `NeedInputs`;
- `resolver.rs:296` retains one semantic `ResolveFrame`, whose attempt is
  `crates/verter_semantic/src/resolver_core/resolve_frame.rs:321`.

C1 introduced typed staged inputs, ordered load sets, replay, and witnesses. The fast path retains
frame geometry and removes dead normalization, but the driver still materializes/copies request-local
input/output state. Allocation and instruction deltas converge on that boundary.

The **bounded C1-local fix target** is request-lifetime allocation/copy elimination inside
`ResolutionInputs` / `drive_attempt` / `load_requested_inputs` / `apply_attempt_output` and the
existing `ResolveFrame`. Reuse or move existing request-owned buffers/keys and avoid duplicate owned
spellings/snapshot materialization across waves. Do not reduce/reorder `NeedInputs` waves; change
ordered load sets, observations, witnesses, results, digest, metrics, public APIs; add cross-request
caching/new retention; or alter thresholds. No broader resolver redesign is authorized.

## Next lawful action

1. Add RED/GREEN allocation discriminators at the named request-local driver boundary, including
   multi-wave and revert controls.
2. Implement the smallest request-local buffer/key/snapshot reuse preserving exact
   wave/load/observation/witness/result behavior.
3. Rerun the 24 conversion and seven driver cases, enabled A6 counters/oracle, then the full disabled
   protocol with fresh isolated binaries, ABBA, controls, receipts, and unchanged limits.
4. Do not freeze or land C1 until wall passes both 100 ms absolute and 3.0% same-host relative with
   every other A6 conjunct green.

No production code, lock field, statistic, or threshold was changed.
