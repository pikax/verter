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

test("ARH7-ratification: clean products validate and cover every mandatory case", () => {
  const result = validate(clean);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "ARH7-authority",
    "ARH7-cost",
    "ARH7-cutover",
    "ARH7-delivery",
    "ARH7-ratification",
    "ARH7-work",
  ]);
});

test("ARH7-cutover dirty twin: surviving owner retargeted at a missing path is rejected", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].cutover[0].survivingOwner.path =
    "packages/vue-vscode/src/inventedLocator.ts";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH7-cutover" && e.code === "owner-path-missing"),
    JSON.stringify(result.errors),
  );
});

test("ARH7-cutover dirty twin: dropping a cutover row is rejected", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].cutover = dirty["activation-cutover"].cutover.slice(0, 1);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH7-cutover" && e.code === "cutover-cardinality"),
    JSON.stringify(result.errors),
  );
});

test("ARH7-authority dirty twin: dropping the reload discriminator is rejected (AC2)", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].ac2.evidence = dirty["activation-cutover"].ac2.evidence.filter(
    (e) => !e.file.endsWith("activationSession.spec.ts"),
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH7-authority" && e.code === "ac2-evidence-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH7-work dirty twin: dropping stale-generation evidence is rejected (AC3)", () => {
  const dirty = cloneProducts();
  const concern = dirty["activation-cutover"].ac3.concerns.find(
    (c) => c.concern === "stale/partial rejection",
  );
  concern.evidence = [];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH7-work" && e.code === "ac3-concern-without-evidence",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH7-work dirty twin: dropping cancellation evidence is rejected (AC3)", () => {
  const dirty = cloneProducts();
  const concern = dirty["activation-cutover"].ac3.concerns.find(
    (c) => c.concern === "cancellation",
  );
  concern.evidence = [];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH7-work" && e.code === "ac3-concern-without-evidence",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH7-work dirty twin: inventing an AC3 concern is rejected", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].ac3.concerns.push({
    concern: "template codegen",
    applicable: false,
    rationale: "invented",
  });
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH7-work" && e.code === "ac3-concern-missing"),
    JSON.stringify(result.errors),
  );
});

test("ARH7-delivery dirty twin: empty AC4 rationale is rejected", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].ac4Rationale = "";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH7-delivery" && e.code === "ac4-rationale-missing"),
    JSON.stringify(result.errors),
  );
});

test("ARH7-cost dirty twin: committed wall-clock is rejected (AC5)", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].wallNs = 12;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH7-cost" && e.code === "dimension-commits-wall-clock",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH7-ratification dirty twin: invented case is rejected", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.cases.push({
    id: "ARH7-phantom",
    disposition: "reject",
    twins: ["clean products"],
  });
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH7-ratification" && e.code === "manifest-case-drift"),
    JSON.stringify(result.errors),
  );
  assert.ok(selectedCaseIds(result).includes("ARH7-ratification"));
});

test("ARH7-ratification dirty twin: candidate pin drift is rejected", () => {
  const dirty = cloneProducts();
  dirty["activation-cutover"].candidate = "not-a-commit";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH7-ratification" && e.code === "candidate-basis-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH7 CI: architecture-health filter still selects the train home and vscode sources", () => {
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
    `arch filter omits examples/reference/**: ${paths.join(", ")}`,
  );
  assert.ok(
    paths.includes("packages/vue-vscode/**"),
    `arch filter omits packages/vue-vscode/**, the ARH7 live-source join: ${paths.join(", ")}`,
  );
});

test("ARH7-verify CLI: the manifest verify command runs validate() and exits 0", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], { encoding: "utf8" });
  assert.match(stdout, /ARH7 verify: PASS/);
});

test("ARH7-verify CLI: --provenance accepts on the clean tree (CI lane shape)", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const candidate = clean["activation-cutover"].candidate;
  const real = validateProvenance(candidate);
  const carriesCommit = real.ok || !/not a commit/.test(real.reason);
  if (carriesCommit) {
    assert.deepEqual(real, { ok: true }, real.reason);
  }
  const args = carriesCommit ? [verifyPath, "--provenance"] : [verifyPath];
  const stdout = execFileSync(process.execPath, args, { encoding: "utf8" });
  assert.match(stdout, /ARH7 verify: PASS/);
});
