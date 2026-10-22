import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { probeMapperHost, resolveEngine } from "../../../scripts/sfc-projection/verify-node.mjs";
import { runMapperHostEndToEnd } from "./mapper-host/e2e.mjs";
import { createMapperHandler } from "./mapper-host/fixture-mapper/server.mjs";
import {
  FIXTURE_MAPPER_DIR,
  INVALID_MAPPER_OPTIONS,
  STP5_PARSE_ERROR_SOURCE,
} from "./mapper-host/harness.mjs";
import {
  CLEAN_ALIAS,
  CLEAN_DEFINITION,
  CLEAN_GUARD,
  CLEAN_STOCK_CLI,
  CLEAN_TRANSFORM,
  CLEAN_UNUSED_EXPECT,
  DIRTY_ALIAS_EDIT,
  DIRTY_BLOCKED_EMPTY_EVIDENCE,
  DIRTY_DORMANT_CLAIM,
  DIRTY_GUARD_AMBIGUOUS_INDEX,
  DIRTY_GUARD_BARE_ARRAY,
  DIRTY_GUARD_DUPLICATE,
  DIRTY_RAW_CLI,
  DIRTY_SILENT_DEGRADE_CLAIM,
  DIRTY_STALE_TARGET,
  DIRTY_UNUSED_EXPECT,
  DIRTY_UNUSED_EXPECT_WRONG_CODE,
  DIRTY_VERSION_LABEL_CLAIM,
  DiagnosticDirectivePolicy,
  ENCODING_FIXTURE,
  HOST_CAPABILITY_OPERATIONS,
  MAPPER_HOST_OPERATIONS,
  POSITION_ENCODINGS,
  SpanMapFeature,
  VIRTUAL_EXTENSIONS,
  WIRE_REJECT_TWINS,
  assertCleanGeometry,
  assertEncodingIdentity,
  assertFeatureMaskDoesNotSuppressDiagnostics,
  assertNonOverlappingVirtualSpans,
  capabilityRowsFromProbe,
  createMapperSession,
  decodeSpan,
  evaluateCapabilityRows,
  evaluateRejectTwins,
  evaluateStp5,
  helpTextHasMapperHost,
  loadStp5Product,
  noMapperHostEvidence,
  validateStp5Products,
  validateUnusedExpectReportedInOriginal,
  originalToVirtual,
  spanForToken,
  validateCapabilityClaim,
  validateCliAttribution,
  validateDefinitionTarget,
  validateDiagnosticDirectives,
  validateEdit,
  validateInitializeResult,
  validateOpenProjectResult,
  validateTransformResult,
  validateWireDiagnosticDirectives,
} from "./protocol.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

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

test("STP5-capability: neither-status is silent-degrade; blocked empty evidence is rejected", () => {
  assert.ok(
    validateCapabilityClaim(DIRTY_SILENT_DEGRADE_CLAIM).some(
      (error) => error.code === "silent-degrade",
    ),
  );
  assert.ok(
    validateCapabilityClaim(DIRTY_BLOCKED_EMPTY_EVIDENCE).some(
      (error) => error.code === "missing-defect-evidence",
    ),
  );
});

test("STP5-guard-duplicate: unused Expect must be reported in the original file", () => {
  const dirty = validateUnusedExpectReportedInOriginal(DIRTY_UNUSED_EXPECT);
  assert.ok(dirty.some((error) => error.code === "unused-expect-not-in-original"));
});

