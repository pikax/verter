#!/usr/bin/env node

/**
 * ci-change-corpus.mjs — replay and compare the versioned CI change corpus.
 *
 * `scripts/ci-change-corpus/scenarios.v1.json` is the input; the protocol and
 * the metric definitions are `docs/contributing/ci-change-corpus.v1.md` and
 * `docs/contributing/ci-change-corpus-collection.v1.md`. This tool answers
 * those scenarios through the existing authorities (`ci-impact.mjs` for lane
 * selection, the named test suites for fixtures). It holds no selection logic,
 * runs no hosted workflow and stores no result in the repository: a run's
 * results belong to the experiment that collected them.
 *
 *   node scripts/ci-change-corpus.mjs replay --out <dir> --arm <baseline|candidate>
 *     --repetition <n> [--corpus <file>] [--metadata <cargo-metadata.json>] [--no-fixtures]
 *   node scripts/ci-change-corpus.mjs compare --baseline <dir> --candidate <dir> [--out <file>]
 *   node scripts/ci-change-corpus.mjs plant <scenario-id> [--corpus <file>] [--worktree <dir>]
 *
 * `replay` and `compare` exit 1 on a violated expectation or an incomparable
 * pair. `plant` mutates its worktree for the duration of the obligation, so it
 * refuses a dirty tree and exits 1 unless the plant applied and failed for the
 * declared reason.
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  classifyCiImpact,
  composeLaneGates,
  LANE_ROOTS,
  loadCargoMetadata,
  matchPathFilters,
  releaseGates,
  SELECTION_CONFIG_ERRORS,
} from "./ci-impact.mjs";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const DEFAULT_CORPUS = join(REPO_ROOT, "scripts", "ci-change-corpus", "scenarios.v1.json");

/** A complete corpus exercises each of these; `validateCorpus` refuses one that does not. */
const REQUIRED_DIMENSIONS = Object.freeze([
  "passing",
  "failing",
  "concurrent-prs",
  "superseded-push",
  "shared-input",
  "platform-feature",
  "release-tag",
  "cold-warm",
]);

/** Inputs whose bytes decide a selection or a run's obligations; recorded per run. */
const CONFIG_INPUTS = Object.freeze([
  ".github/workflows/ci.yml",
  ".github/workflows/release.yml",
  ".github/workflows/release-ide.yml",
  "scripts/ci-impact.mjs",
  "scripts/ci-inert-paths.json",
  "scripts/provider-ci-internals.mjs",
  "tests/sfc-projection/STP1/products/current-feature-inventory.json",
  "scripts/lib/crate-graph.mjs",
  "scripts/check-ci-aggregate.mjs",
  "scripts/release-lanes.mjs",
  "scripts/release-proof.mjs",
  "scripts/ci-junit-metadata.mjs",
  "Cargo.toml",
  "Cargo.lock",
  "rust-toolchain.toml",
  ".cargo/config.toml",
  ".config/nextest.toml",
  "package.json",
  "pnpm-lock.yaml",
]);

/** The protocol documents: an experiment's arms must read the same metric definitions. */
const DEFINITION_INPUTS = Object.freeze([
  "docs/contributing/ci-change-corpus.v1.md",
  "docs/contributing/ci-change-corpus-collection.v1.md",
]);

/** Text digests ignore checkout line endings, so a Windows and a Linux arm agree. */
const textDigest = (text) =>
  createHash("sha256").update(text.replace(/\r\n/g, "\n"), "utf8").digest("hex");

const fileDigest = (root, relPath) => {
  const path = join(root, relPath);
  return existsSync(path) ? textDigest(readFileSync(path, "utf8")) : "unavailable";
};

export function loadCorpus(path = DEFAULT_CORPUS) {
  const text = readFileSync(path, "utf8");
  return { corpus: JSON.parse(text), digest: textDigest(text) };
}

/** Path-independent digest of a crate graph: no checkout location reaches it. */
export function metadataDigest(metadata) {
  const root = String(metadata.workspace_root ?? "");
  const forms = [JSON.stringify(root).slice(1, -1), root.replaceAll("\\", "/"), root];
  let text = JSON.stringify(metadata);
  for (const form of new Set(forms.filter(Boolean))) text = text.split(form).join("<workspace>");
  // JSON writes a Windows separator as an escaped pair.
  return textDigest(text.replaceAll("\\\\", "/"));
}

export const readMetadata = (root = REPO_ROOT) => loadCargoMetadata(root);

