#!/usr/bin/env node
// One auditable pass: append the rendered cell to performance-gates.toml,
// append the extension register to the Implementation Lock Record, rewrite the
// ruling's calibration section from the real session artifacts, then recompute
// every affected digest over real bytes.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";

const ROOT = process.cwd();
if (!existsSync("performance-gates.toml")) {
  console.error("run from the repository root");
  process.exit(2);
}
const BASE = "docs/arch/refactor/rev11/evidence/B6/cell-lock";
const GATES = "performance-gates.toml";
const LOCK = "docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md";
const RULING =
  "docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-23-B6-ROUTE-OVERHEAD-CELL-LOCK.md";
const REGISTRY = "docs/arch/architecture-lock/ledger/authority-registry.toml";
const IMPACT =
  "docs/arch/refactor/rev11/evidence/framework-conformance/performance-impact.md";

const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const cal = JSON.parse(readFileSync(`${BASE}/calibration/summary.json`, "utf8"));
const hold = JSON.parse(readFileSync(`${BASE}/holdout/summary.json`, "utf8"));
const cell = execFileSync(
  "node",
  [
    `${BASE}/emit-cell.mjs`,
    "--calibration",
    `${BASE}/calibration`,
    "--holdout",
    `${BASE}/holdout`,
    "--harness",
    "crates/verter_bench/examples/route_overhead_baseline.rs",
  ],
  { encoding: "utf8" },
);

const wallRel = /name = "wall_ns"[\s\S]*?no_regression_percent_max"\nlimit = ([0-9.]+)/.exec(cell)[1];
const rssRel = /name = "peak_rss_bytes"[\s\S]*?no_regression_percent_max"\nlimit = ([0-9.]+)/.exec(cell)[1];
const drift =
  (100 * Math.abs(hold.median_wall_ns - cal.median_wall_ns)) / cal.median_wall_ns;
const ms = (ns) => (ns / 1e6).toFixed(4);
const harnessBlob = execFileSync(
  "git",
  ["hash-object", "crates/verter_bench/examples/route_overhead_baseline.rs"],
  { encoding: "utf8" },
).trim();
const A6_WALL_NOISE = 1.4757;
const wallHeadroom = (20_000_000 / hold.median_wall_ns).toFixed(2);
const wallTripPercent = ((20_000_000 / hold.median_wall_ns - 1) * 100).toFixed(0);
const rssHeadroom = (134_217_728 / hold.max_peak_rss_bytes).toFixed(2);
const rssExcursion = ((cal.max_peak_rss_bytes / cal.median_peak_rss_bytes - 1) * 100).toFixed(2);
const cvVsA6 = (cal.wall_cv_percent / A6_WALL_NOISE).toFixed(2);
const boundVsA6 = (Number.parseFloat(wallRel) / 3.0).toFixed(2);
const mib = (b) => (b / 1048576).toFixed(2);

