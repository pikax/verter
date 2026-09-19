#!/usr/bin/env node
// browser-host-domain gate runner (BWH1).
//
// Builds the actual WASM session dependency closure, runs the native
// feasibility probe, drives the same source-linked probes in Chromium,
// Firefox and WebKit workers (Node globals unavailable), and fails closed
// when the artifact, exports, tests, browsers or operation results are
// missing. A build that replaces an operation with an empty array is
// rejected (BWH1-AC2). Missing preconditions NEVER pass.
//
// Usage:
//   node scripts/browser-host-gate.mjs [--root <repo>] [--skip-build]

import { spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  REQUIRED_OPERATIONS,
  compareCodeUnits,
  emptyOperationFailures,
  nodeGlobalsPresent,
  canonicalizeTypeInfo,
} from "../packages/browser-host/src/index.js";
import { pnpmCommand, resolvePnpm } from "./gate-internals.mjs";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const defaultRepoRoot = path.dirname(scriptDir);

export const ENGINES = ["chromium", "firefox", "webkit"];
export const FORBIDDEN_NATIVE_CRATES = ["tokio", "rayon", "mio", "nix", "tempfile", "socket2"];
export const REQUIRED_ARTIFACT_EXPORTS = ["default", "initSync", "VerterHost"];
export const REQUIRED_HOST_METHODS = [
  "upsert",
  "compileRequest",
  "listSymbols",
  "resolveSymbolWithAudit",
  "resolveTypeWithAudit",
  "matchCssSelectors",
];
export const WASM_JS_REL = path.join("packages", "wasm", "wasm", "verter_wasm.js");
export const WASM_BIN_REL = path.join("packages", "wasm", "wasm", "verter_wasm_bg.wasm");
export const HARNESS_REL = path.join("packages", "browser-host", "src");
export const NATIVE_PROBE_TEST = "feasibility_probe_tests::bwh1_native_feasibility_probe";
export const RECEIPT_MARKER = "BWH1_NATIVE_RECEIPT:";

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".ts": "text/plain; charset=utf-8",
  ".vue": "text/plain; charset=utf-8",
  ".json": "application/json; charset=utf-8",
};

function extractJsonObject(text) {
  const start = text.indexOf("{");
  if (start < 0) throw new Error("no JSON object");
  let depth = 0;
  let inString = false;
  let escape = false;
  for (let i = start; i < text.length; i++) {
    const char = text[i];
    if (inString) {
      if (escape) escape = false;
      else if (char === "\\") escape = true;
      else if (char === '"') inString = false;
      continue;
    }
    if (char === '"') inString = true;
    else if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return text.slice(start, i + 1);
    }
  }
  throw new Error("unterminated JSON object");
}

export function parseNativeReceipt(text) {
  const haystack = text ?? "";
  const idx = haystack.indexOf(RECEIPT_MARKER);
  if (idx < 0) return { ok: false, reason: "native probe printed no BWH1_NATIVE_RECEIPT" };
  try {
    return {
      ok: true,
      receipt: JSON.parse(extractJsonObject(haystack.slice(idx + RECEIPT_MARKER.length))),
    };
  } catch (error) {
    return { ok: false, reason: `native receipt JSON parse failed: ${error.message}` };
  }
}

export function censusForbiddenHits(treeText) {
  const hits = [];
  for (const line of (treeText ?? "").split(/\r?\n/)) {
    const trimmed = line.trim();
    for (const crate of FORBIDDEN_NATIVE_CRATES) {
      if (trimmed === crate || trimmed.startsWith(`${crate} `) || trimmed.startsWith(`${crate}v`)) {
        hits.push(crate);
      }
    }
  }
  return [...new Set(hits)];
}

/** wasm-bindgen 0.2.122 `--target web` emits `export { initSync, __wbg_init as default }`, not `export default`. */
export function hasDefaultExport(jsText) {
  const text = jsText ?? "";
  return /\bexport\s+default\b/.test(text) || /\bexport\s*\{[^}]*\bas\s+default\b/.test(text);
}

export function missingArtifactExports(jsText) {
  const missing = [];
  for (const name of REQUIRED_ARTIFACT_EXPORTS) {
    const present =
      name === "default" ? hasDefaultExport(jsText) : new RegExp(`\\b${name}\\b`).test(jsText);
    if (!present) missing.push(name);
  }
  for (const method of REQUIRED_HOST_METHODS) {
    if (!new RegExp(`\\b${method}\\b`).test(jsText)) missing.push(`VerterHost.${method}`);
  }
  return missing;
}