/** Problems that make a corpus unusable as a complete comparison input; empty when it is sound. */
export function validateCorpus(corpus, root = REPO_ROOT) {
  const problems = [];
  const entries = [
    ...(corpus.scenarios ?? []),
    ...(corpus.streams ?? []),
    ...(corpus.cells ?? []),
    ...(corpus.cacheConditions ?? []),
  ];
  const ids = new Set();
  for (const { id } of entries) {
    if (ids.has(id)) problems.push(`duplicate id ${id}`);
    ids.add(id);
  }
  const covered = new Set(entries.flatMap((entry) => entry.dimensions ?? []));
  for (const dimension of REQUIRED_DIMENSIONS) {
    if (!covered.has(dimension)) problems.push(`no entry covers dimension ${dimension}`);
  }
  for (const cell of corpus.cells ?? []) {
    const path = join(root, cell.workflow);
    const workflow = existsSync(path) ? readFileSync(path, "utf8").replace(/\r\n/g, "\n") : "";
    if (!workflow.split("\n").includes(`  ${cell.job}:`)) {
      problems.push(`cell ${cell.id} names ${cell.workflow} job ${cell.job}, which does not exist`);
    }
  }
  for (const scenario of (corpus.scenarios ?? []).filter(({ kind }) => kind === "fixture")) {
    for (const { file, test } of scenario.fixtures ?? []) {
      const path = join(root, file);
      const source = existsSync(path) ? readFileSync(path, "utf8") : null;
      if (source === null)
        problems.push(`fixture ${scenario.id} names ${file}, which does not exist`);
      else if (!source.includes(test)) {
        problems.push(`fixture ${scenario.id} names test "${test}", which ${file} no longer holds`);
      }
    }
  }
  return problems;
}

/** The selection CI publishes for a sample, plus the crate closure behind it. */
export function answerFor(sample, metadata) {
  if (SELECTION_CONFIG_ERRORS.length > 0) {
    const files = SELECTION_CONFIG_ERRORS.map(({ file }) => file).join(", ");
    throw new Error(`ci-change-corpus: selection configuration is unreadable (${files})`);
  }
  const files = sample.changedFiles;
  const { hits, owned } = matchPathFilters(files);
  const impact = classifyCiImpact(files, metadata, LANE_ROOTS, { ownedFiles: owned });
  const gates = releaseGates(composeLaneGates(hits, impact), sample.release ?? "");
  const gatesOn = Object.keys(gates)
    .filter((gate) => gates[gate] === "true")
    .sort();
  const selectionClass =
    gatesOn.length === 0
      ? "none"
      : impact.everything
        ? "everything"
        : impact.full
          ? "impact-full"
          : "narrow";
  return {
    class: selectionClass,
    gatesOn,
    directCrates: impact.directCrates,
    impactedCrates: impact.impactedCrates,
    fullReasons: impact.fullReasons.map(({ id }) => id),
  };
}

/** Each way an answer departs from a sample's declared expectation. */
export function checkExpectation(answer, expect = {}) {
  const violations = [];
  if (expect.class && answer.class !== expect.class) {
    violations.push(`answered class ${answer.class}, expected ${expect.class}`);
  }
  for (const gate of expect.mustSelect ?? []) {
    if (!answer.gatesOn.includes(gate)) violations.push(`must select ${gate}`);
  }
  for (const gate of expect.mustNotSelect ?? []) {
    if (answer.gatesOn.includes(gate)) violations.push(`must not select ${gate}`);
  }
  for (const crate of expect.mustImpact ?? []) {
    if (!answer.impactedCrates.includes(crate)) violations.push(`must impact ${crate}`);
  }
  for (const crate of expect.mustNotImpact ?? []) {
    if (answer.impactedCrates.includes(crate)) violations.push(`must not impact ${crate}`);
  }
  return violations;
}

/** A copy of `metadata` with the dependency edges a plant declares removed. */
export function withoutEdges(metadata, { from, kind } = {}) {
  const names = new Map(metadata.packages.map((pkg) => [pkg.id, pkg.name]));
  const nodes = metadata.resolve.nodes.map((node) => {
    if (from && names.get(node.id) !== from) return node;
    const deps = node.deps.flatMap((dep) => {
      const kinds = kind ? dep.dep_kinds.filter((row) => row.kind !== kind) : [];
      return kinds.length > 0 ? [{ ...dep, dep_kinds: kinds }] : [];
    });
    return { ...node, deps };
  });
  return { ...metadata, resolve: { ...metadata.resolve, nodes } };
}