// ── 1. gates file ───────────────────────────────────────────────────────────
const banner = `

# ─────────────────────────────────────────────────────────────────────────────
# EXTENSION under this file's own SCOPE header: one B6-owned cell, added to
# close the route-overhead performance lock that BF1's exit #6 owned, that a
# later accepted disposition deferred to B6's own landing, and that B6's charter
# still imports as an acceptance condition. The contradiction and its resolution
# are recorded in
# docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-23-B6-ROUTE-OVERHEAD-CELL-LOCK.md.
# This does not touch any A6 or BF2 cell, the primary_suite, or any field above
# this marker, and it does not accept B6.
#
# WHY B6 DOES NOT CHOOSE THIS GATE. ADR-016 forbids post-measurement gate
# selection, and this repository already rejected exactly that pattern for
# BF2_OFFICIAL_COMPILER_INVOCATION_GOLDEN_GENERATE (see the open row at the end
# of this file). Every threshold here is therefore sourced from something other
# than B6:
#   * the two ABSOLUTE budgets are product/CI budgets derived from the ALREADY
#     LOCKED A6 cell above — 8 x A6's 2.5 ms/component cold budget for wall, and
#     half A6's 41-file host RSS catastrophe stop for memory. Neither is a
#     multiple of any observed route-overhead figure;
#   * the two RELATIVE bounds instantiate a formula frozen in
#     ${BASE}/pre-measure-registration.md
#     section 7 BEFORE the calibration session ran, applied to the B5 DIRECT
#     leg — the pre-existing one-shot StandaloneCompiler path that B6 will
#     replace, not B6's own arms.
# B6's existing timing/RSS results are contaminated audit evidence (they failed
# the idle-machine protocol) and were not read when this cell was written.
#
# MEASUREMENT PROTOCOL. The bootstrap shape of
# docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:
# digest-addressed pre-measure registration, then a 30-cold-invocation
# calibration session ([statistics].short_min_samples) whose ONLY output is the
# CV that instantiates the frozen formula, then a DISJOINT 30-cold-invocation
# holdout session that is the pass/fail evidence. Calibration numbers never
# became limits. Both sessions ran under the registration's idle-machine
# protocol (1-minute load average < 2.00, zero foreign cargo/rustc/nextest
# processes, low-power mode off) with runner.control_benchmark executed at the
# start AND end of each session; a session was void if control medians drifted
# by more than runner.max_control_drift_percent. Raw per-invocation samples,
# control readings and derivations:
#   ${BASE}/calibration/
#   ${BASE}/holdout/
#
# THREE ARMS DO NOT EXIST YET, AND THE HARNESS SAYS SO. prepared-first,
# prepared-repeat and batch are part of this cell's identity because they are
# B6's route surface, but they do not exist on the B5 tree this cell was locked
# from. The harness REFUSES those arms with an explicit error rather than
# reporting a number for them, so no fabricated baseline can be mistaken for a
# measurement. Their absolute ceilings are the B5-direct product budget: a route
# that did not exist at lock time does not earn a larger budget than the one it
# replaces.
#
# METRICS ARE CONJUNCTIVE, as for every cell in this file. An absolute pass does
# not offset a relative failure, and a timing pass does not offset a
# work-counter, zero-work, or output-oracle failure.
# ─────────────────────────────────────────────────────────────────────────────
`;
const gatesText = readFileSync(GATES, "utf8");
if (gatesText.includes("B6_COMPILER_ROUTE_OVERHEAD\"")) {
  console.error("gates file already contains the cell — refusing to double-append");
  process.exit(1);
}
writeFileSync(GATES, `${gatesText.replace(/\s*$/, "")}\n${banner}${cell}`);
console.log("gates: appended");

