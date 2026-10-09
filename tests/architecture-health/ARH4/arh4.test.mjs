import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { matchPathFilters } from "../../../scripts/ci-impact.mjs";
import { loadProducts as loadArh1Products } from "../ARH1/verify.mjs";
import {
  deriveCallFormPopulation,
  loadManifest,
  loadProducts,
  mandatoryCases,
  validate,
} from "./verify.mjs";

const clean = loadProducts();
const cloneProducts = () => structuredClone(clean);
const arh1 = loadArh1Products();
const cutRow = (products, id) => products["lifecycle-cutover"].cutover.find((r) => r.id === id);

function assertRejects(result, caseId, code) {
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === caseId && e.code === code),
    JSON.stringify(result.errors),
  );
}

test("ARH4-ratification: clean products validate and cover every mandatory case", () => {
  const result = validate(clean);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "ARH4-authority",
    "ARH4-cost",
    "ARH4-cutover",
    "ARH4-delivery",
    "ARH4-ratification",
    "ARH4-work",
  ]);
});

test("ARH4-cutover: the audit drain/publish population is derived from live production source", () => {
  const live = deriveCallFormPopulation(cutRow(clean, "ARH4-CUT-1").callForms);
  assert.ok(live.includes("crates/verter_session/src/audited_request.rs"), JSON.stringify(live));
  assert.ok(
    live.every((rel) => !rel.endsWith("_tests.rs") && !rel.includes("/tests/")),
    JSON.stringify(live),
  );
});

test("ARH4-cutover dirty twin: a dropped inventory concern is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].inventory = dirty["lifecycle-cutover"].inventory.filter(
    (row) => row.concern !== "observation",
  );
  assertRejects(validate(dirty), "ARH4-cutover", "inventory-concern-drift");
});

test("ARH4-cutover dirty twin: a no-residual concern without evidence is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].inventory.find((r) => r.concern === "source management").evidence =
    "none";
  assertRejects(validate(dirty), "ARH4-cutover", "inventory-residual-unevidenced");
});

test("ARH4-cutover dirty twin: an inventory owner that declares no such type is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].inventory.find(
    (r) => r.concern === "audit access",
  ).survivingOwner.typeName = "InventedAuditOwner";
  assertRejects(validate(dirty), "ARH4-cutover", "inventory-owner-missing");
});

test("ARH4-cutover dirty twin: an invented inventory disposition is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].inventory.find((r) => r.concern === "construction").disposition =
    "deferred";
  assertRejects(validate(dirty), "ARH4-cutover", "inventory-disposition-invented");
});

test("ARH4-cutover dirty twin: a cutover row no concern claims is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].inventory.find((r) => r.concern === "audit access").cutover = [
    "ARH4-CUT-1",
  ];
  assertRejects(validate(dirty), "ARH4-cutover", "inventory-narrowing-unbound");
});

test("ARH4-cutover dirty twin: dropping a cutover row is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].cutover = dirty["lifecycle-cutover"].cutover.slice(0, 3);
  assertRejects(validate(dirty), "ARH4-cutover", "cutover-cardinality");
});

test("ARH4-cutover dirty twin: a boundary type the path does not declare is rejected", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].boundary.typeName = "InventedHost";
  assertRejects(validate(dirty), "ARH4-cutover", "boundary-path-missing");
});

test("ARH4-cutover dirty twin: a surviving owner at a missing path is rejected", () => {
  const dirty = cloneProducts();
  cutRow(dirty, "ARH4-CUT-1").survivingOwner.path = "crates/verter_session/src/invented_audit.rs";
  assertRejects(validate(dirty), "ARH4-cutover", "owner-path-missing");
});

test("ARH4-cutover dirty twin: an owner type or method the owner does not declare is rejected", () => {
  const type = cloneProducts();
  cutRow(type, "ARH4-CUT-2").survivingOwner.typeName = "InventedHost";
  assertRejects(validate(type), "ARH4-cutover", "owner-type-missing");

  const method = cloneProducts();
  cutRow(method, "ARH4-CUT-1").survivingOwner.methods.push("insert_record");
  assertRejects(validate(method), "ARH4-cutover", "owner-method-missing");
});

