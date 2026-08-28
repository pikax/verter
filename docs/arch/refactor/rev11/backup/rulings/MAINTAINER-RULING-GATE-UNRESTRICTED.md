---
ruling_id: "GATE-UNRESTRICTED"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["program-wide gate execution parameters"]
source_file: "MAINTAINER-RULING-GATE-UNRESTRICTED.md"
summary: "Ruling 1: re-diagnoses the OOM reboots as caused by CONCURRENT gate runs, not single-gate parallelism — one gate at a time stays the real control (unrelaxed); within that one gate, use full host parallelism, dropping the earlier --build-jobs/--test-threads throttle; keep a hard --memory-limit RSS-kill watchdog (18GiB on a 24GiB host). Ruling 2: every regression found gets a test hardened structurally (privacy/type-state/sealed-trait first, then whole-artifact assertion, then a proven plant-red-green, only then an ordinary assertion) — never a name-keyed source scanner. Ruling 3: adopts remaining audit recommendations (consumer-falsification gate, capped ledger notes, fixing the gate's own false-greens). CORRECTION (same day): the leak hypothesis is falsified by root-cause investigation — the dominant memory consumer is the BUILD (concurrent rustc), not the guards; refines policy to --build-jobs at tool default (do not raise to 8), --test-threads raised to 8, --memory-limit 18GiB kept, one-gate-at-a-time kept."
supersedes:
  - document: "an earlier memory-ceiling ruling (not part of this corpus)"
    claim: "The --build-jobs 2 --test-threads 2 throttle."
superseded_by: []
contradicts: []
notes: "Records a residual, explicitly unresolved tension: an earlier program ruling claimed even CARGO_BUILD_JOBS=2 was insufficient because Surface 1 (nextest) drove memory critical, which is in tension with this document's build-dominates finding — the document states both cannot be fully right and defers to the watchdog as an empirical resolver rather than forcing a rule. This is a genuine open self-contradiction in the corpus's ancestry, flagged here per the migration brief rather than resolved."
---

# Maintainer ruling — run the gate unrestricted under a hard memory kill (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax). Binding immediately.
SUPERSEDES the `--build-jobs 2 --test-threads 2` throttle in the earlier memory-ceiling ruling.

## Verbatim

> the machine oom was probably caused by gate running multiple times, you may run gate unrestricted
> with a check to confirm it does not balloon the memory, if it balloons it should kill straight up.
> I agree with the other suggestions, we must always strengthen the tests against any regression we
> find, it just makes it harder for an agent to introduce it without changing something they
> shouldn't

## Ruling 1 — the gate runs unrestricted, under a watchdog

**The re-diagnosis:** the OOM reboots were caused by CONCURRENT gate runs, not by a single gate being
too parallel. So the correct control is the concurrency rule, and the per-gate throttle was collateral
damage — it has been taxing every run since for no correctness benefit.

**New policy:**
- **ONE gate at a time on this machine, strictly.** This is the real control and it is NOT relaxed.
  Before starting a gate, check for a live `gate.mjs`/`nextest`/`cargo` build and wait it out with a
  single blocking foreground poll. Do not run cargo-heavy work in another track while a gate is live.
- **Within that one gate, use the whole machine.** Drop `--build-jobs 2 --test-threads 2`. Let the
  gate default to host parallelism (8 cores here) or set it explicitly to the core count.
- **Keep a hard RSS ceiling that kills immediately on a balloon.** `--memory-limit` is exactly this
  mechanism: an RSS-sampling watchdog that aborts the active child tree fast and exits 123
  (`ABORTED — memory ceiling`). Set it with real headroom on a 24 GiB host — `--memory-limit 18GiB`
  — so a runaway dies before the machine does.
- **Canonical invocation from now on:**
  `node scripts/gate.mjs --memory-limit 18GiB 2>&1 | tee /tmp/<block>-gate.txt`
- Unchanged: exit 123/124/125, or a run with no terminal summary, is **NEVER a PASS**. An aborted or
  incomplete run is NOT PROVEN and must never be recorded as one. `zsh` `$PIPESTATUS` is empty — read
  `${pipestatus[1]}`.
- Unchanged: the gate still runs ONCE per landing, at landing readiness, never mid-work or per round
  (the gate-scope ruling).

## Ruling 2 — every regression found is hardened against structurally

> we must always strengthen the tests against any regression we find, it just makes it harder for an
> agent to introduce it without changing something they shouldn't

Every regression this program finds gets a test that makes REINTRODUCING it require changing
something an agent should visibly not be touching. The bar is not "a test that fails when the bug is
present" — it is "a test an agent cannot route around without an obviously illegitimate edit".

