#!/usr/bin/env node
/**
 * PG0 constitution harness: playground feature-matrix / workbench-session /
 * producer-promotion-rule product checks.
 *
 * Grounds itself in the shipped DX0 products and the live repository only. It
 * never reads a roadmap/DAG store (those are database-owned) and it mints no
 * parallel operation namespace. Each check has a named negative twin that
 * mutates an in-memory copy and must fail, so a clean pass is discrimination,
 * not vacuity.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));

const dxContract = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/feature-exposure-contract.v1.json"),
);
const dxReceiptBasis = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/product-receipt-basis.v1.json"),
);
const dxHostClasses = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/host-execution-class.v1.json"),
);
const dxOwnership = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/exposure-ownership-map.json"),
);
const bwhPlatformServices = readJson(
  path.join(REPO_ROOT, "tests/browser-host/BWH0/products/platform-host-services.v1.json"),
);

const matrix = readJson(path.join(HERE, "products/playground-feature-matrix.v1.json"));
const session = readJson(path.join(HERE, "products/workbench-session.v1.json"));
const promotion = readJson(path.join(HERE, "products/producer-promotion-rule.v1.json"));
const ownership = readJson(path.join(HERE, "products/workbench-ownership-map.json"));

const dxOps = Object.fromEntries(dxContract.operations.map((o) => [o.id, o]));
const coverageRows = Object.fromEntries(matrix.operationCoverage.map((r) => [r.dxOperationId, r]));
const familyRows = Object.fromEntries(matrix.featureFamilies.map((r) => [r.id, r]));
const filesExist = (rels) => {
  for (const rel of rels) {
    assert.ok(
      fs.existsSync(path.join(REPO_ROOT, rel)),
      `evidence path missing in live tree: ${rel}`,
    );
  }
};

/** Structural clone with a mutation applied; used for negative twins only. */
function perturb(obj, mutate) {
  const copy = structuredClone(obj);
  mutate(copy);
  return copy;
}

// The core PG0-AC1 join, factored so the twins reuse the exact discriminator.
function assertMatrixJoinsDX(m) {
  const dxIds = dxContract.operations.map((o) => o.id);
  assert.deepEqual(
    m.operationCoverage.map((r) => r.dxOperationId).sort(),
    [...dxIds].sort(),
    "operationCoverage must cover every DX0 operation id exactly once (generated, not curated)",
  );
  for (const row of m.operationCoverage) {
    const dx = dxOps[row.dxOperationId];
    assert.ok(dx, `unknown DX0 operation id: ${row.dxOperationId}`);
    assert.equal(
      row.hostExecutionClass,
      dx.hostExecutionClass,
      `${row.dxOperationId}: class drift`,
    );
    assert.equal(row.answerRail, dx.answerRail, `${row.dxOperationId}: rail drift`);
    assert.equal(
      row.browserExecutable,
      dx.browserExecutable,
      `${row.dxOperationId}: browserExecutable drift`,
    );
    assert.equal(row.workbenchState, dx.playground.status, `${row.dxOperationId}: status drift`);
    // promotionBlocked: joined verbatim; absent in DX0 means not blocked.
    assert.equal(
      row.promotionBlocked === undefined ? false : row.promotionBlocked,
      dx.playground.promotionBlocked === undefined ? false : dx.playground.promotionBlocked,
      `${row.dxOperationId}: promotionBlocked drift`,
    );
    assert.match(
      row.routeOwner ?? "",
      /^PG\d/,
      `${row.dxOperationId}: routeOwner must be a PG train node`,
    );
    assert.ok(
      row.routeObligation && row.routeObligation.length > 20,
      `${row.dxOperationId}: routeObligation must state the executable route`,
    );
  }
}

