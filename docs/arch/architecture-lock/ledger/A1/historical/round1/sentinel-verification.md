# A1 Sentinel Verification (`verification.md` §2)

Requirement: "Sentinel verification is required for critical harnesses: in an
isolated non-candidate run, deliberately break one known assertion or test fixture
and prove the canonical selector fails."

**Isolated copy:** a full `git clone` of the A1 candidate worktree at
`b7ea2dc88bda86473de81de3438b7f88ef30adc7` (branch `block/a1-command-truth`,
clean, tree `47645406a9246e600af995c62608b709347e13a4`), placed OUTSIDE both the
primary checkout and the candidate worktree (referred to below as `<SENTINEL>`),
with its own `pnpm install --frozen-lockfile`, its own TS builds, and its own cargo
target directory. No candidate file was modified at any point; the candidate
worktree remained clean throughout (proven by `git status --porcelain` before and
after the battery).

**Plant-proof discipline (CLAUDE.md "Verification Must Prove Execution"):** every
plant carries a globally unique token. Before each planted run we prove the token
is (a) PRESENT in the planted file, (b) UNIQUE in the sentinel tree (exactly the
expected occurrence count), and (c) NEW — zero occurrences in the unmodified
candidate tree. A green planted run is treated as a failure to prove, never as a
pass. Because the baseline gate is RED on this candidate (one pre-existing
failure), overall exit codes cannot discriminate a plant; the discriminator is the
planted test's OWN result line (present-failing in the planted run, absent/passing
in the restored run), recorded verbatim below.

Raw outputs live in `sentinel-runs/` beside this file; digests in the table at the
end.

---

# Sentinel C — conformance selector (`pnpm run gen:vue-goldens:check`)

- **Plant:** appended `/*A1_SENTINEL_PLANT_GOLDEN_c94af2*/` to the committed
  official-Vue golden artifact
  `crates/verter_vue_conformance/corpus/goldens/3.6.0-rc.1/vapor/built-ins/teleport.js`
  in `<SENTINEL>`.
- **Plant-proof:** token occurrences BEFORE plant — sentinel tree 0, candidate
  tree 0; AFTER plant — sentinel tree exactly 1 (line shown in raw log). Present,
  unique, new.
- **Planted run:** exit **1**. The selector named the exact planted artifact:
  `DRIFTED (bytes differ): crates/verter_vue_conformance/corpus/goldens/3.6.0-rc.1/vapor/built-ins/teleport.js`
  and `goldens check failed: 0 missing, 1 drifted, 0 stale`.
- **Restore:** `git checkout --` the file; token occurrences back to 0.
- **Restored run:** exit **0**, `goldens check OK: 286 committed artifacts match a
  fresh run` — byte-identical verdict to the candidate run (command-proof row 12).
- **Verdict: DISCRIMINATING.** One mutated golden byte flips the canonical
  selector to a named failure; restoration returns the prior result.
- Raw: `sentinel-runs/sentinel-C-vue-goldens.txt`.

# Supplementary negative control — `cargo fmt --all --check`

Not one of the three mandated sentinels; run because rustfmt reports no
file/test counts, so the candidate's green row 05 needed an independent
non-vacuity proof.

- **Plant:** appended misformatted `fn  a1_sentinel_plant_fmt_93d1( ) ->i32{  1 }`
  to `crates/verter_span/src/lib.rs` in `<SENTINEL>`.
- **Plant-proof:** token 0 before (both trees), exactly 1 after (line 631 shown).
- **Planted run:** exit **1**, `Diff in …\crates\verter_span\src\lib.rs` (1 diff).
- **Restore + rerun:** token 0; exit **0** with empty output — the candidate
  result.
- **Verdict: DISCRIMINATING.** Raw: `sentinel-runs/fmt-negative-control.txt`.

# Sentinel A — the Rust gate (`node scripts/gate.mjs --timeout 420m`)

Same invocation as candidate command-proof row 01; run twice in `<SENTINEL>` with
its own cold `target/gate-runner`.

