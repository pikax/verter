/**
 * Audit-emitting per-component worker.
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
 * - `VERTER_COMPONENT_META_FOCUSED_JSONL_PATH` (optional) —
 *   path to a JSONL file where the worker APPENDS one line carrying
 *   the focused-counter slice for the audited component. The line is
 *   a self-contained JSON object: `{ component, queryMs, counters }`
 *   where `counters` is the focused slice (semantic-query, substitute,
 *   build_typeof, prepared_decl_bundle, cache_outcomes, truncation).
 *   This is the bench-side telemetry surface for focused investigation.
 *
 * Stdout contract: writes a single `Done in <ms>ms ...` line on
 * success and a `Closed` line on shutdown so
 * `parseStdoutFields()` in the runner classifies the run.
 */

import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
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

// Focused telemetry — when the focused-JSONL path is configured, emit
// a self-contained one-line JSON slice with the focused
// counter set: semantic-query cold/warm per kind, substitute
// telemetry, build_typeof telemetry, prepared-decl-bundle per-rejection
// counters, cache_outcomes, truncation_counters. The shape is
// intentionally small (~5-10 KB per component) so a corpus run does
// not OOM the harness like the full per-component audit JSON.
const focusedJsonlPath = process.env.VERTER_COMPONENT_META_FOCUSED_JSONL_PATH;
if (focusedJsonlPath) {
  type RustAuditRecord = {
    request_id?: number;
    footprint?: {
      cache_outcomes?: Record<string, number>;
      resolver_hot_path?: Record<string, number>;
      truncation_counters?: Record<string, number | string>;
      materializations?: unknown[];
      instantiations?: unknown[];
      substitutions?: unknown[];
      projections?: unknown[];
      conditional_decisions?: unknown[];
    };
    timings?: {
      total_ms?: number;
      materialize_ms?: number;
      solver_ms?: number;
      store_read_ms?: number;
      store_merge_ms?: number;
    };
  };
  const record = bundle.record as RustAuditRecord;
  const sf = record.footprint ?? {};
  const rhp = sf.resolver_hot_path ?? {};
  const co = sf.cache_outcomes ?? {};
  const tc = sf.truncation_counters ?? {};
  const pt = record.timings ?? {};
  const componentRelForJsonl = relative(uiRoot, componentFile).replace(/\\/g, "/");
  const componentNameForJsonl = componentRelForJsonl.split("/").pop() ?? componentRelForJsonl;
  const line = {
    component: componentNameForJsonl,
    componentRel: componentRelForJsonl,
    queryMs,
    propsCount: (bundle.analysis as { props?: unknown[] } | null)?.props?.length ?? 0,
    // Focused counter slice — ALL fields keyed to the Rust
    // `ResolverHotPathCounters` (snake_case from serde) so the JSONL
    // line is stable across regens.
    counters: {
      // Semantic query dispatches by kind (cold = build closure ran;
      // warm = memo short-circuit).
      semantic_query_typeof_cold: rhp.semantic_query_typeof_cold ?? 0,
      semantic_query_typeof_warm: rhp.semantic_query_typeof_warm ?? 0,
      semantic_query_instantiate_cold: rhp.semantic_query_instantiate_cold ?? 0,
      semantic_query_instantiate_warm: rhp.semantic_query_instantiate_warm ?? 0,
      semantic_query_conditional_cold: rhp.semantic_query_conditional_cold ?? 0,
      semantic_query_conditional_warm: rhp.semantic_query_conditional_warm ?? 0,
      semantic_query_mapped_type_cold: rhp.semantic_query_mapped_type_cold ?? 0,
      semantic_query_mapped_type_warm: rhp.semantic_query_mapped_type_warm ?? 0,
      semantic_query_indexed_access_cold: rhp.semantic_query_indexed_access_cold ?? 0,
      semantic_query_indexed_access_warm: rhp.semantic_query_indexed_access_warm ?? 0,
      semantic_query_keyof_cold: rhp.semantic_query_keyof_cold ?? 0,
      semantic_query_keyof_warm: rhp.semantic_query_keyof_warm ?? 0,
      semantic_query_project_path_cold: rhp.semantic_query_project_path_cold ?? 0,
      semantic_query_project_path_warm: rhp.semantic_query_project_path_warm ?? 0,
      // Substitute telemetry.
      substitute_top_level_calls: rhp.substitute_top_level_calls ?? 0,
      substitute_memo_hits: rhp.substitute_memo_hits ?? 0,
      substitute_typeof_opaque: rhp.substitute_typeof_opaque ?? 0,
      substitute_conditional_descend: rhp.substitute_conditional_descend ?? 0,
      substitute_mapped_type_descend: rhp.substitute_mapped_type_descend ?? 0,
      // build_typeof telemetry.
      build_typeof_calls: rhp.build_typeof_calls ?? 0,
      build_typeof_prepared_value_misses: rhp.build_typeof_prepared_value_misses ?? 0,
      // Focused mapped-member materialization counters.
      mapped_member_plain_unique: rhp.mapped_member_plain_unique ?? 0,
      mapped_member_plain_repeated: rhp.mapped_member_plain_repeated ?? 0,
      mapped_member_selected_key_unique: rhp.mapped_member_selected_key_unique ?? 0,
      mapped_member_selected_key_repeated: rhp.mapped_member_selected_key_repeated ?? 0,
      prepared_decl_bundle_callsite_scope_payload:
        rhp.prepared_decl_bundle_callsite_scope_payload ?? 0,
      prepared_decl_bundle_callsite_build_instantiate:
        rhp.prepared_decl_bundle_callsite_build_instantiate ?? 0,
      prepared_decl_bundle_callsite_other: rhp.prepared_decl_bundle_callsite_other ?? 0,
      mapped_binder_ordinal_collision: rhp.mapped_binder_ordinal_collision ?? 0,
      // Focused recursive-substitution counters.
      recursive_substitute_unique: rhp.recursive_substitute_unique ?? 0,
      recursive_substitute_repeated: rhp.recursive_substitute_repeated ?? 0,
      substitute_mapped_rebuild: rhp.substitute_mapped_rebuild ?? 0,
      substitute_conditional_rebuild: rhp.substitute_conditional_rebuild ?? 0,
      recursive_substitute_memo_hits: rhp.recursive_substitute_memo_hits ?? 0,
      // Prepared-decl-bundle per-rejection counter set.
      prepared_decl_bundle_cold: rhp.prepared_decl_bundle_cold ?? 0,
      prepared_decl_bundle_warm: rhp.prepared_decl_bundle_warm ?? 0,
      prepared_decl_bundle_reject_entry_missing: rhp.prepared_decl_bundle_reject_entry_missing ?? 0,
      prepared_decl_bundle_reject_self_root_untracked:
        rhp.prepared_decl_bundle_reject_self_root_untracked ?? 0,
      prepared_decl_bundle_reject_self_root_hash_mismatch:
        rhp.prepared_decl_bundle_reject_self_root_hash_mismatch ?? 0,
      prepared_decl_bundle_reject_import_route_absent:
        rhp.prepared_decl_bundle_reject_import_route_absent ?? 0,
      prepared_decl_bundle_reject_import_route_mismatch:
        rhp.prepared_decl_bundle_reject_import_route_mismatch ?? 0,
      prepared_decl_bundle_reject_other: rhp.prepared_decl_bundle_reject_other ?? 0,
      // Cache outcomes from per-context counters (exact).
      cache_outcomes_cold_builds: co.cold_builds ?? 0,
      cache_outcomes_warm_hits: co.warm_hits ?? 0,
      cache_outcomes_joined_waits: co.joined_waits ?? 0,
      cache_outcomes_sentinels: co.sentinels ?? 0,
      // Vector lane lengths (filtered after caps applied at accumulator).
      materializations_count: sf.materializations?.length ?? 0,
      instantiations_count: sf.instantiations?.length ?? 0,
      substitutions_count: sf.substitutions?.length ?? 0,
      projections_count: sf.projections?.length ?? 0,
      conditional_decisions_count: sf.conditional_decisions?.length ?? 0,
      // Truncation counters — non-zero = the cap was hit on that lane.
      // Decimal-string transport for u64; coerced to number for JSONL.
      truncation_structured_events: Number(tc.structured_events_truncated ?? 0),
      truncation_derivation_edges_raw: Number(tc.derivation_edges_raw_truncated ?? 0),
      truncation_derivation_nodes: Number(tc.derivation_nodes_truncated ?? 0),
      truncation_vfs_reads: Number(tc.vfs_reads_truncated ?? 0),
      truncation_indexed_ready_builds: Number(tc.indexed_ready_builds_truncated ?? 0),
      truncation_materializations: Number(tc.materializations_truncated ?? 0),
      truncation_instantiations: Number(tc.instantiations_truncated ?? 0),
      truncation_substitutions: Number(tc.substitutions_truncated ?? 0),
      truncation_projections: Number(tc.projections_truncated ?? 0),
      truncation_conditional_decisions: Number(tc.conditional_decisions_truncated ?? 0),
      truncation_alias_resolutions: Number(tc.alias_resolutions_truncated ?? 0),
      truncation_shared_load_reuses: Number(tc.shared_load_reuses_truncated ?? 0),
      // Time decomposition — present when the host emits per-phase timings.
      phase_total_ms: pt.total_ms ?? 0,
      phase_materialize_ms: pt.materialize_ms ?? 0,
      phase_solver_ms: pt.solver_ms ?? 0,
      phase_store_read_ms: pt.store_read_ms ?? 0,
      phase_store_merge_ms: pt.store_merge_ms ?? 0,
    },
  };
  appendFileSync(focusedJsonlPath, JSON.stringify(line) + "\n", "utf-8");
}

const propsCount = (bundle.analysis as { props?: unknown[] } | null)?.props?.length ?? 0;
console.log(`Done in ${queryMs}ms (${propsCount} props) audit=true setup=0ms`);

session.close();
project.shutdown();
console.log("Closed");
