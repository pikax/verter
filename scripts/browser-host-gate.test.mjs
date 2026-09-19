// Selftest for scripts/browser-host-gate.mjs (node --test, no WASM/browser required).
// Proves the browser-host-domain gate FAILS on every pass-shaped hole it exists
// to close (BWH1-AC1/AC2/AC3) and passes only with real injected evidence.

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  RECEIPT_MARKER,
  censusForbiddenHits,
  compareIdentities,
  evaluateBrowserResult,
  missingArtifactExports,
  parseNativeReceipt,
  runGate,
} from "./browser-host-gate.mjs";
import {
  REQUIRED_OPERATIONS,
  emptyOperationFailures,
  nodeGlobalsPresent,
} from "../packages/browser-host/src/index.js";

const SYMBOLS = [
  { name: "ProbeAlias", kind: "typeAlias", isExported: true },
  { name: "ProbeClass", kind: "class", isExported: true },
  { name: "ProbeProps", kind: "interface", isExported: true },
  { name: "probeValue", kind: "const", isExported: true },
];
const TYPEINFO = { kind: "primitive", name: "string" };

function operations(overrides = {}) {
  return {
    session: [{ kind: "runtimeClient", nodes: [{ sourceMap: "{}" }] }],
    typeinfo: SYMBOLS,
    style: [{ selectorText: ".probe", matches: [{ tag: "div", result: "match" }] }],
    mapping: ['{"version":3}'],
    query: [{ decl: "ProbeAlias", hasRecord: true }],
    ...overrides,
  };
}

function receipt(overrides = {}) {
  return {
    operations: operations(),
    identities: { symbols: SYMBOLS, typeinfo: TYPEINFO },
    ...overrides,
  };
}

function browserResult(overrides = {}) {
  return {
    ok: true,
    nodeGlobals: { process: false, require: false, module: false, __dirname: false },
    operations: operations(),
    identities: { symbols: SYMBOLS, typeinfo: TYPEINFO },
    startupMs: 12,
    memory: { retainedBytes: 1024, unavailable: null },
    ...overrides,
  };
}

const WASM_JS = `
export default function init() {}
export function initSync() {}
export class VerterHost {
  upsert() {}
  compileRequest() {}
  listSymbols() {}
  resolveSymbolWithAudit() {}
  resolveTypeWithAudit() {}
  matchCssSelectors() {}
}
`;

function makeSandbox() {
  const root = mkdtempSync(path.join(tmpdir(), "browser-host-gate-"));
  mkdirSync(path.join(root, "packages", "wasm", "wasm"), { recursive: true });
  mkdirSync(path.join(root, "packages", "browser-host", "src", "fixtures"), { recursive: true });
  writeFileSync(path.join(root, "packages", "wasm", "wasm", "verter_wasm.js"), WASM_JS);
  writeFileSync(
    path.join(root, "packages", "wasm", "wasm", "verter_wasm_bg.wasm"),
    Buffer.alloc(2048),
  );
  writeFileSync(
    path.join(root, "packages", "browser-host", "src", "harness.html"),
    "<!doctype html>",
  );
  writeFileSync(path.join(root, "packages", "browser-host", "src", "worker.js"), "");
  return root;
}

function spawnOk(tree = "serde v1.0.0\n", native = receipt()) {
  return (command, args) => {
    if (command === "pnpm") return { status: 0, stdout: "", stderr: "" };
    if (command === "cargo" && args[0] === "tree") return { status: 0, stdout: tree, stderr: "" };
    if (command === "cargo" && args[0] === "test") {
      return { status: 0, stdout: `${RECEIPT_MARKER}${JSON.stringify(native)}\n`, stderr: "" };
    }
    return { status: 1, stdout: "", stderr: `unexpected ${command} ${args.join(" ")}` };
  };
}

function browsersAll(result = browserResult()) {
  return async () => ({
    ok: true,
    results: { chromium: result, firefox: result, webkit: result },
    failures: [],
  });
}

test("emptyOperationFailures rejects an empty array replacement (BWH1-AC2)", () => {
  assert.deepEqual(emptyOperationFailures(operations()), []);
  const failed = emptyOperationFailures(operations({ session: [] }));
  assert.equal(failed.length, 1);
  assert.match(failed[0], /empty array \(BWH1-AC2\)/);
  assert.ok(REQUIRED_OPERATIONS.includes("session"));
});

test("nodeGlobalsPresent is the AC3 discriminator", () => {
  assert.equal(
    nodeGlobalsPresent({ process: false, require: false, module: false, __dirname: false }),
    false,
  );
  assert.equal(
    nodeGlobalsPresent({ process: true, require: false, module: false, __dirname: false }),
    true,
  );
  assert.equal(nodeGlobalsPresent(undefined), true);
});