test("STP5-capability: encoding-negotiation needs the executed host run AND encoding identity, not a hard-coded supported row", () => {
  const observation = {
    diagnostics: true,
    hover: true,
    definition: true,
    references: true,
    edits: true,
  };
  const hostRun = {
    performed: true,
    operations: HOST_CAPABILITY_OPERATIONS.map((operation) => ({
      operation,
      ok: true,
      detail: "observed",
    })),
  };
  const withHost = {
    mapperHostPresent: true,
    mapperHostEvidence: "executable-selected-build",
    hostRun,
    observation,
    engineId: "ts-native",
    engineVersion: "7.1.0-beta",
  };
  const encodingRow = (rows) => rows.find((row) => row.operation === "encoding-negotiation");
  const ok = encodingRow(capabilityRowsFromProbe({ ...withHost, encodingIdentityOk: true }));
  assert.equal(ok.status, "supported");
  assert.equal(ok.evidence, "executable-selected-build");
  const failed = encodingRow(capabilityRowsFromProbe({ ...withHost, encodingIdentityOk: false }));
  assert.equal(failed.status, "blocking-upstream-defect");
  assert.equal(encodingRow(capabilityRowsFromProbe(withHost)).status, "blocking-upstream-defect");
  // Encoding identity alone is not a host capability: without a host the
  // row is blocked, named by the build.
  const noHost = encodingRow(
    capabilityRowsFromProbe({
      mapperHostPresent: false,
      mapperHostEvidence: noMapperHostEvidence(
        "ts-native",
        "7.0.2",
        "tsc --help --all lists neither",
      ),
      observation,
      engineId: "ts-native",
      engineVersion: "7.0.2",
      encodingIdentityOk: true,
    }),
  );
  assert.equal(noHost.status, "blocking-upstream-defect");
  assert.match(noHost.evidence, /^no mapper host on ts-native@7\.0\.2/);
  // A host whose end-to-end run was not executed is not a supported host.
  const notRun = capabilityRowsFromProbe({ ...withHost, hostRun: null, encodingIdentityOk: true });
  assert.ok(
    notRun
      .filter((row) => HOST_CAPABILITY_OPERATIONS.includes(row.operation))
      .every((row) => row.status === "blocking-upstream-defect"),
  );
  // One failed operation blocks that operation only, with the run's detail.
  const partial = capabilityRowsFromProbe({
    ...withHost,
    encodingIdentityOk: true,
    hostRun: {
      performed: true,
      operations: hostRun.operations.map((row) =>
        row.operation === "watched-files" ? { ...row, ok: false, detail: "no reopen" } : row,
      ),
    },
  });
  const watched = partial.find((row) => row.operation === "watched-files");
  assert.equal(watched.status, "blocking-upstream-defect");
  assert.match(watched.evidence, /ts-native@7\.1\.0-beta watched-files: no reopen/);
  assert.equal(partial.find((row) => row.operation === "transform").status, "supported");
  assert.equal(evaluateCapabilityRows(partial).length, 0);
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
    mapperHostEvidence:
      "no mapper host on ts-native@7.0.2: tsc --help --all lists neither --runExternalCode nor contentMappers",
    observation: {
      diagnostics: true,
      hover: true,
      definition: true,
      references: true,
      edits: true,
    },
    engineId: "ts-native",
    engineVersion: "7.0.2",
    encodingIdentityOk: true,
  });
  const errors = evaluateCapabilityRows(rows);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  assert.ok(
    rows.some((row) => row.operation === "initialize" && row.status === "blocking-upstream-defect"),
  );
  assert.ok(rows.some((row) => row.operation === "hover" && row.status === "supported"));
  assert.match(rows.find((row) => row.operation === "initialize").evidence, /tsc --help/);
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
  assert.equal(validateStp5Products().length, 0);
});

test("STP5 final wire: initialize carries no protocolVersion and a non-reserved diagnosticSource", () => {
  const session = createMapperSession();
  const init = session.initialize({
    positionEncodings: [...POSITION_ENCODINGS],
    locale: "fr",
    chosen: "utf-16",
  });
  assert.equal(init.ok, true);
  assert.deepEqual(Object.keys(init.result).sort(), ["diagnosticSource", "positionEncoding"]);
  assert.equal(init.locale, "fr");
  assert.equal(validateInitializeResult(init.result).length, 0);
  for (const source of ["tsc", "TypeScript", "ts", "JSON", " "]) {
    assert.ok(
      validateInitializeResult({ positionEncoding: "utf-8", diagnosticSource: source }).length > 0,
      source,
    );
  }
});

test("STP5 final wire: dynamicConfig is a manifest field; configIdentity and watchedFiles are openProject results", () => {
  const configFileName = path.resolve("/tmp/tsconfig.json");
  const dynamic = createMapperSession({ manifest: { dynamicConfig: true } });
  dynamic.initialize({ positionEncodings: [...POSITION_ENCODINGS] });
  const opened = dynamic.openProject({ configFileName, projectHandle: "p", compilerOptions: {} });
  assert.equal(validateOpenProjectResult(opened.result, { dynamicConfig: true }).length, 0);
  assert.ok(opened.result.configIdentity);
  const fixed = createMapperSession();
  fixed.initialize({ positionEncodings: [...POSITION_ENCODINGS] });
  const staticOpened = fixed.openProject({
    configFileName,
    projectHandle: "p",
    compilerOptions: {},
  });
  assert.equal(validateOpenProjectResult(staticOpened.result, { dynamicConfig: false }).length, 0);
  assert.equal(staticOpened.result.configIdentity, undefined);
  // An openProject PARAM named dynamicConfig changes nothing.
  const smuggled = fixed.openProject({
    configFileName,
    projectHandle: "q",
    compilerOptions: {},
    dynamicConfig: true,
    configIdentity: "x",
  });
  assert.equal(smuggled.result.configIdentity, undefined);
  assert.ok(
    validateOpenProjectResult(
      { watchedFiles: ["relative.json"], configIdentity: "x" },
      { dynamicConfig: true },
    ).some((error) => error.code === "relative-watched-file"),
  );
  assert.ok(
    validateOpenProjectResult(
      { optionDiagnostics: [{ path: ["plugins", -1], messageText: "m", code: 1 }] },
      { dynamicConfig: false },
    ).some((error) => error.code === "option-diagnostic-shape"),
  );
});