// ── 2. lock record extension register ───────────────────────────────────────
const lockText = readFileSync(LOCK, "utf8");
if (lockText.includes("# 13. Gate-file extension register")) {
  console.error("lock record already has section 13 — refusing to double-append");
  process.exit(1);
}
const section13 = `

---

# 13. Gate-file extension register

\`performance-gates.toml\`'s SCOPE header allows cells to be ADDED for later blocks and requires each
addition to carry "a new lock record digest and the same independent review class". This section is
that register: it is the record's list of gate-file extensions accepted after this record's original
acceptance. Adding a row changes this file's bytes and therefore its digest, which is exactly the
mechanism the SCOPE header asks for. **No row here may weaken, reweight, subset or reinterpret an
existing cell**, and none does: every extension below is strictly additive, and \`[primary_suite]\`,
\`[runner]\`, \`[statistics]\` and the A6 cell are untouched by all of them.

| # | Cell(s) added | Owner | Landed | Threshold source |
|---|---|---|---|---|
| E-1 | \`BF2_VUE_ORACLE_MANIFEST_GENERATE\`, \`BF2_SVELTE_ORACLE_MANIFEST_GENERATE\` | BF1 (for BF2) | \`630595072\` | A 10-invocation session of the BF1-owned, already-authored \`generate-official-case-manifests.mjs\` against the pinned oracle sources — a reference tool that is not BF2's candidate harness |
| E-2 | \`B6_COMPILER_ROUTE_OVERHEAD\` | B6 | this record's amending commit | Absolutes from the already-locked A6 cell's per-component product budget; relatives from a frozen formula instantiated on a neutral B5-direct calibration session, confirmed by a disjoint holdout |

**E-1 is recorded retroactively.** That extension landed without amending this record, so the
"new lock record digest" half of its own SCOPE rule went unsatisfied at the time. Recording it here
does not re-open or re-review it — its cells, thresholds and evidence are unchanged — it closes a
bookkeeping gap that would otherwise make this register look complete while omitting the first
extension that ever happened. The gap is disclosed rather than quietly backfilled.

**E-2 threshold provenance, in full.** This is the extension this amendment exists for, and it is the
one case in this file where the block that will be MEASURED by a cell is not permitted anywhere near
the choice of that cell's numbers.

- **Absolute wall \`20_000_000\` ns.** \`A6_META_COMPILE_40_COLD_RUST\` locks 100 ms for 40 components,
  i.e. 2.5 ms per component, for a **heavier** workload: a fresh host, upsert, load, per-component
  metadata, then a host-backed batch compile. The route-overhead cell's direct arm is
  \`StandaloneCompiler::compile\` over eight local sources with no host, no component-meta and no VFS.
  A strictly lighter path may not be budgeted slower than an already-locked heavier one at the same
  per-file product rate, so the budget is 8 x 2.5 ms.
- **Absolute peak RSS \`134_217_728\` bytes.** Half of A6's 256 MiB catastrophe stop for a 41-file
  host process, for an eight-file process with no host or session at all. Like A6's, this is a
  catastrophe stop; the tight fence is the relative bound.
- **Relative bounds.** \`max(3.0000, 2 x population CV)\` — \`[statistics].no_regression_floor_percent\`
  and \`noise_multiplier\` — frozen in
  \`docs/arch/refactor/rev11/evidence/B6/cell-lock/pre-measure-registration.md\` section 7 and
  committed **before** the calibration session ran. Instantiated on the B5 direct leg, the
  pre-existing one-shot path B6 replaces: wall CV ${cal.wall_cv_percent.toFixed(4)}% gives
  **${wallRel}%**, peak-RSS CV ${cal.rss_cv_percent.toFixed(4)}% gives **${rssRel}%**. Truncated at
  four decimal places, never rounded up: verification.md 8.3 is an upper bound.
- **What none of them is.** No threshold is \`k x\` any B6 observation, and none was read from B6's
  own measurement evidence. B6's existing timing and RSS figures additionally failed the
  idle-machine protocol and are retained as contaminated audit evidence only.

**E-2 sessions.** Calibration ${cal.invocations} cold invocations, median wall ${ms(cal.median_wall_ns)} ms,
max peak RSS ${mib(cal.max_peak_rss_bytes)} MiB. Disjoint holdout ${hold.invocations} cold invocations,
median wall ${ms(hold.median_wall_ns)} ms, max peak RSS ${mib(hold.max_peak_rss_bytes)} MiB. The
holdout is the pass/fail evidence and it passes both absolutes with the observed
holdout-to-calibration wall drift at ${drift.toFixed(4)}%, inside the ${wallRel}% bound. Every
invocation in both sessions reproduced the pinned output digest, so the correctness oracle held
throughout. Raw per-invocation samples and control readings are committed under
\`docs/arch/refactor/rev11/evidence/B6/cell-lock/\`.

**Which E-2 gates can actually fail.** Recorded here because a reader judging a future B6 run needs it,
and because the honest version is less flattering than the headline. BOTH wall metrics have near-zero
teeth, and the ABSOLUTE is the weaker of the two: 20 ms sits ${wallHeadroom}x above the holdout median
(${ms(hold.median_wall_ns)} ms) and first trips at roughly a ${wallTripPercent}% regression, while the
${wallRel}% relative bound rests on a ${cal.wall_cv_percent.toFixed(4)}% wall CV — ${cvVsA6}x A6's
1.4757% measured noise floor, so the bound is ${boundVsA6}x wider than A6's 3.0%. That is scale, not
sloppiness: the operation is ~${ms(hold.median_wall_ns)} ms against A6's ~70 ms, so cold-process startup
jitter dominates. The peak-RSS ABSOLUTE is weak too (${rssHeadroom}x headroom) and is a catastrophe stop,
as at A6. E-2's real discriminating power is the output oracle, the two-sided work counters
(8 / 8 / 5384 exact equality), the peak-RSS RELATIVE bound (${rssRel}% against a
${cal.rss_cv_percent.toFixed(4)}% CV and a ${rssExcursion}% observed excursion), and the three structural
route counters. A block wanting a tight wall bound adds an in-process arm excluding process startup and
calibrates it under this discipline; it does not narrow this bound after the fact, which ADR-016 forbids.

**E-2 forward hazard: the corpus pin versus the three unmeasured arms.** \`corpus_fingerprint\` pins
harness git-blob \`${harnessBlob}\`, and A6's discipline treats a run whose harness blob differs as not
this cell. That blob deliberately REFUSES \`--arm prepared-first|prepared-repeat|batch\` — which is why
no fabricated baseline exists for them — so measuring the three arms E-2 gates necessarily requires a
different harness blob and therefore necessarily breaks the pin. Neither this record, the ruling, nor
the registration says how that resolves, and this register does not invent a resolution. **Owner: B6**,
which must settle it before claiming E-2's arm metrics, by an explicit route (re-pin under the
recalibration rule with the direct arm's numbers reproduced, or a successor cell id) rather than by
silently measuring against a different blob.

**E-2 evidence caveat.** \`route.direct.payload_bytes\` is gated at exact equality 5384 but has no
per-invocation column in the raw sample rows: the recorded per-invocation evidence is the output digest,
and identical digests imply identical code bytes and therefore identical payload length. The section 10
condition-4 claim for payload_bytes is sound BY IMPLICATION from the digest, not by direct measurement,
and is stated that way rather than presented as a recorded number.

**E-2 outstanding governance step.** The SCOPE header requires an extension to carry a new lock record
digest AND the same independent review class (ADR-016). This register delivers the digest half. The
independent performance reviewer's sign-off on this specific addition is an OUTSTANDING follow-up, in
the same posture the BF2 banner records for E-1 — the cell is locked and binding, but no claim is made
here that that review class has signed it off.

**What this register does not do.** It does not accept B6, amend B6's charter, alter the DAG, or add a
ledger block row. B6 is still measured against E-2 later, on its own idle-machine run.
`;
writeFileSync(LOCK, `${lockText.replace(/\s*$/, "")}\n${section13}`);
console.log("lock record: section 13 appended");

