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
  defaultRunBrowsers,
  evaluateBrowserResult,
  missingArtifactExports,
  parseNativeReceipt,
  playwrightEngine,
  runGate,
  stableSerialize,
} from "./browser-host-gate.mjs";
import {
  REQUIRED_OPERATIONS,
  emptyOperationFailures,
  nodeGlobalsPresent,
  normalizeSymbolIdentities,
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
export function initSync() {}
export class VerterHost {
  upsert() {}
  compileRequest() {}
  listSymbols() {}
  resolveSymbolWithAudit() {}
  resolveTypeWithAudit() {}
  matchCssSelectors() {}
}
export { initSync, __wbg_init as default };
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

function isPnpmLaunch(command) {
  const base = path.basename(String(command)).toLowerCase();
  return (
    base === "pnpm" ||
    base === "pnpm.cmd" ||
    base === "pnpm.exe" ||
    base === "pnpm.bat" ||
    base === "cmd.exe"
  );
}

function spawnOk(tree = "serde v1.0.0\n", native = receipt()) {
  return (command, args) => {
    if (isPnpmLaunch(command)) return { status: 0, stdout: "", stderr: "" };
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

test("compareIdentities ignores serde-sorted vs JS-insertion key order (BWH1-AC1)", () => {
  const native = receipt({
    identities: {
      symbols: SYMBOLS.map(({ name, kind, isExported }) => ({ isExported, kind, name })),
      typeinfo: { name: "string", kind: "primitive" },
    },
  });
  const browser = receipt({
    identities: {
      symbols: SYMBOLS.map(({ name, kind, isExported }) => ({ name, kind, isExported })),
      typeinfo: { kind: "primitive", name: "string" },
    },
  });
  assert.deepEqual(compareIdentities(native, browser), []);
  assert.notEqual(
    JSON.stringify(native.identities.symbols),
    JSON.stringify(browser.identities.symbols),
  );
  assert.equal(
    stableSerialize(native.identities.symbols),
    stableSerialize(browser.identities.symbols),
  );
});

test("normalizeSymbolIdentities sorts by code-unit order, not localeCompare", () => {
  const rows = normalizeSymbolIdentities([
    { name: "aValue", kind: "const", isExported: true },
    { name: "BValue", kind: "const", isExported: true },
  ]);
  assert.deepEqual(
    rows.map((row) => row.name),
    ["BValue", "aValue"],
  );
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

test("evaluateBrowserResult does not relabel a worker error as BWH1-AC3", () => {
  const failed = evaluateBrowserResult("chromium", {
    ok: false,
    error: "WebAssembly.instantiate failed",
  });
  assert.ok(failed.some((row) => /WebAssembly\.instantiate failed/.test(row)));
  assert.ok(!failed.some((row) => /BWH1-AC3/.test(row)));
});

test("evaluateBrowserResult fail-closes AC3 when a successful worker omits nodeGlobals", () => {
  const leaked = evaluateBrowserResult("chromium", {
    ok: true,
    operations: operations(),
  });
  assert.ok(leaked.some((row) => /BWH1-AC3/.test(row)));
});

test("evaluateBrowserResult rejects a waitForFunction boolean handle", () => {
  const failed = evaluateBrowserResult("chromium", true);
  assert.ok(failed.some((row) => /worker failed/.test(row)));
  assert.ok(failed.some((row) => /operations object missing/.test(row)));
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

test("missingArtifactExports accepts wasm-bindgen 0.2.122 export-list default", () => {
  assert.ok(!/export default/.test(WASM_JS));
  assert.match(WASM_JS, /as\s+default/);
  assert.deepEqual(missingArtifactExports(WASM_JS), []);
  assert.ok(missingArtifactExports("export function initSync() {}").includes("default"));
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

test("runGate default build launches pnpm via cmd.exe on win32 and surfaces spawn error.code", async () => {
  const root = makeSandbox();
  try {
    const launched = [];
    const outcome = await runGate({
      repoRoot: root,
      skipBuild: false,
      spawn: (command, args) => {
        launched.push({ command, args });
        return {
          status: null,
          error: { code: "ENOENT", message: "spawn ENOENT" },
          stdout: "",
          stderr: "",
        };
      },
      playwrightModule: {},
      runBrowsers: browsersAll(),
    });
    assert.equal(outcome.ok, false);
    assert.ok(
      outcome.failures.some((row) => /wasm build failed:.*ENOENT/.test(row)),
      JSON.stringify(outcome.failures),
    );
    assert.equal(launched.length, 1);
    const launchedCmd = path.basename(launched[0].command).toLowerCase();
    if (process.platform === "win32") {
      assert.notEqual(launchedCmd, "pnpm");
      assert.match(path.basename(launched[0].command), /cmd\.exe/i);
      assert.ok(launched[0].args.some((arg) => /pnpm/i.test(String(arg))));
    } else {
      assert.equal(launchedCmd, "pnpm");
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("playwrightEngine reads CJS namespace default when named exports are absent", () => {
  const launch = () => {};
  const cjs = { default: { chromium: { launch } }, "module.exports": {} };
  assert.equal(playwrightEngine(cjs, "chromium")?.launch, launch);
  assert.equal(playwrightEngine({ chromium: { launch } }, "chromium")?.launch, launch);
  assert.equal(playwrightEngine(cjs, "firefox"), undefined);
});

test("defaultRunBrowsers launches off CJS default instead of reporting launcher missing", async () => {
  const launched = [];
  const waitCalls = [];
  const payload = browserResult();
  const fakeEngine = {
    launch: async () => {
      launched.push("ok");
      return {
        newPage: async () => ({
          goto: async () => {},
          waitForFunction: async (fn, arg, options) => {
            waitCalls.push({ arg, options });
            const prev = globalThis.window;
            globalThis.window = { __bwh1: { ...payload, pending: false } };
            try {
              const value = fn(arg);
              return { jsonValue: async () => value };
            } finally {
              if (prev === undefined) delete globalThis.window;
              else globalThis.window = prev;
            }
          },
        }),
        close: async () => {},
      };
    },
  };
  const outcome = await defaultRunBrowsers({
    origin: "http://127.0.0.1:1",
    engines: ["chromium"],
    playwrightModule: { default: { chromium: fakeEngine }, "module.exports": {} },
  });
  assert.deepEqual(launched, ["ok"]);
  assert.equal(waitCalls.length, 1);
  assert.equal(waitCalls[0].arg, undefined);
  assert.equal(waitCalls[0].options?.timeout, 120_000);
  assert.equal(outcome.ok, true, JSON.stringify(outcome.failures));
  assert.equal(outcome.results.chromium.ok, true);
  assert.equal(outcome.results.chromium.pending, false);
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

// ---------------------------------------------------------------------------
// Async input acquisition → committed snapshot handoff (pure client half;
// the committed-input core and its typed statuses are pinned natively in
// crates/verter_wasm/src/input_snapshot.rs).
// ---------------------------------------------------------------------------

test("acquisitionWaveFailures reports every incoherent-wave shape", async () => {
  const { acquisitionWaveFailures } = await import("../packages/browser-host/src/input-handoff.js");
  assert.deepEqual(
    acquisitionWaveFailures([{ canonical: "/a.ts", content: "a" }], ["/gone.ts"]),
    [],
  );
  assert.ok(
    acquisitionWaveFailures(
      [
        { canonical: "/a.ts", content: "1" },
        { canonical: "/a.ts", content: "2" },
        { canonical: "", content: "x" },
      ],
      [],
    ).some((failure) => failure.includes("canonical")),
  );
  assert.ok(
    acquisitionWaveFailures([{ canonical: "/a.ts", content: "1" }], ["/a.ts"]).some((failure) =>
      failure.includes("both acquired and probed missing"),
    ),
  );
  assert.deepEqual(acquisitionWaveFailures(null, []), ["files is not an array"]);
  assert.deepEqual(acquisitionWaveFailures([], "nope"), ["missing is not an array"]);
});

test("acquireInputWave awaits the adapter once per canonical and never fetches on commit", async () => {
  const { acquireInputWave, commitInputSnapshot } =
    await import("../packages/browser-host/src/input-handoff.js");
  const reads = [];
  const wave = await acquireInputWave(
    async (canonical) => {
      reads.push(canonical);
      return canonical === "/gone.ts" ? null : `content of ${canonical}`;
    },
    ["/a.ts", "/gone.ts"],
  );
  assert.deepEqual(reads, ["/a.ts", "/gone.ts"]);
  assert.deepEqual(wave.files, [{ canonical: "/a.ts", content: "content of /a.ts" }]);
  assert.deepEqual(wave.missing, ["/gone.ts"]);

  const committed = [];
  const host = {
    commitInputSnapshot(files, missing) {
      committed.push({ files, missing });
      return { basisId: "b".repeat(64), files: files.length, missing: missing.length };
    },
  };
  const receipt = commitInputSnapshot(host, wave);
  assert.equal(committed.length, 1);
  assert.equal(receipt.basisId, "b".repeat(64));
});

test("observeInputSnapshot raises typed NeedInputsError only for needInputs", async () => {
  const { NeedInputsError, observeInputSnapshot, needInputsKeys } =
    await import("../packages/browser-host/src/input-handoff.js");
  const host = {
    observeInputSnapshot(_basisId, canonical) {
      if (canonical === "/present.ts") {
        return { canonical, status: "file", content: "x" };
      }
      if (canonical === "/gone.ts") {
        return { canonical, status: "absent" };
      }
      return { canonical, status: "needInputs" };
    },
  };
  assert.deepEqual(host.observeInputSnapshot("basis", "/present.ts"), {
    canonical: "/present.ts",
    status: "file",
    content: "x",
  });
  // Probed-missing is a complete negative, NOT an acquisition demand.
  assert.deepEqual(observeInputSnapshot(host, "basis", "/gone.ts"), {
    canonical: "/gone.ts",
    status: "absent",
  });
  assert.throws(
    () => observeInputSnapshot(host, "basis", "/never.ts"),
    (error) => {
      assert.ok(error instanceof NeedInputsError);
      assert.deepEqual(error.keys, ["/never.ts"]);
      assert.deepEqual(needInputsKeys(error), ["/never.ts"]);
      return true;
    },
  );
  // A non-NeedInputs error keeps its own shape.
  assert.deepEqual(needInputsKeys(new Error("other")), []);
});