Concretely, in rough order of preference:
1. **Structural confinement** — privacy/visibility (`E0603`), type-state, sealed traits, marker
   traits, unforgeable witnesses, mutual compile-coupling. Reintroducing the bug becomes a compile
   error, not a test failure. This is the strongest and the program has landed real examples (the
   `NoTypeExpr` marker; the mutual `mod` coupling that turned a vacuous-green suite into `E0433`).
2. **Assert the artifact, not a proxy.** The Rollup target that asserted a probe's boolean instead of
   the map is the anti-pattern — a lying boolean greened it. Assert whole-artifact parity.
3. **Prove the test discriminates** — plant the defect, watch it go RED, revert, watch GREEN, and
   prove the plant was applied, unique and new. A green planted run means the plant failed until
   proven otherwise.
4. Only then, an ordinary assertion.

**Never a name-keyed source-tree scanner as landed enforcement** (the standing forward-only rule).
Structural first; a scanner is a WIP artifact, not a control.

## Ruling 3 — the remaining audit recommendations are ADOPTED

- **R3 — consumer-falsification gate.** On a block producing an artifact other blocks consume (a
  harness, oracle, contract, lock record), one downstream consumer must actually drive it against a
  real case before acceptance; mandate rounds there cap at 2 and the third slot is spent on that
  falsification. Production-behaviour blocks keep all three mandates at full force. Evidence: BF2
  passed 3/3 TWICE with an oracle unsound by construction; the defect surfaced only when a consumer
  used it, costing ~29 h.
- **R5 — cap the record.** Ledger `notes` ≤ 3 sentences plus a pointer to the evidence file. Stop
  committing raw gate transcripts. Fast-forward landings always.
- **R6 — fix the gate's own false-greens** before further acceptances: the shared
  `.verter-probe-recompile` path collision (concurrent probes exit 0 reporting `fresh:true` while
  recording recompile `outcome:"error"`) and `validate-program-state.mjs` printing `OK` through a
  binding failure. Both were found by review and wrongly dispositioned out of scope.
- **Mechanism, not another rule.** When an incident recurs, build a script, a permission boundary or
  a pre-flight check. Seven rules in this program had their originating incident recur AFTER the rule
  was written; a rule that does not change behaviour is not a control.

## CORRECTION — refined by root-cause evidence (2026-08-17, same day)

The leak hypothesis behind the memory ceiling is **FALSIFIED**: there is no `OnceLock`-retained
repo-wide `syn` AST. `output_projector_residual_guards.rs` has zero statics; `architecture_guards.rs`
has exactly one scan-retaining `OnceLock` and it holds violation TUPLES, empty on a clean tree. The
"~11 OnceLocks" was a grep miscount over string literals and doc comments used as synthetic fixtures.
The 11 whole-workspace scans are text-only `read_to_string`→`lines()` loops that drop each file.

**But the same investigation found the dominant consumer is the BUILD, not the tests:** five
concurrent `rustc` totalling ~3.5 GB — and that was BEFORE reaching the 694k-line `verter_session`
crate or the 207 MB binary link, with the workspace compiled twice per gate. The fact that
`--build-jobs 2` stopped the incidents corroborates this: the guards do not even run during the
archive build.

**Therefore "unrestricted on every axis" is NOT safe** and would risk a third reboot. Refined policy,
which keeps the maintainer's intent (stop needless throttling; let the watchdog kill a balloon)
without ignoring the evidence:

- **`--build-jobs`: leave at the tool's own default (4 on this host). Do NOT raise to 8.** Concurrent
  `rustc` on the largest crates is the measured dominant consumer.
- **`--test-threads`: raise to full host parallelism (8).** This is the axis the evidence says is
  safe to open, and it is where the 1,321 s Surface-1 cost sits.
- **`--memory-limit 18GiB` stays** — it is the real protection and makes a wrong guess kill the run
  instead of the machine.
- **One gate at a time stays.** Unchanged and non-negotiable.
- Canonical: `node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB 2>&1 | tee /tmp/<name>-gate.txt`

Residual conflict, recorded honestly: an earlier program ruling states that even `CARGO_BUILD_JOBS=2`
was insufficient because *Surface 1* (the nextest run) drove memory critical — which is in tension
with the finding that the build dominates. Both cannot be fully right. The watchdog resolves it
empirically: if a full-`--test-threads` run trips 18 GiB, we learn the test run is the consumer and
dial back. That is the correct way to settle it — one bounded experiment, not another rule.

**Cheap latency win found alongside (not memory):** `production_src_files()` re-reads 696 files /
29.75 MB / 694k lines and is called 18× inside the `HOT_TERMINAL_SINKS` loop. Hoisting it out (~5
lines, zero detection risk) plus sharing the corpus across three parses in the >120 s
`hot_materialize_scanner_flags_in_memory_injected_offender` is under an hour's work.

**Stale doc found:** `CLAUDE.md` says "~25 verter_session integration binaries"; there are **2**.