/** Recursively sort object keys so serde_json BTreeMap order matches JS insertion order. */
export function stableSerialize(value) {
  return JSON.stringify(sortKeys(value));
}

function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value != null && typeof value === "object") {
    const sorted = {};
    for (const key of Object.keys(value).sort(compareCodeUnits)) {
      sorted[key] = sortKeys(value[key]);
    }
    return sorted;
  }
  return value;
}

export function compareIdentities(native, browser) {
  const failures = [];
  const nativeSymbols = stableSerialize(native?.identities?.symbols ?? null);
  const browserSymbols = stableSerialize(browser?.identities?.symbols ?? null);
  if (nativeSymbols !== browserSymbols) {
    failures.push(
      `native/browser symbol identities diverge after normalization (BWH1-AC1): native=${nativeSymbols} browser=${browserSymbols}`,
    );
  }
  const nativeType = stableSerialize(canonicalizeTypeInfo(native?.identities?.typeinfo));
  const browserType = stableSerialize(canonicalizeTypeInfo(browser?.identities?.typeinfo));
  if (nativeType !== browserType) {
    failures.push(
      `native/browser TypeInfo observations diverge (BWH1-AC1): native=${nativeType} browser=${browserType}`,
    );
  }
  return failures;
}

export function evaluateBrowserResult(engine, result) {
  const failures = [];
  if (!result) {
    failures.push(`${engine}: no worker result`);
    return failures;
  }
  if (result.ok !== true) {
    failures.push(`${engine}: ${result.error ?? "worker failed"}`);
  }
  const nodeGlobalsReported = result.nodeGlobals != null && typeof result.nodeGlobals === "object";
  // Success without a census is fail-closed (the field is expected). A worker
  // error with no nodeGlobals must not be relabelled as BWH1-AC3.
  if (nodeGlobalsReported ? nodeGlobalsPresent(result.nodeGlobals) : result.ok === true) {
    failures.push(`${engine}: Node globals available in worker (BWH1-AC3)`);
  }
  failures.push(
    ...emptyOperationFailures(result.operations).map((reason) => `${engine}: ${reason}`),
  );
  return failures;
}

function defaultSpawn(command, args, options) {
  return spawnSync(command, args, {
    cwd: options.cwd,
    env: options.env ?? process.env,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
    shell: false,
    windowsVerbatimArguments: options.windowsVerbatimArguments,
  });
}

function spawnFailureDetail(result) {
  const output = (result.stderr || result.stdout || "").trim();
  const code = result.error?.code;
  const message = result.error?.message;
  const bits = [code, output || message].filter(Boolean);
  return (bits.join(": ") || `status ${result.status ?? "null"}`).slice(-2000);
}

function spawnPnpm(spawn, args, options) {
  const env = options.env ?? process.env;
  const pnpmPath = resolvePnpm(env);
  if (pnpmPath === null) {
    return {
      status: 1,
      stdout: "",
      stderr: "pnpm not found on PATH",
      error: { code: "ENOENT", message: "pnpm not found on PATH" },
    };
  }
  const launch = pnpmCommand(pnpmPath, args, undefined, env);
  if (launch.setupFail) {
    return {
      status: 1,
      stdout: "",
      stderr: launch.detail,
      error: { code: "SETUP_FAIL", message: launch.detail },
    };
  }
  return spawn(launch.cmd, launch.args, {
    cwd: options.cwd,
    env,
    windowsVerbatimArguments: launch.windowsVerbatimArguments,
  });
}

function log(line) {
  process.stdout.write(`browser-host-gate: ${line}\n`);
}

