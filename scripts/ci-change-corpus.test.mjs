import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  answerFor,
  checkExpectation,
  compareRuns,
  loadCorpus,
  metadataDigest,
  replay,
  replayFixture,
  replayPlant,
  replaySelectorPlant,
  validateCorpus,
  withoutEdges,
} from "./ci-change-corpus.mjs";
import { LANE_ROOTS } from "./ci-impact.mjs";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SCRIPT = join(ROOT, "scripts", "ci-change-corpus.mjs");

const { corpus } = loadCorpus();

// Crate-graph changes do not select the lane that runs this file, so no
// assertion here may depend on the live workspace graph. The graph is a
// synthetic `cargo metadata` document that holds every lane root plus the few
// crates the selector plants name; replay and compare answer the real graph.
function syntheticMetadata(workspaceRoot = "/repo") {
  const edges = {
    verter_wasm: [{ name: "verter_span" }],
    verter_session: [{ name: "verter_span" }, { name: "verter_test_support", kind: "dev" }],
    verter_lsp: [{ name: "verter_session" }, { name: "verter_test_support", kind: "dev" }],
  };
  const names = [
    ...new Set([
      ...Object.values(LANE_ROOTS).flat(),
      "verter_span",
      "verter_test_support",
      ...Object.keys(edges),
    ]),
  ];
  const idOf = (name) => `path+file://${workspaceRoot}/crates/${name}#0.0.0`;
  return {
    workspace_root: workspaceRoot,
    workspace_members: names.map(idOf),
    packages: names.map((name) => ({
      id: idOf(name),
      name,
      manifest_path: `${workspaceRoot}/crates/${name}/Cargo.toml`,
      targets: [{ kind: ["lib"] }],
    })),
    resolve: {
      nodes: names.map((name) => ({
        id: idOf(name),
        deps: (edges[name] ?? []).map((dep) => ({
          name: dep.name,
          pkg: idOf(dep.name),
          dep_kinds: [{ kind: dep.kind ?? null, target: null }],
        })),
      })),
    },
  };
}
const metadata = syntheticMetadata();
const scenarios = (kind) => corpus.scenarios.filter((scenario) => scenario.kind === kind);

