import assert from "node:assert/strict";
import test from "node:test";

import {
  CLEAN_ALIAS,
  CLEAN_DEFINITION,
  CLEAN_GUARD,
  CLEAN_STOCK_CLI,
  CLEAN_TRANSFORM,
  DIRTY_ALIAS_EDIT,
  DIRTY_DORMANT_CLAIM,
  DIRTY_GUARD_DUPLICATE,
  DIRTY_RAW_CLI,
  DIRTY_STALE_TARGET,
  DIRTY_VERSION_LABEL_CLAIM,
  ENCODING_FIXTURE,
  POSITION_ENCODINGS,
  assertCleanGeometry,
  assertEncodingIdentity,
  assertFeatureMaskDoesNotSuppressDiagnostics,
  assertNonOverlappingVirtualSpans,
  capabilityRowsFromProbe,
  createMapperSession,
  evaluateCapabilityRows,
  evaluateRejectTwins,
  evaluateStp5,
  helpTextHasMapperHost,
  originalToVirtual,
  spanForToken,
  validateCapabilityClaim,
  validateCliAttribution,
  validateDefinitionTarget,
  validateDiagnosticDirectives,
  validateEdit,
} from "./protocol.mjs";

test("STP5-encoding: emoji and CRLF map identically under UTF-8 and UTF-16", () => {
  const errors = assertEncodingIdentity(ENCODING_FIXTURE);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  const emoji = spanForToken(ENCODING_FIXTURE, "😀");
  assert.equal(emoji.utf8.length, 4);
  assert.equal(emoji.utf16.length, 2);
  const crlf = spanForToken(ENCODING_FIXTURE, "\r\n");
  assert.equal(crlf.utf8.length, 2);
  assert.equal(crlf.utf16.length, 2);
});

test("STP5-encoding dirty twin: dropping CRLF from the fixture is rejected", () => {
  const errors = assertEncodingIdentity('const emoji = "😀";\n');
  assert.ok(errors.some((error) => error.caseId === "STP5-encoding"));
});

test("STP5-guard-duplicate: invalid repeated guards are rejected; one valid directive is accepted", () => {
  assert.equal(
    validateDiagnosticDirectives(CLEAN_GUARD.directives, CLEAN_GUARD.original).length,
    0,
  );
  const dirty = validateDiagnosticDirectives(
    DIRTY_GUARD_DUPLICATE.directives,
    DIRTY_GUARD_DUPLICATE.original,
  );
  assert.ok(dirty.some((error) => error.caseId === "STP5-guard-duplicate"));
});

test("STP5-guard-duplicate dirty twin: feature-mask diagnostic suppression is rejected", () => {
  const dirty = assertFeatureMaskDoesNotSuppressDiagnostics({
    featureMaskSuppressesDiagnostics: true,
    diagnosticsDroppedBecause: "feature-mask",
  });
  assert.ok(dirty.some((error) => error.code === "feature-mask-suppression"));
});

test("STP5-alias-edit: Alias is not a kebab/Pascal rename codec; Verbatim edits stay length-preserving", () => {
  assert.equal(validateEdit(CLEAN_ALIAS.mapping, CLEAN_ALIAS.edit).length, 0);
  const dirty = validateEdit(DIRTY_ALIAS_EDIT.mapping, DIRTY_ALIAS_EDIT.edit);
  assert.ok(dirty.some((error) => error.caseId === "STP5-alias-edit"));
});

test("STP5-stale-target: a foreign definition must use the target file snapshot", () => {
  assert.equal(validateDefinitionTarget(CLEAN_DEFINITION.query, CLEAN_DEFINITION.target).length, 0);
  const dirty = validateDefinitionTarget(DIRTY_STALE_TARGET.query, DIRTY_STALE_TARGET.target);
  assert.ok(dirty.some((error) => error.caseId === "STP5-stale-target"));
});

test("STP5-raw-cli: stock CLI diagnostics cannot be claimed from a Verter-only postprocessor", () => {
  assert.equal(validateCliAttribution(CLEAN_STOCK_CLI).length, 0);
  const dirty = validateCliAttribution(DIRTY_RAW_CLI);
  assert.ok(dirty.some((error) => error.caseId === "STP5-raw-cli"));
});

test("STP5-capability: version labels and dormant TCM2 are not complete-capability proof", () => {
  assert.ok(
    validateCapabilityClaim(DIRTY_VERSION_LABEL_CLAIM).some(
      (error) => error.caseId === "STP5-capability",
    ),
  );
  assert.ok(
    validateCapabilityClaim(DIRTY_DORMANT_CLAIM).some(
      (error) => error.caseId === "STP5-capability",
    ),
  );
});

test("STP5.2 geometry: verbatim, alias, multi-observation, synthesized scaffolding, non-overlapping virtual spans", () => {
  assert.equal(assertCleanGeometry(CLEAN_TRANSFORM).length, 0);
  assert.equal(assertNonOverlappingVirtualSpans(CLEAN_TRANSFORM.mappings).length, 0);
  assert.ok(originalToVirtual(CLEAN_TRANSFORM.mappings, 8).length >= 2);
  const overlapping = assertNonOverlappingVirtualSpans([
    [0, 10, 0, 10, 0],
    [5, 10, 20, 10, 0],
  ]);
  assert.ok(overlapping.some((error) => error.code === "overlapping-virtual-spans"));
});

test("STP5.1 session: encoding negotiation, projectHandle, full-content transform, closeProject", () => {
  const session = createMapperSession();
  const init = session.initialize({ positionEncodings: [...POSITION_ENCODINGS], chosen: "utf-8" });
  assert.equal(init.ok, true);
  assert.equal(init.result.positionEncoding, "utf-8");
  const opened = session.openProject({
    configFileName: "/tmp/tsconfig.json",
    projectHandle: "proj-a",
    compilerOptions: { strict: true },
  });
  assert.equal(opened.ok, true);
  const transformed = session.transform({
    projectHandle: "proj-a",
    fileName: "App.vue",
    content: CLEAN_TRANSFORM.original,
  });
  assert.equal(transformed.ok, true);
  assert.equal(session.closeProject({ projectHandle: "proj-a" }).ok, true);
  const stale = session.transform({
    projectHandle: "proj-a",
    fileName: "App.vue",
    content: CLEAN_TRANSFORM.original,
  });
  assert.equal(stale.ok, false);
});

test("STP5-capability: selected-build rows accept blocking upstream defects; help text is not a version-label proof", () => {
  assert.equal(helpTextHasMapperHost("tsc: The TypeScript Compiler"), false);
  const rows = capabilityRowsFromProbe({
    mapperHostPresent: false,
    observation: {
      diagnostics: true,
      hover: true,
      definition: true,
      references: true,
      edits: true,
    },
    engineId: "ts-native",
    engineVersion: "7.0.2",
  });
  const errors = evaluateCapabilityRows(rows);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  assert.ok(
    rows.some((row) => row.operation === "initialize" && row.status === "blocking-upstream-defect"),
  );
  assert.ok(rows.some((row) => row.operation === "hover" && row.status === "supported"));
});

test("STP5 evaluateStp5: clean twins plus reject dirty twins", async () => {
  const result = await evaluateStp5({
    mapperHostPresent: false,
    observation: {
      diagnostics: true,
      hover: true,
      definition: true,
      references: true,
      edits: true,
    },
    engineId: "ts-native",
    engineVersion: "7.0.2",
  });
  assert.equal(result.errors.length, 0, JSON.stringify(result.errors, null, 2));
  assert.equal(evaluateRejectTwins().length, 0);
});
