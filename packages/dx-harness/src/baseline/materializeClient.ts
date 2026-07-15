/**
 * Client for the `verter_dx_baseline materialize` one-shot.
 *
 * This is BRIDGE-CLIENT code, not a materializer: C owns all baseline
 * materialization (compile `.vue`→TSX, public-API twins, specifier rewrites,
 * source-map shifting, `sourceMapIdentity`, `@verter/types` injection, tsconfig
 * synthesis, vendored-shim copy, vendored-Vue version sync). B only builds the
 * request, spawns the one-shot, and parses the report DTO. The DTO field names
 * mirror `crates/verter_dx_baseline/src/main.rs` (`MaterializeCli` /
 * `MaterializeCliResult` / `ArtifactDto`) under serde `rename_all = "camelCase"`.
 *
 * The `sourceMap` on each artifact is ALREADY shifted by C to match the rewritten
 * `.vue`→`.vue.ts` generated code; B surfaces it verbatim and never recomputes it.
 */

import { runOneShot } from "./childProcess.js";

/** Inputs to a materialization request (B-facing). */
export interface MaterializeRequestInput {
  /** Absolute workspace root holding the stripped `.vue` sources. */
  workspaceRoot: string;
  /** Absolute entry `.vue` paths. */
  entries: string[];
  /** Absolute vendored `node_modules` directory (committed shims). */
  vendorNodeModules?: string;
  /** The resolved Vue line the vendored `vue`/`@vue/*` declarations must match. */
  expectedVueVersion?: string;
  /**
   * Whether a vendored-Vue version mismatch hard-fails. Defaults to `true` — the
   * B↔C contract requires strict vendored-Vue sync — so an unset caller gets the
   * hard-fail; pass `false` to downgrade drift to a warning.
   */
  strictVueVersion?: boolean;
}

/** The exact camelCase wire object the materialize CLI deserializes (`MaterializeCli`). */
export interface MaterializeWireRequest {
  workspaceRoot: string;
  entries: string[];
  strictVueVersion: boolean;
  vendorNodeModules?: string;
  expectedVueVersion?: string;
}

/** One emitted artifact (`ArtifactDto`). */
export interface MaterializeArtifact {
  /** Canonical authored `.vue` id this artifact derives from. */
  sourceVue: string;
  /** Generated artifact path on disk. */
  generatedPath: string;
  /** Whether the host produced a source map for it. */
  sourceMapPresent: boolean;
  /**
   * The artifact's V3 source map, ALREADY shifted by C to match the rewritten
   * generated code. Authoritative — surfaced verbatim, never recomputed by B.
   * Absent when the host produced no map.
   */
  sourceMap?: string;
}

/** A `.vue` that failed `ensure_compiled` (`CompileErrorDto`). */
export interface MaterializeCompileError {
  canonical: string;
  message: string;
}

/** A vendored-Vue declaration version mismatch recorded in non-strict mode. */
export interface VueVersionWarning {
  package: string;
  expected: string;
  found: string;
}

/** The parsed materialization report (`MaterializeCliResult`). */
export interface MaterializeResult {
  ideArtifacts: MaterializeArtifact[];
  publicApiTwins: MaterializeArtifact[];
  verterTypesDts: string | null;
  mapAbsent: string[];
  sourceMapIdentities: Record<string, string>;
  compileErrors: MaterializeCompileError[];
  tsconfigPath: string | null;
  synthesizedTsconfig: boolean;
  supportRewrites: string[];
  vueVersionWarnings: VueVersionWarning[];
}

/** Build the exact camelCase wire request, omitting unset optionals. */
export function buildMaterializeRequest(input: MaterializeRequestInput): MaterializeWireRequest {
  const req: MaterializeWireRequest = {
    workspaceRoot: input.workspaceRoot,
    entries: input.entries,
    // Strict-by-default: the B↔C contract hard-fails on vendored-Vue drift.
    strictVueVersion: input.strictVueVersion ?? true,
  };
  if (input.vendorNodeModules !== undefined) req.vendorNodeModules = input.vendorNodeModules;
  if (input.expectedVueVersion !== undefined) req.expectedVueVersion = input.expectedVueVersion;
  return req;
}

function isArray(v: unknown): v is unknown[] {
  return Array.isArray(v);
}

/** Parse the one-shot's stdout into a typed {@link MaterializeResult}. */
export function parseMaterializeResult(json: string): MaterializeResult {
  const raw: unknown = JSON.parse(json);
  if (raw === null || typeof raw !== "object") {
    throw new Error("materialize result is not an object");
  }
  const r = raw as Record<string, unknown>;
  for (const key of [
    "ideArtifacts",
    "publicApiTwins",
    "mapAbsent",
    "compileErrors",
    "supportRewrites",
    "vueVersionWarnings",
  ]) {
    if (!isArray(r[key])) {
      throw new Error(`materialize result field "${key}" must be an array`);
    }
  }
  if (typeof r.synthesizedTsconfig !== "boolean") {
    throw new Error('materialize result field "synthesizedTsconfig" must be a boolean');
  }
  if (r.sourceMapIdentities === null || typeof r.sourceMapIdentities !== "object") {
    throw new Error('materialize result field "sourceMapIdentities" must be an object');
  }
  return r as unknown as MaterializeResult;
}

/** Options for {@link runMaterialize}. */
export interface RunMaterializeOptions {
  /** Args placed BEFORE the `materialize` subcommand (e.g. a fake-binary script path). */
  extraArgs?: string[];
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  timeoutMs?: number;
}

/**
 * Spawn `bin [...extraArgs] materialize`, pipe `request` over stdin, and parse
 * the report DTO. Rejects with the child's stderr when it exits non-zero.
 */
export async function runMaterialize(
  bin: string,
  request: MaterializeWireRequest,
  opts: RunMaterializeOptions = {},
): Promise<MaterializeResult> {
  const result = await runOneShot(bin, {
    args: [...(opts.extraArgs ?? []), "materialize"],
    input: JSON.stringify(request),
    cwd: opts.cwd,
    env: opts.env,
    timeoutMs: opts.timeoutMs ?? 120_000,
  });
  if (result.code !== 0) {
    throw new Error(
      `verter-dx-baseline materialize exited with code ${result.code}: ${result.stderr.trim()}`,
    );
  }
  return parseMaterializeResult(result.stdout);
}