test("STP5 final wire: diagnosticDirectives is an object with an unused-diagnostic table and indexed tuples", () => {
  assert.equal(validateWireDiagnosticDirectives(CLEAN_GUARD.directives).length, 0);
  assert.ok(
    validateWireDiagnosticDirectives(DIRTY_GUARD_BARE_ARRAY.directives).some(
      (error) => error.code === "directive-shape",
    ),
  );
  assert.ok(
    validateWireDiagnosticDirectives(DIRTY_GUARD_AMBIGUOUS_INDEX.directives).some(
      (error) => error.code === "unused-expect-index",
    ),
  );
  const indexed = {
    ...DIRTY_GUARD_AMBIGUOUS_INDEX.directives,
    directives: [[0, 24, 0, 40, DiagnosticDirectivePolicy.Expect, 1]],
  };
  assert.equal(validateWireDiagnosticDirectives(indexed).length, 0);
  // The host accepts an Ignore with no authored span and an empty virtual
  // range; Verter's emission policy does not emit either.
  const hostOnly = {
    unusedExpectDirectiveDiagnostics: [],
    directives: [[0, 0, 5, 5, DiagnosticDirectivePolicy.Ignore]],
  };
  assert.equal(validateWireDiagnosticDirectives(hostOnly).length, 0);
  assert.ok(validateDiagnosticDirectives(hostOnly, CLEAN_GUARD.original).length > 0);
  assert.ok(
    validateUnusedExpectReportedInOriginal(DIRTY_UNUSED_EXPECT_WRONG_CODE).some(
      (error) => error.code === "unused-expect-not-in-original",
    ),
  );
  assert.equal(validateUnusedExpectReportedInOriginal(CLEAN_UNUSED_EXPECT).length, 0);
});

test("STP5 final wire: every wire reject twin is refused with its code", () => {
  for (const twin of WIRE_REJECT_TWINS) {
    assert.ok(
      twin.validate().some((error) => error.code === twin.code),
      `${twin.name} not refused with ${twin.code}`,
    );
  }
  const { original, targetFile: _targetFile, ...output } = CLEAN_TRANSFORM;
  assert.equal(validateTransformResult(output, original).length, 0);
  assert.equal(SpanMapFeature.All, (SpanMapFeature.CodeLens << 1) - 1);
  assert.deepEqual(
    [...VIRTUAL_EXTENSIONS],
    loadStp5Product("mapper-capability-evidence.json").selectedMapperProtocol.virtualExtensions,
  );
});

test("STP5 fixture mapper: its replies conform to the final wire under both encodings", () => {
  const line = 'let e: string = "\u{1F600}"; let bad: number = e';
  for (const encoding of POSITION_ENCODINGS) {
    const handler = createMapperHandler({ encoding });
    const init = handler.handle("initialize", { positionEncodings: [...POSITION_ENCODINGS] });
    assert.equal(validateInitializeResult(init).length, 0);
    assert.equal(init.positionEncoding, encoding);
    const opened = handler.handle("openProject", {
      configFileName: path.resolve("/tmp/stp5/tsconfig.json"),
      projectHandle: "h",
      options: INVALID_MAPPER_OPTIONS,
      compilerOptions: { strict: true },
    });
    assert.equal(validateOpenProjectResult(opened, { dynamicConfig: true }).length, 0);
    assert.deepEqual(
      opened.optionDiagnostics.map((row) => row.path),
      [["plugins", 1, "name"]],
    );
    const result = handler.handle("transform", {
      fileName: "App.stp5",
      content: STP5_PARSE_ERROR_SOURCE,
      projectHandle: "h",
    });
    assert.equal(validateTransformResult(result, STP5_PARSE_ERROR_SOURCE, encoding).length, 0);
    if (encoding === "utf-16") {
      assert.equal(
        validateDiagnosticDirectives(result.diagnosticDirectives, STP5_PARSE_ERROR_SOURCE).length,
        0,
      );
    }
    // The Verbatim span holding `bad` decodes to the same text in the
    // original and the virtual output, in the negotiated encoding.
    const mapping = result.mappings.find(
      (row) =>
        decodeSpan(STP5_PARSE_ERROR_SOURCE, { start: row[2], length: row[3] }, encoding) === line,
    );
    assert.ok(mapping, `${encoding}: no verbatim mapping for the emoji line`);
    assert.equal(
      decodeSpan(result.text, { start: mapping[0], length: mapping[1] }, encoding),
      line,
    );
    assert.equal(handler.handle("closeProject", { projectHandle: "h" }), null);
    assert.throws(() =>
      handler.handle("transform", { fileName: "App.stp5", content: "", projectHandle: "h" }),
    );
  }
  const staticHandler = createMapperHandler({ twin: "static-config" });
  staticHandler.handle("initialize", { positionEncodings: [...POSITION_ENCODINGS] });
  const staticOpened = staticHandler.handle("openProject", {
    configFileName: path.resolve("/tmp/stp5/tsconfig.json"),
    projectHandle: "s",
    compilerOptions: {},
  });
  assert.equal(validateOpenProjectResult(staticOpened, { dynamicConfig: false }).length, 0);
});