// ── 3. ruling: replace the "not obtained" tail ──────────────────────────────
const rulingText = readFileSync(RULING, "utf8");
const marker = "## Calibration and holdout — not obtained";
const idx = rulingText.indexOf(marker);
if (idx < 0) {
  console.error("ruling: calibration section marker not found");
  process.exit(1);
}
const newTail = `## Calibration and holdout — obtained

The pre-measure registration requires a 1-minute load average below 2.00, no foreign
\`cargo\` / \`cargo-nextest\` / \`rustc\` / \`gate.mjs\` process, low-power mode off, and the
runner's control benchmark at session start and end. This host is shared with other
concurrent build agents and did not satisfy that protocol across two earlier attempts
totalling roughly four hours — 12:39–14:44 (1-minute load 2.94–40.63) and 17:07–18:02
(641 samples, load 3.84–74.34, peak 22 concurrent compilers) — both of which stopped
rather than measure under load (\`evidence/B6/cell-lock/idle-protocol-log.md\`). The
session below ran only after the maintainer authorised draining the host, and only once
the protocol actually held.

Compliance was enforced mechanically rather than by eye. The session runner re-checks
the load average **and** the foreign-compiler set before every measured step — both
control runs and all thirty invocations — and aborts the whole session if either fails;
the wait driver discards **both** sessions and restarts on any break, so a holdout can
never be re-drawn against a calibration already sitting on disk. The foreign-compiler
half of that per-step check was added before this window precisely because the residual
contention was bursty on a 30–40 s cadence against a ~15 s session, which a start-only
check would not have caught.

| | calibration | holdout |
|---|---:|---:|
| cold invocations | ${cal.invocations} | ${hold.invocations} |
| median wall | ${ms(cal.median_wall_ns)} ms | ${ms(hold.median_wall_ns)} ms |
| min / max wall | ${ms(cal.min_wall_ns)} / ${ms(cal.max_wall_ns)} ms | ${ms(hold.min_wall_ns)} / ${ms(hold.max_wall_ns)} ms |
| population CV (wall) | ${cal.wall_cv_percent.toFixed(4)}% | ${hold.wall_cv_percent.toFixed(4)}% |
| max peak RSS | ${mib(cal.max_peak_rss_bytes)} MiB | ${mib(hold.max_peak_rss_bytes)} MiB |
| population CV (RSS) | ${cal.rss_cv_percent.toFixed(4)}% | ${hold.rss_cv_percent.toFixed(4)}% |
| control drift | ${cal.control_drift_percent.toFixed(4)}% | ${hold.control_drift_percent.toFixed(4)}% |
| load at session start | ${cal.idle.load} | ${hold.idle.load} |

Derivation, by the formula frozen in section 7 of the registration before either session
ran: wall \`max(3.0000, 2 x ${cal.wall_cv_percent.toFixed(4)}) = ${wallRel}\`, peak RSS
\`max(3.0000, 2 x ${cal.rss_cv_percent.toFixed(4)}) = ${rssRel}\`. Truncated at four decimal
places, never rounded up.

Holdout verdict against section 10, all six conjunctive:

1. median wall ${ms(hold.median_wall_ns)} ms <= the 20 ms pre-registered product budget — **pass**;
2. max peak RSS ${mib(hold.max_peak_rss_bytes)} MiB <= the 128 MiB catastrophe stop — **pass**;
3. holdout-to-calibration wall drift ${drift.toFixed(4)}% <= ${wallRel}% — **pass**;
4. work counters equal section 8 for the direct arm (8 compiles, 8 artifacts, 5384 payload bytes) — **pass**;
5. output digest equal to the correctness pin on every invocation of both sessions — **pass**;
6. idle-machine protocol held for the whole session, control drift inside
   \`runner.max_control_drift_percent\` — **pass**.

The cell is therefore locked into repo-root \`performance-gates.toml\` as an EXTENSION under that
file's SCOPE header, and registered as row E-2 of the new section 13 extension register in the
Implementation Lock Record, which is what the SCOPE header's "new lock record digest" requires.
Both absolute budgets were registered before any of this ran and are unchanged by it; the observed
medians sit far inside them, which is the expected shape for a product budget rather than a fit.

**This ruling still does not accept B6.** B6 is measured against this cell later, on its own
idle-machine run, and must satisfy every metric conjunctively — including the three arms
(prepared-first, prepared-repeat, batch) that do not exist on the B5 tree and that the harness
deliberately refuses rather than fabricating a baseline for.

Correctness pin (load-insensitive, taken 2026-08-23, reproduced by every invocation of both sessions):

- \`output_digest\` = \`577f62e3ba72dcf39cd56d62285372b249752be1c1b8c3bedf02e70070446131\`
- \`payload_bytes\` = 5384
- harness git-blob \`${harnessBlob}\`
- request-identity sha256 \`bf427b56a4f46a151d818c52e9493fd5817da4a7ac2e74352612962ea2f4ab80\`
`;
writeFileSync(RULING, rulingText.slice(0, idx) + newTail);
console.log("ruling: calibration section rewritten");

