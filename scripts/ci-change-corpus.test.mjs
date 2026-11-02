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
  readMetadata,
  replay,
  replayFixture,
  replayPlant,
  replaySelectorPlant,
  validateCorpus,
  withoutEdges,
} from "./ci-change-corpus.mjs";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SCRIPT = join(ROOT, "scripts", "ci-change-corpus.mjs");

const { corpus } = loadCorpus();
const metadata = readMetadata(ROOT);
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

test("every change sample answers as the corpus expects on the real crate graph", () => {
  for (const scenario of scenarios("change")) {
    for (const sample of scenario.samples) {
      assert.deepEqual(
        checkExpectation(answerFor(sample, metadata), sample.expect),
        [],
        `${scenario.id}/${sample.id}`,
      );
    }
  }
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

test("every selector plant answers correctly on the true graph and is caught on the planted one", () => {
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

const runResult = (overrides = {}) => ({
  manifest: { corpus: "c/1", corpusDigest: "d", arm: "baseline", repetition: 1 },
  ok: true,
  selection: [
    { scenario: "A", sample: "a", gatesOn: ["rust"], impactedCrates: ["x"], violations: [] },
    { scenario: "A", sample: "b", gatesOn: ["js"], impactedCrates: [], violations: [] },
  ],
  fixtures: [{ scenario: "F", rows: [{ test: "t", verdict: "passed" }] }],
  ...overrides,
});

test("comparison pairs identical inputs and reports widened and narrowed selections", () => {
  const candidate = runResult({
    manifest: { corpus: "c/1", corpusDigest: "d", arm: "candidate", repetition: 1 },
    selection: [
      { scenario: "A", sample: "a", gatesOn: ["wasm"], impactedCrates: ["x", "y"], violations: [] },
      { scenario: "A", sample: "b", gatesOn: ["js"], impactedCrates: [], violations: [] },
    ],
  });
  const comparison = compareRuns(runResult(), candidate);
  assert.equal(comparison.comparable, true);
  assert.deepEqual(comparison.changed, [
    {
      scenario: "A",
      sample: "a",
      gatesAdded: ["wasm"],
      gatesRemoved: ["rust"],
      cratesAdded: ["y"],
      cratesRemoved: [],
    },
  ]);
  assert.equal(comparison.unchanged, 1);
});

test("comparison refuses different corpus bytes, unmatched samples and reports a violated candidate", () => {
  const otherBytes = runResult();
  otherBytes.manifest = { ...otherBytes.manifest, corpusDigest: "other" };
  assert.equal(compareRuns(runResult(), otherBytes).comparable, false);

  const dropped = runResult({ selection: runResult().selection.slice(1) });
  const missing = compareRuns(runResult(), dropped);
  assert.equal(missing.comparable, false);
  assert.match(missing.problems.join(), /A\/a/);

  const violated = runResult({
    ok: false,
    selection: [
      {
        scenario: "A",
        sample: "a",
        gatesOn: [],
        impactedCrates: [],
        violations: ["must select rust"],
      },
      runResult().selection[1],
    ],
  });
  const comparison = compareRuns(runResult(), violated);
  assert.equal(comparison.comparable, true);
  assert.deepEqual(comparison.candidateViolations, ["A/a: must select rust"]);
});

test("a replay records identities and answers, names unavailable evidence and never overwrites", () => {
  withTempDir((dir) => {
    const out = join(dir, "baseline-1");
    const options = { out, arm: "baseline", repetition: 1, metadata, fixtures: false, root: ROOT };
    const result = replay(options);
    assert.equal(result.ok, true);
    const stored = JSON.parse(readFileSync(join(out, "results.json"), "utf8"));
    assert.equal(stored.manifest.corpus, corpus.corpus);
    assert.match(stored.manifest.corpusDigest, /^[0-9a-f]{64}$/);
    assert.match(stored.manifest.configDigests[".github/workflows/ci.yml"], /^[0-9a-f]{64}$/);
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

test("the CLI exits non-zero when two runs cannot be compared", () => {
  withTempDir((dir) => {
    const write = (name, value) => {
      const run = join(dir, name);
      mkdirSync(run);
      writeFileSync(join(run, "results.json"), JSON.stringify(value));
      return run;
    };
    const other = runResult();
    other.manifest = { ...other.manifest, corpusDigest: "other" };
    const run = spawnSync(
      process.execPath,
      [SCRIPT, "compare", "--baseline", write("a", runResult()), "--candidate", write("b", other)],
      { cwd: ROOT, encoding: "utf8" },
    );
    assert.equal(run.status, 1, run.stdout + run.stderr);
    assert.match(run.stdout + run.stderr, /digest/);
  });
});
