/**
 * The local-analysis-input config loader (TS side).
 *
 * Mirrors the Rust `verter_analysis_inputs` crate's `verter.analysis-projects.v1`
 * schema one-to-one — the JSON authority is shared, so the two sides stay in
 * lockstep. Opaque project ids (`^p[0-9]{4}$`) are ENFORCED at load, never trusted.
 *
 * The `DX_HARNESS_EXTERNAL_CORPUS` env var is the inert-by-default hook: when set
 * (to the config path), {@link resolveCorpusSource} returns the loaded external
 * projects so a runner can drive the campaign over real projects; when UNSET,
 * behavior is byte-identical to today (the committed hermetic fixtures), and no
 * config is read. This is the single source-swap point — there is no second loader.
 */

import { readFileSync } from "node:fs";

/** The env var an opt-in runner sets (to the config path) to load a local corpus. */
export const ANALYSIS_CORPUS_ENV = "DX_HARNESS_EXTERNAL_CORPUS";

/** The schema discriminant every valid config carries (shared with the Rust side). */
export const ANALYSIS_PROJECTS_SCHEMA = "verter.analysis-projects.v1";

/** The opaque-id shape, enforced not trusted. */
const OPAQUE_ID = /^p[0-9]{4}$/;

/** A project's shape. */
export type ProjectKind = "vite" | "nuxt" | "lib";
/** A campaign workstream. */
export type Workstream = "ide" | "tsc" | "build";

/** One analysis-input project (real paths included — kept off any emitted artifact). */
export interface AnalysisProject {
  /** The opaque id — the ONLY identity safe to emit. */
  readonly id: string;
  /** The project root (a private path; redact via the redactor before emitting). */
  readonly root: string;
  readonly tsconfig: string | null;
  readonly kind: ProjectKind;
  readonly ambientDts: readonly string[];
  readonly vueTscBin: string | null;
  readonly workstreams: readonly Workstream[];
}

/** The whole config. */
export interface AnalysisProjects {
  readonly schema: string;
  readonly checkerBin: string | null;
  readonly projects: readonly AnalysisProject[];
}

/** A load/parse/validation failure. Its message NEVER embeds a config path. */
export class AnalysisConfigError extends Error {
  constructor(message: string) {
    super(`analysis-config error: ${message}`);
    this.name = "AnalysisConfigError";
  }
}

function asString(v: unknown): string | null {
  return typeof v === "string" ? v : null;
}

function validateProject(raw: unknown, index: number): AnalysisProject {
  if (typeof raw !== "object" || raw === null) {
    throw new AnalysisConfigError(`projects[${index}] is not an object`);
  }
  const o = raw as Record<string, unknown>;
  const id = asString(o.id);
  // Opaque id ENFORCED, not trusted: reject a descriptive id outright.
  if (id === null || !OPAQUE_ID.test(id)) {
    throw new AnalysisConfigError(
      `projects[${index}].id must match ^p[0-9]{4}$ (got ${JSON.stringify(o.id)})`,
    );
  }
  const root = asString(o.root);
  if (root === null) throw new AnalysisConfigError(`projects[${index}].root is required`);
  const kind = asString(o.kind);
  if (kind !== "vite" && kind !== "nuxt" && kind !== "lib") {
    throw new AnalysisConfigError(`projects[${index}].kind must be vite|nuxt|lib`);
  }
  const workstreamsRaw = Array.isArray(o.workstreams) ? o.workstreams : [];
  const workstreams = workstreamsRaw.map((w) => {
    if (w !== "ide" && w !== "tsc" && w !== "build") {
      throw new AnalysisConfigError(`projects[${index}].workstreams has an unknown value`);
    }
    return w as Workstream;
  });
  const ambientDts = Array.isArray(o.ambientDts)
    ? o.ambientDts.map((p, i) => {
        const s = asString(p);
        if (s === null)
          throw new AnalysisConfigError(`projects[${index}].ambientDts[${i}] not a string`);
        return s;
      })
    : [];
  return {
    id,
    root,
    tsconfig: asString(o.tsconfig),
    kind,
    ambientDts,
    vueTscBin: asString(o.vueTscBin),
    workstreams,
  };
}

/**
 * Parse + validate a config from an explicit JSON string. Always available (no
 * I/O). Enforces the schema discriminant and every opaque id.
 */
export function parseAnalysisConfig(json: string): AnalysisProjects {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    // Never include the raw source or a path in the error.
    throw new AnalysisConfigError("config is not valid JSON");
  }
  if (typeof raw !== "object" || raw === null) {
    throw new AnalysisConfigError("config is not an object");
  }
  const o = raw as Record<string, unknown>;
  if (o.schema !== ANALYSIS_PROJECTS_SCHEMA) {
    throw new AnalysisConfigError(`schema must be ${ANALYSIS_PROJECTS_SCHEMA}`);
  }
  const projectsRaw = Array.isArray(o.projects) ? o.projects : [];
  const projects = projectsRaw.map((p, i) => validateProject(p, i));
  return { schema: o.schema, checkerBin: asString(o.checkerBin), projects };
}

/**
 * Load + validate a config from a file path. The path is the operator's, supplied
 * via the env var; a read failure throws a PATH-FREE {@link AnalysisConfigError}.
 */
export function loadAnalysisConfig(path: string): AnalysisProjects {
  let bytes: string;
  try {
    bytes = readFileSync(path, "utf-8");
  } catch {
    throw new AnalysisConfigError("could not read the configured analysis config");
  }
  return parseAnalysisConfig(bytes);
}

/**
 * The corpus source for a run: either the default committed fixtures (env UNSET —
 * byte-identical to today) or the external projects loaded from the config the
 * `DX_HARNESS_EXTERNAL_CORPUS` env var points at. This is the SINGLE source-swap
 * point the scenario/TSC loaders consult — there is no second loader path.
 */
export type CorpusSource =
  | { readonly kind: "default" }
  | { readonly kind: "external"; readonly config: AnalysisProjects };

/**
 * Resolve the corpus source from the environment. With `DX_HARNESS_EXTERNAL_CORPUS`
 * unset (or empty), returns `{ kind: "default" }` and reads NOTHING — the default
 * committed-fixtures behavior is unchanged. With it set, loads + validates the
 * config and returns the external projects.
 *
 * @param env the environment to read (defaults to `process.env`); injectable for
 *   tests so the real process env is never mutated.
 */
export function resolveCorpusSource(env: NodeJS.ProcessEnv = process.env): CorpusSource {
  const configured = env[ANALYSIS_CORPUS_ENV];
  if (configured === undefined || configured === "") {
    return { kind: "default" };
  }
  return { kind: "external", config: loadAnalysisConfig(configured) };
}