function withTempDir(body) {
  const dir = mkdtempSync(join(tmpdir(), "ci-change-corpus-"));
  try {
    return body(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("the versioned corpus covers every required dimension and names real workflow jobs", () => {
  assert.deepEqual(validateCorpus(corpus, ROOT), []);
});

test("a corpus missing a required dimension, a stream or a live job is refused", () => {
  const untagged = {
    ...corpus,
    scenarios: corpus.scenarios.map((scenario) => ({
      ...scenario,
      dimensions: (scenario.dimensions ?? []).filter((d) => d !== "release-tag"),
    })),
  };
  assert.match(validateCorpus(untagged, ROOT).join("\n"), /release-tag/);

  assert.match(validateCorpus({ ...corpus, streams: [] }, ROOT).join("\n"), /concurrent-prs/);

  const staleCell = {
    ...corpus,
    cells: [{ id: "gone", workflow: ".github/workflows/ci.yml", job: "no-such-job" }],
  };
  assert.match(validateCorpus(staleCell, ROOT).join("\n"), /no-such-job/);

  const duplicate = { ...corpus, scenarios: [...corpus.scenarios, corpus.scenarios[0]] };
  assert.match(validateCorpus(duplicate, ROOT).join("\n"), /duplicate/);
});

test("a replayed answer is what the CI command publishes for the same input", () => {
  withTempDir((dir) => {
    const metadataPath = join(dir, "metadata.json");
    writeFileSync(metadataPath, JSON.stringify(metadata));
    const samples = scenarios("change").flatMap((scenario) => scenario.samples);
    const released = samples.find((sample) => sample.release);
    assert.ok(released, "the corpus holds a release sample");
    for (const sample of [samples[0], samples[1], released]) {
      const changed = join(dir, "changed.json");
      const output = join(dir, `${sample.id}.output`);
      writeFileSync(changed, JSON.stringify(sample.changedFiles));
      const run = spawnSync(
        process.execPath,
        [
          join(ROOT, "scripts", "ci-impact.mjs"),
          "--changed-files",
          changed,
          "--metadata",
          metadataPath,
          "--github-output",
          output,
          ...(sample.release ? ["--release", sample.release] : []),
        ],
        { cwd: ROOT, encoding: "utf8" },
      );
      assert.equal(run.status, 0, run.stderr);
      const published = readFileSync(output, "utf8")
        .split("\n")
        .filter((line) => /^gate_.*=true$/.test(line))
        .map((line) => line.slice("gate_".length, -"=true".length))
        .sort();
      assert.deepEqual(answerFor(sample, metadata).gatesOn, published, sample.id);
    }
  });
});

test("an expectation rejects a dropped gate, an added gate, a wrong class and a closure inversion", () => {
  const answer = {
    class: "narrow",
    gatesOn: ["rust", "wasm"],
    impactedCrates: ["verter_session", "verter_type_engine"],
  };
  assert.deepEqual(
    checkExpectation(answer, {
      class: "narrow",
      mustSelect: ["rust"],
      mustNotSelect: ["providers"],
      mustImpact: ["verter_session"],
      mustNotImpact: ["verter_lsp"],
    }),
    [],
  );
  assert.match(checkExpectation(answer, { mustSelect: ["lsp"] }).join(), /must select lsp/);
  assert.match(
    checkExpectation(answer, { mustNotSelect: ["wasm"] }).join(),
    /must not select wasm/,
  );
  assert.match(checkExpectation(answer, { class: "everything" }).join(), /class narrow/);
  assert.match(
    checkExpectation(answer, { mustNotImpact: ["verter_type_engine"] }).join(),
    /must not impact verter_type_engine/,
  );
  assert.match(
    checkExpectation(answer, { mustImpact: ["verter_lsp"] }).join(),
    /must impact verter_lsp/,
  );
});

test("removing a dependency edge removes only edges of that kind and leaves the input intact", () => {
  const countEdges = (graph) => graph.resolve.nodes.flatMap((node) => node.deps).length;
  const before = countEdges(metadata);
  const noDev = withoutEdges(metadata, { kind: "dev" });
  const depKinds = noDev.resolve.nodes.flatMap((node) =>
    node.deps.flatMap((dep) => dep.dep_kinds.map(({ kind }) => kind)),
  );
  assert.equal(depKinds.includes("dev"), false);
  assert.equal(depKinds.includes(null), true);
  assert.equal(countEdges(metadata), before);
});

test("every selector plant answers correctly on the true graph and is caught on the planted one (synthetic graph)", () => {
  const plants = scenarios("selector-plant");
  assert.ok(plants.length >= 2);
  for (const scenario of plants) {
    const result = replaySelectorPlant(scenario, metadata);
    assert.deepEqual(result.control, [], `${scenario.id}: control`);
    assert.notDeepEqual(result.planted, [], `${scenario.id}: an unsound graph must be rejected`);
    assert.equal(result.verdict, "rejected-as-expected", scenario.id);
  }
});

test("a selector plant that changes nothing is a failed qualification, not a pass", () => {
  const [scenario] = scenarios("selector-plant");
  const inert = { ...scenario, dropEdges: { from: "no_such_crate" } };
  assert.equal(replaySelectorPlant(inert, metadata).verdict, "plant-not-effective");
});

test("a fixture replay fails on a filtered-out name, a skip and a failed subject", () => {
  const scenario = {
    id: "X",
    kind: "fixture",
    fixtures: [{ file: "scripts/x.test.mjs", test: "a named case", subject: "reject" }],
  };
  const run = (stdout, status = 0) =>
    replayFixture(scenario, { run: () => ({ status, stdout }) }, ROOT);
  assert.equal(run("TAP version 13\n# tests 0\n").rows[0].verdict, "no-matching-test");
  assert.equal(
    run("ok 1 - a named case # SKIP test name patterns\n").rows[0].verdict,
    "no-matching-test",
  );
  assert.equal(run("not ok 1 - a named case\n", 1).rows[0].verdict, "failed");
  const passed = run("ok 1 - a named case\n");
  assert.equal(passed.rows[0].verdict, "passed");
  assert.equal(passed.rows[0].subject, "reject");
  assert.equal(passed.ok, true);

  // A failure is not erased by a later same-named pass, a body-less TODO ran
  // nothing, and a child that exits non-zero certifies none of its passes.
  const laterPass = run("not ok 1 - a named case\nok 2 - a named case\n", 1);
  assert.equal(laterPass.rows[0].verdict, "failed");
  assert.equal(laterPass.ok, false);
  assert.equal(run("ok 1 - a named case # TODO\n").rows[0].verdict, "no-matching-test");
  const crashed = run("ok 1 - a named case\n", 1);
  assert.equal(crashed.rows[0].verdict, "runner-failed");
  assert.equal(crashed.ok, false);
  const killed = replayFixture(
    scenario,
    { run: () => ({ status: null, stdout: "ok 1 - a named case\n", error: new Error("timeout") }) },
    ROOT,
  );
  assert.equal(killed.ok, false);
});

test("a named fixture actually runs through the test runner", () => {
  const scenario = {
    id: "X",
    kind: "fixture",
    fixtures: [
      {
        file: "scripts/release-proof.spec.mjs",
        test: "refuses a tagged tree that is not the tree the pull request's CI tested",
        subject: "reject",
      },
      { file: "scripts/release-proof.spec.mjs", test: "no such fixture name", subject: "reject" },
    ],
  };
  const result = replayFixture(scenario, {}, ROOT);
  assert.deepEqual(
    result.rows.map((row) => row.verdict),
    ["passed", "no-matching-test"],
  );
  assert.equal(result.ok, false);
});

function plantRepo(dir) {
  const git = (...args) => {
    const run = spawnSync("git", ["-C", dir, ...args], { encoding: "utf8" });
    assert.equal(run.status, 0, run.stderr);
  };
  git("init", "-q");
  git("config", "core.autocrlf", "false");
  writeFileSync(join(dir, "subject.txt"), "clean\n");
  writeFileSync(
    join(dir, "check.js"),
    `const t = require("fs").readFileSync("subject.txt", "utf8");
     if (t.includes("PLANTED")) { console.log("plant-failure PLANTED"); process.exit(1); }`,
  );
  git("add", ".");
  git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "seed");
}

const plantScenario = (overrides = {}) => ({
  id: "PX",
  kind: "plant",
  plant: { file: "subject.txt", append: "PLANTED\n", marker: "PLANTED" },
  obligation: { argv: [process.execPath, "check.js"], expectFailureMatch: "plant-failure" },
  ...overrides,
});

test("a failing-change plant applies once, fails its obligation and restores the tree", () => {
  withTempDir((dir) => {
    plantRepo(dir);
    const result = replayPlant(plantScenario(), dir);
    assert.equal(result.verdict, "failed-as-expected");
    assert.equal(readFileSync(join(dir, "subject.txt"), "utf8"), "clean\n");
  });
});

test("a plant creating a new file removes it again", () => {
  withTempDir((dir) => {
    plantRepo(dir);
    const scenario = plantScenario({
      plant: { file: "new.txt", create: "x PLANTED\n", marker: "PLANTED" },
      obligation: {
        argv: [process.execPath, "-e", "console.log('plant-failure');process.exit(1)"],
        expectFailureMatch: "plant-failure",
      },
    });
    assert.equal(replayPlant(scenario, dir).verdict, "failed-as-expected");
    assert.equal(existsSync(join(dir, "new.txt")), false);
  });
});

test("a plant that passes, fails for another reason, or cannot prove it applied is not qualification", () => {
  withTempDir((dir) => {
    plantRepo(dir);
    assert.equal(
      replayPlant(
        plantScenario({
          obligation: { argv: [process.execPath, "-e", ""], expectFailureMatch: "x" },
        }),
        dir,
      ).verdict,
      "obligation-passed",
    );
    assert.equal(
      replayPlant(
        plantScenario({
          obligation: {
            argv: [process.execPath, "-e", "process.exit(1)"],
            expectFailureMatch: "plant-failure",
          },
        }),
        dir,
      ).verdict,
      "failed-for-another-reason",
    );
    assert.equal(
      replayPlant(
        plantScenario({ plant: { file: "subject.txt", append: "\n", marker: "PLANTED" } }),
        dir,
      ).verdict,
      "plant-not-applied",
    );
    assert.equal(
      replayPlant(
        plantScenario({ plant: { file: "subject.txt", append: "clean\n", marker: "clean" } }),
        dir,
      ).verdict,
      "plant-not-applied",
    );
    assert.equal(readFileSync(join(dir, "subject.txt"), "utf8"), "clean\n");
  });
});

test("a plant refuses a dirty tree rather than discard someone's work", () => {
  withTempDir((dir) => {
    plantRepo(dir);
    writeFileSync(join(dir, "subject.txt"), "someone's edit\n");
    assert.equal(replayPlant(plantScenario(), dir).verdict, "dirty-tree");
    assert.equal(readFileSync(join(dir, "subject.txt"), "utf8"), "someone's edit\n");
  });
});

const selectionRow = (sample, overrides = {}) => ({
  scenario: "A",
  sample,
  class: "narrow",
  gatesOn: ["rust"],
  impactedCrates: [],
  violations: [],
  ...overrides,
});

const runResult = (arm = "baseline", overrides = {}) => ({
  manifest: { corpus: "c/1", corpusDigest: "d", definitionsDigest: "m", arm, repetition: 1 },
  ok: true,
  corpusProblems: [],
  selection: [selectionRow("a", { impactedCrates: ["x"] }), selectionRow("b", { gatesOn: ["js"] })],
  selectorPlants: [{ scenario: "SP", verdict: "rejected-as-expected" }],
  fixtures: [
    {
      scenario: "F",
      rows: [{ file: "f.mjs", test: "t", subject: "reject", verdict: "passed" }],
    },
  ],
  ...overrides,
});

const fixtureRun = (verdict) => [
  { scenario: "F", rows: [{ file: "f.mjs", test: "t", subject: "reject", verdict }] },
];

test("comparison pairs identical inputs and reports widened, narrowed and reclassified selections", () => {
  const candidate = runResult("candidate", {
    selection: [
      selectionRow("a", { gatesOn: ["wasm"], impactedCrates: ["x", "y"], class: "impact-full" }),
      selectionRow("b", { gatesOn: ["js"] }),
    ],
  });
  const comparison = compareRuns(runResult(), candidate);
  assert.equal(comparison.comparable, true);
  assert.deepEqual(comparison.changed, [
    {
      scenario: "A",
      sample: "a",
      classChanged: { from: "narrow", to: "impact-full" },
      gatesAdded: ["wasm"],
      gatesRemoved: ["rust"],
      cratesAdded: ["y"],
      cratesRemoved: [],
    },
  ]);
  assert.equal(comparison.unchanged, 1);

  const classOnly = runResult("candidate", {
    selection: [
      selectionRow("a", { impactedCrates: ["x"], class: "everything" }),
      selectionRow("b", { gatesOn: ["js"] }),
    ],
  });
  assert.equal(compareRuns(runResult(), classOnly).changed.length, 1);
});

test("comparison refuses different corpus bytes, unmatched samples and reports a violated candidate", () => {
  const otherBytes = runResult("candidate");
  otherBytes.manifest = { ...otherBytes.manifest, corpusDigest: "other" };
  assert.equal(compareRuns(runResult(), otherBytes).comparable, false);

  const dropped = runResult("candidate", { selection: runResult().selection.slice(1) });
  const missing = compareRuns(runResult(), dropped);
  assert.equal(missing.comparable, false);
  assert.match(missing.problems.join(), /A\/a/);

  const violated = runResult("candidate", {
    ok: false,
    selection: [
      selectionRow("a", { gatesOn: [], violations: ["must select rust"] }),
      runResult().selection[1],
    ],
  });
  const comparison = compareRuns(runResult(), violated);
  assert.equal(comparison.comparable, true);
  assert.deepEqual(comparison.candidateViolations, ["A/a: must select rust"]);
});

test("comparison refuses swapped arms, mismatched repetitions and arms that did not run the same fixtures", () => {
  assert.match(compareRuns(runResult("candidate"), runResult()).problems.join(), /arms are/);
  assert.match(compareRuns(runResult(), runResult("baseline")).problems.join(), /arms are/);

  const later = runResult("candidate");
  later.manifest = { ...later.manifest, repetition: 2 };
  assert.match(compareRuns(runResult(), later).problems.join(), /repetition differs/);

  const noFixtures = runResult("candidate", { fixtures: [] });
  const lost = compareRuns(runResult(), noFixtures);
  assert.equal(lost.comparable, false);
  assert.match(lost.problems.join(), /candidate has no answer for fixture F\/f\.mjs\/t/);
  assert.equal(compareRuns(noFixtures, runResult("candidate")).comparable, false);

  const noPlants = runResult("candidate", { selectorPlants: [] });
  assert.match(compareRuns(runResult(), noPlants).problems.join(), /selector plant SP/);
});

test("a candidate that lost a selector plant, a fixture or a clean corpus is a violation", () => {
  const plant = runResult("candidate", {
    ok: false,
    selectorPlants: [{ scenario: "SP", verdict: "plant-not-effective" }],
  });
  const plantComparison = compareRuns(runResult(), plant);
  assert.equal(plantComparison.comparable, true);
  assert.deepEqual(plantComparison.candidateViolations, ["SP: selector plant plant-not-effective"]);
  assert.deepEqual(plantComparison.selectorPlantChanges, [
    "SP: rejected-as-expected -> plant-not-effective",
  ]);

  const fixture = runResult("candidate", { ok: false, fixtures: fixtureRun("failed") });
  const fixtureComparison = compareRuns(runResult(), fixture);
  assert.deepEqual(fixtureComparison.candidateViolations, ["F: t: failed"]);
  assert.deepEqual(fixtureComparison.fixtureChanges, ["F/f.mjs/t: passed -> failed"]);

  const corpusRun = runResult("candidate", { ok: false, corpusProblems: ["no entry covers x"] });
  assert.deepEqual(compareRuns(runResult(), corpusRun).candidateViolations, [
    "corpus: no entry covers x",
  ]);
  assert.deepEqual(
    compareRuns(runResult(), runResult("candidate", { ok: false })).candidateViolations,
    ["run reported ok=false"],
  );
});

test("a crate graph digest ignores the checkout path and sees a graph change", () => {
  const here = syntheticMetadata("/work/one");
  assert.equal(metadataDigest(here), metadataDigest(syntheticMetadata("/other/two")));
  assert.equal(
    metadataDigest(syntheticMetadata("D:\\work\\one")),
    metadataDigest(syntheticMetadata("/other/two")),
  );
  assert.notEqual(metadataDigest(here), metadataDigest(withoutEdges(here, { kind: "dev" })));
});

test("a replay records identities and answers, names unavailable evidence and never overwrites", () => {
  withTempDir((dir) => {
    const out = join(dir, "baseline-1");
    const options = { out, arm: "baseline", repetition: 1, metadata, fixtures: false, root: ROOT };
    const result = replay(options);
    assert.deepEqual(result.corpusProblems, []);
    const stored = JSON.parse(readFileSync(join(out, "results.json"), "utf8"));
    assert.equal(stored.manifest.corpus, corpus.corpus);
    assert.match(stored.manifest.corpusDigest, /^[0-9a-f]{64}$/);
    assert.match(stored.manifest.configDigests[".github/workflows/ci.yml"], /^[0-9a-f]{64}$/);
    assert.match(
      stored.manifest.configDigests["scripts/provider-ci-internals.mjs"],
      /^[0-9a-f]{64}$/,
    );
    assert.equal(typeof stored.manifest.tree, "string");
    assert.ok(stored.selection.length > 0);
    assert.equal(
      stored.unavailable.some(({ id }) => id === "CC01"),
      true,
    );
    assert.equal(
      stored.unavailable.every(({ reason }) => reason.length > 0),
      true,
    );
    assert.throws(() => replay(options), /already holds/);
  });
});

test("a replay of an empty or incomplete corpus is not ok, in the library and the CLI", () => {
  withTempDir((dir) => {
    const corpusFile = join(dir, "empty.json");
    writeFileSync(corpusFile, JSON.stringify({ corpus: "ci-change-corpus/0", scenarios: [] }));
    const result = replay({
      corpusFile,
      out: join(dir, "library"),
      arm: "baseline",
      repetition: 1,
      metadata,
      fixtures: false,
      root: ROOT,
    });
    assert.equal(result.ok, false);
    assert.match(result.corpusProblems.join(), /no entry covers dimension/);

    const metadataFile = join(dir, "metadata.json");
    writeFileSync(metadataFile, JSON.stringify(metadata));
    const run = spawnSync(
      process.execPath,
      [
        SCRIPT,
        "replay",
        "--out",
        join(dir, "cli"),
        "--arm",
        "baseline",
        "--repetition",
        "1",
        "--corpus",
        corpusFile,
        "--metadata",
        metadataFile,
        "--no-fixtures",
      ],
      { cwd: ROOT, encoding: "utf8" },
    );
    assert.equal(run.status, 1, run.stdout + run.stderr);
  });
});

test("a corpus naming a fixture the suite no longer holds is refused", () => {
  const [first] = scenarios("fixture");
  const renamed = {
    ...corpus,
    scenarios: corpus.scenarios.map((scenario) =>
      scenario === first
        ? { ...scenario, fixtures: [{ ...first.fixtures[0], test: "a case renamed away" }] }
        : scenario,
    ),
  };
  assert.match(validateCorpus(renamed, ROOT).join("\n"), /renamed away/);
});

function writeRun(dir, name, value) {
  const run = join(dir, name);
  mkdirSync(run);
  writeFileSync(join(run, "results.json"), JSON.stringify(value));
  return run;
}

const compareCli = (baseline, candidate) =>
  spawnSync(
    process.execPath,
    [SCRIPT, "compare", "--baseline", baseline, "--candidate", candidate],
    { cwd: ROOT, encoding: "utf8" },
  );

test("the CLI exits non-zero when two runs cannot be compared", () => {
  withTempDir((dir) => {
    const other = runResult("candidate");
    other.manifest = { ...other.manifest, corpusDigest: "other" };
    const run = compareCli(writeRun(dir, "a", runResult()), writeRun(dir, "b", other));
    assert.equal(run.status, 1, run.stdout + run.stderr);
    assert.match(run.stdout + run.stderr, /digest/);
  });
});

test("the CLI compare exits non-zero for a candidate that lost its plants or fixtures, and zero for a clean pair", () => {
  withTempDir((dir) => {
    const baseline = writeRun(dir, "base", runResult());
    const clean = compareCli(baseline, writeRun(dir, "clean", runResult("candidate")));
    assert.equal(clean.status, 0, clean.stdout + clean.stderr);

    const failedPlant = runResult("candidate", {
      ok: false,
      selectorPlants: [{ scenario: "SP", verdict: "plant-not-effective" }],
      fixtures: fixtureRun("failed"),
    });
    const failed = compareCli(baseline, writeRun(dir, "failed", failedPlant));
    assert.equal(failed.status, 1, failed.stdout + failed.stderr);

    const noFixtures = compareCli(
      baseline,
      writeRun(dir, "none", runResult("candidate", { fixtures: [] })),
    );
    assert.equal(noFixtures.status, 1, noFixtures.stdout + noFixtures.stderr);
  });
});
