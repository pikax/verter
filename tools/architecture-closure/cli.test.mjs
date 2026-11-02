import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { resolveScriptTests } from "../../scripts/run-script-tests.mjs";
import { fixturePath, loadFixture } from "./test-support.mjs";

const ENTRY = fileURLToPath(new URL("../validate-architecture-closure.mjs", import.meta.url));
const HERE = fileURLToPath(new URL(".", import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../..");

function run(args, cwd = REPO_ROOT) {
  return spawnSync(process.execPath, [ENTRY, ...args], { cwd, encoding: "utf8" });
}

const fixtureArgs = (mode) => [
  "--mode",
  mode,
  "--plan",
  fileURLToPath(fixturePath(`plan-${mode}.json`)),
  "--report",
  fileURLToPath(fixturePath(`report-${mode}.json`)),
];

for (const mode of ["closure", "owner-lock"]) {
  test(`the entry point accepts the ${mode} fixture and prints its decision`, () => {
    const result = run(fixtureArgs(mode));
    assert.equal(result.status, 0, result.stderr);
    const decision = JSON.parse(result.stdout);
    assert.equal(decision.ok, true);
    assert.equal(decision.mode, mode);
  });
}

test("the entry point lists pending later owners in owner-lock mode", () => {
  const decision = JSON.parse(run(fixtureArgs("owner-lock")).stdout);
  assert.ok(decision.pending.some((row) => row.slug === "P-PERF" && row.status === "pending"));
});

test("the entry point exits 1 with reason codes on a rejected report", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "architecture-closure-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const report = loadFixture("report-closure.json");
  report.owners = report.owners.filter((row) => row.slug !== "S-OBS");
  const reportFile = path.join(dir, "report.json");
  writeFileSync(reportFile, JSON.stringify(report));
  const result = run([
    "--mode",
    "closure",
    "--plan",
    fileURLToPath(fixturePath("plan-closure.json")),
    "--report",
    reportFile,
  ]);
  assert.equal(result.status, 1, result.stderr);
  const decision = JSON.parse(result.stdout);
  assert.deepEqual(
    decision.errors.map((error) => [error.code, error.subject]),
    [["owner-row-missing", "S-OBS"]],
  );
});

test("the answer depends only on the supplied inputs, not on the working directory", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "architecture-closure-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const here = run(fixtureArgs("closure"), dir);
  assert.equal(here.status, 0, here.stderr);
  assert.deepEqual(JSON.parse(here.stdout), JSON.parse(run(fixtureArgs("closure")).stdout));
});

const USAGE_FAILURES = [
  ["no arguments", []],
  ["an unknown mode", ["--mode", "partial", "--plan", "p", "--report", "r"]],
  ["no plan", ["--mode", "closure", "--report", "r"]],
  ["no report", ["--mode", "closure", "--plan", "p"]],
  ["an unknown flag", ["--mode", "closure", "--plan", "p", "--report", "r", "--live"]],
  [
    "unreadable inputs",
    ["--mode", "closure", "--plan", "absent-plan", "--report", "absent-report"],
  ],
];

for (const [name, args] of USAGE_FAILURES) {
  test(`the entry point exits 2 and makes no decision on ${name}`, () => {
    const result = run(args);
    assert.equal(result.status, 2, result.stdout);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /usage: /);
  });
}

test("invalid JSON input is a usage failure, not a decision", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "architecture-closure-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const planFile = path.join(dir, "plan.json");
  writeFileSync(planFile, "{ not json");
  const result = run([
    "--mode",
    "closure",
    "--plan",
    planFile,
    "--report",
    fileURLToPath(fixturePath("report-closure.json")),
  ]);
  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
});

test("a malformed export element is a decision with its reason code, not a crash", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "architecture-closure-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const plan = JSON.parse(readFileSync(fixturePath("plan-closure.json"), "utf8"));
  plan.closure.metricCells.push(null);
  const planFile = path.join(dir, "plan.json");
  writeFileSync(planFile, JSON.stringify(plan));
  const result = run([
    "--mode",
    "closure",
    "--plan",
    planFile,
    "--report",
    fileURLToPath(fixturePath("report-closure.json")),
  ]);
  assert.equal(result.status, 1, result.stderr);
  const decision = JSON.parse(result.stdout);
  assert.deepEqual(
    decision.errors.map((error) => [error.code, error.subject]),
    [["plan-field-missing", `closure.metricCells[${plan.closure.metricCells.length - 1}]`]],
  );
});

test("every test file here is registered once through its script-test fragment", () => {
  const onDisk = readdirSync(HERE)
    .filter((name) => name.endsWith(".test.mjs"))
    .map((name) => `tools/architecture-closure/${name}`)
    .sort();
  const fragment = JSON.parse(
    readFileSync(path.join(REPO_ROOT, "scripts/test-scripts.d/architecture-closure.json"), "utf8"),
  );
  assert.deepEqual(fragment.tests.map((entry) => entry.file).sort(), onDisk);
  for (const entry of fragment.tests) {
    assert.equal(entry.runner, "node-test");
    assert.equal(entry.ci, true);
  }
  for (const ci of [false, true]) {
    const commands = resolveScriptTests({ ci }).map((entry) => entry.display);
    for (const file of onDisk) {
      assert.equal(
        commands.filter((command) => command.includes(file)).length,
        1,
        `${file} in the ${ci ? "ci" : "full"} lane`,
      );
    }
  }
});
