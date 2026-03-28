/**
 * Runtime module — pooled engine registry for component-meta.
 *
 * @internal This module is consumed by the public API (project.ts)
 * and compat layer, not by end users directly.
 */

export {
  computeEngineKey,
  normalizePath,
  stableHash,
  stableSelectiveConfigHash,
} from "./engine-key.js";
export type { EngineKeyInput } from "./engine-key.js";

export { ProjectEngine, generateLeaseId } from "./project-engine.js";
export type {
  NativeMetaProject,
  NativeMetaSession,
  LeaseId,
  EngineState,
} from "./project-engine.js";

export { ProjectSession } from "./project-session.js";

export { createMetaRuntime, getMetaRuntime, shutdownMetaRuntime } from "./meta-runtime.js";
export type { MetaRuntimeImpl, BootstrapFn, EngineBootstrapResult } from "./meta-runtime.js";

export { parseTsconfig, extractPathAliases, discoverVueFiles } from "./discovery.js";

export { IDLE_TTL_MS, SWEEP_INTERVAL_MS, POOL_CAP } from "./constants.js";