function serveHarness({ hostSrc, wasmDir }) {
  const server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    let rel = decodeURIComponent(url.pathname);
    if (rel === "/") rel = "/harness.html";
    const rooted = rel.startsWith("/wasm/")
      ? path.join(wasmDir, rel.slice("/wasm/".length))
      : path.join(hostSrc, rel.slice(1));
    const resolved = path.resolve(rooted);
    const allowed =
      resolved.startsWith(path.resolve(hostSrc)) || resolved.startsWith(path.resolve(wasmDir));
    if (!allowed || !existsSync(resolved) || statSync(resolved).isDirectory()) {
      response.writeHead(404);
      response.end("not found");
      return;
    }
    const ext = path.extname(resolved);
    response.writeHead(200, { "content-type": MIME[ext] ?? "application/octet-stream" });
    response.end(readFileSync(resolved));
  });
  return new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      resolve({
        server,
        origin: `http://127.0.0.1:${address.port}`,
        close: () =>
          new Promise((done) => {
            server.close(() => done());
          }),
      });
    });
    server.on("error", reject);
  });
}

async function loadPlaywright(repoRoot) {
  const candidates = [
    path.join(
      repoRoot,
      "packages",
      "playground",
      "node_modules",
      "@playwright",
      "test",
      "index.js",
    ),
    path.join(repoRoot, "packages", "playground", "node_modules", "playwright", "index.js"),
    path.join(repoRoot, "node_modules", "@playwright", "test", "index.js"),
    path.join(repoRoot, "node_modules", "playwright", "index.js"),
  ];
  for (const candidate of candidates) {
    if (!existsSync(candidate)) continue;
    return import(pathToFileURL(candidate).href);
  }
  return null;
}

/** `@playwright/test` is CJS; `import(fileURL)` exposes launchers on `ns.default`, not the namespace. */
export function playwrightEngine(playwrightModule, engine) {
  if (playwrightModule == null) return undefined;
  return playwrightModule[engine] ?? playwrightModule.default?.[engine];
}

