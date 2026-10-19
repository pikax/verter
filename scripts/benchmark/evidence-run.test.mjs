// Self-tests of the evidence-run layer: manifest validation on planted
// manifests, the dry run of every shipped manifest, and the summary of a
// real run driven through a stand-in harness that writes synthetic results
// (no benchmark executes, and no test reads a timing).
//
//   node --test scripts/benchmark/evidence-run.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, describe, test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  BENCH_TAG,
  buildSummary,
  defaultValidator,
  loadManifest,
  main,
  MANIFEST_DIR,
  readWorker,
  SUMMARY_FILE,
  validateManifest,
} from "./evidence-run.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(HERE, "evidence-run-fixtures");
const RUNNER = join(HERE, "evidence-run.mjs");

const scratch = mkdtempSync(join(tmpdir(), "evidence-run-test-"));
after(() => rmSync(scratch, { recursive: true, force: true }));
let scratchCount = 0;
const freshDir = () => {
  const dir = join(scratch, String(scratchCount++));
  mkdirSync(dir, { recursive: true });
  return dir;
};
const readSummary = (dir) => JSON.parse(readFileSync(join(dir, SUMMARY_FILE), "utf8"));
const quiet = async (fn) => {
  const log = console.log;
  console.log = () => {};
  try {
    return await fn();
  } finally {
    console.log = log;
  }
};

