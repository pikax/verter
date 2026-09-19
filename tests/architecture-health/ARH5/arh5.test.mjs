import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  loadManifest,
  loadProducts,
  mandatoryCases,
  selectedCaseIds,
  validate,
  validateProvenance,
} from "./verify.mjs";

const clean = loadProducts();
const cloneProducts = () => structuredClone(clean);

test("ARH5-ratification: clean products validate and cover every mandatory case", () => {
  const result = validate(clean);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "ARH5-authority",
    "ARH5-cost",
    "ARH5-cutover",
    "ARH5-delivery",
    "ARH5-ratification",
    "ARH5-work",
  ]);
});

test("ARH5-cutover dirty twin: parse listed as displaced is rejected (AC1)", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].boundary.displacedProductionMethods.push("parse");
  dirty["facade-cutover"].boundary.retainedProductionMethods = dirty[
    "facade-cutover"
  ].boundary.retainedProductionMethods.filter((n) => n !== "parse");
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH5-cutover" && e.code === "displaced-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH5-cutover dirty twin: dropping compile_bundle is rejected (AC1)", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].boundary.displacedProductionMethods = ["compile_ide"];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH5-cutover" && e.code === "displaced-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH5-cutover dirty twin: surviving owner retargeted at a missing type is rejected", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].cutover[0].survivingOwner.typeName = "InventedBackend";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH5-cutover" && e.code === "owner-type-missing"),
    JSON.stringify(result.errors),
  );
});

test("ARH5-cutover dirty twin: stealing a CMP1 adapter is rejected", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].retainedAdapters[0].symbols = ["InventedOptions"];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH5-cutover" && e.code === "retained-adapter-stolen"),
    JSON.stringify(result.errors),
  );
});

test("ARH5-authority dirty twin: dropping the merge-authority compile-fail is rejected (AC2)", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].ac2.evidence = dirty["facade-cutover"].ac2.evidence.filter(
    (e) => !e.file.endsWith("frontend_only_has_no_runtime_accessor.rs"),
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH5-authority" && e.code === "ac2-evidence-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH5-work dirty twin: dropping stale/partial evidence is rejected (AC3)", () => {
  const dirty = cloneProducts();
  const concern = dirty["facade-cutover"].ac3.concerns.find(
    (c) => c.concern === "stale/partial rejection",
  );
  concern.evidence = [];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH5-work" && e.code === "ac3-concern-without-evidence",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH5-work dirty twin: inventing an AC3 concern is rejected", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].ac3.concerns.push({
    concern: "template codegen",
    applicable: false,
    rationale: "invented",
  });
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH5-work" && e.code === "ac3-concern-missing"),
    JSON.stringify(result.errors),
  );
});

test("ARH5-delivery dirty twin: empty AC4 rationale is rejected", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].ac4Rationale = "";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH5-delivery" && e.code === "ac4-rationale-missing"),
    JSON.stringify(result.errors),
  );
});

test("ARH5-cost dirty twin: committed wall-clock is rejected (AC5)", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].wallNs = 12;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH5-cost" && e.code === "dimension-commits-wall-clock",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH5-ratification dirty twin: invented case is rejected", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.cases.push({
    id: "ARH5-phantom",
    disposition: "reject",
    twins: ["clean products"],
  });
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH5-ratification" && e.code === "manifest-case-drift"),
    JSON.stringify(result.errors),
  );
  assert.ok(selectedCaseIds(result).includes("ARH5-ratification"));
});

test("ARH5-ratification dirty twin: candidate pin drift is rejected", () => {
  const dirty = cloneProducts();
  dirty["facade-cutover"].candidate = "not-a-commit";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH5-ratification" && e.code === "candidate-basis-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH5 CI: architecture-health filter still selects the train home", () => {
  const ci = fs.readFileSync(new URL("../../../.github/workflows/ci.yml", import.meta.url), "utf8");
  const start = ci.indexOf("\n            arch:\n");
  assert.notEqual(start, -1, "ci.yml must declare the arch filter");
  const rest = ci.slice(start + 1);
  const next = rest.search(/\n            [a-z_]+:\n/);
  const block = next === -1 ? rest : rest.slice(0, next);
  const paths = [...block.matchAll(/- '([^']+)'/g)].map((m) => m[1]);
  assert.ok(
    paths.includes("tests/architecture-health/**"),
    `arch filter omits tests/architecture-health/**: ${paths.join(", ")}`,
  );
  assert.ok(
    paths.includes("examples/reference/**"),
    `arch filter omits examples/reference/**, the ARH5 public-example guard input: ${paths.join(", ")}`,
  );
});

test("ARH5-verify CLI: the manifest verify command runs validate() and exits 0", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], { encoding: "utf8" });
  assert.match(stdout, /ARH5 verify: PASS/);
});

test("ARH5-verify CLI: --provenance accepts on the clean tree (CI lane shape)", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const candidate = clean["facade-cutover"].candidate;
  const real = validateProvenance(candidate);
  const carriesCommit = real.ok || !/not a commit/.test(real.reason);
  if (carriesCommit) {
    assert.deepEqual(real, { ok: true }, real.reason);
  }
  const args = carriesCommit ? [verifyPath, "--provenance"] : [verifyPath];
  const stdout = execFileSync(process.execPath, args, { encoding: "utf8" });
  assert.match(stdout, /ARH5 verify: PASS/);
});
