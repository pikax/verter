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

// PG0-AC-OWNER predicates, factored so the twins re-run the exact
// discriminator the clean products pass instead of asserting the mutation.
function assertPg0ContractOnlyReceiver(map) {
  const self = map.receivingAmendments.find((a) => a.receiver === "PG0");
  assert.ok(self, "DX0 ownership map names PG0 as a receiver");
  assert.equal(
    self.productionCapable,
    false,
    "PG0 is a contract-only receiver; PG1+ own workbench production",
  );
}

function assertNativeOnlyClauseHolds(rule) {
  assert.ok(
    rule.nativeOnlyClause.includes("explicitly selected native execution"),
    "native-only clause requires real explicitly selected native execution",
  );
  assert.ok(
    rule.nativeOnlyClause.includes("inactive"),
    "native-only clause forbids the inactive-card presentation",
  );
}

// The DX0 receipt-basis field loop, factored so the session twin re-runs the
// exact discriminator the ratified session passes instead of asserting the
// mutation.
function assertSessionBindsReceiptBasis(s) {
  const bindings = Object.values(s.model).map((g) => g.binding);
  const bindsField = (field) => bindings.some((b) => new RegExp(`\\b${field}\\b`).test(b));
  for (const field of dxReceiptBasis.fields) {
    assert.ok(bindsField(field), `session model must bind receipt-basis field ${field}`);
  }
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
    // PG0-AC-EXPOSURE: the replay case is registered here, joined from DX0.
    assert.equal(row.replayCase, dx.exposure.replayCase, `${row.dxOperationId}: replayCase drift`);
    if (dx.playground.promotionBlocked) {
      assert.equal(
        row.blockedUntilRoute,
        dx.playground.blockedUntil.route,
        `${row.dxOperationId}: blockedUntil route drift`,
      );
      assert.equal(
        row.blockedUntilReplayCase,
        dx.playground.blockedUntil.replayCase,
        `${row.dxOperationId}: blockedUntil replayCase drift`,
      );
    } else {
      // The join is symmetric: a blockedUntil pin on an operation DX0 does
      // not block is a fabricated route, not a joined one.
      for (const field of ["blockedUntilRoute", "blockedUntilReplayCase"]) {
        assert.equal(
          row[field],
          undefined,
          `${row.dxOperationId}: ${field} declared for an operation DX0 does not block`,
        );
      }
    }
    const dxMerge = dx.playground.unlabelledAnalysisMerge;
    if (dxMerge) {
      assert.ok(
        row.unlabelledAnalysisMerge,
        `${row.dxOperationId}: DX0 pins an unlabelledAnalysisMerge that the matrix row must disclose`,
      );
      assert.equal(
        row.unlabelledAnalysisMerge.comparisonRailSelection,
        dxMerge.comparisonRailSelection,
        `${row.dxOperationId}: merge comparisonRailSelection drift`,
      );
      assert.equal(
        row.unlabelledAnalysisMerge.defect,
        dxMerge.defect,
        `${row.dxOperationId}: merge defect drift`,
      );
      assert.deepEqual(
        row.unlabelledAnalysisMerge.evidence,
        dxMerge.evidence,
        `${row.dxOperationId}: merge evidence drift`,
      );
      assert.match(
        row.routeObligation,
        /labelled split/,
        `${row.dxOperationId}: routeObligation must carry the DX0 labelled-split route, not a single TS-worker authority over the live merge`,
      );
      assert.match(
        row.routeObligation,
        /comparisonRail/,
        `${row.dxOperationId}: routeObligation must name the comparisonRail selection`,
      );
    } else {
      assert.equal(
        row.unlabelledAnalysisMerge,
        undefined,
        `${row.dxOperationId}: unlabelledAnalysisMerge declared without a DX0 pin`,
      );
    }
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
      // A missing family's preservation evidence is the DX0 gap evidence for
      // its own operations (or an explicit descriptorGap), never an unrelated
      // live panel passed off as the missing producer's surface.
      const dxEvidence = new Set(
        fam.dxOperations.flatMap((id) => dxOps[id]?.playground?.evidence ?? []),
      );
      for (const p of fam.preservationEvidence) {
        assert.ok(
          dxEvidence.has(p),
          `${fam.id}: preservation evidence ${p} is not DX0 gap evidence for its operations`,
        );
      }
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

test("PG0-AC1 twin: dropping the unlabelledAnalysisMerge pin from a live-merge row fails", () => {
  const m = perturb(matrix, (c) => {
    delete c.operationCoverage.find((r) => r.dxOperationId === "playground.hover")
      .unlabelledAnalysisMerge;
  });
  assert.throws(() => assertMatrixJoinsDX(m), /unlabelledAnalysisMerge/);
});

test("PG0-AC1 twin: a blockedUntil pin declared on an operation DX0 does not block fails", () => {
  const m = perturb(matrix, (c) => {
    const row = c.operationCoverage.find((r) => r.dxOperationId === "playground.rename");
    row.blockedUntilRoute = "PG3";
    row.blockedUntilReplayCase = "replay:playground.rename";
  });
  assert.throws(
    () => assertMatrixJoinsDX(m),
    /blockedUntilRoute declared for an operation DX0 does not block/,
  );
});

test("PG0-AC1 twin: an unlabelledAnalysisMerge pin declared without a DX0 pin fails", () => {
  const m = perturb(matrix, (c) => {
    const row = c.operationCoverage.find((r) => r.dxOperationId === "lsp.hover");
    row.unlabelledAnalysisMerge = structuredClone(
      c.operationCoverage.find((r) => r.dxOperationId === "playground.hover")
        .unlabelledAnalysisMerge,
    );
  });
  assert.throws(() => assertMatrixJoinsDX(m), /unlabelledAnalysisMerge declared without a DX0 pin/);
});

test("PG0-AC1 twin: a single TS-worker authority routeObligation over a live merge fails", () => {
  const m = perturb(matrix, (c) => {
    c.operationCoverage.find((r) => r.dxOperationId === "playground.hover").routeObligation =
      "Monaco language-service adapter over the browser host; engine identity stays the pinned browser typescript worker and is disclosed on the result";
  });
  assert.throws(() => assertMatrixJoinsDX(m), /labelled-split|comparisonRail/);
});

test("PG0-AC1 twin: a row that drops its joined replay case fails", () => {
  const m = perturb(matrix, (c) => {
    delete c.operationCoverage.find((r) => r.dxOperationId === "playground.completion").replayCase;
  });
  assert.throws(() => assertMatrixJoinsDX(m), /replayCase drift/);
});

test("PG0-AC1 twin: a missing family citing an unrelated live panel as preservation evidence fails", () => {
  const m = perturb(matrix, (c) => {
    c.featureFamilies.find((f) => f.id === "workbench.family.typeinfo").preservationEvidence = [
      "packages/playground/src/output/AnalysisPanel.vue",
    ];
  });
  assert.throws(() => assertFamiliesConsistent(m), /is not DX0 gap evidence/);
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
  assertPg0ContractOnlyReceiver(dxOwnership);

  // Twin: flipping PG0 to production-capable fails the same predicate the
  // ratified map passes (the discriminator re-runs, not just the mutation).
  const flipped = perturb(dxOwnership, (c) => {
    c.receivingAmendments.find((a) => a.receiver === "PG0").productionCapable = true;
  });
  assert.throws(
    () => assertPg0ContractOnlyReceiver(flipped),
    /contract-only receiver/,
    "flipped map must fail the contract-only receiver predicate",
  );
});

test("PG0 session model binds the receipt basis verbatim and adds no engine", () => {
  assertSessionBindsReceiptBasis(session);
  assert.ok(session.law.includes("no second semantic"), "session law forbids a second engine");

  // Browser session project access rides the BWH platform-host io service.
  const io = bwhPlatformServices.services.find((s) => s.id === "io");
  assert.ok(io, "BWH platform-host-services io service exists");
  assert.ok(
    session.model.project.obligations.some((o) => o.includes("BWH platform-host io service")),
    "project obligations bind the BWH io service, not a new VFS authority",
  );

  // Twin: a session model dropping the engineIdentity/hostIdentity binding
  // fails the same field loop the ratified session passes.
  const dropped = perturb(session, (c) => {
    c.model.engineProfile.binding = "presentation only";
  });
  assert.throws(
    () => assertSessionBindsReceiptBasis(dropped),
    /receipt-basis field/,
    "twin must fail the receipt-basis field discriminator",
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
  assertNativeOnlyClauseHolds(promotion.rule);
  assert.ok(promotion.rule.futureClause.includes("future"));
  assert.ok(promotion.rule.enforcement.some((e) => e.startsWith("PG0-AC1")));
  // Every receiving amendment is a real PG-train node or DX1.
  for (const a of promotion.receivingAmendments) {
    assert.match(a.receiver, /^(DX1|PG\d+[A-Z]?)$/, `unknown receiver ${a.receiver}`);
    assert.equal(typeof a.productionCapable, "boolean");
  }
  const receivers = promotion.receivingAmendments.map((a) => a.receiver);
  assert.equal(new Set(receivers).size, receivers.length, "duplicate receiver amendment");

  // Twin: removing the native-only companion clause fails the same predicate
  // the ratified rule passes.
  const weakened = perturb(promotion, (c) => {
    c.rule.nativeOnlyClause = "native features show a disabled card";
  });
  assert.throws(
    () => assertNativeOnlyClauseHolds(weakened.rule),
    /explicitly selected native execution/,
    "weakened clause must fail the native-only predicate",
  );
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

// Every live Output.vue tab mode (allTabs plus the ssr insert) must map to a
// family that cites the tab's panel as preservation evidence, so no
// user-visible surface loses its surviving producer at the PG2 cutover.
function assertDisplacedTabsOwned(m, outputVue) {
  const allTabsBlock = outputVue.match(/const allTabs[\s\S]*?\];/);
  assert.ok(allTabsBlock, "the curated allTabs list is where it lives in Output.vue");
  const liveModes = new Set([...allTabsBlock[0].matchAll(/mode:\s*"([^"]+)"/g)].map((mt) => mt[1]));
  assert.match(outputVue, /\{ mode: "ssr", label: "SSR" \}/, "the ssr insert is characterized");
  liveModes.add("ssr");
  assert.deepEqual(
    m.displacedRoute.tabs.map((t) => t.mode).sort(),
    [...liveModes].sort(),
    "displacedRoute.tabs must cover exactly the live Output.vue tab modes (no tab without a family owner)",
  );
  const byFamily = Object.fromEntries(m.featureFamilies.map((f) => [f.id, f.preservationEvidence]));
  for (const t of m.displacedRoute.tabs) {
    assert.ok(byFamily[t.family], `tab ${t.mode}: unknown family ${t.family}`);
    assert.ok(
      fs.existsSync(path.join(REPO_ROOT, t.panel)),
      `tab ${t.mode}: panel missing in live tree: ${t.panel}`,
    );
    assert.ok(
      byFamily[t.family].includes(t.panel),
      `tab ${t.mode}: panel ${t.panel} is not preservation evidence of family ${t.family}`,
    );
  }
}

// Every live App.vue output-nav control (Output | Why?) must map to a
// family-owned panel — or, for the Output shell, to the displacedRoute.tabs
// surface whose modes are owned one-for-one — so the Why?/AuditTree
// provenance view keeps a surviving producer at the PG2 cutover.
function assertDisplacedAppNavOwned(m, appVue) {
  const navControls = [...appVue.matchAll(/<button[^>]*class="output-tab"[\s\S]*?<\/button>/g)].map(
    (mt) => mt[0].replace(/<[^>]*>/g, " ").trim(),
  );
  assert.ok(navControls.length >= 2, "the App.vue Output|Why? nav is where it lives in App.vue");
  assert.deepEqual(
    m.displacedRoute.appNav.controls.map((c) => c.control).sort(),
    [...navControls].sort(),
    "displacedRoute.appNav.controls must cover exactly the live App.vue nav controls (no control without an owner)",
  );
  const byFamily = Object.fromEntries(m.featureFamilies.map((f) => [f.id, f.preservationEvidence]));
  for (const c of m.displacedRoute.appNav.controls) {
    assert.ok(
      fs.existsSync(path.join(REPO_ROOT, c.panel)),
      `control ${c.control}: panel missing in live tree: ${c.panel}`,
    );
    if (c.routesTo) {
      assert.equal(
        c.routesTo,
        "displacedRoute.tabs",
        `control ${c.control}: routesTo must name the displaced tabs surface`,
      );
      assert.match(
        c.panel,
        /Output\.vue$/,
        `control ${c.control}: routes through the Output.vue tab strip characterized by displacedRoute.tabs`,
      );
    } else {
      assert.ok(
        c.family,
        `control ${c.control}: must name a family or route through displacedRoute.tabs`,
      );
      assert.ok(byFamily[c.family], `control ${c.control}: unknown family ${c.family}`);
      assert.ok(
        byFamily[c.family].includes(c.panel),
        `control ${c.control}: panel ${c.panel} is not preservation evidence of family ${c.family}`,
      );
    }
  }
}

test("PG0 displaced tabs: every Output.vue allTabs mode plus the ssr insert has an owning family", () => {
  const outputVue = fs.readFileSync(
    path.join(REPO_ROOT, "packages/playground/src/output/Output.vue"),
    "utf8",
  );
  assertDisplacedTabsOwned(matrix, outputVue);

  // Twin 1: an unowned tab mode (cssMatch detached from the matrix) fails.
  const m1 = perturb(matrix, (c) => {
    c.displacedRoute.tabs = c.displacedRoute.tabs.filter((t) => t.mode !== "cssMatch");
  });
  assert.throws(
    () => assertDisplacedTabsOwned(m1, outputVue),
    /displacedRoute\.tabs must cover exactly/,
  );

  // Twin 2: a tab whose panel is dropped from its owning family's evidence fails.
  const m2 = perturb(matrix, (c) => {
    const fam = c.featureFamilies.find((f) => f.id === "workbench.family.compiler-ir-mappings");
    fam.preservationEvidence = fam.preservationEvidence.filter(
      (p) => p !== "packages/playground/src/output/CssMatchPanel.vue",
    );
  });
  assert.throws(
    () => assertDisplacedTabsOwned(m2, outputVue),
    /is not preservation evidence of family/,
  );
});

test("PG0 displaced nav: the App.vue Output|Why? controls keep an owning family", () => {
  const appVue = fs.readFileSync(path.join(REPO_ROOT, "packages/playground/src/App.vue"), "utf8");
  assertDisplacedAppNavOwned(matrix, appVue);

  // Twin 1: dropping the Why? control from the matrix fails (unexplained UI
  // feature removal: the live nav still renders Why?).
  const m1 = perturb(matrix, (c) => {
    c.displacedRoute.appNav.controls = c.displacedRoute.appNav.controls.filter(
      (ctl) => ctl.control !== "Why?",
    );
  });
  assert.throws(
    () => assertDisplacedAppNavOwned(m1, appVue),
    /displacedRoute\.appNav\.controls must cover exactly/,
  );

  // Twin 2: AuditTree.vue dropped from its owning family's evidence fails.
  const m2 = perturb(matrix, (c) => {
    const fam = c.featureFamilies.find((f) => f.id === "workbench.family.audit-provenance");
    fam.preservationEvidence = fam.preservationEvidence.filter(
      (p) => p !== "packages/playground/src/components/AuditTree.vue",
    );
  });
  assert.throws(
    () => assertDisplacedAppNavOwned(m2, appVue),
    /is not preservation evidence of family/,
  );

  // Twin 3: parking Why?/AuditTree.vue on a missing family fails the family
  // consistency rule (a missing family cites only DX0 gap evidence for its
  // own operations), so the preserved live surface cannot be presented as
  // the missing producer's coverage.
  const m3 = perturb(matrix, (c) => {
    c.displacedRoute.appNav.controls.find((ctl) => ctl.control === "Why?").family =
      "workbench.family.metadata";
    c.featureFamilies
      .find((f) => f.id === "workbench.family.metadata")
      .preservationEvidence.push("packages/playground/src/components/AuditTree.vue");
  });
  assert.throws(() => assertFamiliesConsistent(m3), /is not DX0 gap evidence/);
});