- **Plant:** `panic!("A1_SENTINEL_PLANT_GATE_7f3a9c");` inserted as the first
  statement of `#[test] fn tracked_paths_are_portable_across_platforms()` in
  `crates/verter_session/tests/cases/tracked_paths_are_portable.rs` — a test that
  executes on ALL THREE gate surfaces.
- **Plant-proof:** token 0 before (sentinel AND candidate trees); exactly 1 after
  (line 124 shown); `git diff --stat` = 1 file, 1 insertion — the plant is the
  only change.
- **Planted run:** exit **1**, `VERDICT: FAIL — 3 non-tolerated failure(s)`, and
  ALL THREE are the planted test, named per surface:
  `[nextest]`, `[libtest:verter_session::main]`, and `[shipped-cfg/nextest]`
  `cases::tracked_paths_are_portable::tracked_paths_are_portable_across_platforms`,
  with the panic backtrace citing the planted line (`tracked_paths_are_portable.rs:124:5`).
  Surface summaries: S1 24044 run / 24043 passed / 1 failed; S3 8477 run / 8476
  passed / 1 failed — the single failure on each surface IS the plant.
- **Restore:** `git checkout --` the file; token 0; sentinel tree clean (0 entries).
- **Restored run:** exit 1 with `VERDICT: FAIL — 1 non-tolerated failure(s)` that
  is **NOT the plant**: the planted test name appears ZERO times in the restored
  raw output (it passed among S1's 24043; S2 3 suites clean; S3 8477/8477 passed —
  the planted-surface profile fully recovered). The residual failure is
  `[nextest] verter_tsc::main cases::fallthrough::fallthrough_attrs_accepted_only_where_they_reach_the_dom`
  (5.48s) — an environment-sensitive/flaky real-provider-class test that failed in
  NEITHER the candidate run nor the planted sentinel run.
- **Verdict: DISCRIMINATING.** The canonical gate cannot stay green (or keep its
  prior failure identity) over a broken known assertion: the plant was named as a
  failure on every surface, and after restoration the plant is gone. Because the
  baseline is red, discrimination is per-test identity, not overall exit — exactly
  as pre-registered in the discipline note above.
- **Two A1 environment findings recorded (not fixed):**
  1. The candidate worktree's single pre-existing gate failure
     (`verter_language::main cases::compile_fail::registered_authority_capabilities_are_not_mintable_outside_their_authorities`,
     trybuild, 6/6 sub-cases) did NOT reproduce in the fresh clone of the SAME
     commit — sentinel runs passed it in both directions. The failure is
     checkout-environment-sensitive (linked-worktree vs plain clone), not purely
     tree-determined.
  2. `verter_tsc::main cases::fallthrough::fallthrough_attrs_accepted_only_where_they_reach_the_dom`
     failed once (restored run) and passed twice (candidate + planted runs) on the
     same tree content — a flake under this machine's conditions.
- Raw: `sentinel-runs/sentinel-A-gate.txt` (driver log),
  `sentinel-runs/sentinel-A-gate-planted-raw.txt`,
  `sentinel-runs/sentinel-A-gate-restored-raw.txt`.

# Sentinel B — the vitest suite (`pnpm test`)

Three rounds; the first two are recorded because each exposed a real property of
the canonical selector that a passing sentinel would have hidden.

- **Plant site (all rounds):** appended `describe("a1 sentinel", …)` with the
  unique token `A1_SENTINEL_PLANT_VITEST_b27d41` to
  `packages/types/src/exports.spec.ts` (`@verter/types` — hermetic, no
  native/wasm dependency). Plant-proof every round: token 0 before (both trees),
  exactly 2 occurrences in exactly 1 file after (test name + payload),
  `git diff --stat` = 1 file / 6 insertions.

