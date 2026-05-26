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
 * at the fixture's `uiRoot`. `NapiWorkspace::new` is lazy by design
 * (auto-discovering tsconfigs on construction was tried in 9ae1171b8
 * and reverted after a 3.6x bench regression on `repo_first_pass`).
 * The harness therefore explicitly mirrors the
 * `@verter/component-meta/compat/checker.ts:2265` pattern: parse the
 * project's tsconfig chain into an alias map and install it via
 * `workspace.configureProjects(...)` BEFORE any `ensureLoaded` /
 * `upsertBase` / `getComponentMetaWithAudit` call. With the alias map
 * installed, `Engine::resolve_import` walks the populated
 * `ProjectGraph` and reaches the pnpm-aware resolver — components
 * that import types from other files (the majority of real nuxt-ui
 * components) get a full audit bundle instead of a degenerate
 * single-file view.
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

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";
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

/**
 * Read a tsconfig file and return its parsed compilerOptions
 * (best-effort; tolerates JSONC comments). Returns `null` when the
 * file does not exist or cannot be parsed.
 */
function readTsconfigCompilerOptions(
  tsconfigPath: string,
): { baseUrl?: string; paths?: Record<string, string[]> } | null {
  if (!existsSync(tsconfigPath)) {
    return null;
  }
  let raw: string;
  try {
    raw = readFileSync(tsconfigPath, "utf8");
  } catch {
    return null;
  }
  try {
    const stripped = raw.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
    const parsed = JSON.parse(stripped) as { compilerOptions?: Record<string, unknown> };
    const opts = (parsed.compilerOptions ?? {}) as Record<string, unknown>;
    return {
      baseUrl: typeof opts.baseUrl === "string" ? opts.baseUrl : undefined,
      paths: (opts.paths ?? undefined) as Record<string, string[]> | undefined,
    };
  } catch {
    return null;
  }
}

/**
 * Install the project's path-alias map on the workspace so
 * `Engine::resolve_import` can route through the pnpm-aware resolver.
 *
 * Mirrors `@verter/component-meta/compat/checker.ts:2265`
 * (`extractPathAliases` + `workspace.configureProjects([aliases])`)
 * but reads the tsconfig directly so the audit harness has no compat
 * dependency. For Nuxt projects, prefers the generated
 * `.nuxt/tsconfig.app.json` + `.nuxt/tsconfig.shared.json` (where the
 * real alias map lives); falls back to a top-level `tsconfig.json`
 * for non-Nuxt fixtures (e.g. the discriminator spec).
 */
function configureWorkspaceProjects(
  workspace: { configureProjects: (configs: unknown[]) => void },
  uiRoot: string,
): void {
  // 1) Prefer the Nuxt-generated tsconfigs — they carry the real
  //    pnpm-aware alias maps. The bench's `readNuxtCompilerOptions`
  //    in `meta-ui-bench.ts` uses the same pair.
  const nuxtAppTsconfig = resolve(uiRoot, ".nuxt", "tsconfig.app.json");
  const nuxtSharedTsconfig = resolve(uiRoot, ".nuxt", "tsconfig.shared.json");
  const appOpts = readTsconfigCompilerOptions(nuxtAppTsconfig);
  const sharedOpts = readTsconfigCompilerOptions(nuxtSharedTsconfig);

  let baseUrl: string | undefined;
  let mergedPaths: Record<string, string[]> = {};
  if (appOpts || sharedOpts) {
    baseUrl = resolve(uiRoot, ".nuxt").replace(/\\/g, "/");
    mergedPaths = {
      ...(appOpts?.paths ?? {}),
      ...(sharedOpts?.paths ?? {}),
    };
  } else {
    // 2) Fall back to the top-level tsconfig.json for non-Nuxt fixtures.
    const topTsconfig = resolve(uiRoot, "tsconfig.json");
    const opts = readTsconfigCompilerOptions(topTsconfig);
    if (opts) {
      baseUrl = opts.baseUrl;
      mergedPaths = opts.paths ?? {};
    }
  }

  const pathsArray = Object.entries(mergedPaths).map(([pattern, targets]) => ({
    pattern,
    targets,
  }));

  const normalizedRoot = uiRoot.replace(/\\/g, "/");
  workspace.configureProjects([
    {
      root: normalizedRoot,
      workspaceRoot: normalizedRoot,
      compilerOptions: {
        baseUrl,
        paths: pathsArray.length > 0 ? pathsArray : undefined,
      },
    },
  ]);
}

const uiRoot = getDefaultUiRoot(import.meta.dirname);
let componentFile: string;
try {
  componentFile = resolveComponentFile(componentToken, { uiRoot }).replace(/\\/g, "/");
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`ERROR: ${message}`);
  process.exit(2);
}

// Use the absolute forward-slash path as the canonical id — the same
// shape the benchmark and LSP both produce (see
// `meta-ui-bench.ts:componentSnapshots.absolutePath` and the LSP's
// `uri_to_canonical_id_from_str`). The earlier root-relative form
// (`"/" + relative(uiRoot, componentFile)`) does not match the
// absolute project roots configured anywhere else in Verter and
// short-circuited `Engine::resolve_import` against an unrelated
// canonical, producing a degenerate empty-topology audit.
const canonical = componentFile;

// Workspace-backed host: `new native.Workspace([uiRoot])` is lazy by
// design (eager auto-discovery was reverted from 9ae1171b8 after a
// 3.6x bench regression). Mirror `compat/checker.ts:2265` exactly:
// build an alias map from the project's tsconfig chain and install
// it via `workspace.configureProjects(...)` BEFORE the first
// `ensureLoaded` / `upsertBase` / `getComponentMetaWithAudit` call.
// With the alias map installed, `Engine::resolve_import` walks the
// populated `ProjectGraph` and reaches the pnpm-aware resolver.
const workspace = new native.Workspace([uiRoot]);
configureWorkspaceProjects(workspace, uiRoot);
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
