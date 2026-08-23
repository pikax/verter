#!/usr/bin/env node
// Renders the B6_COMPILER_ROUTE_OVERHEAD [[cell]] block from the committed
// session artifacts. Every number is read from disk; none is typed by hand.
//
//   node emit-cell.mjs --calibration <dir> --holdout <dir> --harness <path>
//
// Absolute limits come from pre-measure-registration.md sections 6.1/6.2 and are
// constants here: a run cannot move them. The only measured inputs are the two
// no-regression bounds, instantiated from the calibration CV by the frozen
// formula in section 7.

import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import process from "node:process";

// pre-measure-registration.md 6.1 / 6.2 — product budgets, not fits.
const WALL_ABS_NS = 20_000_000;
const RSS_ABS_BYTES = 134_217_728;
// section 8 — structural work contracts.
const COMPILE_CALLS = 8;
const ARTIFACT_COUNT = 8;
const PAYLOAD_BYTES = 5384;
// section 9 — correctness pin.
const OUTPUT_DIGEST = "577f62e3ba72dcf39cd56d62285372b249752be1c1b8c3bedf02e70070446131";
const REQUEST_IDENTITY_SHA =
  "bf427b56a4f46a151d818c52e9493fd5817da4a7ac2e74352612962ea2f4ab80";

function arg(name) {
  const i = process.argv.indexOf(name);
  if (i < 0 || i + 1 >= process.argv.length) {
    process.stderr.write(`missing ${name}\n`);
    process.exit(2);
  }
  return process.argv[i + 1];
}

// section 7: "The result is not rounded up." Truncating at 4 dp can only tighten
// the bound, never loosen it.
function truncate4(v) {
  return (Math.floor(v * 10_000) / 10_000).toFixed(4);
}

function frozenRelative(cvPercent) {
  return truncate4(Math.max(3.0, 2 * cvPercent));
}

const calibration = JSON.parse(
  readFileSync(resolve(arg("--calibration"), "summary.json"), "utf8"),
);
const holdout = JSON.parse(readFileSync(resolve(arg("--holdout"), "summary.json"), "utf8"));
const harness = arg("--harness");
const harnessBlob = execFileSync("git", ["hash-object", harness], { encoding: "utf8" }).trim();

// Holdout is the pass/fail evidence (section 10). Refuse to emit a cell the
// holdout does not actually pass.
const wallRel = frozenRelative(calibration.wall_cv_percent);
const rssRel = frozenRelative(calibration.rss_cv_percent);
// Reporting figures for the cell's own "which gates can actually fail" note.
const cal = calibration;
const hold = holdout;
const ms = (ns) => (ns / 1e6).toFixed(4);
// A6's measured wall noise floor, from the locked A6 cell's derivation.
const A6_WALL_NOISE = 1.4757;
const wallHeadroom = (WALL_ABS_NS / hold.median_wall_ns).toFixed(2);
const wallTripPercent = ((WALL_ABS_NS / hold.median_wall_ns - 1) * 100).toFixed(0);
const rssHeadroom = (RSS_ABS_BYTES / hold.max_peak_rss_bytes).toFixed(2);
const rssExcursion = (
  (cal.max_peak_rss_bytes / cal.median_peak_rss_bytes - 1) * 100
).toFixed(2);
const cvVsA6 = (cal.wall_cv_percent / A6_WALL_NOISE).toFixed(2);
const boundVsA6 = (Number.parseFloat(wallRel) / 3.0).toFixed(2);

const failures = [];
if (!(holdout.median_wall_ns <= WALL_ABS_NS))
  failures.push(`holdout median wall ${holdout.median_wall_ns} > ${WALL_ABS_NS}`);
if (!(holdout.max_peak_rss_bytes <= RSS_ABS_BYTES))
  failures.push(`holdout max rss ${holdout.max_peak_rss_bytes} > ${RSS_ABS_BYTES}`);
const drift =
  (100 * Math.abs(holdout.median_wall_ns - calibration.median_wall_ns)) /
  calibration.median_wall_ns;
if (!(drift <= Number.parseFloat(wallRel)))
  failures.push(`holdout/calibration wall drift ${drift.toFixed(4)}% > ${wallRel}% (section 10.3)`);
for (const [label, s] of [
  ["calibration", calibration],
  ["holdout", holdout],
]) {
  if (s.unique_output_digests.length !== 1 || s.unique_output_digests[0] !== OUTPUT_DIGEST)
    failures.push(`${label} output digest ${JSON.stringify(s.unique_output_digests)}`);
}
if (failures.length > 0) {
  process.stderr.write(`HOLDOUT FAILED — no cell emitted:\n  ${failures.join("\n  ")}\n`);
  process.exit(1);
}