/**
 * An unsound crate graph must change the answer: the sample holds on the true
 * graph and is violated on the planted one. A plant that leaves the answer
 * intact proved nothing, so it is its own failing verdict.
 */
export function replaySelectorPlant(scenario, metadata) {
  const control = checkExpectation(answerFor(scenario, metadata), scenario.expect);
  const planted = checkExpectation(
    answerFor(scenario, withoutEdges(metadata, scenario.dropEdges)),
    scenario.expect,
  );
  const verdict =
    control.length > 0
      ? "control-violated"
      : planted.length > 0
        ? "rejected-as-expected"
        : "plant-not-effective";
  return { scenario: scenario.id, control, planted, verdict };
}

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function runFixtureFile({ file, tests, root }) {
  // Inherited from a parent `node --test`, this switches the child's output to
  // the runner's binary protocol, which reports no test by name.
  const { NODE_TEST_CONTEXT: _parentRunner, ...env } = process.env;
  return spawnSync(
    process.execPath,
    [
      "--test",
      "--test-reporter=tap",
      ...tests.map((name) => `--test-name-pattern=^${escapeRegExp(name)}$`),
      file,
    ],
    { cwd: root, encoding: "utf8", env, maxBuffer: 64 * 1024 * 1024 },
  );
}

/**
 * Run a fixture scenario's named tests. Each must report a real pass: a name
 * the suite no longer holds selects zero work, which is a failure and never a
 * quiet success. `subject` records what the implementation under test does in
 * that fixture; a passing `reject` row means the plant was correctly refused.
 */
export function replayFixture(scenario, { run = runFixtureFile } = {}, root = REPO_ROOT) {
  const files = [...new Set(scenario.fixtures.map(({ file }) => file))];
  const rows = [];
  const logs = [];
  for (const file of files) {
    const fixtures = scenario.fixtures.filter((fixture) => fixture.file === file);
    const result = run({ file, tests: fixtures.map(({ test }) => test), root });
    const stdout = result.stdout ?? "";
    logs.push(`# ${file} (exit ${result.status})\n${stdout}`);
    // Any `not ok` for a name wins over a later same-named pass; a skipped or
    // body-less TODO case ran no assertion, so it reports nothing.
    const reported = new Map();
    for (const line of stdout.split(/\r?\n/)) {
      const match = /^\s*(not ok|ok) \d+ - (.*)$/.exec(line);
      if (!match) continue;
      const [name, directive] = match[2].split(" # ");
      if (/^(skip|todo)/i.test(directive ?? "")) continue;
      if (match[1] === "ok" && reported.get(name) === "failed") continue;
      reported.set(name, match[1] === "ok" ? "passed" : "failed");
    }
    // A child that did not exit cleanly (non-zero, killed, or never started)
    // certifies none of its passes.
    const clean = result.status === 0 && !result.error;
    for (const { test, subject } of fixtures) {
      const verdict = reported.get(test) ?? "no-matching-test";
      rows.push({
        file,
        test,
        subject,
        verdict: verdict === "passed" && !clean ? "runner-failed" : verdict,
      });
    }
  }
  return {
    scenario: scenario.id,
    ok: rows.every((row) => row.verdict === "passed"),
    rows,
    log: logs.join("\n"),
  };
}

const gitStatus = (worktree) =>
  spawnSync("git", ["-C", worktree, "status", "--porcelain"], { encoding: "utf8" });