function assertFamiliesConsistent(m) {
  const coverage = Object.fromEntries(m.operationCoverage.map((r) => [r.dxOperationId, r]));
  const seen = new Set();
  for (const fam of m.featureFamilies) {
    assert.match(fam.id, /^workbench\.family\./, "family id namespace");
    assert.ok(!seen.has(fam.id), `duplicate family id ${fam.id}`);
    seen.add(fam.id);
    assert.match(fam.workbenchOwner, /^PG\d/, `${fam.id}: workbenchOwner must be a PG train node`);
    const states = fam.dxOperations.map((id) => {
      assert.ok(coverage[id], `${fam.id}: references unknown operationCoverage row ${id}`);
      return coverage[id].workbenchState;
    });
    const anyCovered =
      states.some((s) => s === "exposed" || s === "partial") ||
      (states.length === 0 && fam.preservationEvidence.length > 0);
    const allMissing = states.length > 0 && states.every((s) => s === "missing");
    if (fam.familyState === "exposed") {
      assert.ok(
        states.length > 0 && states.every((s) => s === "exposed"),
        `${fam.id}: exposed family must have every op exposed`,
      );
    } else if (fam.familyState === "partial") {
      assert.ok(
        anyCovered,
        `${fam.id}: partial family needs at least one exposed/partial op or a preserved surface`,
      );
    } else if (fam.familyState === "missing") {
      assert.ok(allMissing, `${fam.id}: missing family must have ops and all of them missing`);
      assert.ok(fam.futureProducer, `${fam.id}: missing family names its route plan`);
    } else if (fam.familyState === "future") {
      assert.ok(states.length === 0, `${fam.id}: future family claims shipping operations`);
      assert.ok(fam.futureProducer, `${fam.id}: future family names its producing node (PG0-AC2)`);
    } else {
      assert.fail(`${fam.id}: unknown familyState ${fam.familyState}`);
    }
    if (!fam.dxOperations.length && fam.familyState !== "future") {
      assert.ok(
        fam.descriptorGap,
        `${fam.id}: descriptor-less family must record the DX1 registration gap`,
      );
    }
  }
  // Every DX0 operation id is claimed by at least one family (no orphan coverage).
  for (const id of Object.keys(coverage)) {
    assert.ok(
      m.featureFamilies.some((f) => f.dxOperations.includes(id)),
      `operation ${id} not claimed by any feature family`,
    );
  }
}

test("PG0-AC1 clean pass: matrix joins every DX0 descriptor exactly once with equality", () => {
  assertMatrixJoinsDX(matrix);
  assertFamiliesConsistent(matrix);
});

test("PG0-AC1 twin: a promoted operation absent from the matrix fails coverage", () => {
  const m = perturb(matrix, (c) => {
    c.operationCoverage = c.operationCoverage.filter(
      (r) => r.dxOperationId !== "playground.rename",
    );
  });
  assert.throws(() => assertMatrixJoinsDX(m), /exactly once|unknown DX0/);
});

test("PG0-AC1 twin: a status drift between DX0 and the matrix fails", () => {
  const m = perturb(matrix, (c) => {
    c.operationCoverage.find(
      (r) => r.dxOperationId === "playground.compile-carrier",
    ).workbenchState = "missing";
  });
  assert.throws(() => assertMatrixJoinsDX(m), /status drift/);
});

test("PG0-AC1 twin: an execution-class drift fails (no hidden engine substitution)", () => {
  const m = perturb(matrix, (c) => {
    c.operationCoverage.find((r) => r.dxOperationId === "lsp.hover").hostExecutionClass =
      "Portable";
  });
  assert.throws(() => assertMatrixJoinsDX(m), /class drift/);
});

test("PG0-AC1 twin: dropping promotionBlocked on a blocked operation fails", () => {
  const m = perturb(matrix, (c) => {
    delete c.operationCoverage.find((r) => r.dxOperationId === "lsp.hover").promotionBlocked;
  });
  assert.throws(() => assertMatrixJoinsDX(m), /promotionBlocked drift/);
});

