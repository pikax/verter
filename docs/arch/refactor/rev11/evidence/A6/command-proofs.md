# A6 — Command proofs

The non-vacuous command manifest the lock record's §2 points at. Recorded per
`contracts/baseline-lock.md` §4: exact command, working directory, environment and features, exit
code, executed and skipped counts, and a raw-output digest. **A green command that executed zero
intended work is a failure**, so the "executed" column is the load-bearing one, not the exit code.

Working directory for every row: the repository root of the block's worktree. Environment: the
toolchains recorded in the lock record §1, `node_modules` present, `packages/*/dist` built.

**Raw logs are NOT committed in-tree, and that is a rule rather than an omission.** Every one of them
embeds this machine's absolute worktree path in thousands of places, which the
`tracked_paths_no_machine_roots` guard rejects — the very guard §3.1 records the two preceding blocks
tripping. They live in the external evidence bundle, in the same form the first command-proof block
established, and are bound here by digest. The digest column is the first 16 hex of `shasum -a 256`
over each raw log.

## 1. The manifest

| # | Exact command | Executed | Skipped | Exit | Wall | Raw log digest |
|---|---|---:|---:|---:|---:|---|
| 01 | `cargo fmt --all --check` | whole workspace | — | **0** | 1s | `c45b11f437393f29` |
| 02 | `pnpm build:ts` | all TS packages | — | **0** | — | — |
| 03 | `node scripts/gate.mjs` | 24,156 (S1) + 3 suites (S2) + 8,533 (S3) | 581 / 563 | **1** | 43m15s | `05c9ffc890377940` |
| 04 | `cargo clippy --workspace --all-targets -- -D warnings` | all targets, host | — | **0** | 3m16s | `142532f24744724e` |
| 05 | `cargo check --workspace --release` | real release profile (opt-level 3 + fat LTO) | — | **0** | 1m22s | `10e70690aaad8991` |
| 06 | `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | the wasm32 artifact | — | **0** | 1m05s | `9efe1a76f2a4067d` |
| 07 | `cargo check --workspace --all-targets --features verter_audit/attribution` | all targets, enabled arm | — | **0** | 1m51s | `8295e3a2625b6eba` |
| 08 | `cargo test -p verter_audit --features attribution` | 50 + 36 (1 ignored) | 3 ignored | **0** | 33s | `4ee9010dae74a641` |
| 09 | `cargo test -p verter_audit --features compile-fail` | 46 + **37 trybuild** | 1 ignored | **0** | 43s | `5879a24ddbae7556` |
| 11 | `pnpm run test:scripts` | 14 + 5 + 7 + 26 + **26** | 0 | **0** | 4s | `196208a39f1ffa92` |
| 12 | `pnpm install --frozen-lockfile` | lockfile in sync | — | **0** | 3s | `426d05bd6d78a919` |
| 13 | `pnpm run build:native` | the `.node` bindings | — | see §4 | — | — |
| 14 | `pnpm test` | the JS/TS suites | — | see §4 | — | — |

Rows 04–07 are the end-of-change checks `CLAUDE.md` requires; 05 and 06 are included because neither
is reachable from the gate (05 compiles the real release profile the gate never builds; 06 covers the
wasm32 artifact host clippy cannot see).

**Rows 07–09 are the locked per-block set (decision A5-G1).** They are run here first so the
requirement this lock places on every later block ships with a proof that it can be met. Row 09's 37
trybuild cases are the seal proving the counter-reader surface is absent from a default build — the
negative control for the no-semantic-authority claim, which the canonical gate never executes.

**Row 11 executes the new gate-file validator's suite** (26 tests, the last figure) alongside the
program-state validator's. Both are wired into the same authoritative runner.

## 2. Non-vacuity

Each row's selector was checked to have matched real work rather than an empty set:

- **03** lists 78 suites from the archive and starts 24,156 tests across 78 binaries on surface 1;
  surface 2 executes 3 `verter_session` libtest binaries directly; surface 3 builds a second
  whole-workspace archive under the shipped `cfg(debug_assertions)`-off profile and runs 8,533 tests.
  Its build-prerequisite preflight reported **SATISFIED** (the TypeScript plugin entry loaded from
  the probe directory), and its freshness-tooling preflight reported **already-present — tolerance
  DISABLED**, so the proto/TS byte-pin ran genuinely and a freshness failure would have been a hard
  failure. It was not one.
- **08/09** report per-binary `running N tests` lines with non-zero N; row 09's trybuild binary
  compiles 37 fixtures under `target/tests/trybuild/verter_audit`.
- **11** reports five separate suite summaries with non-zero counts.
- **07** is a compile of the enabled arm, which the default arm does not type-check; it is the check
  that caught a real error during the instrumentation block.

## 3. The gate's verdict, and every failure classified

`node scripts/gate.mjs` returned **FAIL — 5 non-tolerated failures**, which are **3 distinct tests**
(the machine-roots guard is reported once per surface):

```
[nextest]                cases::tracked_paths_no_machine_roots::tracked_files_contain_no_machine_specific_path_markers
[nextest]                resilient::resilient_tests::failed_respawn_retries_within_budget_and_recovers
[nextest:TIMEOUT]        cases::g_compile::compile_fail::hot_materialize_and_script_fact_structural_rails_smoke
[libtest:…]              cases::tracked_paths_no_machine_roots::… (same test, surface 2)
[shipped-cfg/nextest]    cases::tracked_paths_no_machine_roots::… (same test, surface 3)
```

Each is classified below with the evidence that classifies it. None is left as "probably fine".

### 3.1 `tracked_paths_no_machine_roots` — REAL, and pre-existing

The guard rejects tracked files embedding a machine/user absolute-path root. It named **three**
files:

```
docs/arch/refactor/rev11/evidence/A4/context-packet.md: contains machine-specific marker `/Users/…`
docs/arch/refactor/rev11/evidence/A5/context-packet.md: contains machine-specific marker `/Users/…`
docs/arch/refactor/rev11/evidence/A6/context-packet.md: contains machine-specific marker `/Users/…`
```

**Two of the three are pre-existing at the implementation baseline**, proven directly against the
baseline commit rather than inferred:

```sh
git grep -l "/Users/…" <baseline-sha> -- docs
# docs/arch/refactor/rev11/evidence/A4/context-packet.md
# docs/arch/refactor/rev11/evidence/A5/context-packet.md
```

They landed undetected because neither of those blocks ran the canonical gate — the instrumentation
block's own summary records that the full gate was NOT run, and the inventory block ran no test suite
at all on the reasoning that it changed no production source. The guard scans *tracked bytes*, not
production source, so that reasoning had a hole in it. **This is the concrete cost of an
evidence-only block skipping the gate, and it is exactly what a lock block running the gate is for.**

The third instance was this block's own, and it is **fixed**: the one absolute path in this block's
context packet is normalised to `<MACHINE_ROOT>`, disclosed at the top of that file, following the
same normalisation the ledger transport copy uses.

The fix is verified discriminating rather than assumed — re-running the guard alone after the change
reports **2 violations, not 3**, and names only the two pre-existing files:

```
tracked files must not embed machine/user/session/orchestration absolute-path markers; 2 violation(s):
  docs/arch/refactor/rev11/evidence/A4/context-packet.md
  docs/arch/refactor/rev11/evidence/A5/context-packet.md
