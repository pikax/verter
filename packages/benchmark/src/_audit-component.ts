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
 * Env contract:
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
const source = readFileSync(componentFile, "utf-8");

const project = new native.ComponentMetaHost({
  auditEnabled: true,
  footprintCapture: true,
});
project.upsertBase(canonical, source);

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