- **Round 1 — canonical `pnpm test`, runtime-throw plant: NOT PROVEN, and the
  reason is itself an A1 finding.** The run exited 1, but the raw output contains
  ZERO occurrences of the token: `@verter/native`'s pretest
  (`ensure-native-loader`) failed first in the sentinel copy (no built `.node`),
  and the root selector's PARALLEL + BAIL-FAST semantics killed the planted suite
  before it reported. Per the pre-registered discipline this is a failure to
  prove, not a pass. **Finding:** the canonical root JS selector cannot be relied
  on to even REACH an arbitrary package's tests when any unrelated package fails
  earlier — the same structural vacuity row 08 records for the candidate.
- **Round 2 — no-bail variant, runtime-throw plant: NOT DISCRIMINATING, second
  finding.** With `pnpm -r --no-bail --parallel run test` the planted suite ran —
  and reported `Tests 615 passed (615)`: the throwing test PASSED. Cause
  (verified in `packages/types/vitest.config.ts`): `typecheck: { enabled: true,
  only: true, checker: "tsc" }` — `@verter/types` is a TYPECHECK-ONLY suite whose
  test bodies never execute, so a runtime `throw` is invisible BY DESIGN.
  **Finding:** the `@verter/types` suite discriminates type-level breakage only;
  runtime regressions in spec bodies cannot fail it.
- **Round 3 — no-bail variant, TYPE-LEVEL plant: DISCRIMINATING.** Plant =
  `assertType<number>("…" as string)` inside the planted test (a TS type error,
  the failure class this suite owns). Planted run: exit 1 with
  `× A1_SENTINEL_PLANT_VITEST_b27d41` and
  `Tests  1 failed | 614 passed (615)` in `@verter/types`. Restore (token 0);
  restored run: `Tests  614 passed (614)`, token absent from the output — equal
  to the candidate baseline (command-proof row 08c: types 614 passed). Overall
  exit remains 1 in both directions from the PRE-EXISTING typeinfo/unplugin reds,
  so discrimination is per-suite identity, as pre-registered.
- **Selector note:** rounds 2–3 use `pnpm -r --no-bail --parallel run test` —
  identical package selection to the canonical `pnpm test`, differing only in
  bail semantics — because round 1 proved the bail-fast form cannot reliably
  deliver the planted suite's verdict on a red baseline. The candidate evidence
  records both forms (rows 08 and 08c).
- Raw: `sentinel-runs/sentinel-B-vitest.txt` (driver log),
  `sentinel-runs/sentinel-B-vitest-{planted,restored}-raw.txt` (round 1),
  `sentinel-runs/sentinel-B2-{planted,restored}-raw.txt` (round 2),
  `sentinel-runs/sentinel-B3-{planted,restored}-raw.txt` (round 3).

# Result summary

| Harness | Canonical selector | Plant | Plant proven applied | Planted result | Restored result | Verdict |
|---|---|---|---|---|---|---|
| Rust gate | `node scripts/gate.mjs --timeout 420m` | `panic!` in a 3-surface verter_session test | yes (0→1 occurrence, 1-file diff, both-tree negative) | exit 1; VERDICT FAIL names the plant on ALL 3 surfaces | plant absent/passing; surfaces recover (S3 8477/8477); one unrelated flake surfaced | DISCRIMINATING |
| vitest suite | `pnpm test` (+ no-bail variant, same selection) | type error in `@verter/types` spec | yes (same discipline, every round) | `× A1_SENTINEL_PLANT_VITEST_b27d41`, types 1 failed / 614 passed | types 614/614, token absent | DISCRIMINATING (round 3; rounds 1–2 recorded as selector findings) |
| conformance | `pnpm run gen:vue-goldens:check` | 1 mutated byte in a committed official golden | yes | exit 1 naming the exact drifted artifact | exit 0, 286 artifacts in sync | DISCRIMINATING |
| (supplementary) rustfmt | `cargo fmt --all --check` | misformatted fn in verter_span | yes | exit 1 naming the file | exit 0 | DISCRIMINATING |

Digest table for the raw sentinel files is in `command-proofs/index.md`'s final
digest table.
