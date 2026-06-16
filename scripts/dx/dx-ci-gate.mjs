/**
 * DX harness strict-baseline CI gate.
 *
 * This is the non-vacuous half of the required raw-LSP hermetic DX job: it drives
 * the REAL `verter-dx-baseline` bridge against the committed, vendored hermetic
 * corpus in STRICT mode and refuses to pass unless the differential baseline
 * actually ran. Four properties are asserted, each as a hard failure (exit 1):
 *
 *   1. Pinned tool root (P2-A) — `repoRoot` / `tsserverTsdk` / `expectedTsserverJs`
 *      / TypeScript version are all present, the pinned `tsserver.js` exists on
 *      disk, and it lives UNDER the repository (never an ambient/global-npm
 *      TypeScript). The live strict bridge then reports the SAME pinned path back
 *      as `baselineToolRootUsed`, proving a real provider matched the pin rather
 *      than drifting onto a discovered one.
 *   2. Vendored Vue declaration version-sync (P3-3) — every vendored `vue`/`@vue/*`
 *      package carries the pinned line AND that line equals the Vue version the
 *      repo resolves for its provider-facing tests, so the hermetic differential
 *      can never silently run against Vue declarations different from the ones the
 *      provider resolves.
 *   3. Versioned per-edit artifact sync (P2-B) — a probe requested at version V is
 *      REFUSED (`baseline_artifact_stale`, carrying the requested/have versions)
 *      when the bridge only holds artifacts for an earlier version, so the
 *      differential never compares verter@editN against baseline@edit0.
 *   4. `baseline-ran > 0` — the strict bridge executed at least one real provider
 *      probe over the materialized corpus; a zero count fails the job.
 *
 * Every semantic decision is delegated to the shared harness API
 * (`@verter/dx-harness`) and the real bridge binary; this gate only orchestrates
 * and asserts. It is hermetic: the provider is the repo-pinned `tsserver`, the
 * corpus is committed, and the vendored Vue shims stand in for an install.
 *
 * Inputs (env):
 *   - DX_BASELINE_BIN  (required) absolute path to the built `verter-dx-baseline`.
 *   - DX_REPO_ROOT     (optional) repo root; defaults to this script's repo root.
 *   - DX_FIXTURE_DIR   (optional) corpus dir to materialize + probe; defaults to the
 *                      committed vendored hermetic scenario. The scheduled
 *                      external-corpus lane points this at a non-vendored corpus.
 *   - DX_ALLOW_COMPILE_ERRORS (optional) "1" downgrades materialize compile errors
 *                      to a warning — for external corpora that pull third-party
 *                      deps absent from the vendored shims. The required vendored
 *                      gate never sets it, so compile errors stay fatal there.
 */

import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HARNESS_DIST = new URL("../../packages/dx-harness/dist/index.js", import.meta.url);

const {
  resolveToolRoots,
  canonicalizePath,
  VENDORED_VUE_VERSION,
  collectVuePackageVersions,
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
  BridgeClient,
} = await import(HARNESS_DIST.href);

/** A single failed gate check; collected so the run reports every violation. */
const failures = [];
function check(ok, message) {
  if (!ok) failures.push(message);
  return ok;
}
function fatal(message) {
  console.error(`::error::DX strict-baseline gate: ${message}`);
  process.exit(1);
}

const repoRoot = canonicalizePath(
  process.env.DX_REPO_ROOT ?? fileURLToPath(new URL("../../", import.meta.url)),
).replace(/\/+$/, "");
const baselineBin = process.env.DX_BASELINE_BIN;
if (!baselineBin) {
  fatal("DX_BASELINE_BIN is required (absolute path to the built verter-dx-baseline binary)");
}
if (!existsSync(baselineBin)) {
  fatal(`DX_BASELINE_BIN does not exist: ${baselineBin}`);
}