test("compareIdentities fails on native/browser divergence (BWH1-AC1)", () => {
  const native = receipt();
  const browser = receipt({
    identities: {
      symbols: [{ name: "Other", kind: "const", isExported: true }],
      typeinfo: TYPEINFO,
    },
  });
  const failures = compareIdentities(native, browser);
  assert.equal(failures.length, 1);
  assert.match(failures[0], /BWH1-AC1/);
  assert.deepEqual(compareIdentities(native, native), []);
});

test("evaluateBrowserResult fails Node globals and empty operations", () => {
  const empty = evaluateBrowserResult(
    "chromium",
    browserResult({
      operations: operations({ mapping: [] }),
      nodeGlobals: { process: true, require: false, module: false, __dirname: false },
    }),
  );
  assert.ok(empty.some((row) => /BWH1-AC3/.test(row)));
  assert.ok(empty.some((row) => /empty array/.test(row)));
});

test("censusForbiddenHits catches a wasm32 tokio edge and ignores inert noise", () => {
  assert.deepEqual(censusForbiddenHits("serde v1\nwasm-bindgen v0.2\n"), []);
  assert.deepEqual(censusForbiddenHits("tokio v1.40.0\nserde v1\n"), ["tokio"]);
});

test("missingArtifactExports fails a stub without host methods", () => {
  const missing = missingArtifactExports("export default function init() {}");
  assert.ok(missing.includes("initSync"));
  assert.ok(missing.includes("VerterHost"));
  assert.deepEqual(missingArtifactExports(WASM_JS), []);
});

test("parseNativeReceipt requires the marker", () => {
  assert.equal(parseNativeReceipt("ok\n").ok, false);
  const parsed = parseNativeReceipt(`${RECEIPT_MARKER}{"operations":{}}\n`);
  assert.equal(parsed.ok, true);
  const noisy = parseNativeReceipt(
    `${RECEIPT_MARKER}     Running unittests src\\lib.rs\n{"operations":{"session":[1]}}\n`,
  );
  assert.equal(noisy.ok, true);
  assert.deepEqual(noisy.receipt.operations.session, [1]);
});

test("runGate fails closed when the WASM artifact is missing", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "browser-host-missing-"));
  try {
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: true,
      spawn: spawnOk(),
      playwrightModule: {},
      runBrowsers: browsersAll(),
    });
    assert.equal(outcome.ok, false);
    assert.ok(outcome.failures.some((row) => /WASM artifact missing/.test(row)));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("runGate fails closed on a native process import in the census", async () => {
  const root = makeSandbox();
  try {
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: true,
      spawn: spawnOk("tokio v1.40.0\n"),
      playwrightModule: {},
      runBrowsers: browsersAll(),
    });
    assert.equal(outcome.ok, false);
    assert.ok(outcome.failures.some((row) => /tokio/.test(row)));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("runGate fails closed when playwright cannot be resolved", async () => {
  const root = makeSandbox();
  try {
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: true,
      spawn: spawnOk(),
      playwrightModule: null,
    });
    assert.equal(outcome.ok, false);
    assert.ok(outcome.failures.some((row) => /playwright/.test(row)));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("runGate fails closed on pass-with-no-tests (zero browsers)", async () => {
  const root = makeSandbox();
  try {
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: true,
      spawn: spawnOk(),
      playwrightModule: {},
      runBrowsers: async () => ({ ok: true, results: {}, failures: [] }),
    });
    assert.equal(outcome.ok, false);
    assert.ok(outcome.failures.some((row) => /no browser executed/.test(row)));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("runGate fails closed when an operation is an empty array", async () => {
  const root = makeSandbox();
  try {
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: true,
      spawn: spawnOk(),
      playwrightModule: {},
      runBrowsers: browsersAll(browserResult({ operations: operations({ query: [] }) })),
    });
    assert.equal(outcome.ok, false);
    assert.ok(outcome.failures.some((row) => /empty array \(BWH1-AC2\)/.test(row)));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("runGate passes only with matching native/browser evidence", async () => {
  const root = makeSandbox();
  try {
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: true,
      spawn: spawnOk(),
      playwrightModule: {},
      runBrowsers: browsersAll(),
    });
    assert.equal(outcome.ok, true, JSON.stringify(outcome.failures));
    assert.deepEqual(outcome.summary.engines, ["chromium", "firefox", "webkit"]);
    assert.deepEqual(outcome.summary.operations, REQUIRED_OPERATIONS);
    assert.ok(outcome.summary.artifactBytes >= 1000);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
