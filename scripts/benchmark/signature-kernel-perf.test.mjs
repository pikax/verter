// Tests for the signature-kernel performance runner's verdict discipline: a workload
// gets a ratio only over matched work, and lock evidence only on the locked runner
// class of performance-gates.toml.
//
//   node --test scripts/benchmark/signature-kernel-perf.test.mjs

import assert from "node:assert/strict";
import test from "node:test";

import {
  LOCKED_RUNNER,
  canonicalCpu,
  canonicalLockedRunner,
  compareOutcomes,
  minInvocationsOf,
  renderMarkdown,
  runnerMismatches,
  rustcFacts,
  summarize,
  workloadIsMatched,
} from "./signature-kernel-perf.mjs";

const ORIGINAL = {
  "/sk/m0.ts#witnessChain0": "complete { v: number }",
  "/sk/m1.ts#witnessChain1": "complete { v: number }",
};
const LOCAL_EDIT = { "/sk/m0.ts#witnessChain0": "complete { v: string }" };

function document(outcomes, scale = 1) {
  const samples = (base) => Array.from({ length: 30 }, (_, i) => Math.round((base + i) * scale));
  return {
    corpus: { modules: 2, depth: 2, witnesses: 2 },
    census: { complete: 2, degraded: 0, refused: 0 },
    census_by_witness: {},
    outcomes,
    workloads: {
      cold_load: { samples_ns: samples(1000), alloc_bytes: 10, alloc_count: 1 },
      local_edit: { samples_ns: samples(500), alloc_bytes: 5, alloc_count: 1 },
    },
    concurrent_queries: {
      host_workers: 4,
      sweeps_per_sample: 10,
      witnesses: 2,
      by_callers: {
        1: {
          callers: 1,
          samples_ns: samples(2000),
          qps: [100, 110, 120],
          query_latency_ns: { p50: 1000, p95: 2000, p99: 3000, max: 4000, n: 60 },
        },
        2: {
          callers: 2,
          samples_ns: samples(3000),
          qps: [150, 160, 170],
          query_latency_ns: { p50: 1500, p95: 2500, p99: 3500, max: 4500, n: 120 },
        },
      },
    },
    scheduler_scaling: {
      callers: 1,
      modules: 2,
      witnesses: 2,
      by_workers: {
        1: { workers: 1, samples_ns: samples(4000), cpu_utilisation_total: 0.9 },
        2: { workers: 2, samples_ns: samples(2000), cpu_utilisation_total: null },
      },
    },
    full_check: {
      files: 2,
      witnesses: 2,
      by_workers: {
        1: {
          workers: 1,
          callers: 1,
          samples_ns: samples(4000),
          files_per_second: [500, 510],
          cpu_utilisation_total: 1,
        },
        2: {
          workers: 2,
          callers: 2,
          samples_ns: samples(2000),
          files_per_second: [990, 1000],
          cpu_utilisation_total: 0.8,
        },
      },
    },
    soak_live_bytes: [100, 100, 100, 100, 100, 100, 100, 100],
  };
}

function runs(baselineOutcomes, candidateOutcomes) {
  const arm = (outcomes, scale) =>
    Array.from({ length: 4 }, () => ({ document: document(outcomes(), scale), wall_ms: 1 }));
  return {
    baseline: arm(baselineOutcomes, 1),
    candidate: arm(candidateOutcomes, 0.9),
  };
}

const same = () => ({
  original: { ...ORIGINAL },
  local_edit: { ...LOCAL_EDIT },
  check: { ...ORIGINAL },
});

function summary(allRuns, mismatches = [], cancelRuns = []) {
  return summarize({
    opts: {
      quick: false,
      skipControl: false,
      invocations: 4,
      forwarded: {},
      settleSeconds: 0,
      cooldownSeconds: 0,
    },
    revs: { baseline: "b".repeat(40), candidate: "c".repeat(40) },
    harnessDigest: "d".repeat(64),
    runs: allRuns,
    rawDigests: [],
    controlStart: 10,
    controlEnd: 10,
    controlDrift: 0,
    idle: {
      satisfied: true,
      at_start: { idle: true, load_average_1m: null, waited_seconds: 0, foreign_processes: [] },
      foreign_processes_during_session: [],
      thermal: {},
    },
    sessionVoid: false,
    cancelProbe: cancelRuns.length > 0 ? { sha256: "e".repeat(64) } : null,
    cancelRuns,
    runnerCheck: { class: "test-class", expected: {}, observed: {}, mismatches },
  });
}

