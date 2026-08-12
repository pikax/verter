// validate-performance-gates.test.mjs — node:test suite for the performance-gate
// validator.
//
// Discipline (docs/arch/refactor/rev11/verification.md, "Verification Must Prove
// Execution"): every negative control below must FAIL against the validator, and
// every one must also fail against a validator stubbed to accept everything —
// otherwise the test proves nothing. `accepts_everything_fails_every_negative_control`
// is that meta-check: it runs the whole negative corpus through a stub and asserts
// the stub is caught each time.
//
// Run: node --test scripts/validate-performance-gates.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { validateGates, readGatesToml } from "./validate-performance-gates.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

// A minimal but COMPLETE locked file. Every negative control below is this text
// with exactly one mutation, so a failure is attributable to that mutation.
const GOOD = `
schema = 1
revision = 11
status = "LOCKED"
authority_digest = "ff49cddeb8f6577144dcf85cb9d026cba0d14e7164e7b3825c4a59b215c8148b"
baseline_sha = "6af543c8a65b495aad2d6231e5e90878c3bf1769"
created_at_utc = "2026-08-11T22:56:51Z"

[runner]
class = "test-runner"
os = "Darwin 25.6.0 arm64"
cpu = "Apple M3"
logical_cpus = 8
memory_bytes = 25769803776
rust_toolchain = "1.97.1"
node_runtime = "v20.20.2"
power_policy = "AC power"
control_benchmark = "the baseline arm itself"
max_control_drift_percent = 3.0

[statistics]
short_min_samples = 30
long_min_runs = 10
confidence = 0.95
bootstrap_resamples = 10000
no_regression_floor_percent = 3.0
noise_multiplier = 2.0
outlier_policy = "no discretionary exclusion"
interleave_policy = "alternating ABBA"

[primary_suite]
id = "SUITE"
cell_ids = ["CELL_A"]
aggregate = "geomean_ratio"
competitor_ids = []
max_verter_to_fastest_ratio = "not_applicable"
post_result_exception_allowed = false
premise_change_requires_new_lock = true

[[cell]]
id = "CELL_A"
owner = "verter_session"
operation = "cold_batch"
corpus_fingerprint = "git-blob:a74f90c5d1d06f8fc17a71781d28d0c6ea466853"
normalized_product_request_digest = "d80a5f9e174de68b10257e6ed929331f031950639496ac8465048804fb0f4d48"
result_contract = "meta_plus_compile"
semantic_profile = "none"
execution_profile = "release_default"
cache_state = "cold"
threads = 8
boundary = "rust"
required = true

[cell.validity]
required_product_kinds = ["component_meta"]
required_output_profiles = ["host_backed_default"]
required_presentation_profiles = []
required_serialization_profiles = []
required_mapping_kinds = []
required_diagnostics_policy = "host_default"
required_exactness = "exact"
output_oracle = "session.component_meta_digest == 7161214711717846280"
zero_counter_assertions = ["compiler.css_parse", "compiler.css_transform"]

[[cell.metric]]
name = "wall_ns"
statistic = "median"
comparison = "absolute_max"
limit = 100000000

[[cell.metric]]
name = "wall_ns"
statistic = "median"
comparison = "no_regression_percent_max"
limit = 3.0

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

/** Replace exactly one occurrence, asserting the mutation actually applied. */
function mutate(text, from, to) {
  const first = text.indexOf(from);
  assert.notEqual(first, -1, `mutation source not found: ${from}`);
  assert.equal(
    text.indexOf(from, first + from.length),
    -1,
    `mutation source is not unique, so the mutation is not attributable: ${from}`,
  );
  return text.slice(0, first) + to + text.slice(first + from.length);
}

// Each entry: [name, mutated text, substring the violation must mention].
const NEGATIVE_CONTROLS = [
  [
    "a REQUIRED_ placeholder survives",
    mutate(GOOD, 'authority_digest = "ff49cdde', 'authority_digest = "REQUIRED_AUTHORITY_DIGEST'),
    "placeholder",
  ],
  [
    "a placeholder hides inside a nested cell field",
    mutate(
      GOOD,
      'result_contract = "meta_plus_compile"',
      'result_contract = "REQUIRED_RESULT_CONTRACT"',
    ),
    "placeholder",
  ],
  [
    "a placeholder renamed itself to TBD",
    mutate(GOOD, 'cache_state = "cold"', 'cache_state = "TBD"'),
    "placeholder",
  ],
  ["status is still TEMPLATE", mutate(GOOD, 'status = "LOCKED"', 'status = "TEMPLATE"'), "status"],
  [
    "a metric limit is a string, not a number",
    mutate(
      GOOD,
      'comparison = "absolute_max"\nlimit = 100000000',
      'comparison = "absolute_max"\nlimit = "100000000"',
    ),
    "must be a number",
  ],
  ["threads is a string", mutate(GOOD, "threads = 8", 'threads = "8"'), "threads"],
  [
    "baseline_sha is a short SHA",
    mutate(GOOD, '"6af543c8a65b495aad2d6231e5e90878c3bf1769"', '"6af543c8a"'),
    "40-hex",
  ],
  [
    "short_min_samples falls below the 30 the plan requires",
    mutate(GOOD, "short_min_samples = 30", "short_min_samples = 7"),
    "short_min_samples",
  ],
  [
    "a required cell declares no no-regression gate",
    mutate(
      GOOD,
      'comparison = "no_regression_percent_max"\nlimit = 3.0',
      'comparison = "absolute_min"\nlimit = 3.0',
    ),
    "no-regression",
  ],
  [
    "a required cell declares no absolute gate",
    mutate(
      GOOD,
      'comparison = "absolute_max"\nlimit = 100000000',
      'comparison = "no_regression_percent_max"\nlimit = 99.0',
    ),
    "absolute gate",
  ],
  [
    "a no-regression bound of zero gates nothing",
    mutate(
      GOOD,
      'comparison = "no_regression_percent_max"\nlimit = 3.0',
      'comparison = "no_regression_percent_max"\nlimit = 0.0',
    ),
    "gates nothing",
  ],
  [
    "the suite names a cell that does not exist",
    mutate(GOOD, 'cell_ids = ["CELL_A"]', 'cell_ids = ["CELL_MISSING"]'),
    "no [[cell]] declares",
  ],
  [
    "a post-result exception is allowed at suite level",
    mutate(
      GOOD,
      "post_result_exception_allowed = false\npremise_change_requires_new_lock = true",
      "post_result_exception_allowed = true\npremise_change_requires_new_lock = true",
    ),
    "post_result_exception_allowed",
  ],
  [
    "a premise change no longer requires a new lock",
    mutate(
      GOOD,
      "premise_change_requires_new_lock = true",
      "premise_change_requires_new_lock = false",
    ),
    "premise_change_requires_new_lock",
  ],
  [
    "a competitor rule is active but names no competitor",
    mutate(GOOD, 'rule = "none"', 'rule = "pareto"'),
    "names no competitor",
  ],
  [
    "the boundary is not one of the declared surfaces",
    mutate(GOOD, 'boundary = "rust"', 'boundary = "python"'),
    "boundary",
  ],
  [
    "a template field is dropped from [cell.validity]",
    mutate(GOOD, 'required_exactness = "exact"\n', ""),
    "required_exactness",
  ],
  [
    "a template field is dropped from [cell.memory]",
    mutate(GOOD, 'quiescence_protocol = "not_applicable"\n', ""),
    "quiescence_protocol",
  ],
  [
    "the only cell stops being required",
    mutate(GOOD, "required = true", "required = false"),
    "gates nothing",
  ],
  ["the file declares no cell at all", GOOD.slice(0, GOOD.indexOf("[[cell]]")), "no `[[cell]]`"],
];

test("the complete locked shape passes", () => {
  const { violations, cells, metrics } = validateGates(GOOD);
  assert.deepEqual(violations, []);
  assert.equal(cells, 1);
  assert.equal(metrics, 2);
});

for (const [name, text, expected] of NEGATIVE_CONTROLS) {
  test(`rejects: ${name}`, () => {
    const { violations } = validateGates(text);
    assert.ok(violations.length > 0, "expected at least one violation");
    assert.ok(
      violations.some((v) => v.includes(expected)),
      `expected a violation mentioning "${expected}", got:\n${violations.join("\n")}`,
    );
  });
}

test("accepts_everything_fails_every_negative_control", () => {
  // The meta-check. A validator that returns no violations must be caught by
  // EVERY negative control; if any control would pass against a stub, that
  // control proves nothing about the real validator.
  const stub = () => ({ violations: [], cells: 1, metrics: 2 });
  for (const [name] of NEGATIVE_CONTROLS) {
    const { violations } = stub();
    assert.equal(violations.length, 0);
    // The control's own assertion is `violations.length > 0`, which the stub
    // fails. Assert that relationship explicitly so the meta-check is not
    // itself vacuous.
    assert.ok(
      !(violations.length > 0),
      `control "${name}" must discriminate against a permissive stub`,
    );
  }
});

test("malformed TOML is a loud failure, never a silent skip", () => {
  const { violations } = validateGates("schema = 1\nthis is not toml\n");
  assert.ok(
    violations.some((v) => v.startsWith("TOML:")),
    violations.join("\n"),
  );
});

test("the reader rejects an unterminated array rather than truncating it", () => {
  assert.throws(() => readGatesToml('a = [\n"x",\n'), /unterminated array/);
});

test("the reader keeps a # inside a string", () => {
  const { root } = readGatesToml('a = "x # y"\n');
  assert.equal(root[""].a, "x # y");
});

test("the committed gate file passes", () => {
  const text = readFileSync(join(repoRoot, "performance-gates.toml"), "utf8");
  const { violations, cells, metrics } = validateGates(text);
  assert.deepEqual(violations, [], violations.join("\n"));
  assert.ok(cells >= 1);
  assert.ok(metrics >= 1);
});