// The committed, vendored hermetic corpus scenario the strict baseline runs over.
// A single self-contained `<script setup>` SFC (no imports) so the materialized
// TSX type-checks against the vendored Vue shims with no fixture install. The
// scheduled external-corpus lane overrides this with DX_FIXTURE_DIR.
const fixtureDir =
  process.env.DX_FIXTURE_DIR ??
  path.join(repoRoot, "packages", "dx-harness", "fixtures", "hermetic", "minimal-member-access");
if (!existsSync(fixtureDir)) {
  fatal(`hermetic corpus fixture missing: ${fixtureDir}`);
}
const allowCompileErrors = process.env.DX_ALLOW_COMPILE_ERRORS === "1";

// ── 1. Pinned tool root (P2-A), static half ──────────────────────────────────
const toolRoots = resolveToolRoots(repoRoot);
check(!!toolRoots.repoRoot, "toolRoot.repoRoot is empty");
check(!!toolRoots.tsserverTsdk, "toolRoot.tsserverTsdk is empty");
check(!!toolRoots.expectedTsserverJs, "toolRoot.expectedTsserverJs is empty");
check(
  typeof toolRoots.tsserverVersion === "string" && toolRoots.tsserverVersion.length > 0,
  "toolRoot.tsserverVersion is absent (pinned TypeScript not resolvable)",
);
check(
  !!toolRoots.expectedTsserverJs && existsSync(toolRoots.expectedTsserverJs),
  `pinned tsserver.js does not exist on disk: ${toolRoots.expectedTsserverJs}`,
);
// The pin must live UNDER the repository — never an ambient/global-npm TypeScript.
const expectedCanon = toolRoots.expectedTsserverJs
  ? canonicalizePath(toolRoots.expectedTsserverJs)
  : "";
check(
  expectedCanon.startsWith(`${repoRoot}/`),
  `pinned tsserver.js is not under the repo root (ambient/global fallback?): ${expectedCanon} not under ${repoRoot}`,
);

// ── 2. Vendored Vue declaration version-sync (P3-3) ──────────────────────────
const vuePkgs = collectVuePackageVersions();
check(vuePkgs.length > 0, "no vendored vue/@vue packages found");
for (const { package: pkg, version } of vuePkgs) {
  check(
    version === VENDORED_VUE_VERSION,
    `vendored ${pkg}@${version} != pinned vendored Vue line ${VENDORED_VUE_VERSION}`,
  );
}
// The vendored line must equal the Vue version the repo resolves for its
// provider-facing tests (packages/benchmark). Otherwise the hermetic differential
// runs against Vue declarations different from the ones the provider resolves.
let resolvedRepoVue;
try {
  const benchRequire = createRequire(path.join(repoRoot, "packages", "benchmark", "package.json"));
  resolvedRepoVue = JSON.parse(
    readFileSync(benchRequire.resolve("vue/package.json"), "utf-8"),
  ).version;
} catch (err) {
  resolvedRepoVue = `<unresolved: ${err instanceof Error ? err.message : String(err)}>`;
}
check(
  resolvedRepoVue === VENDORED_VUE_VERSION,
  `vendored Vue line ${VENDORED_VUE_VERSION} != repo-resolved provider-facing Vue ${resolvedRepoVue} (packages/benchmark)`,
);

if (failures.length > 0) {
  for (const f of failures) console.error(`::error::DX strict-baseline gate: ${f}`);
  fatal(`${failures.length} static check(s) failed`);
}

// ── 3 + 4. Live strict bridge over the vendored corpus ───────────────────────
const ws = await createMaterializedWorkspace({
  fixtureDir,
  repoRoot,
  typeProvider: "tsserver",
  baselineBin,
  // Strict-by-default: a vendored-Vue drift hard-fails materialization too.
});