function runObligation(argv, worktree) {
  // `pnpm` is a cmd shim on Windows, which only a shell can start.
  if (process.platform === "win32" && argv[0] === "pnpm") {
    return spawnSync(argv.join(" "), {
      cwd: worktree,
      encoding: "utf8",
      shell: true,
      maxBuffer: 64 * 1024 * 1024,
    });
  }
  return spawnSync(argv[0], argv.slice(1), {
    cwd: worktree,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
}

/**
 * Apply a failing change, prove it landed, run the obligation it must break
 * and restore the tree. A plant whose marker is not newly present exactly once
 * is `plant-not-applied`, and an obligation that fails without the declared
 * output is `failed-for-another-reason`: neither is evidence of rejection.
 */
export function replayPlant(scenario, worktree = REPO_ROOT) {
  const { plant, obligation } = scenario;
  const finish = (verdict, output = "") => ({ scenario: scenario.id, verdict, output });
  const status = gitStatus(worktree);
  if (status.status !== 0 || status.stdout.trim() !== "") return finish("dirty-tree");

  const path = join(worktree, plant.file);
  const original = existsSync(path) ? readFileSync(path, "utf8") : null;
  const planted = plant.create ?? `${original ?? ""}${plant.append}`;
  const occurrences = (text) => text.split(plant.marker).length - 1;
  if (occurrences(original ?? "") !== 0 || occurrences(planted) !== 1) {
    return finish("plant-not-applied");
  }
  try {
    writeFileSync(path, planted);
    if (occurrences(readFileSync(path, "utf8")) !== 1) return finish("plant-not-applied");
    const result = runObligation(obligation.argv, worktree);
    const output = `${result.stdout ?? ""}${result.stderr ?? ""}${result.error ?? ""}`;
    if (result.status === 0) return finish("obligation-passed", output);
    return finish(
      new RegExp(obligation.expectFailureMatch).test(output)
        ? "failed-as-expected"
        : "failed-for-another-reason",
      output,
    );
  } finally {
    if (original === null) rmSync(path, { force: true });
    else writeFileSync(path, original);
  }
}

const gitValue = (root, args) => {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : "unavailable";
};

/** Answer every locally answerable scenario into a new directory; never overwrite a run. */
export function replay({
  corpusFile,
  out,
  arm,
  repetition,
  metadata,
  fixtures = true,
  root = REPO_ROOT,
}) {
  if (existsSync(out) && readdirSync(out).length > 0) {
    throw new Error(`ci-change-corpus: ${out} already holds a run`);
  }
  const { corpus, digest } = loadCorpus(corpusFile ?? DEFAULT_CORPUS);
  const corpusProblems = validateCorpus(corpus, root);
  const of = (kind) => (corpus.scenarios ?? []).filter((scenario) => scenario.kind === kind);

  const selection = of("change").flatMap((scenario) =>
    scenario.samples.map((sample) => {
      const answer = answerFor(sample, metadata);
      return {
        scenario: scenario.id,
        sample: sample.id,
        changedFiles: sample.changedFiles,
        release: sample.release ?? "",
        ...answer,
        violations: checkExpectation(answer, sample.expect),
      };
    }),
  );
  const selectorPlants = of("selector-plant").map((scenario) =>
    replaySelectorPlant(scenario, metadata),
  );
  const fixtureRuns = fixtures
    ? of("fixture").map((scenario) => replayFixture(scenario, {}, root))
    : [];

  const notRun = of("plant").map(({ id, obligation }) => ({
    id,
    reason: `mutates a worktree; run: node scripts/ci-change-corpus.mjs plant ${id}`,
    obligation: obligation.argv,
  }));
  const unavailable = [
    ...(corpus.streams ?? []).map(({ id, unavailableReason }) => ({
      id,
      reason: unavailableReason,
    })),
    ...(corpus.cells ?? []).map(({ id }) => ({
      id,
      reason: "a hosted job execution; this local replay does not start workflows",
    })),
    ...(corpus.cacheConditions ?? []).map(({ id }) => ({
      id,
      reason: "cache state is only observable on the job that restores it",
    })),
    ...(fixtures ? [] : [{ id: "fixtures", reason: "--no-fixtures" }]),
  ];

  const ok =
    corpusProblems.length === 0 &&
    selection.every((row) => row.violations.length === 0) &&
    selectorPlants.every((row) => row.verdict === "rejected-as-expected") &&
    fixtureRuns.every((run) => run.ok);

  const definitions = DEFINITION_INPUTS.map((path) => fileDigest(root, path)).join("\n");
  const result = {
    manifest: {
      corpus: corpus.corpus,
      corpusDigest: digest,
      definitionsDigest: textDigest(definitions),
      arm,
      repetition,
      collectedAt: new Date().toISOString(),
      head: gitValue(root, ["rev-parse", "HEAD"]),
      tree: gitValue(root, ["rev-parse", "HEAD^{tree}"]),
      dirty: gitValue(root, ["status", "--porcelain"]) !== "",
      configDigests: Object.fromEntries(
        CONFIG_INPUTS.map((path) => [path, fileDigest(root, path)]),
      ),
      metadataDigest: metadataDigest(metadata),
      fixturesRun: fixtures,
      node: process.version,
      platform: process.platform,
      arch: process.arch,
    },
    ok,
    corpusProblems,
    selection,
    selectorPlants,
    fixtures: fixtureRuns.map(({ scenario, ok: passed, rows }) => ({ scenario, ok: passed, rows })),
    notRun,
    unavailable,
  };
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, "results.json"), `${JSON.stringify(result, null, 2)}\n`);
  for (const run of fixtureRuns) writeFileSync(join(out, `${run.scenario}.tap`), run.log);
  return result;
}