// ── 3b. the deferral record this ruling cites ──────────────────────────────
// performance-impact.md is the document the ruling names as the deferral
// record. Leaving it saying the cell is unlocked would contradict
// performance-gates.toml for exactly the reader most likely to consult it.
const impactText = readFileSync(IMPACT, "utf8");
const staleProse = `The cell is **not yet a locked \`[[cell]]\`**: idle-machine calibration and
holdout were not obtained (see \`../B6/cell-lock/idle-protocol-log.md\`). No
threshold is invented from B6's contaminated timing/RSS.`;
if (!impactText.includes(staleProse)) {
  console.error("performance-impact: stale prose not found");
  process.exit(1);
}
const freshProse = `The cell is now **LOCKED** in repo-root \`performance-gates.toml\` as an EXTENSION
under that file's SCOPE header, and registered as row E-2 of the Implementation
Lock Record's section 13 extension register. No threshold is derived from B6's
contaminated timing/RSS: the absolutes come from the already-locked A6 cell and
the relative bounds instantiate a pre-registered formula on a neutral B5-direct
calibration confirmed by a disjoint holdout (see
\`../B6/cell-lock/idle-protocol-log.md\` for the machine conditions). Locking the
cell does NOT accept B6, which is still measured against it later.`;
let impactOut = impactText.replace(staleProse, freshProse);
const staleRow =
  "| \`B6_COMPILER_ROUTE_OVERHEAD\` | pre-B6 gate-authority repair, then B6 | **PRE-REGISTERED, NOT YET LOCKED.**";