test("STP5 fixture mapper: JSON-RPC over stdio with Content-Length framing; exits when TypeScript closes the pipe", async () => {
  const child = spawn(
    process.execPath,
    [path.join(FIXTURE_MAPPER_DIR, "server.mjs"), "--encoding=utf-8"],
    { stdio: ["pipe", "pipe", "inherit"] },
  );
  const replies = [];
  let buffer = Buffer.alloc(0);
  child.stdout.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const end = buffer.indexOf("\r\n\r\n");
      if (end < 0) return;
      const length = Number(/Content-Length: (\d+)/.exec(buffer.subarray(0, end).toString())[1]);
      if (buffer.length < end + 4 + length) return;
      replies.push(JSON.parse(buffer.subarray(end + 4, end + 4 + length).toString("utf8")));
      buffer = buffer.subarray(end + 4 + length);
    }
  });
  const send = (id, method, params) => {
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", id, method, params }), "utf8");
    child.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
    child.stdin.write(body);
  };
  const exited = new Promise((resolve) => child.on("exit", resolve));
  send("a1", "initialize", { positionEncodings: ["utf-8", "utf-16"] });
  send("a2", "transform", { fileName: "x.stp5", content: "", projectHandle: "missing" });
  send("a3", "bogus", {});
  const deadline = Date.now() + 10000;
  while (replies.length < 3 && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  child.stdin.end();
  assert.equal(await exited, 0);
  assert.deepEqual(
    replies.map((reply) => reply.id),
    ["a1", "a2", "a3"],
  );
  assert.deepEqual(replies[0].result, {
    positionEncoding: "utf-8",
    diagnosticSource: "stp5-fixture",
  });
  assert.equal(replies[1].error.code, -32602);
  assert.equal(replies[2].error.code, -32601);
});

/** The repo's ts-native engine (the root `typescript` pin), resolved as the verifier does. */
async function resolveTsNative() {
  const matrix = JSON.parse(
    fs.readFileSync(
      path.join(REPO_ROOT, "tests/sfc-projection/STP1/products/engine-matrix.json"),
      "utf8",
    ),
  );
  const pin = matrix.engines.find((engine) => engine.id === "ts-native");
  const resolved = await resolveEngine(pin, REPO_ROOT);
  assert.equal(resolved.ok, true, JSON.stringify(resolved.error));
  return resolved.engine;
}

test("STP5 executed mapper host: the root ts-native engine runs the fixture mapper end to end once it has a host", async (t) => {
  const engine = await resolveTsNative();
  const probe = probeMapperHost(engine, { helpTextHasMapperHost });
  assert.equal(probe.performed, true, probe.evidence);
  if (!probe.present) {
    // Not a pass: the build has no host, recorded by name. This turns on by
    // itself when the root pin moves to a mapper-capable TypeScript.
    assert.ok(
      probe.evidence.startsWith(`no mapper host on ts-native@${engine.version}`),
      probe.evidence,
    );
    t.skip(probe.evidence);
    return;
  }
  const run = await runMapperHostEndToEnd(engine);
  assert.deepEqual(
    run.operations.map((row) => row.operation),
    [...MAPPER_HOST_OPERATIONS],
  );
  const failed = run.operations.filter((row) => !row.ok);
  assert.equal(failed.length, 0, failed.map((row) => `${row.operation}: ${row.detail}`).join("\n"));
});