let baselineRan = -1;
try {
  if (allowCompileErrors && ws.materializeReport.compileErrors.length > 0) {
    console.warn(
      `::warning::DX strict-baseline gate: tolerating ${ws.materializeReport.compileErrors.length} compile error(s) (DX_ALLOW_COMPILE_ERRORS=1)`,
    );
  } else {
    check(
      ws.materializeReport.compileErrors.length === 0,
      `materialize reported compile errors: ${JSON.stringify(ws.materializeReport.compileErrors)}`,
    );
  }
  check(
    ws.materializeReport.vueVersionWarnings.length === 0,
    `materialize reported vendored-Vue version warnings: ${JSON.stringify(ws.materializeReport.vueVersionWarnings)}`,
  );
  const entry = ws.materializeReport.ideArtifacts[0];
  if (!check(!!entry, "materialize produced no IDE artifact for the corpus entry")) {
    fatal("no IDE artifact to probe");
  }

  const client = new BridgeClient(baselineBin, { requestTimeoutMs: 120_000 });
  try {
    // Strict hello against the pinned tsserver tool root.
    const hello = await client.hello({
      workspaceRoot: ws.root,
      repoRoot,
      provider: "tsserver",
      strictCi: true,
      toolRoot: {
        tsserverTsdk: toolRoots.tsserverTsdk,
        expectedTsserverJs: toolRoots.expectedTsserverJs,
        tsserverVersion: toolRoots.tsserverVersion,
      },
    });
    check(hello.type === "hello", `hello failed: ${JSON.stringify(hello)}`);
    if (hello.type === "hello") {
      check(hello.ok === true, "strict hello not ok");
      check(hello.skipped === false, `strict hello SKIPPED (must not skip): ${hello.skipReason}`);
      // The live provider must report the SAME pinned path — proof it matched the
      // pin rather than drifting onto a discovered/ambient tsserver.
      check(
        !!hello.baselineToolRootUsed &&
          canonicalizePath(hello.baselineToolRootUsed) === expectedCanon,
        `strict hello tool root ${hello.baselineToolRootUsed} != pinned ${expectedCanon}`,
      );
    }

    // Open the materialized artifacts (entry .vue.tsx + .vue.ts public-API twins).
    const files = [];
    for (const a of ws.materializeReport.ideArtifacts) {
      files.push({
        path: a.generatedPath,
        content: readFileSync(a.generatedPath, "utf-8"),
        role: "entry",
      });
    }
    for (const a of ws.materializeReport.publicApiTwins) {
      files.push({
        path: a.generatedPath,
        content: readFileSync(a.generatedPath, "utf-8"),
        role: "api",
      });
    }
    const opened = await client.open(files, 1);
    check(opened.type === "open", `open failed: ${JSON.stringify(opened)}`);

    // A fresh probe at the opened version runs against the provider (a real
    // baseline run). Diagnostics is whole-file, so no cursor-offset fragility.
    const diag = await client.diagnostics({
      uri: entry.sourceVue,
      path: entry.generatedPath,
      version: 1,
    });
    check(
      diag.type === "diagnostics",
      `fresh diagnostics probe did not run: ${JSON.stringify(diag)}`,
    );

    // P2-B: a probe at a NEWER version than the synced overlay must be refused as
    // stale (never compare verter@editN with baseline@edit0).
    const stale = await client.query({
      method: "hover",
      uri: entry.sourceVue,
      path: entry.generatedPath,
      offset: 0,
      version: 2,
    });
    check(
      stale.type === "error" &&
        stale.kind === "baseline_artifact_stale" &&
        stale.requestedVersion === 2 &&
        stale.haveVersion === 1,
      `stale-version probe was not refused as baseline_artifact_stale: ${JSON.stringify(stale)}`,
    );

    const bye = await client.shutdown();
    check(bye.type === "shutdown", `shutdown failed: ${JSON.stringify(bye)}`);
    if (bye.type === "shutdown") baselineRan = bye.baselineRan;
  } finally {
    await client.dispose();
  }
} finally {
  disposeMaterializedWorkspace(ws);
}

// ── 4. baseline-ran > 0 ──────────────────────────────────────────────────────
check(baselineRan > 0, `baseline-ran is ${baselineRan} (must be > 0)`);

if (failures.length > 0) {
  for (const f of failures) console.error(`::error::DX strict-baseline gate: ${f}`);
  fatal(`${failures.length} check(s) failed`);
}

console.log(`baseline-ran: ${baselineRan} probes`);
console.log("DX strict-baseline gate: PASS");