test("ARH4-cutover dirty twin: an audit runtime constructor that takes a store is rejected", () => {
  const dirty = cloneProducts();
  cutRow(dirty, "ARH4-CUT-1").survivingOwner.constructorSignature =
    "pub fn new(config: AuditConfig, records: Arc<AuditRecordsStore>) -> Self";
  assertRejects(validate(dirty), "ARH4-cutover", "owner-constructor-drift");
});

test("ARH4-cutover dirty twin: a recorded consumer that never reaches the owner is rejected", () => {
  const dirty = cloneProducts();
  cutRow(dirty, "ARH4-CUT-2").consumers.push("crates/verter_session/src/host_lifecycle.rs");
  assertRejects(validate(dirty), "ARH4-cutover", "consumer-bypasses-owner");

  const missing = cloneProducts();
  cutRow(missing, "ARH4-CUT-1").consumers.push("crates/verter_session/src/invented_consumer.rs");
  assertRejects(validate(missing), "ARH4-cutover", "consumer-missing");
});

test("ARH4-cutover dirty twin: an omitted live audit consumer is rejected", () => {
  const dirty = cloneProducts();
  cutRow(dirty, "ARH4-CUT-1").consumers = cutRow(dirty, "ARH4-CUT-1").consumers.filter(
    (c) => !c.endsWith("audited_request.rs"),
  );
  assertRejects(validate(dirty), "ARH4-cutover", "consumer-population-drift");
});

test("ARH4-cutover dirty twin: executing a register row another node owns is rejected", () => {
  const dirty = cloneProducts();
  cutRow(dirty, "ARH4-CUT-3").executesRegisterRow = "ARH1-CUT-1";
  assertRejects(validate(dirty), "ARH4-cutover", "register-row-unowned");
});

test("ARH4-cutover dirty twin: a register row whose narrowing is not on the tree is rejected", () => {
  const field = cloneProducts();
  cutRow(field, "ARH4-CUT-3").survivingOwner.visibility = "pub";
  assertRejects(validate(field), "ARH4-cutover", "register-row-unexecuted");

  // A hook the ARH1 contract narrows to test configuration must carry the
  // gate: `remove` is production surface, so binding it as a hook fails.
  const contracts = structuredClone(arh1);
  contracts["dependency-contracts"].hotspots
    .find((h) => h.path === "crates/verter_scheduler/src/scheduler.rs")
    .minimalPublicSurface.narrow.push({
      item: "remove",
      kind: "fn",
      to: "test-configuration",
      referenceForms: [".remove("],
      consumersAffected: [],
    });
  assertRejects(
    validate(clean, loadManifest(), contracts),
    "ARH4-cutover",
    "register-row-unexecuted",
  );
});

test("ARH4-authority dirty twin: dropping the shared key-space discriminator is rejected (AC2)", () => {
  const dirty = cloneProducts();
  dirty["lifecycle-cutover"].ac2.evidence = dirty["lifecycle-cutover"].ac2.evidence.filter(
    (e) => !e.file.endsWith("audit_request_ids_share_one_host_key_space.rs"),
  );
  assertRejects(validate(dirty), "ARH4-authority", "ac2-evidence-population-drift");
});

test("ARH4-authority dirty twin: an unbound or missing AC2 test and an empty defect are rejected", () => {
  const unbound = cloneProducts();
  unbound["lifecycle-cutover"].ac2.evidence[1].test = "invented_key_space_test";
  assertRejects(validate(unbound), "ARH4-authority", "ac2-evidence-unbound");

  const missing = cloneProducts();
  missing["lifecycle-cutover"].ac2.evidence[0].file =
    "crates/verter_session/tests/cases/g_audit/invented.rs";
  assertRejects(validate(missing), "ARH4-authority", "ac2-evidence-missing");

  const defect = cloneProducts();
  defect["lifecycle-cutover"].ac2.defect = "";
  assertRejects(validate(defect), "ARH4-authority", "ac2-defect-missing");
});