const out = `[[cell]]
id = "B6_COMPILER_ROUTE_OVERHEAD"
owner = "B6"
# The four route arms B6 owns, over one RuntimeClient product. Only the DIRECT
# arm exists on the B5 tree this cell was locked from; the harness REFUSES the
# other three rather than reporting a fabricated number for them.
operation = "route_overhead_direct_prepared_first_prepared_repeat_batch; product=RuntimeClient; frameworks=vue+svelte"
corpus_fingerprint = "git-blob:${harnessBlob} (crates/verter_bench/examples/route_overhead_baseline.rs); eight in-process sources (4 Vue, 4 Svelte), no fixture directory, no third-party corpus"
# SHA-256 over the literal request-identity string frozen in
# evidence/B6/cell-lock/pre-measure-registration.md section 11 (no trailing newline).
normalized_product_request_digest = "${REQUEST_IDENTITY_SHA}"
result_contract = "vue_svelte_runtime_client_artifact_set"
semantic_profile = "none"
execution_profile = "cargo build -p verter_bench --release --example route_overhead_baseline, CARGO_BUILD_JOBS=4, system allocator (no attribution feature), single-threaded, one cold process per sample"
cache_state = "cold_process_per_invocation"
threads = 1
boundary = "rust"
required = true

[cell.validity]
required_product_kinds = ["runtime_client_compile"]
required_output_profiles = []
required_presentation_profiles = []
required_serialization_profiles = []
required_mapping_kinds = []
required_diagnostics_policy = "not_applicable"
required_exactness = "exact"
# Payload = concatenation over corpus order of id || 0x00 || artifact.code() ||
# 0x00 || decimal(styles.len()) || 0x0a. Load-insensitive: a candidate that emits
# different code faster fails here regardless of machine conditions.
output_oracle = "sha256(route_overhead_payload) == ${OUTPUT_DIGEST}"
zero_counter_assertions = ["network.dns_resolution_attempts", "network.socket_connect_attempts", "artifact.ide_companion_published", "artifact.runtime_server_published", "artifact.declarations_published", "artifact.public_api_published", "artifact.analysis_published"]

# ── wall clock ──
#
# WHICH GATES CAN ACTUALLY FAIL (read this before judging a B6 run). BOTH wall
# metrics below have near-zero teeth on the B5-direct arm, and the absolute is
# the weaker of the two:
#   * the 20 ms absolute sits ${wallHeadroom}x above the measured holdout median
#     (${ms(hold.median_wall_ns)} ms) and first trips at roughly a ${wallTripPercent}% regression;
#   * the ${wallRel}% relative bound is derived from a ${cal.wall_cv_percent.toFixed(4)}% wall CV, which is
#     ${cvVsA6}x A6's 1.4757% measured noise floor, making the bound ${boundVsA6}x wider
#     than A6's 3.0%. The cause is scale, not sloppiness: this operation is
#     ~${ms(hold.median_wall_ns)} ms against A6's ~70 ms, so cold-process startup jitter
#     dominates a measurement two orders of magnitude shorter.
# The cell's real discriminating power is therefore the output oracle, the
# two-sided work counters (8 / 8 / 5384 exact equality), the peak-RSS RELATIVE
# bound (${rssRel}% against a ${cal.rss_cv_percent.toFixed(4)}% CV and a ${rssExcursion}% observed
# excursion), and the three structural route counters. The peak-RSS ABSOLUTE is
# weak too (${rssHeadroom}x headroom) and is, as at A6, a catastrophe stop rather than a
# fence. A future block wanting a tight wall bound should add an in-process arm
# that excludes process startup and calibrate it under this same discipline --
# NOT narrow this bound after the fact, which ADR-016 forbids.
#
# Absolute: 8 x A6's 2.5 ms/component cold product budget
# (A6_META_COMPILE_40_COLD_RUST locks 100 ms for 40 components). This arm is
# STRICTLY LIGHTER than that locked path — no host, no component-meta, no VFS —
# so it may not be budgeted slower at the same per-file product rate. It is NOT
# a multiple of any observed route-overhead median.
[[cell.metric]]
name = "wall_ns"
statistic = "median"
comparison = "absolute_max"
limit = ${WALL_ABS_NS}

# Relative: max(3.0000, 2 x population CV) over the 30-sample calibration
# session, per the formula frozen in pre-measure-registration.md section 7
# BEFORE that session ran. Calibration wall CV was
# ${calibration.wall_cv_percent.toFixed(4)}%. Truncated at 4 dp, never rounded up.
[[cell.metric]]
name = "wall_ns"
statistic = "median"
comparison = "no_regression_percent_max"
limit = ${wallRel}

# ── peak RSS ──
# Absolute: half of A6's 256 MiB 41-file HOST catastrophe stop. This is a
# standalone eight-file process with no host or session. A catastrophe stop, not
# a fit; the tight fence is the relative bound below.
[[cell.metric]]
name = "peak_rss_bytes"
statistic = "max"
comparison = "absolute_max"
limit = ${RSS_ABS_BYTES}

# Calibration peak-RSS CV was ${calibration.rss_cv_percent.toFixed(4)}%.
[[cell.metric]]
name = "peak_rss_bytes"
statistic = "max"
comparison = "no_regression_percent_max"
limit = ${rssRel}

# ── the three arms that do not exist on the B5 tree ──
# They share the B5-direct product ceiling. A route that did not exist at lock
# time does not earn a larger budget than the one it replaces. They also carry
# the SAME relative wall bound: the registration (section 7) freezes "the same
# wall percentage is the no-regression bound on every wall metric in the cell",
# because prepared/batch have no independent B5 noise and the B5-direct
# calibration is their comparable leg. Metric names are free-form strings to the
# validator, so a bound on wall_ns does NOT implicitly cover these three --
# each is stated explicitly or it does not exist.
[[cell.metric]]
name = "route.prepared_first.wall_ns"
statistic = "median"
comparison = "absolute_max"
limit = ${WALL_ABS_NS}

[[cell.metric]]
name = "route.prepared_first.wall_ns"
statistic = "median"
comparison = "no_regression_percent_max"
limit = ${wallRel}

[[cell.metric]]
name = "route.prepared_repeat.wall_ns"
statistic = "median"
comparison = "absolute_max"
limit = ${WALL_ABS_NS}

[[cell.metric]]
name = "route.prepared_repeat.wall_ns"
statistic = "median"
comparison = "no_regression_percent_max"
limit = ${wallRel}

[[cell.metric]]
name = "route.batch.wall_ns"
statistic = "median"
comparison = "absolute_max"
limit = ${WALL_ABS_NS}

[[cell.metric]]
name = "route.batch.wall_ns"
statistic = "median"
comparison = "no_regression_percent_max"
limit = ${wallRel}

# ── structural work counters (not timed) ──
# Two-sided: a faster run that compiled fewer files, or emitted less, fails.
[[cell.metric]]
name = "route.direct.compile_calls"
statistic = "max"
comparison = "absolute_max"
limit = ${COMPILE_CALLS}

[[cell.metric]]
name = "route.direct.compile_calls"
statistic = "max"
comparison = "absolute_min"
limit = ${COMPILE_CALLS}

[[cell.metric]]
name = "route.direct.artifact_count"
statistic = "max"
comparison = "absolute_max"
limit = ${ARTIFACT_COUNT}

[[cell.metric]]
name = "route.direct.artifact_count"
statistic = "max"
comparison = "absolute_min"
limit = ${ARTIFACT_COUNT}

[[cell.metric]]
name = "route.direct.payload_bytes"
statistic = "max"
comparison = "absolute_max"
limit = ${PAYLOAD_BYTES}

[[cell.metric]]
name = "route.direct.payload_bytes"
statistic = "max"
comparison = "absolute_min"
limit = ${PAYLOAD_BYTES}

# Reuse IS the prepared route. A reparse on repeat is the overhead this cell exists to catch.
[[cell.metric]]
name = "route.prepared_repeat.additional_parse_calls"
statistic = "max"
comparison = "absolute_max"
limit = 0

# A batch of 8 unique sources parses each exactly once.
[[cell.metric]]
name = "route.batch.unique_source_parse_calls"
statistic = "max"
comparison = "absolute_max"
limit = ${COMPILE_CALLS}

# parses / unique sources. Identically 1.0 on the DIRECT arm (8/8).
[[cell.metric]]
name = "parse_amplification"
statistic = "max"
comparison = "absolute_max"
limit = 1.0

[cell.competitor]
rule = "none"
competitor_ids = []
max_wall_slowdown_percent = "not_applicable"
max_peak_rss_increase_percent = "not_applicable"
post_result_exception_allowed = false

[cell.memory]
owner_budget_bytes = "not_applicable"
allocator_slack_bytes = "not_applicable"
quiescence_protocol = "not_applicable"
max_positive_slope_bytes_per_hour = "not_applicable"
`;

process.stdout.write(out);
process.stderr.write(
  `emitted; calibration wall CV ${calibration.wall_cv_percent.toFixed(4)}% -> ${wallRel}%, ` +
    `rss CV ${calibration.rss_cv_percent.toFixed(4)}% -> ${rssRel}%, ` +
    `holdout/calibration drift ${drift.toFixed(4)}%\n`,
);