```

**The residue is not this block's to repair, and that is a scoping fact rather than a preference.**
Both files' digests are recorded in the ledger as `block.A4.context_packet_digest` and
`block.A5.context_packet_digest`. Editing them invalidates two recorded digests on two already-accepted
blocks, which is an orchestrator/maintainer action. Raised as an open item in the block report.

### 3.2 `failed_respawn_retries_within_budget_and_recovers` — flaky under load

A real-tsserver test: it kills the provider, waits for respawn, and asserts a typed hover on the
recovered carrier through a retry ladder totalling ~9.75 s.

| run | machine state | result |
|---|---|---|
| in the gate | load average ≈ 34, 8 logical CPUs | FAIL — "returned no hover for recovered carrier" |
| 3-test rerun | competing with a 260 s compile-fail test | FAIL — same message |
| isolated rerun 1 | idle | **PASS** (10.4 s) |
| isolated rerun 2 | idle | **PASS** (12.0 s) |

Two passes out of two in isolation, two failures out of two under contention: the retry ladder is not
long enough to cover tsserver respawn on a saturated machine.

Not attributable to this candidate on independent grounds: the candidate changes **zero bytes** under
`crates/` or `packages/`, which is every tree-derived input this test reads
(`git diff --stat <baseline>..HEAD -- crates packages` is empty), and the tsserver binary is pinned.

### 3.3 `hot_materialize_and_script_fact_structural_rails_smoke` — TIMEOUT under load only

A trybuild compile-fail smoke test, killed at the runner's 360 s cap during the gate. Its two sibling
tests in the same file were also flagged `>120 s` in the same run.

Isolated rerun on an idle machine: **PASS at 260.0 s** — inside the cap, but with only 100 s of
headroom, which is why concurrent load pushes it over. Same non-attribution argument as §3.2.

### 3.4 What that means for this candidate

One genuine tracked-tree defect exists on the baseline and this block **reduces** it from three
instances to two; the remaining two require a ledger action outside this block's write set. The other
two failures are load-sensitivity in the runner, reproduced as passes in isolation.

Stated plainly rather than rounded off: **the canonical gate does not currently return PASS on this
tree, and it did not return PASS on the baseline either.** The lock record does not claim a green
gate. It claims what the commands actually did, which is the standard
`CLAUDE.md`'s *Verification Must Prove Execution* rule sets — exit status alone is neither a pass nor,
by itself, a failure worth acting on until each failure is attributed.

## 4. The two rows that needed a build this worktree did not have

`pnpm test` initially failed at `@verter/native`'s `pretest` → `ensure-native-loader`: a fresh
worktree has no built `.node` binding, because `pnpm install` does not produce one. That is an
environment prerequisite, not a result — the same class as the TypeScript-plugin dist the gate's own
preflight fails closed on, and precisely the kind of "green command that proved nothing" the baseline
contract warns about, inverted.

So `pnpm run build:native` was run and `pnpm test` re-run. Both outcomes are recorded in §5 with the
same honesty as §3: if the re-run does not pass, it is reported as a failure and classified, not
dropped.

## 5. Re-run outcomes after the native build

See [`command-proofs-native.md`](command-proofs-native.md).

## 6. Baseline measurement commands

Not part of the gate, and recorded separately because they produce the numbers the gate file freezes:

```sh
cargo build -p verter_bench --release --example attribution_baseline
for i in 1 2 3 4; do /usr/bin/time -l ./target/release/examples/attribution_baseline --files 40 --runs 30; done

cargo build -p verter_bench --release --features attribution --example attribution_baseline
./target/release/examples/attribution_baseline --files 40 --runs 3 --format tsv   # ×3

node scripts/validate-performance-gates.mjs --gates performance-gates.toml
# PASS performance-gates.toml: 1 cell(s), 15 metric(s), no placeholders
```

Full derivation in [`baseline-measurement.md`](baseline-measurement.md) and
[`counter-reproduction.md`](counter-reproduction.md).