test("identical outcomes match every workload and report a ratio", () => {
  const s = summary(runs(same, same));
  for (const name of ["cold_load", "local_edit"]) {
    assert.equal(s.workloads[name].matched, true, name);
    assert.equal(typeof s.workloads[name].median_ratio, "number", name);
    assert.notEqual(s.workloads[name].verdict, "not comparable — the arms answer differently");
  }
  assert.equal(s.lock_evidence, true);
});

test("a differing original witness unmatches only the workloads that query it", () => {
  const candidate = () => ({
    original: { ...ORIGINAL, "/sk/m1.ts#witnessChain1": "degraded(Gap) { v: number }" },
    local_edit: { ...LOCAL_EDIT },
  });
  const s = summary(runs(same, candidate));
  assert.equal(s.workloads.cold_load.matched, false);
  assert.equal(s.workloads.cold_load.median_ratio, null);
  assert.equal(s.workloads.cold_load.ratio_ci, null);
  assert.equal(s.workloads.cold_load.verdict, "not comparable — the arms answer differently");
  // The local edit re-queries module 0 only, whose outcomes agree in both states.
  assert.equal(s.workloads.local_edit.matched, true);
  assert.equal(s.lock_evidence, false);
  assert.ok(s.lock_evidence_refusals.some((r) => r.includes("unmatched work in cold_load")));
});

test("a differing edited state unmatches its edit workload", () => {
  const candidate = () => ({
    original: { ...ORIGINAL },
    local_edit: { "/sk/m0.ts#witnessChain0": "complete { v: number }" },
  });
  const s = summary(runs(same, candidate));
  assert.equal(s.workloads.cold_load.matched, true);
  assert.equal(s.workloads.local_edit.matched, false);
});

test("an arm whose invocations disagree matches nothing", () => {
  // The second invocation of the arm answers one witness differently.
  const flaky = () => {
    let call = 0;
    return () => {
      call += 1;
      return call === 2
        ? {
            original: { ...ORIGINAL, "/sk/m0.ts#witnessChain0": "refused(Budget)" },
            local_edit: {},
          }
        : same();
    };
  };
  const outcomes = compareOutcomes(runs(flaky(), same));
  assert.deepEqual(outcomes.nondeterministic, ["baseline"]);
  assert.equal(workloadIsMatched(outcomes, "cold_load"), false);
  const s = summary(runs(flaky(), same));
  assert.ok(s.lock_evidence_refusals.some((r) => r.startsWith("outcomes differ between")));
});

test("a runner mismatch refuses lock evidence even when the control passes", () => {
  const s = summary(runs(same, same), ["os is Windows_NT, locked Darwin 25.6.0 arm64"]);
  assert.equal(s.lock_evidence, false);
  assert.ok(s.lock_evidence_refusals.some((r) => r.startsWith("not the locked runner class")));
});

