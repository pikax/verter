import assert from "node:assert/strict";
import test from "node:test";

import {
  ACCEPTED_PRODUCTS,
  DIRTY_OVERLAP,
  DIRTY_STALE,
  DIRTY_SYNTHETIC,
  STP10_MANDATORY_CASES,
  assertCorrespondencePolicy,
  assertNoIndependentMapGenerator,
  assertRustCases,
  cloneJson,
  evaluateRejectTwins,
  evaluateStp10,
  loadStp10Product,
  productionOriginSource,
  validateStp10Products,
} from "./protocol.mjs";

test("STP10 products name the charter mapping contracts", () => {
  const product = loadStp10Product();
  assert.equal(product.schema, "EmissionCorrespondence");
  assert.deepEqual([...product.products].sort(), [...ACCEPTED_PRODUCTS].sort());
  const errors = validateStp10Products(product);
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STP10-overlap/stale-map/synthetic dirty twins are rejected", () => {
  const product = loadStp10Product();
  assert.equal(assertCorrespondencePolicy(product).length, 0);
  const overlap = cloneJson(product);
  Object.assign(overlap.correspondence, DIRTY_OVERLAP);
  assert.ok(
    assertCorrespondencePolicy(overlap).some((error) => error.caseId === "STP10-overlap"),
    JSON.stringify(assertCorrespondencePolicy(overlap)),
  );
  const stale = cloneJson(product);
  Object.assign(stale.correspondence, DIRTY_STALE);
  assert.ok(
    assertCorrespondencePolicy(stale).some((error) => error.caseId === "STP10-stale-map"),
    JSON.stringify(assertCorrespondencePolicy(stale)),
  );
  const synthetic = cloneJson(product);
  Object.assign(synthetic.correspondence, DIRTY_SYNTHETIC);
  assert.ok(
    assertCorrespondencePolicy(synthetic).some((error) => error.caseId === "STP10-synthetic"),
    JSON.stringify(assertCorrespondencePolicy(synthetic)),
  );
  assert.equal(assertNoIndependentMapGenerator(productionOriginSource()).length, 0);
});

test("STP10 dirty twins for overlap, stale-map, synthetic, and independent maps are rejected", () => {
  const errors = evaluateRejectTwins();
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STP10 evaluate joins rust roundtrip/role/overlap/stale/synthetic cases", async () => {
  const result = await evaluateStp10();
  assert.equal(result.errors.length, 0, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(
    [...STP10_MANDATORY_CASES],
    ["STP10-roundtrip", "STP10-role", "STP10-overlap", "STP10-stale-map", "STP10-synthetic"],
  );
});

test("mandatory rust cases fail closed on zero-selection, ignored, and unrelated-only cargo output", () => {
  const zero = assertRustCases({
    status: 0,
    error: null,
    stdout: "test result: ok. 0 passed; 0 failed\n",
    ok: true,
  });
  assert.ok(zero.length > 0, "zero-selection cargo ok must not pass");
  const ignored = assertRustCases({
    status: 0,
    error: null,
    stdout: [
      "test framework_common::projection_plan::origin::tests::stp10_roundtrip_verbatim_unicode_and_crlf ... ok",
      "test framework_common::projection_plan::origin::tests::stp10_role_view_property_and_binding_share_origin_not_symbol ... ok",
      "test framework_common::projection_plan::origin::tests::stp10_overlap_rejects_overlapping_virtual_spans ... ok",
      "test framework_common::projection_plan::origin::tests::stp10_stale_map_rejects_reused_pre_edit_positions ... ok",
      "test result: ok. 4 passed; 0 failed",
    ].join("\n"),
    ok: true,
  });
  assert.ok(
    ignored.some((row) => row.caseId === "STP10-synthetic"),
    JSON.stringify(ignored),
  );
  const unrelated = assertRustCases({
    status: 0,
    error: null,
    stdout: "test some_other_filter ... ok\ntest result: ok. 1 passed; 0 failed\n",
    ok: true,
  });
  assert.equal(unrelated.length, STP10_MANDATORY_CASES.length, JSON.stringify(unrelated));
});
