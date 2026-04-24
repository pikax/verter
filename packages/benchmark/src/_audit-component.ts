/**
 * Audit-emitting per-component worker. Plan §3 Commit 10 (F8).
 *
 * Spawned per-component by `corpus-trace-runner.ts` (and orchestrated
 * by `scripts/benchmark/trace-component-corpus.mjs`). Drives the NAPI
 * `getComponentMetaWithAudit` binding directly — NO compat layer, NO
 * regex-validator coupling — and emits both the `RustAuditRecord`
 * JSON and the accompanying `ComponentMetaAnalysis` JSON to disk so
 * the parent can hand them to `audit-validator.ts`.
 *
 * ## Multi-file dependency handling
 *
 * The worker backs the `ComponentMetaHost` with a `Workspace` rooted
 * at the fixture's `uiRoot`. The workspace auto-discovers `tsconfig`
 * files, builds the project graph, and resolves imports against the
 * real dependency tree — so components that import types from other
 * files (the majority of real nuxt-ui components) get a full audit
 * bundle instead of a degenerate single-file view. `ensureLoaded()`
 * pulls the target canonical through the workspace path before the
 * audit call.
 *
 * ## Env contract
 *
 * - `VERTER_COMPONENT_META_AUDIT_PATH` (required) — destination for
 *   `JSON.stringify(record)`.
 * - `VERTER_COMPONENT_META_ANALYSIS_PATH` (required) — destination
 *   for `JSON.stringify(analysis)`.
 * - `VERTER_COMPONENT_META_RESULT_PATH` (optional) — back-compat with
 *   the legacy normalized-artifact path used by the meta-ui benches.
 *   When set, the worker also writes the normalized artifact via
 *   `normalizeComponentMetaArtifact`.
 *
 * Stdout contract: writes a single `Done in <ms>ms ...` line on
 * success and a `Closed` line on shutdown so
 * `parseStdoutFields()` in the runner classifies the run.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { relative } from "node:path";
import { performance } from "node:perf_hooks";

import {
  normalizeComponentMetaArtifact,
  writeNormalizedComponentMetaArtifact,
} from "./component-meta-artifact.js";
import { getDefaultUiRoot, resolveComponentFile } from "./trace-component-resolver.js";

const componentToken = process.argv[2];
if (!componentToken) {
  console.error("Usage: tsx src/_audit-component.ts <ComponentPathOrName>");
  process.exit(1);
}

const auditPath = process.env.VERTER_COMPONENT_META_AUDIT_PATH;
const analysisPath = process.env.VERTER_COMPONENT_META_ANALYSIS_PATH;
if (!auditPath) {
  console.error("FATAL: VERTER_COMPONENT_META_AUDIT_PATH is required");
  process.exit(2);
}
if (!analysisPath) {
  console.error("FATAL: VERTER_COMPONENT_META_ANALYSIS_PATH is required");
  process.exit(2);
}

// `@verter/native` is a CommonJS package — load via `createRequire`
// so the audit worker compiles under tsx without ESM interop noise.
const { createRequire } = await import("node:module");
const requireFromHere = createRequire(import.meta.url);
const native = requireFromHere("@verter/native");

const uiRoot = getDefaultUiRoot(import.meta.dirname);
let componentFile: string;
try {
  componentFile = resolveComponentFile(componentToken, { uiRoot }).replace(/\\/g, "/");
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`ERROR: ${message}`);
  process.exit(2);
}

const canonical = "/" + relative(uiRoot, componentFile).replace(/\\/g, "/");

// Workspace-backed host: auto-discovers tsconfig + builds project
// graph, so cross-file imports resolve against the real dependency
// tree. `ensureLoaded` pulls the target file through the workspace
// before the audit request; if the workspace misses (rare — e.g.
// the fixture has no matching tsconfig), we fall back to a direct
// source upsert so the worker still produces an audit record.
const workspace = new native.Workspace([uiRoot]);
const project = native.ComponentMetaHost.withWorkspace(
  { auditEnabled: true, footprintCapture: true },
  workspace,
);
const loaded = project.ensureLoaded(canonical);
if (!loaded) {
  const source = readFileSync(componentFile, "utf-8");
  project.upsertBase(canonical, source);
}

const session = project.openSession();

const start = performance.now();
const buffer: Buffer | null = session.getComponentMetaWithAudit(canonical);
const queryMs = Math.round(performance.now() - start);

if (buffer === null) {
  console.error(`ERROR: getComponentMetaWithAudit returned null for ${canonical}`);
  session.close();
  project.shutdown();
  process.exit(3);
}

const bundle = JSON.parse(buffer.toString("utf-8")) as {
  analysis: unknown;
  resolution: unknown;
  record: unknown;
};

writeFileSync(auditPath, JSON.stringify(bundle.record), "utf-8");
writeFileSync(analysisPath, JSON.stringify(bundle.analysis), "utf-8");

const legacyResultPath = process.env.VERTER_COMPONENT_META_RESULT_PATH;
if (legacyResultPath) {
  const componentRel = relative(uiRoot, componentFile).replace(/\\/g, "/");
  const artifact = normalizeComponentMetaArtifact(componentRel, bundle.analysis);
  writeNormalizedComponentMetaArtifact(legacyResultPath, artifact);
}

const propsCount = (bundle.analysis as { props?: unknown[] } | null)?.props?.length ?? 0;
console.log(`Done in ${queryMs}ms (${propsCount} props) audit=true setup=0ms`);

session.close();
project.shutdown();
console.log("Closed");