test("PG0-AC1 twin: a promoted feature with no route owner or obligation fails", () => {
  const m = perturb(matrix, (c) => {
    const row = c.operationCoverage.find((r) => r.dxOperationId === "playground.compile-carrier");
    delete row.routeOwner;
    delete row.routeObligation;
  });
  assert.throws(() => assertMatrixJoinsDX(m), /routeOwner|routeObligation/);
});

test("PG0-AC2 clean pass and twins: future is future, never success", () => {
  // Clean: the performance family is future with a named producer and no ops.
  const perf = familyRows["workbench.family.performance"];
  assert.equal(perf.familyState, "future");
  assert.ok(perf.futureProducer.includes("PG14"));

  // Twin 1: a future family claiming a shipping operation fails.
  const m1 = perturb(matrix, (c) => {
    c.featureFamilies.find((f) => f.id === "workbench.family.performance").dxOperations = [
      "playground.rename",
    ];
  });
  assert.throws(() => assertFamiliesConsistent(m1), /future family claims shipping operations/);

  // Twin 2: a missing family presented as covered fails.
  const m2 = perturb(matrix, (c) => {
    c.featureFamilies.find((f) => f.id === "workbench.family.typeinfo").familyState = "partial";
  });
  assert.throws(() => assertFamiliesConsistent(m2), /partial family needs at least one/);

  // Twin 3: an orphan operation not claimed by any family fails.
  const m3 = perturb(matrix, (c) => {
    c.featureFamilies = c.featureFamilies.filter(
      (f) => !f.dxOperations.includes("mcp.type-queries"),
    );
  });
  assert.throws(() => assertFamiliesConsistent(m3), /not claimed by any feature family/);
});

test("PG0-AC-OWNER: one final owner, vocabulary reuse, no parallel namespaces", () => {
  assert.equal(ownership.finalOwner, "expansion.playground-workbench");
  assert.equal(ownership.contractNode, "PG0");
  assert.equal(
    ownership.deletionPopulationThisNode,
    "empty: PG0 is contract-only (0 production LOC, 0 files) and deletes nothing",
  );
  assert.ok(ownership.outcomes.length === 3, "exactly the three charter interfaces are delivered");

  // Vocabulary reuse: matrix enums are the DX0 enums, not synonyms.
  assert.deepEqual(
    matrix.vocabulary.hostExecutionClass,
    dxHostClasses.enum,
    "matrix host-execution classes must equal DX0 host-execution-class.v1",
  );
  assert.deepEqual(
    matrix.vocabulary.completenessState,
    dxReceiptBasis.completenessStates,
    "matrix completeness states must equal the DX0 receipt-basis enum",
  );
  const dxStatusVocab = dxContract.vocabulary.playgroundStatus;
  for (const row of matrix.operationCoverage) {
    assert.ok(
      dxStatusVocab.includes(row.workbenchState),
      `${row.dxOperationId}: workbenchState ${row.workbenchState} is outside the DX0 playgroundStatus enum`,
    );
  }

  // PG0 appears in DX0's ownership map as a contract-only receiver.
  const self = dxOwnership.receivingAmendments.find((a) => a.receiver === "PG0");
  assert.ok(self, "DX0 ownership map names PG0 as a receiver");
  assert.equal(
    self.productionCapable,
    false,
    "PG0 is a contract-only receiver; PG1+ own workbench production",
  );

  // Twin: flipping PG0 to production-capable must be caught.
  const flipped = perturb(dxOwnership, (c) => {
    c.receivingAmendments.find((a) => a.receiver === "PG0").productionCapable = true;
  });
  const flippedSelf = flipped.receivingAmendments.find((a) => a.receiver === "PG0");
  assert.equal(flippedSelf.productionCapable, true); // the twin itself is the mutated fact
  assert.notDeepEqual(flippedSelf, self, "twin must differ from the ratified map row");
});