test("ARH4-work dirty twins: AC3 concerns need evidence or a rationale (AC3)", () => {
  const concern = (p, name) => p["lifecycle-cutover"].ac3.concerns.find((c) => c.concern === name);

  const noEvidence = cloneProducts();
  concern(noEvidence, "cancellation").evidence = [];
  assertRejects(validate(noEvidence), "ARH4-work", "ac3-concern-without-evidence");

  const notATest = cloneProducts();
  concern(
    notATest,
    "deterministic ordering under perturbed discovery or scheduling",
  ).evidence[0].test = "invented_ordering_test";
  assertRejects(validate(notATest), "ARH4-work", "ac3-evidence-not-a-test");

  const missingFile = cloneProducts();
  concern(missingFile, "cancellation").evidence[0].file =
    "crates/verter_session/tests/cases/g_audit/invented.rs";
  assertRejects(validate(missingFile), "ARH4-work", "ac3-evidence-missing");

  const noRationale = cloneProducts();
  concern(noRationale, "edit/revert").rationale = "";
  assertRejects(validate(noRationale), "ARH4-work", "ac3-na-rationale-missing");

  const invented = cloneProducts();
  concern(invented, "stale/partial rejection").applicable = "maybe";
  assertRejects(validate(invented), "ARH4-work", "ac3-concern-invented");

  const dropped = cloneProducts();
  dropped["lifecycle-cutover"].ac3.concerns.pop();
  assertRejects(validate(dropped), "ARH4-work", "ac3-concern-missing");
});

test("ARH4-delivery dirty twins: migration notes must be documented in the API reference", () => {
  const undocumented = cloneProducts();
  undocumented["lifecycle-cutover"].migration[0].to = "host_audit_runtime().drain(request_id)";
  assertRejects(validate(undocumented), "ARH4-delivery", "migration-note-undocumented");

  const none = cloneProducts();
  none["lifecycle-cutover"].migration = [];
  assertRejects(validate(none), "ARH4-delivery", "migration-note-missing");

  const rationale = cloneProducts();
  rationale["lifecycle-cutover"].ac4Rationale = "n/a";
  assertRejects(validate(rationale), "ARH4-delivery", "ac4-rationale-missing");
});

test("ARH4-cost dirty twins: the arh4-perf disposition must match its manifest", () => {
  const applicable = cloneProducts();
  applicable["lifecycle-cutover"].evidenceRun.applicable = true;
  assertRejects(validate(applicable), "ARH4-cost", "evidence-run-manifest-drift");

  const undecided = cloneProducts();
  delete undecided["lifecycle-cutover"].evidenceRun.applicable;
  assertRejects(validate(undecided), "ARH4-cost", "evidence-run-undecided");

  const rationale = cloneProducts();
  rationale["lifecycle-cutover"].ac5Rationale = "cheap";
  assertRejects(validate(rationale), "ARH4-cost", "ac5-rationale-missing");

  const wallClock = cloneProducts();
  wallClock["lifecycle-cutover"].evidenceRun.wallNs = 1;
  assertRejects(validate(wallClock), "ARH4-cost", "dimension-commits-wall-clock");
});

test("ARH4-ratification dirty twins: manifest and product identity drift are rejected", () => {
  const manifest = structuredClone(loadManifest());
  manifest.cases = manifest.cases.slice(1);
  assertRejects(validate(clean, manifest), "ARH4-ratification", "manifest-case-drift");

  const products = structuredClone(loadManifest());
  products.products = ["InventedCutover"];
  assertRejects(validate(clean, products), "ARH4-ratification", "manifest-product-drift");

  const command = structuredClone(loadManifest());
  command.verify = "node tests/architecture-health/ARH4/invented.mjs";
  assertRejects(validate(clean, command), "ARH4-ratification", "manifest-command-drift");

  const schema = cloneProducts();
  schema["lifecycle-cutover"].schema = "InventedLifecycleCutover";
  assertRejects(validate(schema), "ARH4-ratification", "product-schema-drift");
});

test("ARH4 CI: the architecture-health filter selects the API reference the delivery check reads", () => {
  for (const input of [
    "tests/architecture-health/ARH4/verify.mjs",
    "crates/verter_session/src/lib.rs",
    "docs/audit-footprint/api-reference.md",
  ]) {
    assert.equal(matchPathFilters([input]).hits.arch, true, `arch filter omits ${input}`);
  }
});

test("ARH4-verify CLI: the manifest verify command runs validate() and exits 0", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], { encoding: "utf8" });
  assert.match(stdout, /ARH4 verify: PASS/);
});