const difference = (left, right) => left.filter((item) => !right.includes(item));

/** Pair rows of one kind by identity; an unmatched row on either side is a problem. */
function pairRows(kind, baselineRows, candidateRows, key, problems) {
  const baseline = new Map(baselineRows.map((row) => [key(row), row]));
  const candidate = new Map(candidateRows.map((row) => [key(row), row]));
  for (const id of baseline.keys()) {
    if (!candidate.has(id)) problems.push(`candidate has no answer for ${kind} ${id}`);
  }
  for (const id of candidate.keys()) {
    if (!baseline.has(id)) problems.push(`baseline has no answer for ${kind} ${id}`);
  }
  return [...baseline].flatMap(([id, row]) =>
    candidate.has(id) ? [{ id, baseline: row, candidate: candidate.get(id) }] : [],
  );
}

const fixtureRows = (run) =>
  (run.fixtures ?? []).flatMap(({ scenario, rows }) => rows.map((row) => ({ scenario, ...row })));

/** Everything in a run that is not the answer the corpus qualifies. */
function runViolations(run) {
  const found = [
    ...(run.corpusProblems ?? []).map((problem) => `corpus: ${problem}`),
    ...(run.selection ?? []).flatMap((row) =>
      row.violations.map((violation) => `${row.scenario}/${row.sample}: ${violation}`),
    ),
    ...(run.selectorPlants ?? [])
      .filter((row) => row.verdict !== "rejected-as-expected")
      .map((row) => `${row.scenario}: selector plant ${row.verdict}`),
    ...fixtureRows(run)
      .filter((row) => row.verdict !== "passed")
      .map((row) => `${row.scenario}: ${row.test}: ${row.verdict}`),
  ];
  if (found.length === 0 && run.ok === false) found.push("run reported ok=false");
  return found;
}

/**
 * Pair two runs over byte-identical inputs. Selection answers, selector plants
 * and fixture rows are each paired by scenario; a row one arm lacks, a
 * mismatch in inputs, or arms that are not a baseline and a candidate of the
 * same repetition means the arms did not answer the same question. Differences
 * between paired answers are reported, not judged; a candidate answer that is
 * not the qualifying one is a violation.
 */
export function compareRuns(baseline, candidate) {
  const problems = [];
  for (const field of ["corpus", "corpusDigest", "definitionsDigest"]) {
    if (baseline.manifest[field] !== candidate.manifest[field]) {
      problems.push(`${field} differs: the arms did not read the same corpus (digest mismatch)`);
    }
  }
  if (baseline.manifest.arm !== "baseline" || candidate.manifest.arm !== "candidate") {
    problems.push(
      `arms are ${baseline.manifest.arm}/${candidate.manifest.arm}, expected baseline/candidate`,
    );
  }
  if (baseline.manifest.repetition !== candidate.manifest.repetition) {
    problems.push(
      `repetition differs: ${baseline.manifest.repetition} against ${candidate.manifest.repetition}`,
    );
  }

  const selectionPairs = pairRows(
    "sample",
    baseline.selection ?? [],
    candidate.selection ?? [],
    ({ scenario, sample }) => `${scenario}/${sample}`,
    problems,
  );
  const plantPairs = pairRows(
    "selector plant",
    baseline.selectorPlants ?? [],
    candidate.selectorPlants ?? [],
    ({ scenario }) => scenario,
    problems,
  );
  const fixturePairs = pairRows(
    "fixture",
    fixtureRows(baseline),
    fixtureRows(candidate),
    ({ scenario, file, test }) => `${scenario}/${file}/${test}`,
    problems,
  );

  const changed = [];
  let unchanged = 0;
  for (const { baseline: row, candidate: other } of selectionPairs) {
    const entry = {
      scenario: row.scenario,
      sample: row.sample,
      classChanged: row.class === other.class ? null : { from: row.class, to: other.class },
      gatesAdded: difference(other.gatesOn, row.gatesOn),
      gatesRemoved: difference(row.gatesOn, other.gatesOn),
      cratesAdded: difference(other.impactedCrates, row.impactedCrates),
      cratesRemoved: difference(row.impactedCrates, other.impactedCrates),
    };
    const moved =
      (entry.classChanged ? 1 : 0) +
      entry.gatesAdded.length +
      entry.gatesRemoved.length +
      entry.cratesAdded.length +
      entry.cratesRemoved.length;
    if (moved > 0) changed.push(entry);
    else unchanged += 1;
  }

  const verdictChanges = (pairs) =>
    pairs
      .filter(({ baseline: before, candidate: after }) => before.verdict !== after.verdict)
      .map(
        ({ id, baseline: before, candidate: after }) =>
          `${id}: ${before.verdict} -> ${after.verdict}`,
      );
  const notCompared = [
    "plants (PL): they mutate a worktree, so each arm runs them with `plant <id>`",
    ...(plantPairs.length === 0 && fixturePairs.length === 0
      ? ["selector plants and fixtures: neither arm ran them"]
      : []),
  ];

  return {
    comparable: problems.length === 0,
    problems,
    arms: {
      baseline: baseline.manifest,
      candidate: candidate.manifest,
    },
    changed,
    unchanged,
    candidateViolations: runViolations(candidate),
    baselineViolations: runViolations(baseline),
    selectorPlantChanges: verdictChanges(plantPairs),
    fixtureChanges: verdictChanges(fixturePairs),
    notCompared,
  };
}