if (!impactOut.includes(staleRow)) {
  console.error("performance-impact: stale table row not found");
  process.exit(1);
}
const rowStart = impactOut.indexOf(staleRow);
const rowEnd = impactOut.indexOf("\n", rowStart);
impactOut =
  impactOut.slice(0, rowStart) +
  "| \`B6_COMPILER_ROUTE_OVERHEAD\` | pre-B6 gate-authority repair, then B6 | " +
  "**FROZEN** — see the ruling cited above. Identical corpus across direct, prepared " +
  "first/repeat, and batch; output digest, reuse/cold-build counts, latency/RSS. " +
  `Absolutes 20 ms / 128 MiB from the locked A6 cell; relatives ${wallRel}% wall and ` +
  `${rssRel}% peak RSS from a 30-invocation B5-direct calibration confirmed by a disjoint ` +
  "30-invocation holdout. Only the DIRECT arm exists on the B5 tree — the harness refuses " +
  "the other three rather than reporting a number, and they inherit the same ceilings and " +
  "the same relative wall bound |" +
  impactOut.slice(rowEnd);
writeFileSync(IMPACT, impactOut);
console.log("performance-impact: deferral record updated");

// ── 4. digests over real bytes ──────────────────────────────────────────────
const rulingDigest = sha(RULING);
let reg = readFileSync(REGISTRY, "utf8");
// The registry comment described the pre-measurement state ("document-only
// until an idle calibration+holdout can instantiate the relative bound").
// That is no longer true once the cell is locked.
const staleComment = `# or add a DAG/ledger block. Document-only until an idle calibration+holdout
# can instantiate the relative bound.`;
if (!reg.includes(staleComment)) {
  console.error("registry: expected stale comment not found");
  process.exit(1);
}
reg = reg.replace(
  staleComment,
  `# or add a DAG/ledger block. The cell is locked into repo-root
# performance-gates.toml from a neutral B5-direct calibration and a disjoint
# holdout, registered as row E-2 of the Implementation Lock Record extension
# register.`,
);
const oldRow = /(\[\[document\]\]\nid = "RULING-2026-08-23-B6-ROUTE-OVERHEAD-CELL-LOCK"\nkind = "RULING"\npath = "[^"]+"\nsha256 = ")([0-9a-f]{64})(")/;
if (!oldRow.test(reg)) {
  console.error("registry: B6 ruling row not found");
  process.exit(1);
}
writeFileSync(REGISTRY, reg.replace(oldRow, `$1${rulingDigest}$3`));

console.log(`\nruling sha256          ${rulingDigest}`);
console.log(`lock record sha256     ${sha(LOCK)}`);
console.log(`performance-gates sha256 ${sha(GATES)}`);
console.log(`wall no_regression     ${wallRel}%`);
console.log(`rss  no_regression     ${rssRel}%`);