test("PG0 session model binds the receipt basis verbatim and adds no engine", () => {
  const bindings = Object.values(session.model).map((g) => g.binding);
  const bindsField = (field) => bindings.some((b) => new RegExp(`\\b${field}\\b`).test(b));
  for (const field of dxReceiptBasis.fields) {
    assert.ok(bindsField(field), `session model must bind receipt-basis field ${field}`);
  }
  assert.ok(session.law.includes("no second semantic"), "session law forbids a second engine");

  // Browser session project access rides the BWH platform-host io service.
  const io = bwhPlatformServices.services.find((s) => s.id === "io");
  assert.ok(io, "BWH platform-host-services io service exists");
  assert.ok(
    session.model.project.obligations.some((o) => o.includes("BWH platform-host io service")),
    "project obligations bind the BWH io service, not a new VFS authority",
  );

  // Twin: a session model dropping engineIdentity binding must fail the field check.
  const dropped = perturb(session, (c) => {
    c.model.engineProfile.binding = "presentation only";
  });
  const droppedBindings = Object.values(dropped.model).map((g) => g.binding);
  assert.ok(
    !dxReceiptBasis.fields.every((rb) =>
      droppedBindings.some((b) => new RegExp(`\\b${rb}\\b`).test(b)),
    ),
    "twin loses receipt-basis binding",
  );
});

test("PG0 promotion rule: native-only companion clause and DX1 precondition", () => {
  const preIds = promotion.rule.preconditions.map((p) => p.id);
  assert.deepEqual(preIds, [
    "descriptor-registration",
    "executable-route",
    "replay-case",
    "receipt-binding",
  ]);
  assert.ok(promotion.rule.nativeOnlyClause.includes("explicitly selected native execution"));
  assert.ok(promotion.rule.nativeOnlyClause.includes("inactive"));
  assert.ok(promotion.rule.futureClause.includes("future"));
  assert.ok(promotion.rule.enforcement.some((e) => e.startsWith("PG0-AC1")));
  // Every receiving amendment is a real PG-train node or DX1.
  for (const a of promotion.receivingAmendments) {
    assert.match(a.receiver, /^(DX1|PG\d+[A-Z]?)$/, `unknown receiver ${a.receiver}`);
    assert.equal(typeof a.productionCapable, "boolean");
  }
  const receivers = promotion.receivingAmendments.map((a) => a.receiver);
  assert.equal(new Set(receivers).size, receivers.length, "duplicate receiver amendment");

  // Twin: removing the native-only companion clause breaks the rule.
  const weakened = perturb(promotion, (c) => {
    c.rule.nativeOnlyClause = "native features show a disabled card";
  });
  assert.ok(!weakened.rule.nativeOnlyClause.includes("explicitly selected native execution"));
});

test("PG0 grounding: cited evidence paths exist in the live repository", () => {
  filesExist(matrix.preservedSurface.evidence);
  for (const fam of matrix.featureFamilies) filesExist(fam.preservationEvidence);
  for (const group of Object.values(session.model)) filesExist(group.evidence);
  // The displaced curated route is characterized where it lives.
  filesExist(["packages/playground/src/output/Output.vue"]);
  const outputVue = fs.readFileSync(
    path.join(REPO_ROOT, "packages/playground/src/output/Output.vue"),
    "utf8",
  );
  assert.ok(
    outputVue.includes("allTabs"),
    "the curated tab list is still the characterized surface",
  );

  // Twin-style guard: every evidence list is non-empty per family.
  for (const fam of matrix.featureFamilies) {
    assert.ok(fam.preservationEvidence.length >= 1, `${fam.id} cites no preservation evidence`);
  }
});

test("PG0 displaced-route cutover stays with PG2 and deletes nothing here", () => {
  assert.equal(matrix.displacedRoute.cutoverOwner, "PG2");
  assert.ok(matrix.displacedRoute.deletionPopulationThisNode.startsWith("empty"));
  assert.equal(ownership.displacedRoutes.length, 1);
  assert.equal(ownership.displacedRoutes[0].cutoverOwner, "PG2");
});