const flag = (argv, name) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv[at + 1] : undefined;
};

const readRun = (dir) => JSON.parse(readFileSync(join(resolve(dir), "results.json"), "utf8"));

function main(argv = process.argv.slice(2)) {
  const [command, ...rest] = argv;
  if (command === "replay") {
    const out = flag(rest, "--out");
    const arm = flag(rest, "--arm");
    const repetition = Number(flag(rest, "--repetition"));
    if (!out || !["baseline", "candidate"].includes(arm) || !Number.isInteger(repetition)) {
      process.stderr.write("ci-change-corpus: replay needs --out, --arm and --repetition\n");
      return 2;
    }
    const metadataFile = flag(rest, "--metadata");
    const result = replay({
      corpusFile: flag(rest, "--corpus") && resolve(flag(rest, "--corpus")),
      out: resolve(out),
      arm,
      repetition,
      metadata: metadataFile ? JSON.parse(readFileSync(metadataFile, "utf8")) : readMetadata(),
      fixtures: !rest.includes("--no-fixtures"),
    });
    const failed = [
      ...result.corpusProblems.map((problem) => ({ corpus: problem })),
      ...result.selection.filter((row) => row.violations.length > 0),
      ...result.selectorPlants.filter((row) => row.verdict !== "rejected-as-expected"),
      ...result.fixtures.filter((row) => !row.ok),
    ];
    process.stdout.write(
      `${JSON.stringify(
        {
          corpus: result.manifest.corpus,
          arm,
          repetition,
          ok: result.ok,
          selection: result.selection.length,
          selectorPlants: result.selectorPlants.length,
          fixtures: result.fixtures.flatMap(({ rows }) => rows).length,
          notRun: result.notRun.length,
          unavailable: result.unavailable.length,
          failed,
        },
        null,
        2,
      )}\n`,
    );
    return result.ok ? 0 : 1;
  }
  if (command === "compare") {
    const baseline = flag(rest, "--baseline");
    const candidate = flag(rest, "--candidate");
    if (!baseline || !candidate) {
      process.stderr.write("ci-change-corpus: compare needs --baseline and --candidate\n");
      return 2;
    }
    const comparison = compareRuns(readRun(baseline), readRun(candidate));
    const text = `${JSON.stringify(comparison, null, 2)}\n`;
    const out = flag(rest, "--out");
    if (out) writeFileSync(out, text);
    process.stdout.write(text);
    return comparison.comparable && comparison.candidateViolations.length === 0 ? 0 : 1;
  }
  if (command === "plant") {
    const id = rest[0];
    const { corpus } = loadCorpus(flag(rest, "--corpus") ?? DEFAULT_CORPUS);
    const scenario = corpus.scenarios.find((entry) => entry.id === id && entry.kind === "plant");
    if (!scenario) {
      process.stderr.write(`ci-change-corpus: no plant scenario ${id}\n`);
      return 2;
    }
    const result = replayPlant(scenario, resolve(flag(rest, "--worktree") ?? REPO_ROOT));
    process.stdout.write(`${result.output}\n${JSON.stringify({ id, verdict: result.verdict })}\n`);
    return result.verdict === "failed-as-expected" ? 0 : 1;
  }
  process.stderr.write("ci-change-corpus: expected replay, compare or plant\n");
  return 2;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  process.exitCode = main();
}
