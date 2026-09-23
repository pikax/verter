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
    throughput_qps: {},
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

const same = () => ({ original: { ...ORIGINAL }, local_edit: { ...LOCAL_EDIT } });

function summary(allRuns, mismatches = []) {
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
    cancelProbe: null,
    cancelRuns: [],
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
