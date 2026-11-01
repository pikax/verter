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
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, describe, test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  BENCH_TAG,
  buildSummary,
  defaultValidator,
  invokeSoakRunner,
  SOAK_BUILD,
  SOAK_METRICS,
  loadManifest,
  main,
  MANIFEST_DIR,
  readWorker,
  SUMMARY_FILE,
  validateManifest,
} from "./evidence-run.mjs";
import { loadFormatterLock, ROOT, validateFormatterLock } from "../../tests/formatter/verify.mjs";

import { buildFormatterSummary } from "../../tests/formatter/evidence.mjs";

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

describe("formatter contract", () => {
  test("churn cancellation reaches rendering and edit-revert shares two incremental allowances", () => {
    const actions = loadFormatterLock().performance.churn_action;
    for (const [i, action] of actions.entries()) {
      if (action.action === "cancellation") {
        const previous = actions[i - 1];
        assert.notEqual(action.source_revision, previous.source_revision);
      }
      if (action.action === "edit-revert") {
        assert.equal(action.ceiling_mode, "incremental");
        assert.equal(action.pair_start_ordinal, action.ordinal - 1);
        assert.equal(actions[i - 1].action, "incremental");
      }
    }
  });

  test("exact cells match the pinned local oracle and remain idempotent", () => {
    const lock = loadFormatterLock();
    const catalog = lock.compatibility.find((c) => c.languages.length === 10);
    const require = createRequire(import.meta.url);
    const oracle = require(
      join(
        ROOT,
        "node_modules",
        ".pnpm",
        `prettier@${catalog.prettier_release}`,
        "node_modules",
        "prettier",
      ),
    );
    assert.equal(oracle.version, catalog.prettier_release);
    const corpus = new Map(catalog.corpus.map((c) => [c.id, c]));
    for (const cell of catalog.cell.filter((c) => c.classification === "prettier-exact")) {
      const input = lock.artifacts[corpus.get(cell.corpus_id).path].text;
      const expected = lock.artifacts[cell.expected_outcome].json;
      const parser =
        { javascript: "babel", jsx: "babel", tsx: "typescript" }[cell.language] ?? cell.language;
      const options = { ...expected.normalized_options, parser };
      const geometry =
        cell.request_mode === "cursor"
          ? {
              cursorOffset: Buffer.from(input).subarray(0, expected.request.cursor).toString()
                .length,
            }
          : cell.request_mode === "range"
            ? { rangeStart: 0, rangeEnd: input.length }
            : {};
      const result = oracle.formatWithCursor(input, { ...options, ...geometry });
      assert.equal(result.formatted, lock.artifacts[expected.output].text, cell.id);
      assert.equal(oracle.format(result.formatted, options), result.formatted, cell.id);
      if (cell.request_mode === "cursor")
        assert.equal(
          Buffer.byteLength(result.formatted.slice(0, result.cursorOffset)),
          expected.formatted_cursor,
          cell.id,
        );
    }
  });

  test("the complete hermetic lock validates and rejects missing dimensions", () => {
    const lock = loadFormatterLock();
    assert.deepEqual(validateFormatterLock(lock), []);
    const mutations = [
      ["missing cell", (l) => l.compatibility[0].cell.pop(), /missing cell/],
      [
        "duplicate cell",
        (l) => l.compatibility[0].cell.push(l.compatibility[0].cell[0]),
        /duplicate cell/,
      ],
      ["option vocabulary", (l) => l.compatibility[0].option.pop(), /option vocabulary/],
      ["corpus digest", (l) => (l.compatibility[0].corpus[0].sha256 = "0".repeat(64)), /digest/],
      ["outside path", (l) => (l.compatibility[0].corpus[0].path = "../outside.vue"), /path/],
      ["churn count", (l) => l.performance.churn_action.pop(), /10,000/],
      ["churn ordinal", (l) => (l.performance.churn_action[1].ordinal = 1), /ordinal/],
      [
        "cancellation warm identity",
        (l) =>
          (l.performance.churn_action[7].source_revision =
            l.performance.churn_action[6].source_revision),
        /cancellation/,
      ],
      [
        "edit-revert pair allowance",
        (l) => (l.performance.churn_action[3].ceiling_mode = "edit-revert"),
        /pair allowance/,
      ],
      ["calibration", (l) => delete l.performance.calibration.operation_schedule, /calibration/],
      [
        "empty operations",
        (l) => (l.performance.calibration.operation_schedule = []),
        /calibration/,
      ],
      ["empty sampling", (l) => (l.performance.calibration.sample_order = []), /calibration/],
      ["invalid seed", (l) => (l.performance.calibration.seed = -1), /calibration/],
      [
        "query source",
        (l) => {
          l.artifacts[
            "fixtures/formatter/performance/range-cursor.json"
          ].json.queries[0].source_path = "fixtures/formatter/performance/query-inserted-input.txt";
        },
        /query basis/,
      ],
      ["numeric mode", (l) => l.performance.mode.pop(), /mode/],
      ["route owner", (l) => l.inventory.routes.pop(), /route inventory/],
      ["duplicate owner", (l) => l.inventory.routes.push(l.inventory.routes[0]), /duplicate route/],
    ];
    for (const [name, mutate, expected] of mutations) {
      const copy = structuredClone(lock);
      mutate(copy);
      assert.match(validateFormatterLock(copy).join("\n"), expected, name);
    }
  });

  test("evidence requires the candidate and a known corpus, with every mode unmeasured", async () => {
    const lock = loadFormatterLock();
    const loaded = loadManifest("formatter-performance");
    for (const corpus of lock.performance.fixture) {
      const summary = buildFormatterSummary(loaded, {
        candidate: "review-candidate",
        corpus: corpus.id,
        dryRun: true,
        worker: readWorker(),
      });
      assert.equal(summary.run.candidate, "review-candidate");
      assert.equal(summary.run.corpus, corpus.id);
      assert.equal(summary.validation.verdict, "not run");
      assert.equal(summary.cells.length, lock.performance.mode.length);
      assert.ok(summary.cells.every((c) => c.status === "not run"));
    }
    const dir = freshDir();
    assert.equal(
      await quiet(() =>
        main([
          "--run",
          "formatter-performance",
          "--candidate",
          "review-candidate",
          "--corpus",
          "medium",
          "--dry-run",
          "--out",
          dir,
        ]),
      ),
      0,
    );
    assert.equal(readSummary(dir).run.corpus, "medium");
    await assert.rejects(
      main(["--run", "formatter-performance", "--corpus", "medium", "--dry-run"]),
      /candidate/,
    );
    await assert.rejects(
      main(["--run", "skr-perf0-structural", "--candidate", "review-candidate", "--dry-run"]),
      /formatter-only/,
    );
    await assert.rejects(
      main([
        "--run",
        "formatter-performance",
        "--candidate",
        "review-candidate",
        "--corpus",
        "unknown",
        "--dry-run",
      ]),
      /corpus/,
    );
  });
});

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
      typescript: {
        versionText: "Version 7.1.0-beta",
        platformPackage: "@typescript/typescript-test",
      },
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