export async function defaultRunBrowsers({ origin, engines = ENGINES, playwrightModule }) {
  if (!playwrightModule) {
    return {
      ok: false,
      failures: ["playwright is not installed (Chromium/Firefox/WebKit required)"],
    };
  }
  const results = {};
  const failures = [];
  for (const engine of engines) {
    const launcher = playwrightEngine(playwrightModule, engine);
    if (typeof launcher?.launch !== "function") {
      failures.push(`${engine}: playwright launcher missing`);
      continue;
    }
    let browser;
    try {
      browser = await launcher.launch({ headless: true });
      const page = await browser.newPage();
      await page.goto(`${origin}/harness.html?engine=${engine}`, { waitUntil: "load" });
      const result = await page.waitForFunction(
        () => window.__bwh1 && window.__bwh1.pending === false,
        {
          timeout: 120_000,
        },
      );
      results[engine] = await result.jsonValue();
    } catch (error) {
      failures.push(`${engine}: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      if (browser) await browser.close();
    }
  }
  return { ok: failures.length === 0, results, failures };
}

export async function runGate({
  repoRoot = defaultRepoRoot,
  skipBuild = false,
  spawn = defaultSpawn,
  runBrowsers = defaultRunBrowsers,
  playwrightModule = undefined,
} = {}) {
  const failures = [];
  const wasmJs = path.join(repoRoot, WASM_JS_REL);
  const wasmBin = path.join(repoRoot, WASM_BIN_REL);
  const hostSrc = path.join(repoRoot, HARNESS_REL);
  const wasmDir = path.dirname(wasmJs);

  if (!skipBuild) {
    log("building @verter/wasm (bindgen, not wasm-opt)");
    const build = spawnPnpm(spawn, ["--filter", "@verter/wasm", "build:dev"], { cwd: repoRoot });
    if ((build.status ?? 1) !== 0) {
      return {
        ok: false,
        failures: [`wasm build failed: ${spawnFailureDetail(build)}`],
      };
    }
  }

  if (!existsSync(wasmBin) || !existsSync(wasmJs)) {
    return {
      ok: false,
      failures: [`WASM artifact missing at ${WASM_BIN_REL} (build it or drop --skip-build)`],
    };
  }
  const wasmBytes = statSync(wasmBin).size;
  if (wasmBytes < 1000) {
    return { ok: false, failures: [`WASM artifact too small (${wasmBytes} bytes)`] };
  }

  const missingExports = missingArtifactExports(readFileSync(wasmJs, "utf8"));
  if (missingExports.length > 0) {
    failures.push(`missing WASM exports/methods: ${missingExports.join(", ")}`);
  }

  log(`cargo tree census (${FORBIDDEN_NATIVE_CRATES.join(", ")} must be absent)`);
  const tree = spawn(
    "cargo",
    [
      "tree",
      "--target",
      "wasm32-unknown-unknown",
      "-p",
      "verter_wasm",
      "-e",
      "normal",
      "--prefix",
      "none",
    ],
    { cwd: repoRoot },
  );
  if ((tree.status ?? 1) !== 0) {
    failures.push(
      `cargo tree wasm32 census failed: ${(tree.stderr || tree.stdout || "").slice(-1500)}`,
    );
  } else {
    const hits = censusForbiddenHits(`${tree.stdout ?? ""}\n${tree.stderr ?? ""}`);
    if (hits.length > 0) {
      failures.push(
        `native process import in wasm32 closure: ${hits.join(", ")} (BWH0-AC1 / BWH1 census)`,
      );
    }
  }

  log(`native probe cargo test -p verter_wasm --lib ${NATIVE_PROBE_TEST}`);
  const native = spawn(
    "cargo",
    ["test", "-p", "verter_wasm", "--lib", NATIVE_PROBE_TEST, "--", "--exact", "--nocapture"],
    { cwd: repoRoot },
  );
  if ((native.status ?? 1) !== 0) {
    return {
      ok: false,
      failures: [
        ...failures,
        `native probe failed: ${(native.stderr || native.stdout || "").slice(-2000)}`,
      ],
    };
  }
  const parsed = parseNativeReceipt(`${native.stdout ?? ""}\n${native.stderr ?? ""}`);
  if (!parsed.ok) {
    return { ok: false, failures: [...failures, parsed.reason] };
  }
  failures.push(
    ...emptyOperationFailures(parsed.receipt.operations).map((reason) => `native: ${reason}`),
  );

  let playwright = playwrightModule;
  if (playwright === undefined) {
    playwright = await loadPlaywright(repoRoot);
  }
  if (!playwright) {
    return {
      ok: false,
      failures: [...failures, "playwright is not resolvable; Chromium/Firefox/WebKit are required"],
    };
  }

  const http = await serveHarness({ hostSrc, wasmDir });
  let browserRun;
  try {
    log(`browser probes on ${http.origin} (${ENGINES.join(", ")})`);
    browserRun = await runBrowsers({
      origin: http.origin,
      engines: ENGINES,
      playwrightModule: playwright,
    });
  } finally {
    await http.close();
  }
  if (!browserRun.ok) {
    failures.push(...(browserRun.failures ?? ["browser run failed"]));
  }
  const executed = Object.keys(browserRun.results ?? {});
  if (executed.length === 0) {
    failures.push("no browser executed the worker (pass-with-no-tests is rejected)");
  }
  for (const engine of ENGINES) {
    const result = browserRun.results?.[engine];
    if (!result) {
      failures.push(`${engine}: missing worker execution`);
      continue;
    }
    failures.push(...evaluateBrowserResult(engine, result));
    failures.push(...compareIdentities(parsed.receipt, result));
  }

  const metrics = {
    artifactBytes: wasmBytes,
    engines: {},
  };
  for (const engine of ENGINES) {
    const result = browserRun.results?.[engine];
    metrics.engines[engine] = {
      startupMs: result?.startupMs ?? null,
      memory: result?.memory ?? { retainedBytes: null, unavailable: "engine did not report" },
    };
  }

  if (failures.length > 0) return { ok: false, failures, metrics };
  return {
    ok: true,
    summary: {
      artifactBytes: wasmBytes,
      engines: ENGINES,
      operations: REQUIRED_OPERATIONS,
      metrics,
    },
  };
}

async function main() {
  const argv = process.argv.slice(2);
  const args = { root: defaultRepoRoot, skipBuild: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--root") args.root = path.resolve(argv[++i]);
    else if (argv[i] === "--skip-build") args.skipBuild = true;
    else {
      process.stderr.write(`browser-host-gate: unknown argument '${argv[i]}'\n`);
      process.exit(2);
    }
  }
  const outcome = await runGate({ repoRoot: args.root, skipBuild: args.skipBuild });
  if (outcome.ok) {
    process.stdout.write(`browser-host-gate: PASS ${JSON.stringify(outcome.summary)}\n`);
    return;
  }
  for (const failure of outcome.failures) {
    process.stderr.write(`browser-host-gate: FAIL — ${failure}\n`);
  }
  process.exit(1);
}

if (
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main();
}