test("the markdown renders an unmatched workload without a ratio", () => {
  const candidate = () => ({
    original: { ...ORIGINAL, "/sk/m1.ts#witnessChain1": "complete { v: string }" },
    local_edit: { ...LOCAL_EDIT },
  });
  const s = summary(runs(same, candidate));
  s.machine = {
    cpu_model: "x",
    logical_cpus: 1,
    memory_bytes: 2 ** 30,
    platform: "p",
    os_release: "r",
    node: "n",
  };
  const markdown = renderMarkdown(s);
  assert.match(markdown, /\| cold_load \|[^\n]*\| n\/a \| n\/a \| not comparable/);
  assert.match(markdown, /## Matched work/);
  assert.match(markdown, /`original` `\/sk\/m1\.ts#witnessChain1`/);
});

const MACHINE = {
  cpu_model: "x",
  logical_cpus: 1,
  memory_bytes: 2 ** 30,
  platform: "p",
  os_release: "r",
  node: "n",
};

test("every scaling point is compared on matched work and rendered in its own table", () => {
  const s = summary(runs(same, same));
  const concurrent = s.scaling.concurrent_queries;
  assert.equal(concurrent.host_workers, 4);
  assert.equal(concurrent.matched, true);
  assert.deepEqual(Object.keys(concurrent.points), ["1", "2"]);
  assert.equal(concurrent.points["2"].baseline.qps, 160);
  assert.deepEqual(concurrent.points["1"].candidate.query_latency_ns, {
    p50: 1000,
    p95: 2000,
    p99: 3000,
  });
  assert.equal(typeof concurrent.points["1"].median_ratio, "number");
  const scheduler = s.scaling.scheduler_scaling.points;
  assert.equal(scheduler["1"].baseline.speedup_vs_1, 1);
  assert.ok(scheduler["2"].baseline.speedup_vs_1 > 1.5);
  assert.equal(scheduler["1"].baseline.cpu_utilisation, 0.9);
  // One invocation without a CPU clock leaves the utilisation unknown, not zero.
  assert.equal(scheduler["2"].baseline.cpu_utilisation, null);
  assert.equal(s.scaling.full_check.points["2"].candidate.files_per_second, 995);
  assert.equal(s.lock_evidence, true);

  s.machine = MACHINE;
  const markdown = renderMarkdown(s);
  const has = (text) => assert.ok(markdown.includes(text), text);
  has("## Concurrent query scalability (one warm host, 4 host workers, N callers)");
  has("| 2 | 160 | 160 | 1.5 / 2.5 / 3.5 | 1.5 / 2.5 / 3.5 |");
  has("## Internal scheduler scalability (one caller, N host workers)");
  has("| 1.00 | 1.00 | 0.90 | 0.90 |");
  has("| n/a | n/a |");
  has("## Full-check throughput (N host workers, N callers)");
  has("| 2 | 995.0 | 995.0 |");
  assert.ok(!markdown.includes("## Throughput"));
});

test("a differing check-corpus witness unmatches only the scaling benchmarks that run it", () => {
  const candidate = () => ({
    ...same(),
    check: { ...ORIGINAL, "/sk/m1.ts#witnessChain1": "complete { v: string }" },
  });
  const s = summary(runs(same, candidate));
  assert.equal(s.scaling.concurrent_queries.matched, true);
  assert.equal(s.scaling.scheduler_scaling.matched, false);
  assert.equal(s.scaling.full_check.matched, false);
  assert.equal(s.scaling.full_check.points["1"].median_ratio, null);
  assert.equal(s.workloads.cold_load.matched, true);
  assert.ok(
    s.lock_evidence_refusals.includes("unmatched work in scheduler_scaling, full_check"),
    s.lock_evidence_refusals.join("; "),
  );
});

test("the cancellation summary reports every injection point beside the aggregate", () => {
  const probe = (offset) => ({
    corpus: { modules: 2, depth: 2 },
    fractions: [0.1, 0.9],
    rounds: 2,
    cold_request_median_ns: 1000,
    completed_before_cancel: 1,
    workloads: {
      cold_request: { samples_ns: [1000, 1000] },
      cancel_stop: { samples_ns: [10, 20, 90] },
      restart: { samples_ns: [900, 950, 100, 150] },
    },
    by_fraction: [
      {
        fraction: 0.1,
        delay_ns: 100,
        completed_before_cancel: 0,
        landed_ns: { samples_ns: [100 + offset, 100 + offset] },
        cancel_stop: { samples_ns: [10, 20] },
        restart: { samples_ns: [900, 950] },
      },
      {
        fraction: 0.9,
        delay_ns: 900,
        completed_before_cancel: 1,
        landed_ns: { samples_ns: [900, 900] },
        cancel_stop: { samples_ns: [90] },
        restart: { samples_ns: [100, 150] },
      },
    ],
  });
  const s = summary(runs(same, same), [], [probe(0), probe(0)]);
  const [early, late] = s.cancellation.by_fraction;
  assert.equal(early.fraction, 0.1);
  assert.equal(early.cancel_stop.n, 4);
  assert.equal(early.cancel_stop.p50, 15);
  assert.equal(early.landed_fraction.p50, 0.1);
  assert.equal(early.completed_before_cancel, 0);
  assert.equal(late.cancel_stop.n, 2);
  assert.equal(late.completed_before_cancel, 2);
  assert.equal(late.restart.p50, 125);
  assert.equal(s.cancellation.distributions.cancel_stop.n, 6);

  s.machine = MACHINE;
  const markdown = renderMarkdown(s);
  const has = (text) => assert.ok(markdown.includes(text), text);
  has("Per injection point:");
  has("| 10% | 10.0% | 0.000015 / 0.00002 / 0.00002 | 4 | 0 | 0.000925 / 0.00095 / 0.00095 |");
  has("| 90% | 90.0% | 0.00009 / 0.00009 / 0.00009 | 2 | 2 |");
});

const LOCKED = canonicalLockedRunner(LOCKED_RUNNER);

/** What `rustc -vV` prints for the locked toolchain (full commit hash). */
const LOCKED_RUSTC_VV = `rustc ${LOCKED_RUNNER.rust_toolchain}
binary: rustc
commit-hash: ${LOCKED.rust.hash}68e0e26f0bb7960be334d5b520ea452
commit-date: ${LOCKED.rust.date}
host: aarch64-apple-darwin
release: ${LOCKED.rust.release}
LLVM version: 22.1.6
`;

/** The locked runner's facts as `checkLockedRunner` observes them on that machine. */
const LOCKED_FACTS = {
  os: { ...LOCKED.os },
  cpu: LOCKED.cpu,
  logical_cpus: LOCKED.logical_cpus,
  memory_bytes: LOCKED.memory_bytes,
  node: LOCKED.node.join("."),
  rust: { baseline: rustcFacts(LOCKED_RUSTC_VV), candidate: rustcFacts(LOCKED_RUSTC_VV) },
  platform: "darwin",
  power_source: "AC Power",
  low_power_mode: "0",
};

test("the locked runner class is read from performance-gates.toml", () => {
  assert.equal(LOCKED_RUNNER.cpu, "Apple M3");
  assert.equal(LOCKED_RUNNER.class, "apple-silicon-laptop-8core-24gib");
  assert.deepEqual(runnerMismatches(LOCKED, LOCKED_FACTS), []);
});

test("formatting differences alone are never a lock mismatch", () => {
  // A CPU model with incidental whitespace, the full commit hash `rustc -vV` reports
  // against the lock's short one, and a node version with its `v` all match.
  assert.equal(
    canonicalCpu(`  ${LOCKED_RUNNER.cpu.replace(" ", "   ")}
`),
    LOCKED.cpu,
  );
  assert.equal(rustcFacts(LOCKED_RUSTC_VV).hash.length, 40);
  assert.deepEqual(
    runnerMismatches(LOCKED, { ...LOCKED_FACTS, node: `v${LOCKED.node.join(".")}` }),
    [],
  );
});

test("every locked runner dimension refuses on a genuine mismatch", () => {
  const cases = [
    [{ os: { ...LOCKED.os, release: "26.0.0" } }, "os release"],
    [{ os: { ...LOCKED.os, system: "Linux" } }, "os system"],
    [{ os: { ...LOCKED.os, machine: "x86_64" } }, "os machine"],
    [{ cpu: "Apple M4" }, "cpu "],
    [{ logical_cpus: 10 }, "logical_cpus "],
    [{ memory_bytes: 17179869184 }, "memory_bytes "],
    [{ node: "24.0.0" }, "node runtime"],
    [
      {
        rust: {
          baseline: { ...LOCKED_FACTS.rust.baseline, release: "1.98.0" },
          candidate: LOCKED_FACTS.rust.candidate,
        },
      },
      "baseline toolchain",
    ],
    [
      {
        rust: {
          baseline: LOCKED_FACTS.rust.baseline,
          candidate: { ...LOCKED_FACTS.rust.candidate, hash: "0000000000" },
        },
      },
      "candidate toolchain",
    ],
    [{ power_source: "Battery Power" }, "power source"],
    [{ low_power_mode: "1" }, "low-power mode"],
    [
      { platform: "linux", power_source: undefined, low_power_mode: undefined },
      "power state cannot be verified",
    ],
  ];
  for (const [change, expected] of cases) {
    const mismatches = runnerMismatches(LOCKED, { ...LOCKED_FACTS, ...change });
    assert.equal(mismatches.length, 1, `${JSON.stringify(change)}: ${mismatches.join("; ")}`);
    assert.ok(mismatches[0].startsWith(expected), mismatches[0]);
  }
});

test("a locked runner value that does not parse fails loudly", () => {
  assert.throws(() => canonicalLockedRunner({ ...LOCKED_RUNNER, os: "Darwin" }));
  assert.throws(() => canonicalLockedRunner({ ...LOCKED_RUNNER, rust_toolchain: "1.97.1" }));
  assert.throws(() => canonicalLockedRunner({ ...LOCKED_RUNNER, node_runtime: "twenty" }));
});

test("the invocation minimum is read from the interleave policy", () => {
  assert.equal(minInvocationsOf("A,B,B,A order, at least four invocations per arm, idle"), 4);
  assert.throws(() => minInvocationsOf("alternate the arms"));
});