describe("soak manifests", () => {
  const registered = () =>
    JSON.parse(readFileSync(join(MANIFEST_DIR, "rev11-l1-soak.json"), "utf8"));
  const problemsOf = (edit) => {
    const manifest = registered();
    edit(manifest);
    return validateManifest(manifest, "rev11-l1-soak.json").problems;
  };

  test("the registered soak manifest resolves to both modes of the full workload", () => {
    const { run } = loadManifest("rev11-l1-soak");
    assert.deepEqual(run.soak, { modes: ["normal", "pressure"], tranches: 100 });
  });

  test("surge bounds are complete finite registrations, independent of requested work", () => {
    assert.deepEqual(
      problemsOf(() => {}),
      [],
    );
    for (const edit of [
      (m) => delete m.soak.surgeLimits,
      (m) => delete m.soak.surgeLimits.full,
      (m) => (m.soak.surgeLimits.bounded = null),
      (m) => (m.soak.surgeLimits.full.extra = 1),
      ...Object.keys(registered().soak.surgeLimits.full).flatMap((field) => [
        (m) => delete m.soak.surgeLimits.full[field],
        (m) => (m.soak.surgeLimits.full[field] = 0),
        (m) => (m.soak.surgeLimits.full[field] = Number.MAX_SAFE_INTEGER + 1),
      ]),
      (m) => (m.soak.surgeLimits.full.modules += 1),
      (m) => (m.soak.surgeLimits.full.concurrency += 1),
      (m) => (m.soak.surgeLimits.full.retainedBytesMax = 2 ** 30 + 1),
      (m) => (m.soak.surgeLimits.bounded.retainedBytesMax = 2 ** 26 + 1),
    ]) {
      const problems = problemsOf(edit);
      assert.ok(
        problems.some((p) => /soak.surgeLimits/.test(p)),
        problems.join("\n"),
      );
    }
  });

  test("a time or memory metric outside a bench-m3-only cell is rejected", () => {
    const problems = problemsOf((m) => m.requiredCells[0].metrics.push("peakRssBytes"));
    assert.ok(
      problems.some((p) => /mark the cell benchM3Only/.test(p)),
      problems.join("\n"),
    );
  });

  test("a workload below the distinct-query floor or without a digest is rejected", () => {
    assert.ok(
      problemsOf((m) => (m.soak.workloads.normal.distinctQueries = 2999)).some((p) =>
        /fewer than 3000 distinct queries/.test(p),
      ),
    );
    assert.ok(
      problemsOf((m) => (m.soak.workloads.pressure.digest = "nope")).some((p) =>
        /soak.workloads.pressure needs a sha-256 digest/.test(p),
      ),
    );
  });

  test("the full run binds only to bench-m3 and samples at least every 100 actions", () => {
    assert.ok(problemsOf((m) => (m.soak.binding.full = "any")).some((p) => /soak.binding/.test(p)));
    assert.ok(
      problemsOf((m) => (m.soak.sampleEvery = 101)).some((p) => /at least every 100/.test(p)),
    );
  });

  test("the run declares exactly the soak-support build of the runner", () => {
    assert.deepEqual(registered().soak.build, SOAK_BUILD);
    for (const edit of [
      (m) => delete m.soak.build,
      (m) => (m.soak.build.features = []),
      (m) => m.soak.build.features.push("semantic-observe"),
      (m) => (m.soak.build.profile = "dev"),
    ])
      assert.ok(
        problemsOf(edit).some((p) => /soak.build must declare the support-feature build/.test(p)),
      );
  });

  test("surge figures are the surges' own deltas and window peaks, not lifetime counters", () => {
    const result = {
      summary: {
        records: [
          { class: "cold-demand", refusalsPressure: 50 },
          {
            class: "pressure-surge",
            refusalsPressure: 2,
            refusalsOversized: 1,
            pressure: { refusalsActive: 0, active: { attributedPeak: 10 } },
          },
          {
            class: "pressure-surge",
            refusalsPressure: 3,
            refusalsOversized: 0,
            pressure: { refusalsActive: 0, active: { attributedPeak: null } },
          },
        ],
        samples: [{ at: "closed", counts: { refusalsPressure: 99, peakTotalBytes: 1e12 } }],
      },
    };
    assert.equal(SOAK_METRICS.surgeRefusalsPressure.from(result), 5);
    assert.equal(SOAK_METRICS.surgeRefusalsOversized.from(result), 1);
    assert.equal(SOAK_METRICS.surgeRefusalsActive.from(result), 0);
    assert.equal(SOAK_METRICS.surgeActivePeakBytes.from(result), 10);
    assert.equal(SOAK_METRICS.surgeActivePeakBytes.from({ summary: { records: [] } }), null);
  });

  test("an unknown kind is rejected", () => {
    assert.ok(problemsOf((m) => (m.kind = "loop")).some((p) => /unknown kind "loop"/.test(p)));
  });

  const controlsPassed = (loaded) => ({
    verdict: "passed",
    controls: loaded.manifest.soak.controls.names.map((name) => ({ name, bit: true })),
  });
  const stand =
    (verdict, digest, { summary = true, controls = controlsPassed, figures = true } = {}) =>
    (loaded) => {
      const shared = controls(loaded);
      return {
        controls: shared,
        ...Object.fromEntries(
          loaded.run.soak.modes.map((mode) => [
            mode,
            {
              verdict: {
                verdict,
                failures: verdict === "passed" ? [] : [{ kind: "retained-leak" }],
              },
              summary: summary
                ? {
                    mode,
                    tranches: loaded.run.soak.tranches,
                    manifestDigest: digest ?? loaded.manifest.soak.workloads[mode].digest,
                    ...(figures
                      ? {
                          records: Array.from({ length: 10_000 }, (_, i) =>
                            i === 0
                              ? {
                                  class: "pressure-surge",
                                  refusalsPressure: 1,
                                  refusalsOversized: 1,
                                  pressure: { refusalsActive: 0, active: { attributedPeak: 5 } },
                                }
                              : { class: "cold-demand" },
                          ),
                          samples: [
                            { at: "closed", counts: { retainedBytes: 0, peakTotalBytes: 9 } },
                            { at: "final", counts: { retainedBytes: 0, peakTotalBytes: 9 } },
                          ],
                          process: { peakBytes: 7 },
                        }
                      : {}),
                  }
                : null,
              controls: shared,
              wallMs: 1,
            },
          ]),
        ),
      };
    };

  test("a run passes only when both modes pass the frozen workload", async () => {
    const run = async (runSoak) => {
      const dir = freshDir();
      const code = await quiet(() => main(["--run", "rev11-l1-soak", "--out", dir], { runSoak }));
      return { code, summary: readSummary(dir) };
    };
    const passed = await run(stand("passed"));
    assert.equal(passed.code, 0);
    assert.equal(passed.summary.cells[0].metrics.actions.values, 10_000);
    assert.equal(passed.summary.cells[2].metrics.peakRssBytes.status, "not measured");
    const failed = await run(stand("failed"));
    assert.equal(failed.code, 1);
    assert.ok(failed.summary.validation.problems.some((p) => /retained-leak/.test(p)));
    const drifted = await run(stand("passed", "0".repeat(64)));
    assert.equal(drifted.code, 1);
    assert.ok(drifted.summary.validation.problems.some((p) => /not the frozen/.test(p)));
  });

  test("a passing verdict with required work figures unrecorded is not accepted", async () => {
    const dir = freshDir();
    const code = await quiet(() =>
      main(["--run", "rev11-l1-soak", "--out", dir], {
        runSoak: stand("passed", undefined, { figures: false }),
      }),
    );
    assert.equal(code, 1);
    const problems = readSummary(dir).validation.problems;
    for (const metric of ["actions", "samples", "closedRetainedBytes", "surgeRefusalsOversized"])
      assert.ok(
        problems.some((p) => p.includes(`required work metric ${metric} is unavailable`)),
        problems.join("\n"),
      );
    // Time and memory stay advisory.
    assert.ok(!problems.some((p) => /peakRssBytes|wallMs/.test(p)));
  });

  test("a passing verdict without the summary it judged is not accepted", async () => {
    const dir = freshDir();
    const code = await quiet(() =>
      main(["--run", "rev11-l1-soak", "--out", dir], {
        runSoak: stand("passed", undefined, { summary: false }),
      }),
    );
    assert.equal(code, 1);
    assert.ok(readSummary(dir).validation.problems.some((p) => /wrote no summary/.test(p)));
  });

  test("negative controls that did not run or did not bite fail the run", async () => {
    const check = async (controls, pattern) => {
      const dir = freshDir();
      const code = await quiet(() =>
        main(["--run", "rev11-l1-soak", "--out", dir], {
          runSoak: stand("passed", undefined, { controls }),
        }),
      );
      assert.equal(code, 1);
      const problems = readSummary(dir).validation.problems;
      assert.ok(
        problems.some((p) => pattern.test(p)),
        problems.join("\n"),
      );
    };
    await check(
      () => ({ verdict: "passed", controls: [] }),
      /negative control retained-key-leak did not run/,
    );
    await check(
      (loaded) => ({
        verdict: "failed",
        controls: loaded.manifest.soak.controls.names.map((name) => ({
          name,
          bit: name !== "unexpected-skip",
        })),
      }),
      /negative controls failed: unexpected-skip/,
    );
  });

  test("a runner that ends abnormally leaves no earlier artifact to accept", () => {
    const dir = freshDir();
    const crash = join(dir, "crash.mjs");
    writeFileSync(crash, "process.exit(3);\n");
    const out = join(dir, "run");
    mkdirSync(out, { recursive: true });
    writeFileSync(join(out, "soak-verdict.json"), JSON.stringify({ verdict: "passed" }));
    writeFileSync(join(out, "soak-summary.json"), JSON.stringify({ manifestDigest: "old" }));
    const run = invokeSoakRunner(process.execPath, [crash], out);
    assert.match(run.ended ?? "", /ended abnormally \(status 3\)/);
    assert.equal(run.read("soak-verdict.json"), null);
    assert.equal(run.read("soak-summary.json"), null);
  });
});