describe("manifest validation", () => {
  test("every shipped manifest validates", () => {
    const files = readdirSync(MANIFEST_DIR).filter((f) => f.endsWith(".json"));
    assert.ok(files.includes("skr-perf0-structural.json"));
    for (const f of files) assert.doesNotThrow(() => loadManifest(f.replace(/\.json$/, "")), f);
  });

  test("the planted valid manifest resolves to the harness invocation", () => {
    const { run } = loadManifest("valid-minimal", FIXTURES);
    assert.deepEqual(run.scenarios, ["baseline-empty"]);
    assert.deepEqual(run.harnessArgs, [
      "--tier",
      "quick",
      "--only",
      "baseline-empty",
      "--settings",
      "strict",
      "--arms",
      "verter,tsc-api,verter-counted",
      "--repeat",
      "2",
      "--warmup",
      "0",
      "--warm-repeats",
      "1",
    ]);
  });

  const rejected = [
    ["unknown-field", /unknown field budgetMs/],
    ["missing-field", /missing required field requiredCells/],
    ["unknown-arm", /unknown arm tsc-watch/],
    ["cell-outside-run", /scenario alias-chain-50 is not one of the run's scenarios/],
    ["name-mismatch", /name other-name differs from the file name name-mismatch\.json/],
    ["invalid-json", /is not valid JSON/],
    ["cli-warm", /arm tsc-cli has no warm mode/],
    ["wrong-threads", /arm tsc-cli-1 runs at threads 1, not default/],
    ["unknown-metric", /arm verter has no cold metric checkMs/],
    ["bad-noise", /noise reference .*#no-such-section is not a section/],
    ["settings-subset", /the harness runs either strict alone or all/],
    ["unknown-scenario", /scenario no-such-scenario selects no scenario/],
    ["duplicate-cell", /duplicate cell/],
    ["bad-require-all-matched", /requireAllMatched must be true or false/],
  ];
  for (const [name, message] of rejected)
    test(`rejects ${name}`, () => assert.throws(() => loadManifest(name, FIXTURES), message));

  test("rejects a name two manifests declare", () => {
    const dir = freshDir();
    const manifest = JSON.parse(readFileSync(join(FIXTURES, "valid-minimal.json"), "utf8"));
    writeFileSync(join(dir, "dup-run.json"), JSON.stringify({ ...manifest, name: "dup-run" }));
    writeFileSync(join(dir, "other.json"), JSON.stringify({ ...manifest, name: "Dup-Run" }));
    assert.throws(() => loadManifest("dup-run", dir), /declared by dup-run\.json, other\.json/);
  });

  test("a rejected manifest exits 2 through the CLI", () => {
    const r = spawnSync(
      process.execPath,
      [
        RUNNER,
        "--run",
        "unknown-arm",
        "--dry-run",
        "--manifest-dir",
        FIXTURES,
        "--out",
        freshDir(),
      ],
      { encoding: "utf8" },
    );
    assert.equal(r.status, 2);
    assert.match(r.stderr, /unknown arm tsc-watch/);
  });
});

describe("skr-scale-answers", () => {
  test("demands the larger answers exactly and keeps the timing cells", () => {
    const { manifest, run } = loadManifest("skr-scale-answers");
    assert.equal(run.requireAllMatched, true);
    for (const id of ["infer-pattern-repeat-6000", "route-tree-names-2000"]) {
      assert.ok(run.scenarios.includes(id), id);
      for (const arm of ["verter", "tsc-cli"])
        assert.ok(
          manifest.requiredCells.some((c) => c.scenario === id && c.arm === arm),
          `${id} ${arm}`,
        );
    }
    for (const id of [
      "route-tree-names-300",
      "route-tree-paths-300",
      "infer-pattern-repeat-1000",
      "reference-infer-aliases-1000",
    ])
      assert.ok(
        manifest.requiredCells.some(
          (c) => c.scenario === id && c.arm === "verter" && c.metrics.includes("coldMs"),
        ),
        id,
      );
  });
});

describe("scenario selection", () => {
  test("overlapping prefixes select each scenario once", () => {
    const base = JSON.parse(readFileSync(join(FIXTURES, "valid-minimal.json"), "utf8"));
    const manifest = { ...base, scenarios: [base.scenarios[0], base.scenarios[0]] };
    const { run } = validateManifest(manifest, join(MANIFEST_DIR, base.name + ".json"));
    assert.deepEqual(run.scenarios, [...new Set(run.scenarios)]);
  });
});

describe("dry run", () => {
  test("skr-perf0-structural validates and emits a summary skeleton", () => {
    const out = freshDir();
    const r = spawnSync(
      process.execPath,
      [RUNNER, "--run", "skr-perf0-structural", "--dry-run", "--out", out],
      { encoding: "utf8" },
    );
    assert.equal(r.status, 0, r.stderr);
    const summary = readSummary(out);
    const bytes = readFileSync(join(MANIFEST_DIR, "skr-perf0-structural.json"));
    const manifest = JSON.parse(bytes.toString("utf8"));
    assert.equal(summary.dryRun, true);
    assert.equal(summary.run.name, "skr-perf0-structural");
    assert.equal(summary.run.manifestSha256, createHash("sha256").update(bytes).digest("hex"));
    assert.equal(summary.worker.benchM3, false);
    assert.equal(summary.validation.verdict, "not run");
    assert.equal(summary.cells.length, manifest.requiredCells.length);
    manifest.requiredCells.forEach((cell, i) => {
      const got = summary.cells[i];
      assert.equal(got.status, "not run");
      assert.deepEqual(Object.keys(got.metrics), cell.metrics);
      for (const m of Object.values(got.metrics)) assert.equal(m.status, "not measured");
    });
    for (const p of Object.values(summary.prerequisites))
      assert.ok(["met", "unavailable"].includes(p.status));
  });
});

/** Synthetic harness results for the valid-minimal manifest. */
function syntheticResults({ arms = ["verter", "tsc-api", "verter-counted"], dropArm = null } = {}) {
  const stat = (v) => ({ n: 2, min: v, median: v, max: v });
  const armSummaries = {
    verter: {
      class: "matched",
      repetitionsDiffer: false,
      answerDigest: { sha256: "a".repeat(64) },
      metrics: { firstTypeMs: stat(2), peakBytes: stat(4096) },
    },
    "tsc-api": { class: "reference", metrics: { firstTypeMs: stat(1) } },
    "verter-counted": {
      class: "matched",
      metrics: {},
      coldAllocations: stat(1234),
      coldAllocatedBytes: stat(99999),
    },
  };
  if (dropArm) delete armSummaries[dropArm];
  return {
    schema: 1,
    meta: {
      options: { tier: "quick", arms, settings: "strict", repeat: 2, warmup: 0, warmRepeats: 1 },
      scenarios: { "baseline-empty/strict": { id: "baseline-empty", setting: "strict" } },
      binaries: { supervisor: { origin: "workspace", sha256: "b".repeat(64) } },
      build: { cargo: "cargo 1.97.0" },
      typescript: { versionText: "Version 7.0.2", platformPackage: "@typescript/typescript-test" },
    },
    invocations: [{ arm: "verter", supervisor: { backend: "test-backend", containment: "hard" } }],
    summary: {
      cells: [{ key: "baseline-empty/strict", arms: armSummaries }],
      verterClassCounts: { matched: 1 },
    },
  };
}

/** Run valid-minimal through a stand-in harness; returns the exit code, summary and harness args. */
async function fakeRun({
  results,
  harnessStatus = 0,
  validatorProblems = [],
  worker = null,
  run = "valid-minimal",
}) {
  const out = freshDir();
  let harnessArgs = null;
  let validatorOptions = null;
  const argv = ["--run", run, "--manifest-dir", FIXTURES, "--out", out];
  if (worker) {
    const path = join(out, "worker.json");
    writeFileSync(path, JSON.stringify(worker));
    argv.push("--worker", path);
  }
  const code = await quiet(() =>
    main(argv, {
      runHarness: (args) => {
        harnessArgs = args;
        const dir = args[args.indexOf("--out") + 1];
        if (results) {
          mkdirSync(dir, { recursive: true });
          writeFileSync(join(dir, "results.json"), JSON.stringify(results));
        }
        return { status: harnessStatus };
      },
      validate: (_path, options) => {
        validatorOptions = options;
        return validatorProblems;
      },
    }),
  );
  return { code, summary: readSummary(out), harnessArgs, validatorOptions };
}

describe("real run", () => {
  test("off bench-m3, time and memory are not measured; work counts and answers are", async () => {
    const { code, summary, harnessArgs } = await fakeRun({ results: syntheticResults() });
    assert.equal(code, 0);
    assert.equal(summary.dryRun, false);
    assert.equal(summary.validation.verdict, "passed");
    assert.ok(harnessArgs.includes("--out"));
    assert.deepEqual(harnessArgs.slice(0, 8), [
      "--tier",
      "quick",
      "--only",
      "baseline-empty",
      "--settings",
      "strict",
      "--arms",
      "verter,tsc-api,verter-counted",
    ]);
    const [verter, counted, tsc] = summary.cells;
    assert.equal(verter.answerClass, "matched");
    assert.equal(tsc.answerClass, "reference");
    for (const m of [
      verter.metrics.firstTypeMs,
      verter.metrics.peakBytes,
      tsc.metrics.firstTypeMs,
    ]) {
      assert.equal(m.status, "not measured");
      assert.equal(m.values, undefined);
    }
    assert.equal(counted.metrics.coldAllocations.status, "measured");
    assert.equal(counted.metrics.coldAllocations.values.median, 1234);
    assert.equal(summary.prerequisites.containment.status, "met");
  });

  test("on bench-m3, time and memory are measured", async () => {
    const { code, summary } = await fakeRun({
      results: syntheticResults(),
      worker: { id: "m3", tags: [BENCH_TAG] },
    });
    assert.equal(code, 0);
    assert.equal(summary.worker.benchM3, true);
    assert.equal(summary.cells[0].metrics.firstTypeMs.status, "measured");
    assert.equal(summary.cells[0].metrics.peakBytes.values.median, 4096);
  });

  test("a metric the run lacks is unavailable, never zero", async () => {
    const results = syntheticResults();
    results.summary.cells[0].arms.verter.metrics.peakBytes = null;
    const { summary } = await fakeRun({ results, worker: { id: "m3", tags: [BENCH_TAG] } });
    assert.equal(summary.cells[0].metrics.peakBytes.status, "unavailable");
    assert.ok(summary.cells[0].metrics.peakBytes.reason);
  });

  const failing = [
    [
      "a missing required cell",
      { results: syntheticResults({ dropArm: "verter-counted" }) },
      /required cell baseline-empty\/strict\|verter-counted\|cold\|threads=default is missing/,
    ],
    [
      "a failed harness validation",
      { results: syntheticResults(), validatorProblems: ["an invalid answer counted as a win"] },
      /an invalid answer counted as a win/,
    ],
    [
      "results from another invocation",
      { results: syntheticResults({ arms: ["verter", "tsc-api"] }) },
      /the run's arms .* are not the manifest's/,
    ],
    [
      "a harness that wrote no results",
      { results: null, harnessStatus: 2 },
      /exited 2 without results/,
    ],
  ];
  for (const [label, input, message] of failing)
    test(`fails on ${label}`, async () => {
      const { code, summary } = await fakeRun(input);
      assert.equal(code, 1);
      assert.equal(summary.validation.verdict, "failed");
      assert.ok(
        summary.validation.problems.some((p) => message.test(p)),
        summary.validation.problems.join("\n"),
      );
    });

  test("a harness without results leaves prerequisites unavailable", async () => {
    const { summary } = await fakeRun({ results: null, harnessStatus: 2 });
    for (const p of Object.values(summary.prerequisites)) assert.equal(p.status, "unavailable");
    assert.ok(summary.cells.every((c) => c.status === "missing"));
  });
});

/** Synthetic harness results for skr-augment-scaleb with the given retention-proof verdicts. */
function augmentResults(verdicts, { dropSession = null, classes = {} } = {}) {
  const stat = (v) => ({ n: 3, min: v, median: v, max: v });
  const verter = {
    class: "matched",
    metrics: { firstTypeMs: stat(20), coldMs: stat(25), peakBytes: stat(50_000_000) },
  };
  const ids = [
    "augment-global-growth",
    "augment-global-baseline",
    "augment-module-growth",
    "augment-module-baseline",
  ];
  const sessionCells = ids
    .filter((id) => id !== dropSession)
    .map((id) => {
      const cell = { key: id, arms: { verter: { class: classes[id] ?? "matched" } } };
      if (id.endsWith("-growth")) {
        const verdict = verdicts[id] ?? "passed";
        cell.retentionProof = {
          baseline: id.replace("-growth", "-baseline"),
          first: 500,
          verdict,
          pairs: verdict === "n/a" ? 0 : 9,
          figures: null,
          problems: verdict === "failed" ? ["snapshotLeases 3503 exceeds the baseline's 2003"] : [],
        };
      }
      return cell;
    });
  return {
    schema: 1,
    meta: {
      options: { tier: "standard", arms: ["verter"], settings: "strict" },
      scenarios: {
        "augment-global-2000/strict": { id: "augment-global-2000" },
        "augment-module-2000/strict": { id: "augment-module-2000" },
      },
      sessions: Object.fromEntries(ids.map((id) => [id, { id }])),
    },
    invocations: [],
    summary: {
      cells: ["augment-global-2000", "augment-module-2000"].map((id) => ({
        key: `${id}/strict`,
        arms: { verter },
      })),
      sessions: { cells: sessionCells },
    },
  };
}

describe("session retention proofs", () => {
  const loaded = loadManifest("skr-augment-scaleb");
  const summarize = (results) =>
    buildSummary(loaded, {
      dryRun: false,
      worker: readWorker(null),
      results,
      harness: { status: 0 },
      validatorProblems: [],
    });

  test("the manifest runs its scenarios and both growth sessions with their baselines", () => {
    assert.deepEqual(loaded.run.sessions, [
      "augment-global-growth",
      "augment-global-baseline",
      "augment-module-growth",
      "augment-module-baseline",
    ]);
    const only = loaded.run.harnessArgs[loaded.run.harnessArgs.indexOf("--only") + 1];
    assert.deepEqual(only.split(","), [...loaded.manifest.scenarios, ...loaded.run.sessions]);
  });

  test("passing proofs are read on any worker", () => {
    const summary = summarize(augmentResults({}));
    assert.equal(summary.validation.verdict, "passed", summary.validation.problems.join("\n"));
    const growth = summary.sessions.find((x) => x.session === "augment-module-growth");
    assert.equal(growth.retentionProof.verdict, "passed");
    assert.equal(growth.answerClass, "matched");
  });

  const failing = [
    [
      "a failed proof",
      augmentResults({ "augment-global-growth": "failed" }),
      /augment-global-growth: retained growth beyond the answers against augment-global-baseline: snapshotLeases/,
    ],
    [
      "a proof with nothing compared",
      augmentResults({ "augment-module-growth": "n/a" }),
      /augment-module-growth: the retention proof has no completed pair to compare/,
    ],
    [
      "a passing proof over a growth session that answered wrongly",
      augmentResults({}, { classes: { "augment-global-growth": "mismatch" } }),
      /augment-global-growth: the retention proof needs augment-global-growth to answer every demand, its class is mismatch/,
    ],
    [
      "a passing proof over a baseline that refused",
      augmentResults({}, { classes: { "augment-module-baseline": "refusal" } }),
      /augment-module-growth: the retention proof needs augment-module-baseline to answer every demand, its class is refusal/,
    ],
    [
      "a missing session",
      augmentResults({}, { dropSession: "augment-module-baseline" }),
      /session augment-module-baseline is missing/,
    ],
  ];
  for (const [label, results, message] of failing)
    test(`fails on ${label}`, () => {
      const summary = summarize(results);
      assert.equal(summary.validation.verdict, "failed");
      assert.ok(
        summary.validation.problems.some((p) => message.test(p)),
        summary.validation.problems.join("\n"),
      );
    });

  test("a growth session without its baseline is rejected", () => {
    const manifest = {
      ...loaded.manifest,
      sessions: ["augment-global-growth", "augment-module-growth", "augment-module-baseline"],
    };
    const { problems } = validateManifest(manifest, loaded.path);
    assert.ok(
      problems.some((p) =>
        /augment-global-growth is proved against augment-global-baseline, which sessions does not list/.test(
          p,
        ),
      ),
      problems.join("\n"),
    );
    const unknown = validateManifest(
      { ...loaded.manifest, sessions: ["no-such-session"] },
      loaded.path,
    );
    assert.ok(unknown.problems.some((p) => /session no-such-session is not a session/.test(p)));
  });
});

describe("answer policy", () => {
  test("a manifest without requireAllMatched validates answers as findings", async () => {
    const { code, summary, validatorOptions } = await fakeRun({ results: syntheticResults() });
    assert.equal(code, 0);
    assert.deepEqual(validatorOptions, { requireAllMatched: false });
    assert.equal(summary.run.requireAllMatched, false);
  });

  test("an opted-in manifest forwards the policy and fails on the validator's rejection", async () => {
    const { code, summary, validatorOptions } = await fakeRun({
      run: "valid-require-all-matched",
      results: syntheticResults(),
      validatorProblems: [
        "baseline-empty/strict|verter: Verter's answer is refusal (BudgetExceeded); --require-all-matched",
      ],
    });
    assert.deepEqual(validatorOptions, { requireAllMatched: true });
    assert.equal(summary.run.requireAllMatched, true);
    assert.equal(code, 1);
    assert.ok(summary.validation.problems.some((p) => /--require-all-matched/.test(p)));
  });

  test("the default validator passes the policy to validate.mjs as --require-all-matched", () => {
    // A stand-in validate.mjs that rejects exactly when it is told every
    // answer must match: the policy is forwarded, its FAIL lines read back.
    const dir = freshDir();
    const script = join(dir, "validate-stand-in.mjs");
    writeFileSync(
      script,
      'if (process.argv.includes("--require-all-matched")) {\n' +
        '  console.log("FAIL: synthetic: Verter\'s answer is partial (x); --require-all-matched");\n' +
        "  process.exit(1);\n" +
        "}\n" +
        'console.log("validation passed");\n',
    );
    const results = join(dir, "results.json");
    writeFileSync(results, "{}");
    assert.deepEqual(defaultValidator(results, { requireAllMatched: false, script }), []);
    assert.deepEqual(defaultValidator(results, { requireAllMatched: true, script }), [
      "synthetic: Verter's answer is partial (x); --require-all-matched",
    ]);
  });
});

describe("worker record", () => {
  test("no record is not bench-m3", () => assert.equal(readWorker(null).benchM3, false));
  test("a malformed record is rejected", () => {
    const path = join(freshDir(), "w.json");
    writeFileSync(path, JSON.stringify({ id: "x", tags: "bench-m3" }));
    assert.throws(() => readWorker(path), /list of string tags/);
  });
});
